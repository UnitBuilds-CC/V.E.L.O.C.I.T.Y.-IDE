//! Live-VC team pipeline — the headless twin of the orchestrator panel.
//!
//! When a `team_dispatch` request classifies as [`TaskScope::Team`] or
//! [`TaskHorizon::Long`], the single-member headless chain is the wrong tool:
//! one agent, one context window, one wall-clock budget cannot rewrite a
//! repository. This module turns the intent's role list into a [`TaskGraph`]
//! (one task per member, dependency edges inferred from scope overlap) and
//! drives the same [`OrchestratorRegistry`] + [`spawn_live_worker`] tick loop
//! the egui panel runs, only without the paint step. The panel's own tick body
//! lives in `editor/orchestrator/panel/execution.rs`; this file is the
//! non-interactive mirror.
//!
//! Correctness-critical pieces imported from the panel driver. In order:
//! the ONE shared [`MediatorArena`] per run, the per-spawn
//! `planned_site_map_root` freshness gate, `reconciliation_error` +
//! `requires_follow_up` before marking Done, and `propagate_blocked_dependents`
//! plus `complete_reconcile_root` after every wave. Load-balancer seat admission
//! and the UI watchdog are deliberately absent. With no human on the panel,
//! the only useful back-pressure is the mediator's file lock, and every
//! worker already caps its own wall clock through `budget_left`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use velocity_ide::site_map::SiteMap;

use crate::agent::intent::{IntentDecision, TaskHorizon, TaskScope};
use crate::agent::AiProvider;
use crate::automation::instruction_registry::AgentTaskKind;
use crate::automation::mediator::MediatorArena;
use crate::automation::task_router::RoutedModelRoute;
use crate::editor::expert_team::{ExpertMember, ExpertTeam};
use crate::editor::orchestrator::{
    complete_reconcile_root, propagate_blocked_dependents, reconciliation_error,
    requires_follow_up, task_result_outputs,
};
use crate::orchestrator::blueprint::{Task, TaskGraph};
use crate::orchestrator::checkpoint::{
    self as cp, clear_checkpoint, matches as cp_matches, read_checkpoint, snapshot as cp_snapshot,
    write_checkpoint,
};
use crate::orchestrator::directive::{plan_directive_deliveries, RosterEntry, WorkerState};
use crate::orchestrator::live_runs::{LiveRun, RunStatus, TaskView};
use crate::orchestrator::registry::{OrchestratorRegistry, TaskStatus};
use crate::orchestrator::worker::{
    spawn_live_worker, WorkerAssignment, WorkerHandle, WorkerResult,
};
use crate::orchestrator::TaskId;

/// Reason the pipeline chose the orchestrator over the single-member chain.
///
/// Kept as an enum so the JSON response can tell the operator which signal
/// fired — the team-scope verdict and the long-horizon verdict are separate
/// axes (see [`TaskHorizon`]) and only the second can surprise: a request
/// that scores as a single-agent chat message but says "port the whole repo"
/// still needs the parallel plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineTrigger {
    TeamScope,
    LongHorizon,
}

/// Decide whether a dispatch runs through the orchestrator, and why.
///
/// Both axes are consulted: the user asked for a team, or the classifier
/// decided the work outlives one session. Short single-agent requests return
/// `None` so they fall through to the pre-existing headless chain — the
/// crucible's `run_headless_subagent` path must not see a behaviour change.
pub fn pipeline_trigger(decision: &IntentDecision) -> Option<PipelineTrigger> {
    if decision.scope == TaskScope::Team {
        Some(PipelineTrigger::TeamScope)
    } else if decision.horizon == TaskHorizon::Long {
        Some(PipelineTrigger::LongHorizon)
    } else {
        None
    }
}

/// The reconciler root id is always `TaskId(1)`; role tasks start at `2`.
///
/// This is the same convention `build_routed_graph` uses, which means the
/// existing `complete_reconcile_root`, `reconciliation_error` and
/// `propagate_blocked_dependents` helpers keep working unchanged.
pub const RECONCILE_ROOT_ID: u64 = 1;

/// One member-bound task in the pipeline.
///
/// The graph node gives the scheduler what it needs; the assignment is the
/// `WorkerAssignment` minus `workspace_root`/`planned_site_map_root`/`weight_root`,
/// which are filled in at spawn time so a stale plan is caught once, at the
/// same moment the freshness gate fires.
#[derive(Debug, Clone)]
pub struct RoleTask {
    pub member_id: String,
    pub role: String,
    pub task: Task,
    pub instructions: String,
    pub task_kind: AgentTaskKind,
    pub provider: AiProvider,
    pub provider_label: String,
    pub model_id: String,
    pub model_label: String,
    pub thinking: bool,
    pub fallback_chain: Vec<RoutedModelRoute>,
}

/// Per-task outcome the MCP response reports.
#[derive(Debug, Clone)]
pub struct TaskOutcome {
    pub task_id: u64,
    pub member_id: String,
    pub role: String,
    pub status: &'static str,
    pub success: bool,
    pub message: String,
    pub outputs: Vec<String>,
    pub created_files: Vec<String>,
    pub provider_label: String,
    pub model_label: String,
    pub duration_secs: u64,
}

/// Aggregated pipeline result — what `team_dispatch` serialises back to the caller.
#[derive(Debug, Clone)]
pub struct PipelineOutcome {
    pub trigger: PipelineTrigger,
    pub rationale: String,
    pub tasks: Vec<TaskOutcome>,
    pub succeeded: usize,
    pub failed: usize,
    pub blocked: usize,
    pub wall_elapsed: Duration,
}

/// Textual label for a [`PipelineTrigger`] — used in the JSON response so an
/// operator can see *why* the run went parallel without parsing signals.
pub fn trigger_label(trigger: PipelineTrigger) -> &'static str {
    match trigger {
        PipelineTrigger::TeamScope => "team_scope",
        PipelineTrigger::LongHorizon => "long_horizon",
    }
}

