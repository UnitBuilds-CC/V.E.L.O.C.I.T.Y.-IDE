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
    Failed(String),
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
        Self { statuses, outputs: HashMap::new() }
    }

    pub fn is_complete(&self) -> bool {
        self.statuses.values().all(|s| matches!(s, TaskStatus::Done(_) | TaskStatus::Failed(_)))
    }

    pub fn ready_ids(&self, graph: &TaskGraph) -> Vec<TaskId> {
        let completed: std::collections::HashSet<_> = self
            .statuses
            .iter()
            .filter(|(_, s)| matches!(s, TaskStatus::Done(_) | TaskStatus::Failed(_)))
            .map(|(id, _)| *id)
            .collect();
        graph
            .ready(&completed)
            .into_iter()
            .filter(|task| matches!(self.statuses.get(&task.id), Some(TaskStatus::Pending) | None))
            .map(|t| t.id)
            .collect()
    }
}
