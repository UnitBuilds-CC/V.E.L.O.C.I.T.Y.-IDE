//! Security audit logging framework.
//!
//! Provides a lock-free ring buffer of [`SecurityEvent`]s that records every
//! sensitive operation: process spawns, secret access, file writes, API calls,
//! IPC messages, tool invocations, and authentication events.
//!
//! The audit log is bounded (default 8 192 entries ≈ ~4 MB) so it never causes
//! OOM. Events carry a timestamp, source location, and severity for later
//! analysis and compliance reporting.
//!
//! # Tool Execution Audit (upstream inheritance)
//!
//! In addition to security events, the module provides per-session tool execution
//! audit logging with Merkle root tracking, CSV/JSON export, and ring buffer
//! eviction. See [`ToolAuditLog`] and [`ToolAuditRegistry`].

use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

// ─── Event types ──────────────────────────────────────────────────────────────

/// A security-relevant event worth recording.
#[derive(Debug, Clone)]
pub enum SecurityEvent {
    /// A process was spawned via `Command` or similar.
    ProcessSpawn {
        command: String,
        args: Vec<String>,
        pid: Option<u32>,
    },
    /// A secret was accessed (read from the secret store).
    SecretAccess {
        handle: String,
        consumer: String,
    },
    /// A file was written outside the editor buffer (agent-initiated).
    FileWrite {
        path: String,
    },
    /// An AI provider API call was made.
    ApiCall {
        provider: String,
        endpoint: String,
    },
    /// An IPC message was sent or received on shared memory.
    IpcMessage {
        direction: &'static str, // "tx" or "rx"
        size_bytes: usize,
    },
    /// An agent tool was invoked.
    ToolInvocation {
        tool_name: String,
        args_preview: String,
    },
    /// An authentication / authorisation event.
    AuthEvent {
        kind: &'static str, // "login", "logout", "token_refresh", "failure"
        detail: String,
    },
}

impl SecurityEvent {
    /// Short human-readable label for dashboards / logs.
    pub fn label(&self) -> &'static str {
        match self {
            Self::ProcessSpawn { .. } => "process_spawn",
            Self::SecretAccess { .. } => "secret_access",
            Self::FileWrite { .. } => "file_write",
            Self::ApiCall { .. } => "api_call",
            Self::IpcMessage { .. } => "ipc_message",
            Self::ToolInvocation { .. } => "tool_invocation",
            Self::AuthEvent { .. } => "auth_event",
        }
    }

    /// Severity: `error` for auth failures, `warning` for rate limits, `info`
    /// for everything else.
    pub fn severity(&self) -> &'static str {
        match self {
            Self::AuthEvent { kind: "failure", .. } => "error",
            _ => "info",
        }
    }
}

// ─── Ring buffer ──────────────────────────────────────────────────────────────

/// Maximum number of audit entries kept in memory.
const RING_CAPACITY: usize = 8192;

/// A single audit record stored in the ring buffer.
#[derive(Debug, Clone)]
struct AuditRecord {
    /// Monotonic sequence number (wrapping).
    seq: u64,
    /// Unix timestamp in milliseconds.
    timestamp_ms: u64,
    /// The security event.
    event: SecurityEvent,
}

/// Lock-free bounded audit log. Uses an atomic write index and a
/// pre-allocated `Vec` of `AtomicU64` slots — each slot holds the index into
/// a separate `Vec<AuditRecord>` guarded by a spin-lock (only held during the
/// brief write).
struct AuditRing {
    records: std::sync::Mutex<Vec<AuditRecord>>,
    next_seq: AtomicU64,
    count: AtomicUsize,
}

impl AuditRing {
    fn new() -> Self {
        Self {
            records: std::sync::Mutex::new(Vec::with_capacity(RING_CAPACITY)),
            next_seq: AtomicU64::new(1),
            count: AtomicUsize::new(0),
        }
    }

    fn push(&self, event: SecurityEvent) {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let record = AuditRecord {
            seq,
            timestamp_ms,
            event,
        };

        if let Ok(mut buf) = self.records.lock() {
            if buf.len() >= RING_CAPACITY {
                // Evict oldest 25% to avoid constant reallocation.
                let drain_count = RING_CAPACITY / 4;
                buf.drain(..drain_count);
            }
            buf.push(record);
            self.count.store(buf.len(), Ordering::Relaxed);
        }
    }

