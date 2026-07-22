use crate::registry::call_tool_in_workspace;
use serde_json::json;
use std::fs;

#[test]
fn wa_session_and_snapshot_round_trip() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();

    let session = call_tool_in_workspace(
        &root,
        "wa_create_session",
        &json!({"sessionId": "wa-session"}),
    )
    .unwrap();
    assert!(session.contains("Created WA session 'wa-session'"));

    let snapshot = call_tool_in_workspace(
        &root,
        "wa_save_snapshot",
        &json!({
            "sessionId": "wa-session",
            "snapshotName": "main-view",
            "url": "app://main",
            "title": "Main Window",
            "focusNodeId": "btn-1",
            "nodes": [
                {
                    "id": "btn-1",
                    "role": "button",
                    "name": "Submit",
                    "value": "",
                    "actions": ["click"],
                    "visible": true,
                    "enabled": true,
                    "provenance": "ui_automation",
                    "confidence": 1.0
                }
            ]
        }),
    )
    .unwrap();
    assert!(snapshot.contains("Saved WA snapshot 'main-view'"));

    let read_snap = call_tool_in_workspace(
        &root,
        "wa_read_snapshot",
        &json!({"sessionId": "wa-session", "snapshotName": "main-view"}),
    )
    .unwrap();
    assert!(read_snap.contains("Submit"));

    let resolved = call_tool_in_workspace(
        &root,
        "wa_resolve_selector",
        &json!({"sessionId": "wa-session", "role": "button", "name": "Submit"}),
    )
    .unwrap();
    assert!(resolved.contains("Matched node: btn-1"));
}

#[test]
fn wa_script_save_read_list_round_trip() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();

    let saved = call_tool_in_workspace(
        &root,
        "wa_save_script",
        &json!({
            "name": "Test Script",
            "startUrl": "app://start",
            "steps": [
                {"action": "click", "role": "button", "name": "Start", "required": true}
            ]
        }),
    )
    .unwrap();
    assert!(saved.contains("Saved WA script 'Test Script'"));

    let listed = call_tool_in_workspace(&root, "wa_list_scripts", &json!({})).unwrap();
    assert!(listed.contains("Test Script"));
}