/// Pick an [`AgentTaskKind`] for a role from its skills and title.
///
/// Priority order matters here: an auditor whose skills include "test" is
/// still doing analysis, not writing test files, so the "audit/review/security"
/// check runs before "test". A doc-role with "port" in its skills is a
/// documentation pass over a port, which is documentation, not refactoring.
pub fn task_kind_for_member(member: &ExpertMember) -> AgentTaskKind {
    let role = member.role.to_lowercase();
    let skills = member.skills.join(" ").to_lowercase();
    let blob = format!("{role} {skills}");
    let has = |needle: &str| blob.contains(needle);

    if has("audit") || has("review") || has("security") || has("analysis") {
        return AgentTaskKind::Analysis;
    }
    if has("plan") || has("architect") || has("scout") {
        return AgentTaskKind::Planning;
    }
    if has("doc") || has("readme") || has("wiki") {
        return AgentTaskKind::Documentation;
    }
    if has("test") {
        return AgentTaskKind::Test;
    }
    if has("bug") || has("fix") {
        return AgentTaskKind::BugFix;
    }
    AgentTaskKind::Refactor
}

/// Two members "overlap" when any of their scope patterns shares a directory
/// prefix or one subsumes the other. Empty scopes overlap nothing: an
/// unrestricted role (a reviewer, typically) must not serialize behind every
/// concrete specialist or the run degenerates back to sequential.
fn scopes_overlap(a: &[String], b: &[String]) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let norm = |p: &str| p.trim_matches('/').replace('\\', "/");
    for pa in a {
        for pb in b {
            let pa = norm(pa);
            let pb = norm(pb);
            if pa.is_empty() || pb.is_empty() {
                continue;
            }
            if pa == pb || pa.starts_with(&pb) || pb.starts_with(&pa) {
                return true;
            }
            let pa_dir = pa.rsplit_once('/').map(|(head, _)| head).unwrap_or(&pa);
            let pb_dir = pb.rsplit_once('/').map(|(head, _)| head).unwrap_or(&pb);
            if pa_dir == pb_dir && !pa_dir.is_empty() {
                return true;
            }
        }
    }
    false
}

/// Build the DAG: one task per member, dependency edges where scopes overlap.
///
/// Members are visited in roster order — the lead is index 0 by the
/// `provision_team` convention, so any specialist that collides with the
/// lead's territory waits on the lead, not the other way round. That is the
/// intended ordering for a port sweep: the auditor reads the plan before
/// anyone writes to it.
///
/// The reconciler root (`TaskId(1)`) has all role tasks as dependencies; when
/// they all finish, [`complete_reconcile_root`] marks it Done and
/// [`OrchestratorRegistry::is_complete`] flips.
pub fn build_team_graph(prompt: &str, team: &ExpertTeam, files: &[String]) -> Vec<RoleTask> {
    let members = &team.members;
    let n = members.len();
    // Priority: roster order reversed so index 0 (the lead) has the highest
    // priority. Ties break by `ready_ids_conflict_aware`'s stable iteration,
    // which visits the priority-sorted `ready()` list top-down.
    let mut role_tasks: Vec<RoleTask> = Vec::with_capacity(n);
    // Per-member scope: the member's own patterns first, then any files the
    // caller handed to `team_dispatch` that match one of those patterns. The
    // pre-bound scope is what the mediator locks; the wider prompt context is
    // what the worker actually reads.
    let scope_for = |m: &ExpertMember| -> Vec<String> {
        let mut scope: Vec<String> = m.scope_patterns.clone();
        for f in files {
            let matches = m.scope_patterns.iter().any(|p| file_matches_glob(f, p));
            if matches && !scope.iter().any(|s| s == f) {
                scope.push(f.clone());
            }
        }
        scope
    };

    for (idx, member) in members.iter().enumerate() {
        let scope = scope_for(member);
        let title = format!("{} — {}", member.role, member.name);
        let description = if member.workflow_instructions.trim().is_empty() {
            prompt.to_string()
        } else {
            format!("{}\n\n{}\n", prompt, member.workflow_instructions)
        };
        let task_id = TaskId(idx as u64 + 2);
        let priority = (n - idx) as u32;
        let estimated_weight = if idx == 0 { 2 } else { 3 };
        let task = Task {
            id: task_id,
            title,
            description,
            scope,
            dependencies: Vec::new(),
            output: None,
            priority,
            estimated_weight,
        };
        let kind = task_kind_for_member(member);
        role_tasks.push(RoleTask {
            member_id: member.id.clone(),
            role: member.role.clone(),
            task,
            instructions: String::new(),
            task_kind: kind,
            provider: member.provider,
            provider_label: member.provider.label().to_string(),
            model_id: member.model_id.clone(),
            model_label: member.model_id.clone(),
            thinking: false,
            fallback_chain: Vec::new(),
        });
    }

    // Wire dependency edges across role tasks (indices 1..n in role_tasks).
    // Two role tasks that own overlapping territory must not race — the
    // mediator would still serialize them at the file level, but the second
    // would then waste an entire dispatch attempt on a lock refusal. Adding
    // the edge keeps the wave clean.
    for j in 1..role_tasks.len() {
        for i in 0..j {
            if scopes_overlap(&role_tasks[i].task.scope, &role_tasks[j].task.scope) {
                let dep_id = role_tasks[i].task.id;
                if !role_tasks[j].task.dependencies.contains(&dep_id) {
                    role_tasks[j].task.dependencies.push(dep_id);
                }
            }
        }
    }

    role_tasks
}

/// Assemble the [`TaskGraph`] the scheduler will consume.
///
/// Reconciler root at `TaskId(1)`, all role tasks as its dependencies — the
/// exact shape [`complete_reconcile_root`] inspects on the way out.
pub fn graph_from_role_tasks(goal: &str, role_tasks: &[RoleTask]) -> TaskGraph {
    let mut graph = TaskGraph::default();
    graph.root = TaskId(RECONCILE_ROOT_ID);
    graph.add(
        TaskId(RECONCILE_ROOT_ID),
        "Reconcile team plan",
        format!("Reconcile live-VC outputs for goal: {goal}"),
        vec![".velocity/agentic".to_string()],
        vec![],
        None,
    );
    for role_task in role_tasks {
        let t = &role_task.task;
        graph.add(
            t.id,
            t.title.clone(),
            t.description.clone(),
            t.scope.clone(),
            t.dependencies.clone(),
            Some(TaskId(RECONCILE_ROOT_ID)),
        );
        // `add` hardcodes priority/weight to 0/1; re-apply what the caller
        // chose so `ready()` sorts by roster order and the load-aware
        // scheduler sees a specialist as heavier than the lead.
        if let Some(node) = graph.tasks.get_mut(&t.id) {
            node.priority = t.priority;
            node.estimated_weight = t.estimated_weight;
        }
    }
    graph
}