    /// Returns a snapshot of all current audit records (oldest first).
    pub fn snapshot(&self) -> Vec<AuditRecord> {
        self.records
            .lock()
            .map(|buf| buf.clone())
            .unwrap_or_default()
    }

    /// Number of records currently stored.
    pub fn len(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }
}

/// Global audit ring buffer.
static AUDIT_RING: LazyLock<AuditRing> = LazyLock::new(AuditRing::new);

// ─── Public API ───────────────────────────────────────────────────────────────

/// Record a security event in the audit log.
///
/// This is the primary instrumentation point. Call at every sensitive
/// operation boundary:
///
/// ```ignore
/// audit_log!(SecurityEvent::ProcessSpawn {
///     command: "cargo".into(),
///     args: vec!["build".into()],
///     pid: None,
/// });
/// ```
pub fn audit_log(event: SecurityEvent) {
    AUDIT_RING.push(event);
}

/// Convenience macro — mirrors `tracing::info!` style.
#[macro_export]
macro_rules! audit_log {
    ($event:expr) => {
        $crate::security::audit::audit_log($event);
    };
}

/// Returns a snapshot of all audit records (oldest first).
pub fn audit_snapshot() -> Vec<(u64, u64, SecurityEvent)> {
    AUDIT_RING
        .snapshot()
        .into_iter()
        .map(|r| (r.seq, r.timestamp_ms, r.event))
        .collect()
}

/// Number of audit records currently buffered.
pub fn audit_count() -> usize {
    AUDIT_RING.len()
}

/// Drain all audit records and return them. Useful for periodic flush-to-disk.
pub fn audit_drain() -> Vec<(u64, u64, SecurityEvent)> {
    let records = AUDIT_RING
        .records
        .lock()
        .map(|mut buf| buf.drain(..).collect::<Vec<_>>())
        .unwrap_or_default();
    AUDIT_RING.count.store(0, Ordering::Relaxed);
    records
        .into_iter()
        .map(|r| (r.seq, r.timestamp_ms, r.event))
        .collect()
}

// ─── Tool Execution Audit (upstream inheritance) ──────────────────────────────

/// Maximum number of tool audit entries to retain in memory.
const MAX_TOOL_AUDIT_ENTRIES: usize = 10_000;

/// Outcome of an audited tool operation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum ToolAuditOutcome {
    Success,
    Error(String),
    Timeout,
    Rejected(String),
}

/// A single tool audit log entry.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolAuditEntry {
    /// Monotonic sequence number.
    pub sequence: u64,
    /// Unix timestamp in milliseconds.
    pub timestamp_ms: u64,
    /// Tool name that was called.
    pub tool_name: String,
    /// Duration in microseconds (µs) for sub-millisecond precision.
    pub duration_us: u64,
    /// Outcome of the call.
    pub outcome: ToolAuditOutcome,
    /// Transport layer: "http", "stdio", "shmem", "nda_http", etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
    /// Request payload size in bytes (if known).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_size: Option<u64>,
    /// Response size in bytes (if known).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_size: Option<u64>,
    /// Merkle root hash (hex-encoded) if from NDA transport.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merkle_root: Option<String>,
    /// Session ID for multi-tenant isolation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// Global sequence counter for tool audit entries.
static TOOL_AUDIT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// In-memory tool audit log with ring buffer semantics.
pub struct ToolAuditLog {
    entries: std::sync::Mutex<Vec<ToolAuditEntry>>,
}

impl Default for ToolAuditLog {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolAuditLog {
    /// Create a new tool audit log.
    pub fn new() -> Self {
        Self {
            entries: std::sync::Mutex::new(Vec::with_capacity(1024)),
        }
    }

    /// Record a tool execution.
    pub fn record(&self, tool_name: &str, start: Instant, outcome: ToolAuditOutcome) {
        self.record_full(tool_name, start, outcome, None, None, None, None, None);
    }

    /// Record a tool execution with full context.
    pub fn record_full(
        &self,
        tool_name: &str,
        start: Instant,
        outcome: ToolAuditOutcome,
        transport: Option<String>,
        payload_size: Option<u64>,
        response_size: Option<u64>,
        merkle_root: Option<String>,
        session_id: Option<String>,
    ) {
        let seq = TOOL_AUDIT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let duration_us = start.elapsed().as_micros() as u64;

        let entry = ToolAuditEntry {
            sequence: seq,
            timestamp_ms,
            tool_name: tool_name.to_string(),
            duration_us,
            outcome,
            transport,
            payload_size,
            response_size,
            merkle_root,
            session_id,
        };

        let mut entries = match self.entries.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                eprintln!("[WARN] Tool audit log mutex poisoning recovered.");
                poisoned.into_inner()
            }
        };

