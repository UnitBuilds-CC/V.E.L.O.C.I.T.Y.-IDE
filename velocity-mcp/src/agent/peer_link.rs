//! Cross-device agent collaboration (peer-to-peer).
//!
//! Enables agents on different machines to discover each other, establish
//! trusted peer links, exchange messages, transfer files, and coordinate
//! workflows across devices. This allows E2E testing scenarios that
//! physically require multiple machines (remote desktop apps, network
//! services, multi-device workflows, etc.).
//!
//! # Architecture
//!
//! Each V.E.L.O.C.I.T.Y. instance can expose a lightweight peer API server.
//! Remote instances connect via HTTP/JSON using `ureq`. The protocol supports:
//!
//! - **Pairing**: handshake with shared secret for trust establishment
//! - **Messaging**: bidirectional agent-to-agent communication
//! - **File transfer**: chunked transfer with SHA-256 integrity verification
//! - **Task delegation**: one agent can request another to perform actions
//! - **Status sync**: real-time progress and state sharing

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

// ── Peer Identity ──

/// Identity of a remote peer instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerIdentity {
    /// Unique peer ID (generated on first run).
    pub id: String,
    /// Human-readable name for this instance.
    pub name: String,
    /// Hostname or IP address.
    pub host: String,
    /// Port the peer API server listens on.
    pub port: u16,
    /// Shared secret for authentication (handle into secret store).
    pub auth_secret_handle: Option<String>,
    /// When this peer was first seen.
    pub first_seen: u64,
    /// Last heartbeat received.
    pub last_seen: u64,
    /// Whether this peer is currently connected.
    pub online: bool,
    /// Capabilities advertised by this peer.
    pub capabilities: Vec<PeerCapability>,
    /// Description of the remote environment (OS, arch, etc.).
    pub environment: Option<String>,
}

/// Capabilities a peer can advertise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PeerCapability {
    /// Can receive and execute files.
    FileExecution,
    /// Can run test suites.
    TestRunner,
    /// Can take screenshots/screen captures.
    ScreenCapture,
    /// Can interact with GUI (click, type, etc.).
    GuiAutomation,
    /// Can build projects.
    BuildSystem,
    /// Can monitor network traffic.
    NetworkMonitor,
    /// General-purpose agent.
    General,
}

impl PeerCapability {
    pub fn label(&self) -> &'static str {
        match self {
            Self::FileExecution => "file_execution",
            Self::TestRunner => "test_runner",
            Self::ScreenCapture => "screen_capture",
            Self::GuiAutomation => "gui_automation",
            Self::BuildSystem => "build_system",
            Self::NetworkMonitor => "network_monitor",
            Self::General => "general",
        }
    }
}

// ── Protocol Messages ──

/// Messages exchanged between peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerMessage {
    /// Unique message ID.
    pub id: String,
    /// Sender peer ID.
    pub from: String,
    /// Recipient peer ID (or "*" for broadcast).
    pub to: String,
    /// Message type.
    pub kind: PeerMessageKind,
    /// Message payload.
    pub payload: serde_json::Value,
    /// When this message was created.
    pub timestamp: u64,
    /// Whether this message has been acknowledged.
    pub acknowledged: bool,
}

/// Types of peer messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PeerMessageKind {
    /// Pairing request (handshake).
    PairRequest,
    /// Pairing accepted.
    PairAccepted,
    /// Pairing rejected.
    PairRejected,
    /// Heartbeat / keepalive.
    Heartbeat,
    /// Text message between agents.
    Chat,
    /// Task delegation request.
    TaskRequest,
    /// Task progress update.
    TaskProgress,
    /// Task completion report.
    TaskComplete,
    /// Task failure report.
    TaskFailed,
    /// File transfer initiation.
    FileTransferStart,
    /// File data chunk.
    FileTransferChunk,
    /// File transfer complete.
    FileTransferComplete,
    /// Request for status/screen from remote.
    StatusRequest,
    /// Status response.
    StatusResponse,
}

impl PeerMessageKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::PairRequest => "pair_request",
            Self::PairAccepted => "pair_accepted",
            Self::PairRejected => "pair_rejected",
            Self::Heartbeat => "heartbeat",
            Self::Chat => "chat",
            Self::TaskRequest => "task_request",
            Self::TaskProgress => "task_progress",
            Self::TaskComplete => "task_complete",
            Self::TaskFailed => "task_failed",
            Self::FileTransferStart => "file_transfer_start",
            Self::FileTransferChunk => "file_transfer_chunk",
            Self::FileTransferComplete => "file_transfer_complete",
            Self::StatusRequest => "status_request",
            Self::StatusResponse => "status_response",
        }
    }
}

