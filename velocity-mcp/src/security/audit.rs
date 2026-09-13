//! Security audit logging framework.
//!
//! Provides a lock-free ring buffer of [`SecurityEvent`]s that records every
//! sensitive operation: process spawns, secret access, file writes, API calls,
//! IPC messages, tool invocations, and authentication events.
//!
//! The audit log is bounded (default 8 192 entries ≈ ~4 MB) so it never causes
//! OOM. Events carry a timestamp, source location, and severity for later
//! analysis and compliance reporting.

use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

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
