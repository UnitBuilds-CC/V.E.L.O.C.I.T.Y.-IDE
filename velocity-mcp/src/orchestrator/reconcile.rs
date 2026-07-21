//! Detect file overlap/collision between parallel task outputs.

use std::collections::{HashMap, HashSet};

use super::blueprint::TaskGraph;
use super::TaskId;

/// A conflict between two tasks touching the same file.
#[derive(Debug, Clone)]
pub struct Collision {
    pub path: String,
    pub task_a: TaskId,
    pub task_b: TaskId,
}

/// Report files modified by more than one task.
pub fn detect_collisions(
    graph: &TaskGraph,
    outputs: &HashMap<TaskId, Vec<String>>,
) -> Vec<Collision> {
    let mut files_to_tasks: HashMap<String, Vec<TaskId>> = HashMap::new();

    for (task_id, files) in outputs {
        for file in files {
            files_to_tasks
                .entry(file.clone())
                .or_default()
                .push(*task_id);
        }
    }

    let mut collisions = Vec::new();
    for (path, tasks) in files_to_tasks {
        if tasks.len() > 1 {
            for i in 0..tasks.len() {
                for j in (i + 1)..tasks.len() {
                    collisions.push(Collision {
                        path: path.clone(),
                        task_a: tasks[i],
                        task_b: tasks[j],
                    });
                }
            }
        }
    }
    collisions
}

/// Report files touched by a task that were not in its declared scope.
pub fn scope_violations(
    graph: &TaskGraph,
    outputs: &HashMap<TaskId, Vec<String>>,
) -> Vec<(TaskId, String)> {
    let mut violations = Vec::new();
    for (task_id, files) in outputs {
        if let Some(task) = graph.tasks.get(task_id) {
            let scope: HashSet<_> = task.scope.iter().cloned().collect();
            for file in files {
                if scope.is_empty() {
                    continue;
                }
                let inside = scope.iter().any(|prefix| file.starts_with(prefix));
                if !inside {
                    violations.push((*task_id, file.clone()));
                }
            }
        }
    }
    violations
}
