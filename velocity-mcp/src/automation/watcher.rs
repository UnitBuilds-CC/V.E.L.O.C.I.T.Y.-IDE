use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
use crate::compiler::parser_loader::DynamicParser;
use crate::ipc::telemetry_share::{TelemetryClient, TelemetryRequest};
use sha2::{Sha256, Digest};

pub fn spawn_ast_watcher(workspace_root: PathBuf, shmem_path: PathBuf) {
    thread::spawn(move || {
        let mut client = match TelemetryClient::open(&shmem_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[watcher] Failed to open Shared Memory telemetry client: {}", e);
                return;
            }
        };

        println!("[watcher] AST Watcher active. Monitoring {}", workspace_root.display());

        // Simple poll-based file watcher to avoid external dependency compile issues
        // in environments without native notify-compatible loops.
        let mut file_timestamps = std::collections::HashMap::new();

        loop {
            let mut changed_files = Vec::new();
            scan_directory(&workspace_root, &mut file_timestamps, &mut changed_files);

            for file in changed_files {
                if let Ok(content) = std::fs::read_to_string(&file) {
                    let rel_path = pathdiff::diff_paths(&file, &workspace_root)
                        .unwrap_or_else(|| file.clone())
                        .to_string_lossy()
                        .to_string();

                    println!("[watcher] Parsing AST changes in {}", rel_path);

                    let triples = parse_file_ast(&file, &content, &workspace_root);
                    let req = TelemetryRequest::AstUpdate {
                        file_path: rel_path,
                        triples,
                    };

                    if let Err(e) = client.send(&req) {
                        eprintln!("[watcher] Failed to stream telemetry: {}", e);
                    }
                }
            }

            thread::sleep(Duration::from_millis(500));
        }
    });
}

fn scan_directory(
    dir: &Path,
    stamps: &mut std::collections::HashMap<PathBuf, std::time::SystemTime>,
    changed: &mut Vec<PathBuf>,
) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Ignore build/git folders
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name != "bin" && name != "obj" && name != "target" && name != ".git" && name != ".velocity" {
                    scan_directory(&path, stamps, changed);
                }
            } else {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext == "cs" || ext == "rs" || ext == "py" || ext == "js" {
                    if let Ok(meta) = entry.metadata() {
                        if let Ok(mtime) = meta.modified() {
                            if let Some(&old_mtime) = stamps.get(&path) {
                                if mtime > old_mtime {
                                    stamps.insert(path.clone(), mtime);
                                    changed.push(path);
                                }
                            } else {
                                stamps.insert(path.clone(), mtime);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Dynamic Tree-sitter parser with regex fallback
fn parse_file_ast(file: &Path, content: &str, workspace_root: &Path) -> Vec<(u64, u16, u64)> {
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
    let parser_dir = workspace_root.join(".velocity").join("parsers");
    
    // Predicate IDs
    const DECLARES: u16 = 1;
    const CALLS: u16 = 2;

    // Try dynamic tree-sitter load first
    let (dll_name, symbol_name) = match ext {
        "cs" => ("tree-sitter-c-sharp.dll", "tree_sitter_c_sharp"),
        "rs" => ("tree-sitter-rust.dll", "tree_sitter_rust"),
        "py" => ("tree-sitter-python.dll", "tree_sitter_python"),
        _ => ("", ""),
    };

    let dll_path = parser_dir.join(dll_name);
    if !dll_name.is_empty() && dll_path.exists() {
        if let Ok(dp) = DynamicParser::load(&dll_path, symbol_name) {
            let mut parser = tree_sitter::Parser::new();
            if parser.set_language(dp.language()).is_ok() {
                if let Some(tree) = parser.parse(content, None) {
                    return extract_triples_from_tree(&tree, content);
                }
            }
        }
    }

    // Fallback: Regex matching for class and function declarations
    // (Subject_Hash, Predicate_Id, Object_Hash)
    let mut triples = Vec::new();
    let file_hash = hash_str(&file.to_string_lossy());

    for line in content.lines() {
        let line = line.trim();
        // C# / Rust / JS / Python function declaration heuristics
        if line.contains("fn ") || line.contains("void ") || line.contains("def ") || line.contains("class ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            for (i, &word) in parts.iter().enumerate() {
                if word == "fn" || word == "def" || word == "class" || word == "void" {
                    if let Some(&name) = parts.get(i + 1) {
                        let name_cleaned = name.split('(').next().unwrap_or(name);
                        let node_hash = hash_str(name_cleaned);
                        triples.push((file_hash, DECLARES, node_hash));
                    }
                }
            }
        }
    }

    triples
}

fn extract_triples_from_tree(tree: &tree_sitter::Tree, content: &str) -> Vec<(u64, u16, u64)> {
    let mut triples = Vec::new();
    let mut cursor = tree.walk();
    let mut reached_root = false;

    const DECLARES: u16 = 1;

    while !reached_root {
        let node = cursor.node();
        let kind = node.kind();
        if kind == "method_declaration" || kind == "function_definition" || kind == "class_declaration" {
            if let Ok(text) = node.utf8_text(content.as_bytes()) {
                let name = text.split_whitespace().nth(1).unwrap_or(kind);
                let name_cleaned = name.split('(').next().unwrap_or(name);
                triples.push((hash_str("file"), DECLARES, hash_str(name_cleaned)));
            }
        }

        if cursor.goto_first_child() {
            continue;
        }

        if cursor.goto_next_sibling() {
            continue;
        }

        loop {
            if !cursor.goto_parent() {
                reached_root = true;
                break;
            }
            if cursor.goto_next_sibling() {
                break;
            }
        }
    }

    triples
}

fn hash_str(s: &str) -> u64 {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    let d = h.finalize();
    u64::from_le_bytes(d[..8].try_into().unwrap())
}
