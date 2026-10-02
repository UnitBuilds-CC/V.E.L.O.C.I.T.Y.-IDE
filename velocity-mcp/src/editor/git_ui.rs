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

    /// Explorer decoration precedence: when a folder holds mixed statuses,
    /// the most urgent one colors the folder (lower number wins).
    pub fn priority(&self) -> u8 {
        match self {
            Self::Conflicted => 0,
            Self::Modified => 1,
            Self::Deleted => 2,
            Self::Renamed => 3,
            Self::Untracked => 4,
            Self::Added => 5,
        }
    }

    /// RGB for the explorer decoration color of this status (VS Code-like:
    /// greens for new things, amber for edits, red for removals/conflicts).
    pub fn decoration_rgb(&self) -> (u8, u8, u8) {
        match self {
            Self::Added | Self::Renamed | Self::Untracked => (129, 184, 90),
            Self::Modified => (222, 179, 92),
            Self::Deleted => (205, 92, 92),
            Self::Conflicted => (224, 108, 117),
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
        if let Some(output) = run_git(
            root,
            &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
        ) {
            let parts: Vec<&str> = output.split_whitespace().collect();
            self.ahead = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
            self.behind = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        }
    }

    fn refresh_status(&mut self, root: &Path) {
        self.entries.clear();
        if let Some(output) = run_git(root, &["status", "--porcelain=v1"]) {
            for line in output.lines() {
                if let Some(entry) = parse_porcelain_line(line) {
                    self.entries.push(entry);
                }
            }
        }
    }

    /// Build the explorer decoration map: absolute path → status, with file
    /// statuses bubbled up onto every ancestor directory (a folder takes its
    /// most urgent child status). Pure function of `entries` so the tree can
    /// color rows without any git calls.
    pub fn decorations(
        &self,
        workspace_root: &Path,
    ) -> std::collections::HashMap<PathBuf, GitFileStatus> {
        use std::collections::HashMap;
        let mut map: HashMap<PathBuf, GitFileStatus> = HashMap::new();
        for entry in &self.entries {
            let abs = workspace_root.join(&entry.path);
            let best = match map.get(&abs).copied() {
                Some(existing) => {
                    if entry.status.priority() < existing.priority() {
                        entry.status
                    } else {
                        existing
                    }
                }
                None => entry.status,
            };
            map.insert(abs.clone(), best);
            // Bubble onto ancestors up to (but not including) the workspace
            // root; the root header is the project name, not a plain row.
            let mut parent = abs.parent();
            while let Some(p) = parent {
                if p == workspace_root {
                    break;
                }
                let fold = match map.get(p).copied() {
                    Some(existing) => {
                        if best.priority() < existing.priority() {
                            best
                        } else {
                            existing
                        }
                    }
                    None => best,
                };
                map.insert(p.to_path_buf(), fold);
                parent = p.parent();
            }
        }
        map
    }

    /// Stage a file.
    pub fn stage_file(&mut self, workspace_root: &Path, path: &Path) {
        run_git(workspace_root, &["add", &path.display().to_string()]);
        self.refresh_status(workspace_root);
    }

    /// Unstage a file.
    pub fn unstage_file(&mut self, workspace_root: &Path, path: &Path) {
        run_git(
            workspace_root,
            &["restore", "--staged", &path.display().to_string()],
        );
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
        } else if let Some(output) = run_git(
            workspace_root,
            &["diff", "--cached", &path.display().to_string()],
        ) {
            self.diff_output = output;
        }
    }

    /// Get blame for a file.
    pub fn blame_file(workspace_root: &Path, path: &Path) -> Vec<BlameLine> {
        let mut results = Vec::new();
        if let Some(output) = run_git(
            workspace_root,
            &["blame", "--porcelain", &path.display().to_string()],
        ) {
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
        if let Some(output) = run_git(
            workspace_root,
            &["log", "--oneline", "-30", "--format=%H|%h|%an|%ar|%s"],
        ) {
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
        run_git(
            workspace_root,
            &["branch", "--list", "--format=%(refname:short)"],
        )
        .map(|output| output.lines().map(|l| l.trim().to_string()).collect())
        .unwrap_or_default()
    }

    /// Switch to a branch.
    pub fn checkout_branch(&mut self, workspace_root: &Path, branch: &str) -> Result<(), String> {
        run_git(workspace_root, &["checkout", branch])
            .map(|_| {
                self.refresh(workspace_root);
            })
            .ok_or_else(|| "Checkout failed".to_string())
    }

    /// Create and switch to a new branch.
    pub fn create_branch(&mut self, workspace_root: &Path, name: &str) -> Result<(), String> {
        run_git(workspace_root, &["checkout", "-b", name])
            .map(|_| {
                self.refresh(workspace_root);
            })
            .ok_or_else(|| "Branch creation failed".to_string())
    }

    pub fn staged_count(&self) -> usize {
        self.entries.iter().filter(|e| e.staged).count()
    }

    pub fn unstaged_count(&self) -> usize {
        self.entries.iter().filter(|e| !e.staged).count()
    }

    /// Why the Commit button should be disabled, or `None` when a commit can
    /// run. A commit needs both a non-empty message and at least one staged
    /// file; surfacing the reason (instead of silently no-oping the button)
    /// is what keeps the panel honest about what it will actually do.
    pub fn commit_blocker(&self) -> Option<&'static str> {
        if self.staged_count() == 0 {
            Some("Stage changes to commit")
        } else if self.commit_message.trim().is_empty() {
            Some("Enter a commit message")
        } else {
            None
        }
    }

    /// Status line for a stage / unstage toggle. Pure so the wording is
    /// testable without spawning git.
    pub fn stage_toggle_message(staged_now: bool) -> &'static str {
        if staged_now {
            "Staged 1 file"
        } else {
            "Unstaged 1 file"
        }
    }
}

