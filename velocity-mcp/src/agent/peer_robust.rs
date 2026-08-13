//! Robustness layer for cross-device peer collaboration.
//!
//! Adds production-grade reliability features on top of the base peer system:
//!
//! - **Auto-reconnect**: Exponential backoff reconnection to dropped peers
//! - **Heartbeat**: Background keepalive to detect stale connections
//! - **Transfer resume**: Track received chunks so interrupted transfers can resume
//! - **Peer discovery**: UDP broadcast announcement on the local network
//! - **Connection health**: Aggregate health scoring for each peer link
//!
//! All features are opt-in and composable — use [`RobustPeerManager`] to wrap
//! the base [`PeerManager`] with these capabilities.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use super::peer_link::{now_secs, PeerIdentity, PeerManager, PeerMessageKind};

// ── Auto-Reconnect ──

/// Configuration for automatic reconnection to dropped peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconnectConfig {
    /// Whether auto-reconnect is enabled.
    pub enabled: bool,
    /// Initial delay before first retry (seconds).
    pub initial_delay_secs: u64,
    /// Maximum delay between retries (seconds).
    pub max_delay_secs: u64,
    /// Multiplier applied to delay after each failed attempt.
    pub backoff_multiplier: f64,
    /// Maximum number of retry attempts (0 = unlimited).
    pub max_attempts: u32,
    /// How long to wait before considering a peer permanently dead (seconds).
    pub peer_death_timeout_secs: u64,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            initial_delay_secs: 2,
            max_delay_secs: 300, // 5 minutes
            backoff_multiplier: 2.0,
            max_attempts: 0,               // unlimited
            peer_death_timeout_secs: 3600, // 1 hour
        }
    }
}

/// Tracks reconnection state for a single peer.
#[derive(Debug, Clone)]
pub struct ReconnectState {
    /// Peer ID this state belongs to.
    pub peer_id: String,
    /// Number of consecutive failed reconnect attempts.
    pub attempts: u32,
    /// Current backoff delay in seconds.
    pub current_delay_secs: u64,
    /// Timestamp of the last reconnect attempt.
    pub last_attempt: u64,
    /// Timestamp when the peer was first lost.
    pub first_lost: u64,
    /// Whether we should keep trying.
    pub active: bool,
}

impl ReconnectState {
    fn new(peer_id: &str) -> Self {
        let now = now_secs();
        Self {
            peer_id: peer_id.to_string(),
            attempts: 0,
            current_delay_secs: 0,
            last_attempt: now,
            first_lost: now,
            active: true,
        }
    }

    /// Record a failed attempt and compute the next retry delay.
    pub fn record_failure(&mut self, config: &ReconnectConfig) {
        self.attempts += 1;
        self.last_attempt = now_secs();

        if self.current_delay_secs == 0 {
            self.current_delay_secs = config.initial_delay_secs;
        } else {
            self.current_delay_secs =
                (self.current_delay_secs as f64 * config.backoff_multiplier) as u64;
        }

        if self.current_delay_secs > config.max_delay_secs {
            self.current_delay_secs = config.max_delay_secs;
        }

        // Check if we've exceeded max attempts or death timeout.
        if config.max_attempts > 0 && self.attempts >= config.max_attempts {
            self.active = false;
        }
        if now_secs() - self.first_lost > config.peer_death_timeout_secs {
            self.active = false;
        }
    }

    /// Record a successful reconnection.
    pub fn record_success(&mut self) {
        self.attempts = 0;
        self.current_delay_secs = 0;
        self.active = false;
    }

    /// Whether it's time to retry.
    pub fn is_due(&self) -> bool {
        self.active && now_secs() - self.last_attempt >= self.current_delay_secs
    }
}

// ── Heartbeat ──

/// Configuration for the heartbeat keepalive system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatConfig {
    /// Whether heartbeat is enabled.
    pub enabled: bool,
    /// How often to send heartbeats (seconds).
    pub interval_secs: u64,
    /// How long to wait without a heartbeat before marking a peer offline.
    pub timeout_secs: u64,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_secs: 30,
            timeout_secs: 120,
        }
    }
}

// ── Peer Discovery ──

