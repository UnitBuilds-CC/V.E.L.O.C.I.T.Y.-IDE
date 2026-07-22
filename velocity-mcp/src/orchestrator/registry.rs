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
}
