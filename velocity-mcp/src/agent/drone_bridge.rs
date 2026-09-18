//! Drone bridge — IDE-side client for deploying and communicating with remote drones.
//!
//! Provides:
//! - SSH-based deployment of the drone binary to remote machines
//! - HTTP client for all drone API endpoints
//! - File upload with chunked transfer and SHA-256 verification
//! - Pairing integration with the IDE's peer system
//!
//! Every request body here follows `drone/DRONE_PROTOCOL.md`, which the drone
//! server implements. Field names are load-bearing: the drone reads a missing
//! key as its default rather than rejecting the call, so a misnamed field turns
//! into a command that silently never ran, not an error.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

/// Default drone API port.
pub const DEFAULT_DRONE_PORT: u16 = 9191;
/// Default SSH port.
pub const DEFAULT_SSH_PORT: u16 = 22;
/// Default drone API timeout in seconds.
pub const DRONE_TIMEOUT_SECS: u64 = 30;

/// Monotonic counter behind locally generated task / transfer / message ids.
static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Build an id unique to this process in the shape the protocol docs use
/// (`task_001`, `xfer_001`, `msg_001`).
///
/// Omitting the id is not harmless: the drone keys its task map by `task_id`
/// and files anything absent under the `"unknown"` fallback, so every
/// submission overwrites the previous one's status.
fn next_id(prefix: &str) -> String {
    let n = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}_{}_{n:06}", std::process::id())
}

/// Build the `POST /peer/task` body.
///
/// Split out from the HTTP call so the keys the drone reads are pinned by
/// tests. `instructions` is the field the drone hands to the shell; `prompt` is
/// a label. A body that carries the command under any other key runs an empty
/// string and still reports the task completed.
fn task_body(task_id: &str, command: &str) -> Value {
    json!({
        "task_id": task_id,
        "prompt": command,
        "instructions": command,
        "attached_files": [],
    })
}

/// Build the `POST /peer/file/start` body.
fn file_start_body(
    transfer_id: &str,
    filename: &str,
    total_size: usize,
    sha256: &str,
    total_chunks: u32,
    instructions: Option<&str>,
) -> Value {
    json!({
        "transfer_id": transfer_id,
        "filename": filename,
        "total_size": total_size,
        "sha256": sha256,
        // Omitting this makes the drone default to 1 chunk, accept only index 0
        // and then assemble the single chunk it kept - the last one it received.
        "total_chunks": total_chunks,
        "instructions": instructions,
    })
}

/// Build a `POST /peer/file/chunk` body.
fn file_chunk_body(transfer_id: &str, index: u32, data_b64: &str) -> Value {
    json!({
        "transfer_id": transfer_id,
        "index": index,
        "data": data_b64,
    })
}

/// Build a `POST /peer/message` envelope. `payload` holds the body; the drone
/// stores the whole message and returns `{received, message_id}`.
fn message_envelope(id: &str, kind: &str, payload: Value, text: String) -> Value {
    json!({
        "id": id,
        "from": format!("ide_{}", std::process::id()),
        "kind": kind,
        "payload": payload,
        "text": text,
    })
}

/// Translate the tool's `deploy_instructions` argument into the line format the
/// drone parses. Accepts a string used verbatim, or the `{action, target}`
/// objects the schema advertises.
fn deploy_instruction_lines(value: Option<&Value>) -> Result<Option<String>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(Value::Array(items)) => {
            let mut lines = Vec::with_capacity(items.len());
            for item in items {
                let action = item
                    .get("action")
                    .and_then(Value::as_str)
                    .filter(|a| matches!(*a, "run" | "copy" | "notify"))
                    .ok_or_else(|| {
                        format!(
                            "each deploy instruction needs an action of run, copy or notify; got {item}"
                        )
                    })?;
                let target = item.get("target").and_then(Value::as_str).unwrap_or("");
                lines.push(if target.is_empty() {
                    action.to_string()
                } else {
                    format!("{action} {target}")
                });
            }
            Ok(Some(lines.join("\n")))
        }
        Some(other) => Err(format!(
            "deploy_instructions must be a string or an array of {{action, target}} objects; got {other}"
        )),
    }
}

// ── Drone API Response Types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DroneHealth {
    pub status: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub uptime_secs: u64,
    #[serde(default)]
    pub environment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSubmission {
    pub task_id: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStatus {
    pub task_id: String,
    pub status: String,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub stdout: Option<String>,
    #[serde(default)]
    pub stderr: Option<String>,
    /// Nested result object from the drone (exit_code, stdout, stderr may be here).
    #[serde(default)]
    pub result: Option<TaskResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub stdout: Option<String>,
    #[serde(default)]
    pub stderr: Option<String>,
}

impl TaskStatus {
    /// Get the effective exit code, checking both top-level and nested result.
    pub fn effective_exit_code(&self) -> Option<i32> {
        self.exit_code
            .or_else(|| self.result.as_ref().and_then(|r| r.exit_code))
    }

    /// Get the effective stdout, checking both top-level and nested result.
    pub fn effective_stdout(&self) -> Option<&str> {
        self.stdout
            .as_deref()
            .or_else(|| self.result.as_ref().and_then(|r| r.stdout.as_deref()))
    }

    /// Get the effective stderr, checking both top-level and nested result.
    pub fn effective_stderr(&self) -> Option<&str> {
        self.stderr
            .as_deref()
            .or_else(|| self.result.as_ref().and_then(|r| r.stderr.as_deref()))
    }
}

/// The drone's answer to `POST /peer/file/start`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTransferAccepted {
    #[serde(default = "return_true")]
    pub accepted: bool,
    #[serde(default)]
    pub transfer_id: String,
    #[serde(default)]
    pub save_path: String,
}

