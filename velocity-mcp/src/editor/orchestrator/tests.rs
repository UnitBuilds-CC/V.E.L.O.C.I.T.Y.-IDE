use super::panel::authoring::TaskDraft;
use super::panel::OrchestratorPanel;
use super::types::*;
use crate::automation::{AgentTaskKind, DecompositionStyle, RoutedSubAgentTask};
use crate::orchestrator::blueprint::TaskGraph;
use crate::orchestrator::registry::{OrchestratorRegistry, TaskStatus};
use crate::orchestrator::worker::{WorkerHandle, WorkerResult, WorkerThreadSnapshot};
use crate::orchestrator::{scheduler, TaskId};

struct StubWorkerHandle {
    snapshot: WorkerThreadSnapshot,
    cancelled: bool,
    notes: Vec<String>,
}

impl WorkerHandle for StubWorkerHandle {
    fn poll(&mut self) -> Option<WorkerResult> {
        None
    }

    fn cancel(&mut self) -> bool {
        self.cancelled = true;
        true
    }

    fn send_note(&mut self, note: String) -> bool {
        self.notes.push(note.clone());
        self.snapshot.operator_notes.push(note.clone());
        self.snapshot
            .events
            .push(crate::orchestrator::worker::WorkerThreadEvent {
                kind: crate::orchestrator::worker::WorkerThreadEventKind::OperatorNote,
                message: note,
            });
        self.snapshot
            .events
            .push(crate::orchestrator::worker::WorkerThreadEvent {
                kind: crate::orchestrator::worker::WorkerThreadEventKind::Status,
                message: "Operator note routed to this worker thread.".to_string(),
            });
        self.snapshot
            .status_updates
            .push("Operator note routed to this worker thread.".to_string());
        true
    }

    fn snapshot(&self) -> WorkerThreadSnapshot {
        self.snapshot.clone()
    }
}

/// One routed task, as the planner would hand it to the panel.
fn routed_task(task_id: &str, planned_root: u64) -> RoutedSubAgentTask {
    RoutedSubAgentTask {
        task_id: task_id.to_string(),
        task_kind: AgentTaskKind::Refactor,
        planned_site_map_root: planned_root,
        files: vec![std::path::PathBuf::from("src/main.rs")],
        provider: crate::agent::AiProvider::CloudflareWorkersAi,
        model_id: "routed-model".to_string(),
        model_label: "routed-model".to_string(),
        thinking: false,
        fallback_chain: Vec::new(),
        execution_contract: String::new(),
        summary: String::new(),
        rationale: String::new(),
        decomposition_policy_id: String::new(),
        decomposition_style: DecompositionStyle::CoupledComponents,
    }
}

fn sample_result(task_id: TaskId, message: &str) -> WorkerResult {
    WorkerResult {
        success: false,
        task_id,
        outputs: Vec::new(),
        duration: std::time::Duration::ZERO,
        message: message.to_string(),
        provider_label: String::new(),
        model_label: String::new(),
        transcript: String::new(),
        status_updates: Vec::new(),
        attempts: Vec::new(),
        created_files: Vec::new(),
        deleted_files: Vec::new(),
        out_of_scope_created_files: Vec::new(),
        run_summary_path: None,
        run_facts_path: None,
        wa_run_path: None,
        wa_run_id: None,
        is_read_only: false,
    }
}

#[test]
fn follow_up_detection_matches_mediation_and_reconciliation() {
    assert!(requires_follow_up(&sample_result(
        TaskId(2),
        "MEDIATION CONTRACT:\nConflict Type: DIRECT LINE COLLISION"
    )));
    assert!(requires_follow_up(&sample_result(
        TaskId(2),
        "Reconciliation blocked: overlapping outputs detected"
    )));
    let mut out_of_scope = sample_result(TaskId(2), "provider call succeeded");
    out_of_scope
        .out_of_scope_created_files
        .push("docs/rogue.md".to_string());
    assert!(requires_follow_up(&out_of_scope));
    assert!(!requires_follow_up(&sample_result(
        TaskId(2),
        "provider call failed"
    )));
}

#[test]
fn blocked_tasks_propagate_to_dependents() {
    let mut graph = TaskGraph::default();
    graph.root = TaskId(1);
    graph.add(TaskId(1), "root", "root", vec![], vec![TaskId(2)], None);
    graph.add(TaskId(2), "child", "child", vec![], vec![], None);
    let mut registry = OrchestratorRegistry::new(&graph);
    registry.statuses.insert(
        TaskId(2),
        TaskStatus::Blocked(sample_result(TaskId(2), "MEDIATION CONTRACT:")),
    );

    propagate_blocked_dependents(&graph, &mut registry);

    assert!(matches!(
        registry.statuses.get(&TaskId(1)),
        Some(TaskStatus::Blocked(_))
    ));
}

