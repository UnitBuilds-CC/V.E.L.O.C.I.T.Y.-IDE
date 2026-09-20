//! Hand-authored plans: the Orchestrator panel as an editor.
//!
//! Until now the only way to put a task in front of the scheduler was to route
//! a goal through the model, which meant the panel could show a plan and launch
//! one but could never accept one. Two things had to change for "Execute" to
//! mean something on a plan typed by a person:
//!
//! 1. **Binding by identity, not by position.** `routed_task_for_id` derived a
//!    task's execution parameters from `task_id - 2`, an index into the vector
//!    the router happened to return. Every other task id resolved to `None`,
//!    and `poll_live_workers` treats `None` as "nothing to dispatch" -- so a
//!    hand-built plan started zero workers, reported no error, and looked idle.
//!    Bindings now live in a map keyed by [`TaskId`].
//! 2. **A task needs a route to run on.** A routed task carries the provider,
//!    model, and SiteMap root the planner chose. A hand-authored one carries
//!    the author's intent, and gets its execution binding from the panel's
//!    current defaults the first time work is actually dispatched -- so the
//!    freshness check compares against the root as of *launch*, not as of some
//!    earlier planning run.

use super::struct_def::OrchestratorPanel;
use crate::agent::AiProvider;
use crate::automation::task_router::RoutedModelRoute;
use crate::automation::{AgentTaskKind, DecompositionStyle, RoutedSubAgentTask};
use crate::orchestrator::blueprint::Task;
use crate::orchestrator::registry::{OrchestratorRegistry, TaskStatus};
use crate::orchestrator::scheduler;
use crate::orchestrator::TaskId;
use std::path::{Path, PathBuf};

/// What an unbound task runs with. The app refreshes this from the live
/// provider / model selection each frame, the same way the chat panel reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionDefaults {
    pub provider: AiProvider,
    pub model_id: String,
    pub model_label: String,
    pub thinking: bool,
    pub task_kind: AgentTaskKind,
}

impl Default for ExecutionDefaults {
    fn default() -> Self {
        Self {
            // VelocityApp::new's built-in provider, so a panel that has never
            // been synced is not pointing somewhere the app cannot call.
            provider: AiProvider::CloudflareWorkersAi,
            model_id: String::new(),
            model_label: Self::PLACEHOLDER_MODEL.to_string(),
            thinking: false,
            task_kind: AgentTaskKind::Refactor,
        }
    }
}

impl ExecutionDefaults {
    /// Label shown until a model has been synced or chosen.
    pub const PLACEHOLDER_MODEL: &'static str = "not selected";

    /// True when no usable model has been synced or chosen.
    pub fn has_model(&self) -> bool {
        !self.model_id.trim().is_empty() && self.model_label != Self::PLACEHOLDER_MODEL
    }
}

/// The inline "add task" form. Kept as panel state rather than local `Ui`
/// variables so a half-typed task survives a frame in which the panel loses
/// focus -- egui re-creates locals every pass.
#[derive(Debug, Clone)]
pub struct TaskDraft {
    pub open: bool,
    pub title: String,
    pub description: String,
    /// Comma- or newline-separated paths, as they are typed.
    pub scope: String,
    /// Comma-separated task ids, as they are typed.
    pub dependencies: String,
    pub kind: AgentTaskKind,
    /// Last validation complaint, shown beside the form.
    pub error: String,
}

impl Default for TaskDraft {
    fn default() -> Self {
        Self {
            open: false,
            title: String::new(),
            description: String::new(),
            scope: String::new(),
            dependencies: String::new(),
            kind: AgentTaskKind::Refactor,
            error: String::new(),
        }
    }
}

impl TaskDraft {
    /// Split a typed list on commas and newlines, dropping blanks.
    pub fn parse_list(raw: &str) -> Vec<String> {
        raw.split([',', '\n'])
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(|item| item.replace('\\', "/"))
            .collect()
    }

