//! Topological scheduling of tasks, conflict-aware at the scope level.

use std::collections::{HashMap, HashSet};

use super::blueprint::TaskGraph;
use super::TaskId;

/// A queued execution plan derived from a [`TaskGraph`].
#[derive(Debug, Default)]
pub struct Plan {
    pub phases: Vec<Vec<TaskId>>,
}

/// Normalize a scope/path entry for overlap comparison: forward slashes,
/// no leading/trailing slashes, lowercase (workspace paths are matched
/// case-insensitively on the target platforms).
pub fn normalize_scope(entry: &str) -> String {
    entry.replace('\\', "/").trim_matches('/').to_lowercase()
}

/// Whether two scope entries address the same territory: exact match after
/// normalization, or one containing the other as a directory prefix.
pub fn path_overlaps(a: &str, b: &str) -> bool {
    let a = normalize_scope(a);
    let b = normalize_scope(b);
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/"))
}

/// Whether any scope entry touches a file in a (normalized) file group.
fn touches_group(scope: &[String], group: &HashSet<String>) -> bool {
    scope
        .iter()
        .any(|e| group.iter().any(|f| path_overlaps(e, f)))
}

/// Whether two declared scopes must not run concurrently. Two collision
/// channels are considered:
/// * textual scope overlap (one path contains the other, or they are equal);
/// * a shared *historical conflict group* — sets of files that the durable
///   event store recorded together in past shared-write conflicts, so two
///   scopes touching the same group are serialized even when their declared
///   paths never overlap (the semantic collisions the runtime mediator
///   catches too late).
///
/// A task with no declared scope never conflicts here: the runtime scope
/// locks and the mediator remain its safety net.
pub fn scopes_conflict(a: &[String], b: &[String], groups: &[HashSet<String>]) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a.iter().any(|x| b.iter().any(|y| path_overlaps(x, y))) {
        return true;
    }
    groups
        .iter()
        .any(|g| touches_group(a, g) && touches_group(b, g))
}

/// Greedily split one ready wave into conflict-free sub-waves, preserving
/// the incoming (priority-descending) order: each task joins the earliest
/// sub-wave it can share with nobody it collides with.
pub fn split_conflict_free(
    graph: &TaskGraph,
    wave: &[TaskId],
    groups: &[HashSet<String>],
) -> Vec<Vec<TaskId>> {
    let scope_of = |id: &TaskId| -> &[String] {
        graph
            .tasks
            .get(id)
            .map(|t| t.scope.as_slice())
            .unwrap_or(&[])
    };
    let mut waves: Vec<Vec<TaskId>> = Vec::new();
    for &id in wave {
        let scope = scope_of(&id);
        let target = waves.iter_mut().find(|w| {
            !w.iter()
                .any(|other| scopes_conflict(scope_of(other), scope, groups))
        });
        match target {
            Some(w) => w.push(id),
            None => waves.push(vec![id]),
        }
    }
    waves
}

/// Build a phase-based execution plan so tasks in the same phase are
/// independent both by dependency edges *and* by write scope: phases that
/// would collide are split into sequential sub-phases, so the plan itself
/// never schedules a shared-write race the mediator would have to reject at
/// runtime. No conflict history is consulted; see [`plan_with_conflicts`].
pub fn plan(graph: &TaskGraph) -> Plan {
    plan_with_conflicts(graph, &[])
}

/// [`plan`] with historical conflict groups (as produced by
/// [`crate::registry::event_store::conflict_file_groups`): file sets that
/// conflicted in past sessions also get serialized.
pub fn plan_with_conflicts(graph: &TaskGraph, groups: &[HashSet<String>]) -> Plan {
    let mut completed: HashSet<TaskId> = HashSet::new();
    let mut phases: Vec<Vec<TaskId>> = Vec::new();

    while completed.len() < graph.tasks.len() {
        let ready: Vec<TaskId> = graph.ready(&completed).into_iter().map(|t| t.id).collect();
        if ready.is_empty() {
            break; // cycle or misconfiguration
        }
        // graph.ready() already sorts by priority (highest first); splitting
        // keeps that order, so the most urgent task owns the shared scope and
        // lower-priority collisions queue behind it.
        for wave in split_conflict_free(graph, &ready, groups) {
            phases.push(wave);
        }
        completed.extend(ready.iter().copied());
    }

    Plan { phases }
}

/// Basic breadth-first ordering.
pub fn bfs(graph: &TaskGraph) -> Vec<TaskId> {
    plan(graph).phases.into_iter().flatten().collect()
}

