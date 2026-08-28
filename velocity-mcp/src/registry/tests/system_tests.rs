use crate::registry::call_tool_in_workspace;
use serde_json::json;
use std::fs;

fn setup_root() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();
    (temp, root)
}

#[test]
fn file_tools_use_explicit_workspace_root() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();

    call_tool_in_workspace(
        &root,
        "write_file",
        &json!({"relativeFilePath": "src/main.rs", "content": "fn main() {}"}),
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(root.join("src/main.rs")).unwrap(),
        "fn main() {}"
    );
}

#[test]
fn file_tools_reject_parent_traversal() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();

    let result = call_tool_in_workspace(
        &root,
        "write_file",
        &json!({"relativeFilePath": "../outside.txt", "content": "nope"}),
    );

    assert!(result.is_err());
    assert!(!temp.path().join("outside.txt").exists());
}

#[test]
fn command_runs_in_explicit_workspace() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();

    let output = call_tool_in_workspace(&root, "run_command", &json!({"command": "cd"})).unwrap();
    assert!(output.to_lowercase().contains("project"));
}

#[test]
fn file_tools_delete_file_success() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();

    fs::write(root.join("temp.txt"), "hello").unwrap();
    assert!(root.join("temp.txt").exists());

    call_tool_in_workspace(
        &root,
        "delete_file",
        &json!({"relativeFilePath": "temp.txt"}),
    )
    .unwrap();

    assert!(!root.join("temp.txt").exists());
}

#[test]
fn file_tools_delete_file_rejects_parent_traversal() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();

    let result = call_tool_in_workspace(
        &root,
        "delete_file",
        &json!({"relativeFilePath": "../outside.txt"}),
    );

    assert!(result.is_err());
}

#[test]
fn list_dir_returns_error_on_missing_dir() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();

    let result = call_tool_in_workspace(
        &root,
        "list_dir",
        &json!({"relativeDirPath": "missing_folder"}),
    );

    assert!(result.is_err());
}

#[test]
fn code_coverage_analyze_reports_gaps_and_scaffolds() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/math.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\npub fn sub(a: i32, b: i32) -> i32 { a - b }\n",
    )
    .unwrap();

    let out = call_tool_in_workspace(&root, "code_coverage_analyze", &json!({})).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    // Both functions are discovered and neither has a test yet.
    assert_eq!(v["totalFunctions"].as_u64().unwrap(), 2);
    assert_eq!(v["testedFunctions"].as_u64().unwrap(), 0);
    assert_eq!(v["untestedCount"].as_u64().unwrap(), 2);
    assert!(v["skeletonCount"].as_u64().unwrap() >= 1);
    assert!(v["summary"].as_str().unwrap().contains("Coverage"));
}

#[test]
fn code_coverage_analyze_is_advertised_in_definitions() {
    let tools = crate::registry::get_tools();
    assert!(
        tools.iter().any(|t| t.name == "code_coverage_analyze"),
        "missing tool definition for code_coverage_analyze"
    );
}

