//! Multi-agent live coordination: file locking, progress reporting, and
//! inter-agent communication via a shared message bus.
//!
//! Each agent thread receives a handle to the [`CoordinationBus`] and uses it
//! to claim files before writing, broadcast state changes, and delegate work.
//!
//! Wired into the live loop today: `claim_file`/`release_file` (with their
//! `FileClaimed`/`FileReleased` broadcasts) gate concurrent writes, and
//! `report_progress` records per-agent progress. The remaining surface
//! (`request_help`/`HelpRequested`, `drain`/`try_recv`, `all_progress`,
//! `pending_help_for`, `reset`) is the delegation API for the multi-agent team
//! runtime and is intentionally retained ahead of that integration.

use crate::automation::write_gate::WriteGate;
use crate::safety::SafeMutex;
use crossbeam_channel::{Receiver, Sender};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// A message broadcast to all agents on the coordination bus.
#[derive(Debug, Clone)]
pub enum AgentBroadcast {
    /// An agent claimed a file for writing.
    FileClaimed { agent_id: String, path: PathBuf },
    /// An agent released a previously claimed file.
    FileReleased { agent_id: String, path: PathBuf },
    /// An agent is requesting help from another agent.
    HelpRequested {
        from: String,
        to: String,
        task: String,
    },
    /// An agent reported progress on its current task.
    ProgressReported {
        agent_id: String,
        percent: f32,
        status: String,
    },
    /// An agent finished its task.
    AgentFinished { agent_id: String, summary: String },
}

/// Progress report from a single agent.
#[derive(Debug, Clone)]
pub struct AgentProgress {
    pub agent_id: String,
    pub percent: f32,
    pub status: String,
    pub updated_at: u64,
}

/// Shared state guarded by a mutex inside the coordination bus. File claims live on
/// the shared [`WriteGate`], not here; this holds only the delegation surface.
#[derive(Debug, Default)]
struct BusState {
    /// Latest progress per agent.
    progress: HashMap<String, AgentProgress>,
    /// Pending help requests: (from, to, task).
    help_requests: Vec<(String, String, String)>,
}

/// A handle to the multi-agent coordination bus.
///
/// Cheap to clone (Arc-based) so each agent thread gets its own handle.
#[derive(Clone)]
pub struct CoordinationBus {
    gate: Arc<WriteGate>,
    state: Arc<Mutex<BusState>>,
    tx: Sender<AgentBroadcast>,
    rx: Receiver<AgentBroadcast>,
}