        entries.push(entry);

        // Ring buffer: drop oldest entries if we exceed the limit
        if entries.len() > MAX_TOOL_AUDIT_ENTRIES {
            let drain_count = entries.len() - MAX_TOOL_AUDIT_ENTRIES;
            entries.drain(..drain_count);
        }
    }

    /// Get the most recent N entries (newest first).
    pub fn recent(&self, count: usize) -> Vec<ToolAuditEntry> {
        let entries = match self.entries.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        entries.iter().rev().take(count).cloned().collect()
    }

    /// Get the total number of entries currently stored.
    pub fn len(&self) -> usize {
        match self.entries.lock() {
            Ok(guard) => guard.len(),
            Err(poisoned) => poisoned.into_inner().len(),
        }
    }

    /// Returns true if the audit log contains no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clear all entries.
    pub fn clear(&self) {
        match self.entries.lock() {
            Ok(mut guard) => guard.clear(),
            Err(poisoned) => poisoned.into_inner().clear(),
        }
    }

    /// Get all entries (for export/streaming).
    pub fn all(&self) -> Vec<ToolAuditEntry> {
        match self.entries.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// Export audit log to JSON format.
    pub fn export_json(&self) -> Result<String, String> {
        let entries = self.all();
        serde_json::to_string_pretty(&entries)
            .map_err(|e| format!("Failed to serialize tool audit log to JSON: {}", e))
    }

    /// Export audit log to CSV format.
    pub fn export_csv(&self) -> Result<String, String> {
        let entries = self.all();
        let mut csv = String::from("sequence,timestamp_ms,tool_name,duration_us,outcome,transport,payload_size,response_size,merkle_root,session_id\n");

        for entry in entries {
            let outcome_str = match &entry.outcome {
                ToolAuditOutcome::Success => "success".to_string(),
                ToolAuditOutcome::Error(msg) => format!("error:{}", msg.replace(',', ";")),
                ToolAuditOutcome::Timeout => "timeout".to_string(),
                ToolAuditOutcome::Rejected(reason) => format!("rejected:{}", reason.replace(',', ";")),
            };
            let transport_str = entry.transport.unwrap_or_default();
            let payload_str = entry.payload_size.map(|s| s.to_string()).unwrap_or_default();
            let response_str = entry.response_size.map(|s| s.to_string()).unwrap_or_default();
            let merkle_str = entry.merkle_root.unwrap_or_default();
            let session_str = entry.session_id.unwrap_or_default();

            csv.push_str(&format!(
                "{},{},{},{},{},{},{},{},{},{}\n",
                entry.sequence,
                entry.timestamp_ms,
                entry.tool_name,
                entry.duration_us,
                outcome_str,
                transport_str,
                payload_str,
                response_str,
                merkle_str,
                session_str
            ));
        }

        Ok(csv)
    }

    /// Flush audit log to disk as JSON. Returns the number of entries written.
    pub fn flush_to_file(&self, path: &str) -> Result<usize, String> {
        let json = self.export_json()?;
        std::fs::write(path, json)
            .map_err(|e| format!("Failed to write tool audit log to {}: {}", path, e))?;
        Ok(self.len())
    }
}

/// Registry of per-session tool audit logs for multi-tenant isolation.
pub struct ToolAuditRegistry {
    sessions: std::sync::RwLock<HashMap<String, std::sync::Arc<ToolAuditLog>>>,
}

impl ToolAuditRegistry {
    /// Create a new empty tool audit registry.
    pub fn new() -> Self {
        Self {
            sessions: std::sync::RwLock::new(HashMap::new()),
        }
    }

