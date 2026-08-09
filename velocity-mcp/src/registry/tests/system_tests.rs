use crate::registry::call_tool_in_workspace;
use serde_json::json;
use std::fs;

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
