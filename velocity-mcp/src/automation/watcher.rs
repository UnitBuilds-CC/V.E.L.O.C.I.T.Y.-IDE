use crate::compiler::parser_loader::DynamicParser;
use crate::ipc::telemetry_share::{TelemetryClient, TelemetryRequest};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime};

pub fn spawn_ast_watcher(workspace_root: PathBuf, shmem_path: PathBuf) {
    thread::spawn(move || {
        let mut client = match TelemetryClient::open(&shmem_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!(
                    "[watcher] Failed to open Shared Memory telemetry client: {}",
                    e
                );
                return;
            }
        };

        println!(
            "[watcher] AST Watcher active. Monitoring {}",
            workspace_root.display()
        );

        let mut file_timestamps = HashMap::new();
        let mut known_files = HashSet::new();
        let mut initial_files = Vec::new();
        scan_directory(
            &workspace_root,
            &mut file_timestamps,
            &mut known_files,
            &mut initial_files,
            true,
        );
        publish_ast_updates(&workspace_root, &mut client, initial_files);

        loop {
            let mut changed_files = Vec::new();
            let deleted_files = scan_directory(
                &workspace_root,
                &mut file_timestamps,
                &mut known_files,
                &mut changed_files,
                false,
            );
            publish_ast_deletes(&workspace_root, &mut client, deleted_files);
            publish_ast_updates(&workspace_root, &mut client, changed_files);
            thread::sleep(Duration::from_millis(500));
        }
    });
}

fn publish_ast_deletes(
    workspace_root: &Path,
    client: &mut TelemetryClient,
    deleted_files: Vec<PathBuf>,
) {
    for file in deleted_files {
        let req = TelemetryRequest::AstDelete {
            file_path: relative_path_string(&file, workspace_root),
        };
        if let Err(e) = client.send(&req) {
            eprintln!("[watcher] Failed to stream AST delete telemetry: {}", e);
        }
    }
}

fn publish_ast_updates(
    workspace_root: &Path,
    client: &mut TelemetryClient,
    changed_files: Vec<PathBuf>,
) {
    for file in changed_files {
        match std::fs::read_to_string(&file) {
            Ok(content) => {
                let rel_path = relative_path_string(&file, workspace_root);
                println!("[watcher] Parsing AST changes in {}", rel_path);

                let triples = parse_file_ast(&file, &content, workspace_root);
                let req = TelemetryRequest::AstUpdate {
                    file_path: rel_path,
                    triples,
                };

                if let Err(e) = client.send(&req) {
                    eprintln!("[watcher] Failed to stream telemetry: {}", e);
                }
            }
            Err(err) => {
                eprintln!("[watcher] Failed to read {}: {}", file.display(), err);
            }
        }
    }
}

