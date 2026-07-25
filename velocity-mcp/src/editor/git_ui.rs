#![allow(dead_code)]
//! Git integration — stage, commit, diff, blame, branch UI.
//!
//! Provides real git operations by invoking the git CLI and parsing output.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Git file status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitFileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
}

impl GitFileStatus {
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Modified => "M",
            Self::Added => "A",
            Self::Deleted => "D",
            Self::Renamed => "R",
            Self::Untracked => "?",
            Self::Conflicted => "!",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Modified => "Modified",
            Self::Added => "Added",
            Self::Deleted => "Deleted",
            Self::Renamed => "Renamed",
            Self::Untracked => "Untracked",
            Self::Conflicted => "Conflicted",
        }
    }
}

/// A file with git status.
#[derive(Debug, Clone)]
pub struct GitStatusEntry {
    pub path: PathBuf,
    pub status: GitFileStatus,
    pub staged: bool,
}

/// A git blame entry for a line.
#[derive(Debug, Clone)]
pub struct BlameLine {
    pub commit_hash: String,
    pub author: String,
    pub date: String,
    pub line_content: String,
}

/// A git log entry.
#[derive(Debug, Clone)]
pub struct GitLogEntry {
    pub hash: String,
    pub short_hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
}

/// Git integration state.
#[derive(Debug, Clone, Default)]
pub struct GitState {
    pub branch: String,
    pub ahead: usize,
    pub behind: usize,
    pub entries: Vec<GitStatusEntry>,
    pub commit_message: String,
    pub log: Vec<GitLogEntry>,
    pub diff_output: String,
    pub last_error: Option<String>,
}

impl GitState {
    /// Create a GitState populated from the given workspace root.
    pub fn from_workspace(workspace_root: &Path) -> Self {
        let mut state = Self::default();
        state.refresh(workspace_root);
        state
    }

    /// Refresh git status from the workspace.
    pub fn refresh(&mut self, workspace_root: &Path) {
        self.last_error = None;
        self.refresh_branch(workspace_root);
        self.refresh_status(workspace_root);
    }

    fn refresh_branch(&mut self, root: &Path) {
        if let Some(output) = run_git(root, &["branch", "--show-current"]) {
            self.branch = output.trim().to_string();
        }
        // Ahead/behind
        if let Some(output) = run_git(root, &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"]) {
            let parts: Vec<&str> = output.trim().split_whitespace().collect();
            self.ahead = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
            self.behind = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        }
    }

    fn refresh_status(&mut self, root: &Path) {
        self.entries.clear();
        if let Some(output) = run_git(root, &["status", "--porcelain=v1"]) {
            for line in output.lines() {
                if line.len() < 4 { continue; }
                let index_status = line.as_bytes()[0] as char;
                let worktree_status = line.as_bytes()[1] as char;
                let file_path = PathBuf::from(line[3..].trim());

                let (status, staged) = match (index_status, worktree_status) {
                    ('M', _) => (GitFileStatus::Modified, true),
                    ('A', _) => (GitFileStatus::Added, true),
                    ('D', _) => (GitFileStatus::Deleted, true),
                    ('R', _) => (GitFileStatus::Renamed, true),
                    (_, 'M') => (GitFileStatus::Modified, false),
                    (_, 'D') => (GitFileStatus::Deleted, false),
                    ('?', '?') => (GitFileStatus::Untracked, false),
                    ('U', _) | (_, 'U') => (GitFileStatus::Conflicted, false),
                    _ => continue,
                };

                self.entries.push(GitStatusEntry { path: file_path, status, staged });
            }
        }
    }

    /// Stage a file.
    pub fn stage_file(&mut self, workspace_root: &Path, path: &Path) {
        run_git(workspace_root, &["add", &path.display().to_string()]);
        self.refresh_status(workspace_root);
    }

    /// Unstage a file.
    pub fn unstage_file(&mut self, workspace_root: &Path, path: &Path) {
        run_git(workspace_root, &["restore", "--staged", &path.display().to_string()]);
        self.refresh_status(workspace_root);
    }

    /// Stage all files.
    pub fn stage_all(&mut self, workspace_root: &Path) {
        run_git(workspace_root, &["add", "-A"]);
        self.refresh_status(workspace_root);
    }

    /// Commit staged changes.
    pub fn commit(&mut self, workspace_root: &Path) -> Result<(), String> {
        if self.commit_message.trim().is_empty() {
            return Err("Commit message cannot be empty".to_string());
        }
        let result = run_git(workspace_root, &["commit", "-m", &self.commit_message]);
        if result.is_some() {
            self.commit_message.clear();
            self.refresh(workspace_root);
            Ok(())
        } else {
            Err("Commit failed".to_string())
        }
    }