/// Configuration for UDP broadcast peer discovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    /// Whether discovery is enabled.
    pub enabled: bool,
    /// UDP port for broadcast announcements.
    pub broadcast_port: u16,
    /// How often to broadcast presence (seconds).
    pub announce_interval_secs: u64,
    /// Broadcast address (default: 255.255.255.255).
    pub broadcast_addr: String,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            broadcast_port: 9190,
            announce_interval_secs: 15,
            broadcast_addr: "255.255.255.255".to_string(),
        }
    }
}

/// A discovered peer from a broadcast announcement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredPeer {
    /// Peer ID.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// IP address of the announcing peer.
    pub host: String,
    /// Peer API port.
    pub port: u16,
    /// Operating system / environment.
    pub environment: Option<String>,
    /// When this announcement was received.
    pub discovered_at: u64,
}

// ── Transfer Resume ──

/// Tracks which chunks have been received for resumable transfers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferCheckpoint {
    /// Transfer ID.
    pub transfer_id: String,
    /// Total expected chunks.
    pub total_chunks: u32,
    /// Set of chunk indices that have been received.
    pub received_chunks: Vec<u32>,
    /// File name being transferred.
    pub filename: String,
    /// Total file size.
    pub total_size: u64,
    /// SHA-256 hash for verification.
    pub sha256: String,
    /// Peer sending the file.
    pub peer_id: String,
    /// Instructions for deployment.
    pub instructions: Option<String>,
    /// When the transfer started.
    pub started_at: u64,
}

impl TransferCheckpoint {
    /// Check if all chunks have been received.
    pub fn is_complete(&self) -> bool {
        self.received_chunks.len() as u32 >= self.total_chunks
    }

    /// Get the list of missing chunk indices.
    pub fn missing_chunks(&self) -> Vec<u32> {
        let received: std::collections::HashSet<u32> =
            self.received_chunks.iter().copied().collect();
        (0..self.total_chunks)
            .filter(|i| !received.contains(i))
            .collect()
    }

    /// Record a received chunk (idempotent).
    pub fn record_chunk(&mut self, index: u32) {
        if !self.received_chunks.contains(&index) {
            self.received_chunks.push(index);
        }
    }

    /// Progress as a percentage.
    pub fn progress_percent(&self) -> f32 {
        if self.total_chunks == 0 {
            return 0.0;
        }
        (self.received_chunks.len() as f32 / self.total_chunks as f32) * 100.0
    }
}

// ── Connection Health ──

/// Aggregate health score for a peer connection.
#[derive(Debug, Clone)]
pub struct PeerHealth {
    /// Peer ID.
    pub peer_id: String,
    /// Round-trip time of the last successful heartbeat (ms).
    pub last_rtt_ms: Option<u64>,
    /// Number of consecutive missed heartbeats.
    pub missed_heartbeats: u32,
    /// Total messages exchanged.
    pub total_messages: u64,
    /// Total bytes transferred.
    pub total_bytes: u64,
    /// Number of failed transfers.
    pub failed_transfers: u32,
    /// Number of successful transfers.
    pub successful_transfers: u32,
    /// Health score 0.0 (dead) to 1.0 (excellent).
    pub score: f32,
    /// Last updated timestamp.
    pub updated_at: u64,
}

impl PeerHealth {
    fn new(peer_id: &str) -> Self {
        Self {
            peer_id: peer_id.to_string(),
            last_rtt_ms: None,
            missed_heartbeats: 0,
            total_messages: 0,
            total_bytes: 0,
            failed_transfers: 0,
            successful_transfers: 0,
            score: 1.0,
            updated_at: now_secs(),
        }
    }

    /// Recompute the health score based on current metrics.
    pub fn recompute_score(&mut self) {
        let mut score = 1.0_f32;

        // Penalize missed heartbeats heavily.
        score -= self.missed_heartbeats as f32 * 0.15;

        // Penalize high RTT.
        if let Some(rtt) = self.last_rtt_ms {
            if rtt > 5000 {
                score -= 0.3;
            } else if rtt > 1000 {
                score -= 0.1;
            }
        }

        // Penalize failed transfers.
        let total_transfers = self.failed_transfers + self.successful_transfers;
        if total_transfers > 0 {
            let fail_rate = self.failed_transfers as f32 / total_transfers as f32;
            score -= fail_rate * 0.3;
        }

        self.score = score.clamp(0.0, 1.0);
        self.updated_at = now_secs();
    }

