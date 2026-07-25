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

#[test]
fn wa_registry_hive_parsing_and_script_generation() {
    use crate::wa::registry::*;

    // Test hive parsing
    assert_eq!(RegistryHive::from_str("HKCU"), Some(RegistryHive::CurrentUser));
    assert_eq!(RegistryHive::from_str("HKLM"), Some(RegistryHive::LocalMachine));
    assert_eq!(RegistryHive::from_str("invalid"), None);

    // Test PS path conversion
    assert_eq!(RegistryHive::CurrentUser.as_ps_path(), "HKCU:");
    assert_eq!(RegistryHive::LocalMachine.as_ps_path(), "HKLM:");

    // Test script generation
    let script = build_read_registry_script(
        RegistryHive::CurrentUser,
        "SOFTWARE\\Test",
        "Value1",
    );
    assert!(script.contains("HKCU:"));
    assert!(script.contains("Get-ItemProperty"));
    assert!(script.contains("Value1"));

    // Test write script generation
    let entry = RegistryEntry {
        hive: RegistryHive::CurrentUser,
        path: "SOFTWARE\\Test".to_string(),
        name: "Setting".to_string(),
        value: RegistryValue::DWord(42),
    };
    let write_script = build_write_registry_script(&entry);
    assert!(write_script.contains("Set-ItemProperty"));
    assert!(write_script.contains("42"));
    assert!(write_script.contains("DWord"));
}

#[test]
fn wa_notifications_script_generation() {
    use crate::wa::notifications::*;

    let detect_script = build_detect_notifications_script();
    assert!(detect_script.contains("CoreWindow"));
    assert!(detect_script.contains("Notification"));
    assert!(detect_script.contains("UIAutomationClient"));

    let dismiss_script = build_dismiss_notifications_script(Some("Update"));
    assert!(dismiss_script.contains("Close"));
    assert!(dismiss_script.contains("Update"));
    assert!(dismiss_script.contains("InvokePattern"));

    let tray_script = build_enumerate_tray_script();
    assert!(tray_script.contains("Shell_TrayWnd"));

    // Test config defaults
    let config = NotificationWatchConfig::default();
    assert_eq!(config.duration, std::time::Duration::from_secs(30));
    assert!(config.capture_content);
}

#[test]
fn wa_virtual_desktop_script_generation() {
    use crate::wa::virtual_desktop::*;

    let enum_script = build_enumerate_desktops_script();
    assert!(enum_script.contains("VirtualDesktops"));
    assert!(enum_script.contains("CurrentVirtualDesktop"));

    let switch_script = build_switch_desktop_script(2);
    assert!(switch_script.contains("keybd_event"));
    assert!(switch_script.contains("targetIdx = 2"));

    let create_script = build_create_desktop_script(Some("Work".into()));
    assert!(create_script.contains("VDCreate"));
    assert!(create_script.contains("0x44")); // D key

    let remove_script = build_remove_desktop_script(1);
    assert!(remove_script.contains("VDRemove"));
    assert!(remove_script.contains("0x73")); // F4 key

    let pin_script = build_pin_window_script(12345);
    assert!(pin_script.contains("VDPin"));
    assert!(pin_script.contains("12345"));
    assert!(pin_script.contains("WS_EX_TOOLWINDOW"));

    // Test manager state
    let state = VirtualDesktopState {
        desktops: vec![
            VirtualDesktop {
                id: "a".into(), name: Some("Work".into()),
                index: 0, is_current: true, window_count: Some(5),
            },
            VirtualDesktop {
                id: "b".into(), name: Some("Personal".into()),
                index: 1, is_current: false, window_count: Some(3),
            },
        ],
        current_index: 0,
        total_count: 2,
        supports_named_desktops: true,
    };
    assert_eq!(state.by_name("personal").unwrap().index, 1);
    assert_eq!(state.by_index(0).unwrap().name.as_deref(), Some("Work"));
}