#[test]
fn dependency_blocked_tasks_return_to_pending_when_dependencies_clear() {
    let mut graph = TaskGraph::default();
    graph.root = TaskId(1);
    graph.add(TaskId(1), "root", "root", vec![], vec![TaskId(2)], None);
    graph.add(TaskId(2), "child", "child", vec![], vec![], None);
    let mut registry = OrchestratorRegistry::new(&graph);
    registry.statuses.insert(
        TaskId(2),
        TaskStatus::Blocked(sample_result(TaskId(2), "MEDIATION CONTRACT:")),
    );

    propagate_blocked_dependents(&graph, &mut registry);
    assert!(matches!(
        registry.statuses.get(&TaskId(1)),
        Some(TaskStatus::Blocked(_))
    ));

    registry.statuses.insert(
        TaskId(2),
        TaskStatus::Done(WorkerResult::new(graph.tasks.get(&TaskId(2)).unwrap())),
    );
    propagate_blocked_dependents(&graph, &mut registry);

    assert!(matches!(
        registry.statuses.get(&TaskId(1)),
        Some(TaskStatus::Pending)
    ));
}

#[test]
fn stale_routed_plan_blocks_dispatch() {
    let temp = tempfile::tempdir().unwrap();
    let workspace_root = temp.path();
    let site_map_dir = workspace_root.join(".velocity").join("site_map");
    std::fs::create_dir_all(&site_map_dir).unwrap();
    let mut site_map = velocity_ide::site_map::SiteMap::open(&site_map_dir, 0).unwrap();
    site_map
        .put_node(&velocity_ide::site_map::NdaNode::Triple {
            subject_hash: 1,
            predicate_id: 2,
            object_hash: 2,
        })
        .unwrap();
    let current_root = site_map.root();

    let mut panel = OrchestratorPanel::new();
    panel.set_routed_tasks(
        "goal".to_string(),
        AgentTaskKind::Refactor,
        1,
        vec![RoutedSubAgentTask {
            task_id: "task-1".to_string(),
            task_kind: AgentTaskKind::Refactor,
            planned_site_map_root: current_root.wrapping_add(1),
            files: vec![std::path::PathBuf::from("src/main.rs")],
            provider: crate::agent::AiProvider::CloudflareWorkersAi,
            model_id: "model".to_string(),
            model_label: "model".to_string(),
            thinking: false,
            fallback_chain: Vec::new(),
            execution_contract: String::new(),
            summary: String::new(),
            rationale: String::new(),
            decomposition_policy_id: String::new(),
            decomposition_style: crate::automation::DecompositionStyle::CoupledComponents,
        }],
    );

    let mediator = std::sync::Arc::new(crate::automation::mediator::MediatorArena::new());
    panel.poll_live_workers(workspace_root, &mediator);

    let registry = panel.registry.as_ref().unwrap();
    assert!(matches!(
        registry.statuses.get(&TaskId(2)),
        Some(TaskStatus::Blocked(_))
    ));
    assert!(panel.running_workers.is_empty());
}

#[test]
fn retry_blocked_tasks_requeues_follow_up_blocks_only() {
    let temp = tempfile::tempdir().unwrap();
    let workspace_root = temp.path();
    let mediator = std::sync::Arc::new(crate::automation::mediator::MediatorArena::new());
    let mut panel = OrchestratorPanel::new();
    panel.graph = TaskGraph::default();
    panel.graph.root = TaskId(1);
    panel.graph.add(
        TaskId(1),
        "root",
        "root",
        vec![],
        vec![TaskId(2), TaskId(3)],
        None,
    );
    panel
        .graph
        .add(TaskId(2), "follow-up", "follow-up", vec![], vec![], None);
    panel
        .graph
        .add(TaskId(3), "stale", "stale", vec![], vec![], None);
    panel.registry = Some(OrchestratorRegistry::new(&panel.graph));

    let reg = panel.registry.as_mut().unwrap();
    reg.statuses.insert(
        TaskId(2),
        TaskStatus::Blocked(sample_result(TaskId(2), "MEDIATION CONTRACT:")),
    );
    reg.statuses.insert(TaskId(3), TaskStatus::Blocked(sample_result(TaskId(3), "stale routed plan: planned SiteMap root 0000000000000001 but current root is 0000000000000002")));

    panel.retry_blocked_tasks(workspace_root, &mediator);

    let reg = panel.registry.as_ref().unwrap();
    assert!(matches!(
        reg.statuses.get(&TaskId(2)),
        Some(TaskStatus::Pending)
    ));
    assert!(matches!(
        reg.statuses.get(&TaskId(3)),
        Some(TaskStatus::Blocked(_))
    ));
}

