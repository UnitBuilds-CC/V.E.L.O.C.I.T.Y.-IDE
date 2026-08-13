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

fn walk(root: &Path, dir: &Path, query: &str, max_results: usize, results: &mut Vec<SearchHit>) {
    if results.len() >= max_results {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
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
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
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

/// Outcome of a workspace-wide replace operation.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReplaceSummary {
    pub files_changed: usize,
    pub replacements: usize,
}

/// Replace every case-sensitive literal occurrence of `find` with `replace`
/// across the workspace, using the same file filtering as `project_search`.
/// Returns how many files changed and how many occurrences were replaced.
pub fn project_replace(root: &Path, find: &str, replace: &str) -> ReplaceSummary {
    let mut summary = ReplaceSummary::default();
    if find.is_empty() {
        return summary;
    }
    replace_walk(root, find, replace, &mut summary);
    summary
}

fn replace_walk(dir: &Path, find: &str, replace: &str, summary: &mut ReplaceSummary) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.')
            || name == "target"
            || name == "node_modules"
            || name == "__pycache__"
        {
            continue;
        }
        if path.is_dir() {
            replace_walk(&path, find, replace, summary);
        } else if path.is_file() {
            replace_file(&path, find, replace, summary);
        }
    }
}

fn replace_file(path: &Path, find: &str, replace: &str, summary: &mut ReplaceSummary) {
    const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
    let Ok(meta) = fs::metadata(path) else { return };
    if meta.len() > MAX_FILE_BYTES {
        return;
    }
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    let count = text.matches(find).count();
    if count == 0 {
        return;
    }
    let updated = text.replace(find, replace);
    if fs::write(path, updated).is_ok() {
        summary.files_changed += 1;
        summary.replacements += count;
    }
}

/// Collect every indexable file in the workspace as a relative path string,
/// for the quick-open switcher. Skips hidden dirs, build output and
/// dependencies, and caps the total so huge trees stay responsive.
pub fn list_workspace_files(root: &Path, max_results: usize) -> Vec<String> {
    let mut results = Vec::new();
    walk_files(root, root, max_results, &mut results);
    results.sort();
    results
}

fn walk_files(root: &Path, dir: &Path, max_results: usize, results: &mut Vec<String>) {
    if results.len() >= max_results {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if results.len() >= max_results {
            return;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.')
            || name == "target"
            || name == "node_modules"
            || name == "__pycache__"
        {
            continue;
        }
        if path.is_dir() {
            walk_files(root, &path, max_results, results);
        } else if path.is_file() {
            let rel = path.strip_prefix(root).unwrap_or(&path);
            results.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// A symbol indexed by the site map: a name plus the relative file that
/// defines it (predicate 1 = "file defines symbol").
#[derive(Clone, Debug)]
pub struct SymbolEntry {
    pub name: String,
    pub file: String,
}

/// Collect every symbol the site map knows about, paired with the relative
/// path of the file that defines it. Symbols whose resolved name looks like a
/// path are skipped.
pub fn collect_workspace_symbols(workspace_root: &Path) -> Vec<SymbolEntry> {
    let Ok(sm) = crate::automation::open_workspace_site_map(workspace_root) else {
        return Vec::new();
    };
    let mut out: Vec<SymbolEntry> = Vec::new();
    // predicate 1 = "file defines/contains symbol".
    for triple in sm.find_live_triples(None, Some(1), None) {
        let file = sm.resolve_string(triple.subject_hash).unwrap_or_default();
        let name = sm.resolve_string(triple.object_hash).unwrap_or_default();
        if file.is_empty() || name.is_empty() {
            continue;
        }
        if !file.contains('/') && !file.contains('\\') {
            continue;
        }
        if name.contains('/') || name.contains('\\') {
            continue;
        }
        out.push(SymbolEntry {
            name,
            file: file.replace('\\', "/"),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.file.cmp(&b.file)));
    out.dedup_by(|a, b| a.name == b.name && a.file == b.file);
    out
}

/// Best-effort 1-based line number where `name` is defined inside `content`.
/// Prefers a definition keyword (`fn`/`struct`/…) on the line, then falls back
/// to the first line mentioning the name.
pub fn find_definition_line(content: &str, name: &str) -> Option<usize> {
    if name.is_empty() {
        return None;
    }
    let keywords = [
        "fn ", "struct ", "enum ", "trait ", "impl ", "type ", "const ", "static ", "mod ",
        "class ", "def ",
    ];
    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        if keywords.iter().any(|k| trimmed.starts_with(k)) && trimmed.contains(name) {
            return Some(idx + 1);
        }
    }
    content
        .lines()
        .position(|line| line.contains(name))
        .map(|idx| idx + 1)
}

/// A top-level symbol extracted from a source file's text, for the Outline view.
#[derive(Clone, Debug)]
pub struct FileSymbol {
    pub name: String,
    /// 1-based line number.
    pub line: usize,
}

/// Extract top-level (column-0) definitions from source text for the Outline
/// view. Keyword-based and language-light: works for Rust, Python, JS/TS, etc.
pub fn extract_file_symbols(content: &str) -> Vec<FileSymbol> {
    const KEYWORDS: &[&str] = &[
        "fn ",
        "struct ",
        "enum ",
        "trait ",
        "impl ",
        "type ",
        "const ",
        "static ",
        "mod ",
        "class ",
        "def ",
        "interface ",
        "function ",
    ];
    let mut out = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        // Only top-level items: the definition keyword starts at column 0.
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        let trimmed = line.trim_start();
        for kw in KEYWORDS {
            if let Some(rest) = trimmed.strip_prefix(kw) {
                if let Some(name) = extract_ident(rest) {
                    out.push(FileSymbol {
                        name,
                        line: idx + 1,
                    });
                }
                break;
            }
        }
    }
    out
}

/// Read the leading identifier (plus generics like `Foo<T>`) from `s`.
fn extract_ident(s: &str) -> Option<String> {
    let s = s.trim_start();
    let mut name = String::new();
    for ch in s.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            name.push(ch);
        } else {
            break;
        }
    }
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

pub fn icon_for_path(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => "rs",
        Some("toml") => "cf",
        Some("md") => "md",
        Some("json") => "{}",
        Some("py") => "py",
        Some("js" | "ts") => "js",
        Some("html" | "css") => "<>",
        Some("cpp" | "c" | "h") => "c",
        _ => "f",
    }
}