/// Render a timeline view of recent commits and current changes.
/// This provides a visual history of what changed and when.
pub fn render_recent_changes_timeline(
    ui: &mut egui::Ui,
    state: &GitState,
    palette: crate::editor::theme::IdePalette,
) {
    use egui;

    ui.label(
        egui::RichText::new("\u{23f1} Recent Changes Timeline")
            .size(11.0)
            .strong()
            .color(palette.text),
    );
    ui.add_space(6.0);

    // Current uncommitted changes section
    if !state.entries.is_empty() {
        ui.label(
            egui::RichText::new("Uncommitted Changes")
                .size(10.0)
                .strong()
                .color(palette.warning),
        );
        ui.add_space(4.0);

        egui::ScrollArea::vertical()
            .max_height(120.0)
            .show(ui, |ui| {
                for entry in &state.entries {
                    let name = entry
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| entry.path.to_string_lossy().to_string());
                    let status_color = match entry.status {
                        GitFileStatus::Modified => palette.warning,
                        GitFileStatus::Added => palette.success,
                        GitFileStatus::Deleted => palette.error,
                        GitFileStatus::Untracked => palette.text_muted,
                        _ => palette.text_muted,
                    };
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(entry.status.icon())
                                .monospace()
                                .size(10.0)
                                .strong()
                                .color(status_color),
                        );
                        ui.label(egui::RichText::new(&name).size(9.0).color(palette.text));
                        if entry.staged {
                            ui.label(
                                egui::RichText::new("staged")
                                    .size(8.0)
                                    .color(palette.success.gamma_multiply(0.7)),
                            );
                        }
                    });
                }
            });
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);
    }

    // Commit history timeline
    if state.log.is_empty() {
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("No commit history")
                    .size(9.0)
                    .color(palette.text_muted),
            );
        });
    } else {
        ui.label(
            egui::RichText::new("Commit History")
                .size(10.0)
                .strong()
                .color(palette.accent),
        );
        ui.add_space(4.0);

        egui::ScrollArea::vertical()
            .max_height(200.0)
            .show(ui, |ui| {
                for (i, commit) in state.log.iter().enumerate() {
                    // Timeline dot and line
                    ui.horizontal(|ui| {
                        // Timeline indicator
                        let dot_color = if i == 0 {
                            palette.accent
                        } else {
                            palette.text_muted
                        };
                        ui.label(egui::RichText::new("\u{25cf}").size(10.0).color(dot_color));

                        // Commit info
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(&commit.short_hash)
                                        .monospace()
                                        .size(9.0)
                                        .strong()
                                        .color(palette.accent),
                                );
                                ui.label(
                                    egui::RichText::new(&commit.message)
                                        .size(9.0)
                                        .color(palette.text),
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(&commit.author)
                                        .size(8.0)
                                        .color(palette.text_muted),
                                );
                                ui.label(
                                    egui::RichText::new(&commit.date)
                                        .size(8.0)
                                        .color(palette.text_muted.gamma_multiply(0.7)),
                                );
                            });
                        });
                    });

                    // Connecting line (except for last item)
                    if i < state.log.len() - 1 {
                        ui.indent("timeline_line", |ui| {
                            ui.label(
                                egui::RichText::new("\u{2502}")
                                    .size(8.0)
                                    .color(palette.text_muted.gamma_multiply(0.4)),
                            );
                        });
                    }
                }
            });
    }
}