#[test]
fn reconcile_root_completes_after_successful_children() {
    let mut graph = build_routed_graph("goal", &[]);
    let mut registry = OrchestratorRegistry::new(&graph);
    registry.statuses.insert(TaskId(1), TaskStatus::Pending);
    complete_reconcile_root(&mut graph, &mut registry);
    assert!(matches!(
        registry.statuses.get(&TaskId(1)),
        Some(TaskStatus::Pending)
    ));

    let mut graph = build_routed_graph(
        "goal",
        &[RoutedSubAgentTask {
            task_id: "task-1".to_string(),
            task_kind: AgentTaskKind::Refactor,
            planned_site_map_root: 0,
            files: Vec::new(),
            provider: crate::agent::AiProvider::CloudflareWorkersAi,
            model_id: "model".to_string(),
            model_label: "model".to_string(),
            thinking: false,
            fallback_chain: Vec::new(),
            execution_contract: String::new(),
            summary: String::new(),
            rationale: String::new(),
            decomposition_policy_id: String::new(),
            decomposition_style: crate::automation::DecompositionStyle::CoupledComponents,
        }],
    );
    let mut registry = OrchestratorRegistry::new(&graph);
    registry.statuses.insert(
        TaskId(2),
        TaskStatus::Done(WorkerResult::new(graph.tasks.get(&TaskId(2)).unwrap())),
    );

    complete_reconcile_root(&mut graph, &mut registry);

    assert!(matches!(
        registry.statuses.get(&TaskId(1)),
        Some(TaskStatus::Done(_))
    ));
}

#[test]
fn send_task_note_routes_to_running_worker() {
    let mut panel = OrchestratorPanel::new();
    panel.running_workers.insert(
        TaskId(7),
        Box::new(StubWorkerHandle {
            snapshot: WorkerThreadSnapshot::default(),
            cancelled: false,
            notes: Vec::new(),
        }),
    );

    assert!(panel.send_task_note(TaskId(7), "Tighten validation".to_string()));
    assert_eq!(panel.runtime_status, "Sent operator note to task 7");
}

#[test]
fn dashboard_snapshot_includes_live_worker_thread() {
    let mut panel = OrchestratorPanel::new();
    panel.graph = TaskGraph::default();
    panel.graph.root = TaskId(1);
    panel
        .graph
        .add(TaskId(1), "root", "root", vec![], vec![TaskId(2)], None);
    panel.graph.add(
        TaskId(2),
        "worker",
        "worker",
        vec!["src/main.rs".to_string()],
        vec![],
        None,
    );
    panel.registry = Some(OrchestratorRegistry::new(&panel.graph));
    panel
        .registry
        .as_mut()
        .unwrap()
        .statuses
        .insert(TaskId(2), TaskStatus::Running);
    panel.running_workers.insert(
        TaskId(2),
        Box::new(StubWorkerHandle {
            snapshot: WorkerThreadSnapshot {
                events: vec![
                    crate::orchestrator::worker::WorkerThreadEvent {
                        kind: crate::orchestrator::worker::WorkerThreadEventKind::Status,
                        message: "Querying provider".to_string(),
                    },
                    crate::orchestrator::worker::WorkerThreadEvent {
                        kind: crate::orchestrator::worker::WorkerThreadEventKind::OperatorNote,
                        message: "Keep login flow".to_string(),
                    },
                ],
                status_updates: vec!["Querying provider".to_string()],
                transcript: "partial answer".to_string(),
                changed_files: vec!["src/main.rs".to_string()],
                operator_notes: vec!["Keep login flow".to_string()],
            },
            cancelled: false,
            notes: Vec::new(),
        }),
    );

    let snapshot = panel.dashboard_snapshot();
    let task = snapshot.tasks.iter().find(|task| task.id == 2).unwrap();
    let thread = task.live_thread.as_ref().unwrap();
    assert_eq!(thread.status_updates, vec!["Querying provider"]);
    assert_eq!(thread.operator_notes, vec!["Keep login flow"]);
    assert_eq!(thread.changed_files, vec!["src/main.rs"]);
    assert_eq!(thread.transcript, "partial answer");
    assert_eq!(thread.events.len(), 2);
    assert_eq!(task.run_summary_path, None);
    assert_eq!(task.run_facts_path, None);
    assert_eq!(task.wa_run_path, None);
    assert_eq!(task.wa_run_id, None);
}

