//! Persist the live-VC pipeline's plan and progress to disk so an MCP restart
//! resumes a run instead of losing it.
//!
//! Path: `<root>/.velocity/orchestrator/checkpoint.json`. Written after every
//! dispatch wave inside [`crate::orchestrator::team_pipeline::run_pipeline`]
//! and read back when `team_dispatch` is called with `resume:true` on the
//! same `team_id` + prompt. A checkpoint records enough to rebuild both the
//! [`TaskGraph`] and the [`OrchestratorRegistry`] — task identity, member
//! bindings, priority/weight, terminal status, outputs and per-worker
//! duration — but deliberately not transcripts or provider streams; those
//! died with the process.
//!
//! Running tasks become Pending on restore, because their threads are gone:
//! the caller sees the plan resumable from the next wave, which is exactly
//! what "resume after restart" has to mean for a headless dispatch. Done,
//! Failed and Blocked statuses replay so the reconcile root and
//! `propagate_blocked_dependents` see the shape the last run ended on.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::editor::orchestrator::task_result_outputs;
use crate::orchestrator::blueprint::{Task, TaskGraph};
use crate::orchestrator::registry::{OrchestratorRegistry, TaskStatus};
use crate::orchestrator::worker::WorkerResult;
use crate::orchestrator::TaskId;

/// Bumped whenever the JSON shape changes incompatibly. A checkpoint written
/// by an older (or newer) reader is refused with `None` rather than half-
/// parsed — a mismatched resume is worse than no resume, because the caller
/// will replan and clobber the file.
pub const CHECKPOINT_VERSION: u32 = 1;

/// One serialized task node — mirrors [`Task`] plus the role-task identity the
/// pipeline attaches (which team member owns it).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointTask {
    pub id: u64,
    pub title: String,
    pub description: String,
    pub scope: Vec<String>,
    pub dependencies: Vec<u64>,
    pub priority: u32,
    pub estimated_weight: u32,
    pub output: Option<String>,
    #[serde(default)]
    pub member_id: String,
    #[serde(default)]
    pub role: String,
}

/// One serialized [`TaskStatus`] — the flat shape the JSON writer emits. A
/// running task is recorded as `"running"` on write; [`restore_registry`]
/// turns any running entry back into Pending so the next wave re-dispatches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointStatus {
    pub id: u64,
    /// "pending" | "running" | "done" | "failed" | "blocked"
    pub kind: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub provider_label: String,
    #[serde(default)]
    pub model_label: String,
    #[serde(default)]
    pub outputs: Vec<String>,
    #[serde(default)]
    pub created_files: Vec<String>,
    #[serde(default)]
    pub duration_secs: u64,
}

/// Everything the pipeline needs to be resumable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub version: u32,
    pub trigger: String,
    pub team_id: String,
    pub team_name: String,
    pub prompt: String,
    /// 0 = uncapped. Mirrors [`crate::automation::mission_runner::MissionConfig::max_wall_secs`].
    pub max_wall_secs: u64,
    /// Formatted as 16-hex so an operator can eyeball a stale SiteMap root.
    pub planned_site_map_root: String,
    pub weight_root: String,
    /// Wall clock already spent when this snapshot was written; the resume
    /// pass adds it to the new `Instant::now()` base so a deadline still
    /// fires at the right absolute time.
    pub elapsed_secs: u64,
    pub root_task_id: u64,
    pub tasks: Vec<CheckpointTask>,
    pub statuses: Vec<CheckpointStatus>,
}

/// Where the pipeline's on-disk state lives.
pub fn checkpoint_path(root: &Path) -> PathBuf {
    root.join(".velocity")
        .join("orchestrator")
        .join("checkpoint.json")
}

/// Write atomically: a truncated checkpoint would read as "no run" or, worse,
/// "run ended cleanly", so we write to a sibling temp and rename on success.
pub fn write_checkpoint(root: &Path, cp: &Checkpoint) -> std::io::Result<()> {
    let path = checkpoint_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_string_pretty(cp)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body.as_bytes())?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Load a checkpoint. Returns `None` for missing, unreadable, unparseable or