    /// Record a successful heartbeat round-trip.
    pub fn record_heartbeat(&mut self, rtt_ms: u64) {
        self.last_rtt_ms = Some(rtt_ms);
        self.missed_heartbeats = 0;
        self.recompute_score();
    }

    /// Record a missed heartbeat.
    pub fn record_missed_heartbeat(&mut self) {
        self.missed_heartbeats += 1;
        self.recompute_score();
    }

    /// Record a completed transfer.
    pub fn record_transfer(&mut self, bytes: u64, success: bool) {
        self.total_bytes += bytes;
        if success {
            self.successful_transfers += 1;
        } else {
            self.failed_transfers += 1;
        }
        self.recompute_score();
    }

    /// Human-readable health label.
    pub fn label(&self) -> &'static str {
        if self.score >= 0.8 {
            "excellent"
        } else if self.score >= 0.6 {
            "good"
        } else if self.score >= 0.4 {
            "fair"
        } else if self.score >= 0.2 {
            "poor"
        } else {
            "dead"
        }
    }
}

// ── Robust Peer Manager ──

/// Wraps [`PeerManager`] with reliability features.
pub struct RobustPeerManager {
    /// The underlying peer manager.
    pub inner: PeerManager,
    /// Reconnect configuration.
    pub reconnect_config: ReconnectConfig,
    /// Heartbeat configuration.
    pub heartbeat_config: HeartbeatConfig,
    /// Discovery configuration.
    pub discovery_config: DiscoveryConfig,
    /// Reconnection state per peer.
    pub reconnect_states: HashMap<String, ReconnectState>,
    /// Transfer checkpoints for resumable transfers.
    pub checkpoints: HashMap<String, TransferCheckpoint>,
    /// Connection health per peer.
    pub health: HashMap<String, PeerHealth>,
    /// Recently discovered peers from broadcast.
    pub discovered: Vec<DiscoveredPeer>,
    /// Whether the heartbeat loop is running.
    pub heartbeat_running: Arc<AtomicBool>,
    /// Whether the discovery listener is running.
    pub discovery_running: Arc<AtomicBool>,
}

