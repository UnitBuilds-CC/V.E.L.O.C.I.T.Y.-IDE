//! Topological scheduling of tasks.

use std::collections::HashSet;

use super::blueprint::TaskGraph;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::blueprint::TaskGraph;

    fn linear_graph() -> TaskGraph {
        let mut g = TaskGraph::default();
        g.root = TaskId(1);
        g.add(TaskId(1), "First", "", vec![], vec![], None);
        g.add(TaskId(2), "Second", "", vec![], vec![TaskId(1)], None);
        g.add(TaskId(3), "Third", "", vec![], vec![TaskId(2)], None);
        g
    }

    fn parallel_graph() -> TaskGraph {
        let mut g = TaskGraph::default();
        g.root = TaskId(1);
        g.add(TaskId(1), "Root", "", vec![], vec![], None);
        g.add(TaskId(2), "A", "", vec![], vec![TaskId(1)], None);
        g.add(TaskId(3), "B", "", vec![], vec![TaskId(1)], None);
        g.add(TaskId(4), "C", "", vec![], vec![TaskId(1)], None);
        g
    }

    #[test]
    fn plan_linear_produces_three_phases() {
        let g = linear_graph();
        let p = plan(&g);
        assert_eq!(p.phases.len(), 3);
        assert_eq!(p.phases[0], vec![TaskId(1)]);
        assert_eq!(p.phases[1], vec![TaskId(2)]);
        assert_eq!(p.phases[2], vec![TaskId(3)]);
    }

    #[test]
    fn plan_parallel_groups_independent_tasks() {
        let g = parallel_graph();
        let p = plan(&g);
        assert_eq!(p.phases.len(), 2);
        assert_eq!(p.phases[0].len(), 1);
        assert_eq!(p.phases[1].len(), 3);
    }

    #[test]
    fn bfs_returns_flat_order() {
        let g = linear_graph();
        let order = bfs(&g);
        assert_eq!(order.len(), 3);
        assert_eq!(order[0], TaskId(1));
        assert_eq!(order[1], TaskId(2));
        assert_eq!(order[2], TaskId(3));
    }

    #[test]
    fn detect_cycle_returns_false_for_dag() {
        let g = linear_graph();
        assert!(!detect_cycle(&g));
    }

    #[test]
    fn detect_cycle_returns_true_for_cycle() {
        let mut g = TaskGraph::default();
        g.add(TaskId(1), "A", "", vec![], vec![TaskId(3)], None);
        g.add(TaskId(2), "B", "", vec![], vec![TaskId(1)], None);
        g.add(TaskId(3), "C", "", vec![], vec![TaskId(2)], None);
        assert!(detect_cycle(&g));
    }

    #[test]
    fn plan_with_cycle_stops_early() {
        let mut g = TaskGraph::default();
        g.add(TaskId(1), "A", "", vec![], vec![TaskId(2)], None);
        g.add(TaskId(2), "B", "", vec![], vec![TaskId(1)], None);
        let p = plan(&g);
        assert!(p.phases.is_empty());
    }

    #[test]
    fn plan_example_game_completes_all_tasks() {
        let g = TaskGraph::example_game();
        let p = plan(&g);
        let total: usize = p.phases.iter().map(|phase| phase.len()).sum();
        assert_eq!(total, 9);
    }
}