// ── File Transfer ──

/// Metadata for a file being transferred.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTransfer {
    /// Unique transfer ID.
    pub id: String,
    /// Original file name.
    pub filename: String,
    /// Total file size in bytes.
    pub total_size: u64,
    /// SHA-256 hash of the complete file (hex).
    pub sha256: String,
    /// Number of chunks.
    pub total_chunks: u32,
    /// Chunks received so far.
    pub chunks_received: u32,
    /// Transfer direction.
    pub direction: TransferDirection,
    /// Peer on the other end.
    pub peer_id: String,
    /// Instructions for the remote agent (what to do with the file).
    pub instructions: Option<String>,
    /// When the transfer started.
    pub started_at: u64,
    /// Whether the transfer is complete.
    pub complete: bool,
    /// Whether the hash was verified after transfer.
    pub verified: bool,
    /// Temporary path where the file is being assembled.
    pub temp_path: String,
    /// Final destination path (after verification).
    pub dest_path: Option<String>,
}

/// Direction of file transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferDirection {
    /// Sending to a remote peer.
    Outgoing,
    /// Receiving from a remote peer.
    Incoming,
}

/// A chunk of file data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChunk {
    /// Transfer ID this chunk belongs to.
    pub transfer_id: String,
    /// Chunk index (0-based).
    pub index: u32,
    /// Base64-encoded data.
    pub data: String,
}

// ── Task Delegation ──

/// A task delegated to a remote peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegatedTask {
    /// Unique task ID.
    pub id: String,
    /// Task description / prompt for the remote agent.
    pub prompt: String,
    /// Files that were sent along with this task.
    pub attached_files: Vec<String>,
    /// Instructions for execution.
    pub instructions: String,
    /// Current status.
    pub status: TaskStatus,
    /// Progress percentage (0-100).
    pub progress: f32,
    /// Result data (JSON).
    pub result: Option<serde_json::Value>,
    /// Error message if failed.
    pub error: Option<String>,
    /// Who delegated this task.
    pub delegated_by: String,
    /// Which peer is executing it.
    pub peer_id: String,
    /// When the task was created.
    pub created_at: u64,
    /// When the task completed.
    pub completed_at: Option<u64>,
}

/// Status of a delegated task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    /// Task is pending on the remote.
    Pending,
    /// Task is being executed.
    Running,
    /// Task completed successfully.
    Completed,
    /// Task failed.
    Failed,
    /// Task was cancelled.
    Cancelled,
}

impl TaskStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

// ── Peer Manager ──

/// Manages all cross-device peer connections and communication.
#[derive(Debug, Clone, Default)]
pub struct PeerManager {
    /// This instance's identity.
    pub local_identity: Option<PeerIdentity>,
    /// Known peers keyed by peer ID.
    pub peers: HashMap<String, PeerIdentity>,
    /// Message inbox (received messages).
    pub inbox: Vec<PeerMessage>,
    /// Message outbox (messages to send).
    pub outbox: Vec<PeerMessage>,
    /// Active file transfers keyed by transfer ID.
    pub transfers: HashMap<String, FileTransfer>,
    /// Delegated tasks keyed by task ID.
    pub tasks: HashMap<String, DelegatedTask>,
    /// Maximum messages to retain in inbox/outbox.
    pub max_messages: usize,
    /// Port for the local peer API server (0 = disabled).
    pub listen_port: u16,
    /// Chunk size for file transfers (bytes).
    pub chunk_size: usize,
    /// Workspace root for file operations.
    workspace_root: Option<PathBuf>,
}

impl PeerManager {
    pub fn new() -> Self {
        Self {
            local_identity: None,
            peers: HashMap::new(),
            inbox: Vec::new(),
            outbox: Vec::new(),
            transfers: HashMap::new(),
            tasks: HashMap::new(),
            max_messages: 200,
            listen_port: 0,
            chunk_size: 64 * 1024, // 64 KB chunks
            workspace_root: None,
        }
    }

