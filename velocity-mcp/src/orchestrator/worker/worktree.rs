#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

#[allow(dead_code)]
pub struct WorktreeIsolationGuard {
    pub original_root: PathBuf,
    pub worktree_root: PathBuf,
    pub active: bool,
}

#[allow(dead_code)]
impl WorktreeIsolationGuard {
    pub fn new(original_root: &Path, subagent_id: &str) -> Result<Self, String> {
        let worktree_dir = original_root.join(".git").join("worktrees").join(format!("velocity_subagent_{}", subagent_id));
        if let Some(parent) = worktree_dir.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let output = Command::new("git")
            .args(&["worktree", "add", "--detach", worktree_dir.to_str().unwrap_or_default()])
            .current_dir(original_root)
            .output();

        let active = match output {
            Ok(res) => res.status.success(),
            Err(_) => false,
        };

        let target_root = if active {
            worktree_dir
        } else {
            original_root.to_path_buf()
        };

        Ok(Self {
            original_root: original_root.to_path_buf(),
            worktree_root: target_root,
            active,
        })
    }

    pub fn cleanup(&mut self) {
        if self.active {
            let _ = Command::new("git")
                .args(&["worktree", "remove", "--force", self.worktree_root.to_str().unwrap_or_default()])
                .current_dir(&self.original_root)
                .output();
            self.active = false;
        }
    }
}

impl Drop for WorktreeIsolationGuard {
    fn drop(&mut self) {
        self.cleanup();
    }
}