/// Very small glob matcher: supports trailing `/**`, trailing `/`, trailing
/// `*`, exact paths and simple `foo/*.ext` patterns.
///
/// Full glob semantics are deliberately out of scope for P1 — a real glob
/// crate is on the P3 roadmap once the synthesiser starts emitting scoped
/// patterns from the LLM decomposition. What this covers is the shape
/// `provision_team` actually writes today.
fn file_matches_glob(file: &str, pattern: &str) -> bool {
    let f = file.replace('\\', "/");
    let p = pattern.replace('\\', "/");
    if p.is_empty() {
        return false;
    }
    if let Some(prefix) = p.strip_suffix("/**") {
        return f == prefix || f.starts_with(&format!("{prefix}/"));
    }
    if let Some(prefix) = p.strip_suffix('/') {
        return f.starts_with(&format!("{prefix}/"));
    }
    if p.ends_with("/*") {
        let prefix = p.trim_end_matches("/*");
        if let Some(rest) = f.strip_prefix(&format!("{prefix}/")) {
            return !rest.contains('/');
        }
        return false;
    }
    if p.contains('*') {
        // Single wildcard, no slash inside the pattern: split, both halves must match
        // and the middle (what the `*` stands for) must not cross a directory boundary.
        let (head, tail) = match p.split_once('*') {
            Some(pair) => pair,
            None => return false,
        };
        if !f.starts_with(head) || !f.ends_with(tail) || f.len() < head.len() + tail.len() {
            return false;
        }
        let middle = &f[head.len()..f.len() - tail.len()];
        return !middle.contains('/');
    }
    f == p
}

/// Fill in the fields the caller could not know until plan time.
///
/// `instructions` becomes the domain brief the worker actually receives; the
/// provider/model/fallback fields come from the caller's chain builder so
/// per-provider selection stays in one place (see `build_fallback_chain`).
#[allow(clippy::too_many_arguments)]
pub fn fill_assignment_context(
    role_tasks: &mut [RoleTask],
    team: &ExpertTeam,
    prompt: &str,
    files: &[String],
    chain_builder: &dyn Fn(&ExpertMember) -> Vec<RoutedModelRoute>,
    deliverable: &str,
    preload: &dyn Fn(&[String]) -> Vec<String>,
) {
    for role_task in role_tasks.iter_mut() {
        let member = team
            .members
            .iter()
            .find(|m| m.id == role_task.member_id)
            .cloned()
            .unwrap_or_else(|| ExpertMember {
                id: role_task.member_id.clone(),
                name: role_task.role.clone(),
                role: role_task.role.clone(),
                provider: role_task.provider,
                model_id: role_task.model_id.clone(),
                skills: Vec::new(),
                scope_patterns: role_task.task.scope.clone(),
                tools: Vec::new(),
                workflow_instructions: String::new(),
                fallback_provider: None,
            });
        let matching_files: Vec<String> = files
            .iter()
            .filter(|f| {
                member.scope_patterns.is_empty()
                    || member
                        .scope_patterns
                        .iter()
                        .any(|p| file_matches_glob(f, p))
            })
            .cloned()
            .collect();
        let contexts = preload(&matching_files);
        role_task.instructions = build_worker_brief(
            &member,
            team,
            prompt,
            &role_task.task.scope,
            &matching_files,
            &contexts,
            deliverable,
        );
        role_task.fallback_chain = chain_builder(&member);
    }
}

fn build_worker_brief(
    member: &ExpertMember,
    team: &ExpertTeam,
    prompt: &str,
    scope: &[String],
    files: &[String],
    contexts: &[String],
    deliverable: &str,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "You are {}, {} on the {} team, running as one specialist in a live-VC pipeline.\n",
        member.name, member.role, team.name
    ));
    if !member.skills.is_empty() {
        out.push_str(&format!("Expertise: {}\n", member.skills.join(", ")));
    }
    if !scope.is_empty() {
        out.push_str(&format!(
            "Your domain (own this; other members own other paths): {}\n",
            scope.join(", ")
        ));
    }
    out.push_str(&format!("\n## Task\n{}\n", prompt));
    if !files.is_empty() {
        out.push_str(&format!(
            "\n## Files you own in this run\n{}\n",
            files.join("\n")
        ));
    }
    if !contexts.is_empty() {
        out.push_str("\n## Target Files (pre-loaded)\n");
        for ctx in contexts {
            out.push_str(ctx);
            out.push('\n');
        }
    }
    out.push_str("\n## Constraints\n");
    out.push_str("- Work ONLY within your declared domain. Other specialists are editing other paths in parallel; touching their files burns a mediator lock.\n");
    out.push_str("- Do NOT create checkpoints, record events, or use meta-tools unless the task explicitly requires it.\n");
    out.push_str("- If the task requires a file outside your domain, STOP and report the blocker in the transcript; the reconciler will re-route it.\n");
    out.push_str(&format!(
        "- Deliverable format: {}. Do not return a different shape.\n",
        deliverable
    ));
    if !member.workflow_instructions.is_empty() {
        out.push_str(&format!("\n(Routing: {} → {})\n", team.name, member.name));
    }
    out
}