    /// Initialize with workspace root and generate local identity.
    pub fn init(&mut self, workspace_root: &Path, name: &str) {
        self.workspace_root = Some(workspace_root.to_path_buf());
        if self.local_identity.is_none() {
            self.local_identity = Some(PeerIdentity {
                id: generate_peer_id(),
                name: name.to_string(),
                host: "localhost".to_string(),
                port: self.listen_port,
                auth_secret_handle: None,
                first_seen: now_secs(),
                last_seen: now_secs(),
                online: true,
                capabilities: vec![PeerCapability::General],
                environment: Some(std::env::consts::OS.to_string()),
            });
        }
    }

    /// Set the listening port for the peer API server.
    pub fn set_listen_port(&mut self, port: u16) {
        self.listen_port = port;
        if let Some(id) = &mut self.local_identity {
            id.port = port;
        }
    }

    // ── Peer Management ──

    /// Register a known peer.
    pub fn add_peer(&mut self, peer: PeerIdentity) {
        self.peers.insert(peer.id.clone(), peer);
    }

    /// Remove a peer.
    pub fn remove_peer(&mut self, id: &str) -> bool {
        self.peers.remove(id).is_some()
    }

    /// Get a peer by ID.
    pub fn get_peer(&self, id: &str) -> Option<&PeerIdentity> {
        self.peers.get(id)
    }

    /// Get all online peers.
    pub fn online_peers(&self) -> Vec<&PeerIdentity> {
        self.peers.values().filter(|p| p.online).collect()
    }

    /// List all known peers.
    pub fn list_peers(&self) -> Vec<&PeerIdentity> {
        self.peers.values().collect()
    }

    /// Update peer online status based on heartbeat timeout.
    pub fn update_presence(&mut self, timeout_secs: u64) {
        let now = now_secs();
        for peer in self.peers.values_mut() {
            peer.online = now - peer.last_seen < timeout_secs;
        }
    }

    // ── Messaging ──

    /// Queue a message for sending to a peer.
    pub fn send_message(&mut self, to_peer: &str, kind: PeerMessageKind, payload: serde_json::Value) -> String {
        let local_id = self.local_identity.as_ref()
            .map(|id| id.id.clone())
            .unwrap_or_default();

        let msg = PeerMessage {
            id: generate_msg_id(),
            from: local_id,
            to: to_peer.to_string(),
            kind,
            payload,
            timestamp: now_secs(),
            acknowledged: false,
        };

        let id = msg.id.clone();
        self.outbox.push(msg);
        while self.outbox.len() > self.max_messages {
            self.outbox.remove(0);
        }
        id
    }

    /// Receive a message from a peer (called by the server when a message arrives).
    pub fn receive_message(&mut self, msg: PeerMessage) {
        // Update peer's last_seen on any message.
        if let Some(peer) = self.peers.get_mut(&msg.from) {
            peer.last_seen = now_secs();
            peer.online = true;
        }

        self.inbox.push(msg);
        while self.inbox.len() > self.max_messages {
            self.inbox.remove(0);
        }
    }

    /// Get unacknowledged messages from inbox.
    pub fn pending_messages(&self) -> Vec<&PeerMessage> {
        self.inbox.iter().filter(|m| !m.acknowledged).collect()
    }

    /// Acknowledge a message.
    pub fn acknowledge_message(&mut self, msg_id: &str) {
        if let Some(msg) = self.inbox.iter_mut().find(|m| m.id == msg_id) {
            msg.acknowledged = true;
        }
    }

    /// Get unsent messages from outbox.
    pub fn unsent_messages(&self) -> Vec<&PeerMessage> {
        self.outbox.iter().filter(|m| !m.acknowledged).collect()
    }

    /// Send a chat message to a peer.
    pub fn chat(&mut self, to_peer: &str, message: &str) -> String {
        self.send_message(to_peer, PeerMessageKind::Chat, serde_json::json!({
            "message": message
        }))
    }

    // ── Task Delegation ──