#[test]
fn dashboard_snapshot_includes_worker_artifact_paths() {
    let mut panel = OrchestratorPanel::new();
    panel.graph = TaskGraph::default();
    panel.graph.root = TaskId(1);
    panel
        .graph
        .add(TaskId(1), "root", "root", vec![], vec![TaskId(2)], None);
    panel.graph.add(
        TaskId(2),
        "worker",
        "worker",
        vec!["src/main.rs".to_string()],
        vec![],
        None,
    );
    panel.registry = Some(OrchestratorRegistry::new(&panel.graph));

    let mut result = WorkerResult::new(panel.graph.tasks.get(&TaskId(2)).unwrap());
    result.provider_label = "Provider".to_string();
    result.model_label = "Model".to_string();
    result.run_summary_path = Some(std::path::PathBuf::from("C:\\temp\\summary.txt"));
    result.run_facts_path = Some(std::path::PathBuf::from("C:\\temp\\facts.nda"));
    result.wa_run_path = Some(".velocity/wa-runs/desktop-run.wa-run.nda".to_string());
    result.wa_run_id = Some("desktop-run".to_string());
    panel
        .registry
        .as_mut()
        .unwrap()
        .statuses
        .insert(TaskId(2), TaskStatus::Done(result));

    let snapshot = panel.dashboard_snapshot();
    let task = snapshot.tasks.iter().find(|task| task.id == 2).unwrap();
    assert_eq!(
        task.run_summary_path.as_deref(),
        Some("C:\\temp\\summary.txt")
    );
    assert_eq!(task.run_facts_path.as_deref(), Some("C:\\temp\\facts.nda"));
    assert_eq!(
        task.wa_run_path.as_deref(),
        Some(".velocity/wa-runs/desktop-run.wa-run.nda")
    );
    assert_eq!(task.wa_run_id.as_deref(), Some("desktop-run"));
}

#[test]
fn stop_task_updates_runtime_status() {
    let mut panel = OrchestratorPanel::new();
    panel.running_workers.insert(
        TaskId(3),
        Box::new(StubWorkerHandle {
            snapshot: WorkerThreadSnapshot::default(),
            cancelled: false,
            notes: Vec::new(),
        }),
    );

    assert!(panel.stop_task(TaskId(3)));
    assert_eq!(panel.runtime_status, "Stopping task 3");
}

#[test]
fn reset_task_clears_outputs_and_running_handle() {
    let mut panel = OrchestratorPanel::new();
    panel.graph = TaskGraph::default();
    panel.graph.root = TaskId(1);
    panel
        .graph
        .add(TaskId(1), "root", "root", vec![], vec![TaskId(2)], None);
    panel.graph.add(
        TaskId(2),
        "worker",
        "worker",
        vec!["src/main.rs".to_string()],
        vec![],
        None,
    );
    panel.registry = Some(OrchestratorRegistry::new(&panel.graph));
    let reg = panel.registry.as_mut().unwrap();
    reg.outputs
        .insert(TaskId(2), vec!["src/main.rs".to_string()]);
    reg.statuses.insert(
        TaskId(2),
        TaskStatus::Done(WorkerResult::new(
            panel.graph.tasks.get(&TaskId(2)).unwrap(),
        )),
    );
    panel.running_workers.insert(
        TaskId(2),
        Box::new(StubWorkerHandle {
            snapshot: WorkerThreadSnapshot::default(),
            cancelled: false,
            notes: Vec::new(),
        }),
    );

    assert!(panel.reset_task(TaskId(2)));
    let reg = panel.registry.as_ref().unwrap();
    assert!(matches!(
        reg.statuses.get(&TaskId(2)),
        Some(TaskStatus::Pending)
    ));
    assert!(!reg.outputs.contains_key(&TaskId(2)));
    assert!(!panel.running_workers.contains_key(&TaskId(2)));
    assert_eq!(panel.runtime_status, "Reset task 2 to pending");
}

