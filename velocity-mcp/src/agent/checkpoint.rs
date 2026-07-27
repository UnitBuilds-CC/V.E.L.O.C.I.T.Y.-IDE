//! Agent workspace checkpointing for safe, reversible operations.
//!
//! Before each file-modifying tool call, the agent creates a lightweight
//! git-based checkpoint. On failure, it can restore to the last good state.
//!
//! NOTE: Some restore/diff/cleanup entry points are part of the checkpoint API
//! surface and are not yet invoked from the live agent loop.
#![allow(dead_code)] // checkpoint API awaiting agent-loop integration

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Information about a single checkpoint.
#[derive(Debug, Clone)]
pub struct CheckpointInfo {
    /// Sequential checkpoint ID.
    pub id: usize,
    /// Human-readable label (e.g., "before write_file src/main.rs").
    pub label: String,
    /// Git stash reference or temp commit hash.
    pub git_ref: String,
    /// Unix timestamp when the checkpoint was created.
    pub created_at: u64,
    /// Number of files that were dirty at checkpoint time.
    pub dirty_files: usize,
}

/// Manages git-based workspace checkpoints for agent operations.
pub struct CheckpointManager {
    /// Workspace root (must be a git repository).
    workspace_root: PathBuf,
    /// All checkpoints created in this session.
    checkpoints: Vec<CheckpointInfo>,
    /// Next checkpoint ID.
    next_id: usize,
    /// Whether checkpointing is enabled.
    pub enabled: bool,
}

impl CheckpointManager {
    /// Create a new checkpoint manager for the given workspace.
    pub fn new(workspace_root: &Path) -> Self {
        let enabled = workspace_root.join(".git").exists();
        Self {
            workspace_root: workspace_root.to_path_buf(),
            checkpoints: Vec::new(),
            next_id: 1,
            enabled,
        }
    }

    /// Create a checkpoint before a potentially destructive operation.
    /// Returns the checkpoint ID, or None if checkpointing failed.
    pub fn checkpoint(&mut self, label: &str) -> Option<usize> {
        if !self.enabled {
            return None;
        }

        // Stage all current changes and create a stash entry as checkpoint
        let git_ref = self.create_stash_checkpoint(label)?;
        let dirty_files = self.count_dirty_files();

        let id = self.next_id;
        self.next_id += 1;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.checkpoints.push(CheckpointInfo {
            id,
            label: label.to_string(),
            git_ref,
            created_at: now,
            dirty_files,
        });

        Some(id)
    }

    /// Restore the workspace to a specific checkpoint.
    /// This applies the stash entry back, reverting any changes made after.
    pub fn restore(&mut self, checkpoint_id: usize) -> Result<(), String> {
        let info = self
            .checkpoints
            .iter()
            .find(|c| c.id == checkpoint_id)
            .ok_or_else(|| format!("Checkpoint {} not found", checkpoint_id))?;

        // Reset working tree to the checkpoint state
        // First, discard all current changes
        self.run_git(&["checkout", "--", "."])?;

        // Apply the stash entry (without dropping it)
        let stash_ref = &info.git_ref;
        self.run_git(&["stash", "apply", stash_ref])?;

        Ok(())
    }

    /// Restore to the most recent checkpoint.
    pub fn restore_latest(&mut self) -> Result<(), String> {
        let latest_id = self
            .checkpoints
            .last()
            .map(|c| c.id)
            .ok_or("No checkpoints available")?;
        self.restore(latest_id)
    }

    /// List all available checkpoints.
    pub fn list(&self) -> &[CheckpointInfo] {
        &self.checkpoints
    }

    /// Get the number of available checkpoints.
    pub fn count(&self) -> usize {
        self.checkpoints.len()
    }

    /// Show what changed since a specific checkpoint.
    pub fn diff_since(&self, checkpoint_id: usize) -> Result<String, String> {
        let _info = self
            .checkpoints
            .iter()
            .find(|c| c.id == checkpoint_id)
            .ok_or_else(|| format!("Checkpoint {} not found", checkpoint_id))?;

        // Show diff of working tree vs HEAD (approximation of changes since checkpoint)
        let output = Command::new("git")
            .args(["diff", "--stat"])
            .current_dir(&self.workspace_root)
            .output()
            .map_err(|e| format!("git diff failed: {}", e))?;

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Drop all checkpoints (cleanup at end of session).
    pub fn cleanup(&mut self) {
        // Drop stash entries we created (in reverse order to maintain indices)
        for info in self.checkpoints.iter().rev() {
            let _ = self.run_git(&["stash", "drop", &info.git_ref]);
        }
        self.checkpoints.clear();
        self.next_id = 1;
    }

    // ─── Internal helpers ────────────────────────────────────────────────────

    fn create_stash_checkpoint(&self, label: &str) -> Option<String> {
        // Create a stash entry with a message (keeps working tree intact)
        let stash_msg = format!("velocity-checkpoint: {}", label);
        let output = Command::new("git")
            .args(["stash", "push", "--keep-index", "-m", &stash_msg])
            .current_dir(&self.workspace_root)
            .output()
            .ok()?;

        if !output.status.success() {
            // If stash fails (e.g., nothing to stash), use HEAD as reference
            return Some("HEAD".to_string());
        }

        // Immediately pop it back (we just wanted the stash entry as a snapshot)
        let _ = Command::new("git")
            .args(["stash", "pop"])
            .current_dir(&self.workspace_root)
            .output();

        // Get the stash reference
        let list_output = Command::new("git")
            .args(["stash", "list", "--format=%gd", "-1"])
            .current_dir(&self.workspace_root)
            .output()
            .ok()?;

        let stash_ref = String::from_utf8_lossy(&list_output.stdout).trim().to_string();
        if stash_ref.is_empty() {
            Some("HEAD".to_string())
        } else {
            Some(stash_ref)
        }
    }

    fn count_dirty_files(&self) -> usize {
        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&self.workspace_root)
            .output();

        match output {
            Ok(o) => String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .count(),
            Err(_) => 0,
        }
    }

    fn run_git(&self, args: &[&str]) -> Result<String, String> {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.workspace_root)
            .output()
            .map_err(|e| format!("git {:?} failed: {}", args, e))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("git {:?} error: {}", args, stderr))
        }
    }
}

impl Drop for CheckpointManager {
    fn drop(&mut self) {
        // Don't auto-cleanup on drop — checkpoints should persist for the session
        // and be explicitly cleaned up or left for manual inspection.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_manager_creation() {
        let mgr = CheckpointManager::new(Path::new("."));
        assert_eq!(mgr.count(), 0);
        assert!(mgr.list().is_empty());
    }

    #[test]
    fn checkpoint_disabled_without_git() {
        let mgr = CheckpointManager::new(Path::new("/nonexistent/path"));
        assert!(!mgr.enabled);
    }

    #[test]
    fn checkpoint_info_fields() {
        let info = CheckpointInfo {
            id: 1,
            label: "test".to_string(),
            git_ref: "stash@{0}".to_string(),
            created_at: 1000,
            dirty_files: 3,
        };
        assert_eq!(info.id, 1);
        assert_eq!(info.dirty_files, 3);
    }
}
