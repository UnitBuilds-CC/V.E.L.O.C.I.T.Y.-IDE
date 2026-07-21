//! Topological scheduling of tasks.

use std::collections::{HashSet, VecDeque};

use super::blueprint::{Task, TaskGraph};
use super::TaskId;

/// A queued execution plan derived from a [`TaskGraph`].
#[derive(Debug, Default)]
pub struct Plan {
    pub phases: Vec<Vec<TaskId>>,
}

/// Build a phase-based execution plan so tasks in the same phase are independent.
pub fn plan(graph: &TaskGraph) -> Plan {
    let mut completed: HashSet<TaskId> = HashSet::new();
    let mut phases: Vec<Vec<TaskId>> = Vec::new();

    while completed.len() < graph.tasks.len() {
        let ready: Vec<TaskId> = graph.ready(&completed).into_iter().map(|t| t.id).collect();
        if ready.is_empty() {
            break; // cycle or misconfiguration
        }
        completed.extend(ready.iter().copied());
        phases.push(ready);
    }

    Plan { phases }
}

/// Basic breadth-first ordering.
pub fn bfs(graph: &TaskGraph) -> Vec<TaskId> {
    plan(graph).phases.into_iter().flatten().collect()
}

/// Find any strongly connected components / cycles.
pub fn detect_cycle(graph: &TaskGraph) -> bool {
    let mut visited = HashSet::new();
    let mut stack = HashSet::new();

    fn dfs(
        graph: &TaskGraph,
        id: TaskId,
        visited: &mut HashSet<TaskId>,
        stack: &mut HashSet<TaskId>,
    ) -> bool {
        visited.insert(id);
        stack.insert(id);
        if let Some(task) = graph.tasks.get(&id) {
            for dep in &task.dependencies {
                if (!visited.contains(dep) && dfs(graph, *dep, visited, stack))
                    || stack.contains(dep)
                {
                    return true;
                }
            }
        }
        stack.remove(&id);
        false
    }

    for &id in graph.tasks.keys() {
        if !visited.contains(&id) && dfs(graph, id, &mut visited, &mut stack) {
            return true;
        }
    }
    false
}