#[test]
fn retry_task_requeues_retryable_blocked_task() {
    let temp = tempfile::tempdir().unwrap();
    let workspace_root = temp.path();
    let mediator = std::sync::Arc::new(crate::automation::mediator::MediatorArena::new());
    let mut panel = OrchestratorPanel::new();
    panel.graph = TaskGraph::default();
    panel.graph.root = TaskId(1);
    panel
        .graph
        .add(TaskId(1), "root", "root", vec![], vec![TaskId(2)], None);
    panel
        .graph
        .add(TaskId(2), "worker", "worker", vec![], vec![], None);
    panel.registry = Some(OrchestratorRegistry::new(&panel.graph));
    panel.registry.as_mut().unwrap().statuses.insert(
        TaskId(2),
        TaskStatus::Blocked(sample_result(TaskId(2), "MEDIATION CONTRACT: retry me")),
    );

    assert!(panel.retry_task(TaskId(2), workspace_root, &mediator));
    assert_eq!(panel.runtime_status, "Waiting for executable routed tasks");
    assert!(matches!(
        panel.registry.as_ref().unwrap().statuses.get(&TaskId(2)),
        Some(TaskStatus::Pending)
    ));
}

#[test]
fn runtime_status_expands_retry_waits() {
    let temp = tempfile::tempdir().unwrap();
    let workspace_root = temp.path();
    let mediator = std::sync::Arc::new(crate::automation::mediator::MediatorArena::new());
    let mut panel = OrchestratorPanel::new();
    panel.graph = TaskGraph::default();
    panel.graph.root = TaskId(2);
    panel
        .graph
        .add(TaskId(2), "worker", "worker", vec![], vec![], None);
    panel.registry = Some(OrchestratorRegistry::new(&panel.graph));
    panel.registry.as_mut().unwrap().statuses.insert(
        TaskId(2),
        TaskStatus::Blocked(sample_result(TaskId(2), "MEDIATION CONTRACT: retry me")),
    );

    panel.poll_live_workers(workspace_root, &mediator);
    assert_eq!(
        panel.runtime_status,
        "Waiting for retry on 1 blocked task(s)"
    );
}

#[test]
fn runtime_status_expands_stale_plan_waits() {
    let temp = tempfile::tempdir().unwrap();
    let workspace_root = temp.path();
    let mediator = std::sync::Arc::new(crate::automation::mediator::MediatorArena::new());
    let mut panel = OrchestratorPanel::new();
    panel.graph = TaskGraph::default();
    panel.graph.root = TaskId(2);
    panel
        .graph
        .add(TaskId(2), "worker", "worker", vec![], vec![], None);
    panel.registry = Some(OrchestratorRegistry::new(&panel.graph));
    panel.registry.as_mut().unwrap().statuses.insert(
        TaskId(2),
        TaskStatus::Blocked(sample_result(
            TaskId(2),
            "stale routed plan: planned SiteMap root 0000000000000001 but current root is 0000000000000002",
        )),
    );

    panel.poll_live_workers(workspace_root, &mediator);
    assert_eq!(
        panel.runtime_status,
        "Waiting for routed plan refresh on 1 blocked task(s)"
    );
}

#[test]
fn runtime_status_expands_mixed_blocked_waits() {
    let temp = tempfile::tempdir().unwrap();
    let workspace_root = temp.path();
    let mediator = std::sync::Arc::new(crate::automation::mediator::MediatorArena::new());
    let mut panel = OrchestratorPanel::new();
    panel.graph = TaskGraph::default();
    panel.graph.root = TaskId(1);
    panel.graph.add(
        TaskId(1),
        "root",
        "root",
        vec![],
        vec![TaskId(2), TaskId(3)],
        None,
    );
    panel
        .graph
        .add(TaskId(2), "retryable", "retryable", vec![], vec![], None);
    panel
        .graph
        .add(TaskId(3), "stale", "stale", vec![], vec![], None);
    panel.registry = Some(OrchestratorRegistry::new(&panel.graph));
    panel.registry.as_mut().unwrap().statuses.insert(
        TaskId(2),
        TaskStatus::Blocked(sample_result(TaskId(2), "MEDIATION CONTRACT: retry me")),
    );
    panel.registry.as_mut().unwrap().statuses.insert(
        TaskId(3),
        TaskStatus::Blocked(sample_result(
            TaskId(3),
            "stale routed plan: planned SiteMap root 0000000000000001 but current root is 0000000000000002",
        )),
    );

    panel.poll_live_workers(workspace_root, &mediator);
    assert_eq!(
        panel.runtime_status,
        "Waiting on 3 blocked task(s) (2 retryable)"
    );
}

// ---------------------------------------------------------------------------
// Hand-authored plans (panel::authoring)
// ---------------------------------------------------------------------------

