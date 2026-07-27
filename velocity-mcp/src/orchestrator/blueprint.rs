//! Decompose a project brief into a directed acyclic graph of work packages.
//!
//! NOTE: Some graph query methods (len/is_empty/get/dependents/leaves) are part
//! of the task-graph API and are exercised by tests ahead of full orchestrator
//! wiring, so they read as dead in the non-test build.
#![allow(dead_code)] // task-graph query API awaiting orchestrator integration

use std::collections::{HashMap, HashSet};

use super::TaskId;

/// A single unit of work that can be assigned to a sub-agent.
#[derive(Debug, Clone, Default)]
pub struct Task {
    pub id: TaskId,
    pub title: String,
    pub description: String,
    /// Files this task should focus on, if any. Used for sandboxing and collision scope.
    pub scope: Vec<String>,
    pub dependencies: Vec<TaskId>,
    #[allow(dead_code)]
    pub output: Option<String>,
}

/// A project plan modeled as a DAG.
#[derive(Debug, Default)]
pub struct TaskGraph {
    pub tasks: HashMap<TaskId, Task>,
    pub root: TaskId,
}

impl TaskGraph {
    /// Return tasks that have all dependencies satisfied by `completed`.
    pub fn ready(&self, completed: &HashSet<TaskId>) -> Vec<&Task> {
        self.tasks
            .values()
            .filter(|t| {
                !completed.contains(&t.id) && t.dependencies.iter().all(|d| completed.contains(d))
            })
            .collect()
    }

    /// Insert a new task and wire it under `parent` if one is provided.
    pub fn add(
        &mut self,
        id: TaskId,
        title: impl Into<String>,
        description: impl Into<String>,
        scope: Vec<String>,
        deps: Vec<TaskId>,
        parent: Option<TaskId>,
    ) {
        self.tasks.insert(
            id,
            Task {
                id,
                title: title.into(),
                description: description.into(),
                scope,
                dependencies: deps,
                output: None,
            },
        );
        if let Some(parent_id) = parent {
            if parent_id != id {
                if let Some(parent) = self.tasks.get_mut(&parent_id) {
                    if !parent.dependencies.contains(&id) {
                        parent.dependencies.push(id);
                    }
                }
            }
        }
    }

    /// A lexical blueprint for an expansive 3D game as a proof-of-concept task graph.
    pub fn example_game() -> Self {
        let mut g = TaskGraph::default();
        g.root = TaskId(1);

        g.add(
            TaskId(1),
            "Architecture & design doc",
            "Write ARCHITECTURE.md, data-flow diagrams, module boundaries.",
            vec!["docs/".into()],
            vec![],
            None,
        );
        g.add(
            TaskId(2),
            "Rendering engine",
            "wgpu abstraction, scene graph, camera, PBR materials.",
            vec!["crates/renderer/".into()],
            vec![TaskId(1)],
            None,
        );
        g.add(
            TaskId(3),
            "Physics system",
            "Spatial hash, rigid bodies, collisions, integrator.",
            vec!["crates/physics/".into()],
            vec![TaskId(1)],
            None,
        );
        g.add(
            TaskId(4),
            "Entity Component System",
            "hecs integration, schedule, systems.",
            vec!["crates/ecs/".into()],
            vec![TaskId(1)],
            None,
        );
        g.add(
            TaskId(5),
            "Asset pipeline",
            "glTF/obj loader, texture cache, hot reload.",
            vec!["crates/assets/".into()],
            vec![TaskId(1)],
            None,
        );
        g.add(
            TaskId(6),
            "Audio engine",
            "cpal/wrapper, spatial audio, event triggers.",
            vec!["crates/audio/".into()],
            vec![TaskId(1)],
            None,
        );
        g.add(
            TaskId(7),
            "Gameplay core",
            "Player input, game states, UI overlay, progression.",
            vec!["crates/gameplay/".into()],
            vec![TaskId(2), TaskId(3), TaskId(4), TaskId(6)],
            None,
        );
        g.add(
            TaskId(8),
            "Integration test harness",
            "Headless renderer, deterministic tests, bot scenarios.",
            vec!["tests/".into(), "crates/".into()],
            vec![
                TaskId(2),
                TaskId(3),
                TaskId(4),
                TaskId(5),
                TaskId(6),
                TaskId(7),
            ],
            None,
        );
        g.add(
            TaskId(9),
            "Launcher & build scripts",
            "Main entry, CI, release packaging.",
            vec!["src/main.rs".into(), "Justfile".into(), ".github/".into()],
            vec![TaskId(8)],
            None,
        );

        g
    }