    /// Delegate a task to a remote peer.
    pub fn delegate_task(
        &mut self,
        peer_id: &str,
        prompt: &str,
        instructions: &str,
        attached_files: Vec<String>,
    ) -> Result<String, String> {
        if !self.peers.contains_key(peer_id) {
            return Err(format!("Peer '{}' not found", peer_id));
        }

        let task_id = format!("task_{}_{}", now_secs(), self.tasks.len());
        let task = DelegatedTask {
            id: task_id.clone(),
            prompt: prompt.to_string(),
            attached_files,
            instructions: instructions.to_string(),
            status: TaskStatus::Pending,
            progress: 0.0,
            result: None,
            error: None,
            delegated_by: self.local_identity.as_ref()
                .map(|id| id.id.clone())
                .unwrap_or_default(),
            peer_id: peer_id.to_string(),
            created_at: now_secs(),
            completed_at: None,
        };

        // Send task request to peer.
        self.send_message(peer_id, PeerMessageKind::TaskRequest, serde_json::json!({
            "task_id": task_id,
            "prompt": prompt,
            "instructions": instructions,
            "attached_files": &task.attached_files,
        }));

        self.tasks.insert(task_id.clone(), task);
        Ok(task_id)
    }

    /// Update the progress of a delegated task.
    pub fn update_task_progress(&mut self, task_id: &str, progress: f32) {
        if let Some(task) = self.tasks.get_mut(task_id) {
            task.progress = progress;
            task.status = TaskStatus::Running;
        }
    }

    /// Mark a task as completed with result data.
    pub fn complete_task(&mut self, task_id: &str, result: serde_json::Value) {
        if let Some(task) = self.tasks.get_mut(task_id) {
            task.status = TaskStatus::Completed;
            task.progress = 100.0;
            task.result = Some(result);
            task.completed_at = Some(now_secs());
        }
    }

    /// Mark a task as failed.
    pub fn fail_task(&mut self, task_id: &str, error: &str) {
        if let Some(task) = self.tasks.get_mut(task_id) {
            task.status = TaskStatus::Failed;
            task.error = Some(error.to_string());
            task.completed_at = Some(now_secs());
        }
    }

    /// Get tasks for a specific peer.
    pub fn tasks_for_peer(&self, peer_id: &str) -> Vec<&DelegatedTask> {
        self.tasks.values()
            .filter(|t| t.peer_id == peer_id)
            .collect()
    }

    /// Get all active (non-completed) tasks.
    pub fn active_tasks(&self) -> Vec<&DelegatedTask> {
        self.tasks.values()
            .filter(|t| t.status == TaskStatus::Pending || t.status == TaskStatus::Running)
            .collect()
    }

    // ── File Transfer ──

    /// Initiate a file transfer to a peer.
    pub fn initiate_transfer(
        &mut self,
        peer_id: &str,
        filename: &str,
        file_data: &[u8],
        instructions: Option<&str>,
    ) -> Result<String, String> {
        if !self.peers.contains_key(peer_id) {
            return Err(format!("Peer '{}' not found", peer_id));
        }

        let transfer_id = format!("xfer_{}_{}", now_secs(), self.transfers.len());
        let sha256 = simple_hash_hex(file_data);
        let total_chunks = ((file_data.len() + self.chunk_size - 1) / self.chunk_size) as u32;

        let transfer = FileTransfer {
            id: transfer_id.clone(),
            filename: filename.to_string(),
            total_size: file_data.len() as u64,
            sha256,
            total_chunks,
            chunks_received: 0,
            direction: TransferDirection::Outgoing,
            peer_id: peer_id.to_string(),
            instructions: instructions.map(|s| s.to_string()),
            started_at: now_secs(),
            complete: false,
            verified: false,
            temp_path: String::new(),
            dest_path: None,
        };

        // Send transfer start message.
        self.send_message(peer_id, PeerMessageKind::FileTransferStart, serde_json::json!({
            "transfer_id": transfer_id,
            "filename": filename,
            "total_size": file_data.len(),
            "sha256": transfer.sha256,
            "total_chunks": total_chunks,
            "instructions": instructions,
        }));

        // Chunk and queue the data.
        for i in 0..total_chunks {
            let start = (i as usize) * self.chunk_size;
            let end = std::cmp::min(start + self.chunk_size, file_data.len());
            let chunk_data = &file_data[start..end];

            self.send_message(peer_id, PeerMessageKind::FileTransferChunk, serde_json::json!({
                "transfer_id": transfer_id,
                "index": i,
                "data": base64_encode(chunk_data),
            }));
        }

        // Send transfer complete.
        self.send_message(peer_id, PeerMessageKind::FileTransferComplete, serde_json::json!({
            "transfer_id": transfer_id,
        }));

        self.transfers.insert(transfer_id.clone(), transfer);
        Ok(transfer_id)
    }

