use std::path::{Path, PathBuf};
use crate::automation::AgentTaskKind;
use super::types::*;

pub fn hash_str(s: &str) -> u64 {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    let d = h.finalize();
    u64::from_le_bytes(d[..8].try_into().unwrap())
}

pub fn get_cursor_pos(text: &str, char_idx: usize) -> (usize, usize) {
    let mut line = 0;
    let mut col = 0;
    for (i, c) in text.chars().enumerate() {
        if i == char_idx {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

pub fn get_git_branch(workspace_root: &Path) -> Option<String> {
    let head_path = workspace_root.join(".git/HEAD");
    if let Ok(head_content) = std::fs::read_to_string(head_path) {
        let trimmed = head_content.trim();
        if trimmed.starts_with("ref: refs/heads/") {
            return Some(trimmed["ref: refs/heads/".len()..].to_string());
        } else if !trimmed.is_empty() {
            return Some(trimmed.chars().take(7).collect());
        }
    }
    None
}

pub fn build_file_tree(dir: &Path) -> FileNode {
    let mut children = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
        entries.sort_by_key(|e| {
            (
                e.file_type().map(|t| !t.is_dir()).unwrap_or(true),
                e.file_name(),
            )
        });
        for entry in entries {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            if path.is_dir() {
                children.push(build_file_tree(&path));
            } else {
                children.push(FileNode {
                    name,
                    path,
                    is_dir: false,
                    children: None,
                });
            }
        }
    }
    FileNode {
        name: dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        path: dir.to_path_buf(),
        is_dir: true,
        children: Some(children),
    }
}

pub fn get_active_symbol(content: &str, cursor_line: usize) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    if cursor_line >= lines.len() {
        return None;
    }
    for idx in (0..=cursor_line).rev() {
        let line = lines[idx].trim();
        if line.contains("fn ")
            || line.contains("void ")
            || line.contains("def ")
            || line.contains("class ")
        {
            let parts: Vec<&str> = line.split_whitespace().collect();
            for (i, &word) in parts.iter().enumerate() {
                if word == "fn" || word == "def" || word == "class" || word == "void" {
                    if let Some(&name) = parts.get(i + 1) {
                        let name_cleaned = name.split('(').next().unwrap_or(name);
                        return Some(name_cleaned.to_string());
                    }
                }
            }
        }
    }
    None
}

pub fn diff_preview(old: &str, new: &str, max_lines: usize) -> (usize, usize, String) {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let mut out = String::new();
    let mut added = 0usize;
    let mut removed = 0usize;
    let mut shown = 0usize;
    let mut o = 0usize;
    let mut n = 0usize;

    while (o < old_lines.len() || n < new_lines.len()) && shown < max_lines {
        if o < old_lines.len() && n < new_lines.len() && old_lines[o] == new_lines[n] {
            o += 1;
            n += 1;
        } else if n < new_lines.len()
            && (o >= old_lines.len() || !old_lines[o..].contains(&new_lines[n]))
        {
            added += 1;
            out.push_str("+ ");
            out.push_str(new_lines[n]);
            out.push('\n');
            n += 1;
            shown += 1;
        } else if o < old_lines.len() {
            removed += 1;
            out.push_str("- ");
            out.push_str(old_lines[o]);
            out.push('\n');
            o += 1;
            shown += 1;
        } else {
            break;
        }
    }

    if out.is_empty() {
        out.push_str("(no line-level changes)");
    }

    (added, removed, out)
}

pub fn wants_workspace_scope(goal: &str) -> bool {
    let lower = goal.to_lowercase();
    lower.contains("codebase")
        || lower.contains("workspace")
        || lower.contains("project")
        || lower.contains("repository")
        || lower.contains("repo")
}

pub fn collect_workspace_routing_files(root: &Path, limit: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_workspace_routing_files_recursive(root, &mut files, limit);
    files
}

fn collect_workspace_routing_files_recursive(root: &Path, files: &mut Vec<PathBuf>, limit: usize) {
    if files.len() >= limit {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if files.len() >= limit {
            break;
        }
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if matches!(
                name.as_ref(),
                ".git" | ".velocity" | "target" | "archive" | "node_modules"
            ) {
                continue;
            }
            collect_workspace_routing_files_recursive(&path, files, limit);
            continue;
        }
        let include = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| matches!(ext, "rs" | "go" | "toml" | "md" | "json" | "yml" | "yaml"))
            .unwrap_or(false)
            || matches!(
                name.as_ref(),
                "Cargo.lock" | "Cargo.toml" | "go.mod" | "go.sum"
            );
        if include {
            files.push(path);
        }
    }
}

pub fn infer_task_kind_from_goal(goal: &str) -> AgentTaskKind {
    let lower = goal.to_lowercase();
    if lower.contains("windows automation")
        || lower.contains("desktop automation")
        || lower.contains("desktop test")
        || lower.contains("uia")
        || lower.contains("ui automation")
        || lower.contains("wa ")
        || lower.starts_with("wa")
    {
        AgentTaskKind::DesktopAutomation
    } else if lower.contains("refactor") {
        AgentTaskKind::Refactor
    } else if lower.contains("fix") || lower.contains("bug") || lower.contains("error") {
        AgentTaskKind::BugFix
    } else if lower.contains("test") || lower.contains("validate") {
        AgentTaskKind::Test
    } else if lower.contains("doc") || lower.contains("readme") {
        AgentTaskKind::Documentation
    } else if lower.contains("merge") || lower.contains("reconcile") {
        AgentTaskKind::Merge
    } else if lower.contains("analy") || lower.contains("investig") {
        AgentTaskKind::Analysis
    } else {
        AgentTaskKind::Planning
    }
}
