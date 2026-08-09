//! Drone core logic: identity, file transfers, task execution, deployment.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::safety::SafeMutex;

// ── Helpers ──

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn generate_drone_id() -> String {
    let ts = now_secs();
    let hash = Sha256::new().chain_update(ts.to_le_bytes()).finalize();
    let hex: String = hash[..8].iter().map(|b| format!("{b:02x}")).collect();
    format!("drone_{hex}")
}

fn get_environment() -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    format!("{os}-{arch}")
}

// ── Identity ──

/// The drone's identity and configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DroneIdentity {
    pub id: String,
    pub name: String,
    pub port: u16,
    pub environment: String,
    pub capabilities: Vec<String>,
    pub first_seen: u64,
    pub start_time: u64,
}

impl DroneIdentity {
    pub fn new(name: &str, port: u16) -> Self {
        let now = now_secs();
        Self {
            id: generate_drone_id(),
            name: name.to_string(),
            port,
            environment: get_environment(),
            capabilities: vec![
                "file_execution".into(),
                "test_runner".into(),
                "build_system".into(),
                "general".into(),
            ],
            first_seen: now,
            start_time: now,
        }
    }

    /// Try to load persisted identity, falling back to a fresh one.
    pub fn load_or_create(name: &str, port: u16, workspace: &Path) -> Self {
        let path = workspace.join(".velocity").join("drone_identity.json");
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(mut id) = serde_json::from_str::<DroneIdentity>(&data) {
                id.port = port;
                id.start_time = now_secs();
                return id;
            }
        }
        let id = Self::new(name, port);
        id.save(workspace).ok();
        id
    }

    /// Persist identity to disk.
    pub fn save(&self, workspace: &Path) -> Result<(), String> {
        let dir = workspace.join(".velocity");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("drone_identity.json"), json).map_err(|e| e.to_string())
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "name": self.name,
            "host": "0.0.0.0",
            "port": self.port,
            "version": env!("CARGO_PKG_VERSION"),
            "environment": self.environment,
            "capabilities": self.capabilities,
            "first_seen": self.first_seen,
            "last_seen": now_secs(),
            "online": true,
        })
    }
}

// ── File Transfer ──

/// Tracks an in-progress file transfer.
pub struct FileTransfer {
    pub transfer_id: String,
    pub filename: String,
    pub total_size: u64,
    pub sha256: String,
    pub total_chunks: u32,
    pub instructions: Option<String>,
    pub chunks: HashMap<u32, Vec<u8>>,
    pub complete: bool,
    pub started_at: u64,
}

impl FileTransfer {
    pub fn new(
        transfer_id: &str,
        filename: &str,
        total_size: u64,
        sha256: &str,
        total_chunks: u32,
        instructions: Option<&str>,
    ) -> Self {
        Self {
            transfer_id: transfer_id.to_string(),
            filename: filename.to_string(),
            total_size,
            sha256: sha256.to_string(),
            total_chunks,
            instructions: instructions.map(|s| s.to_string()),
            chunks: HashMap::new(),
            complete: false,
            started_at: now_secs(),
        }
    }

    pub fn receive_chunk(&mut self, index: u32, data: Vec<u8>) -> bool {
        if index < self.total_chunks {
            self.chunks.insert(index, data);
            true
        } else {
            false
        }
    }

    pub fn is_complete(&self) -> bool {
        self.chunks.len() as u32 >= self.total_chunks
    }

    pub fn missing_chunks(&self) -> Vec<u32> {
        (0..self.total_chunks)
            .filter(|i| !self.chunks.contains_key(i))
            .collect()
    }

    /// Assemble all chunks in order.
    pub fn assemble(&self) -> Result<Vec<u8>, String> {
        let mut parts = Vec::with_capacity(self.total_chunks as usize);
        for i in 0..self.total_chunks {
            match self.chunks.get(&i) {
                Some(chunk) => parts.push(chunk),
                None => return Err(format!("Missing chunk {i}")),
            }
        }
        Ok(parts.iter().flat_map(|c| c.iter()).copied().collect())
    }

    /// Verify assembled data against expected SHA-256 hash.
    pub fn verify(data: &[u8], expected_hash: &str) -> bool {
        if expected_hash.is_empty() {
            return true;
        }
        let actual = format!("{:x}", Sha256::new().chain_update(data).finalize());
        actual == expected_hash
    }
}

// ── Task ──