/// Drive the graph to completion.
///
/// Runs the panel's tick body without the paint step. Returns when
/// [`OrchestratorRegistry::is_complete`] flips, or when `deadline` elapses
/// with tasks still in flight — in which case those tasks are marked Failed
/// with a "wall clock spent" message rather than left Pending, so the report
/// the caller gets back is a *closed* plan.
///
/// `deadline` is `Duration::ZERO` for "unbounded". Every non-zero value comes
/// from the caller's `max_wall_secs` argument (0 in `MissionConfig` is
/// similarly uncapped), so a long-horizon port with no explicit cap runs
/// until it finishes.
///
/// Checkpoint: after every dispatch wave the pipeline state (graph, per-task
/// status, per-worker outputs, elapsed seconds) is snapshotted to
/// `<root>/.velocity/orchestrator/checkpoint.json`. If `ctx.resume` is true
/// and a matching checkpoint exists on disk (same `team_id` AND same prompt),
/// the registry starts from the restored state instead of all-Pending; tasks
/// that were Running when the previous process died reopen as Pending so the
/// wave below re-dispatches them. On clean completion the file is deleted so
/// an identical later dispatch correctly replans.
pub fn run_pipeline(
    workspace_root: &Path,
    graph: &mut TaskGraph,
    role_tasks: &[RoleTask],
    planned_site_map_root: u64,
    weight_root: u64,
    deadline: Duration,
    ctx: &PipelineRunCtx<'_>,
) -> PipelineOutcome {
    let started = Instant::now();
    let mut registry = OrchestratorRegistry::new(graph);
    // Optional restore-before-loop. Only fires when the caller asked to
    // resume AND the on-disk checkpoint identifies the same team + prompt.
    let restored_elapsed = if ctx.resume {
        read_checkpoint(ctx.root)
            .filter(|cp| cp_matches(cp, ctx.team_id, ctx.prompt))
            .map(|cp| {
                let r = cp::restore_registry(&cp, graph);
                registry = r;
                Duration::from_secs(cp.elapsed_secs)
            })
            .unwrap_or(Duration::ZERO)
    } else {
        Duration::ZERO
    };
    let mut role_members: HashMap<u64, (String, String)> = HashMap::new();
    for rt in role_tasks {
        role_members.insert(rt.task.id.0, (rt.member_id.clone(), rt.role.clone()));
    }
    let mediator = Arc::new(MediatorArena::new());
    let mut handles: HashMap<TaskId, Box<dyn WorkerHandle>> = HashMap::new();
    let mut by_id: HashMap<TaskId, &RoleTask> = HashMap::new();
    for rt in role_tasks {
        by_id.insert(rt.task.id, rt);
    }
    // A fresh assignment map so the run loop can consume role tasks without
    // borrowing `role_tasks` for the whole tick (the spawn closure needs
    // ownership of the `WorkerAssignment`).
    let mut assignment_by_id: HashMap<TaskId, WorkerAssignment> = HashMap::new();
    for rt in role_tasks {
        assignment_by_id.insert(
            rt.task.id,
            WorkerAssignment {
                task: rt.task.clone(),
                task_kind: rt.task_kind,
                workspace_root: workspace_root.to_path_buf(),
                instructions: rt.instructions.clone(),
                planned_site_map_root,
                provider: rt.provider,
                provider_label: rt.provider_label.clone(),
                model_id: rt.model_id.clone(),
                model_label: rt.model_label.clone(),
                thinking: rt.thinking,
                fallback_chain: rt.fallback_chain.clone(),
                scoped_files: None,
            },
        );
    }

    // A resumed run has to keep the deadline accounting honest: the previous
    // process spent `restored_elapsed` before it died, so the check below
    // treats `started.elapsed() + restored_elapsed` as the wall clock. Zero
    // deadline still means unbounded.
    let effective_elapsed =
        |now: Instant| -> Duration { now.duration_since(started) + restored_elapsed };
    let trigger_str: &'static str = trigger_label(ctx.trigger);
    let max_wall_secs: u64 = deadline.as_secs();
    let mut wave: u32 = 0;

    loop {
        // 0. Publish the current plan snapshot and fold any mid-run amendment
        // into it (no-op for a synchronous run with no live handle).
        publish_and_drain(
            ctx.live,
            graph,
            &registry,
            &by_id,
            &mut handles,
            &mut assignment_by_id,
            wave,
            effective_elapsed(Instant::now()),
        );
        wave += 1;

        // 1. Poll running handles → mark terminal statuses.
        let mut finished: Vec<TaskId> = Vec::new();
        for (id, handle) in handles.iter_mut() {
            if let Some(result) = handle.poll() {
                finalize_task_result(graph, &mut registry, &by_id, *id, result);
                finished.push(*id);
            }
        }
        for id in &finished {
            handles.remove(id);
        }

        // 2. Blocked-dependent propagation and reconciler completion.
        propagate_blocked_dependents(graph, &mut registry);
        complete_reconcile_root(graph, &mut registry);

        if registry.is_complete() {
            // Clean finish — drop the checkpoint so an identical later
            // dispatch correctly starts from scratch.
            clear_checkpoint(ctx.root);
            break;
        }

        // 3. Deadline check — mark every unfinished task Failed so the
        // report is closed rather than half Pending. A resumed run counts
        // the previous process's elapsed seconds against the same cap.
        if !deadline.is_zero() && effective_elapsed(Instant::now()) >= deadline {
            let spent = effective_elapsed(Instant::now());
            for id in collect_unfinished(&registry) {
                let message = format!(
                    "wall clock spent ({}s elapsed, deadline {}s) — task did not finish",
                    spent.as_secs(),
                    deadline.as_secs()
                );
                let mut result = registry
                    .statuses
                    .get(&id)
                    .and_then(|s| match s {
                        TaskStatus::Running => Some(detached_result(id, &message)),
                        _ => None,
                    })
                    .unwrap_or_else(|| detached_result(id, &message));
                result.success = false;
                registry.statuses.insert(id, TaskStatus::Failed(result));
            }
            for (_, mut handle) in handles.drain() {
                let _ = handle.cancel();
            }
            // Re-run the reconcile pass: root now sees Failed deps and stops
            // blocking, so `is_complete` will flip on the next tick.
            propagate_blocked_dependents(graph, &mut registry);
            complete_reconcile_root(graph, &mut registry);
            let snap = cp_snapshot(
                graph,
                &registry,
                &role_members,
                trigger_str,
                ctx.team_id,
                ctx.team_name,
                ctx.prompt,
                max_wall_secs,
                planned_site_map_root,
                weight_root,
                effective_elapsed(Instant::now()),
            );
            let _ = write_checkpoint(ctx.root, &snap);
            break;
        }

        // 4. Ready wave — conflict-aware when more than one task could start,
        // matching the panel exactly.
        let all_ready = registry.ready_ids(graph);
        let ready_ids: Vec<TaskId> = if all_ready.len() > 1 {
            let groups = crate::registry::event_store::conflict_file_groups(workspace_root);
            registry.ready_ids_conflict_aware(graph, &groups)
        } else {
            all_ready
        };

        // 5. Spawn workers for ready role tasks (the reconciler root is not
        // dispatched; `complete_reconcile_root` handles it).
        for id in ready_ids {
            if id == graph.root {
                continue;
            }
            let Some(assignment) = assignment_by_id.remove(&id) else {
                continue;
            };
            // Freshness: reopen the site map and compare against the planned
            // root. Same check the panel runs at dispatch time, same message
            // shape so downstream tooling that pattern-matches "stale routed
            // plan:" still fires.
            let site_map_path = workspace_root.join(".velocity").join("site_map");
            let current_root = SiteMap::open(&site_map_path, weight_root).map(|sm| sm.root());
            match current_root {
                Ok(root) if root == assignment.planned_site_map_root => {}
                Ok(root) => {
                    let message = format!(
                        "stale routed plan: planned SiteMap root {:016x} but current root is {:016x}",
                        assignment.planned_site_map_root, root
                    );
                    let mut result = detached_result(id, &message);
                    result.success = false;
                    registry.statuses.insert(id, TaskStatus::Blocked(result));
                    continue;
                }
                Err(err) => {
                    let mut result = detached_result(id, &format!("site map unavailable: {err}"));
                    result.success = false;
                    registry.statuses.insert(id, TaskStatus::Blocked(result));
                    continue;
                }
            }
            let handle = spawn_live_worker(assignment, mediator.clone(), weight_root);
            registry.statuses.insert(id, TaskStatus::Running);
            handles.insert(id, handle);
        }

        // 6. Wave boundary: snapshot the pipeline state so an MCP restart
        // resumes from here instead of replanning. Writing every wave means
        // an in-flight worker never gets re-spawned twice after a restart
        // (its id is `Running` in the last snapshot, which the restore path
        // reopens as Pending — but only for tasks that were mid-flight when
        // the process died; already-Done tasks stay Done).
        let snap = cp_snapshot(
            graph,
            &registry,
            &role_members,
            trigger_str,
            ctx.team_id,
            ctx.team_name,
            ctx.prompt,
            max_wall_secs,
            planned_site_map_root,
            weight_root,
            effective_elapsed(Instant::now()),
        );
        let _ = write_checkpoint(ctx.root, &snap);

        // 7. Nothing running and nothing ready means the plan cannot make
        // further progress on its own: either a Failed dep has left a
        // Pending task unreducible (propagate_blocked should have caught it
        // but does not, for a Failed whose scope is disjoint) or the wave is
        // empty because every candidate lost the conflict narrowing and just
        // deferred. Sleep so a defer does not busy-loop, then try again.
        if handles.is_empty() {
            std::thread::sleep(Duration::from_millis(25));
        } else {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    summarize(registry, graph, by_id, effective_elapsed(Instant::now()))
}

/// Bundles what `run_pipeline` needs to persist and resume state — separated
/// from the graph/registry arguments so P4 (amend) and P5 (observability) can
/// reach the same on-disk checkpoint without threading more positional args.
pub struct PipelineRunCtx<'a> {
    pub root: &'a Path,
    pub team_id: &'a str,
    pub team_name: &'a str,
    pub prompt: &'a str,
    pub trigger: PipelineTrigger,
    /// When true and a checkpoint matches (`team_id`, `prompt`), restore the
    /// registry from it instead of starting from all-Pending.
    pub resume: bool,
    /// The shared handle for a background run. `Some` means the loop publishes a
    /// [`RunSnapshot`] every wave and drains an amend directive inbox; `None`
    /// (the synchronous default) leaves the loop byte-for-byte as it was, so the
    /// crucible's blocking `team_dispatch` sees no behaviour change.
    pub live: Option<&'a LiveRun>,
}

/// Flat JSON label for a [`TaskStatus`] — the observability read-out and the
/// checkpoint writer must agree on these five strings.
pub fn status_label(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "pending",
        TaskStatus::Running => "running",
        TaskStatus::Done(_) => "done",
        TaskStatus::Failed(_) => "failed",
        TaskStatus::Blocked(_) => "blocked",
    }
}

