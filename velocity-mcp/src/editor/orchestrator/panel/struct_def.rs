use super::super::types::*;
use crate::automation::{AgentTaskKind, RoutedSubAgentTask};
use crate::orchestrator::blueprint::TaskGraph;
use crate::orchestrator::registry::{OrchestratorRegistry, TaskStatus};
use crate::orchestrator::scheduler;
use crate::orchestrator::worker::WorkerHandle;
use crate::orchestrator::TaskId;
use std::collections::HashMap;

pub struct OrchestratorPanel {
    pub graph: TaskGraph,
    pub registry: Option<OrchestratorRegistry>,
    pub expanded: bool,
    pub show_policy_editor: bool,
    pub routed_plan: Option<RoutedPlanState>,
    pub policy_editor: PolicyEditorState,
    pub planning_status: String,
    pub runtime_status: String,
    pub execution_running: bool,
    pub running_workers: HashMap<TaskId, Box<dyn WorkerHandle>>,
}

impl Default for OrchestratorPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl OrchestratorPanel {
    pub fn new() -> Self {
        let graph = TaskGraph::example_game();
        let registry = OrchestratorRegistry::new(&graph);
        Self {
            graph,
            registry: Some(registry),
            expanded: true,
            show_policy_editor: false,
            routed_plan: None,
            policy_editor: PolicyEditorState::default(),
            planning_status: "No routed sub-agent plan yet.".to_string(),
            runtime_status: "Idle".to_string(),
            execution_running: false,
            running_workers: HashMap::new(),
        }
    }

    pub fn set_routed_tasks(
        &mut self,
        goal: String,
        kind: AgentTaskKind,
        scope_count: usize,
        tasks: Vec<RoutedSubAgentTask>,
    ) {
        self.planning_status = if tasks.is_empty() {
            "No routed tasks were produced for the requested goal.".to_string()
        } else {
            format!(
                "Planned {} routed task(s) from {} scoped file(s).",
                tasks.len(),
                scope_count,
            )
        };
        self.routed_plan = Some(RoutedPlanState {
            goal: goal.clone(),
            kind,
            scope_count,
            tasks: tasks.clone(),
        });
        self.policy_editor.kind = kind;
        self.policy_editor.loaded_policy_id.clear();
        self.graph = build_routed_graph(&goal, &tasks);
        self.registry = Some(OrchestratorRegistry::new(&self.graph));
        self.runtime_status = "Plan ready".to_string();
        self.execution_running = false;
        self.running_workers.clear();
    }

    pub fn selected_policy_kind(&self) -> AgentTaskKind {
        self.policy_editor.kind
    }