/// A task delegated to the drone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub task_id: String,
    pub prompt: String,
    pub instructions: String,
    pub attached_files: Vec<String>,
    pub status: String, // pending, running, completed, failed
    pub progress: f32,
    pub result: Option<TaskResult>,
    pub error: Option<String>,
    pub created_at: u64,
    pub completed_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Task {
    pub fn new(
        task_id: &str,
        prompt: &str,
        instructions: &str,
        attached_files: Vec<String>,
    ) -> Self {
        Self {
            task_id: task_id.to_string(),
            prompt: prompt.to_string(),
            instructions: instructions.to_string(),
            attached_files,
            status: "pending".into(),
            progress: 0.0,
            result: None,
            error: None,
            created_at: now_secs(),
            completed_at: None,
        }
    }

    /// Execute the task as a shell command.
    pub fn execute(&mut self, workspace: &Path) {
        self.status = "running".into();
        self.progress = 10.0;

        let result = std::process::Command::new(if cfg!(windows) { "cmd" } else { "sh" })
            .arg(if cfg!(windows) { "/C" } else { "-c" })
            .arg(&self.instructions)
            .current_dir(workspace)
            .output();

        self.progress = 100.0;

        match result {
            Ok(output) => {
                let exit_code = output.status.code().unwrap_or(-1);
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                if exit_code == 0 {
                    self.status = "completed".into();
                } else {
                    self.status = "failed".into();
                    self.error = Some(format!("Exit code {exit_code}"));
                }
                self.result = Some(TaskResult {
                    exit_code,
                    stdout: stdout.chars().take(10000).collect(),
                    stderr: stderr.chars().take(5000).collect(),
                });
                self.completed_at = Some(now_secs());
            }
            Err(e) => {
                self.status = "failed".into();
                self.error = Some(e.to_string());
                self.completed_at = Some(now_secs());
            }
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "task_id": self.task_id,
            "prompt": self.prompt,
            "status": self.status,
            "progress": self.progress,
            "result": self.result.as_ref().map(|r| serde_json::json!({
                "exit_code": r.exit_code,
                "stdout": r.stdout,
                "stderr": r.stderr,
            })),
            "error": self.error,
            "created_at": self.created_at,
            "completed_at": self.completed_at,
        })
    }
}

// ── Drone Core ──

/// Core drone logic: manages identity, transfers, tasks, messages, and paired peers.
pub struct DroneCore {
    pub identity: DroneIdentity,
    pub workspace: PathBuf,
    pub transfers: Mutex<HashMap<String, FileTransfer>>,
    pub tasks: Mutex<HashMap<String, Arc<Mutex<Task>>>>,
    pub messages: Mutex<Vec<serde_json::Value>>,
    pub paired_peers: Mutex<HashMap<String, serde_json::Value>>,
}

impl DroneCore {
    pub fn new(identity: DroneIdentity, workspace: PathBuf) -> Self {
        // Ensure drops directory exists.
        let drops = workspace.join(".velocity").join("drops");
        std::fs::create_dir_all(&drops).ok();

        Self {
            identity,
            workspace,
            transfers: Mutex::new(HashMap::new()),
            tasks: Mutex::new(HashMap::new()),
            messages: Mutex::new(Vec::new()),
            paired_peers: Mutex::new(HashMap::new()),
        }
    }

    pub fn drops_dir(&self) -> PathBuf {
        self.workspace.join(".velocity").join("drops")
    }

    // ── Pairing ──

    pub fn handle_pair(&self, peer_id: &str, name: &str) -> serde_json::Value {
        let mut peers = self.paired_peers.lock_safe();
        peers.insert(
            peer_id.to_string(),
            serde_json::json!({
                "id": peer_id,
                "name": name,
                "paired_at": now_secs(),
            }),
        );
        serde_json::json!({
            "accepted": true,
            "drone_id": self.identity.id,
            "drone_name": self.identity.name,
        })
    }

    // ── Messages ──

    pub fn handle_message(&self, msg: serde_json::Value) -> serde_json::Value {
        let mut messages = self.messages.lock_safe();
        let msg_id = msg
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        messages.push(msg);
        // Keep only last 200 messages.
        if messages.len() > 200 {
            let drain_count = messages.len() - 200;
            messages.drain(..drain_count);
        }
        serde_json::json!({ "received": true, "message_id": msg_id })
    }

    // ── File Transfer ──

    pub fn handle_file_start(&self, data: &serde_json::Value) -> serde_json::Value {
        let transfer_id = data["transfer_id"].as_str().unwrap_or("");
        let filename = data["filename"].as_str().unwrap_or("");
        let total_size = data["total_size"].as_u64().unwrap_or(0);
        let sha256 = data["sha256"].as_str().unwrap_or("");
        let total_chunks = data["total_chunks"].as_u64().unwrap_or(1) as u32;
        let instructions = data["instructions"].as_str();

        let transfer = FileTransfer::new(
            transfer_id,
            filename,
            total_size,
            sha256,
            total_chunks,
            instructions,
        );

        let mut transfers = self.transfers.lock_safe();
        transfers.insert(transfer_id.to_string(), transfer);

        serde_json::json!({
            "accepted": true,
            "transfer_id": transfer_id,
            "save_path": self.drops_dir().join(format!("{transfer_id}.partial")).to_string_lossy(),
        })
    }