fn return_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingResponse {
    #[serde(default)]
    pub accepted: bool,
    #[serde(default)]
    pub status: String,
    pub drone_id: String,
    pub drone_name: String,
}

// ── Drone Client ──

/// HTTP client for communicating with a remote drone.
pub struct DroneClient {
    base_url: String,
    auth_token: Option<String>,
    timeout_secs: u64,
}

impl DroneClient {
    pub fn new(base_url: &str, auth_token: Option<&str>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            auth_token: auth_token.map(|s| s.to_string()),
            timeout_secs: DRONE_TIMEOUT_SECS,
        }
    }

    fn auth_header(&self) -> Option<String> {
        self.auth_token.as_ref().map(|t| format!("Bearer {}", t))
    }

    /// GET /peer/health
    pub fn health(&self) -> Result<DroneHealth, Box<dyn Error>> {
        let url = format!("{}/peer/health", self.base_url);
        let resp = ureq::get(&url)
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .call()
            .map_err(|e| format!("Drone health check failed: {}", e))?;
        let health: DroneHealth = resp
            .into_json()
            .map_err(|e| format!("Failed to parse health response: {}", e))?;
        Ok(health)
    }

    /// POST /peer/task — submit a command for execution.
    ///
    /// `instructions` is the field the drone actually runs through the shell;
    /// `prompt` is only the human-readable label. Sending the command under any
    /// other key makes the drone execute an empty string and still report the
    /// task `completed` with exit code 0.
    pub fn submit_task(&self, command: &str) -> Result<TaskSubmission, Box<dyn Error>> {
        let url = format!("{}/peer/task", self.base_url);
        let task_id = next_id("task");
        let body = task_body(&task_id, command);
        let mut req = ureq::post(&url).timeout(std::time::Duration::from_secs(self.timeout_secs));
        if let Some(auth) = self.auth_header() {
            req = req.set("Authorization", &auth);
        }
        let resp = req
            .send_json(body)
            .map_err(|e| format!("Task submission failed: {}", e))?;
        let val: Value = resp
            .into_json()
            .map_err(|e| format!("Failed to parse task response: {}", e))?;
        // At its concurrency ceiling the drone answers `accepted: false` with a
        // plain 200, so the status code cannot be the only check.
        if val.get("accepted").and_then(Value::as_bool) == Some(false) {
            return Err(format!(
                "Drone rejected the task: {}",
                val.get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("no reason given")
            )
            .into());
        }
        Ok(TaskSubmission {
            task_id: val
                .get("task_id")
                .and_then(Value::as_str)
                .unwrap_or(task_id.as_str())
                .to_string(),
            status: val
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("pending")
                .to_string(),
        })
    }

    /// GET /peer/task/{id}/status — poll task progress.
    pub fn task_status(&self, task_id: &str) -> Result<TaskStatus, Box<dyn Error>> {
        let url = format!("{}/peer/task/{}/status", self.base_url, task_id);
        let mut req = ureq::get(&url).timeout(std::time::Duration::from_secs(self.timeout_secs));
        if let Some(auth) = self.auth_header() {
            req = req.set("Authorization", &auth);
        }
        let resp = req
            .call()
            .map_err(|e| format!("Task status check failed: {}", e))?;
        let status: TaskStatus = resp
            .into_json()
            .map_err(|e| format!("Failed to parse status response: {}", e))?;
        Ok(status)
    }

    /// POST /peer/pair — initiate pairing.
    pub fn pair(&self, peer_name: &str) -> Result<PairingResponse, Box<dyn Error>> {
        let url = format!("{}/peer/pair", self.base_url);
        let body = json!({
            "peer_id": format!("ide_{}", std::process::id()),
            "name": peer_name,
        });
        let mut req = ureq::post(&url).timeout(std::time::Duration::from_secs(self.timeout_secs));
        if let Some(auth) = self.auth_header() {
            req = req.set("Authorization", &auth);
        }
        let resp = req
            .send_json(body)
            .map_err(|e| format!("Pairing failed: {}", e))?;
        let pairing: PairingResponse = resp
            .into_json()
            .map_err(|e| format!("Failed to parse pairing response: {}", e))?;
        Ok(pairing)
    }

    /// POST /peer/message — deliver a chat message to the drone.
    ///
    /// The protocol puts the body inside `payload` and identifies the message
    /// with a top-level `id`; the receipt echoes that id back.
    pub fn send_message(&self, text: &str) -> Result<Value, Box<dyn Error>> {
        let url = format!("{}/peer/message", self.base_url);
        let body = message_envelope(
            &next_id("msg"),
            "Chat",
            json!({ "text": text }),
            text.to_string(),
        );
        let mut req = ureq::post(&url).timeout(std::time::Duration::from_secs(self.timeout_secs));
        if let Some(auth) = self.auth_header() {
            req = req.set("Authorization", &auth);
        }
        let resp = req
            .send_json(body)
            .map_err(|e| format!("Message send failed: {}", e))?;
        let val: Value = resp
            .into_json()
            .map_err(|e| format!("Failed to parse message response: {}", e))?;
        Ok(val)
    }

    /// Upload a file in chunks with SHA-256 verification.
    ///
    /// `total_chunks` must be sent: the drone defaults it to 1, which makes it
    /// accept only chunk index 0 and then assemble *just the last chunk it was
    /// handed*, so a multi-chunk file lands truncated with no failure raised.
    /// Deploy instructions belong on `start`, where the server reads them, not
    /// on `complete`. The drone saves into its own drop inbox and keeps the
    /// base file name, so there is no destination path in this protocol.
    pub fn upload_file(
        &self,
        local_path: &Path,
        remote_name: Option<&str>,
        instructions: Option<&str>,
    ) -> Result<Value, Box<dyn Error>> {
        let data =
            std::fs::read(local_path).map_err(|e| format!("Failed to read local file: {}", e))?;

        let mut hasher = Sha256::new();
        hasher.update(&data);
        let sha256_hex: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();
        let file_name = remote_name
            .map(|n| n.to_string())
            .or_else(|| {
                local_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
            })
            .unwrap_or_else(|| "upload".to_string());
        let file_size = data.len();
        let chunk_size = 256 * 1024;
        let total_chunks = data.len().div_ceil(chunk_size) as u32;
        let transfer_id = next_id("xfer");

        let start_url = format!("{}/peer/file/start", self.base_url);
        let start_body = file_start_body(
            &transfer_id,
            &file_name,
            file_size,
            &sha256_hex,
            total_chunks,
            instructions,
        );
        let mut req =
            ureq::post(&start_url).timeout(std::time::Duration::from_secs(self.timeout_secs));
        if let Some(auth) = self.auth_header() {
            req = req.set("Authorization", &auth);
        }
        let accepted: FileTransferAccepted = req
            .send_json(start_body)
            .map_err(|e| format!("File upload start failed: {}", e))?
            .into_json()
            .map_err(|e| format!("Failed to parse upload start response: {}", e))?;
        if !accepted.accepted {
            return Err(format!("Drone refused the transfer (id {transfer_id})").into());
        }

        // Step 2: Send chunks (256KB each)
        for (i, chunk) in data.chunks(chunk_size).enumerate() {
            let chunk_url = format!("{}/peer/file/chunk", self.base_url);
            let chunk_body = file_chunk_body(&transfer_id, i as u32, &B64.encode(chunk));
            let mut req = ureq::post(&chunk_url)
                .timeout(std::time::Duration::from_secs(self.timeout_secs * 2));
            if let Some(auth) = self.auth_header() {
                req = req.set("Authorization", &auth);
            }
            let receipt: Value = req
                .send_json(chunk_body)
                .map_err(|e| format!("Chunk {} upload failed: {}", i, e))?
                .into_json()
                .map_err(|e| format!("Failed to parse chunk {} response: {}", i, e))?;
            // The drone answers `received: false` when a chunk is out of range
            // for the declared total; ignoring it loses bytes silently.
            if receipt.get("received").and_then(Value::as_bool) == Some(false) {
                return Err(format!(
                    "Drone rejected chunk {i} of {total_chunks} for {file_name} (index out of range)"
                )
                .into());
            }
        }

        // Step 3: Complete transfer
        let complete_url = format!("{}/peer/file/complete", self.base_url);
        let complete_body = json!({ "transfer_id": transfer_id.as_str() });
        let mut req =
            ureq::post(&complete_url).timeout(std::time::Duration::from_secs(self.timeout_secs));
        if let Some(auth) = self.auth_header() {
            req = req.set("Authorization", &auth);
        }
        let resp = req
            .send_json(complete_body)
            .map_err(|e| format!("File upload complete failed: {}", e))?;
        let val: Value = resp
            .into_json()
            .map_err(|e| format!("Failed to parse complete response: {}", e))?;
        // Carry the transfer identity out so callers can tie the result to what
        // they asked for even when the drone words its reply differently.
        let mut val = val;
        if let Some(obj) = val.as_object_mut() {
            obj.entry("transfer_id")
                .or_insert_with(|| json!(transfer_id));
            obj.entry("sha256").or_insert_with(|| json!(sha256_hex));
        }
        Ok(val)
    }

    /// Queue a system-level request on the drone's message endpoint.
    ///
    /// This is *not* a request/response call. `handle_message` on the drone
    /// appends the message to a bounded queue and returns a receipt; it never
    /// dispatches on `kind`, so no capture, input or monitoring work happens.
    /// The screen-capture, `SendInput` and network-monitor implementations in
    /// `drone/src/system.rs` exist but are unreachable over HTTP. Callers must
    /// surface the receipt as "queued", never as a result.
    pub fn send_system_command(
        &self,
        command_type: &str,
        payload: &Value,
    ) -> Result<Value, Box<dyn Error>> {
        let url = format!("{}/peer/message", self.base_url);
        let body = message_envelope(
            &next_id("msg"),
            "TaskRequest",
            payload.clone(),
            format!("{}:{}", command_type, payload),
        );
        let mut req = ureq::post(&url).timeout(std::time::Duration::from_secs(self.timeout_secs));
        if let Some(auth) = self.auth_header() {
            req = req.set("Authorization", &auth);
        }
        let resp = req
            .send_json(body)
            .map_err(|e| format!("System command failed: {}", e))?;
        let val: Value = resp
            .into_json()
            .map_err(|e| format!("Failed to parse response: {}", e))?;
        Ok(val)
    }
}