fn authored_defaults() -> crate::editor::orchestrator::panel::ExecutionDefaults {
    crate::editor::orchestrator::panel::ExecutionDefaults {
        provider: crate::agent::AiProvider::CloudflareWorkersAi,
        model_id: "@cf/unit-test/model".to_string(),
        model_label: "unit-test-model".to_string(),
        thinking: false,
        task_kind: AgentTaskKind::BugFix,
    }
}

#[test]
fn panel_opens_on_an_empty_plan_that_says_so() {
    let panel = OrchestratorPanel::new();
    assert!(panel.plan_is_empty());
    assert!(!panel.has_dispatchable_work());
    // The old panel opened on a nine-task demo blueprint, so the empty state was
    // never shown and Execute appeared to do nothing.
    assert_eq!(
        panel.execution_blocker(),
        Some("The plan is empty. Add a task, or route a goal.")
    );
}

#[test]
fn adding_tasks_builds_a_plan_the_panel_can_execute() {
    let mut panel = OrchestratorPanel::new();
    let first = panel
        .add_task("Reproduce", "Write a failing test", vec![], vec![], None)
        .expect("first task");
    let second = panel
        .add_task(
            "Fix it",
            "Make the test pass",
            vec!["src/lib.rs".into()],
            vec![first],
            Some(AgentTaskKind::BugFix),
        )
        .expect("second task");

    assert_eq!((first, second), (TaskId(1), TaskId(2)));
    assert_eq!(panel.graph.tasks[&second].dependencies, vec![first]);
    assert!(panel.is_authored(second));
    assert!(!panel.is_authored(first));
    assert!(panel.has_dispatchable_work());
    assert_eq!(
        panel
            .registry
            .as_ref()
            .unwrap()
            .statuses
            .get(&second)
            .cloned()
            .map(|s| matches!(s, TaskStatus::Pending)),
        Some(true)
    );

    // No model has been synced yet, so the blocker is the model, not the plan.
    assert_eq!(
        panel.execution_blocker(),
        Some("No model selected. Pick one in Settings or Chat first.")
    );
    panel.defaults = authored_defaults();
    assert_eq!(panel.execution_blocker(), None);
}

#[test]
fn unknown_dependency_is_refused_with_a_single_hash() {
    let mut panel = OrchestratorPanel::new();
    let err = panel
        .add_task("Orphan", "", vec![], vec![TaskId(9)], None)
        .expect_err("task 9 does not exist");
    // TaskId's Display already carries the '#'; "#{}" here read as "##9".
    assert_eq!(err, "Task #9 does not exist to depend on.");
    assert!(panel.plan_is_empty());
}

#[test]
fn cycle_repair_keeps_every_task_and_names_the_edge_it_cut() {
    let mut panel = OrchestratorPanel::new();
    panel
        .graph
        .add(TaskId(1), "one", "", vec![], vec![TaskId(2)], None);
    panel
        .graph
        .add(TaskId(2), "two", "", vec![], vec![TaskId(1)], None);
    panel.registry = Some(OrchestratorRegistry::new(&panel.graph));
    assert_eq!(
        panel.execution_blocker(),
        Some("Resolve the dependency cycle first.")
    );

    let cut = panel.repair_cycles_action();

    // DFS reaches #2 first and finds #1 still on the path, so the back-edge is
    // the one that closes the loop, not the one that opened it.
    assert_eq!(cut, vec![(TaskId(2), TaskId(1))]);
    assert_eq!(panel.graph.tasks.len(), 2, "no task may be lost");
    assert!(!scheduler::detect_cycle(&panel.graph));
    assert!(panel.repair_report.contains("kept all 2 task(s)"));
    assert_eq!(panel.runtime_status, "Cycle repaired");
    // Repairing must not quietly substitute the demo blueprint.
    assert!(!panel.graph.tasks.contains_key(&TaskId(7)));
}

#[test]
fn removing_a_task_unblocks_the_tasks_that_were_waiting_on_it() {
    let mut panel = OrchestratorPanel::new();
    let first = panel.add_task("one", "", vec![], vec![], None).unwrap();
    let second = panel
        .add_task("two", "", vec![], vec![first], None)
        .unwrap();

    assert!(panel.remove_task(first));
    assert!(!panel.graph.tasks.contains_key(&first));
    assert_eq!(
        panel.graph.tasks[&second].dependencies,
        Vec::new(),
        "a dangling dependency can never be Done and would wedge the plan"
    );
    assert!(!panel
        .registry
        .as_ref()
        .unwrap()
        .statuses
        .contains_key(&first));
}