/// Whether a time-based cache of historical conflict groups should be
/// recomputed. `age` is the time since the last refresh (`None` = never
/// refreshed). Pure, so the plan-preview refresh cadence is unit-testable
/// without wall-clock sleeps: the refresh itself is disk-bound and must not
/// run on every UI repaint.
pub fn conflict_cache_is_stale(age: Option<std::time::Duration>, ttl: std::time::Duration) -> bool {
    match age {
        None => true,
        Some(a) => a >= ttl,
    }
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

/// One edge whose dependency points back at a node already on the DFS path.
///
/// `detect_cycle` can say a plan is impossible but not *which* claim makes it
/// impossible, which left the panel with a single "repair": discard the plan
/// and load a canned example. This walks the graph with an explicit stack
/// (deep plans do not grow the OS stack) in ascending task-id order, so the
/// same plan always yields the same answer.
fn find_back_edge(graph: &TaskGraph) -> Option<(TaskId, TaskId)> {
    const WHITE: u8 = 0;
    const GREY: u8 = 1;
    const BLACK: u8 = 2;

    let mut color: HashMap<TaskId, u8> = graph.tasks.keys().map(|id| (*id, WHITE)).collect();
    let mut roots: Vec<TaskId> = color.keys().cloned().collect();
    roots.sort_by_key(|id| id.0);

    for root in roots {
        if color.get(&root) != Some(&WHITE) {
            continue;
        }
        color.insert(root, GREY);
        let mut stack: Vec<(TaskId, usize)> = vec![(root, 0)];
        while let Some((node, dep_idx)) = stack.pop() {
            let deps = graph
                .tasks
                .get(&node)
                .map(|task| task.dependencies.as_slice())
                .unwrap_or(&[]);
            let Some(&dep) = deps.get(dep_idx) else {
                color.insert(node, BLACK);
                continue;
            };
            stack.push((node, dep_idx + 1));
            match color.get(&dep).copied().unwrap_or(BLACK) {
                // Dependency of a missing task: it can lead nowhere, so it
                // cannot close a cycle.
                GREY => return Some((node, dep)),
                WHITE => {
                    color.insert(dep, GREY);
                    stack.push((dep, 0));
                }
                _ => {}
            }
        }
    }
    None
}

/// Drop the back edges that make a plan unschedulable, keeping every task.
///
/// Returns the `(task, dependency)` pairs that were removed, in the order they
/// were found. Callers should surface them: a plan that had to lose a
/// dependency is not the plan the user wrote, and quietly editing it in place
/// is exactly the kind of silent fix this codebase keeps having to walk back.
pub fn break_cycles(graph: &mut TaskGraph) -> Vec<(TaskId, TaskId)> {
    let edge_budget: usize = graph
        .tasks
        .values()
        .map(|task| task.dependencies.len())
        .sum::<usize>()
        + 1;
    let mut removed = Vec::new();
    // Each iteration deletes a distinct edge, so this terminates in at most
    // `edge_budget` rounds; the bound just stops a logic slip from spinning.
    while removed.len() < edge_budget {
        let Some((node, dep)) = find_back_edge(graph) else {
            break;
        };
        if let Some(task) = graph.tasks.get_mut(&node) {
            task.dependencies.retain(|d| *d != dep);
        }
        removed.push((node, dep));
    }
    removed
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

    #[test]
    fn break_cycles_keeps_every_task_and_restores_a_plan() {
        let mut g = TaskGraph::default();
        g.add(TaskId(1), "A", "", vec![], vec![TaskId(3)], None);
        g.add(TaskId(2), "B", "", vec![], vec![TaskId(1)], None);
        g.add(TaskId(3), "C", "", vec![], vec![TaskId(2)], None);
        assert!(detect_cycle(&g));

        let removed = break_cycles(&mut g);
        assert_eq!(removed.len(), 1, "one back edge closes the loop");
        assert!(!detect_cycle(&g));
        assert_eq!(g.tasks.len(), 3, "repairing a cycle must not delete work");
        let total: usize = plan(&g).phases.iter().map(Vec::len).sum();
        assert_eq!(total, 3, "the repaired plan schedules every task");
    }

    #[test]
    fn break_cycles_on_dag_changes_nothing() {
        let mut g = linear_graph();
        let before = g
            .tasks
            .iter()
            .map(|(id, t)| (*id, t.dependencies.clone()))
            .collect::<Vec<_>>();
        assert!(break_cycles(&mut g).is_empty());
        let after = g
            .tasks
            .iter()
            .map(|(id, t)| (*id, t.dependencies.clone()))
            .collect::<Vec<_>>();
        assert_eq!(before, after);
    }

    #[test]
    fn break_cycles_removes_self_dependency() {
        let mut g = TaskGraph::default();
        g.add(TaskId(1), "Loop", "", vec![], vec![TaskId(1)], None);
        let removed = break_cycles(&mut g);
        assert_eq!(removed, vec![(TaskId(1), TaskId(1))]);
        assert!(g.get(TaskId(1)).unwrap().dependencies.is_empty());
    }

    #[test]
    fn break_cycles_separates_two_independent_loops() {
        let mut g = TaskGraph::default();
        g.add(TaskId(1), "A", "", vec![], vec![TaskId(2)], None);
        g.add(TaskId(2), "B", "", vec![], vec![TaskId(1)], None);
        g.add(TaskId(3), "C", "", vec![], vec![TaskId(4)], None);
        g.add(TaskId(4), "D", "", vec![], vec![TaskId(3)], None);
        assert_eq!(break_cycles(&mut g).len(), 2);
        assert!(!detect_cycle(&g));
        assert_eq!(plan(&g).phases.iter().map(Vec::len).sum::<usize>(), 4);
    }

    #[test]
    fn break_cycles_ignores_dependency_on_a_missing_task() {
        // A dangling id is not a cycle: it has no edges of its own to follow.
        let mut g = TaskGraph::default();
        g.add(TaskId(1), "A", "", vec![], vec![TaskId(99)], None);
        assert!(!detect_cycle(&g));
        assert!(break_cycles(&mut g).is_empty());
    }

    #[test]
    fn path_overlaps_covers_dirs_files_and_separator_cases() {
        assert!(path_overlaps("src/db", "src/db/"));
        assert!(path_overlaps("src/db/", "src/db/pool.rs"));
        assert!(path_overlaps("src\\db\\pool.rs", "src/db/pool.rs"));
        assert!(path_overlaps("SRC/DB", "src/db/pool.rs"));
        assert!(!path_overlaps("src/database", "src/db"));
        assert!(!path_overlaps("docs", "src/db"));
        assert!(!path_overlaps("", "src"));
    }

    #[test]
    fn plan_serializes_textually_overlapping_scopes() {
        // Two independent tasks fighting over the same territory: the
        // higher-priority one owns the phase, the collision queues behind it
        // instead of scheduling a race the mediator would have to reject.
        let mut g = TaskGraph::default();
        g.add(
            TaskId(1),
            "Pool rewrite",
            "",
            vec!["src/db/".into()],
            vec![],
            None,
        );
        g.add(
            TaskId(2),
            "Schema tweak",
            "",
            vec!["src/db/schema.rs".into()],
            vec![],
            None,
        );
        g.tasks.get_mut(&TaskId(1)).unwrap().priority = 5;
        g.tasks.get_mut(&TaskId(2)).unwrap().priority = 1;
        let p = plan(&g);
        assert_eq!(p.phases, vec![vec![TaskId(1)], vec![TaskId(2)]]);
    }

    #[test]
    fn plan_keeps_independent_and_unscoped_tasks_parallel() {
        let mut g = TaskGraph::default();
        g.add(TaskId(1), "A", "", vec!["crates/a/".into()], vec![], None);
        g.add(TaskId(2), "B", "", vec!["crates/b/".into()], vec![], None);
        g.add(TaskId(3), "C", "", vec![], vec![], None);
        let p = plan(&g);
        assert_eq!(p.phases.len(), 1, "no collisions -> one parallel wave");
        assert_eq!(p.phases[0].len(), 3);
    }

    #[test]
    fn plan_with_conflict_groups_serializes_semantic_collisions() {
        // Scopes that never overlap textually but both touch a file set that
        // collided in a past session are split across phases by history.
        let mut g = TaskGraph::default();
        g.add(
            TaskId(1),
            "Auth fix",
            "",
            vec!["auth/login.rs".into()],
            vec![],
            None,
        );
        g.add(
            TaskId(2),
            "Session tweak",
            "",
            vec!["store/session.rs".into()],
            vec![],
            None,
        );
        g.tasks.get_mut(&TaskId(1)).unwrap().priority = 3;
        g.tasks.get_mut(&TaskId(2)).unwrap().priority = 2;
        let groups: Vec<HashSet<String>> = vec![HashSet::from([
            "auth/login.rs".to_string(),
            "store/session.rs".to_string(),
        ])];
        assert_eq!(plan(&g).phases.len(), 1, "without history they race");
        let p = plan_with_conflicts(&g, &groups);
        assert_eq!(p.phases, vec![vec![TaskId(1)], vec![TaskId(2)]]);
    }

    #[test]
    fn conflict_cache_is_stale_gates_the_preview_refresh() {
        use std::time::Duration;
        let ttl = Duration::from_millis(1000);
        // Never refreshed -> stale (must load).
        assert!(conflict_cache_is_stale(None, ttl));
        // Fresh under the TTL -> not stale (skip the disk read).
        assert!(!conflict_cache_is_stale(
            Some(Duration::from_millis(0)),
            ttl
        ));
        assert!(!conflict_cache_is_stale(
            Some(Duration::from_millis(999)),
            ttl
        ));
        // At or past the TTL -> stale again.
        assert!(conflict_cache_is_stale(
            Some(Duration::from_millis(1000)),
            ttl
        ));
        assert!(conflict_cache_is_stale(Some(Duration::from_secs(30)), ttl));
    }
}