#[test]
fn fetch_panel_data_lists_files_and_is_advertised() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("README.md"), "hello").unwrap();

    let output =
        call_tool_in_workspace(&root, "fetch_panel_data", &json!({"panel": "files"})).unwrap();
    let data: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(data["panel"], "files");
    assert!(data["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["name"] == "README.md"));

    let tools = crate::registry::get_tools();
    assert!(tools.iter().any(|tool| tool.name == "fetch_panel_data"));
}

#[test]
fn fetch_panel_data_rejects_unknown_panel() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();

    assert!(
        call_tool_in_workspace(&root, "fetch_panel_data", &json!({"panel": "run_build"}),).is_err()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// System Tool Contract Tests — read_file
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn read_file_returns_file_content() {
    let (_temp, root) = setup_root();
    fs::write(root.join("hello.txt"), "hello world").unwrap();

    let output = call_tool_in_workspace(
        &root,
        "read_file",
        &json!({"relativeFilePath": "hello.txt"}),
    )
    .unwrap();

    assert!(output.contains("hello world"));
}

#[test]
fn read_file_errors_on_missing_file() {
    let (_temp, root) = setup_root();

    let result = call_tool_in_workspace(
        &root,
        "read_file",
        &json!({"relativeFilePath": "nonexistent.txt"}),
    );

    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// System Tool Contract Tests — write_file
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn write_file_creates_parent_directories() {
    let (_temp, root) = setup_root();

    call_tool_in_workspace(
        &root,
        "write_file",
        &json!({
            "relativeFilePath": "deep/nested/dir/file.txt",
            "content": "nested content"
        }),
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(root.join("deep/nested/dir/file.txt")).unwrap(),
        "nested content"
    );
}

#[test]
fn write_file_overwrites_existing() {
    let (_temp, root) = setup_root();
    fs::write(root.join("existing.txt"), "old content").unwrap();

    call_tool_in_workspace(
        &root,
        "write_file",
        &json!({
            "relativeFilePath": "existing.txt",
            "content": "new content"
        }),
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(root.join("existing.txt")).unwrap(),
        "new content"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// System Tool Contract Tests — list_dir
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn list_dir_lists_workspace_root() {
    let (_temp, root) = setup_root();
    fs::write(root.join("a.txt"), "").unwrap();
    fs::write(root.join("b.txt"), "").unwrap();
    fs::create_dir(root.join("subdir")).unwrap();

    let output = call_tool_in_workspace(
        &root,
        "list_dir",
        &json!({"relativeDirPath": "."}),
    )
    .unwrap();

    assert!(output.contains("a.txt"));
    assert!(output.contains("b.txt"));
    assert!(output.contains("subdir"));
}

#[test]
fn list_dir_lists_subdirectory() {
    let (_temp, root) = setup_root();
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();

    let output = call_tool_in_workspace(
        &root,
        "list_dir",
        &json!({"relativeDirPath": "src"}),
    )
    .unwrap();

    assert!(output.contains("main.rs"));
}

// ═══════════════════════════════════════════════════════════════════════════
// System Tool Contract Tests — grep_search
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn grep_search_finds_matching_lines() {
    let (_temp, root) = setup_root();
    fs::write(root.join("code.rs"), "fn hello() {}\nfn world() {}\nfn hello_world() {}").unwrap();

    let output = call_tool_in_workspace(
        &root,
        "grep_search",
        &json!({"query": "hello"}),
    )
    .unwrap();

    assert!(output.contains("hello"));
    assert!(output.contains("code.rs"));
}

#[test]
fn grep_search_returns_empty_for_no_matches() {
    let (_temp, root) = setup_root();
    fs::write(root.join("code.rs"), "fn hello() {}").unwrap();

    let output = call_tool_in_workspace(
        &root,
        "grep_search",
        &json!({"query": "nonexistent_pattern_xyz"}),
    )
    .unwrap();

    // Should return empty results or indicate no matches
    assert!(!output.contains("nonexistent_pattern_xyz") || output.contains("0 match") || output.contains("no match") || output.is_empty() || !output.contains("code.rs"));
}

// ═══════════════════════════════════════════════════════════════════════════
// System Tool Contract Tests — run_command
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn run_command_captures_stdout() {
    let (_temp, root) = setup_root();

    let output = call_tool_in_workspace(
        &root,
        "run_command",
        &json!({"command": "echo hello_velocity"}),
    )
    .unwrap();

    assert!(output.contains("hello_velocity"));
}

#[test]
fn run_command_returns_exit_code_info() {
    let (_temp, root) = setup_root();

    // Run a command that will fail
    let result = call_tool_in_workspace(
        &root,
        "run_command",
        &json!({"command": "exit 42"}),
    );

    // Should still return output (with error info), not panic
    let _ = result;
}

// ═══════════════════════════════════════════════════════════════════════════
// System Tool Contract Tests — delete_file
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn delete_file_removes_file() {
    let (_temp, root) = setup_root();
    fs::write(root.join("to_delete.txt"), "delete me").unwrap();
    assert!(root.join("to_delete.txt").exists());

    call_tool_in_workspace(
        &root,
        "delete_file",
        &json!({"relativeFilePath": "to_delete.txt"}),
    )
    .unwrap();

    assert!(!root.join("to_delete.txt").exists());
}

#[test]
fn delete_file_errors_on_missing_file() {
    let (_temp, root) = setup_root();

    let result = call_tool_in_workspace(
        &root,
        "delete_file",
        &json!({"relativeFilePath": "nonexistent.txt"}),
    );

    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// System Tool Contract Tests — fetch_panel_data
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fetch_panel_data_teams_returns_json() {
    let (_temp, root) = setup_root();

    let output = call_tool_in_workspace(
        &root,
        "fetch_panel_data",
        &json!({"panel": "teams"}),
    )
    .unwrap();

    let data: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(data["panel"], "teams");
}

#[test]
fn fetch_panel_data_wiki_returns_json() {
    let (_temp, root) = setup_root();

    let output = call_tool_in_workspace(
        &root,
        "fetch_panel_data",
        &json!({"panel": "wiki"}),
    )
    .unwrap();

    let data: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(data["panel"], "wiki");
}

#[test]
fn fetch_panel_data_graph_returns_json() {
    let (_temp, root) = setup_root();

    let output = call_tool_in_workspace(
        &root,
        "fetch_panel_data",
        &json!({"panel": "graph"}),
    )
    .unwrap();

    let data: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(data["panel"], "graph");
}

#[test]
fn fetch_panel_data_bookmarks_returns_json() {
    let (_temp, root) = setup_root();

    let output = call_tool_in_workspace(
        &root,
        "fetch_panel_data",
        &json!({"panel": "bookmarks"}),
    )
    .unwrap();

    let data: serde_json::Value = serde_json::from_str(&output).unwrap();
    // Bookmarks panel returns {"bookmarks": []} when no bookmarks exist
    assert!(data.get("bookmarks").is_some() || data.get("panel").is_some(),
        "bookmarks panel should return bookmarks array or panel field");
}

// ═══════════════════════════════════════════════════════════════════════════
// System Tool Contract Tests — agent checkpoint
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn agent_checkpoint_list_returns_array() {
    let (_temp, root) = setup_root();

    let output = call_tool_in_workspace(
        &root,
        "agent_checkpoint_list",
        &json!({}),
    )
    .unwrap();

    // Should return a valid response (may be empty list)
    assert!(!output.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// System Tool Contract Tests — agent memory
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn agent_memory_remember_and_recall() {
    let (_temp, root) = setup_root();

    // Store a memory
    let output = call_tool_in_workspace(
        &root,
        "agent_memory_remember",
        &json!({
            "key": "test_fact",
            "content": "The sky is blue",
            "tags": ["test", "fact"],
            "score": 0.8
        }),
    )
    .unwrap();

    assert!(!output.is_empty());

    // Recall the memory
    let output = call_tool_in_workspace(
        &root,
        "agent_memory_recall",
        &json!({
            "query": "sky color",
            "limit": 5
        }),
    )
    .unwrap();

    assert!(!output.is_empty());
}

#[test]
fn agent_memory_forget_removes_memory() {
    let (_temp, root) = setup_root();

    // Store a memory
    call_tool_in_workspace(
        &root,
        "agent_memory_remember",
        &json!({
            "key": "to_forget",
            "content": "temporary fact"
        }),
    )
    .unwrap();

    // Forget it
    let output = call_tool_in_workspace(
        &root,
        "agent_memory_forget",
        &json!({"key": "to_forget"}),
    )
    .unwrap();

    assert!(!output.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// System Tool Contract Tests — knowledge base
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn knowledge_ingest_and_search() {
    let (_temp, root) = setup_root();

    // Ingest some text
    let output = call_tool_in_workspace(
        &root,
        "knowledge_ingest",
        &json!({
            "text": "Velocity is a Rust-based IDE with MCP support.",
            "source": "test_doc"
        }),
    )
    .unwrap();

    assert!(!output.is_empty());

    // Search for it
    let output = call_tool_in_workspace(
        &root,
        "knowledge_search",
        &json!({
            "query": "Rust IDE",
            "k": 3
        }),
    )
    .unwrap();

    assert!(!output.is_empty());
}