impl CoordinationBus {
    /// Create a new coordination bus with its own private write gate.
    pub fn new() -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        Self {
            gate: Arc::new(WriteGate::new()),
            state: Arc::new(Mutex::new(BusState::default())),
            tx,
            rx,
        }
    }

    /// Create a coordination bus whose file claims share the given gate instance.
    /// This is how the executor thread is pointed at the app's one arena, so a chat
    /// agent's claim and an orchestrator scope lock consult the *same* table (D5).
    pub fn with_gate(gate: Arc<WriteGate>) -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        Self {
            gate,
            state: Arc::new(Mutex::new(BusState::default())),
            tx,
            rx,
        }
    }

    /// Broadcast a message to all agents.
    pub fn broadcast(&self, msg: AgentBroadcast) {
        let _ = self.tx.send(msg);
    }

    /// Try to receive a broadcast message (non-blocking).
    pub fn try_recv(&self) -> Option<AgentBroadcast> {
        self.rx.try_recv().ok()
    }

    /// Drain all pending broadcast messages.
    pub fn drain(&self) -> Vec<AgentBroadcast> {
        let mut msgs = Vec::new();
        while let Ok(msg) = self.rx.try_recv() {
            msgs.push(msg);
        }
        msgs
    }

    /// Attempt to claim a file for exclusive writing on the shared gate.
    /// Returns `true` if the claim succeeded, `false` if another owner holds an
    /// overlapping lock. Broadcasts `FileClaimed` only on success.
    pub fn claim_file(&self, agent_id: &str, path: &Path) -> bool {
        if self.gate.claim_whole_file(agent_id, path) {
            self.broadcast(AgentBroadcast::FileClaimed {
                agent_id: agent_id.to_string(),
                path: path.to_path_buf(),
            });
            true
        } else {
            false
        }
    }

    /// Release a previously claimed file. No-op (and no broadcast) unless the
    /// caller actually holds the claim.
    pub fn release_file(&self, agent_id: &str, path: &Path) {
        if self.gate.holds(agent_id, path) {
            self.gate.release(path, agent_id);
            self.broadcast(AgentBroadcast::FileReleased {
                agent_id: agent_id.to_string(),
                path: path.to_path_buf(),
            });
        }
    }

    /// Check if a file is currently locked by another owner on the shared gate.
    pub fn is_locked_by_other(&self, agent_id: &str, path: &Path) -> bool {
        self.gate.is_locked_by_other(agent_id, path)
    }

    /// Request help from another agent.
    pub fn request_help(&self, from: &str, to: &str, task: &str) {
        {
            let mut state = self.state.lock_safe();
            state
                .help_requests
                .push((from.to_string(), to.to_string(), task.to_string()));
        }
        self.broadcast(AgentBroadcast::HelpRequested {
            from: from.to_string(),
            to: to.to_string(),
            task: task.to_string(),
        });
    }

    /// Report progress for an agent.
    pub fn report_progress(&self, agent_id: &str, percent: f32, status: &str) {
        {
            let mut state = self.state.lock_safe();
            state.progress.insert(
                agent_id.to_string(),
                AgentProgress {
                    agent_id: agent_id.to_string(),
                    percent: percent.clamp(0.0, 100.0),
                    status: status.to_string(),
                    updated_at: current_ts(),
                },
            );
        }
        self.broadcast(AgentBroadcast::ProgressReported {
            agent_id: agent_id.to_string(),
            percent,
            status: status.to_string(),
        });
    }

    /// Get a snapshot of all agent progress reports.
    pub fn all_progress(&self) -> Vec<AgentProgress> {
        let state = self.state.lock_safe();
        state.progress.values().cloned().collect()
    }

    /// Get all currently locked files (delegated to the shared gate).
    pub fn locked_files(&self) -> Vec<(PathBuf, String)> {
        self.gate.locked_files()
    }

    /// Get all current file locks with the owning agent id and the unix-seconds
    /// timestamp at which each lock was claimed, enabling stale-lock detection.
    pub fn lock_details(&self) -> Vec<(PathBuf, String, u64)> {
        self.gate.lock_details()
    }

    /// Get pending help requests for a specific agent.
    pub fn pending_help_for(&self, agent_id: &str) -> Vec<(String, String)> {
        let state = self.state.lock_safe();
        state
            .help_requests
            .iter()
            .filter(|(_, to, _)| to == agent_id)
            .map(|(from, _, task)| (from.clone(), task.clone()))
            .collect()
    }

    /// Clear all locks and progress (session cleanup). Because file claims now live
    /// on the shared gate, this clears every claim on that gate too.
    pub fn reset(&self) {
        self.gate.clear_all();
        let mut state = self.state.lock_safe();
        state.progress.clear();
        state.help_requests.clear();
    }
}

impl Default for CoordinationBus {
    fn default() -> Self {
        Self::new()
    }
}