    /// Task ids as typed. Anything unparseable is reported rather than
    /// ignored, because "1, 2, typo" silently scheduling differently than the
    /// user read is worse than refusing the entry.
    pub fn parse_dependencies(raw: &str) -> Result<Vec<TaskId>, String> {
        let mut ids = Vec::new();
        for item in Self::parse_list(raw) {
            let digits = item.trim_start_matches('#');
            match digits.parse::<u64>() {
                Ok(id) if id > 0 => ids.push(TaskId(id)),
                _ => return Err(format!("'{item}' is not a task id (try '#3')")),
            }
        }
        Ok(ids)
    }
}

/// Text a hand-authored task is executed against. The router builds richer
/// contracts from instruction templates; this says exactly what the author
/// said, in the shape the worker's artifact writer expects.
pub fn manual_execution_contract(task: &Task) -> String {
    let mut lines = vec![format!("Task: {}", task.title)];
    if !task.description.trim().is_empty() {
        lines.push(task.description.trim().to_string());
    }
    if !task.scope.is_empty() {
        lines.push(format!(
            "Declared scope (change nothing outside it): {}",
            task.scope.join(", ")
        ));
    }
    if !task.dependencies.is_empty() {
        lines.push(format!(
            "Runs after task(s): {}",
            task.dependencies
                .iter()
                .map(|id| format!("#{}", id.0))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    lines.join("\n")
}

fn manual_binding(
    task: &Task,
    kind: AgentTaskKind,
    defaults: &ExecutionDefaults,
    site_map_root: u64,
) -> RoutedSubAgentTask {
    RoutedSubAgentTask {
        task_id: format!("manual-{}", task.id.0),
        files: task.scope.iter().map(PathBuf::from).collect(),
        task_kind: kind,
        planned_site_map_root: site_map_root,
        provider: defaults.provider,
        model_id: defaults.model_id.clone(),
        model_label: defaults.model_label.clone(),
        thinking: defaults.thinking,
        fallback_chain: Vec::<RoutedModelRoute>::new(),
        execution_contract: manual_execution_contract(task),
        summary: task.description.clone(),
        rationale: "Hand-authored in the Orchestrator panel; not model-routed.".to_string(),
        decomposition_policy_id: "manual".to_string(),
        decomposition_style: DecompositionStyle::IsolatedFiles,
    }
}

impl OrchestratorPanel {
    /// The next unused task id. Monotonic for the life of the session so a
    /// removed task's number is never handed to different work -- the ids are
    /// quoted in run artifacts and in the user's own notes.
    pub fn next_task_id(&mut self) -> TaskId {
        let in_graph = self.graph.tasks.keys().map(|id| id.0).max().unwrap_or(0);
        self.last_issued_task_id = self.last_issued_task_id.max(in_graph);
        self.last_issued_task_id += 1;
        TaskId(self.last_issued_task_id)
    }

    /// True when there is nothing scheduled -- drives the empty state and the
    /// enabled state of Execute.
    pub fn plan_is_empty(&self) -> bool {
        self.graph.tasks.is_empty()
    }

    /// Validate and add the task described by the current draft.
    pub fn add_draft_task(&mut self) -> Result<TaskId, String> {
        let title = self.draft.title.trim().to_string();
        if title.is_empty() {
            return Err("Give the task a title.".to_string());
        }
        let description = self.draft.description.trim().to_string();
        let scope = TaskDraft::parse_list(&self.draft.scope);
        let dependencies = TaskDraft::parse_dependencies(&self.draft.dependencies)?;
        let kind = self.draft.kind;
        let id = self.add_task(&title, &description, scope, dependencies, Some(kind))?;
        self.draft = TaskDraft::default();
        Ok(id)
    }

    /// Add one task to the plan.
    ///
    /// Rejects unknown dependencies and dependencies that would close a cycle,
    /// leaving the graph untouched when it does -- a plan is worth more than a
    /// panel that accepts input and then needs "Auto-repair" to undo it.
    pub fn add_task(
        &mut self,
        title: &str,
        description: &str,
        scope: Vec<String>,
        dependencies: Vec<TaskId>,
        kind: Option<AgentTaskKind>,
    ) -> Result<TaskId, String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("A task needs a title.".to_string());
        }
        for dep in &dependencies {
            if !self.graph.tasks.contains_key(dep) {
                // `TaskId`'s Display already carries the '#'.
                return Err(format!("Task {dep} does not exist to depend on."));
            }
        }

        let id = self.next_task_id();
        // Paths reach here from the form, the bridge and the planner; normalise
        // once at the boundary so a Windows-style scope is not later handed to
        // the worker half-converted.
        let scope: Vec<String> = scope.iter().map(|path| path.replace('\\', "/")).collect();
        self.graph.add(
            id,
            title.to_string(),
            description.to_string(),
            scope,
            dependencies,
            None,
        );
        if scheduler::detect_cycle(&self.graph) {
            self.graph.tasks.remove(&id);
            self.last_issued_task_id = self.last_issued_task_id.saturating_sub(1);
            return Err(format!(
                "Adding {id} would create a dependency cycle, so it was not added."
            ));
        }

        if let Some(kind) = kind {
            self.authored_kinds.insert(id, kind);
        }
        if self.registry.is_none() {
            self.registry = Some(OrchestratorRegistry::new(&self.graph));
        }
        if let Some(registry) = self.registry.as_mut() {
            registry.statuses.insert(id, TaskStatus::Pending);
        }
        self.runtime_status = format!("Added task #{}", id.0);
        // The report described edges of the plan as it was before this edit.
        self.repair_report.clear();
        Ok(id)
    }

    /// Remove a task from the plan, cancelling it if it is running.
    ///
    /// Also strips the task out of every other task's dependency list: a
    /// dangling id can never be reported Done, so leaving it behind would
    /// wedge the rest of the plan with no visible cause.
    pub fn remove_task(&mut self, id: TaskId) -> bool {
        if !self.graph.tasks.contains_key(&id) {
            return false;
        }
        if Some(id) == self.reconcile_root {
            return false;
        }
        if let Some(handle) = self.running_workers.get_mut(&id) {
            let _ = handle.cancel();
        }
        self.running_workers.remove(&id);
        self.graph.tasks.remove(&id);
        self.bindings.remove(&id);
        self.authored_kinds.remove(&id);
        for task in self.graph.tasks.values_mut() {
            task.dependencies.retain(|dep| *dep != id);
        }
        if let Some(registry) = self.registry.as_mut() {
            registry.statuses.remove(&id);
            registry.outputs.remove(&id);
        }
        self.execution_running = !self.running_workers.is_empty();
        self.runtime_status = format!("Removed task #{}", id.0);
        self.repair_report.clear();
        true
    }

    /// Throw the whole plan away, leaving an empty graph the user can type into.
    pub fn clear_plan(&mut self) {
        for handle in self.running_workers.values_mut() {
            let _ = handle.cancel();
        }
        self.running_workers.clear();
        self.graph = Default::default();
        self.registry = Some(OrchestratorRegistry::new(&self.graph));
        self.bindings.clear();
        self.authored_kinds.clear();
        self.reconcile_root = None;
        self.routed_plan = None;
        self.execution_running = false;
        self.planning_status = "No routed sub-agent plan yet.".to_string();
        self.runtime_status = "Idle".to_string();
        self.repair_report.clear();
    }

    /// The execution binding for a task, routed or hand-authored.
    pub fn binding_for(&self, id: TaskId) -> Option<&RoutedSubAgentTask> {
        self.bindings.get(&id)
    }

    pub fn is_authored(&self, id: TaskId) -> bool {
        self.authored_kinds.contains_key(&id)
    }

    /// Can the user delete this task from the plan?
    ///
    /// The reconcile node is bookkeeping the routed graph hangs its summary off;
    /// deleting it leaves the plan's root dangling, so it has no delete affordance
    /// at all rather than a button that silently does nothing.
    pub fn is_removable(&self, id: TaskId) -> bool {
        Some(id) != self.reconcile_root
    }

    /// Why Execute cannot start, or `None` when it can.
    ///
    /// Returns a sentence rather than a bool because a disabled button with no
    /// explanation is indistinguishable from a broken one, which is exactly how
    /// this panel read when a hand-authored plan silently launched nothing.
    pub fn execution_blocker(&self) -> Option<&'static str> {
        if self.execution_running {
            return Some("Execution is already running.");
        }
        if self.plan_is_empty() {
            return Some("The plan is empty. Add a task, or route a goal.");
        }
        if !self.has_dispatchable_work() {
            return Some("The plan holds only its reconcile step, so there is nothing to run.");
        }
        if scheduler::detect_cycle(&self.graph) {
            return Some("Resolve the dependency cycle first.");
        }
        if !self.defaults.has_model()
            && self
                .graph
                .tasks
                .keys()
                .any(|id| Some(*id) != self.reconcile_root && !self.bindings.contains_key(id))
        {
            return Some("No model selected. Pick one in Settings or Chat first.");
        }
        None
    }

    /// Cut the dependency cycle(s) by dropping only the back-edges, and report
    /// precisely which ones went.
    ///
    /// Both repair buttons used to load `TaskGraph::example_game()`: they "fixed"
    /// a cycle by replacing the user's plan with nine demo tasks and losing every
    /// run result attached to it.
    pub fn repair_cycles_action(&mut self) -> Vec<(TaskId, TaskId)> {
        let removed = scheduler::break_cycles(&mut self.graph);
        self.repair_report = if removed.is_empty() {
            "Nothing to repair: the plan has no dependency cycle.".to_string()
        } else {
            let edges = removed
                .iter()
                .map(|(from, to)| format!("{from} depends on {to}"))
                .collect::<Vec<_>>()
                .join("; ");
            format!(
                "Removed {} back-edge(s) and kept all {} task(s): {}",
                removed.len(),
                self.graph.tasks.len(),
                edges
            )
        };
        self.runtime_status = if removed.is_empty() {
            self.runtime_status.clone()
        } else {
            "Cycle repaired".to_string()
        };
        removed
    }

    /// Ask the app to route a goal through the planner. The panel cannot call it:
    /// planning reaches the coordinator, the SiteMap and Mission Control, all of
    /// which live behind the app borrow that drawing this panel already holds.
    pub fn request_route_goal(&mut self, goal: &str) {
        let goal = goal.trim().to_string();
        if goal.is_empty() {
            return;
        }
        self.goal_draft = goal.clone();
        self.goal_input_open = false;
        self.repair_report.clear();
        self.runtime_status = "Routing goal...".to_string();
        self.route_request = Some(goal);
    }

    /// Give every dispatchable task without a binding one, built from the
    /// panel's defaults and the SiteMap root as of now.
    ///
    /// Returns `Err` when the SiteMap cannot be opened *and* there was actually
    /// something to bind, so the caller can report it instead of watching tasks
    /// sit pending.
    pub fn bind_unbound_tasks(&mut self, workspace_root: &Path) -> Result<(), String> {
        let pending: Vec<TaskId> = self
            .graph
            .tasks
            .keys()
            .copied()
            .filter(|id| !self.bindings.contains_key(id))
            .filter(|id| Some(*id) != self.reconcile_root)
            .collect();
        if pending.is_empty() {
            return Ok(());
        }
        // Refuse rather than invent a route: a binding with an empty model id
        // would launch a worker that cannot call anything, and a task left
        // Pending is one the panel can point at with a reason.
        if !self.defaults.has_model() {
            return Err(
                "No model selected: pick one in Settings or Chat before launching authored tasks."
                    .to_string(),
            );
        }
        let root = self.current_site_map_root(workspace_root);
        for id in pending {
            let Some(task) = self.graph.tasks.get(&id).cloned() else {
                continue;
            };
            let kind = self
                .authored_kinds
                .get(&id)
                .copied()
                .unwrap_or(self.defaults.task_kind);
            self.bindings
                .insert(id, manual_binding(&task, kind, &self.defaults, root));
        }
        Ok(())
    }

    /// Root of the workspace SiteMap, or 0 when it cannot be read. The dispatch
    /// loop already turns an unreadable SiteMap into a blocked task with the
    /// reason, so binding does not duplicate that report.
    fn current_site_map_root(&self, workspace_root: &Path) -> u64 {
        let site_map_path = workspace_root.join(".velocity").join("site_map");
        let weight_root = crate::automation::resolve_weight_root(workspace_root);
        velocity_ide::site_map::SiteMap::open(&site_map_path, weight_root)
            .map(|site_map| site_map.root())
            .unwrap_or(0)
    }
}