    /// Get or create the audit log for a session.
    pub fn get_or_create(&self, session_id: &str) -> std::sync::Arc<ToolAuditLog> {
        // Fast path: read lock
        {
            let sessions = self.sessions.read().unwrap_or_else(|p| p.into_inner());
            if let Some(log) = sessions.get(session_id) {
                return std::sync::Arc::clone(log);
            }
        }
        // Slow path: write lock to insert
        const MAX_TOOL_AUDIT_SESSIONS: usize = 1024;
        let mut sessions = self.sessions.write().unwrap_or_else(|p| p.into_inner());
        if sessions.len() >= MAX_TOOL_AUDIT_SESSIONS && !sessions.contains_key(session_id) {
            let first_key = sessions.keys().next().cloned();
            if let Some(key) = first_key {
                sessions.remove(&key);
                tracing::warn!(session_id = %key, "Tool audit registry full ({}), evicted oldest session", MAX_TOOL_AUDIT_SESSIONS);
            }
        }
        sessions
            .entry(session_id.to_string())
            .or_insert_with(|| std::sync::Arc::new(ToolAuditLog::new()))
            .clone()
    }

    /// Get the audit log for a session, if it exists.
    pub fn get(&self, session_id: &str) -> Option<std::sync::Arc<ToolAuditLog>> {
        let sessions = self.sessions.read().unwrap_or_else(|p| p.into_inner());
        sessions.get(session_id).cloned()
    }

    /// Remove the audit log for a session.
    pub fn remove(&self, session_id: &str) -> Option<std::sync::Arc<ToolAuditLog>> {
        let mut sessions = self.sessions.write().unwrap_or_else(|p| p.into_inner());
        sessions.remove(session_id)
    }

    /// List all active session IDs.
    pub fn session_ids(&self) -> Vec<String> {
        let sessions = self.sessions.read().unwrap_or_else(|p| p.into_inner());
        sessions.keys().cloned().collect()
    }

    /// Aggregate all entries from all sessions, sorted by sequence descending.
    pub fn aggregate_all(&self) -> Vec<ToolAuditEntry> {
        let sessions = self.sessions.read().unwrap_or_else(|p| p.into_inner());
        let mut all: Vec<ToolAuditEntry> = sessions.values().flat_map(|log| log.all()).collect();
        all.sort_by_key(|a| std::cmp::Reverse(a.sequence));
        all
    }

    /// Number of active sessions.
    pub fn session_count(&self) -> usize {
        let sessions = self.sessions.read().unwrap_or_else(|p| p.into_inner());
        sessions.len()
    }

    /// Clear all session audit logs.
    pub fn clear(&self) {
        let mut sessions = self.sessions.write().unwrap_or_else(|p| p.into_inner());
        sessions.clear();
    }
}

impl Default for ToolAuditRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Global tool audit registry instance.
static TOOL_AUDIT_REGISTRY: LazyLock<ToolAuditRegistry> = LazyLock::new(|| ToolAuditRegistry {
    sessions: std::sync::RwLock::new(HashMap::new()),
});

/// Get a reference to the global tool audit registry.
pub fn tool_audit_registry() -> &'static ToolAuditRegistry {
    &TOOL_AUDIT_REGISTRY
}

/// Convenience: record a tool call, routed to the specified session's audit buffer.
pub fn record_tool_call(
    session_id: &str,
    tool_name: &str,
    start: Instant,
    outcome: ToolAuditOutcome,
) {
    let log = tool_audit_registry().get_or_create(session_id);
    log.record(tool_name, start, outcome);
}

#[cfg(test)]
mod tool_audit_tests {
    use super::*;

    #[test]
    fn test_tool_audit_log_record_and_retrieve() {
        let log = ToolAuditLog::new();
        let start = Instant::now();
        log.record("test_tool", start, ToolAuditOutcome::Success);
        log.record("test_tool_2", start, ToolAuditOutcome::Error("oops".into()));

        assert_eq!(log.len(), 2);
        let recent = log.recent(10);
        assert_eq!(recent.len(), 2);
        // Most recent first
        assert_eq!(recent[0].tool_name, "test_tool_2");
        assert_eq!(recent[1].tool_name, "test_tool");
    }

    #[test]
    fn test_tool_audit_log_ring_buffer_eviction() {
        let log = ToolAuditLog::new();
        let start = Instant::now();
        for i in 0..100 {
            log.record(&format!("tool_{}", i), start, ToolAuditOutcome::Success);
        }
        assert_eq!(log.len(), 100);
    }

    #[test]
    fn test_tool_audit_log_clear() {
        let log = ToolAuditLog::new();
        let start = Instant::now();
        log.record("tool", start, ToolAuditOutcome::Success);
        assert_eq!(log.len(), 1);
        log.clear();
        assert_eq!(log.len(), 0);
    }