    pub fn dashboard_snapshot(&self) -> OrchestratorDashboardSnapshot {
        let has_routed_plan = self.routed_plan.is_some();
        let has_dependency_cycle = scheduler::detect_cycle(&self.graph);
        let retryable_blocked_tasks = self.retryable_blocked_task_count();
        let has_runtime_activity = has_routed_plan
            || self.execution_running
            || !self.running_workers.is_empty()
            || self.runtime_status != "Idle"
            || self.registry.as_ref().is_some_and(|reg| {
                !reg.outputs.is_empty()
                    || reg
                        .statuses
                        .values()
                        .any(|status| !matches!(status, TaskStatus::Pending))
            });
        let mut snapshot = OrchestratorDashboardSnapshot {
            goal: self.routed_plan.as_ref().map(|plan| plan.goal.clone()),
            task_kind: self
                .routed_plan
                .as_ref()
                .map(|plan| plan.kind.as_str().to_string()),
            scope_count: self
                .routed_plan
                .as_ref()
                .map(|plan| plan.scope_count)
                .unwrap_or(0),
            planning_status: self.planning_status.clone(),
            runtime_status: self.runtime_status.clone(),
            execution_running: self.execution_running,
            has_routed_plan,
            has_dependency_cycle,
            can_launch_routed_tasks: has_routed_plan
                && !has_dependency_cycle
                && !self.execution_running,
            can_reset_runtime: has_runtime_activity,
            active_workers: self.running_workers.len(),
            retryable_blocked_tasks,
            ..OrchestratorDashboardSnapshot::default()
        };

        for task in self.graph.tasks.values() {
            let status = self
                .registry
                .as_ref()
                .and_then(|registry| registry.statuses.get(&task.id))
                .cloned()
                .unwrap_or(TaskStatus::Pending);
            let routed = routed_task_for_id(&self.routed_plan, task.id);
            let (
                status_label,
                outputs,
                message,
                provider_label,
                model_label,
                run_summary_path,
                run_facts_path,
                wa_run_path,
                wa_run_id,
            ) = match status {
                TaskStatus::Pending => {
                    snapshot.pending_tasks += 1;
                    (
                        "Pending".to_string(),
                        Vec::new(),
                        String::new(),
                        routed
                            .map(|task| task.provider.label().to_string())
                            .unwrap_or_default(),
                        routed
                            .map(|task| task.model_label.clone())
                            .unwrap_or_default(),
                        None,
                        None,
                        None,
                        None,
                    )
                }
                TaskStatus::Running => {
                    snapshot.running_tasks += 1;
                    (
                        "Running".to_string(),
                        Vec::new(),
                        String::new(),
                        routed
                            .map(|task| task.provider.label().to_string())
                            .unwrap_or_default(),
                        routed
                            .map(|task| task.model_label.clone())
                            .unwrap_or_default(),
                        None,
                        None,
                        None,
                        None,
                    )
                }
                TaskStatus::Done(result) => {
                    snapshot.done_tasks += 1;
                    (
                        "Done".to_string(),
                        task_result_outputs(&result),
                        result.message.clone(),
                        result.provider_label,
                        result.model_label,
                        result
                            .run_summary_path
                            .as_ref()
                            .map(|path| path.display().to_string()),
                        result
                            .run_facts_path
                            .as_ref()
                            .map(|path| path.display().to_string()),
                        result.wa_run_path.clone(),
                        result.wa_run_id.clone(),
                    )
                }
                TaskStatus::Failed(result) => {
                    snapshot.failed_tasks += 1;
                    (
                        "Failed".to_string(),
                        task_result_outputs(&result),
                        result.message.clone(),
                        result.provider_label,
                        result.model_label,
                        result
                            .run_summary_path
                            .as_ref()
                            .map(|path| path.display().to_string()),
                        result
                            .run_facts_path
                            .as_ref()
                            .map(|path| path.display().to_string()),
                        result.wa_run_path.clone(),
                        result.wa_run_id.clone(),
                    )
                }
                TaskStatus::Blocked(result) => {
                    snapshot.blocked_tasks += 1;
                    (
                        "Follow-up".to_string(),
                        task_result_outputs(&result),
                        result.message.clone(),
                        result.provider_label,
                        result.model_label,
                        result
                            .run_summary_path
                            .as_ref()
                            .map(|path| path.display().to_string()),
                        result
                            .run_facts_path
                            .as_ref()
                            .map(|path| path.display().to_string()),
                        result.wa_run_path.clone(),
                        result.wa_run_id.clone(),
                    )
                }
            };

            snapshot.tasks.push(OrchestratorTaskSnapshot {
                id: task.id.0,
                title: task.title.clone(),
                description: task.description.clone(),
                status_label,
                provider_label,
                model_label,
                scope: task.scope.clone(),
                rationale: routed
                    .map(|task| {
                        format!(
                            "[{} \u{00b7} {}] {}",
                            task.decomposition_policy_id,
                            task.decomposition_style.as_str(),
                            task.rationale
                        )
                    })
                    .unwrap_or_default(),
                outputs,
                message,
                run_summary_path,
                run_facts_path,
                wa_run_path,
                wa_run_id,
                live_thread: self
                    .running_workers
                    .get(&task.id)
                    .map(|handle| handle.snapshot()),
            });
        }

        snapshot.tasks.sort_by_key(|task| task.id);
        snapshot
    }
}