    /// Get diff for a specific file.
    pub fn diff_file(&mut self, workspace_root: &Path, path: &Path) {
        if let Some(output) = run_git(workspace_root, &["diff", &path.display().to_string()]) {
            self.diff_output = output;
        } else if let Some(output) = run_git(workspace_root, &["diff", "--cached", &path.display().to_string()]) {
            self.diff_output = output;
        }
    }

    /// Get blame for a file.
    pub fn blame_file(workspace_root: &Path, path: &Path) -> Vec<BlameLine> {
        let mut results = Vec::new();
        if let Some(output) = run_git(workspace_root, &["blame", "--porcelain", &path.display().to_string()]) {
            let mut current_hash = String::new();
            let mut current_author = String::new();
            let mut current_date = String::new();

            for line in output.lines() {
                if line.len() >= 40 && line.chars().take(40).all(|c| c.is_ascii_hexdigit()) {
                    current_hash = line[..8].to_string();
                } else if let Some(author) = line.strip_prefix("author ") {
                    current_author = author.to_string();
                } else if let Some(time) = line.strip_prefix("author-time ") {
                    current_date = time.to_string();
                } else if let Some(content) = line.strip_prefix('\t') {
                    results.push(BlameLine {
                        commit_hash: current_hash.clone(),
                        author: current_author.clone(),
                        date: current_date.clone(),
                        line_content: content.to_string(),
                    });
                }
            }
        }
        results
    }

    /// Get recent log entries.
    pub fn refresh_log(&mut self, workspace_root: &Path) {
        self.log.clear();
        if let Some(output) = run_git(workspace_root, &["log", "--oneline", "-30", "--format=%H|%h|%an|%ar|%s"]) {
            for line in output.lines() {
                let parts: Vec<&str> = line.splitn(5, '|').collect();
                if parts.len() == 5 {
                    self.log.push(GitLogEntry {
                        hash: parts[0].to_string(),
                        short_hash: parts[1].to_string(),
                        author: parts[2].to_string(),
                        date: parts[3].to_string(),
                        message: parts[4].to_string(),
                    });
                }
            }
        }
    }

    /// Get list of branches.
    pub fn branches(workspace_root: &Path) -> Vec<String> {
        run_git(workspace_root, &["branch", "--list", "--format=%(refname:short)"])
            .map(|output| output.lines().map(|l| l.trim().to_string()).collect())
            .unwrap_or_default()
    }

    /// Switch to a branch.
    pub fn checkout_branch(&mut self, workspace_root: &Path, branch: &str) -> Result<(), String> {
        run_git(workspace_root, &["checkout", branch])
            .map(|_| { self.refresh(workspace_root); })
            .ok_or_else(|| "Checkout failed".to_string())
    }

    /// Create and switch to a new branch.
    pub fn create_branch(&mut self, workspace_root: &Path, name: &str) -> Result<(), String> {
        run_git(workspace_root, &["checkout", "-b", name])
            .map(|_| { self.refresh(workspace_root); })
            .ok_or_else(|| "Branch creation failed".to_string())
    }

    pub fn staged_count(&self) -> usize {
        self.entries.iter().filter(|e| e.staged).count()
    }

    pub fn unstaged_count(&self) -> usize {
        self.entries.iter().filter(|e| !e.staged).count()
    }
}

/// Run a git command and return stdout on success.
fn run_git(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_status_modified() {
        let entry = GitStatusEntry {
            path: PathBuf::from("src/main.rs"),
            status: GitFileStatus::Modified,
            staged: false,
        };
        assert_eq!(entry.status.icon(), "M");
        assert_eq!(entry.status.label(), "Modified");
    }

    #[test]
    fn status_icon_mapping() {
        assert_eq!(GitFileStatus::Added.icon(), "A");
        assert_eq!(GitFileStatus::Deleted.icon(), "D");
        assert_eq!(GitFileStatus::Untracked.icon(), "?");
    }

    #[test]
    fn staged_counts() {
        let state = GitState {
            entries: vec![
                GitStatusEntry { path: PathBuf::from("a"), status: GitFileStatus::Modified, staged: true },
                GitStatusEntry { path: PathBuf::from("b"), status: GitFileStatus::Added, staged: true },
                GitStatusEntry { path: PathBuf::from("c"), status: GitFileStatus::Modified, staged: false },
            ],
            ..Default::default()
        };
        assert_eq!(state.staged_count(), 2);
        assert_eq!(state.unstaged_count(), 1);
    }
}