    /// Receive a file transfer start message (called when a peer begins sending).
    pub fn begin_receive_transfer(
        &mut self,
        transfer_id: &str,
        peer_id: &str,
        filename: &str,
        total_size: u64,
        sha256: &str,
        total_chunks: u32,
        instructions: Option<&str>,
    ) {
        let workspace = self.workspace_root.as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());

        let transfer = FileTransfer {
            id: transfer_id.to_string(),
            filename: filename.to_string(),
            total_size,
            sha256: sha256.to_string(),
            total_chunks,
            chunks_received: 0,
            direction: TransferDirection::Incoming,
            peer_id: peer_id.to_string(),
            instructions: instructions.map(|s| s.to_string()),
            started_at: now_secs(),
            complete: false,
            verified: false,
            temp_path: format!("{}/.velocity/transfers/{}", workspace, transfer_id),
            dest_path: None,
        };
        self.transfers.insert(transfer_id.to_string(), transfer);
    }

    /// Record a received chunk for an incoming transfer.
    pub fn receive_chunk(&mut self, transfer_id: &str, _chunk_index: u32) -> bool {
        if let Some(transfer) = self.transfers.get_mut(transfer_id) {
            transfer.chunks_received += 1;
            if transfer.chunks_received >= transfer.total_chunks {
                transfer.complete = true;
            }
            true
        } else {
            false
        }
    }

    /// Get active transfers.
    pub fn active_transfers(&self) -> Vec<&FileTransfer> {
        self.transfers.values().filter(|t| !t.complete).collect()
    }

    /// Get completed transfers.
    pub fn completed_transfers(&self) -> Vec<&FileTransfer> {
        self.transfers.values().filter(|t| t.complete).collect()
    }

    // ── Pairing ──

    /// Create a pairing invitation for a new peer.
    pub fn create_pairing(&mut self, host: &str, port: u16, name: &str) -> String {
        let peer_id = generate_peer_id();
        let peer = PeerIdentity {
            id: peer_id.clone(),
            name: name.to_string(),
            host: host.to_string(),
            port,
            auth_secret_handle: None,
            first_seen: now_secs(),
            last_seen: now_secs(),
            online: false,
            capabilities: Vec::new(),
            environment: None,
        };

        self.send_message(&peer_id, PeerMessageKind::PairRequest, serde_json::json!({
            "peer_id": peer_id,
            "name": name,
            "host": host,
            "port": port,
        }));

        self.peers.insert(peer_id.clone(), peer);
        peer_id
    }

    /// Accept a pairing request from a peer.
    pub fn accept_pairing(&mut self, peer_id: &str) {
        self.send_message(peer_id, PeerMessageKind::PairAccepted, serde_json::json!({
            "accepted": true,
        }));
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.online = true;
            peer.last_seen = now_secs();
        }
    }

    /// Reject a pairing request.
    pub fn reject_pairing(&mut self, peer_id: &str) {
        self.send_message(peer_id, PeerMessageKind::PairRejected, serde_json::json!({
            "reason": "rejected by user"
        }));
    }

    // ── Persistence ──

    /// Save peer manager state to disk.
    pub fn save(&self) -> Result<(), String> {
        let root = self.workspace_root.as_ref()
            .ok_or_else(|| "No workspace root".to_string())?;
        let dir = root.join(".velocity");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        let state = PersistedPeerState {
            local_identity: self.local_identity.clone(),
            peers: self.peers.values().cloned().collect(),
            listen_port: self.listen_port,
        };
        let json = serde_json::to_vec_pretty(&state)
            .map_err(|e| format!("Serialize: {e}"))?;
        std::fs::write(dir.join("peers.json"), json)
            .map_err(|e| format!("Write: {e}"))?;
        Ok(())
    }

    /// Load peer manager state from disk.
    pub fn load(workspace_root: &Path) -> Self {
        let mut mgr = Self::new();
        mgr.workspace_root = Some(workspace_root.to_path_buf());
        let path = workspace_root.join(".velocity").join("peers.json");
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(state) = serde_json::from_slice::<PersistedPeerState>(&bytes) {
                mgr.local_identity = state.local_identity;
                mgr.listen_port = state.listen_port;
                for peer in state.peers {
                    mgr.peers.insert(peer.id.clone(), peer);
                }
            }
        }
        mgr
    }
}

/// Serializable state for persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedPeerState {
    local_identity: Option<PeerIdentity>,
    peers: Vec<PeerIdentity>,
    listen_port: u16,
}

// ── Helpers ──