#[test]
fn task_ids_are_never_handed_out_twice() {
    let mut panel = OrchestratorPanel::new();
    let first = panel.add_task("one", "", vec![], vec![], None).unwrap();
    assert!(panel.remove_task(first));
    let next = panel.add_task("two", "", vec![], vec![], None).unwrap();
    assert_ne!(next, first);
    assert_eq!(next, TaskId(2));
}

#[test]
fn the_reconcile_node_has_no_delete_affordance() {
    let mut panel = OrchestratorPanel::new();
    panel.set_routed_tasks(
        "goal".to_string(),
        AgentTaskKind::Refactor,
        1,
        vec![routed_task("r1", 0)],
    );
    let root = panel.reconcile_root.expect("routed plan has a root");
    assert!(!panel.is_removable(root));
    assert!(!panel.remove_task(root));
    assert!(panel.is_removable(TaskId(2)));
}

#[test]
fn rerouting_discards_hand_authored_bindings() {
    let mut panel = OrchestratorPanel::new();
    panel.defaults = authored_defaults();
    let authored = panel
        .add_task(
            "mine",
            "",
            vec!["a.rs".into()],
            vec![],
            Some(AgentTaskKind::Test),
        )
        .unwrap();
    panel
        .bind_unbound_tasks(std::path::Path::new("."))
        .expect("binding");
    assert!(panel.binding_for(authored).is_some());

    panel.set_routed_tasks(
        "goal".to_string(),
        AgentTaskKind::Refactor,
        1,
        vec![routed_task("r1", 0)],
    );

    assert!(!panel.authored_kinds.contains_key(&authored));
    assert!(panel.binding_for(authored).is_none());
    assert!(panel.binding_for(TaskId(2)).is_some());
}

#[test]
fn a_hand_authored_task_binds_to_the_panels_current_model() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let mut panel = OrchestratorPanel::new();
    panel.defaults = authored_defaults();
    let id = panel
        .add_task(
            "Rename the field",
            "Rename `len` to `size` everywhere.",
            vec!["src/a.rs".into(), "src\\b.rs".into()],
            vec![],
            Some(AgentTaskKind::Refactor),
        )
        .unwrap();
    assert!(panel.binding_for(id).is_none(), "not bound until dispatch");

    panel.bind_unbound_tasks(workspace.path()).expect("binding");

    let bound = panel.binding_for(id).expect("binding exists");
    assert_eq!(bound.model_id, "@cf/unit-test/model");
    assert_eq!(bound.model_label, "unit-test-model");
    assert_eq!(bound.task_kind, AgentTaskKind::Refactor);
    assert_eq!(bound.task_id, format!("manual-{}", id.0));
    assert_eq!(bound.decomposition_policy_id, "manual");
    assert_eq!(
        bound.files,
        vec![
            std::path::PathBuf::from("src/a.rs"),
            std::path::PathBuf::from("src/b.rs"),
        ]
    );
    assert!(bound.execution_contract.contains("Task: Rename the field"));
    assert!(bound.execution_contract.contains("Rename the field"));
    assert!(bound
        .execution_contract
        .contains("Declared scope (change nothing outside it): src/a.rs, src/b.rs"));
    // Nothing to bind means nothing was opened, so no SiteMap error either.
    assert_eq!(panel.graph.tasks[&id].title, "Rename the field");
}

#[test]
fn binding_leaves_the_reconcile_node_alone() {
    let mut panel = OrchestratorPanel::new();
    panel.defaults = authored_defaults();
    panel.set_routed_tasks(
        "goal".to_string(),
        AgentTaskKind::Refactor,
        1,
        vec![routed_task("r1", 0)],
    );
    let root = panel.reconcile_root.expect("root");
    panel.bindings.remove(&TaskId(2));

    panel
        .bind_unbound_tasks(std::path::Path::new("."))
        .expect("binding");

    assert!(panel.binding_for(TaskId(2)).is_some());
    assert!(
        !panel.bindings.contains_key(&root),
        "the bookkeeping node must not be given a worker route"
    );
}

