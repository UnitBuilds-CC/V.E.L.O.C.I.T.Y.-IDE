//! Tool parity tests: verify every advertised tool in `get_tools()` can be
//! dispatched without returning "Unknown tool". This is the minimum bar for
//! a tool being "wired" — definition + dispatch parity.
//!
//! Per the testing strategy: "A tool is not considered wired until its definition,
//! dispatch, permission behavior, valid call, and invalid call are covered."

use crate::errors::ToolError;
use crate::registry::{call_tool_in_workspace, get_tools};
use serde_json::json;
use std::fs;

fn setup_root() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();
    (temp, root)
}

/// Every tool returned by `get_tools()` must be dispatchable — i.e. calling it
/// should NOT return an "Unknown tool" error. It may return a different error
/// (e.g. missing argument, network error), but the dispatcher must recognize it.
///
/// NOTE: Some tools are skipped because they block (e.g., execute_nda compiles code).
#[test]
fn all_advertised_tools_are_dispatchable() {
    let (_temp, root) = setup_root();
    let tools = get_tools();

    assert!(
        !tools.is_empty(),
        "get_tools() should return at least one tool"
    );

    // Tools that are known to block or require external resources.
    // These tools are wired but can't be tested with empty args in a unit test.
    let skip_tools = [
        "execute_nda",         // Compiles and runs NDA code — blocks
        "wa_create_session",   // May block on Windows automation init
        "wa_wait_for_process", // Blocks waiting for process
        "wa_wait_for_window",  // Blocks waiting for window
        "wa_record_session",   // Blocks recording input
        "wa_replay_script",    // Blocks replaying script
        "wa_idle_wait",        // Intentionally blocks
    ];

    let mut unknown_tools = Vec::new();
    let mut tested_count = 0;

    for tool in &tools {
        if skip_tools.contains(&tool.name.as_str()) {
            continue;
        }
        tested_count += 1;

        // Call with empty arguments — the tool may fail (missing required args),
        // but it should NOT return "Unknown tool".
        //
        // The two capture tools default `outputPath` to a *relative* file name, so
        // an empty-arg call grabs the real screen and drops a ~7 MB bitmap into the
        // crate directory on every `cargo test`. Aim them at the scratch root
        // instead — dispatch is still exercised, without leaking artifacts.
        let args = match tool.name.as_str() {
            "wa_screenshot" => json!({
                "outputPath": root.join("screenshot.bmp").to_string_lossy()
            }),
            "wa_browser_screenshot" => json!({
                "outputPath": root.join("browser_screenshot.png").to_string_lossy()
            }),
            _ => json!({}),
        };
        let result = call_tool_in_workspace(&root, &tool.name, &args);
        match &result {
            Ok(_) => {} // Tool accepted empty args (rare but valid)
            Err(e) => {
                let err_msg = e.to_string().to_lowercase();
                if err_msg.contains("unknown tool") {
                    unknown_tools.push(tool.name.clone());
                }
                // Any other error is fine — the tool is wired but rejected empty args
            }
        }
    }

    assert!(
        tested_count >= 70,
        "Expected to test at least 70 tools, only tested {}. Did skip list grow too large?",
        tested_count
    );
    assert!(
        unknown_tools.is_empty(),
        "The following tools are advertised in get_tools() but return 'Unknown tool' from dispatch: {:?}. \
         Each tool must be wired in the dispatch chain (system/browser/team/WA).",
        unknown_tools
    );
}

/// Verify the total tool count matches expectations. This catches accidental
/// tool removal or duplicate registration.
#[test]
fn tool_count_matches_expectations() {
    let tools = get_tools();
    // We expect at least 80+ tools across all categories
    assert!(
        tools.len() >= 80,
        "Expected at least 80 tools, got {}. Did a tool category get lost?",
        tools.len()
    );
}

/// Every tool must have a non-empty name and description.
#[test]
fn all_tools_have_name_and_description() {
    let tools = get_tools();
    for tool in &tools {
        assert!(!tool.name.is_empty(), "Tool has empty name");
        assert!(
            !tool.description.is_empty(),
            "Tool '{}' has empty description",
            tool.name
        );
    }
}

/// Every tool must have a valid JSON Schema input_schema with at least "type": "object".
#[test]
fn all_tools_have_valid_input_schema() {
    let tools = get_tools();
    for tool in &tools {
        assert!(
            tool.input_schema.is_object(),
            "Tool '{}' input_schema is not a JSON object",
            tool.name
        );
        // Schema should have a "type" field
        assert!(
            tool.input_schema.get("type").is_some(),
            "Tool '{}' input_schema missing 'type' field",
            tool.name
        );
    }
}