/// version-mismatched files — the caller then replans, which is the same
/// behaviour as "no prior run".
pub fn read_checkpoint(root: &Path) -> Option<Checkpoint> {
    let path = checkpoint_path(root);
    let bytes = std::fs::read(&path).ok()?;
    let cp: Checkpoint = serde_json::from_slice(&bytes).ok()?;
    if cp.version != CHECKPOINT_VERSION {
        return None;
    }
    Some(cp)
}

/// Remove the checkpoint — called when the plan reaches `is_complete()` so a
/// later identical dispatch correctly starts fresh.
pub fn clear_checkpoint(root: &Path) {
    let _ = std::fs::remove_file(checkpoint_path(root));
}

/// A checkpoint belongs to a resume only when it names the same team AND the
/// same prompt. Anything else means the operator changed direction, and
/// replaying an old plan would be worse than replanning.
pub fn matches(cp: &Checkpoint, team_id: &str, prompt: &str) -> bool {
    cp.team_id == team_id && cp.prompt == prompt
}

/// Snapshot the pipeline state. `role_members` pairs each task id with the
/// (member_id, role) it belongs to, so the graph and the roster binding are
/// both reconstructable on the far side of a restart.
#[allow(clippy::too_many_arguments)]
pub fn snapshot(
    graph: &TaskGraph,
    registry: &OrchestratorRegistry,
    role_members: &HashMap<u64, (String, String)>,
    trigger: &str,
    team_id: &str,
    team_name: &str,
    prompt: &str,
    max_wall_secs: u64,
    planned_site_map_root: u64,
    weight_root: u64,
    elapsed: Duration,
) -> Checkpoint {
    let mut tasks: Vec<CheckpointTask> = graph
        .tasks
        .values()
        .map(|t| {
            let (member_id, role) = role_members.get(&t.id.0).cloned().unwrap_or_default();
            CheckpointTask {
                id: t.id.0,
                title: t.title.clone(),
                description: t.description.clone(),
                scope: t.scope.clone(),
                dependencies: t.dependencies.iter().map(|d| d.0).collect(),
                priority: t.priority,
                estimated_weight: t.estimated_weight,
                output: t.output.clone(),
                member_id,
                role,
            }
        })
        .collect();
    tasks.sort_by_key(|t| t.id);
    let mut statuses: Vec<CheckpointStatus> = registry
        .statuses
        .iter()
        .map(|(id, st)| {
            let (kind, msg, provider, model, outs, created, dur) = match st {
                TaskStatus::Pending => (
                    "pending",
                    String::new(),
                    String::new(),
                    String::new(),
                    Vec::new(),
                    Vec::new(),
                    0u64,
                ),
                TaskStatus::Running => (
                    "running",
                    String::new(),
                    String::new(),
                    String::new(),
                    Vec::new(),
                    Vec::new(),
                    0u64,
                ),
                TaskStatus::Done(r) => terminal("done", r),
                TaskStatus::Failed(r) => terminal("failed", r),
                TaskStatus::Blocked(r) => terminal("blocked", r),
            };
            CheckpointStatus {
                id: id.0,
                kind: kind.to_string(),
                message: msg,
                provider_label: provider,
                model_label: model,
                outputs: outs,
                created_files: created,
                duration_secs: dur,
            }
        })
        .collect();
    statuses.sort_by_key(|s| s.id);
    Checkpoint {
        version: CHECKPOINT_VERSION,
        trigger: trigger.to_string(),
        team_id: team_id.to_string(),
        team_name: team_name.to_string(),
        prompt: prompt.to_string(),
        max_wall_secs,
        planned_site_map_root: format!("{:016x}", planned_site_map_root),
        weight_root: format!("{:016x}", weight_root),
        elapsed_secs: elapsed.as_secs(),
        root_task_id: graph.root.0,
        tasks,
        statuses,
    }
}