/// Publish the plan snapshot and fold any pending amendment into the run.
///
/// Returns immediately when `live` is `None`, which is why the synchronous path
/// is unaffected: the whole mechanism is gated behind one argument the blocking
/// dispatcher never sets.
///
/// Two effects each wave:
/// 1. Overwrite [`LiveRun::snapshot`] so a P5 `inspect_task_graph` read sees the
///    real-time graph (ids, owner, scope, status) without touching the driver's
///    own registry.
/// 2. Drain [`LiveRun::inbox`] and hand each directive to the workers its scope
///    reaches — an in-flight handle via `send_note` (the loop turns it into a
///    high-priority steer), a not-yet-dispatched assignment by folding the note
///    into its brief so the worker meets the amendment on its first turn. Every
///    hand-off is written to the delivery ledger, which is what "did the amend
///    reach every in-flight assignment" is actually checked against.
///
/// `pub(crate)` so the acceptance test in `team_tools` can drive this exact
/// wave-head step with a controlled roster — the same call `run_pipeline` makes
/// at the head of every wave — without spawning a real model.
pub(crate) fn publish_and_drain(
    live: Option<&LiveRun>,
    graph: &TaskGraph,
    registry: &OrchestratorRegistry,
    by_id: &HashMap<TaskId, &RoleTask>,
    handles: &mut HashMap<TaskId, Box<dyn WorkerHandle>>,
    assignment_by_id: &mut HashMap<TaskId, WorkerAssignment>,
    wave: u32,
    elapsed: Duration,
) {
    let Some(live) = live else {
        return;
    };

    // 1. Publish the snapshot.
    let mut views: Vec<TaskView> = Vec::new();
    for (id, status) in registry.statuses.iter() {
        if *id == graph.root {
            continue;
        }
        let rt = by_id.get(id).copied();
        views.push(TaskView {
            task_id: id.0,
            member_id: rt.map(|r| r.member_id.clone()).unwrap_or_default(),
            role: rt.map(|r| r.role.clone()).unwrap_or_default(),
            scope: rt.map(|r| r.task.scope.clone()).unwrap_or_default(),
            status: status_label(status).to_string(),
        });
    }
    views.sort_by_key(|v| v.task_id);
    {
        let mut snap = live.snapshot.lock().unwrap();
        snap.wave = wave;
        snap.elapsed_secs = elapsed.as_secs();
        snap.tasks = views;
    }

    // 2. Drain the inbox.
    let drained = live.inbox.lock().unwrap().take_pending();
    if drained.is_empty() {
        return;
    }
    // Build the steerable roster once. A task is Running while its handle is
    // live, Pending while it still has an unspawned assignment, and Terminal
    // otherwise (the planner drops a Terminal entry, but we skip building it).
    let roster: Vec<RosterEntry> = registry
        .statuses
        .iter()
        .filter(|(id, _)| **id != graph.root)
        .filter_map(|(id, status)| {
            let state = if handles.contains_key(id) {
                WorkerState::Running
            } else if assignment_by_id.contains_key(id) {
                WorkerState::Pending
            } else {
                WorkerState::Terminal
            };
            if state == WorkerState::Terminal {
                return None;
            }
            let _ = status;
            let rt = by_id.get(id).copied();
            Some(RosterEntry {
                task_id: id.0,
                role: rt.map(|r| r.role.clone()).unwrap_or_default(),
                scope: rt.map(|r| r.task.scope.clone()).unwrap_or_default(),
                state,
            })
        })
        .collect();

    for planned in plan_directive_deliveries(&drained, &roster) {
        let id = TaskId(planned.task_id);
        let delivered = if let Some(handle) = handles.get_mut(&id) {
            handle.send_note(planned.note.clone())
        } else if let Some(assignment) = assignment_by_id.get_mut(&id) {
            assignment.instructions.push_str("\n\n");
            assignment.instructions.push_str(&planned.note);
            true
        } else {
            false
        };
        if delivered {
            live.inbox
                .lock()
                .unwrap()
                .record_delivery(planned.directive_id, planned.task_id);
        }
    }
}

