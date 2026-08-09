//! Workflow versioning: history tracking and rollback support.
//!
//! Each time a workflow canvas is saved, a snapshot is stored. Users can
//! browse history, compare versions, and rollback to any previous state.

use super::workflow_canvas::WorkflowCanvas;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// A single version snapshot of a workflow canvas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowVersion {
    /// Monotonically increasing version number.
    pub version: u32,
    /// When this version was created.
    pub timestamp: u64,
    /// Optional user-provided note about what changed.
    pub note: String,
    /// The full canvas state at this version.
    pub canvas: WorkflowCanvas,
}

/// Version history for a single workflow.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkflowHistory {
    pub workflow_id: String,
    pub versions: Vec<WorkflowVersion>,
}

impl WorkflowHistory {
    pub fn new(workflow_id: &str) -> Self {
        Self {
            workflow_id: workflow_id.to_string(),
            versions: Vec::new(),
        }
    }

    /// Record a new version from the current canvas state.
    pub fn snapshot(&mut self, canvas: &WorkflowCanvas, note: &str) -> u32 {
        let version = self.versions.last().map(|v| v.version + 1).unwrap_or(1);
        let timestamp = now_secs();
        self.versions.push(WorkflowVersion {
            version,
            timestamp,
            note: note.to_string(),
            canvas: canvas.clone(),
        });
        version
    }

    /// Get a specific version by number.
    pub fn get_version(&self, version: u32) -> Option<&WorkflowVersion> {
        self.versions.iter().find(|v| v.version == version)
    }

    /// Get the latest version.
    pub fn latest(&self) -> Option<&WorkflowVersion> {
        self.versions.last()
    }

    /// Rollback: restore canvas to a specific version.
    /// Returns the canvas state at that version.
    pub fn rollback(&self, version: u32) -> Option<WorkflowCanvas> {
        self.get_version(version).map(|v| v.canvas.clone())
    }

    /// Number of versions in history.
    pub fn len(&self) -> usize {
        self.versions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.versions.is_empty()
    }

    /// Diff summary between two versions.
    pub fn diff(&self, v1: u32, v2: u32) -> Option<VersionDiff> {
        let ver1 = self.get_version(v1)?;
        let ver2 = self.get_version(v2)?;

        let nodes_added = ver2
            .canvas
            .nodes
            .len()
            .saturating_sub(ver1.canvas.nodes.len());
        let nodes_removed = ver1
            .canvas
            .nodes
            .len()
            .saturating_sub(ver2.canvas.nodes.len());
        let edges_added = ver2
            .canvas
            .edges
            .len()
            .saturating_sub(ver1.canvas.edges.len());
        let edges_removed = ver1
            .canvas
            .edges
            .len()
            .saturating_sub(ver2.canvas.edges.len());

        Some(VersionDiff {
            from_version: v1,
            to_version: v2,
            nodes_added,
            nodes_removed,
            edges_added,
            edges_removed,
            time_delta: ver2.timestamp.saturating_sub(ver1.timestamp),
        })
    }
}

/// Summary of changes between two versions.
#[derive(Debug, Clone)]
pub struct VersionDiff {
    pub from_version: u32,
    pub to_version: u32,
    pub nodes_added: usize,
    pub nodes_removed: usize,
    pub edges_added: usize,
    pub edges_removed: usize,
    pub time_delta: u64,
}

impl VersionDiff {
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.nodes_added > 0 {
            parts.push(format!("+{} nodes", self.nodes_added));
        }
        if self.nodes_removed > 0 {
            parts.push(format!("-{} nodes", self.nodes_removed));
        }
        if self.edges_added > 0 {
            parts.push(format!("+{} edges", self.edges_added));
        }
        if self.edges_removed > 0 {
            parts.push(format!("-{} edges", self.edges_removed));
        }
        if parts.is_empty() {
            "No structural changes".to_string()
        } else {
            parts.join(", ")
        }
    }
}

/// Registry managing version histories for all workflows.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VersionRegistry {
    pub histories: std::collections::HashMap<String, WorkflowHistory>,
}

impl VersionRegistry {
    /// Record a snapshot for a workflow.
    pub fn snapshot(&mut self, canvas: &WorkflowCanvas, note: &str) -> u32 {
        let history = self
            .histories
            .entry(canvas.workflow_id.clone())
            .or_insert_with(|| WorkflowHistory::new(&canvas.workflow_id));
        history.snapshot(canvas, note)
    }