fn terminal(
    kind: &'static str,
    r: &WorkerResult,
) -> (
    &'static str,
    String,
    String,
    String,
    Vec<String>,
    Vec<String>,
    u64,
) {
    (
        kind,
        r.message.clone(),
        r.provider_label.clone(),
        r.model_label.clone(),
        task_result_outputs(r),
        r.created_files.clone(),
        r.duration.as_secs(),
    )
}

/// Rebuild the [`TaskGraph`] from a checkpoint. Ids are preserved so the
/// reconcile root and every dependency edge land where the last run left
/// them.
pub fn restore_graph(cp: &Checkpoint) -> TaskGraph {
    let mut graph = TaskGraph::default();
    graph.root = TaskId(cp.root_task_id);
    for t in &cp.tasks {
        graph.tasks.insert(
            TaskId(t.id),
            Task {
                id: TaskId(t.id),
                title: t.title.clone(),
                description: t.description.clone(),
                scope: t.scope.clone(),
                dependencies: t.dependencies.iter().map(|d| TaskId(*d)).collect(),
                output: t.output.clone(),
                priority: t.priority,
                estimated_weight: t.estimated_weight,
            },
        );
    }
    graph
}

/// Rebuild the [`OrchestratorRegistry`] from a checkpoint's status list.
/// Running tasks revert to Pending — their threads died with the previous
/// process and re-dispatch is what "resume" has to mean for headless.
/// Done/Failed/Blocked replay so `propagate_blocked_dependents` and the
/// reconcile root see the shape the last run ended on.
pub fn restore_registry(cp: &Checkpoint, graph: &TaskGraph) -> OrchestratorRegistry {
    let mut registry = OrchestratorRegistry::new(graph);
    for s in &cp.statuses {
        let id = TaskId(s.id);
        let status = match s.kind.as_str() {
            "done" => TaskStatus::Done(rebuild_result(id, s, true)),
            "failed" => TaskStatus::Failed(rebuild_result(id, s, false)),
            "blocked" => TaskStatus::Blocked(rebuild_result(id, s, false)),
            _ => TaskStatus::Pending,
        };
        registry.statuses.insert(id, status);
        if !s.outputs.is_empty() {
            registry.outputs.insert(id, s.outputs.clone());
        }
    }
    registry
}