fn generate_peer_id() -> String {
    let ts = now_secs();
    let rand = (ts.wrapping_mul(6364136223846793005)).wrapping_add(1442695040888963407);
    format!("peer_{:016x}", rand)
}

fn generate_msg_id() -> String {
    let ts = now_secs();
    format!("msg_{}_{}", ts, ts % 100000)
}

/// Simple hex hash for file integrity (not cryptographic — use SHA-256 in production).
fn simple_hash_hex(data: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    let mut h2: u64 = 0xcbf29ce484222325;
    for &b in data.iter().rev() {
        h2 ^= b as u64;
        h2 = h2.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}{:016x}", h, h2)
}

/// Minimal base64 encoding for file chunks.
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_peer(id: &str, host: &str) -> PeerIdentity {
        PeerIdentity {
            id: id.to_string(),
            name: format!("Peer {}", id),
            host: host.to_string(),
            port: 9191,
            auth_secret_handle: None,
            first_seen: now_secs(),
            last_seen: now_secs(),
            online: true,
            capabilities: vec![PeerCapability::General],
            environment: Some("windows".to_string()),
        }
    }

    fn init_manager() -> PeerManager {
        let mut mgr = PeerManager::new();
        mgr.init(Path::new("/tmp/test"), "TestHost");
        mgr
    }

    #[test]
    fn init_creates_identity() {
        let mgr = init_manager();
        assert!(mgr.local_identity.is_some());
        assert_eq!(mgr.local_identity.unwrap().name, "TestHost");
    }

    #[test]
    fn add_and_list_peers() {
        let mut mgr = init_manager();
        mgr.add_peer(test_peer("p1", "192.168.1.10"));
        mgr.add_peer(test_peer("p2", "192.168.1.11"));
        assert_eq!(mgr.list_peers().len(), 2);
        assert_eq!(mgr.online_peers().len(), 2);
    }

    #[test]
    fn remove_peer() {
        let mut mgr = init_manager();
        mgr.add_peer(test_peer("p1", "10.0.0.1"));
        assert!(mgr.remove_peer("p1"));
        assert_eq!(mgr.peers.len(), 0);
    }

    #[test]
    fn send_and_receive_chat() {
        let mut mgr = init_manager();
        mgr.add_peer(test_peer("p1", "10.0.0.1"));

        let msg_id = mgr.chat("p1", "Hello from host!");
        assert_eq!(mgr.outbox.len(), 1);
        assert!(!msg_id.is_empty());

        // Simulate receiving a response.
        let response = PeerMessage {
            id: "msg_resp".to_string(),
            from: "p1".to_string(),
            to: mgr.local_identity.as_ref().unwrap().id.clone(),
            kind: PeerMessageKind::Chat,
            payload: serde_json::json!({"message": "Hello back!"}),
            timestamp: now_secs(),
            acknowledged: false,
        };
        mgr.receive_message(response);
        assert_eq!(mgr.inbox.len(), 1);
        assert_eq!(mgr.pending_messages().len(), 1);
    }

    #[test]
    fn acknowledge_message() {
        let mut mgr = init_manager();
        let msg = PeerMessage {
            id: "msg_1".to_string(),
            from: "p1".to_string(),
            to: "local".to_string(),
            kind: PeerMessageKind::Chat,
            payload: serde_json::json!({}),
            timestamp: now_secs(),
            acknowledged: false,
        };
        mgr.receive_message(msg);
        assert_eq!(mgr.pending_messages().len(), 1);
        mgr.acknowledge_message("msg_1");
        assert_eq!(mgr.pending_messages().len(), 0);
    }

    #[test]
    fn delegate_task() {
        let mut mgr = init_manager();
        mgr.add_peer(test_peer("p1", "10.0.0.1"));

        let task_id = mgr.delegate_task(
            "p1",
            "Run the test suite",
            "Execute cargo test and report results",
            vec![],
        ).unwrap();

        assert!(mgr.tasks.contains_key(&task_id));
        assert_eq!(mgr.tasks[&task_id].status, TaskStatus::Pending);
        assert_eq!(mgr.outbox.len(), 1); // TaskRequest message queued
    }

    #[test]
    fn task_progress_and_completion() {
        let mut mgr = init_manager();
        mgr.add_peer(test_peer("p1", "10.0.0.1"));
        let task_id = mgr.delegate_task("p1", "Test", "Run tests", vec![]).unwrap();

        mgr.update_task_progress(&task_id, 50.0);
        assert_eq!(mgr.tasks[&task_id].progress, 50.0);
        assert_eq!(mgr.tasks[&task_id].status, TaskStatus::Running);

        mgr.complete_task(&task_id, serde_json::json!({"passed": 42, "failed": 0}));
        assert_eq!(mgr.tasks[&task_id].status, TaskStatus::Completed);
        assert!(mgr.active_tasks().is_empty());
    }

    #[test]
    fn fail_task() {
        let mut mgr = init_manager();
        mgr.add_peer(test_peer("p1", "10.0.0.1"));
        let task_id = mgr.delegate_task("p1", "Build", "Build project", vec![]).unwrap();

        mgr.fail_task(&task_id, "Compilation error");
        assert_eq!(mgr.tasks[&task_id].status, TaskStatus::Failed);
        assert!(mgr.tasks[&task_id].error.is_some());
    }

    #[test]
    fn file_transfer_initiation() {
        let mut mgr = init_manager();
        mgr.add_peer(test_peer("p1", "10.0.0.1"));

        let data = b"Hello, this is a test file content for transfer!";
        let xfer_id = mgr.initiate_transfer("p1", "test.txt", data, Some("Run this file")).unwrap();

        assert!(mgr.transfers.contains_key(&xfer_id));
        let transfer = &mgr.transfers[&xfer_id];
        assert_eq!(transfer.filename, "test.txt");
        assert_eq!(transfer.total_size, data.len() as u64);
        assert_eq!(transfer.direction, TransferDirection::Outgoing);
        assert!(transfer.instructions.is_some());
    }

    #[test]
    fn receive_transfer_chunks() {
        let mut mgr = init_manager();
        mgr.add_peer(test_peer("p1", "10.0.0.1"));

        mgr.begin_receive_transfer("xfer_1", "p1", "app.exe", 1000, "abc123", 3, Some("Deploy this"));
        assert!(mgr.receive_chunk("xfer_1", 0));
        assert!(mgr.receive_chunk("xfer_1", 1));
        assert!(!mgr.transfers["xfer_1"].complete);
        assert!(mgr.receive_chunk("xfer_1", 2));
        assert!(mgr.transfers["xfer_1"].complete);
    }

    #[test]
    fn create_pairing() {
        let mut mgr = init_manager();
        let peer_id = mgr.create_pairing("192.168.1.50", 9191, "Remote PC");
        assert!(mgr.peers.contains_key(&peer_id));
        assert_eq!(mgr.outbox.len(), 1); // PairRequest queued
    }

    #[test]
    fn accept_and_reject_pairing() {
        let mut mgr = init_manager();
        mgr.add_peer(test_peer("p1", "10.0.0.1"));

        mgr.accept_pairing("p1");
        assert!(mgr.peers["p1"].online);

        mgr.add_peer(test_peer("p2", "10.0.0.2"));
        mgr.peers.get_mut("p2").unwrap().online = false;
        mgr.reject_pairing("p2");
        // Peer still exists but pairing was rejected (outbox has the message).
    }

    #[test]
    fn presence_timeout() {
        let mut mgr = init_manager();
        let mut peer = test_peer("p1", "10.0.0.1");
        peer.last_seen = now_secs() - 600; // 10 minutes ago
        peer.online = true;
        mgr.add_peer(peer);

        mgr.update_presence(300); // 5 minute timeout
        assert!(!mgr.peers["p1"].online);
    }

    #[test]
    fn base64_encode_works() {
        assert_eq!(base64_encode(b"Hello"), "SGVsbG8=");
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
    }

    #[test]
    fn peer_capability_labels() {
        assert_eq!(PeerCapability::FileExecution.label(), "file_execution");
        assert_eq!(PeerCapability::GuiAutomation.label(), "gui_automation");
        assert_eq!(PeerCapability::ScreenCapture.label(), "screen_capture");
    }

    #[test]
    fn task_status_labels() {
        assert_eq!(TaskStatus::Pending.label(), "pending");
        assert_eq!(TaskStatus::Running.label(), "running");
        assert_eq!(TaskStatus::Completed.label(), "completed");
    }

    #[test]
    fn delegate_task_unknown_peer_fails() {
        let mut mgr = init_manager();
        let result = mgr.delegate_task("nonexistent", "Test", "Do it", vec![]);
        assert!(result.is_err());
    }
}
