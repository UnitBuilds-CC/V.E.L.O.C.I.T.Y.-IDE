#![allow(dead_code)]
//! Workspace Checkpointing — git-stash-based snapshots before agent operations.
//!
//! Creates named checkpoints (git stash) before agents modify the workspace,
//! allowing one-click rollback if changes are unwanted.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// A recorded workspace checkpoint.
#[derive(Debug, Clone)]
pub struct Checkpoint {
    /// Unique checkpoint identifier (stash ref like "stash@{0}").
    pub stash_ref: String,
    /// Human-readable label (e.g. "Before agent: refactor auth module").
    pub label: String,
    /// When the checkpoint was created.
    pub created_at: Instant,
    /// Number of files that were modified at checkpoint time.
    pub files_changed: usize,
}

/// Manages workspace checkpoints backed by git stash.
#[derive(Debug, Clone, Default)]
pub struct CheckpointManager {
    /// List of checkpoints created in this session (most recent first).
    pub checkpoints: Vec<Checkpoint>,
    /// Whether checkpointing is enabled (requires a git repo).
    pub enabled: bool,
    /// Workspace root path.
    pub workspace_root: PathBuf,
}

impl CheckpointManager {
    pub fn new(workspace_root: &Path) -> Self {
        let enabled = workspace_root.join(".git").exists();
        Self {
            checkpoints: Vec::new(),
            enabled,
            workspace_root: workspace_root.to_path_buf(),
        }
    }

    /// Create a checkpoint before an agent operation.
    /// Returns `Ok(checkpoint_index)` or `Err(reason)`.
    pub fn create_checkpoint(&mut self, label: &str) -> Result<usize, String> {
        if !self.enabled {
            return Err("Not a git repository — checkpointing disabled".into());
        }

        // Check if there are any changes to stash
        let status = run_git(&self.workspace_root, &["status", "--porcelain"])?;
        if status.trim().is_empty() {
            // Nothing to checkpoint — workspace is clean
            return Err("Workspace is clean — no checkpoint needed".into());
        }

        let files_changed = status.lines().count();

        // Stage all changes (including untracked) and stash with a label
        let stash_message = format!("velocity-checkpoint: {}", label);
        run_git(&self.workspace_root, &["add", "-A"])?;
        run_git(
            &self.workspace_root,
            &["stash", "push", "--include-untracked", "-m", &stash_message],
        )?;

        // Get the stash ref (always stash@{0} after a push)
        let stash_ref = format!("stash@{{0}}");

        let checkpoint = Checkpoint {
            stash_ref: stash_ref.clone(),
            label: label.to_string(),
            created_at: Instant::now(),
            files_changed,
        };
        self.checkpoints.insert(0, checkpoint);
        Ok(0)
    }

    /// Restore workspace to a checkpoint (applies the stash and drops it).
    pub fn restore_checkpoint(&mut self, index: usize) -> Result<String, String> {
        if index >= self.checkpoints.len() {
            return Err("Invalid checkpoint index".into());
        }

        // Apply the stash
        let stash_ref = &self.checkpoints[index].stash_ref;
        run_git(&self.workspace_root, &["stash", "pop", stash_ref])?;

        let label = self.checkpoints[index].label.clone();
        self.checkpoints.remove(index);
        Ok(label)
    }

    /// Drop a checkpoint without restoring (discards the stash).
    pub fn discard_checkpoint(&mut self, index: usize) -> Result<String, String> {
        if index >= self.checkpoints.len() {
            return Err("Invalid checkpoint index".into());
        }

        let stash_ref = &self.checkpoints[index].stash_ref;
        run_git(&self.workspace_root, &["stash", "drop", stash_ref])?;

        let label = self.checkpoints[index].label.clone();
        self.checkpoints.remove(index);
        Ok(label)
    }

    /// List all stashes from git (not just session checkpoints).
    pub fn list_git_stashes(&self) -> Vec<String> {
        match run_git(&self.workspace_root, &["stash", "list"]) {
            Ok(output) => output.lines().map(String::from).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Number of available checkpoints.
    pub fn count(&self) -> usize {
        self.checkpoints.len()
    }
}

/// Run a git command in the workspace and return stdout.
fn run_git(workspace_root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace_root)
        .output()
        .map_err(|e| format!("git command failed: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("git {}: {}", args.join(" "), stderr.trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn checkpoint_manager_disabled_without_git() {
        let mgr = CheckpointManager::new(Path::new("/nonexistent/path"));
        assert!(!mgr.enabled);
    }

    #[test]
    fn checkpoint_manager_no_checkpoints_initially() {
        let mgr = CheckpointManager::new(Path::new("/tmp"));
        assert_eq!(mgr.count(), 0);
    }

    #[test]
    fn run_git_on_bad_path_returns_error() {
        let result = run_git(Path::new("/nonexistent_xyz"), &["status"]);
        assert!(result.is_err());
    }
}
