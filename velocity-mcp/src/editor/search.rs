use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub path: PathBuf,
    pub line: usize,
    pub text: String,
}

pub fn project_search(root: &Path, query: &str, max_results: usize) -> Vec<SearchHit> {
    let mut results = Vec::new();
    if query.is_empty() {
        return results;
    }
    let lower = query.to_lowercase();
    walk(root, root, &lower, max_results, &mut results);
    results
}

fn walk(
    root: &Path,
    dir: &Path,
    query: &str,
    max_results: usize,
    results: &mut Vec<SearchHit>,
) {
    if results.len() >= max_results {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if results.len() >= max_results {
            return;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "target" || name == "node_modules" {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, query, max_results, results);
        } else if path.is_file() {
            search_file(root, &path, query, results);
        }
    }
}

fn search_file(root: &Path, path: &Path, query: &str, results: &mut Vec<SearchHit>) {
    const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
    let Ok(meta) = fs::metadata(path) else { return };
    if meta.len() > MAX_FILE_BYTES {
        return;
    }
    let Ok(text) = fs::read_to_string(path) else { return };
    for (idx, line) in text.lines().enumerate() {
        if line.to_lowercase().contains(query) {
            results.push(SearchHit {
                path: path.strip_prefix(root).unwrap_or(path).to_path_buf(),
                line: idx + 1,
                text: line.trim().to_string(),
            });
        }
    }
}

pub fn icon_for_path(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => "🦀",
        Some("toml") => "⚙️",
        Some("md") => "📝",
        Some("json") => "📋",
        Some("py") => "🐍",
        Some("js" | "ts") => "📜",
        Some("html" | "css") => "🌐",
        Some("cpp" | "c" | "h") => "⚙️",
        _ => "📄",
    }
}