fn current_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_and_release() {
        let bus = CoordinationBus::new();
        let path = Path::new("src/main.rs");

        assert!(bus.claim_file("agent-1", path));
        // Same agent can re-claim
        assert!(bus.claim_file("agent-1", path));
        // Different agent cannot claim
        assert!(!bus.claim_file("agent-2", path));

        bus.release_file("agent-1", path);
        // Now agent-2 can claim
        assert!(bus.claim_file("agent-2", path));
    }

    #[test]
    fn progress_reporting() {
        let bus = CoordinationBus::new();
        bus.report_progress("agent-1", 50.0, "halfway done");
        bus.report_progress("agent-2", 25.0, "starting");

        let progress = bus.all_progress();
        assert_eq!(progress.len(), 2);
    }

    #[test]
    fn help_requests() {
        let bus = CoordinationBus::new();
        bus.request_help("agent-1", "agent-2", "fix the build");

        let pending = bus.pending_help_for("agent-2");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].0, "agent-1");
        assert_eq!(pending[0].1, "fix the build");

        // agent-1 has no pending requests
        assert!(bus.pending_help_for("agent-1").is_empty());
    }

    #[test]
    fn broadcast_and_drain() {
        let bus = CoordinationBus::new();
        bus.report_progress("a1", 10.0, "working");
        bus.report_progress("a2", 20.0, "also working");

        let msgs = bus.drain();
        assert!(msgs.len() >= 2);
    }

    #[test]
    fn is_locked_by_other_check() {
        let bus = CoordinationBus::new();
        let path = Path::new("test.txt");

        assert!(!bus.is_locked_by_other("agent-1", path));
        bus.claim_file("agent-1", path);
        assert!(!bus.is_locked_by_other("agent-1", path));
        assert!(bus.is_locked_by_other("agent-2", path));
    }

    #[test]
    fn reset_clears_all_state() {
        let bus = CoordinationBus::new();
        let path = Path::new("locked.txt");
        bus.claim_file("agent-1", path);
        bus.report_progress("agent-1", 50.0, "working");
        bus.request_help("agent-1", "agent-2", "help me");

        bus.reset();

        assert!(bus.locked_files().is_empty());
        assert!(bus.all_progress().is_empty());
        assert!(bus.pending_help_for("agent-2").is_empty());
        // After reset, the file is no longer locked
        assert!(!bus.is_locked_by_other("agent-2", path));
    }

    #[test]
    fn locked_files_returns_all_claims() {
        let bus = CoordinationBus::new();
        bus.claim_file("agent-1", Path::new("a.txt"));
        bus.claim_file("agent-2", Path::new("b.txt"));
        bus.claim_file("agent-1", Path::new("c.txt"));

        let locked = bus.locked_files();
        assert_eq!(locked.len(), 3);
        let agents: Vec<&str> = locked.iter().map(|(_, a)| a.as_str()).collect();
        assert!(agents.contains(&"agent-1"));
        assert!(agents.contains(&"agent-2"));
    }

    #[test]
    fn drain_returns_all_pending_messages() {
        let bus = CoordinationBus::new();
        bus.broadcast(AgentBroadcast::AgentFinished {
            agent_id: "a1".to_string(),
            summary: "done".to_string(),
        });
        bus.report_progress("a1", 100.0, "finished");

        let msgs = bus.drain();
        assert!(msgs.len() >= 2);
        // After drain, no more messages
        assert!(bus.drain().is_empty());
    }

    #[test]
    fn try_recv_returns_none_when_empty() {
        let bus = CoordinationBus::new();
        assert!(bus.try_recv().is_none());
    }

    #[test]
    fn release_file_by_non_owner_is_noop() {
        let bus = CoordinationBus::new();
        let path = Path::new("test.txt");
        bus.claim_file("agent-1", path);
        // agent-2 tries to release agent-1's file — should not remove it
        bus.release_file("agent-2", path);
        assert!(bus.is_locked_by_other("agent-2", path));
    }

    #[test]
    fn multiple_help_requests_accumulate() {
        let bus = CoordinationBus::new();
        bus.request_help("a1", "a2", "task1");
        bus.request_help("a3", "a2", "task2");
        bus.request_help("a1", "a2", "task3");

        let pending = bus.pending_help_for("a2");
        assert_eq!(pending.len(), 3);
    }

    #[test]
    fn progress_is_clamped_to_100() {
        let bus = CoordinationBus::new();
        bus.report_progress("a1", 150.0, "over 100");
        let progress = bus.all_progress();
        assert_eq!(progress.len(), 1);
        assert_eq!(progress[0].percent, 100.0);
    }

    #[test]
    fn default_bus_is_empty() {
        let bus = CoordinationBus::default();
        assert!(bus.locked_files().is_empty());
        assert!(bus.all_progress().is_empty());
        assert!(bus.drain().is_empty());
    }

    #[test]
    fn shared_gate_makes_bus_claims_visible_to_mediator() {
        // THE D5 RUNTIME UNIFY: with a chat agent (CoordinationBus) and an
        // orchestrator task (MediatorArena) pointed at ONE shared WriteGate, an
        // executor claim is now seen by the mediator and vice versa — previously
        // impossible because they kept separate lock tables.
        use crate::automation::write_gate::WriteGate;
        use crate::automation::MediatorArena;
        use std::path::Path;
        use std::sync::Arc;
        use tempfile::TempDir;
        use velocity_ide::site_map::SiteMap;

        let gate = Arc::new(WriteGate::new());
        let mediator = MediatorArena::from_gate(gate.clone());
        let bus = CoordinationBus::with_gate(gate);
        let temp = TempDir::new().unwrap();
        let sm = SiteMap::open(temp.path(), 0).unwrap();
        let file = Path::new("src/shared.rs");

        // Executor claims whole-file through the bus ...
        assert!(bus.claim_file("primary", file));
        // ... the orchestrator's mediator now refuses a scope lock on the same file.
        let conflict = mediator.acquire_lock(file.to_path_buf(), (1, 5), "task-9".to_string(), &sm);
        assert!(
            conflict.is_err(),
            "mediator must see the bus claim on the shared gate"
        );

        // Releasing through the bus frees the mediator path too.
        bus.release_file("primary", file);
        assert!(mediator
            .acquire_lock(file.to_path_buf(), (1, 5), "task-9".to_string(), &sm)
            .is_ok());
    }
}
