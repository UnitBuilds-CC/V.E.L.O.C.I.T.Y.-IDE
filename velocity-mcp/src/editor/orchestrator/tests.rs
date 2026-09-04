use super::panel::OrchestratorPanel;
use super::types::*;
use crate::automation::{AgentTaskKind, RoutedSubAgentTask};
use crate::orchestrator::blueprint::TaskGraph;
use crate::orchestrator::registry::{OrchestratorRegistry, TaskStatus};
use crate::orchestrator::worker::{WorkerHandle, WorkerResult, WorkerThreadSnapshot};
use crate::orchestrator::TaskId;

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
    let graph = build_routed_graph("goal", &[]);
    let mut registry = OrchestratorRegistry::new(&graph);
    registry.statuses.insert(TaskId(1), TaskStatus::Pending);
    complete_reconcile_root(&graph, &mut registry);
    assert!(matches!(
        registry.statuses.get(&TaskId(1)),
        Some(TaskStatus::Pending)
    ));

    let graph = build_routed_graph(
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

    complete_reconcile_root(&graph, &mut registry);

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