    /// Get history for a workflow.
    pub fn history(&self, workflow_id: &str) -> Option<&WorkflowHistory> {
        self.histories.get(workflow_id)
    }

    /// Rollback a workflow canvas to a specific version.
    pub fn rollback(&self, workflow_id: &str, version: u32) -> Option<WorkflowCanvas> {
        self.histories.get(workflow_id)?.rollback(version)
    }

    /// Persist version histories to disk.
    pub fn save(&self, workspace_root: &std::path::Path) -> Result<(), String> {
        let dir = workspace_root.join(".velocity").join("workflow_versions");
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create version dir: {e}"))?;

        for (id, history) in &self.histories {
            let json = serde_json::to_vec_pretty(history)
                .map_err(|e| format!("version serialize failed: {e}"))?;
            std::fs::write(dir.join(format!("{id}.json")), json)
                .map_err(|e| format!("cannot write version file: {e}"))?;
        }
        Ok(())
    }

    /// Load version histories from disk.
    pub fn load(workspace_root: &std::path::Path) -> Self {
        let dir = workspace_root.join(".velocity").join("workflow_versions");
        let mut histories = std::collections::HashMap::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                if let Ok(bytes) = std::fs::read(&path) {
                    if let Ok(history) = serde_json::from_slice::<WorkflowHistory>(&bytes) {
                        histories.insert(history.workflow_id.clone(), history);
                    }
                }
            }
        }
        Self { histories }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::workflow_canvas::{CanvasNodeKind, NodePosition};

    fn test_canvas() -> WorkflowCanvas {
        let mut c = WorkflowCanvas::new("wf1", "Test");
        let start = c.nodes[0].id.clone();
        let end = c.nodes[1].id.clone();
        let mid = c.add_node(
            CanvasNodeKind::Tool {
                name: "test".into(),
                args: serde_json::json!({}),
            },
            NodePosition { x: 300.0, y: 200.0 },
        );
        c.add_edge(start, "ok", mid.clone());
        c.add_edge(mid, "ok", end);
        c
    }

    #[test]
    fn snapshot_and_rollback() {
        let mut history = WorkflowHistory::new("wf1");
        let canvas_v1 = test_canvas();
        history.snapshot(&canvas_v1, "initial");

        let mut canvas_v2 = canvas_v1.clone();
        canvas_v2.add_node(
            CanvasNodeKind::AgentTask {
                prompt: "test".into(),
                team: None,
            },
            NodePosition { x: 500.0, y: 300.0 },
        );
        history.snapshot(&canvas_v2, "added agent node");

        assert_eq!(history.len(), 2);
        assert_eq!(history.latest().unwrap().version, 2);

        let rolled_back = history.rollback(1).unwrap();
        assert_eq!(rolled_back.nodes.len(), canvas_v1.nodes.len());
    }

    #[test]
    fn diff_summary() {
        let mut history = WorkflowHistory::new("wf1");
        let canvas_v1 = test_canvas();
        history.snapshot(&canvas_v1, "v1");

        let mut canvas_v2 = canvas_v1.clone();
        canvas_v2.add_node(
            CanvasNodeKind::Tool {
                name: "new".into(),
                args: serde_json::json!({}),
            },
            NodePosition { x: 500.0, y: 300.0 },
        );
        canvas_v2.add_node(
            CanvasNodeKind::Tool {
                name: "new2".into(),
                args: serde_json::json!({}),
            },
            NodePosition { x: 700.0, y: 300.0 },
        );
        history.snapshot(&canvas_v2, "v2");

        let diff = history.diff(1, 2).unwrap();
        assert_eq!(diff.nodes_added, 2);
        assert!(diff.summary().contains("+2 nodes"));
    }

    #[test]
    fn version_registry_persistence() {
        let tmp = tempfile::tempdir().unwrap();
        let mut registry = VersionRegistry::default();
        let canvas = test_canvas();
        registry.snapshot(&canvas, "test version");
        registry.save(tmp.path()).unwrap();

        let loaded = VersionRegistry::load(tmp.path());
        assert_eq!(loaded.histories.len(), 1);
        assert!(loaded.history("wf1").is_some());
    }
}
