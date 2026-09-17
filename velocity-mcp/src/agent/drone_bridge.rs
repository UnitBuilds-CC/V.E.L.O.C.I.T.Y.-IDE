//! Drone bridge — IDE-side client for deploying and communicating with remote drones.
//!
//! Provides:
//! - SSH-based deployment of the drone binary to remote machines
//! - HTTP client for all drone API endpoints
//! - File upload with chunked transfer and SHA-256 verification
//! - Pairing integration with the IDE's peer system

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Default drone API port.
pub const DEFAULT_DRONE_PORT: u16 = 9191;
/// Default SSH port.
pub const DEFAULT_SSH_PORT: u16 = 22;
/// Default drone API timeout in seconds.
pub const DRONE_TIMEOUT_SECS: u64 = 30;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileUploadStart {
    pub upload_id: String,
    pub status: String,
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
    pub fn submit_task(&self, command: &str) -> Result<TaskSubmission, Box<dyn Error>> {
        let url = format!("{}/peer/task", self.base_url);
        let body = json!({ "command": command });
        let mut req = ureq::post(&url).timeout(std::time::Duration::from_secs(self.timeout_secs));
        if let Some(auth) = self.auth_header() {
            req = req.set("Authorization", &auth);
        }
        let resp = req
            .send_json(body)
            .map_err(|e| format!("Task submission failed: {}", e))?;
        let task: TaskSubmission = resp
            .into_json()
            .map_err(|e| format!("Failed to parse task response: {}", e))?;
        Ok(task)
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

    /// POST /peer/message — send a chat message to the drone.
    pub fn send_message(&self, text: &str) -> Result<Value, Box<dyn Error>> {
        let url = format!("{}/peer/message", self.base_url);
        let body = json!({
            "from": "ide",
            "kind": "Chat",
            "text": text,
        });
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
    pub fn upload_file(
        &self,
        local_path: &Path,
        remote_path: &str,
        deploy_instructions: Option<&Value>,
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
        let file_name = local_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "upload".to_string());
        let file_size = data.len();

        // Step 1: Start upload
        let start_url = format!("{}/peer/file/start", self.base_url);
        let start_body = json!({
            "file_name": file_name,
            "file_size": file_size,
            "sha256": sha256_hex,
            "destination": remote_path,
        });
        let mut req =
            ureq::post(&start_url).timeout(std::time::Duration::from_secs(self.timeout_secs));
        if let Some(auth) = self.auth_header() {
            req = req.set("Authorization", &auth);
        }
        let _start_resp: FileUploadStart = req
            .send_json(start_body)
            .map_err(|e| format!("File upload start failed: {}", e))?
            .into_json()
            .map_err(|e| format!("Failed to parse upload start response: {}", e))?;

        // Step 2: Send chunks (256KB each)
        let chunk_size = 256 * 1024;
        let total_chunks = data.len().div_ceil(chunk_size);
        for (i, chunk) in data.chunks(chunk_size).enumerate() {
            let chunk_url = format!("{}/peer/file/chunk", self.base_url);
            let chunk_body = json!({
                "file_name": file_name,
                "chunk_index": i,
                "chunk_total": total_chunks,
                "data": B64.encode(chunk),
            });
            let mut req = ureq::post(&chunk_url)
                .timeout(std::time::Duration::from_secs(self.timeout_secs * 2));
            if let Some(auth) = self.auth_header() {
                req = req.set("Authorization", &auth);
            }
            req.send_json(chunk_body)
                .map_err(|e| format!("Chunk {} upload failed: {}", i, e))?;
        }

        // Step 3: Complete upload
        let complete_url = format!("{}/peer/file/complete", self.base_url);
        let mut complete_body = json!({
            "file_name": file_name,
            "sha256": sha256_hex,
        });
        if let Some(instructions) = deploy_instructions {
            complete_body["deploy_instructions"] = instructions.clone();
        }
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
        Ok(val)
    }

    /// Send a system command via the message endpoint (for GUI automation).
    pub fn send_system_command(
        &self,
        command_type: &str,
        payload: &Value,
    ) -> Result<Value, Box<dyn Error>> {
        let url = format!("{}/peer/message", self.base_url);
        let body = json!({
            "from": "ide",
            "kind": "TaskRequest",
            "text": format!("{}:{}", command_type, payload),
        });
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
        "task_id": task.task_id,
        "status": task.status,
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

fn handle_screenshot(args: &Value) -> Result<String, Box<dyn Error>> {
    let drone_url = args["drone_url"].as_str().ok_or("drone_url is required")?;
    let auth_token = args["auth_token"].as_str();

    let client = DroneClient::new(drone_url, auth_token);
    let result = client.send_system_command("screenshot", &json!({}))?;

    Ok(serde_json::to_string_pretty(&json!({
        "status": "captured",
        "message": "Screenshot request sent to drone.",
        "response": result,
    }))?)
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
    Ok(serde_json::to_string_pretty(&json!({
        "status": "sent",
        "action": action,
        "response": result,
    }))?)
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

    Ok(serde_json::to_string_pretty(&json!({
        "status": "sent",
        "x": x,
        "y": y,
        "button": button,
        "response": result,
    }))?)
}

fn handle_network_stats(args: &Value) -> Result<String, Box<dyn Error>> {
    let drone_url = args["drone_url"].as_str().ok_or("drone_url is required")?;
    let auth_token = args["auth_token"].as_str();

    let client = DroneClient::new(drone_url, auth_token);
    let result = client.send_system_command("network_stats", &json!({}))?;

    Ok(serde_json::to_string_pretty(&result)?)
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
    let deploy_instructions = args.get("deploy_instructions");

    let full_path = if Path::new(local_path).is_absolute() {
        PathBuf::from(local_path)
    } else {
        root.join(local_path)
    };

    let client = DroneClient::new(drone_url, auth_token);
    let result = client.upload_file(&full_path, remote_path, deploy_instructions)?;

    Ok(serde_json::to_string_pretty(&json!({
        "status": "uploaded",
        "local_path": local_path,
        "remote_path": remote_path,
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