// ── SSH Deployment ──

/// Deploy the drone binary to a remote machine via SSH/SCP.
pub struct DroneDeployer {
    ssh_user: String,
    ssh_key_path: Option<PathBuf>,
    ssh_port: u16,
}

impl DroneDeployer {
    pub fn new(ssh_user: &str, ssh_key_path: Option<&Path>, ssh_port: u16) -> Self {
        Self {
            ssh_user: ssh_user.to_string(),
            ssh_key_path: ssh_key_path.map(|p| p.to_path_buf()),
            ssh_port,
        }
    }

    fn ssh_common_args(&self) -> Vec<String> {
        let mut args = vec![
            "-o".into(),
            "StrictHostKeyChecking=accept-new".into(),
            "-o".into(),
            "ConnectTimeout=10".into(),
            "-p".into(),
            self.ssh_port.to_string(),
        ];
        if let Some(ref key) = self.ssh_key_path {
            args.push("-i".into());
            args.push(key.display().to_string());
        }
        args
    }

    fn remote_target(&self, host: &str) -> String {
        format!("{}@{}", self.ssh_user, host)
    }

    /// Find the drone binary in the workspace target directory.
    pub fn find_drone_binary() -> Result<PathBuf, Box<dyn Error>> {
        // Check common locations
        let candidates = [
            PathBuf::from("target/release/velocity-drone"),
            PathBuf::from("target/debug/velocity-drone"),
            PathBuf::from("target/release/velocity-drone.exe"),
            PathBuf::from("target/debug/velocity-drone.exe"),
        ];

        for candidate in &candidates {
            if candidate.exists() {
                return Ok(candidate.clone());
            }
        }

        // Try workspace root
        if let Ok(cwd) = std::env::current_dir() {
            for candidate in &candidates {
                let full = cwd.join(candidate);
                if full.exists() {
                    return Ok(full);
                }
            }
        }

        Err("Drone binary not found. Build with: cargo build --release -p velocity-drone".into())
    }

