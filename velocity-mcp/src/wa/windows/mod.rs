pub mod execution;
pub mod payloads;
pub mod reports;
pub mod scripts;

pub use execution::*;
pub use reports::*;

#[cfg(test)]
pub(crate) fn save_windows_capture_report_from_json(
    root: &std::path::Path,
    session_id: &str,
    snapshot_name: &str,
    title_override: Option<&str>,
    json_payload: &str,
) -> Result<crate::wa::WaWindowsCaptureReport, Box<dyn std::error::Error>> {
    let payload = payloads::parse_capture_payload(json_payload)?;
    save_windows_capture_payload(root, session_id, snapshot_name, title_override, payload)
}

#[cfg(test)]
mod tests {
    use super::payloads::*;
    use super::reports::*;
    use super::*;

    #[test]
    fn saves_windows_capture_payload_into_wa_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        crate::wa::create_session_report(&root, "desktop-auth").unwrap();

        let report = save_windows_capture_report_from_json(
            &root,
            "desktop-auth",
            "live-window",
            None,
            r#"{
                "window_title": "Sign in",
                "process_id": 4242,
                "focus_node_id": "email-field",
                "nodes": [
                    {
                        "id": "email-field",
                        "role": "edit",
                        "name": "Email",
                        "value": "",
                        "actions": ["focus", "type"],
                        "visible": true,
                        "enabled": true,
                        "provenance": "native",
                        "confidence": 1.0
                    },
                    {
                        "id": "continue-button",
                        "role": "button",
                        "name": "Continue",
                        "value": "",
                        "actions": ["click"],
                        "visible": true,
                        "enabled": true,
                        "provenance": "native",
                        "confidence": 1.0
                    }
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(report.source, "windows-uia");
        assert_eq!(report.target_process_id, Some(4242));
        assert_eq!(report.target_window_title, "Sign in");
        assert_eq!(report.snapshot.url, "windows://uia/process/4242");
        assert_eq!(report.snapshot.title, "Sign in");
        assert_eq!(report.snapshot.focus_node_id.as_deref(), Some("email-field"));
        assert_eq!(report.snapshot.nodes.len(), 2);
        assert!(report.snapshot_nda_path.contains(".velocity/wa-snapshots/desktop-auth--live-window.nda"));
        assert!(report.session_nda_path.contains(".velocity/wa-sessions/desktop-auth.nda"));

        let saved = crate::wa::read_snapshot_report(&root, "desktop-auth", "live-window").unwrap();
        assert_eq!(saved.snapshot.nodes[1].id, "continue-button");
        assert_eq!(saved.snapshot.title, "Sign in");
    }

    #[test]
    fn builds_windows_action_report_from_planned_selector() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        crate::wa::create_session_report(&root, "desktop-auth").unwrap();
        crate::wa::save_snapshot_report(
            &root,
            "desktop-auth",
            "login-form",
            "windows://uia/process/4242",
            "Sign in",
            Some("email-field"),
            vec![
                crate::wa::WaNode {
                    id: "email-field".to_string(),
                    role: "textbox".to_string(),
                    name: "Email".to_string(),
                    value: "".to_string(),
                    actions: vec!["focus".to_string(), "type".to_string()],
                    visible: true,
                    enabled: true,
                    provenance: "native".to_string(),
                    confidence: 1.0,
                },
                crate::wa::WaNode {
                    id: "continue-button".to_string(),
                    role: "button".to_string(),
                    name: "Continue".to_string(),
                    value: "".to_string(),
                    actions: vec!["click".to_string()],
                    visible: true,
                    enabled: true,
                    provenance: "native".to_string(),
                    confidence: 1.0,
                },
            ],
        )
        .unwrap();

        let report = build_action_report_from_payload(
            &root,
            "desktop-auth",
            Some("login-form"),
            "click",
            None,
            Some("button"),
            Some("Continue"),
            None,
            WindowsActionPayload {
                window_title: "Sign in".to_string(),
                process_id: Some(4242),
                executed_node_id: "continue-button".to_string(),
                status: "executed".to_string(),
                detail: "invoke pattern executed".to_string(),
            },
        )
        .unwrap();

        assert_eq!(report.session_id, "desktop-auth");
        assert_eq!(report.snapshot_name, "login-form");
        assert_eq!(report.action, "click");
        assert_eq!(report.matched.id, "continue-button");
        assert_eq!(report.executed_node_id, "continue-button");
        assert_eq!(report.execution_status, "executed");
        assert_eq!(report.target_process_id, Some(4242));
        assert!(report.preconditions.iter().any(|value| value == "supports:click"));
        assert!(report.snapshot_nda_path.contains(".velocity/wa-snapshots/desktop-auth--login-form.nda"));
    }

    #[test]
    fn builds_windows_wait_report_from_resolved_selector() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        crate::wa::create_session_report(&root, "desktop-auth").unwrap();
        crate::wa::save_snapshot_report(
            &root,
            "desktop-auth",
            "login-form",
            "windows://uia/process/4242",
            "Sign in",
            Some("email-field"),
            vec![crate::wa::WaNode {
                id: "email-field".to_string(),
                role: "textbox".to_string(),
                name: "Email".to_string(),
                value: "agent@example.com".to_string(),
                actions: vec!["focus".to_string(), "type".to_string()],
                visible: true,
                enabled: true,
                provenance: "native".to_string(),
                confidence: 1.0,
            }],
        )
        .unwrap();

        let report = build_wait_report_from_payload(
            &root,
            "desktop-auth",
            Some("login-form"),
            "value_equals",
            None,
            Some("textbox"),
            Some("Email"),
            Some("agent@example.com"),
            3000,
            100,
            WindowsWaitPayload {
                window_title: "Sign in".to_string(),
                process_id: Some(4242),
                observed_value: Some("agent@example.com".to_string()),
                satisfied: true,
                elapsed_ms: 120,
                detail: "condition satisfied".to_string(),
            },
        )
        .unwrap();

        assert_eq!(report.session_id, "desktop-auth");
        assert_eq!(report.snapshot_name, "login-form");
        assert_eq!(report.condition, "value_equals");
        assert_eq!(report.expected_value.as_deref(), Some("agent@example.com"));
        assert_eq!(report.observed_value.as_deref(), Some("agent@example.com"));
        assert!(report.satisfied);
        assert_eq!(report.elapsed_ms, 120);
        assert_eq!(report.target_process_id, Some(4242));
        assert_eq!(report.matched.id, "email-field");
        assert!(report.snapshot_nda_path.contains(".velocity/wa-snapshots/desktop-auth--login-form.nda"));
    }
}