impl RobustPeerManager {
    /// Create a new robust peer manager wrapping the given base manager.
    pub fn new(inner: PeerManager) -> Self {
        Self {
            inner,
            reconnect_config: ReconnectConfig::default(),
            heartbeat_config: HeartbeatConfig::default(),
            discovery_config: DiscoveryConfig::default(),
            reconnect_states: HashMap::new(),
            checkpoints: HashMap::new(),
            health: HashMap::new(),
            discovered: Vec::new(),
            heartbeat_running: Arc::new(AtomicBool::new(false)),
            discovery_running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create with default initialization.
    pub fn with_workspace(workspace_root: &Path, name: &str) -> Self {
        let mut mgr = PeerManager::load(workspace_root);
        if mgr.local_identity.is_none() {
            mgr.init(workspace_root, name);
        }
        Self::new(mgr)
    }

    // ── Reconnect ──

    /// Mark a peer as lost and begin reconnection attempts.
    pub fn mark_peer_lost(&mut self, peer_id: &str) {
        if !self.reconnect_states.contains_key(peer_id) {
            self.reconnect_states
                .insert(peer_id.to_string(), ReconnectState::new(peer_id));
        }
        if let Some(peer) = self.inner.peers.get_mut(peer_id) {
            peer.online = false;
        }
    }

    /// Process pending reconnection attempts.
    /// Returns the list of peer IDs that were successfully reconnected.
    pub fn tick_reconnect(&mut self) -> Vec<String> {
        if !self.reconnect_config.enabled {
            return Vec::new();
        }

        let mut reconnected = Vec::new();
        let due_peers: Vec<String> = self
            .reconnect_states
            .values()
            .filter(|s| s.active && s.is_due())
            .map(|s| s.peer_id.clone())
            .collect();

        for peer_id in due_peers {
            let peer = match self.inner.peers.get(&peer_id) {
                Some(p) => p.clone(),
                None => continue,
            };

            // Attempt a health check as a reconnect probe.
            match super::peer_server::peer_health_check(&peer.host, peer.port) {
                Ok(_) => {
                    // Peer is back online!
                    if let Some(state) = self.reconnect_states.get_mut(&peer_id) {
                        state.record_success();
                    }
                    if let Some(p) = self.inner.peers.get_mut(&peer_id) {
                        p.online = true;
                        p.last_seen = now_secs();
                    }
                    if let Some(h) = self.health.get_mut(&peer_id) {
                        h.missed_heartbeats = 0;
                        h.recompute_score();
                    }
                    reconnected.push(peer_id);
                }
                Err(_) => {
                    // Still unreachable — record failure.
                    if let Some(state) = self.reconnect_states.get_mut(&peer_id) {
                        state.record_failure(&self.reconnect_config);
                        if !state.active {
                            // Peer is considered permanently dead.
                            if let Some(p) = self.inner.peers.get_mut(&peer_id) {
                                p.online = false;
                            }
                        }
                    }
                }
            }
        }

        reconnected
    }

    // ── Heartbeat ──

    /// Send heartbeats to all online peers and update health.
    pub fn tick_heartbeat(&mut self) {
        if !self.heartbeat_config.enabled {
            return;
        }

        let online_peers: Vec<PeerIdentity> =
            self.inner.online_peers().into_iter().cloned().collect();

        for peer in &online_peers {
            let start = now_secs();
            match super::peer_server::peer_health_check(&peer.host, peer.port) {
                Ok(_) => {
                    let rtt = (now_secs() - start) * 1000; // rough ms
                    let health = self
                        .health
                        .entry(peer.id.clone())
                        .or_insert_with(|| PeerHealth::new(&peer.id));
                    health.record_heartbeat(rtt);
                    if let Some(p) = self.inner.peers.get_mut(&peer.id) {
                        p.last_seen = now_secs();
                    }
                }
                Err(_) => {
                    let health = self
                        .health
                        .entry(peer.id.clone())
                        .or_insert_with(|| PeerHealth::new(&peer.id));
                    health.record_missed_heartbeat();
                    if health.missed_heartbeats >= 3 {
                        self.mark_peer_lost(&peer.id);
                    }
                }
            }
        }
    }

    // ── Discovery ──

    /// Broadcast our presence on the local network via UDP.
    pub fn broadcast_presence(&self) -> Result<(), String> {
        if !self.discovery_config.enabled {
            return Ok(());
        }

        let identity = self
            .inner
            .local_identity
            .as_ref()
            .ok_or_else(|| "Not initialized".to_string())?;

        let announcement = serde_json::json!({
            "id": identity.id,
            "name": identity.name,
            "port": identity.port,
            "env": identity.environment,
        });

        let payload =
            serde_json::to_string(&announcement).map_err(|e| format!("Serialize: {e}"))?;

        let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("Bind: {e}"))?;
        socket
            .set_broadcast(true)
            .map_err(|e| format!("Set broadcast: {e}"))?;

        let addr: SocketAddr = format!(
            "{}:{}",
            self.discovery_config.broadcast_addr, self.discovery_config.broadcast_port
        )
        .parse()
        .map_err(|e| format!("Parse addr: {e}"))?;

        socket
            .send_to(payload.as_bytes(), addr)
            .map_err(|e| format!("Send: {e}"))?;

        Ok(())
    }

    /// Listen for peer broadcast announcements (blocking — call from a thread).
    pub fn listen_for_discovery(&mut self, running: Arc<AtomicBool>) -> Result<(), String> {
        let port = self.discovery_config.broadcast_port;
        let socket = UdpSocket::bind(format!("0.0.0.0:{}", port))
            .map_err(|e| format!("Bind discovery: {e}"))?;
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .map_err(|e| format!("Timeout: {e}"))?;

        let mut buf = [0u8; 1024];

        while running.load(Ordering::SeqCst) {
            match socket.recv_from(&mut buf) {
                Ok((len, addr)) => {
                    let data = String::from_utf8_lossy(&buf[..len]);
                    if let Ok(announcement) = serde_json::from_str::<serde_json::Value>(&data) {
                        let id = announcement
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        // Don't discover ourselves.
                        if let Some(local) = &self.inner.local_identity {
                            if id == local.id {
                                continue;
                            }
                        }

                        let discovered = DiscoveredPeer {
                            id: id.clone(),
                            name: announcement
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
                                .to_string(),
                            host: addr.ip().to_string(),
                            port: announcement
                                .get("port")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(9191) as u16,
                            environment: announcement
                                .get("env")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                            discovered_at: now_secs(),
                        };

                        // Update or add to discovered list.
                        if let Some(existing) = self.discovered.iter_mut().find(|d| d.id == id) {
                            *existing = discovered;
                        } else {
                            self.discovered.push(discovered);
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(_) => continue,
            }
        }

        Ok(())
    }

    /// Promote a discovered peer to a connected peer.
    pub fn connect_discovered(&mut self, discovered_id: &str) -> Option<String> {
        let disc = self
            .discovered
            .iter()
            .find(|d| d.id == discovered_id)
            .cloned()?;

        let peer = PeerIdentity {
            id: disc.id.clone(),
            name: disc.name.clone(),
            host: disc.host.clone(),
            port: disc.port,
            auth_secret_handle: None,
            first_seen: now_secs(),
            last_seen: now_secs(),
            online: true,
            capabilities: vec![super::peer_link::PeerCapability::General],
            environment: disc.environment.clone(),
        };

        self.inner.add_peer(peer);
        Some(disc.id)
    }

    // ── Transfer Resume ──

    /// Create a checkpoint for a resumable transfer.
    pub fn create_checkpoint(
        &mut self,
        transfer_id: &str,
        peer_id: &str,
        filename: &str,
        total_size: u64,
        total_chunks: u32,
        sha256: &str,
        instructions: Option<&str>,
    ) {
        let checkpoint = TransferCheckpoint {
            transfer_id: transfer_id.to_string(),
            total_chunks,
            received_chunks: Vec::new(),
            filename: filename.to_string(),
            total_size,
            sha256: sha256.to_string(),
            peer_id: peer_id.to_string(),
            instructions: instructions.map(|s| s.to_string()),
            started_at: now_secs(),
        };
        self.checkpoints.insert(transfer_id.to_string(), checkpoint);
    }

    /// Record a received chunk in the checkpoint.
    pub fn record_chunk_received(&mut self, transfer_id: &str, chunk_index: u32) -> bool {
        if let Some(cp) = self.checkpoints.get_mut(transfer_id) {
            cp.record_chunk(chunk_index);
            true
        } else {
            false
        }
    }

    /// Get missing chunks for a transfer (for resume).
    pub fn missing_chunks(&self, transfer_id: &str) -> Vec<u32> {
        self.checkpoints
            .get(transfer_id)
            .map(|cp| cp.missing_chunks())
            .unwrap_or_default()
    }

    /// Check if a transfer checkpoint is complete.
    pub fn is_transfer_complete(&self, transfer_id: &str) -> bool {
        self.checkpoints
            .get(transfer_id)
            .map(|cp| cp.is_complete())
            .unwrap_or(false)
    }

    /// Remove a completed checkpoint.
    pub fn remove_checkpoint(&mut self, transfer_id: &str) -> Option<TransferCheckpoint> {
        self.checkpoints.remove(transfer_id)
    }

    // ── Health ──

    /// Get the health score for a peer.
    pub fn peer_health(&self, peer_id: &str) -> Option<&PeerHealth> {
        self.health.get(peer_id)
    }

    /// Get all peer health scores.
    pub fn all_health(&self) -> Vec<&PeerHealth> {
        self.health.values().collect()
    }

    /// Record a message exchange for health tracking.
    pub fn record_message(&mut self, peer_id: &str, bytes: u64) {
        let health = self
            .health
            .entry(peer_id.to_string())
            .or_insert_with(|| PeerHealth::new(peer_id));
        health.total_messages += 1;
        health.total_bytes += bytes;
    }

    // ── Lifecycle ──

    /// Run one tick of all robustness features.
    pub fn tick(&mut self) -> Vec<String> {
        let reconnected = self.tick_reconnect();
        self.tick_heartbeat();
        reconnected
    }

    /// Persist state to disk.
    pub fn save(&self) -> Result<(), String> {
        self.inner.save()
    }

    /// Load state from disk.
    pub fn load(workspace_root: &Path) -> Self {
        let mgr = PeerManager::load(workspace_root);
        Self::new(mgr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_robust() -> RobustPeerManager {
        let mut inner = PeerManager::new();
        inner.init(Path::new("/tmp"), "TestHost");
        RobustPeerManager::new(inner)
    }

    fn add_test_peer(robust: &mut RobustPeerManager, id: &str, host: &str) {
        let peer = PeerIdentity {
            id: id.to_string(),
            name: format!("Peer-{}", id),
            host: host.to_string(),
            port: 9191,
            auth_secret_handle: None,
            first_seen: now_secs(),
            last_seen: now_secs(),
            online: true,
            capabilities: vec![],
            environment: None,
        };
        robust.inner.add_peer(peer);
        robust.health.insert(id.to_string(), PeerHealth::new(id));
    }

    #[test]
    fn reconnect_state_backoff() {
        let config = ReconnectConfig::default();
        let mut state = ReconnectState::new("p1");

        state.record_failure(&config);
        assert_eq!(state.attempts, 1);
        assert_eq!(state.current_delay_secs, 2); // initial

        state.record_failure(&config);
        assert_eq!(state.attempts, 2);
        assert_eq!(state.current_delay_secs, 4); // 2 * 2.0

        state.record_failure(&config);
        assert_eq!(state.attempts, 3);
        assert_eq!(state.current_delay_secs, 8); // 4 * 2.0

        state.record_success();
        assert_eq!(state.attempts, 0);
        assert!(!state.active);
    }

    #[test]
    fn reconnect_state_max_delay_cap() {
        let config = ReconnectConfig {
            max_delay_secs: 10,
            ..Default::default()
        };
        let mut state = ReconnectState::new("p1");

        for _ in 0..20 {
            state.record_failure(&config);
        }
        assert!(state.current_delay_secs <= 10);
    }

    #[test]
    fn reconnect_state_max_attempts() {
        let config = ReconnectConfig {
            max_attempts: 3,
            ..Default::default()
        };
        let mut state = ReconnectState::new("p1");

        state.record_failure(&config);
        assert!(state.active);
        state.record_failure(&config);
        assert!(state.active);
        state.record_failure(&config);
        assert!(!state.active); // 3 attempts reached
    }

    #[test]
    fn reconnect_is_due() {
        let mut state = ReconnectState::new("p1");
        let config = ReconnectConfig::default();

        // Initially due (delay is 0).
        assert!(state.is_due());

        state.record_failure(&config);
        // Not due immediately after failure (delay is 2s).
        assert!(!state.is_due());

        // Simulate time passing.
        state.last_attempt = now_secs() - 10;
        assert!(state.is_due());
    }

    #[test]
    fn health_score_excellent() {
        let mut health = PeerHealth::new("p1");
        health.record_heartbeat(50);
        assert_eq!(health.score, 1.0);
        assert_eq!(health.label(), "excellent");
    }

    #[test]
    fn health_score_degrades_with_missed_heartbeats() {
        let mut health = PeerHealth::new("p1");
        for _ in 0..3 {
            health.record_missed_heartbeat();
        }
        assert!(health.score < 1.0);
        assert_eq!(health.missed_heartbeats, 3);
    }

    #[test]
    fn health_score_degrades_with_failed_transfers() {
        let mut health = PeerHealth::new("p1");
        health.record_transfer(1000, true);
        health.record_transfer(1000, false);
        health.record_transfer(1000, false);
        assert!(health.score < 1.0);
        assert_eq!(health.successful_transfers, 1);
        assert_eq!(health.failed_transfers, 2);
    }

    #[test]
    fn health_labels() {
        let mut h = PeerHealth::new("p1");
        h.score = 0.9;
        assert_eq!(h.label(), "excellent");
        h.score = 0.7;
        assert_eq!(h.label(), "good");
        h.score = 0.5;
        assert_eq!(h.label(), "fair");
        h.score = 0.3;
        assert_eq!(h.label(), "poor");
        h.score = 0.1;
        assert_eq!(h.label(), "dead");
    }

    #[test]
    fn transfer_checkpoint_basic() {
        let mut cp = TransferCheckpoint {
            transfer_id: "xfer_1".into(),
            total_chunks: 5,
            received_chunks: Vec::new(),
            filename: "test.bin".into(),
            total_size: 5000,
            sha256: "abc".into(),
            peer_id: "p1".into(),
            instructions: None,
            started_at: now_secs(),
        };

        assert!(!cp.is_complete());
        assert_eq!(cp.missing_chunks(), vec![0, 1, 2, 3, 4]);
        assert_eq!(cp.progress_percent(), 0.0);

        cp.record_chunk(0);
        cp.record_chunk(2);
        cp.record_chunk(4);
        assert!(!cp.is_complete());
        assert_eq!(cp.missing_chunks(), vec![1, 3]);
        assert!((cp.progress_percent() - 60.0).abs() < 0.1);

        cp.record_chunk(1);
        cp.record_chunk(3);
        assert!(cp.is_complete());
        assert_eq!(cp.missing_chunks(), Vec::<u32>::new());
    }

    #[test]
    fn transfer_checkpoint_idempotent() {
        let mut cp = TransferCheckpoint {
            transfer_id: "xfer_2".into(),
            total_chunks: 3,
            received_chunks: Vec::new(),
            filename: "data.bin".into(),
            total_size: 3000,
            sha256: "def".into(),
            peer_id: "p1".into(),
            instructions: None,
            started_at: now_secs(),
        };

        cp.record_chunk(0);
        cp.record_chunk(0); // duplicate
        cp.record_chunk(0); // duplicate
        assert_eq!(cp.received_chunks.len(), 1);
    }

    #[test]
    fn mark_peer_lost_starts_reconnect() {
        let mut robust = make_robust();
        add_test_peer(&mut robust, "p1", "10.0.0.1");

        robust.mark_peer_lost("p1");
        assert!(!robust.inner.peers["p1"].online);
        assert!(robust.reconnect_states.contains_key("p1"));
        assert!(robust.reconnect_states["p1"].active);
    }

    #[test]
    fn checkpoint_management() {
        let mut robust = make_robust();

        robust.create_checkpoint(
            "xfer_1",
            "p1",
            "app.exe",
            3000,
            3,
            "hash1",
            Some("run {file}"),
        );
        assert!(robust.checkpoints.contains_key("xfer_1"));
        assert!(!robust.is_transfer_complete("xfer_1"));

        robust.record_chunk_received("xfer_1", 0);
        robust.record_chunk_received("xfer_1", 1);
        assert_eq!(robust.missing_chunks("xfer_1"), vec![2]);

        robust.record_chunk_received("xfer_1", 2);
        assert!(robust.is_transfer_complete("xfer_1"));

        let cp = robust.remove_checkpoint("xfer_1").unwrap();
        assert_eq!(cp.filename, "app.exe");
        assert!(!robust.checkpoints.contains_key("xfer_1"));
    }

    #[test]
    fn health_tracking() {
        let mut robust = make_robust();
        add_test_peer(&mut robust, "p1", "10.0.0.1");

        robust.record_message("p1", 500);
        robust.record_message("p1", 300);

        let h = robust.peer_health("p1").unwrap();
        assert_eq!(h.total_messages, 2);
        assert_eq!(h.total_bytes, 800);
    }

    #[test]
    fn broadcast_presence_requires_init() {
        let inner = PeerManager::new(); // not initialized
        let robust = RobustPeerManager::new(inner);
        assert!(robust.broadcast_presence().is_err());
    }

    #[test]
    fn discovered_peer_promotion() {
        let mut robust = make_robust();

        robust.discovered.push(DiscoveredPeer {
            id: "disc_1".into(),
            name: "Discovered PC".into(),
            host: "192.168.1.50".into(),
            port: 9191,
            environment: Some("linux".into()),
            discovered_at: now_secs(),
        });

        let result = robust.connect_discovered("disc_1");
        assert!(result.is_some());
        assert!(robust.inner.peers.contains_key("disc_1"));
        assert_eq!(robust.inner.peers["disc_1"].host, "192.168.1.50");
    }

    #[test]
    fn discovered_peer_unknown_returns_none() {
        let mut robust = make_robust();
        let result = robust.connect_discovered("nonexistent");
        assert!(result.is_none());
    }
}