fn relative_path_string(path: &Path, workspace_root: &Path) -> String {
    pathdiff::diff_paths(path, workspace_root)
        .unwrap_or_else(|| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
}

fn scan_directory(
    dir: &Path,
    stamps: &mut HashMap<PathBuf, SystemTime>,
    known_files: &mut HashSet<PathBuf>,
    changed: &mut Vec<PathBuf>,
    initial_scan: bool,
) -> Vec<PathBuf> {
    let mut current_files = HashSet::new();
    scan_directory_inner(dir, stamps, changed, initial_scan, &mut current_files);

    let deleted_files = known_files
        .difference(&current_files)
        .cloned()
        .collect::<Vec<_>>();
    stamps.retain(|path, _| current_files.contains(path));
    known_files.clear();
    known_files.extend(current_files);
    deleted_files
}

fn scan_directory_inner(
    dir: &Path,
    stamps: &mut HashMap<PathBuf, SystemTime>,
    changed: &mut Vec<PathBuf>,
    initial_scan: bool,
    current_files: &mut HashSet<PathBuf>,
) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name != "bin"
                    && name != "obj"
                    && name != "target"
                    && name != ".git"
                    && name != ".velocity"
                {
                    scan_directory_inner(&path, stamps, changed, initial_scan, current_files);
                }
            } else if is_supported_source_file(&path) {
                current_files.insert(path.clone());
                if let Ok(meta) = entry.metadata() {
                    if let Ok(mtime) = meta.modified() {
                        match stamps.get(&path).copied() {
                            Some(old_mtime) if mtime > old_mtime => {
                                stamps.insert(path.clone(), mtime);
                                changed.push(path);
                            }
                            Some(_) => {}
                            None => {
                                stamps.insert(path.clone(), mtime);
                                if initial_scan {
                                    changed.push(path);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn is_supported_source_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("cs" | "rs" | "py" | "js")
    )
}

/// Dynamic Tree-sitter parser with regex fallback
fn parse_file_ast(file: &Path, content: &str, workspace_root: &Path) -> Vec<(u64, u16, u64)> {
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
    let parser_dir = workspace_root.join(".velocity").join("parsers");
    let file_identity = relative_path_string(file, workspace_root);
    let file_hash = hash_str(&file_identity);

    const DECLARES: u16 = 1;

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
                    return extract_triples_from_tree(&tree, content, file_hash);
                }
            }
        }
    }

    let mut triples = Vec::new();
    for line in content.lines() {
        let line = line.trim();
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
                        let node_hash = hash_str(name_cleaned);
                        triples.push((file_hash, DECLARES, node_hash));
                    }
                }
            }
        }
    }

    triples
}

fn extract_triples_from_tree(
    tree: &tree_sitter::Tree,
    content: &str,
    file_hash: u64,
) -> Vec<(u64, u16, u64)> {
    let mut triples = Vec::new();
    let mut cursor = tree.walk();
    let mut reached_root = false;

    const DECLARES: u16 = 1;

    while !reached_root {
        let node = cursor.node();
        let kind = node.kind();
        if kind == "method_declaration"
            || kind == "function_definition"
            || kind == "class_declaration"
        {
            if let Ok(text) = node.utf8_text(content.as_bytes()) {
                let name = text.split_whitespace().nth(1).unwrap_or(kind);
                let name_cleaned = name.split('(').next().unwrap_or(name);
                triples.push((file_hash, DECLARES, hash_str(name_cleaned)));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_file_ast_uses_workspace_relative_file_identity() {
        let temp = tempfile::tempdir().unwrap();
        let src_dir = temp.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        let file = src_dir.join("main.rs");
        let content = "fn launch() {}";

        let triples = parse_file_ast(&file, content, temp.path());
        assert_eq!(triples.len(), 1);
        assert_eq!(triples[0].0, hash_str("src/main.rs"));
    }

    #[test]
    fn initial_scan_publishes_existing_supported_files() {
        let temp = tempfile::tempdir().unwrap();
        let src_dir = temp.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        let file = src_dir.join("lib.rs");
        std::fs::write(&file, "fn existing() {}\n").unwrap();

        let mut stamps = HashMap::new();
        let mut known = HashSet::new();
        let mut changed = Vec::new();
        let deleted = scan_directory(temp.path(), &mut stamps, &mut known, &mut changed, true);

        assert!(deleted.is_empty());
        assert_eq!(changed, vec![file.clone()]);
        assert!(known.contains(&file));
    }

    #[test]
    fn scan_directory_prunes_deleted_files_from_watcher_state() {
        let temp = tempfile::tempdir().unwrap();
        let src_dir = temp.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        let file = src_dir.join("lib.rs");
        std::fs::write(&file, "fn existing() {}\n").unwrap();

        let mut stamps = HashMap::new();
        let mut known = HashSet::new();
        let mut changed = Vec::new();
        let deleted = scan_directory(temp.path(), &mut stamps, &mut known, &mut changed, true);
        assert!(deleted.is_empty());

        std::fs::remove_file(&file).unwrap();
        changed.clear();
        let deleted = scan_directory(temp.path(), &mut stamps, &mut known, &mut changed, false);

        assert!(changed.is_empty());
        assert_eq!(deleted, vec![file.clone()]);
        assert!(!known.contains(&file));
        assert!(!stamps.contains_key(&file));
    }
}