    /// Return the total number of tasks in the graph.
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    /// Check if the graph has no tasks.
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Find a task by ID.
    pub fn get(&self, id: TaskId) -> Option<&Task> {
        self.tasks.get(&id)
    }

    /// Return all tasks that depend on the given task.
    pub fn dependents(&self, id: TaskId) -> Vec<&Task> {
        self.tasks
            .values()
            .filter(|t| t.dependencies.contains(&id))
            .collect()
    }

    /// Return all leaf tasks (no other task depends on them).
    pub fn leaves(&self) -> Vec<&Task> {
        let all_deps: HashSet<TaskId> = self
            .tasks
            .values()
            .flat_map(|t| t.dependencies.iter().copied())
            .collect();
        self.tasks
            .values()
            .filter(|t| !all_deps.contains(&t.id))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_graph() -> TaskGraph {
        let mut g = TaskGraph::default();
        g.root = TaskId(1);
        g.add(TaskId(1), "Root", "root task", vec![], vec![], None);
        g.add(TaskId(2), "Child A", "first child", vec!["a.rs".into()], vec![TaskId(1)], None);
        g.add(TaskId(3), "Child B", "second child", vec!["b.rs".into()], vec![TaskId(1)], None);
        g.add(
            TaskId(4),
            "Grandchild",
            "depends on A and B",
            vec![],
            vec![TaskId(2), TaskId(3)],
            None,
        );
        g
    }

    #[test]
    fn ready_returns_tasks_with_satisfied_deps() {
        let g = make_graph();
        let completed = HashSet::new();
        let ready = g.ready(&completed);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, TaskId(1));
    }

    #[test]
    fn ready_excludes_completed_tasks() {
        let g = make_graph();
        let mut completed = HashSet::new();
        completed.insert(TaskId(1));
        let ready = g.ready(&completed);
        assert_eq!(ready.len(), 2);
        let ids: HashSet<TaskId> = ready.iter().map(|t| t.id).collect();
        assert!(ids.contains(&TaskId(2)));
        assert!(ids.contains(&TaskId(3)));
    }

    #[test]
    fn ready_with_all_completed_returns_empty() {
        let g = make_graph();
        let completed: HashSet<TaskId> = g.tasks.keys().copied().collect();
        let ready = g.ready(&completed);
        assert!(ready.is_empty());
    }

    #[test]
    fn add_with_parent_wires_dependency() {
        let mut g = TaskGraph::default();
        g.add(TaskId(1), "Parent", "", vec![], vec![], None);
        g.add(TaskId(2), "Child", "", vec![], vec![], Some(TaskId(1)));
        let parent = g.get(TaskId(1)).unwrap();
        assert!(parent.dependencies.contains(&TaskId(2)));
    }

    #[test]
    fn add_self_parent_is_ignored() {
        let mut g = TaskGraph::default();
        g.add(TaskId(1), "Self", "", vec![], vec![], Some(TaskId(1)));
        let task = g.get(TaskId(1)).unwrap();
        assert!(task.dependencies.is_empty());
    }

    #[test]
    fn example_game_has_nine_tasks() {
        let g = TaskGraph::example_game();
        assert_eq!(g.len(), 9);
        assert_eq!(g.root, TaskId(1));
    }

    #[test]
    fn example_game_root_is_first_ready() {
        let g = TaskGraph::example_game();
        let completed = HashSet::new();
        let ready = g.ready(&completed);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, TaskId(1));
    }

    #[test]
    fn example_game_leaves_include_launcher() {
        let g = TaskGraph::example_game();
        let leaves = g.leaves();
        assert!(leaves.iter().any(|t| t.id == TaskId(9)));
    }

    #[test]
    fn dependents_returns_correct_tasks() {
        let g = make_graph();
        let deps = g.dependents(TaskId(1));
        assert_eq!(deps.len(), 2);
        let ids: HashSet<TaskId> = deps.iter().map(|t| t.id).collect();
        assert!(ids.contains(&TaskId(2)));
        assert!(ids.contains(&TaskId(3)));
    }

    #[test]
    fn len_and_is_empty() {
        let g = make_graph();
        assert_eq!(g.len(), 4);
        assert!(!g.is_empty());

        let empty = TaskGraph::default();
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
    }
}