#[test]
fn the_draft_form_parses_and_validates_typed_input() {
    assert_eq!(
        TaskDraft::parse_list(" src/a.rs , \n src/b.rs \n"),
        vec!["src/a.rs", "src/b.rs"]
    );
    assert_eq!(
        TaskDraft::parse_dependencies("#1, 2").expect("parses"),
        vec![TaskId(1), TaskId(2)]
    );
    assert_eq!(
        TaskDraft::parse_dependencies("").expect("blank is no dependencies"),
        Vec::new()
    );
    assert!(TaskDraft::parse_dependencies("1, soon").is_err());

    let mut panel = OrchestratorPanel::new();
    panel.defaults = authored_defaults();
    panel.draft.title = "   ".to_string();
    assert_eq!(
        panel.add_draft_task().expect_err("blank title"),
        "Give the task a title."
    );
    panel.draft.title = "Ship it".to_string();
    panel.draft.dependencies = "nope".to_string();
    assert!(panel.add_draft_task().is_err());
    assert!(panel.plan_is_empty(), "a rejected draft adds nothing");

    panel.draft.dependencies = String::new();
    let id = panel.add_draft_task().expect("valid draft");
    assert_eq!(id, TaskId(1));
    assert!(panel.draft.title.is_empty(), "form resets after adding");
}

#[test]
fn clearing_the_plan_returns_the_panel_to_an_empty_slate() {
    let mut panel = OrchestratorPanel::new();
    panel.defaults = authored_defaults();
    panel
        .add_task("one", "", vec![], vec![], None)
        .expect("task");
    panel.clear_plan();
    assert!(panel.plan_is_empty());
    assert!(panel.bindings.is_empty());
    assert!(panel.reconcile_root.is_none());
    assert!(panel.routed_plan.is_none());
    assert!(!panel.has_dispatchable_work());
}

#[test]
fn routing_a_goal_from_the_panel_asks_the_app_and_ignores_blanks() {
    let mut panel = OrchestratorPanel::new();
    panel.request_route_goal("   ");
    assert!(panel.route_request.is_none());

    panel.request_route_goal("  Extract the retry helper  ");
    assert_eq!(
        panel.route_request.as_deref(),
        Some("Extract the retry helper")
    );
    assert_eq!(panel.goal_draft, "Extract the retry helper");
    assert!(!panel.goal_input_open);
    assert_eq!(panel.runtime_status, "Routing goal...");
}
#[test]
fn a_plan_with_no_model_stays_pending_instead_of_launching_a_worker() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mediator = std::sync::Arc::new(crate::automation::mediator::MediatorArena::new());
    let mut panel = OrchestratorPanel::new();
    let id = panel
        .add_task(
            "no route yet",
            "",
            vec![],
            vec![],
            Some(AgentTaskKind::BugFix),
        )
        .expect("task");

    let err = panel
        .bind_unbound_tasks(temp.path())
        .expect_err("nothing to run on");
    assert!(err.starts_with("No model selected"), "was: {err}");
    assert!(panel.binding_for(id).is_none());

    panel.execution_running = true;
    panel.poll_live_workers(temp.path(), &mediator);

    assert!(
        panel.running_workers.is_empty(),
        "no worker without a model"
    );
    assert!(matches!(
        panel.registry.as_ref().unwrap().statuses.get(&id),
        Some(TaskStatus::Pending)
    ));
    // The reason belongs next to the button, which is what the panel renders.
    assert_eq!(
        panel.execution_blocker(),
        Some("No model selected. Pick one in Settings or Chat first.")
    );
}

#[test]
fn mission_control_and_the_panel_agree_on_whether_work_can_launch() {
    let mut panel = OrchestratorPanel::new();
    panel
        .add_task("one", "", vec![], vec![], Some(AgentTaskKind::BugFix))
        .expect("task");
    // Hand-authored work exists, but with no model neither surface may offer it.
    assert!(panel.has_dispatchable_work());
    assert!(!panel.dashboard_snapshot().can_launch_routed_tasks);

    panel.defaults = authored_defaults();
    assert!(panel.dashboard_snapshot().can_launch_routed_tasks);
}

#[test]
fn typed_scope_paths_are_normalised_where_the_task_is_born() {
    let mut panel = OrchestratorPanel::new();
    panel.defaults = authored_defaults();
    let id = panel
        .add_task(
            "windows paths in",
            "",
            vec!["src\\win\\a.rs".into()],
            vec![],
            Some(AgentTaskKind::Refactor),
        )
        .expect("task");
    panel
        .bind_unbound_tasks(std::path::Path::new("."))
        .expect("binding");

    assert_eq!(
        panel.graph.tasks[&id].scope,
        vec!["src/win/a.rs".to_string()]
    );
    assert_eq!(
        panel.binding_for(id).expect("binding").files,
        vec![std::path::PathBuf::from("src/win/a.rs")]
    );
}
