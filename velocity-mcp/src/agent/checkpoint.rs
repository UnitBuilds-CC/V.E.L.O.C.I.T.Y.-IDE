//! Agent workspace checkpointing for safe, reversible operations.
//!
//! Before each file-modifying tool batch, the agent creates a lightweight
//! git-based checkpoint. If the whole batch fails it rolls back to the last
//! good state (see `run_agent_reasoning_loop`), and on a clean session exit the
//! checkpoints are dropped.
//!
//! Snapshots are taken with `git stash create`, which produces a dangling stash
//! commit *without* mutating the working tree or the stash stack. The earlier
//! implementation used `stash push` + `stash pop`, which dropped the very stash
//! it created — leaving `restore` with nothing to apply.
#![allow(dead_code)] // a few entry points (restore_latest, list) are public API used situationally

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
    /// Overwrites tracked files in the working tree with the snapshot's content.
    pub fn restore(&mut self, checkpoint_id: usize) -> Result<(), String> {
        let git_ref = self
            .checkpoints
            .iter()
            .find(|c| c.id == checkpoint_id)
            .ok_or_else(|| format!("Checkpoint {} not found", checkpoint_id))?
            .git_ref
            .clone();

        // `git checkout <ref> -- .` rewrites every tracked path to the state it
        // had in the snapshot commit (or HEAD when the tree was clean at
        // checkpoint time), discarding the changes made after it.
        self.run_git(&["checkout", &git_ref, "--", "."])?;

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
        // Snapshots created with `git stash create` are dangling commits: they
        // are not on the stash stack and carry no refs, so git's gc reclaims
        // them automatically. We only need to clear our own tracking.
        self.checkpoints.clear();
        self.next_id = 1;
    }

    // ─── Internal helpers ────────────────────────────────────────────────────

    fn create_stash_checkpoint(&self, _label: &str) -> Option<String> {
        // `git stash create` builds a stash commit and prints its SHA without
        // touching the working tree or pushing onto the stash stack. That SHA is
        // a stable snapshot we can later restore from via `git checkout <sha> -- .`.
        let output = Command::new("git")
            .args(["stash", "create"])
            .current_dir(&self.workspace_root)
            .output()
            .ok()?;

        if !output.status.success() {
            // Fall back to HEAD (e.g. detached/edge states) so restore still works.
            return Some("HEAD".to_string());
        }

        let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if sha.is_empty() {
            // Nothing to stash: the working tree is clean, so HEAD *is* the
            // restore point.
            Some("HEAD".to_string())
        } else {
            Some(sha)
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

    /// End-to-end proof that a checkpoint can actually be restored. The previous
    /// implementation popped the stash it created, so this would have failed.
    /// Skips (passes trivially) when git is unavailable.
    #[test]
    fn checkpoint_restore_reverts_working_tree() {
        fn git(dir: &Path, args: &[&str]) -> Option<bool> {
            let out = Command::new("git").args(args).current_dir(dir).output().ok()?;
            Some(out.status.success())
        }

        // Unique temp workspace.
        let base = std::env::temp_dir().join(format!(
            "velocity_cp_test_{}",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        if std::fs::create_dir_all(&base).is_err() {
            return;
        }
        // git init + local identity (stash create builds a commit object).
        if git(&base, &["init", "--quiet"]) != Some(true) {
            let _ = std::fs::remove_dir_all(&base);
            return; // git not available — skip.
        }
        let _ = git(&base, &["config", "user.email", "t@t.local"]);
        let _ = git(&base, &["config", "user.name", "t"]);

        let file = base.join("f.txt");
        std::fs::write(&file, "original").unwrap();
        assert_eq!(git(&base, &["add", "."]), Some(true));
        assert_eq!(git(&base, &["commit", "-m", "init", "--quiet"]), Some(true));

        // Modify, then checkpoint that state.
        std::fs::write(&file, "checkpointed").unwrap();
        let mut mgr = CheckpointManager::new(&base);
        assert!(mgr.enabled);
        let cp = mgr.checkpoint("before edit").expect("checkpoint created");

        // Make a further (bad) edit, then roll back.
        std::fs::write(&file, "broken edit").unwrap();
        mgr.restore(cp).expect("restore succeeds");

        let restored = std::fs::read_to_string(&file).unwrap();
        assert_eq!(restored, "checkpointed", "restore must revert the later edit");

        let _ = std::fs::remove_dir_all(&base);
    }
}