/// Mark the live run's terminal counts on the shared status slot. No-op for a
/// synchronous run.
pub fn finish_live_run(live: Option<&LiveRun>, outcome: &PipelineOutcome) {
    if let Some(live) = live {
        *live.status.lock().unwrap() = RunStatus::Finished {
            succeeded: outcome.succeeded,
            failed: outcome.failed,
            blocked: outcome.blocked,
            wall_secs: outcome.wall_elapsed.as_secs(),
        };
    }
}

/// Mark a polled result Done/Failed/Blocked using the same rules as the panel:
/// success + validator.ok + no reconciliation collision + no follow-up flag →
/// Done; follow-up or reconciliation error → Blocked; anything else → Failed.
fn finalize_task_result(
    graph: &mut TaskGraph,
    registry: &mut OrchestratorRegistry,
    by_id: &HashMap<TaskId, &RoleTask>,
    id: TaskId,
    result: WorkerResult,
) {
    let outputs = task_result_outputs(&result);
    let workspace_root = by_id
        .get(&id)
        .map(|rt| rt.task.scope.clone())
        .unwrap_or_default();
    let _ = &workspace_root;
    let report = crate::orchestrator::validator::validate(&result);
    let recon = reconciliation_error(graph, &registry.outputs, id, &outputs);
    let needs_follow_up = requires_follow_up(&result);
    if result.success && report.ok && recon.is_none() && !needs_follow_up {
        registry.outputs.insert(id, outputs);
        if let Some(task) = graph.tasks.get_mut(&id) {
            task.output = Some(result.message.clone());
        }
        registry.statuses.insert(id, TaskStatus::Done(result));
    } else if needs_follow_up || recon.is_some() {
        let mut result = result;
        if let Some(msg) = recon {
            result.message = msg;
            result.status_updates.push(result.message.clone());
        }
        result.success = false;
        registry.statuses.insert(id, TaskStatus::Blocked(result));
    } else {
        registry.statuses.insert(id, TaskStatus::Failed(result));
    }
}

fn detached_result(id: TaskId, message: &str) -> WorkerResult {
    let mut result = WorkerResult {
        success: false,
        task_id: id,
        outputs: Vec::new(),
        duration: Duration::ZERO,
        message: message.to_string(),
        provider_label: String::new(),
        model_label: String::new(),
        transcript: String::new(),
        status_updates: vec![message.to_string()],
        attempts: Vec::new(),
        created_files: Vec::new(),
        deleted_files: Vec::new(),
        out_of_scope_created_files: Vec::new(),
        run_summary_path: None,
        run_facts_path: None,
        wa_run_path: None,
        wa_run_id: None,
        is_read_only: false,
    };
    result.message = message.to_string();
    result
}

fn collect_unfinished(registry: &OrchestratorRegistry) -> Vec<TaskId> {
    registry
        .statuses
        .iter()
        .filter(|(_, s)| matches!(s, TaskStatus::Pending | TaskStatus::Running))
        .map(|(id, _)| *id)
        .collect()
}

