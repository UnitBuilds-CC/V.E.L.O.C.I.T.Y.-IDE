//! Decompose a project brief into a directed acyclic graph of work packages.

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
            if let Some(parent) = self.tasks.get_mut(&parent_id) {
                if !parent.dependencies.contains(&id) {
                    parent.dependencies.push(id);
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
            Some(TaskId(1)),
        );
        g.add(
            TaskId(3),
            "Physics system",
            "Spatial hash, rigid bodies, collisions, integrator.",
            vec!["crates/physics/".into()],
            vec![TaskId(1)],
            Some(TaskId(1)),
        );
        g.add(
            TaskId(4),
            "Entity Component System",
            "hecs integration, schedule, systems.",
            vec!["crates/ecs/".into()],
            vec![TaskId(1)],
            Some(TaskId(1)),
        );
        g.add(
            TaskId(5),
            "Asset pipeline",
            "glTF/obj loader, texture cache, hot reload.",
            vec!["crates/assets/".into()],
            vec![TaskId(1)],
            Some(TaskId(1)),
        );
        g.add(
            TaskId(6),
            "Audio engine",
            "cpal/wrapper, spatial audio, event triggers.",
            vec!["crates/audio/".into()],
            vec![TaskId(1)],
            Some(TaskId(1)),
        );
        g.add(
            TaskId(7),
            "Gameplay core",
            "Player input, game states, UI overlay, progression.",
            vec!["crates/gameplay/".into()],
            vec![TaskId(2), TaskId(3), TaskId(4), TaskId(6)],
            Some(TaskId(1)),
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
            Some(TaskId(1)),
        );
        g.add(
            TaskId(9),
            "Launcher & build scripts",
            "Main entry, CI, release packaging.",
            vec!["src/main.rs".into(), "Justfile".into(), ".github/".into()],
            vec![TaskId(8)],
            Some(TaskId(1)),
        );

        g
    }
}
