use super::super::types::*;
use super::authoring::{ExecutionDefaults, TaskDraft};
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
    /// What each task actually runs with: provider, model, execution contract,
    /// and the SiteMap root it was planned against. Routed plans fill this from
    /// the router; hand-authored tasks are filled in at dispatch by
    /// [`OrchestratorPanel::bind_unbound_tasks`](super::authoring::OrchestratorPanel::bind_unbound_tasks).
    /// Keyed by task id, because the previous positional lookup
    /// (`task_id - 2`) returned `None` for anything the model did not create.
    pub bindings: HashMap<TaskId, RoutedSubAgentTask>,
    /// Task kind the author chose, per hand-authored task. Presence in this map
    /// is also what marks a task as hand-authored rather than routed.
    pub authored_kinds: HashMap<TaskId, AgentTaskKind>,
    /// Provider/model a hand-authored task runs on, synced from the app.
    pub defaults: ExecutionDefaults,
    /// The inline add-task form.
    pub draft: TaskDraft,
    /// Highest task id ever issued, so ids are never reused within a session.
    pub last_issued_task_id: u64,
    /// The synthetic task that reconciles a routed plan. It is a bookkeeping
    /// node rather than a unit of work, so it must never be given a binding.
    pub reconcile_root: Option<TaskId>,
    /// Set by the panel when the user asks to route a goal from inside it, so
    /// the app can run the planner on the next pass. Same reason `checkpoint_action`
    /// exists on the bottom panel: the panel cannot reach the app, and the app
    /// cannot borrow the panel while it is drawing.
    pub route_request: Option<String>,
    /// Inline "route a goal" box: whether it is open, and the goal being typed.
    pub goal_input_open: bool,
    pub goal_draft: String,
    /// What the last cycle repair actually changed, shown until the next plan edit
    /// so a repair is auditable instead of a silent graph swap.
    pub repair_report: String,
}

impl Default for OrchestratorPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl OrchestratorPanel {
    pub fn new() -> Self {
        // Empty on purpose. This used to open on `TaskGraph::example_game()`, a
        // nine-task demo blueprint with no execution bindings: Execute started
        // nothing, the plan was not the user's, and the only way to get an empty
        // slate was to break the graph and repair it.
        let graph = TaskGraph::default();
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
            bindings: HashMap::new(),
            authored_kinds: HashMap::new(),
            defaults: ExecutionDefaults::default(),
            draft: TaskDraft::default(),
            last_issued_task_id: 0,
            reconcile_root: None,
            route_request: None,
            goal_input_open: false,
            goal_draft: String::new(),
            repair_report: String::new(),
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
        // A re-route replaces the plan wholesale: bindings for tasks that no
        // longer exist would otherwise be handed to whatever id is reused next.
        self.bindings.clear();
        self.authored_kinds.clear();
        self.reconcile_root = Some(self.graph.root);
        for (idx, task) in tasks.iter().enumerate() {
            self.bindings.insert(TaskId(idx as u64 + 2), task.clone());
        }
        self.last_issued_task_id = self
            .graph
            .tasks
            .keys()
            .map(|id| id.0)
            .max()
            .unwrap_or(self.last_issued_task_id);
    }

    pub fn selected_policy_kind(&self) -> AgentTaskKind {
        self.policy_editor.kind
    }

    /// Is there any task a worker could be pointed at?
    ///
    /// A routed graph always contains the reconcile node, which is bookkeeping
    /// rather than work, so counting `graph.tasks` would report a plan that
    /// cannot launch anything -- and, from the other direction, a hand-authored
    /// plan with no routed goal counted as nothing at all.
    pub fn has_dispatchable_work(&self) -> bool {
        self.graph
            .tasks
            .keys()
            .any(|id| Some(*id) != self.reconcile_root)
    }

    pub fn dashboard_snapshot(&self) -> OrchestratorDashboardSnapshot {
        let has_routed_plan = self.routed_plan.is_some();
        let has_dependency_cycle = scheduler::detect_cycle(&self.graph);
        let retryable_blocked_tasks = self.retryable_blocked_task_count();
        let has_runtime_activity = has_routed_plan
            || self.has_dispatchable_work()
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
            can_launch_routed_tasks: self.has_dispatchable_work()
                && !has_dependency_cycle
                && !self.execution_running
                // Launching from Mission Control must respect the same reasons
                // the panel states next to its Execute button, or the two
                // surfaces disagree about whether work can start.
                && self.execution_blocker().is_none(),
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
            let routed = self.binding_for(task.id);
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