/// No duplicate tool names should exist.
#[test]
fn no_duplicate_tool_names() {
    let tools = get_tools();
    let mut seen = std::collections::HashSet::new();
    let mut duplicates = Vec::new();

    for tool in &tools {
        if !seen.insert(&tool.name) {
            duplicates.push(tool.name.clone());
        }
    }

    assert!(
        duplicates.is_empty(),
        "Duplicate tool names found: {:?}",
        duplicates
    );
}

/// Schema snapshot for the disk-hygiene pair: the safety story depends on
/// `clean_disk_artifacts` defaulting to a dry run and exposing the `paths`
/// subset selector, and on `scan_disk_artifacts` taking no arguments. If
/// either shape changes, this test must change deliberately.
#[test]
fn disk_hygiene_tool_schemas() {
    let tools = get_tools();
    let scan = tools
        .iter()
        .find(|t| t.name == "scan_disk_artifacts")
        .expect("scan_disk_artifacts must be advertised");
    assert_eq!(scan.input_schema["type"], "object");
    assert!(
        scan.input_schema["properties"]
            .as_object()
            .is_none_or(|p| p.is_empty()),
        "scan_disk_artifacts should take no arguments"
    );

    let clean = tools
        .iter()
        .find(|t| t.name == "clean_disk_artifacts")
        .expect("clean_disk_artifacts must be advertised");
    assert_eq!(clean.input_schema["properties"]["dryRun"]["type"], "boolean");
    assert_eq!(clean.input_schema["properties"]["paths"]["type"], "array");
    assert_eq!(clean.input_schema["properties"]["paths"]["items"]["type"], "string");
    // Nothing is required: with no arguments the tool must resolve to the
    // dry-run-everything behavior, never to an implicit destructive call.
    assert!(
        clean.input_schema["required"]
            .as_array()
            .is_none_or(|r| r.is_empty()),
        "clean_disk_artifacts must not require arguments"
    );
    // The handler itself must agree with the schema promise: default dry_run.
    assert!(
        clean.description.contains("dry run"),
        "clean_disk_artifacts description must document the dry-run default"
    );
}

/// Tool categories should be represented (system, browser, team, WA).
#[test]
fn tool_categories_are_represented() {
    let tools = get_tools();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();

    // System tools
    assert!(
        names.contains(&"read_file"),
        "missing system tool: read_file"
    );
    assert!(
        names.contains(&"write_file"),
        "missing system tool: write_file"
    );
    assert!(
        names.contains(&"run_command"),
        "missing system tool: run_command"
    );

    // Browser tools
    assert!(
        names.contains(&"web_navigate"),
        "missing browser tool: web_navigate"
    );
    assert!(
        names.contains(&"browser_create_session"),
        "missing browser tool: browser_create_session"
    );

    // Team tools
    assert!(
        names.contains(&"create_expert_team"),
        "missing team tool: create_expert_team"
    );
    assert!(
        names.contains(&"list_expert_teams"),
        "missing team tool: list_expert_teams"
    );

    // WA tools
    assert!(
        names.contains(&"wa_create_session"),
        "missing WA tool: wa_create_session"
    );
    assert!(
        names.contains(&"wa_save_snapshot"),
        "missing WA tool: wa_save_snapshot"
    );
}

/// Unknown tools should return a structured `ToolError::ToolNotFound`.
#[test]
fn unknown_tool_returns_error() {
    let (_temp, root) = setup_root();
    let result = call_tool_in_workspace(&root, "totally_fake_tool_xyz", &json!({}));
    let err = result.unwrap_err();

    // Match the variant, not the message text: dispatch must report an
    // unrecognised tool in a machine-readable form.
    match err.downcast_ref::<ToolError>() {
        Some(ToolError::ToolNotFound(name)) => assert_eq!(name, "totally_fake_tool_xyz"),
        Some(other) => panic!("expected ToolNotFound, got {other:?}"),
        None => panic!("dispatch error should downcast to ToolError, got: {err}"),
    }

    // The rendered message still has to name the offending tool.
    assert!(err.to_string().contains("totally_fake_tool_xyz"));
}

/// Tool names should follow naming conventions (snake_case, category prefix).
#[test]
fn tool_names_follow_conventions() {
    let tools = get_tools();
    for tool in &tools {
        // Tool names should be snake_case (no spaces, no camelCase)
        assert!(
            !tool.name.contains(' '),
            "Tool name '{}' contains spaces — use snake_case",
            tool.name
        );
        // Tool names should be lowercase
        assert_eq!(
            tool.name,
            tool.name.to_lowercase(),
            "Tool name '{}' should be lowercase",
            tool.name
        );
    }
}
