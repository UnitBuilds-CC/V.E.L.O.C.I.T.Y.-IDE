//! Central registry for task status and artifacts.

use std::collections::HashMap;

use super::blueprint::TaskGraph;
use super::worker::WorkerResult;
use super::TaskId;

#[derive(Debug, Clone, Default)]
pub enum TaskStatus {
    #[default]
    Pending,
    Running,
    Done(WorkerResult),
    Failed(WorkerResult),
    Blocked(WorkerResult),
}

#[derive(Debug, Default)]
pub struct OrchestratorRegistry {
    pub statuses: HashMap<TaskId, TaskStatus>,
    pub outputs: HashMap<TaskId, Vec<String>>,
}

impl OrchestratorRegistry {
    pub fn new(graph: &TaskGraph) -> Self {
        let mut statuses = HashMap::new();
        for id in graph.tasks.keys() {
            statuses.insert(*id, TaskStatus::Pending);
        }
        Self {
            statuses,
            outputs: HashMap::new(),
        }
    }

    pub fn is_complete(&self) -> bool {
        self.statuses.values().all(|s| {
            matches!(
                s,
                TaskStatus::Done(_) | TaskStatus::Failed(_) | TaskStatus::Blocked(_)
            )
        })
    }

    #[allow(dead_code)]
    pub fn has_blocked(&self) -> bool {
        self.statuses
            .values()
            .any(|s| matches!(s, TaskStatus::Blocked(_)))
    }

    pub fn ready_ids(&self, graph: &TaskGraph) -> Vec<TaskId> {
        let completed: std::collections::HashSet<_> = self
            .statuses
            .iter()
            .filter(|(_, s)| matches!(s, TaskStatus::Done(_)))
            .map(|(id, _)| *id)
            .collect();
        graph
            .ready(&completed)
            .into_iter()
            .filter(|task| {
                matches!(
                    self.statuses.get(&task.id),
                    Some(TaskStatus::Pending) | None
                )
            })
            .map(|t| t.id)
            .collect()
    }

    /// [`ready_ids`] narrowed to the first conflict-free wave: tasks are
    /// considered in priority order and any task whose scope collides —
    /// textual overlap, or both touching a shared historical conflict group
    /// (see [`crate::registry::event_store::conflict_file_groups`]) — with an
    /// already-accepted task is deferred to a later tick instead of racing it
    /// and burning a mediator rejection. Tasks with no declared scope never
    /// defer; runtime scope locks remain their safety net.
    pub fn ready_ids_conflict_aware(
        &self,
        graph: &TaskGraph,
        groups: &[std::collections::HashSet<String>],
    ) -> Vec<TaskId> {
        use super::scheduler::scopes_conflict;
        let scope_of = |id: &TaskId| -> &[String] {
            graph
                .tasks
                .get(id)
                .map(|t| t.scope.as_slice())
                .unwrap_or(&[])
        };
        let mut accepted: Vec<TaskId> = Vec::new();
        for id in self.ready_ids(graph) {
            if accepted
                .iter()
                .all(|a| !scopes_conflict(scope_of(a), scope_of(&id), groups))
            {
                accepted.push(id);
            }
        }
        accepted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::blueprint::TaskGraph;

    fn sample_result(task_id: TaskId) -> WorkerResult {
        WorkerResult {
            success: true,
            task_id,
            outputs: Vec::new(),
            duration: std::time::Duration::ZERO,
            message: "ok".to_string(),
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
    fn ready_ids_require_successful_dependencies() {
        let graph = TaskGraph::example_game();
        let mut registry = OrchestratorRegistry::new(&graph);
        registry
            .statuses
            .insert(TaskId(1), TaskStatus::Failed(sample_result(TaskId(1))));
        assert!(registry.ready_ids(&graph).is_empty());

        registry
            .statuses
            .insert(TaskId(1), TaskStatus::Done(sample_result(TaskId(1))));
        let ready = registry.ready_ids(&graph);
        assert!(ready.contains(&TaskId(2)));
        assert!(ready.contains(&TaskId(3)));
    }

    #[test]
    fn conflict_aware_ready_defers_scope_collisions() {
        // Two undependent tasks over the same territory: only the
        // higher-priority one may run this tick; the other defers.
        let mut g = TaskGraph::default();
        g.add(TaskId(1), "A", "", vec!["src/db/".into()], vec![], None);
        g.add(
            TaskId(2),
            "B",
            "",
            vec!["src/db/pool.rs".into()],
            vec![],
            None,
        );
        g.tasks.get_mut(&TaskId(1)).unwrap().priority = 5;
        let mut registry = OrchestratorRegistry::new(&g);
        registry.statuses.insert(TaskId(1), TaskStatus::Pending);
        registry.statuses.insert(TaskId(2), TaskStatus::Pending);
        let wave = registry.ready_ids_conflict_aware(&g, &[]);
        assert_eq!(wave, vec![TaskId(1)], "priority winner takes the scope");

        // A historical conflict group defers tasks whose paths never
        // overlap textually but share a contention hot area.
        let mut g2 = TaskGraph::default();
        g2.add(
            TaskId(1),
            "A",
            "",
            vec!["auth/login.rs".into()],
            vec![],
            None,
        );
        g2.add(
            TaskId(2),
            "B",
            "",
            vec!["store/session.rs".into()],
            vec![],
            None,
        );
        g2.tasks.get_mut(&TaskId(1)).unwrap().priority = 5;
        let groups: Vec<std::collections::HashSet<String>> =
            vec![std::collections::HashSet::from([
                "auth/login.rs".to_string(),
                "store/session.rs".to_string(),
            ])];
        let registry2 = OrchestratorRegistry::new(&g2);
        assert_eq!(registry2.ready_ids_conflict_aware(&g2, &[]).len(), 2);
        assert_eq!(
            registry2.ready_ids_conflict_aware(&g2, &groups),
            vec![TaskId(1)]
        );
    }
}
