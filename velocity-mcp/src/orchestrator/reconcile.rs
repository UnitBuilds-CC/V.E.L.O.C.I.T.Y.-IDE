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
    _graph: &TaskGraph,
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

/// Does `file` sit under the declared `prefix`?
///
/// Both sides are workspace-relative but written by different hands: the scope
/// comes from the planner's JSON and is `/`-joined, while an output is
/// `Path::display()` of a `strip_prefix` result, which is `\`-joined on Windows.
/// A raw string compare therefore matches nothing, and that failure is silent in
/// the worst direction — every touched file reads as out of scope, so a task
/// whose scope is the very directory it works in gets blocked. Measured on the
/// 2026-10-25 single-member port dispatch: scope `["rust_port/",
/// "rust_port/src/lib.rs"]`, output `rust_port\src\_internal_context.rs`, and
/// the run came back `blocked` with a green artifact beside it.
fn path_under_prefix(file: &str, prefix: &str) -> bool {
    file.replace('\\', "/")
        .starts_with(&prefix.replace('\\', "/"))
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
                let inside = scope.iter().any(|prefix| path_under_prefix(file, prefix));
                if !inside {
                    violations.push((*task_id, file.clone()));
                }
            }
        }
    }
    violations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::blueprint::Task;

    fn graph_with_scope(task_id: TaskId, scope: &[&str]) -> TaskGraph {
        let mut tasks = HashMap::new();
        tasks.insert(
            task_id,
            Task {
                id: task_id,
                title: "port".to_string(),
                description: String::new(),
                scope: scope.iter().map(|s| s.to_string()).collect(),
                dependencies: Vec::new(),
                output: None,
                priority: 0,
                estimated_weight: 1,
            },
        );
        TaskGraph {
            tasks,
            root: task_id,
        }
    }

    #[test]
    fn a_native_path_output_matches_the_forward_slash_scope() {
        // The dispatch that failed: the planner declared the crate directory and
        // the worker reported its own deliverable inside it, with native
        // separators. Separator-normalised or this comparison matches nothing.
        let task_id = TaskId(2);
        let graph = graph_with_scope(task_id, &["rust_port/", "rust_port/src/lib.rs"]);
        let native = format!(
            "rust_port{}src{}_internal_context.rs",
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR
        );
        let mut outputs = HashMap::new();
        outputs.insert(task_id, vec![native]);
        assert!(
            scope_violations(&graph, &outputs).is_empty(),
            "a file inside the declared scope blocked the task"
        );
    }

    #[test]
    fn a_backslash_scope_still_matches_a_forward_slash_output() {
        // Scope patterns also arrive from an operator's team definition, so the
        // normalisation has to hold in whichever direction the separators differ.
        let task_id = TaskId(3);
        let scope_dir = format!("rust_port{}src", std::path::MAIN_SEPARATOR);
        let graph = graph_with_scope(task_id, &[scope_dir.as_str()]);
        let mut outputs = HashMap::new();
        outputs.insert(task_id, vec!["rust_port/src/pool.rs".to_string()]);
        assert!(scope_violations(&graph, &outputs).is_empty());
    }

    #[test]
    fn a_file_really_outside_the_scope_is_still_a_violation() {
        // Non-vacuity: normalising separators must not turn the check into an
        // admission. One wrong byte after the root is still outside.
        let task_id = TaskId(4);
        let graph = graph_with_scope(task_id, &["rust_port/src/"]);
        let native = format!(
            "rust_port{}tests{}rogue.rs",
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR
        );
        let mut outputs = HashMap::new();
        outputs.insert(task_id, vec![native]);
        let violations = scope_violations(&graph, &outputs);
        assert_eq!(violations.len(), 1, "the outside file was admitted");
        assert!(violations[0].1.starts_with("rust_port"));
    }

    #[test]
    fn an_undeclared_scope_still_admits_everything() {
        // A task with no scope is unscoped, not blocked-by-default. This is the
        // branch a `broad()`-style fallback relies on.
        let task_id = TaskId(5);
        let graph = graph_with_scope(task_id, &[]);
        let mut outputs = HashMap::new();
        outputs.insert(task_id, vec!["anywhere/at/all.rs".to_string()]);
        assert!(scope_violations(&graph, &outputs).is_empty());
    }
}