fn summarize(
    registry: OrchestratorRegistry,
    graph: &TaskGraph,
    by_id: HashMap<TaskId, &RoleTask>,
    elapsed: Duration,
) -> PipelineOutcome {
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut blocked = 0usize;
    let mut tasks: Vec<TaskOutcome> = Vec::new();
    // Sort by task id so the JSON response is deterministic across runs and
    // the tests do not need to filter by id.
    let mut ids: Vec<TaskId> = registry.statuses.keys().copied().collect();
    ids.sort_by_key(|t| t.0);
    for id in ids {
        if id == graph.root {
            continue;
        }
        let Some(status) = registry.statuses.get(&id) else {
            continue;
        };
        let (label, success, message, result_ref) = match status {
            TaskStatus::Pending => ("pending", false, "not started".to_string(), None),
            TaskStatus::Running => ("running", false, "still in flight".to_string(), None),
            TaskStatus::Done(r) => ("done", true, r.message.clone(), Some(r)),
            TaskStatus::Failed(r) => ("failed", false, r.message.clone(), Some(r)),
            TaskStatus::Blocked(r) => ("blocked", false, r.message.clone(), Some(r)),
        };
        match status {
            TaskStatus::Done(_) => succeeded += 1,
            TaskStatus::Failed(_) => failed += 1,
            TaskStatus::Blocked(_) => blocked += 1,
            _ => {}
        }
        let rt = by_id.get(&id).copied();
        tasks.push(TaskOutcome {
            task_id: id.0,
            member_id: rt.map(|r| r.member_id.clone()).unwrap_or_default(),
            role: rt.map(|r| r.role.clone()).unwrap_or_default(),
            status: label,
            success,
            message,
            outputs: result_ref.map(task_result_outputs).unwrap_or_default(),
            created_files: result_ref
                .map(|r| r.created_files.clone())
                .unwrap_or_default(),
            provider_label: result_ref
                .map(|r| r.provider_label.clone())
                .unwrap_or_default(),
            model_label: result_ref
                .map(|r| r.model_label.clone())
                .unwrap_or_default(),
            duration_secs: result_ref.map(|r| r.duration.as_secs()).unwrap_or_default(),
        });
    }
    PipelineOutcome {
        // Filled in by the caller — this function does not see the trigger.
        trigger: PipelineTrigger::TeamScope,
        rationale: String::new(),
        tasks,
        succeeded,
        failed,
        blocked,
        wall_elapsed: elapsed,
    }
}

/// Serialize the pipeline outcome into the JSON shape `team_dispatch` returns.
pub fn outcome_to_json(outcome: &PipelineOutcome) -> serde_json::Value {
    use serde_json::json;
    json!({
        "pipeline": trigger_label(outcome.trigger),
        "rationale": outcome.rationale,
        "succeeded": outcome.succeeded,
        "failed": outcome.failed,
        "blocked": outcome.blocked,
        "wall_secs": outcome.wall_elapsed.as_secs(),
        "tasks": outcome.tasks.iter().map(|t| json!({
            "task_id": t.task_id,
            "member_id": t.member_id,
            "role": t.role,
            "status": t.status,
            "success": t.success,
            "message": t.message,
            "outputs": t.outputs,
            "created_files": t.created_files,
            "provider": t.provider_label,
            "model": t.model_label,
            "duration_secs": t.duration_secs,
        })).collect::<Vec<_>>(),
    })
}

/// Files the caller passed in, but only those the role owns.
///
/// Re-exported so `team_dispatch` can hand the pipeline the same scoped-file
/// list it hands the pre-injection loader.
pub fn scoped_files_for_root(root: &Path, files: &[String]) -> Vec<PathBuf> {
    files.iter().map(|f| root.join(f)).collect()
}

