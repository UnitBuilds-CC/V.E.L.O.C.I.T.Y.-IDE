use crate::automation::{
    AgentTaskKind, DecompositionStyle, RoutedSubAgentTask,
};
use crate::orchestrator::blueprint::TaskGraph;
use crate::orchestrator::registry::{OrchestratorRegistry, TaskStatus};
use crate::orchestrator::worker::{WorkerResult, WorkerThreadSnapshot};
use crate::orchestrator::TaskId;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct RoutedPlanState {
    pub goal: String,
    pub kind: AgentTaskKind,
    pub scope_count: usize,
    pub tasks: Vec<RoutedSubAgentTask>,
}

#[derive(Debug, Clone)]
pub struct PolicyEditorState {
    pub kind: AgentTaskKind,
    pub selected_policy_id: String,
    pub loaded_policy_id: String,
    pub draft_label: String,
    pub draft_template_id: String,
    pub draft_style: DecompositionStyle,
    pub draft_expectations: String,
    pub status: String,
}

impl Default for PolicyEditorState {
    fn default() -> Self {
        Self {
            kind: AgentTaskKind::Refactor,
            selected_policy_id: String::new(),
            loaded_policy_id: String::new(),
            draft_label: String::new(),
            draft_template_id: String::new(),
            draft_style: DecompositionStyle::CoupledComponents,
            draft_expectations: String::new(),
            status: "Select a policy to tune routed planning.".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct OrchestratorDashboardSnapshot {
    pub goal: Option<String>,
    pub task_kind: Option<String>,
    pub scope_count: usize,
    pub planning_status: String,
    pub runtime_status: String,
    pub execution_running: bool,
    pub has_routed_plan: bool,
    pub has_dependency_cycle: bool,
    pub can_launch_routed_tasks: bool,
    pub can_reset_runtime: bool,
    pub active_workers: usize,
    pub pending_tasks: usize,
    pub running_tasks: usize,
    pub done_tasks: usize,
    pub failed_tasks: usize,
    pub blocked_tasks: usize,
    pub retryable_blocked_tasks: usize,
    pub tasks: Vec<OrchestratorTaskSnapshot>,
}

#[derive(Debug, Clone)]
pub struct OrchestratorTaskSnapshot {
    pub id: u64,
    pub title: String,
    pub description: String,
    pub status_label: String,
    pub provider_label: String,
    pub model_label: String,
    pub scope: Vec<String>,
    pub rationale: String,
    pub outputs: Vec<String>,
    pub message: String,
    pub run_summary_path: Option<String>,
    pub run_facts_path: Option<String>,
    pub wa_run_path: Option<String>,
    pub wa_run_id: Option<String>,
    pub live_thread: Option<WorkerThreadSnapshot>,
}

pub fn routed_task_for_id(
    plan: &Option<RoutedPlanState>,
    task_id: TaskId,
) -> Option<&RoutedSubAgentTask> {
    let routed_idx = task_id.0.checked_sub(2)? as usize;
    plan.as_ref()?.tasks.get(routed_idx)
}

pub fn task_result_outputs(result: &WorkerResult) -> Vec<String> {
    let mut outputs = result.outputs.clone();
    outputs.extend(result.created_files.clone());
    outputs.extend(result.deleted_files.clone());
    outputs.sort();
    outputs.dedup();
    outputs
}

pub fn reconciliation_error(
    graph: &TaskGraph,
    existing_outputs: &HashMap<TaskId, Vec<String>>,
    task_id: TaskId,
    outputs: &[String],
) -> Option<String> {
    let mut candidate_outputs = existing_outputs.clone();
    candidate_outputs.insert(task_id, outputs.to_vec());

    let scope_violations =
        crate::orchestrator::reconcile::scope_violations(graph, &candidate_outputs)
            .into_iter()
            .filter(|(violating_task_id, _)| *violating_task_id == task_id)
            .map(|(_, path)| path)
            .collect::<Vec<_>>();
    if !scope_violations.is_empty() {
        return Some(format!(
            "Reconciliation blocked: task touched files outside its declared scope: {}",
            scope_violations.join(", ")
        ));
    }

    let collisions = crate::orchestrator::reconcile::detect_collisions(graph, &candidate_outputs)
        .into_iter()
        .filter(|collision| collision.task_a == task_id || collision.task_b == task_id)
        .map(|collision| {
            let other_task_id = if collision.task_a == task_id {
                collision.task_b.0
            } else {
                collision.task_a.0
            };
            format!("{} with task {}", collision.path, other_task_id)
        })
        .collect::<Vec<_>>();
    if !collisions.is_empty() {
        return Some(format!(
            "Reconciliation blocked: overlapping outputs detected for {}",
            collisions.join(", ")
        ));
    }

    None
}

pub fn requires_follow_up(result: &WorkerResult) -> bool {
    !result.out_of_scope_created_files.is_empty()
        || result.message.contains("MEDIATION CONTRACT:")
        || result.message.contains("Reconciliation blocked:")
        || result.message.contains("cancelled by operator")
        || result.status_updates.iter().any(|status| {
            status.contains("MEDIATION CONTRACT:")
                || status.contains("Reconciliation blocked:")
                || status.contains("cancelled by operator")
        })
}

pub fn is_dependency_blocked_message(message: &str) -> bool {
    message.starts_with("Follow-up required before this task can run because dependency task(s)")
}

pub fn is_retryable_blocked_result(result: &WorkerResult) -> bool {
    !is_stale_plan_blocked_result(result)
        && (requires_follow_up(result) || is_dependency_blocked_message(&result.message))
}

pub fn is_stale_plan_blocked_result(result: &WorkerResult) -> bool {
    result.message.contains("stale routed plan:")
        || result
            .status_updates
            .iter()
            .any(|status| status.contains("stale routed plan:"))
}

pub fn propagate_blocked_dependents(graph: &TaskGraph, registry: &mut OrchestratorRegistry) {
    loop {
        let mut changed = false;
        for task in graph.tasks.values() {
            let current_status = registry.statuses.get(&task.id).cloned().unwrap_or_default();
            if matches!(current_status, TaskStatus::Done(_) | TaskStatus::Running) {
                continue;
            }

            let dependency_blocked = matches!(
                &current_status,
                TaskStatus::Blocked(result) if is_dependency_blocked_message(&result.message)
            );
            if !matches!(current_status, TaskStatus::Pending | TaskStatus::Blocked(_))
                || (!dependency_blocked && matches!(current_status, TaskStatus::Blocked(_)))
            {
                continue;
            }

            let blocking_dependencies = task
                .dependencies
                .iter()
                .filter(|dependency| {
                    matches!(
                        registry.statuses.get(dependency),
                        Some(TaskStatus::Failed(_)) | Some(TaskStatus::Blocked(_))
                    )
                })
                .copied()
                .collect::<Vec<_>>();

            if blocking_dependencies.is_empty() {
                if dependency_blocked {
                    registry.statuses.insert(task.id, TaskStatus::Pending);
                    changed = true;
                }
                continue;
            }

            let message = format!(
                "Follow-up required before this task can run because dependency task(s) {} did not complete cleanly.",
                blocking_dependencies
                    .iter()
                    .map(|dependency| dependency.0.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );

            let needs_update = match &current_status {
                TaskStatus::Pending => true,
                TaskStatus::Blocked(result) if is_dependency_blocked_message(&result.message) => {
                    result.message != message
                }
                _ => false,
            };

            if !needs_update {
                continue;
            }

            let mut result = WorkerResult::new(task);
            result.success = false;
            result.message = message;
            result.status_updates.push(result.message.clone());
            registry
                .statuses
                .insert(task.id, TaskStatus::Blocked(result));
            changed = true;
        }
        if !changed {
            break;
        }
    }
}

pub fn complete_reconcile_root(graph: &TaskGraph, registry: &mut OrchestratorRegistry) {
    let Some(root_task) = graph.tasks.get(&graph.root) else {
        return;
    };
    if !matches!(
        registry.statuses.get(&graph.root),
        Some(TaskStatus::Pending) | None
    ) {
        return;
    }
    if root_task.dependencies.is_empty() {
        return;
    }
    if !root_task
        .dependencies
        .iter()
        .all(|dependency| matches!(registry.statuses.get(dependency), Some(TaskStatus::Done(_))))
    {
        return;
    }

    let mut result = WorkerResult::new(root_task);
    result.message = format!(
        "Reconciliation complete across {} routed task(s).",
        root_task.dependencies.len()
    );
    result.status_updates.push(result.message.clone());
    registry
        .statuses
        .insert(graph.root, TaskStatus::Done(result));
}

pub fn build_routed_graph(goal: &str, tasks: &[RoutedSubAgentTask]) -> TaskGraph {
    let mut graph = TaskGraph::default();
    graph.root = TaskId(1);
    graph.add(
        TaskId(1),
        "Reconcile routed plan",
        format!("Reconcile sub-agent outputs for goal: {goal}"),
        vec![".velocity/agentic".to_string()],
        vec![],
        None,
    );

    for (idx, task) in tasks.iter().enumerate() {
        let scope = task
            .files
            .iter()
            .map(|file| file.display().to_string())
            .collect::<Vec<_>>();
        graph.add(
            TaskId(idx as u64 + 2),
            format!("{} {}", task.task_kind.as_str(), idx + 1),
            format!("{}\n{}", task.summary, task.rationale),
            scope,
            vec![],
            Some(TaskId(1)),
        );
    }

    graph
}