/// Parse one `git status --porcelain=v1` line into an entry. Rename/copy
/// lines carry both paths (`R  old -> new`); the destination is the one that
/// exists on disk, so that is what gets decorated. Branch header and other
/// non-file lines yield `None`.
fn parse_porcelain_line(line: &str) -> Option<GitStatusEntry> {
    if line.len() < 4 {
        return None;
    }
    let index_status = line.as_bytes()[0] as char;
    let worktree_status = line.as_bytes()[1] as char;
    let raw = line[3..].trim();
    let path_str = match raw.find(" -> ") {
        Some(i) => raw[i + 4..].trim(),
        None => raw,
    };
    // Git quotes paths containing unusual characters.
    let path_str = path_str.strip_prefix('"').unwrap_or(path_str);
    let path_str = path_str.strip_suffix('"').unwrap_or(path_str);
    if path_str.is_empty() {
        return None;
    }
    let (status, staged) = match (index_status, worktree_status) {
        ('M', _) => (GitFileStatus::Modified, true),
        ('A', _) => (GitFileStatus::Added, true),
        ('D', _) => (GitFileStatus::Deleted, true),
        ('R', _) => (GitFileStatus::Renamed, true),
        (_, 'M') => (GitFileStatus::Modified, false),
        (_, 'D') => (GitFileStatus::Deleted, false),
        ('?', '?') => (GitFileStatus::Untracked, false),
        ('U', _) | (_, 'U') => (GitFileStatus::Conflicted, false),
        _ => return None,
    };
    Some(GitStatusEntry {
        path: PathBuf::from(path_str),
        status,
        staged,
    })
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

/// Case-insensitive substring match for branch filtering.
pub fn branch_matches_filter(branch: &str, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    branch.to_lowercase().contains(&query.to_lowercase())
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
                GitStatusEntry {
                    path: PathBuf::from("a"),
                    status: GitFileStatus::Modified,
                    staged: true,
                },
                GitStatusEntry {
                    path: PathBuf::from("b"),
                    status: GitFileStatus::Added,
                    staged: true,
                },
                GitStatusEntry {
                    path: PathBuf::from("c"),
                    status: GitFileStatus::Modified,
                    staged: false,
                },
            ],
            ..Default::default()
        };
        assert_eq!(state.staged_count(), 2);
        assert_eq!(state.unstaged_count(), 1);
    }

    #[test]
    fn porcelain_lines_parse_including_renames_and_noise() {
        let e = parse_porcelain_line(" M src/main.rs").unwrap();
        assert_eq!(
            (e.path.to_str().unwrap(), e.status, e.staged),
            ("src/main.rs", GitFileStatus::Modified, false)
        );
        let e = parse_porcelain_line("M  staged.txt").unwrap();
        assert!(e.staged);
        // Rename lines decorate the destination path, not the source.
        let e = parse_porcelain_line("R  old/name.rs -> new/name.rs").unwrap();
        assert_eq!(e.path, PathBuf::from("new/name.rs"));
        assert_eq!(e.status, GitFileStatus::Renamed);
        // Quoted paths lose their quotes; branch headers are ignored.
        let e = parse_porcelain_line("?? \"weird dir/f.txt\"").unwrap();
        assert_eq!(e.path, PathBuf::from("weird dir/f.txt"));
        assert!(parse_porcelain_line("## main...origin/main").is_none());
        assert!(parse_porcelain_line("").is_none());
    }

    #[test]
    fn decorations_absolute_paths_bubble_to_parent_folders() {
        let root = Path::new("/work/proj");
        let state = GitState {
            entries: vec![
                GitStatusEntry {
                    path: PathBuf::from("src/a.rs"),
                    status: GitFileStatus::Modified,
                    staged: false,
                },
                GitStatusEntry {
                    path: PathBuf::from("src/b/c.rs"),
                    status: GitFileStatus::Untracked,
                    staged: false,
                },
            ],
            ..Default::default()
        };
        let decos = state.decorations(root);
        assert_eq!(
            decos.get(&root.join("src/a.rs")),
            Some(&GitFileStatus::Modified)
        );
        assert_eq!(
            decos.get(&root.join("src/b/c.rs")),
            Some(&GitFileStatus::Untracked)
        );
        // Folders inherit; the deeper folder keeps only its own child's state.
        assert_eq!(
            decos.get(&root.join("src/b")),
            Some(&GitFileStatus::Untracked)
        );
        assert_eq!(decos.get(&root.join("src")), Some(&GitFileStatus::Modified));
        // The workspace root itself is never decorated.
        assert!(!decos.contains_key(root));
    }

    #[test]
    fn folder_takes_most_urgent_child_status() {
        let root = Path::new("/work");
        let state = GitState {
            entries: vec![
                GitStatusEntry {
                    path: PathBuf::from("docs/a.md"),
                    status: GitFileStatus::Untracked,
                    staged: false,
                },
                GitStatusEntry {
                    path: PathBuf::from("docs/b.md"),
                    status: GitFileStatus::Conflicted,
                    staged: false,
                },
            ],
            ..Default::default()
        };
        let decos = state.decorations(root);
        assert_eq!(
            decos.get(&root.join("docs")),
            Some(&GitFileStatus::Conflicted)
        );
    }

    #[test]
    fn commit_blocker_requires_staged_and_message() {
        // Nothing staged → blocked on staging, regardless of message.
        let s = GitState {
            entries: vec![GitStatusEntry {
                path: PathBuf::from("a"),
                status: GitFileStatus::Modified,
                staged: false,
            }],
            commit_message: "work".into(),
            ..Default::default()
        };
        assert_eq!(s.commit_blocker(), Some("Stage changes to commit"));
        // Staged but blank message → blocked on the message.
        let s = GitState {
            entries: vec![GitStatusEntry {
                path: PathBuf::from("a"),
                status: GitFileStatus::Modified,
                staged: true,
            }],
            commit_message: "   ".into(),
            ..Default::default()
        };
        assert_eq!(s.commit_blocker(), Some("Enter a commit message"));
        // Staged + message → ready.
        let s = GitState {
            entries: vec![GitStatusEntry {
                path: PathBuf::from("a"),
                status: GitFileStatus::Modified,
                staged: true,
            }],
            commit_message: "fix: thing".into(),
            ..Default::default()
        };
        assert_eq!(s.commit_blocker(), None);
    }

    #[test]
    fn stage_toggle_message_reflects_new_state() {
        assert_eq!(GitState::stage_toggle_message(true), "Staged 1 file");
        assert_eq!(GitState::stage_toggle_message(false), "Unstaged 1 file");
    }

    #[test]
    fn branch_matches_filter_case_insensitive_substring() {
        assert!(branch_matches_filter("feature/Login", "login"));
        assert!(branch_matches_filter("main", ""));
        assert!(!branch_matches_filter("develop", "prod"));
        assert!(branch_matches_filter("release/v2", "V2"));
    }
}