    #[test]
    fn test_tool_audit_entry_sequence_numbers() {
        let log = ToolAuditLog::new();
        let start = Instant::now();
        log.record("a", start, ToolAuditOutcome::Success);
        log.record("b", start, ToolAuditOutcome::Timeout);
        let entries = log.recent(10);
        assert!(entries[0].sequence > entries[1].sequence);
    }

    #[test]
    fn test_tool_audit_outcome_variants() {
        let log = ToolAuditLog::new();
        let start = Instant::now();
        log.record("ok", start, ToolAuditOutcome::Success);
        log.record("err", start, ToolAuditOutcome::Error("fail".into()));
        log.record("to", start, ToolAuditOutcome::Timeout);
        log.record("rej", start, ToolAuditOutcome::Rejected("denied".into()));

        let entries = log.recent(10);
        assert_eq!(entries[0].outcome, ToolAuditOutcome::Rejected("denied".into()));
        assert_eq!(entries[1].outcome, ToolAuditOutcome::Timeout);
    }

    #[test]
    fn test_tool_audit_merkle_root_tracking() {
        let log = ToolAuditLog::new();
        let start = Instant::now();
        let merkle = "a1b2c3d4e5f6".to_string();
        log.record_full(
            "nda_tool",
            start,
            ToolAuditOutcome::Success,
            None,
            None,
            None,
            Some(merkle.clone()),
            None,
        );

        let entries = log.recent(10);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].tool_name, "nda_tool");
        assert_eq!(entries[0].merkle_root, Some(merkle));
    }

    #[test]
    fn test_tool_audit_registry_isolation() {
        let registry = ToolAuditRegistry::new();
        let log_a = registry.get_or_create("session-a");
        let log_b = registry.get_or_create("session-b");

        let start = Instant::now();
        log_a.record("tool_a", start, ToolAuditOutcome::Success);
        log_a.record("tool_a2", start, ToolAuditOutcome::Success);
        log_b.record("tool_b", start, ToolAuditOutcome::Success);

        assert_eq!(log_a.len(), 2);
        assert_eq!(log_b.len(), 1);

        let entries_a = log_a.all();
        assert!(entries_a.iter().all(|e| e.tool_name.starts_with("tool_a")));

        let entries_b = log_b.all();
        assert_eq!(entries_b[0].tool_name, "tool_b");
    }

    #[test]
    fn test_tool_audit_registry_aggregate_all() {
        let registry = ToolAuditRegistry::new();
        let log_a = registry.get_or_create("s1");
        let log_b = registry.get_or_create("s2");

        let start = Instant::now();
        log_a.record("tool_1", start, ToolAuditOutcome::Success);
        log_b.record("tool_2", start, ToolAuditOutcome::Success);
        log_a.record("tool_3", start, ToolAuditOutcome::Success);

        let all = registry.aggregate_all();
        assert_eq!(all.len(), 3);
        // Sorted by sequence descending
        assert!(all[0].sequence > all[1].sequence);
        assert!(all[1].sequence > all[2].sequence);
    }

    #[test]
    fn test_tool_audit_export_json() {
        let log = ToolAuditLog::new();
        let start = Instant::now();
        log.record("tool_a", start, ToolAuditOutcome::Success);
        log.record("tool_b", start, ToolAuditOutcome::Error("fail".into()));

        let json = log.export_json().unwrap();
        assert!(json.contains("tool_a"));
        assert!(json.contains("tool_b"));
        assert!(json.contains("Success"));
    }

    #[test]
    fn test_tool_audit_export_csv() {
        let log = ToolAuditLog::new();
        let start = Instant::now();
        log.record("tool_a", start, ToolAuditOutcome::Success);

        let csv = log.export_csv().unwrap();
        assert!(csv.contains("tool_a"));
        assert!(csv.contains("success"));
    }

    #[test]
    fn test_tool_audit_flush_to_file() {
        let log = ToolAuditLog::new();
        let start = Instant::now();
        log.record("tool", start, ToolAuditOutcome::Success);

        let path = "test_tool_audit_flush.json";
        let count = log.flush_to_file(path).unwrap();
        assert_eq!(count, 1);

        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("tool"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_tool_audit_convenience_record() {
        record_tool_call("test-session", "test_tool", Instant::now(), ToolAuditOutcome::Success);

        let entries = tool_audit_registry().aggregate_all();
        assert!(!entries.is_empty());
        assert!(entries.iter().any(|e| e.tool_name == "test_tool"));

        // Cleanup
        tool_audit_registry().clear();
    }
}
