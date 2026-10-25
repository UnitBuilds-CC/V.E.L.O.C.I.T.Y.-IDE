//! Process-global table of live-VC team runs currently in flight.
//!
//! The stdio MCP server reads requests one at a time and runs each handler to
//! completion before reading the next. A `team_dispatch` whose plan routes
//! through the orchestrator therefore cannot be amended *while it runs* unless
//! the run lives somewhere other than the request's own stack frame. This
//! module is that somewhere: `team_dispatch` with `background: true` builds a
//! [`LiveRun`], registers it here by id, hands the tick loop to a worker thread
//! and returns immediately — so the very next `tools/call` (`amend_running_team`,
//! or the P5 observability tools) sees a run that is genuinely executing.
//!
//! The run is shared state, so everything it publishes lives behind `Arc`
//! mutexes the driver thread writes and the request threads read. The one thing
//! a request thread *writes* is the [`DirectiveInbox`], which is why the inbox
//! and not the handles is the amendment surface: the handles belong to the
//! driver thread and cannot be reached cross-thread, but a directive pushed here
//! is drained by that thread on its next wave.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use super::directive::DirectiveInbox;

/// One task as an outside reader sees it — enough to draw the graph and tell
/// which specialist owns what without borrowing the driver's own types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskView {
    pub task_id: u64,
    pub member_id: String,
    pub role: String,
    pub scope: Vec<String>,
    /// "pending" | "running" | "done" | "failed" | "blocked".
    pub status: String,
}

/// The driver thread's published view of the plan, refreshed every wave.
#[derive(Debug, Clone, Default)]
pub struct RunSnapshot {
    pub wave: u32,
    pub elapsed_secs: u64,
    pub tasks: Vec<TaskView>,
}

/// Lifecycle of the run itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunStatus {
    Running,
    Finished {
        succeeded: usize,
        failed: usize,
        blocked: usize,
        wall_secs: u64,
    },
}

/// A team run the pipeline is driving on a background thread.
///
/// `run_id` keys the global table; the three `Arc<Mutex<_>>` fields are the
/// shared surfaces the request threads touch. `driver_done` is what the
/// observability tool checks before offering the final outcome.
pub struct LiveRun {
    pub run_id: String,
    pub team_id: String,
    pub team_name: String,
    pub prompt: String,
    pub root: PathBuf,
    pub trigger: &'static str,
    pub inbox: Arc<Mutex<DirectiveInbox>>,
    pub snapshot: Arc<Mutex<RunSnapshot>>,
    pub status: Arc<Mutex<RunStatus>>,
}

impl LiveRun {
    pub fn new(
        run_id: String,
        team_id: String,
        team_name: String,
        prompt: String,
        root: PathBuf,
        trigger: &'static str,
    ) -> Self {
        Self {
            run_id,
            team_id,
            team_name,
            prompt,
            root,
            trigger,
            inbox: Arc::new(Mutex::new(DirectiveInbox::new())),
            snapshot: Arc::new(Mutex::new(RunSnapshot::default())),
            status: Arc::new(Mutex::new(RunStatus::Running)),
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(*self.status.lock().unwrap(), RunStatus::Running)
    }
}

fn live_runs() -> &'static Mutex<HashMap<String, Arc<LiveRun>>> {
    static TABLE: OnceLock<Mutex<HashMap<String, Arc<LiveRun>>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Insert a run so request threads can find it. Idempotent on the id — a
/// re-register replaces the entry, which is what a resume with the same id
/// needs.
pub fn register(run: Arc<LiveRun>) {
    let mut table = live_runs().lock().unwrap();
    table.insert(run.run_id.clone(), run);
}

pub fn get(run_id: &str) -> Option<Arc<LiveRun>> {
    live_runs().lock().unwrap().get(run_id).cloned()
}

/// The newest run for a team that is still going, when an amendment names a
/// team instead of a run id. `Finished` runs are skipped so an amend never
/// steers a dead plan.
pub fn find_running_by_team(team_id: &str) -> Option<Arc<LiveRun>> {
    let table = live_runs().lock().unwrap();
    table
        .values()
        .filter(|r| r.team_id == team_id && r.is_running())
        .max_by_key(|r| r.run_id.clone())
        .cloned()
}

/// Every registered run id, sorted — the observability list endpoint.
pub fn list_ids() -> Vec<String> {
    let mut ids: Vec<String> = live_runs().lock().unwrap().keys().cloned().collect();
    ids.sort();
    ids
}

/// Drop a run once its driver thread has finished and the outcome has been
/// collected, so the table does not grow without bound across a long session.
pub fn remove(run_id: &str) -> Option<Arc<LiveRun>> {
    live_runs().lock().unwrap().remove(run_id)
}

/// A short, collision-resistant run id derived from the team slug plus a
/// monotonic counter. Deterministic enough to be readable, unique enough that
/// two dispatches of the same team never share an inbox.
pub fn next_run_id(team_slug: &str) -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    format!("run_{team_slug}_{nanos:x}_{n:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(id: &str, team: &str) -> Arc<LiveRun> {
        Arc::new(LiveRun::new(
            id.to_string(),
            team.to_string(),
            "Demo".to_string(),
            "port everything".to_string(),
            PathBuf::from("/tmp"),
            "team_scope",
        ))
    }

    #[test]
    fn register_get_remove_round_trips_a_run() {
        let r = run("run_reg_1", "team_reg");
        register(r.clone());
        assert!(get("run_reg_1").is_some());
        assert!(remove("run_reg_1").is_some());
        assert!(get("run_reg_1").is_none());
    }

    #[test]
    fn find_running_by_team_skips_finished_runs() {
        let r = run("run_find_1", "team_find");
        register(r.clone());
        // Still running → found.
        assert!(find_running_by_team("team_find").is_some());
        *r.status.lock().unwrap() = RunStatus::Finished {
            succeeded: 1,
            failed: 0,
            blocked: 0,
            wall_secs: 3,
        };
        // After finishing, the same team resolves to nothing: an amend must not
        // steer a plan that already closed.
        assert!(find_running_by_team("team_find").is_none());
        remove("run_find_1");
    }

    #[test]
    fn run_ids_are_unique_within_a_process() {
        let a = next_run_id("alpha");
        let b = next_run_id("alpha");
        assert_ne!(a, b, "two dispatches of one team never share an inbox id");
    }
}