    /// Deploy the drone binary to a remote machine and start it.
    pub fn deploy(
        &self,
        host: &str,
        drone_port: u16,
        drone_name: &str,
        auth_token: &str,
    ) -> Result<DroneHealth, Box<dyn Error>> {
        let binary_path = Self::find_drone_binary()?;
        let remote_target = self.remote_target(host);
        let ssh_args = self.ssh_common_args();

        // Step 1: Create remote directory
        let mut cmd = Command::new("ssh");
        for arg in &ssh_args {
            cmd.arg(arg);
        }
        cmd.arg(&remote_target).arg("mkdir -p ~/.velocity-drone");
        let output = cmd
            .output()
            .map_err(|e| format!("SSH mkdir failed: {}", e))?;
        if !output.status.success() {
            return Err(format!(
                "SSH mkdir failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }

        // Step 2: SCP the binary
        let mut scp_cmd = Command::new("scp");
        for arg in &ssh_args {
            scp_cmd.arg(arg);
        }
        scp_cmd.arg(&binary_path).arg(format!(
            "{}:~/.velocity-drone/velocity-drone",
            remote_target
        ));
        let output = scp_cmd.output().map_err(|e| format!("SCP failed: {}", e))?;
        if !output.status.success() {
            return Err(format!("SCP failed: {}", String::from_utf8_lossy(&output.stderr)).into());
        }

        // Step 3: Make executable and start
        // Write the auth token to a restricted env file so it doesn't appear in the process list.
        let start_cmd = format!(
            "chmod +x ~/.velocity-drone/velocity-drone && \
             printf 'DRONE_AUTH_TOKEN={}' > ~/.velocity-drone/.env && \
             chmod 600 ~/.velocity-drone/.env && \
             set -a && . ~/.velocity-drone/.env && set +a && \
             nohup ~/.velocity-drone/velocity-drone \
             --port {} --name \"{}\" > ~/.velocity-drone/drone.log 2>&1 &",
            auth_token, drone_port, drone_name
        );
        let mut cmd = Command::new("ssh");
        for arg in &ssh_args {
            cmd.arg(arg);
        }
        cmd.arg(&remote_target).arg(&start_cmd);
        let output = cmd
            .output()
            .map_err(|e| format!("SSH start failed: {}", e))?;
        if !output.status.success() {
            return Err(format!(
                "SSH start failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }

        // Step 4: Wait for drone to come up and verify health
        let drone_url = format!("http://{}:{}", host, drone_port);
        let client = DroneClient::new(&drone_url, Some(auth_token));

        // Retry health check a few times
        for attempt in 0..10 {
            std::thread::sleep(std::time::Duration::from_secs(1));
            match client.health() {
                Ok(health) => return Ok(health),
                Err(_) if attempt < 9 => continue,
                Err(e) => {
                    return Err(format!("Drone deployed but health check failed: {}", e).into())
                }
            }
        }

        Err("Drone deployed but did not respond to health checks within 10 seconds".into())
    }
}

// ── Tool Execution Handler ──

/// Handle a drone tool call from the MCP registry.
/// Returns `Ok(Some(result))` if the tool was handled, `Ok(None)` if the tool name is not a drone tool.
pub fn handle_drone_tool(
    root: &Path,
    name: &str,
    arguments: &Value,
) -> Result<Option<String>, Box<dyn Error>> {
    match name {
        "drone_deploy" => handle_deploy(root, arguments).map(Some),
        "drone_command" => handle_command(arguments).map(Some),
        "drone_task_status" => handle_task_status(arguments).map(Some),
        "drone_status" => handle_status(arguments).map(Some),
        "drone_screenshot" => handle_screenshot(arguments).map(Some),
        "drone_type_keys" => handle_type_keys(arguments).map(Some),
        "drone_click" => handle_click(arguments).map(Some),
        "drone_network_stats" => handle_network_stats(arguments).map(Some),
        "drone_upload" => handle_upload(root, arguments).map(Some),
        "drone_pair" => handle_pair(arguments).map(Some),
        _ => Ok(None),
    }
}

fn handle_deploy(_root: &Path, args: &Value) -> Result<String, Box<dyn Error>> {
    let host = args["host"].as_str().ok_or("host is required")?;
    let ssh_port = args["port"].as_u64().unwrap_or(DEFAULT_SSH_PORT as u64) as u16;
    let drone_port = args["drone_port"]
        .as_u64()
        .unwrap_or(DEFAULT_DRONE_PORT as u64) as u16;
    let drone_name = args["drone_name"]
        .as_str()
        .unwrap_or(&format!("drone-{}", host.replace('.', "-")))
        .to_string();
    let auth_token = args["auth_token"]
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            use sha2::{Digest, Sha256};
            let hash = Sha256::new()
                .chain_update(format!("{}-{}-{}", host, drone_port, std::process::id()))
                .finalize();
            hash.iter()
                .map(|b| format!("{:02x}", b))
                .collect::<String>()[..32]
                .to_string()
        });
    let ssh_user = args["ssh_user"].as_str().unwrap_or("root");
    let ssh_key_path = args["ssh_key_path"].as_str().map(PathBuf::from);

    let deployer = DroneDeployer::new(ssh_user, ssh_key_path.as_deref(), ssh_port);
    let health = deployer.deploy(host, drone_port, &drone_name, &auth_token)?;

    Ok(serde_json::to_string_pretty(&json!({
        "status": "deployed",
        "drone_url": format!("http://{}:{}", host, drone_port),
        "drone_id": health.id,
        "drone_name": health.name,
        "auth_token": auth_token,
        "capabilities": health.capabilities,
        "environment": health.environment,
    }))?)
}

fn handle_command(args: &Value) -> Result<String, Box<dyn Error>> {
    let drone_url = args["drone_url"].as_str().ok_or("drone_url is required")?;
    let command = args["command"].as_str().ok_or("command is required")?;
    let auth_token = args["auth_token"].as_str();

    let client = DroneClient::new(drone_url, auth_token);
    let task = client.submit_task(command)?;

    Ok(serde_json::to_string_pretty(&json!({
        "success": true,
        "task_id": task.task_id,
        "status": task.status,
        "command": command,
        "message": "Command submitted. Use drone_task_status to check progress.",
    }))?)
}

fn handle_task_status(args: &Value) -> Result<String, Box<dyn Error>> {
    let drone_url = args["drone_url"].as_str().ok_or("drone_url is required")?;
    let task_id = args["task_id"].as_str().ok_or("task_id is required")?;
    let auth_token = args["auth_token"].as_str();

    let client = DroneClient::new(drone_url, auth_token);
    let status = client.task_status(task_id)?;

    Ok(serde_json::to_string_pretty(&status)?)
}

fn handle_status(args: &Value) -> Result<String, Box<dyn Error>> {
    let drone_url = args["drone_url"].as_str().ok_or("drone_url is required")?;
    let auth_token = args["auth_token"].as_str();

    let client = DroneClient::new(drone_url, auth_token);
    let health = client.health()?;

    Ok(serde_json::to_string_pretty(&health)?)
}

/// Shape the in-band answer for a request the drone only queued.
///
/// A receipt is not a result. Reporting `success: true` here is what made
/// `drone_screenshot` claim `"status": "captured"` while returning no image and
/// `drone_network_stats` hand back `{"received": true}` where the tool
/// advertises byte counters.
fn queued_without_result(action: &str, wanted: &str, receipt: &Value) -> Value {
    json!({
        "success": false,
        "status": "queued",
        "action": action,
        "receipt": receipt,
        "detail": format!(
            "The drone queued the {action} request and returned a receipt; no {wanted} was produced. \
             POST /peer/message is one-way and the drone does not dispatch on message kind, so screen \
             capture, input synthesis and network monitoring are not reachable over HTTP even though \
             drone/src/system.rs implements them.",
        ),
    })
}

fn handle_screenshot(args: &Value) -> Result<String, Box<dyn Error>> {
    let drone_url = args["drone_url"].as_str().ok_or("drone_url is required")?;
    let auth_token = args["auth_token"].as_str();

    let client = DroneClient::new(drone_url, auth_token);
    let result = client.send_system_command("screenshot", &json!({}))?;

    Ok(serde_json::to_string_pretty(&queued_without_result(
        "screenshot",
        "image data",
        &result,
    ))?)
}

fn handle_type_keys(args: &Value) -> Result<String, Box<dyn Error>> {
    let drone_url = args["drone_url"].as_str().ok_or("drone_url is required")?;
    let action = args["action"].as_str().ok_or("action is required")?;
    let auth_token = args["auth_token"].as_str();

    let client = DroneClient::new(drone_url, auth_token);
    let payload = match action {
        "type" => {
            let text = args["text"]
                .as_str()
                .ok_or("text is required for type action")?;
            json!({ "action": "type", "text": text })
        }
        "press_key" => {
            let key = args["key"]
                .as_str()
                .ok_or("key is required for press_key action")?;
            json!({ "action": "press_key", "key": key })
        }
        _ => return Err(format!("Unknown action: {}", action).into()),
    };

    let result = client.send_system_command("input", &payload)?;
    Ok(serde_json::to_string_pretty(&queued_without_result(
        "keyboard input",
        "keystroke delivered",
        &result,
    ))?)
}

fn handle_click(args: &Value) -> Result<String, Box<dyn Error>> {
    let drone_url = args["drone_url"].as_str().ok_or("drone_url is required")?;
    let x = args["x"].as_i64().ok_or("x is required")? as i32;
    let y = args["y"].as_i64().ok_or("y is required")? as i32;
    let button = args["button"].as_str().unwrap_or("left");
    let auth_token = args["auth_token"].as_str();

    let client = DroneClient::new(drone_url, auth_token);
    let payload = json!({ "action": "click", "x": x, "y": y, "button": button });
    let result = client.send_system_command("input", &payload)?;

    Ok(serde_json::to_string_pretty(&queued_without_result(
        "mouse click",
        "click delivered",
        &result,
    ))?)
}

fn handle_network_stats(args: &Value) -> Result<String, Box<dyn Error>> {
    let drone_url = args["drone_url"].as_str().ok_or("drone_url is required")?;
    let auth_token = args["auth_token"].as_str();

    let client = DroneClient::new(drone_url, auth_token);
    let result = client.send_system_command("network_stats", &json!({}))?;

    Ok(serde_json::to_string_pretty(&queued_without_result(
        "network stats",
        "counter",
        &result,
    ))?)
}

fn handle_upload(root: &Path, args: &Value) -> Result<String, Box<dyn Error>> {
    let drone_url = args["drone_url"].as_str().ok_or("drone_url is required")?;
    let local_path = args["local_path"]
        .as_str()
        .ok_or("local_path is required")?;
    let remote_path = args["remote_path"]
        .as_str()
        .ok_or("remote_path is required")?;
    let auth_token = args["auth_token"].as_str();
    // The tool schema offers deploy instructions as `{action, target}` objects;
    // the drone's `instructions` field is a string of `run`/`copy`/`notify`
    // lines. Passing the array straight through is what made the server's
    // `as_str()` read it as absent and skip the deploy entirely.
    let deploy_instructions =
        deploy_instruction_lines(args.get("deploy_instructions")).map_err(|e| e.to_string())?;

    let full_path = if Path::new(local_path).is_absolute() {
        PathBuf::from(local_path)
    } else {
        root.join(local_path)
    };

    let client = DroneClient::new(drone_url, auth_token);
    // The protocol carries no destination, only a file name, so honour the part
    // of `remote_path` a drone can actually act on.
    let wanted_name = Path::new(remote_path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty());
    let source_name = full_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "upload".to_string());
    let sent_as = wanted_name.unwrap_or_else(|| source_name.clone());
    let result = client.upload_file(
        &full_path,
        Some(sent_as.as_str()),
        deploy_instructions.as_deref(),
    )?;

    let verified = result.get("verified").and_then(Value::as_bool);
    let dest = result
        .pointer("/deploy_result/dest_path")
        .or_else(|| result.get("save_path"))
        .and_then(Value::as_str);
    // A failed checksum means the drone kept something other than the bytes we
    // sent; calling that "uploaded" hands the caller a corrupt file to deploy.
    if verified == Some(false) {
        return Err(format!(
            "Upload did not verify: the drone reassembled {source_name} under a SHA-256 that does not \
             match what was sent, so the copy on the drone is unusable. Destination: {dest:?}"
        )
        .into());
    }

    Ok(serde_json::to_string_pretty(&json!({
        "success": true,
        "status": "uploaded",
        "local_path": local_path,
        "requested_remote_path": remote_path,
        "dropped_as": dest,
        "note": format!(
            "The drone stores drops in its own inbox directory and keeps only the file name, so the \
             directory part of remote_path is not honoured; {sent_as} landed where the drone chose."
        ),
        "result": result,
    }))?)
}

fn handle_pair(args: &Value) -> Result<String, Box<dyn Error>> {
    let drone_url = args["drone_url"].as_str().ok_or("drone_url is required")?;
    let drone_name = args["drone_name"].as_str().unwrap_or("Velocity Drone");
    let auth_token = args["auth_token"].as_str();

    let client = DroneClient::new(drone_url, auth_token);
    let pairing = client.pair(drone_name)?;

    Ok(serde_json::to_string_pretty(&json!({
        "status": "paired",
        "drone_id": pairing.drone_id,
        "drone_name": pairing.drone_name,
        "message": "Drone paired successfully. It will appear in the Peers panel.",
    }))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drone_client_construction() {
        let client = DroneClient::new("http://localhost:9191", Some("test-token"));
        assert_eq!(client.base_url, "http://localhost:9191");
        assert_eq!(client.auth_token, Some("test-token".to_string()));
    }

    #[test]
    fn test_drone_client_trims_trailing_slash() {
        let client = DroneClient::new("http://localhost:9191/", None);
        assert_eq!(client.base_url, "http://localhost:9191");
    }

    #[test]
    fn test_drone_deployer_construction() {
        let deployer = DroneDeployer::new("admin", None, 22);
        assert_eq!(deployer.ssh_user, "admin");
        assert_eq!(deployer.ssh_port, 22);
    }

    #[test]
    fn test_remote_target() {
        let deployer = DroneDeployer::new("admin", None, 22);
        assert_eq!(deployer.remote_target("192.168.1.1"), "admin@192.168.1.1");
    }

    #[test]
    fn test_ssh_common_args_includes_port() {
        let deployer = DroneDeployer::new("admin", None, 2222);
        let args = deployer.ssh_common_args();
        assert!(args.contains(&"2222".to_string()));
    }

    #[test]
    fn test_ssh_common_args_includes_key() {
        let deployer =
            DroneDeployer::new("admin", Some(Path::new("/home/user/.ssh/id_ed25519")), 22);
        let args = deployer.ssh_common_args();
        assert!(args.contains(&"/home/user/.ssh/id_ed25519".to_string()));
    }

    #[test]
    fn test_handle_drone_tool_unknown() {
        let result = handle_drone_tool(Path::new("/tmp"), "drone_nonexistent", &json!({}));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    /// Mirror how `drone/src/core.rs` reads a `/peer/task` body: every field is
    /// looked up by name with a default, never rejected.
    fn drone_reads_task(body: &Value) -> (String, String) {
        (
            body.get("task_id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            body.get("instructions")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        )
    }

    #[test]
    fn the_command_lands_where_the_drone_actually_reads_it() {
        // The drone runs `instructions` through the shell. The client used to
        // send `{"command": ...}`, so the drone executed an empty string and
        // reported the task completed with exit code 0 - a command that silently
        // never ran.
        let body = task_body("task_1", "cargo test --release");
        let (task_id, instructions) = drone_reads_task(&body);
        assert_eq!(instructions, "cargo test --release");
        assert_eq!(task_id, "task_1");
        assert!(
            body.get("command").is_none(),
            "a `command` key is read by nobody: {body}"
        );
    }

    #[test]
    fn each_submission_gets_its_own_task_id() {
        // Keyed by `task_id`, the drone files anything missing under "unknown",
        // so back-to-back commands overwrite one another's status.
        let ids: Vec<String> = (0..200).map(|_| next_id("task")).collect();
        let unique: std::collections::HashSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "task ids collided");
    }

    /// Mirror `handle_file_start`: it defaults `total_chunks` to 1 and every
    /// other field to an empty value.
    fn drone_reads_start(body: &Value) -> (String, String, u64) {
        (
            body["transfer_id"].as_str().unwrap_or("").to_string(),
            body["filename"].as_str().unwrap_or("").to_string(),
            body["total_chunks"].as_u64().unwrap_or(1),
        )
    }

    #[test]
    fn a_start_body_names_the_transfer_and_declares_its_chunk_count() {
        let body = file_start_body("xfer_7", "app.exe", 786_456, "abc", 3, None);
        let (transfer_id, filename, total_chunks) = drone_reads_start(&body);
        assert_eq!(transfer_id, "xfer_7", "an empty id shares one map slot");
        assert_eq!(filename, "app.exe", "an empty name drops as 'unnamed'");
        assert_eq!(
            total_chunks, 3,
            "defaulting to 1 makes the drone keep only the last chunk"
        );
    }

    #[test]
    fn deploy_instructions_reach_the_drone_as_lines_at_start() {
        // The server reads `instructions` in the *start* body; the client used
        // to attach them to the complete body under a different key, so a
        // transfer finished and nothing was deployed.
        let body = file_start_body("xfer_7", "app.exe", 10, "abc", 1, Some("notify shipped"));
        assert_eq!(body["instructions"].as_str(), Some("notify shipped"));
    }

    #[test]
    fn chunk_bodies_number_each_chunk_under_one_transfer() {
        let first = file_chunk_body("xfer_7", 0, "AAA=");
        let second = file_chunk_body("xfer_7", 1, "BBB=");
        // The client used to send `chunk_index`, which the server read as 0 for
        // every chunk - so each write replaced the previous one.
        assert_eq!(first["index"].as_u64(), Some(0));
        assert_eq!(second["index"].as_u64(), Some(1));
        assert_eq!(first["transfer_id"], second["transfer_id"]);
        assert_eq!(first["data"].as_str(), Some("AAA="));
    }

    #[test]
    fn message_envelopes_identify_themselves_and_carry_a_payload() {
        let env = message_envelope(
            "msg_3",
            "TaskRequest",
            json!({ "action": "type", "text": "hi" }),
            "input:{\"action\":\"type\"}".to_string(),
        );
        // The drone echoes `id` back as `message_id`; with no id the receipt was
        // always blank, so nothing could be correlated.
        assert_eq!(env["id"].as_str(), Some("msg_3"));
        assert_eq!(env["kind"].as_str(), Some("TaskRequest"));
        assert_eq!(env["payload"]["text"].as_str(), Some("hi"));
        assert!(env["from"].as_str().unwrap().starts_with("ide_"));
    }

    #[test]
    fn the_advertised_instruction_objects_become_drone_lines() {
        let args = json!([
            { "action": "run", "target": "{file} --test" },
            { "action": "notify", "target": "shipped" },
        ]);
        assert_eq!(
            deploy_instruction_lines(Some(&args)).unwrap().as_deref(),
            Some("run {file} --test\nnotify shipped")
        );
        assert_eq!(
            deploy_instruction_lines(Some(&json!("notify shipped")))
                .unwrap()
                .as_deref(),
            Some("notify shipped"),
            "a plain string is already in the drone's format"
        );
        assert_eq!(deploy_instruction_lines(None).unwrap(), None);
        assert_eq!(deploy_instruction_lines(Some(&json!(null))).unwrap(), None);
        assert!(
            deploy_instruction_lines(Some(&json!([{"action": "rm", "target": "-rf"}]))).is_err()
        );
        assert!(deploy_instruction_lines(Some(&json!(7))).is_err());
    }

    #[test]
    fn a_queued_request_never_reports_itself_done() {
        // `drone_screenshot` used to answer `{"status": "captured"}` from a
        // mailbox receipt, advertising base64 PNG it never produced.
        let out = queued_without_result("screenshot", "image data", &json!({"received": true}));
        assert_eq!(out["success"], false);
        assert_eq!(out["status"], "queued");
        assert!(out.get("image_data").is_none());
        assert!(out["detail"]
            .as_str()
            .unwrap()
            .contains("no image data was produced"));
    }

    /// Integration test: requires a running drone on localhost:9191 (no auth).
    /// Run with: cargo test -p velocity_mcp --lib -- drone_integration --ignored
    #[test]
    #[ignore]
    fn drone_integration_health_check() {
        let client = DroneClient::new("http://127.0.0.1:9191", None);
        let health = client.health().expect("health check should succeed");
        assert_eq!(health.status, "ok");
        assert_eq!(health.name, "TestDrone");
        assert!(health.capabilities.contains(&"screen_capture".to_string()));
    }

    #[test]
    #[ignore]
    fn drone_integration_submit_and_poll_task() {
        let client = DroneClient::new("http://127.0.0.1:9191", None);
        let task = client
            .submit_task("echo integration-test")
            .expect("task submit should succeed");
        assert!(!task.task_id.is_empty());

        // Poll until complete (max 5 seconds)
        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let status = client
                .task_status(&task.task_id)
                .expect("status poll should succeed");
            if status.status == "completed" {
                assert_eq!(status.effective_exit_code(), Some(0));
                return;
            }
        }
        panic!("Task did not complete within 5 seconds");
    }

    #[test]
    #[ignore]
    fn drone_integration_pair() {
        let client = DroneClient::new("http://127.0.0.1:9191", None);
        let pairing = client
            .pair("IntegrationTestIDE")
            .expect("pairing should succeed");
        assert!(pairing.accepted);
        assert!(!pairing.drone_id.is_empty());
    }

    #[test]
    #[ignore]
    fn drone_integration_send_message() {
        let client = DroneClient::new("http://127.0.0.1:9191", None);
        let resp = client
            .send_message("Hello from integration test")
            .expect("message should succeed");
        assert_eq!(resp["received"], true);
    }
}
