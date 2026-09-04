//! Speculative pre-computation: pre-indexes scoped files before agent workers
//! spawn, providing warm context caches that accelerate agent execution.

use crate::safety::SafeMutex;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

/// Pre-computed context for a single file: symbol outline + key content summary.
#[derive(Debug, Clone)]
pub struct FilePrecomputation {
    pub path: PathBuf,
    pub symbols: Vec<String>,
    pub line_count: usize,
    pub byte_size: u64,
    pub imports: Vec<String>,
    pub top_level_summary: String,
}

/// Result of a speculative pre-computation pass over a set of files.
#[derive(Debug, Clone, Default)]
pub struct PrecomputationResult {
    pub files: HashMap<PathBuf, FilePrecomputation>,
    pub total_symbols: usize,
    pub total_lines: usize,
}

impl PrecomputationResult {
    /// Get a compact context summary suitable for agent system prompts.
    pub fn context_summary(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "Pre-indexed {} files ({} symbols, {} lines total)",
            self.files.len(),
            self.total_symbols,
            self.total_lines
        ));
        for (path, precomp) in &self.files {
            let rel = path.display();
            let symbols_preview: Vec<_> = precomp.symbols.iter().take(5).cloned().collect();
            lines.push(format!(
                "  {} ({} lines, {} symbols): {}",
                rel,
                precomp.line_count,
                precomp.symbols.len(),
                symbols_preview.join(", ")
            ));
        }
        lines.join("\n")
    }
}

/// Shared pre-computation cache accessed by workers.
#[derive(Debug, Clone, Default)]
pub struct PrecomputationCache {
    inner: Arc<Mutex<HashMap<u64, PrecomputationResult>>>,
}

impl PrecomputationCache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Store a pre-computation result keyed by task ID.
    pub fn store(&self, task_id: u64, result: PrecomputationResult) {
        self.inner.lock_safe().insert(task_id, result);
    }

    /// Retrieve pre-computation for a task (consuming it).
    pub fn take(&self, task_id: u64) -> Option<PrecomputationResult> {
        self.inner.lock_safe().remove(&task_id)
    }

    /// Peek without consuming.
    pub fn peek(&self, task_id: u64) -> Option<PrecomputationResult> {
        self.inner.lock_safe().get(&task_id).cloned()
    }
}

/// Pre-compute file context for a set of scoped paths.
/// This is designed to run on a background thread before the worker spawns.
pub fn precompute_files(workspace_root: &Path, files: &[PathBuf]) -> PrecomputationResult {
    let mut result = PrecomputationResult::default();

    for file_path in files {
        let full_path = if file_path.is_absolute() {
            file_path.clone()
        } else {
            workspace_root.join(file_path)
        };

        if !full_path.is_file() {
            continue;
        }

        let Ok(meta) = fs::metadata(&full_path) else {
            continue;
        };

        // Skip very large files (>2MB)
        if meta.len() > 2 * 1024 * 1024 {
            continue;
        }

        let Ok(content) = fs::read_to_string(&full_path) else {
            continue;
        };

        let line_count = content.lines().count();
        let symbols = extract_symbols(&content);
        let imports = extract_imports(&content);
        let top_level_summary = build_summary(&content, &symbols);

        result.total_symbols += symbols.len();
        result.total_lines += line_count;
        result.files.insert(
            file_path.clone(),
            FilePrecomputation {
                path: file_path.clone(),
                symbols,
                line_count,
                byte_size: meta.len(),
                imports,
                top_level_summary,
            },
        );
    }

    result
}

/// Spawn pre-computation on a background thread, returning a handle.
pub fn spawn_precompute(
    workspace_root: PathBuf,
    task_id: u64,
    files: Vec<PathBuf>,
    cache: PrecomputationCache,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let result = precompute_files(&workspace_root, &files);
        cache.store(task_id, result);
    })
}

/// Extract top-level symbol names from source code.
fn extract_symbols(content: &str) -> Vec<String> {
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
        "export ",
    ];
    let mut symbols = Vec::new();
    for line in content.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        let trimmed = line.trim_start();
        for kw in KEYWORDS {
            if let Some(rest) = trimmed.strip_prefix(kw) {
                if let Some(name) = extract_ident(rest) {
                    symbols.push(name);
                }
                break;
            }
        }
    }
    symbols
}

/// Extract import/use statements.
fn extract_imports(content: &str) -> Vec<String> {
    let mut imports = Vec::new();
    for line in content.lines().take(50) {
        let trimmed = line.trim();
        if trimmed.starts_with("use ")
            || trimmed.starts_with("import ")
            || trimmed.starts_with("from ")
            || trimmed.starts_with("#include")
            || trimmed.starts_with("require")
        {
            imports.push(trimmed.to_string());
        }
    }
    imports
}

/// Build a compact summary from symbols and structure.
fn build_summary(content: &str, symbols: &[String]) -> String {
    let line_count = content.lines().count();
    let symbol_preview: Vec<_> = symbols.iter().take(10).cloned().collect();
    format!(
        "{} lines, defines: {}",
        line_count,
        if symbol_preview.is_empty() {
            "(no top-level symbols)".to_string()
        } else {
            symbol_preview.join(", ")
        }
    )
}

/// Read the leading identifier from a string.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_symbols_from_rust_code() {
        let code = "fn main() {}\nstruct Foo {}\nenum Bar {}\n    fn nested() {}";
        let symbols = extract_symbols(code);
        assert_eq!(symbols, vec!["main", "Foo", "Bar"]);
    }

    #[test]
    fn extract_imports_from_rust() {
        let code = "use std::path::Path;\nuse std::fs;\n\nfn main() {}";
        let imports = extract_imports(code);
        assert_eq!(imports.len(), 2);
        assert!(imports[0].contains("std::path"));
    }

    #[test]
    fn precompute_result_summary() {
        let mut result = PrecomputationResult::default();
        result.total_symbols = 10;
        result.total_lines = 200;
        result.files.insert(
            PathBuf::from("src/main.rs"),
            FilePrecomputation {
                path: PathBuf::from("src/main.rs"),
                symbols: vec!["main".into(), "App".into()],
                line_count: 100,
                byte_size: 2048,
                imports: vec!["use std::fs;".into()],
                top_level_summary: "100 lines, defines: main, App".into(),
            },
        );
        let summary = result.context_summary();
        assert!(summary.contains("Pre-indexed 1 files"));
        assert!(summary.contains("main"));
    }

    #[test]
    fn cache_store_and_take() {
        let cache = PrecomputationCache::new();
        let result = PrecomputationResult {
            total_symbols: 5,
            total_lines: 50,
            ..Default::default()
        };
        cache.store(42, result);
        assert!(cache.peek(42).is_some());
        let taken = cache.take(42);
        assert!(taken.is_some());
        assert_eq!(taken.unwrap().total_symbols, 5);
        assert!(cache.take(42).is_none());
    }
}