    pub fn handle_file_chunk(&self, data: &serde_json::Value) -> serde_json::Value {
        let transfer_id = data["transfer_id"].as_str().unwrap_or("");
        let index = data["index"].as_u64().unwrap_or(0) as u32;
        let b64_data = data["data"].as_str().unwrap_or("");

        let chunk_data = match B64.decode(b64_data) {
            Ok(d) => d,
            Err(e) => return serde_json::json!({ "error": format!("Base64 decode: {e}") }),
        };

        let mut transfers = self.transfers.lock_safe();
        match transfers.get_mut(transfer_id) {
            Some(transfer) => {
                let ok = transfer.receive_chunk(index, chunk_data);
                serde_json::json!({ "received": ok, "index": index })
            }
            None => serde_json::json!({ "error": format!("Unknown transfer {transfer_id}") }),
        }
    }

    pub fn handle_file_complete(&self, data: &serde_json::Value) -> serde_json::Value {
        let transfer_id = data["transfer_id"].as_str().unwrap_or("");

        let mut transfers = self.transfers.lock_safe();
        let transfer = match transfers.get_mut(transfer_id) {
            Some(t) => t,
            None => {
                return serde_json::json!({ "error": format!("Unknown transfer {transfer_id}") })
            }
        };

        if !transfer.is_complete() {
            let missing = transfer.missing_chunks().len();
            return serde_json::json!({
                "complete": false,
                "error": format!("Missing {missing} chunks"),
            });
        }

        // Assemble the file.
        let file_data = match transfer.assemble() {
            Ok(d) => d,
            Err(e) => return serde_json::json!({ "complete": false, "error": e }),
        };

        // Verify hash.
        let verified = FileTransfer::verify(&file_data, &transfer.sha256);

        // Save to destination.
        let dest_path = self.drops_dir().join(&transfer.filename);
        if let Some(parent) = dest_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        if let Err(e) = std::fs::write(&dest_path, &file_data) {
            return serde_json::json!({ "complete": false, "error": format!("Write: {e}") });
        }

        // Execute deployment instructions.
        let deploy_result = if let Some(ref instructions) = transfer.instructions {
            let output = execute_deploy_instructions(
                instructions,
                &dest_path.to_string_lossy(),
                &self.workspace,
            );
            serde_json::json!({
                "deployed": true,
                "dest_path": dest_path.to_string_lossy(),
                "execution_output": output,
            })
        } else {
            serde_json::json!({
                "deployed": true,
                "dest_path": dest_path.to_string_lossy(),
            })
        };

        transfer.complete = true;
        serde_json::json!({
            "complete": true,
            "verified": verified,
            "deploy_result": deploy_result,
        })
    }

    // ── Tasks ──