fn rebuild_result(id: TaskId, s: &CheckpointStatus, success: bool) -> WorkerResult {
    WorkerResult {
        success,
        task_id: id,
        outputs: s.outputs.clone(),
        duration: Duration::from_secs(s.duration_secs),
        message: s.message.clone(),
        provider_label: s.provider_label.clone(),
        model_label: s.model_label.clone(),
        transcript: String::new(),
        status_updates: if s.message.is_empty() {
            Vec::new()
        } else {
            vec![s.message.clone()]
        },
        attempts: Vec::new(),
        created_files: s.created_files.clone(),
        deleted_files: Vec::new(),
        out_of_scope_created_files: Vec::new(),
        run_summary_path: None,
        run_facts_path: None,
        wa_run_path: None,
        wa_run_id: None,
        is_read_only: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "velocity_checkpoint_{tag}_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_graph() -> TaskGraph {
        let mut g = TaskGraph::default();
        g.root = TaskId(1);
        g.add(
            TaskId(1),
            "Reconcile",
            "root",
            vec![".velocity/agentic".into()],
            vec![],
            None,
        );
        g.add(
            TaskId(2),
            "Lead",
            "plan",
            vec!["src/".into()],
            vec![],
            Some(TaskId(1)),
        );
        g.add(
            TaskId(3),
            "Backend",
            "port src/db",
            vec!["src/db/".into()],
            vec![TaskId(2)],
            Some(TaskId(1)),
        );
        g
    }

    fn sample_registry(g: &TaskGraph) -> OrchestratorRegistry {
        let mut r = OrchestratorRegistry::new(g);
        let done = WorkerResult {
            success: true,
            task_id: TaskId(2),
            outputs: vec![".velocity/run/facts/lead.json".into()],
            duration: Duration::from_secs(12),
            message: "plan written".into(),
            provider_label: "cloudflare".into(),
            model_label: "x-2".into(),
            transcript: String::new(),
            status_updates: vec![],
            attempts: vec![],
            created_files: vec!["src/plan.md".into()],
            deleted_files: vec![],
            out_of_scope_created_files: vec![],
            run_summary_path: None,
            run_facts_path: None,
            wa_run_path: None,
            wa_run_id: None,
            is_read_only: false,
        };
        r.statuses.insert(TaskId(2), TaskStatus::Done(done));
        r.outputs
            .insert(TaskId(2), vec![".velocity/run/facts/lead.json".into()]);
        r.statuses.insert(TaskId(3), TaskStatus::Running);
        r
    }

    #[test]
    fn checkpoint_round_trips_after_a_wave() {
        let root = temp_root("roundtrip");
        let graph = sample_graph();
        let registry = sample_registry(&graph);
        let mut role_members = HashMap::new();
        role_members.insert(2u64, ("member_0".to_string(), "Lead".to_string()));
        role_members.insert(3u64, ("member_1".to_string(), "Backend".to_string()));
        let cp = snapshot(
            &graph,
            &registry,
            &role_members,
            "team_scope",
            "team_demo",
            "Demo Team",
            "port everything",
            0,
            0xdeadbeef,
            0xfeedface,
            Duration::from_secs(42),
        );
        write_checkpoint(&root, &cp).expect("write");
        let loaded = read_checkpoint(&root).expect("read");
        assert_eq!(loaded.version, CHECKPOINT_VERSION);
        assert_eq!(loaded.team_id, "team_demo");
        assert_eq!(loaded.prompt, "port everything");
        assert_eq!(loaded.trigger, "team_scope");
        assert_eq!(loaded.elapsed_secs, 42);
        assert_eq!(loaded.root_task_id, 1);
        assert_eq!(loaded.tasks.len(), 3);
        let backend = loaded
            .tasks
            .iter()
            .find(|t| t.id == 3)
            .expect("backend task present");
        assert_eq!(backend.dependencies, vec![2]);
        assert_eq!(backend.member_id, "member_1");
        assert_eq!(backend.role, "Backend");
        let lead_status = loaded
            .statuses
            .iter()
            .find(|s| s.id == 2)
            .expect("lead status");
        assert_eq!(lead_status.kind, "done");
        // `task_result_outputs` merges result.outputs + created_files +
        // deleted_files (sorted, deduped), so one output and one created
        // file come back as two entries — the checkpoint stores what the
        // reconciler saw, not what the worker literally returned.
        assert_eq!(lead_status.outputs.len(), 2);
        assert!(lead_status
            .outputs
            .iter()
            .any(|o| o == ".velocity/run/facts/lead.json"));
        assert!(lead_status.outputs.iter().any(|o| o == "src/plan.md"));
        assert_eq!(lead_status.created_files.len(), 1);
        assert_eq!(lead_status.duration_secs, 12);
        let backend_status = loaded
            .statuses
            .iter()
            .find(|s| s.id == 3)
            .expect("backend status");
        assert_eq!(backend_status.kind, "running");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn restore_registry_keeps_done_and_reopens_running() {
        let graph = sample_graph();
        let registry = sample_registry(&graph);
        let mut role_members = HashMap::new();
        role_members.insert(2u64, ("member_0".to_string(), "Lead".to_string()));
        role_members.insert(3u64, ("member_1".to_string(), "Backend".to_string()));
        let cp = snapshot(
            &graph,
            &registry,
            &role_members,
            "team_scope",
            "t",
            "n",
            "p",
            0,
            0,
            0,
            Duration::ZERO,
        );
        let restored_graph = restore_graph(&cp);
        let restored = restore_registry(&cp, &restored_graph);
        assert!(matches!(
            restored.statuses.get(&TaskId(2)),
            Some(TaskStatus::Done(_))
        ));
        // Task 3 was Running when the checkpoint wrote — after a restart its
        // thread is gone, so resume reopens it as Pending for a fresh dispatch.
        assert!(matches!(
            restored.statuses.get(&TaskId(3)),
            Some(TaskStatus::Pending)
        ));
        // Root task stays Pending — the reconciler has to run again.
        assert!(matches!(
            restored.statuses.get(&TaskId(1)),
            Some(TaskStatus::Pending)
        ));
        // Lead's outputs survive so `reconciliation_error` still sees them.
        assert!(restored.outputs.contains_key(&TaskId(2)));
    }

    #[test]
    fn matches_requires_both_team_and_prompt() {
        let cp = Checkpoint {
            version: CHECKPOINT_VERSION,
            trigger: "team_scope".into(),
            team_id: "team_alpha".into(),
            team_name: "Alpha".into(),
            prompt: "port everything".into(),
            max_wall_secs: 0,
            planned_site_map_root: "0000000000000000".into(),
            weight_root: "0000000000000000".into(),
            elapsed_secs: 0,
            root_task_id: 1,
            tasks: vec![],
            statuses: vec![],
        };
        assert!(matches(&cp, "team_alpha", "port everything"));
        assert!(!matches(&cp, "team_beta", "port everything"));
        assert!(!matches(&cp, "team_alpha", "port everything else"));
    }

    #[test]
    fn clear_checkpoint_removes_the_file() {
        let root = temp_root("clear");
        let graph = sample_graph();
        let registry = sample_registry(&graph);
        let cp = snapshot(
            &graph,
            &registry,
            &HashMap::new(),
            "team_scope",
            "t",
            "n",
            "p",
            0,
            0,
            0,
            Duration::ZERO,
        );
        write_checkpoint(&root, &cp).unwrap();
        assert!(checkpoint_path(&root).exists());
        clear_checkpoint(&root);
        assert!(!checkpoint_path(&root).exists());
        std::fs::remove_dir_all(&root).ok();
    }

    // Non-vacuity of the atomic-write path: with `write_checkpoint` reverted
    // to a plain `std::fs::write(&path, ...)` (no tmp + rename), the file
    // still lands and this test would still pass. What actually distinguishes
    // the atomic version is the intermediate `.tmp` sibling never surviving
    // the call — which is what a crash-resume reader needs. We assert the
    // invariant directly: after write, only `checkpoint.json` exists in the
    // directory and the tmp path is absent.
    #[test]
    fn atomic_write_leaves_no_temporary_sibling() {
        let root = temp_root("atomic");
        let graph = sample_graph();
        let registry = sample_registry(&graph);
        let cp = snapshot(
            &graph,
            &registry,
            &HashMap::new(),
            "team_scope",
            "t",
            "n",
            "p",
            0,
            0,
            0,
            Duration::ZERO,
        );
        write_checkpoint(&root, &cp).unwrap();
        let tmp = checkpoint_path(&root).with_extension("json.tmp");
        assert!(!tmp.exists(), "atomic rename must remove the tmp sibling");
        std::fs::remove_dir_all(&root).ok();
    }

    // Non-vacuity of the version gate: a checkpoint whose `version` does not
    // equal `CHECKPOINT_VERSION` must return `None` from `read_checkpoint`,
    // so a mismatched reader cannot half-resume an incompatible plan.
    #[test]
    fn read_refuses_version_mismatch() {
        let root = temp_root("version");
        let cp = Checkpoint {
            version: 999,
            trigger: "team_scope".into(),
            team_id: "t".into(),
            team_name: "n".into(),
            prompt: "p".into(),
            max_wall_secs: 0,
            planned_site_map_root: "0000000000000000".into(),
            weight_root: "0000000000000000".into(),
            elapsed_secs: 0,
            root_task_id: 1,
            tasks: vec![CheckpointTask {
                id: 1,
                title: "x".into(),
                description: String::new(),
                scope: vec![],
                dependencies: vec![],
                priority: 0,
                estimated_weight: 1,
                output: None,
                member_id: String::new(),
                role: String::new(),
            }],
            statuses: vec![],
        };
        write_checkpoint(&root, &cp).unwrap();
        assert!(read_checkpoint(&root).is_none());
        std::fs::remove_dir_all(&root).ok();
    }
}