// Silence dead-code warnings for helpers reserved for P2/P4 wiring.
#[allow(dead_code)]
fn touch_hashset_marker(_: &HashSet<String>) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::intent::{classify, RoleSpec};
    use crate::editor::expert_team::ExpertTeam;

    fn team_with(roles: &[(&str, &[&str])]) -> ExpertTeam {
        let members: Vec<ExpertMember> = roles
            .iter()
            .enumerate()
            .map(|(i, (role, scope))| ExpertMember {
                id: format!("member_{i}"),
                name: role.to_string(),
                role: role.to_string(),
                provider: AiProvider::CloudflareWorkersAi,
                model_id: "test-model".to_string(),
                skills: vec![],
                scope_patterns: scope.iter().map(|s| s.to_string()).collect(),
                tools: vec![],
                workflow_instructions: String::new(),
                fallback_provider: None,
            })
            .collect();
        ExpertTeam {
            id: "team_demo".to_string(),
            name: "Demo Team".to_string(),
            description: String::new(),
            members,
            is_preset: false,
        }
    }

    #[test]
    fn pipeline_trigger_is_none_for_short_single_agent() {
        let decision = classify("rename the `foo` local in src/worker.rs to `bar`");
        assert_eq!(decision.scope, TaskScope::SingleAgent);
        assert_eq!(decision.horizon, TaskHorizon::Short);
        assert!(pipeline_trigger(&decision).is_none());
    }

    #[test]
    fn pipeline_trigger_fires_for_team_or_long() {
        let team_decision = classify("build me a team to port the entire repo to rust");
        assert!(matches!(
            pipeline_trigger(&team_decision),
            Some(PipelineTrigger::TeamScope)
        ));
        // A long-horizon verdict on a single-agent scope is possible: score
        // stays under TEAM_THRESHOLD but horizon_score clears it. Verify the
        // second arm — a whole-repo rewrite that happens to be phrased
        // narrowly still fires Long.
        let long_decision = IntentDecision {
            scope: TaskScope::SingleAgent,
            requires_repo_ingest: false,
            repo_url: None,
            target_language: Some("rust".to_string()),
            roles: Vec::new(),
            rationale: "test fixture".to_string(),
            confidence: 0.9,
            signals: vec![],
            horizon: TaskHorizon::Long,
            wants_unattended: true,
        };
        assert!(matches!(
            pipeline_trigger(&long_decision),
            Some(PipelineTrigger::LongHorizon)
        ));
    }

    #[test]
    fn build_team_graph_emits_one_task_per_member() {
        let team = team_with(&[
            ("Lead Architect", &["src/"]),
            ("Backend Engineer", &["src/db/", "src/net/"]),
            ("Frontend Engineer", &["web/"]),
        ]);
        let role_tasks = build_team_graph("port everything", &team, &[]);
        assert_eq!(role_tasks.len(), 3);
        assert_eq!(role_tasks[0].task.id, TaskId(2));
        assert_eq!(role_tasks[1].task.id, TaskId(3));
        assert_eq!(role_tasks[2].task.id, TaskId(4));
        assert_eq!(role_tasks[0].role, "Lead Architect");
        // Priority: lead highest, monotonically decreasing with roster index.
        assert!(role_tasks[0].task.priority > role_tasks[1].task.priority);
        assert!(role_tasks[1].task.priority > role_tasks[2].task.priority);
    }

    #[test]
    fn scope_overlap_adds_dependency_edge() {
        // The Backend task owns `src/db/`, the Lead owns `src/`. Because
        // "src" is a prefix of "src/db", Backend must depend on Lead — the
        // lead lays down the plan before anyone writes.
        let team = team_with(&[
            ("Lead", &["src/"]),
            ("Backend", &["src/db/pool.rs"]),
            ("Frontend", &["web/app.tsx"]),
        ]);
        let role_tasks = build_team_graph("port everything", &team, &[]);
        assert!(role_tasks[0].task.dependencies.is_empty());
        assert_eq!(role_tasks[1].task.dependencies, vec![TaskId(2)]);
        assert!(role_tasks[2].task.dependencies.is_empty());
    }

    #[test]
    fn empty_scopes_never_overlap() {
        // A reviewer with no declared scope runs alongside everybody. If it
        // serialized behind each specialist the run would degrade to
        // sequential — exactly what the live-VC path promises not to do.
        let team = team_with(&[
            ("Lead", &["src/"]),
            ("Reviewer", &[]),
            ("Backend", &["src/"]),
        ]);
        let role_tasks = build_team_graph("port everything", &team, &[]);
        assert!(role_tasks[1].task.dependencies.is_empty());
        assert_eq!(role_tasks[2].task.dependencies, vec![TaskId(2)]);
    }

    #[test]
    fn graph_from_role_tasks_wires_reconcile_root() {
        let team = team_with(&[("Lead", &["src/"]), ("Backend", &["src/db/"])]);
        let role_tasks = build_team_graph("port everything", &team, &[]);
        let graph = graph_from_role_tasks("port everything", &role_tasks);
        assert_eq!(graph.root, TaskId(RECONCILE_ROOT_ID));
        // Root depends on both specialists — the same shape `complete_reconcile_root` inspects.
        let root = graph.tasks.get(&TaskId(1)).unwrap();
        assert!(root.dependencies.contains(&TaskId(2)));
        assert!(root.dependencies.contains(&TaskId(3)));
        // Specialist deps survive the copy: Backend still waits on Lead.
        let backend = graph.tasks.get(&TaskId(3)).unwrap();
        assert_eq!(backend.dependencies, vec![TaskId(2)]);
    }

    #[test]
    fn task_kind_mapping_prefers_role_and_skills() {
        let mk = |role: &str, skills: &[&str]| ExpertMember {
            id: "m".into(),
            name: role.into(),
            role: role.into(),
            provider: AiProvider::CloudflareWorkersAi,
            model_id: String::new(),
            skills: skills.iter().map(|s| s.to_string()).collect(),
            scope_patterns: vec![],
            tools: vec![],
            workflow_instructions: String::new(),
            fallback_provider: None,
        };
        assert_eq!(
            task_kind_for_member(&mk("Security Auditor", &["security"])),
            AgentTaskKind::Analysis
        );
        assert_eq!(
            task_kind_for_member(&mk("Docs Engineer", &["documentation"])),
            AgentTaskKind::Documentation
        );
        assert_eq!(
            task_kind_for_member(&mk("Test Engineer", &["test"])),
            AgentTaskKind::Test
        );
        assert_eq!(
            task_kind_for_member(&mk("Port Engineer", &["rust", "port"])),
            AgentTaskKind::Refactor
        );
    }

    #[test]
    fn file_matches_glob_supports_prefix_and_ext_patterns() {
        assert!(file_matches_glob("src/db/pool.rs", "src/**"));
        assert!(file_matches_glob("src/db/pool.rs", "src/"));
        assert!(file_matches_glob("src/db/pool.rs", "src/db/"));
        assert!(!file_matches_glob("web/app.tsx", "src/**"));
        assert!(file_matches_glob("src/main.rs", "src/*.rs"));
        assert!(!file_matches_glob("src/db/pool.rs", "src/*.rs"));
        assert!(file_matches_glob("pool.rs", "*.rs"));
        assert!(file_matches_glob("src/main.rs", "src/main.rs"));
    }

    // ── Non-vacuity of the branch guard ──────────────────────────────────
    //
    // The `pipeline_trigger` classifier IS the guard, and its inputs are the
    // pure `classify()` scorer. Proving the guard is non-vacuous therefore
    // means proving the scorer still distinguishes team-scope from
    // single-agent — which the two tests above (`short_single_agent` → None,
    // `team_or_long` → Some) already do from opposite directions. If the
    // branch condition is byte-reverted to `false`, the second test's
    // `matches!(… TeamScope)` assertion fails; if it is reverted to
    // unconditional `Some`, the first test's `is_none()` fails. The pair is
    // the non-vacuity check, and it lives here so the guard is exercised in
    // the same module that consumes it.
    #[test]
    fn guard_is_pairwise_non_vacuous() {
        // Deliberate belt-and-braces: assert the pair together, so a future
        // regression that quietly swaps the arms of `pipeline_trigger` is
        // caught even if someone edits one of the two tests above.
        let focused = classify("rename `foo` in src/worker.rs");
        let sprawling = classify("port the entire repository to rust, run it end to end");
        let focused_trigger = pipeline_trigger(&focused);
        let sprawling_trigger = pipeline_trigger(&sprawling);
        assert!(
            focused_trigger.is_none(),
            "a focused single-file rename must NOT route through the pipeline"
        );
        assert!(
            sprawling_trigger.is_some(),
            "a whole-repo port MUST route through the pipeline (scope={:?}, horizon={:?})",
            sprawling.scope,
            sprawling.horizon
        );
    }

    // Silences the "field never read" warning for RoleSpec on this test
    // module while keeping the type imported.
    #[allow(dead_code)]
    fn _role_spec_marker(_r: &RoleSpec) {}
}