    pub fn handle_task(&self, data: &serde_json::Value) -> serde_json::Value {
        let task_id = data
            .get("task_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let prompt = data
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let instructions = data
            .get("instructions")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let attached_files: Vec<String> = data
            .get("attached_files")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let task = Task::new(&task_id, &prompt, &instructions, attached_files);
        let task_arc = Arc::new(Mutex::new(task));

        {
            let mut tasks = self.tasks.lock_safe();
            tasks.insert(task_id.clone(), Arc::clone(&task_arc));
        }

        // Execute in a background thread.
        let task_clone = Arc::clone(&task_arc);
        let workspace = self.workspace.clone();
        std::thread::Builder::new()
            .name(format!("drone-task-{task_id}"))
            .spawn(move || {
                let mut t = task_clone.lock_safe();
                t.execute(&workspace);
            })
            .ok();

        serde_json::json!({
            "accepted": true,
            "task_id": task_id,
            "status": "pending",
        })
    }

    pub fn handle_task_status(&self, task_id: &str) -> (u16, serde_json::Value) {
        let tasks = self.tasks.lock_safe();
        match tasks.get(task_id) {
            Some(task_arc) => {
                let task = task_arc.lock_safe();
                (200, task.to_json())
            }
            None => (
                404,
                serde_json::json!({ "error": format!("Unknown task {task_id}") }),
            ),
        }
    }
}

// ── Deployment Instructions ──

/// Execute deployment instructions line by line.
pub fn execute_deploy_instructions(
    instructions: &str,
    file_path: &str,
    workspace: &Path,
) -> String {
    let mut output = Vec::new();

    for line in instructions.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(cmd) = line.strip_prefix("run ") {
            let cmd = cmd.replace("{file}", file_path);
            output.push(format!("[run] {cmd}"));
            let result = std::process::Command::new(if cfg!(windows) { "cmd" } else { "sh" })
                .arg(if cfg!(windows) { "/C" } else { "-c" })
                .arg(&cmd)
                .current_dir(workspace)
                .output();
            match result {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let stdout = stdout.trim();
                    if !stdout.is_empty() {
                        output.push(format!("  stdout: {stdout}"));
                    }
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    let stderr = stderr.trim();
                    if !stderr.is_empty() {
                        output.push(format!("  stderr: {stderr}"));
                    }
                    output.push(format!("  exit: {}", out.status.code().unwrap_or(-1)));
                }
                Err(e) => output.push(format!("  error: {e}")),
            }
        } else if let Some(dest) = line.strip_prefix("copy ") {
            let dest = dest.replace("{file}", file_path);
            output.push(format!("[copy] {file_path} -> {dest}"));
            match std::fs::copy(file_path, &dest) {
                Ok(_) => output.push("  copied successfully".into()),
                Err(e) => output.push(format!("  error: {e}")),
            }
        } else if let Some(msg) = line.strip_prefix("notify ") {
            output.push(format!("[notify] {msg}"));
        } else {
            output.push(format!("[unknown] {line}"));
        }
    }

    output.join("\n")
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    fn test_workspace() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("drone_test_{}", now_secs()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn test_core() -> DroneCore {
        let ws = test_workspace();
        let identity = DroneIdentity::new("TestDrone", 19999);
        DroneCore::new(identity, ws)
    }

    #[test]
    fn identity_creation() {
        let id = DroneIdentity::new("MyDrone", 9191);
        assert_eq!(id.name, "MyDrone");
        assert_eq!(id.port, 9191);
        assert!(id.id.starts_with("drone_"));
        assert!(id.capabilities.contains(&"file_execution".to_string()));
    }

    #[test]
    fn identity_persistence() {
        let ws = test_workspace();
        let id1 = DroneIdentity::new("Original", 9191);
        id1.save(&ws).unwrap();

        let id2 = DroneIdentity::load_or_create("New", 9191, &ws);
        assert_eq!(id2.id, id1.id);
        assert_eq!(id2.name, "Original");
    }

    #[test]
    fn identity_to_json() {
        let id = DroneIdentity::new("JsonTest", 9191);
        let json = id.to_json();
        assert_eq!(json["name"], "JsonTest");
        assert!(json["online"].as_bool().unwrap());
    }

    #[test]
    fn file_transfer_basic() {
        let mut ft = FileTransfer::new("xfer1", "test.bin", 300, "abc", 3, None);
        assert!(!ft.is_complete());
        assert_eq!(ft.missing_chunks(), vec![0, 1, 2]);

        ft.receive_chunk(0, vec![1, 2, 3]);
        ft.receive_chunk(1, vec![4, 5, 6]);
        assert!(!ft.is_complete());
        assert_eq!(ft.missing_chunks(), vec![2]);

        ft.receive_chunk(2, vec![7, 8, 9]);
        assert!(ft.is_complete());

        let data = ft.assemble().unwrap();
        assert_eq!(data, vec![1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn file_transfer_verify() {
        let data = b"hello world";
        let hash = format!("{:x}", Sha256::new().chain_update(data).finalize());
        assert!(FileTransfer::verify(data, &hash));
        assert!(!FileTransfer::verify(b"wrong", &hash));
        assert!(FileTransfer::verify(data, "")); // empty hash = skip verify
    }

    #[test]
    fn file_transfer_reject_bad_index() {
        let mut ft = FileTransfer::new("xfer2", "test.bin", 100, "", 2, None);
        assert!(!ft.receive_chunk(5, vec![])); // out of range
        assert!(ft.receive_chunk(0, vec![1])); // valid
    }

    #[test]
    fn task_creation() {
        let task = Task::new("t1", "Run tests", "cargo test", vec![]);
        assert_eq!(task.status, "pending");
        assert!(task.result.is_none());
    }

    #[test]
    fn task_execution() {
        let ws = test_workspace();
        let mut task = Task::new("t1", "Echo", "echo hello", vec![]);
        task.execute(&ws);
        assert_eq!(task.status, "completed");
        assert_eq!(task.result.as_ref().unwrap().exit_code, 0);
        assert!(task.result.as_ref().unwrap().stdout.contains("hello"));
    }

    #[test]
    fn task_failure() {
        let ws = test_workspace();
        let mut task = Task::new("t2", "Fail", "exit 1", vec![]);
        task.execute(&ws);
        assert_eq!(task.status, "failed");
        assert!(task.error.is_some());
    }

    #[test]
    fn core_pair() {
        let core = test_core();
        let result = core.handle_pair("peer1", "Test IDE");
        assert!(result["accepted"].as_bool().unwrap());
        assert_eq!(result["drone_name"], "TestDrone");

        let peers = core.paired_peers.lock().unwrap();
        assert!(peers.contains_key("peer1"));
    }

    #[test]
    fn core_message() {
        let core = test_core();
        let msg = serde_json::json!({"id": "m1", "from": "p1", "kind": "Chat"});
        let result = core.handle_message(msg);
        assert!(result["received"].as_bool().unwrap());

        let messages = core.messages.lock().unwrap();
        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn core_message_cap() {
        let core = test_core();
        for i in 0..250 {
            core.handle_message(serde_json::json!({"id": format!("m{i}")}));
        }
        let messages = core.messages.lock().unwrap();
        assert_eq!(messages.len(), 200); // capped
    }

    #[test]
    fn core_file_transfer_flow() {
        let core = test_core();
        let content = b"test file content";
        let hash = format!("{:x}", Sha256::new().chain_update(content).finalize());
        let b64 = B64.encode(content);

        // Start.
        let start = core.handle_file_start(&serde_json::json!({
            "transfer_id": "xfer1",
            "filename": "test.txt",
            "total_size": content.len(),
            "sha256": hash,
            "total_chunks": 1,
            "instructions": "notify Received",
        }));
        assert!(start["accepted"].as_bool().unwrap());

        // Chunk.
        let chunk = core.handle_file_chunk(&serde_json::json!({
            "transfer_id": "xfer1",
            "index": 0,
            "data": b64,
        }));
        assert!(chunk["received"].as_bool().unwrap());

        // Complete.
        let complete = core.handle_file_complete(&serde_json::json!({
            "transfer_id": "xfer1",
        }));
        assert!(complete["complete"].as_bool().unwrap());
        assert!(complete["verified"].as_bool().unwrap());
        assert!(complete["deploy_result"]["deployed"].as_bool().unwrap());
    }

    #[test]
    fn core_file_incomplete() {
        let core = test_core();
        core.handle_file_start(&serde_json::json!({
            "transfer_id": "xfer2",
            "filename": "big.bin",
            "total_size": 300,
            "sha256": "abc",
            "total_chunks": 3,
        }));
        core.handle_file_chunk(&serde_json::json!({
            "transfer_id": "xfer2",
            "index": 0,
            "data": B64.encode(b"chunk"),
        }));

        let result = core.handle_file_complete(&serde_json::json!({
            "transfer_id": "xfer2",
        }));
        assert!(!result["complete"].as_bool().unwrap());
    }

    #[test]
    fn core_task() {
        let core = test_core();
        let result = core.handle_task(&serde_json::json!({
            "task_id": "task1",
            "prompt": "Echo test",
            "instructions": "echo drone_test",
        }));
        assert!(result["accepted"].as_bool().unwrap());

        // Wait for background thread.
        std::thread::sleep(std::time::Duration::from_millis(500));

        let (status, json) = core.handle_task_status("task1");
        assert_eq!(status, 200);
        assert_eq!(json["status"], "completed");
        assert!(json["result"]["stdout"]
            .as_str()
            .unwrap()
            .contains("drone_test"));
    }

    #[test]
    fn core_task_unknown() {
        let core = test_core();
        let (status, json) = core.handle_task_status("nonexistent");
        assert_eq!(status, 404);
        assert!(json["error"].as_str().unwrap().contains("Unknown"));
    }

    #[test]
    fn deploy_notify() {
        let ws = test_workspace();
        let output = execute_deploy_instructions("notify Hello World", "/tmp/f.txt", &ws);
        assert!(output.contains("[notify] Hello World"));
    }

    #[test]
    fn deploy_comments_ignored() {
        let ws = test_workspace();
        let output = execute_deploy_instructions("# comment\nnotify After", "/tmp/f.txt", &ws);
        assert!(!output.contains("[unknown]"));
        assert!(output.contains("[notify] After"));
    }

    #[test]
    fn deploy_run() {
        let ws = test_workspace();
        let output = execute_deploy_instructions("run echo {file}", "/tmp/test.exe", &ws);
        assert!(output.contains("[run] echo /tmp/test.exe"));
        assert!(output.contains("exit: 0"));
    }
}
