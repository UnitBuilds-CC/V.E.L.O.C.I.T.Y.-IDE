use std::error::Error;
use std::io::{Error as IoError, ErrorKind};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::wa::model::{
    WaRunArtifactReport, WaScriptReadReport, WaScriptRunReport, WaScriptRunStepReport,
    WaScriptStep, WaWindowsActionReport, WaWindowsWaitReport,
};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn build_run_id(session_id: &str, script_name: &str, created_at_ms: u64) -> String {
    format!("{}-{}-{}", session_id, script_name, created_at_ms)
}

fn resolve_snapshot_name(
    root: &Path,
    session_id: &str,
    snapshot_name: Option<&str>,
) -> Result<String, Box<dyn Error>> {
    if let Some(snapshot_name) = snapshot_name {
        return Ok(snapshot_name.to_string());
    }
    let session = crate::wa::load_session(root, session_id)?;
    session.latest_snapshot_name.ok_or_else(|| {
        IoError::new(
            ErrorKind::NotFound,
            format!("session '{session_id}' has no saved WA snapshot"),
        )
        .into()
    })
}

fn verification_plan(step: &WaScriptStep) -> Option<(&'static str, Option<&str>)> {
    if step.action.eq_ignore_ascii_case("focus") {
        Some(("focused", None))
    } else if step.action.eq_ignore_ascii_case("type") {
        step.value
            .as_deref()
            .map(|value| ("value_equals", Some(value)))
    } else {
        None
    }
}

fn build_script_run_report<F, G>(
    root: &Path,
    session_id: &str,
    snapshot_name: Option<&str>,
    script_report: WaScriptReadReport,
    start_step_index: usize,
    mut executor: F,
    mut verifier: G,
) -> Result<WaScriptRunReport, Box<dyn Error>>
where
    F: FnMut(&str, &WaScriptStep) -> Result<WaWindowsActionReport, Box<dyn Error>>,
    G: FnMut(&str, &WaScriptStep) -> Result<Option<WaWindowsWaitReport>, Box<dyn Error>>,
{
    let resolved_snapshot_name = resolve_snapshot_name(root, session_id, snapshot_name)?;
    let created_at_ms = now_ms();
    let run_id = build_run_id(session_id, &script_report.script.name, created_at_ms);
    let step_count = script_report.script.steps.len();
    if start_step_index > step_count {
        return Err(IoError::new(
            ErrorKind::InvalidInput,
            format!(
                "start step index {} is out of range for script '{}' with {} step(s)",
                start_step_index, script_report.script.name, step_count
            ),
        )
        .into());
    }
    let mut completed_step_count = 0usize;
    let mut verified_step_count = 0usize;
    let mut stopped_at_step_index = None;
    let mut succeeded = true;
    let mut steps = Vec::with_capacity(step_count.saturating_sub(start_step_index));

    for (index, step) in script_report
        .script
        .steps
        .iter()
        .enumerate()
        .skip(start_step_index)
    {
        match executor(&resolved_snapshot_name, step) {
            Ok(action_report) => {
                completed_step_count += 1;
                let mut status = "executed".to_string();
                let mut detail = action_report.execution_detail.clone();
                let mut verification_status = None;
                let mut verification_detail = None;
                let mut wait_report = None;

                if let Some(verification_report) = verifier(&resolved_snapshot_name, step)? {
                    if verification_report.satisfied {
                        verified_step_count += 1;
                        verification_status = Some("verified".to_string());
                        verification_detail = Some(verification_report.detail.clone());
                        wait_report = Some(verification_report);
                        status = "verified".to_string();
                    } else {
                        let verify_status = if step.required {
                            succeeded = false;
                            stopped_at_step_index = Some(index);
                            "verification_failed"
                        } else {
                            "optional_verification_failed"
                        };
                        detail = format!("{}; verification failed", detail);
                        verification_status = Some(verify_status.to_string());
                        verification_detail = Some(verification_report.detail.clone());
                        wait_report = Some(verification_report);
                        status = verify_status.to_string();
                    }
                }

                steps.push(WaScriptRunStepReport {
                    index,
                    action: step.action.clone(),
                    required: step.required,
                    node_id: step.node_id.clone(),
                    role: step.role.clone(),
                    name: step.name.clone(),
                    value: step.value.clone(),
                    status,
                    detail,
                    verification_status,
                    verification_detail,
                    action_report: Some(action_report),
                    wait_report,
                });
                if !succeeded {
                    break;
                }
            }
            Err(err) => {
                let status = if step.required {
                    succeeded = false;
                    stopped_at_step_index = Some(index);
                    "failed"
                } else {
                    "optional_failed"
                };
                steps.push(WaScriptRunStepReport {
                    index,
                    action: step.action.clone(),
                    required: step.required,
                    node_id: step.node_id.clone(),
                    role: step.role.clone(),
                    name: step.name.clone(),
                    value: step.value.clone(),
                    status: status.to_string(),
                    detail: err.to_string(),
                    verification_status: None,
                    verification_detail: None,
                    action_report: None,
                    wait_report: None,
                });
                if !succeeded {
                    break;
                }
            }
        }
    }

    Ok(WaScriptRunReport {
        run_id,
        created_at_ms,
        source: "windows-uia".to_string(),
        session_id: session_id.to_string(),
        snapshot_name: resolved_snapshot_name,
        script_name: script_report.script.name,
        script_relative_file_path: script_report.relative_file_path,
        script_nda_path: script_report.nda_path,
        start_step_index,
        step_count,
        completed_step_count,
        verified_step_count,
        succeeded,
        stopped_at_step_index,
        steps,
    })
}

#[allow(dead_code)]
pub fn run_script_report(
    root: &Path,
    session_id: &str,
    relative_file_path: &Path,
    snapshot_name: Option<&str>,
) -> Result<WaScriptRunReport, Box<dyn Error>> {
    let script_report = crate::wa::read_script_report(root, relative_file_path)?;
    build_script_run_report(
        root,
        session_id,
        snapshot_name,
        script_report,
        0,
        |resolved_snapshot_name, step| {
            crate::wa::execute_windows_action_report(
                root,
                session_id,
                Some(resolved_snapshot_name),
                &step.action,
                step.node_id.as_deref(),
                step.role.as_deref(),
                step.name.as_deref(),
                step.value.as_deref(),
            )
        },
        |resolved_snapshot_name, step| {
            let Some((condition, expected_value)) = verification_plan(step) else {
                return Ok(None);
            };
            let report = crate::wa::wait_for_windows_condition_report(
                root,
                session_id,
                Some(resolved_snapshot_name),
                condition,
                step.node_id.as_deref(),
                step.role.as_deref(),
                step.name.as_deref(),
                expected_value,
                1500,
                100,
            )?;
            Ok(Some(report))
        },
    )
}

pub fn run_and_persist_script_report(
    root: &Path,
    session_id: &str,
    relative_file_path: &Path,
    snapshot_name: Option<&str>,
    start_step_index: Option<usize>,
) -> Result<WaRunArtifactReport, Box<dyn Error>> {
    let script_report = crate::wa::read_script_report(root, relative_file_path)?;
    let report = build_script_run_report(
        root,
        session_id,
        snapshot_name,
        script_report,
        start_step_index.unwrap_or(0),
        |resolved_snapshot_name, step| {
            crate::wa::execute_windows_action_report(
                root,
                session_id,
                Some(resolved_snapshot_name),
                &step.action,
                step.node_id.as_deref(),
                step.role.as_deref(),
                step.name.as_deref(),
                step.value.as_deref(),
            )
        },
        |resolved_snapshot_name, step| {
            let Some((condition, expected_value)) = verification_plan(step) else {
                return Ok(None);
            };
            let report = crate::wa::wait_for_windows_condition_report(
                root,
                session_id,
                Some(resolved_snapshot_name),
                condition,
                step.node_id.as_deref(),
                step.role.as_deref(),
                step.name.as_deref(),
                expected_value,
                1500,
                100,
            )?;
            Ok(Some(report))
        },
    )?;
    crate::wa::save_run_report(root, &report)
}

pub fn render_script_run_report(report: &WaScriptRunReport) -> String {
    let mut lines = vec![format!(
        "Ran WA script '{}' in session '{}' snapshot '{}'.",
        report.script_name, report.session_id, report.snapshot_name
    )];
    lines.push(format!("Run id: {}", report.run_id));
    lines.push(format!(
        "Succeeded: {} ({}/{} executed, {} verified)",
        report.succeeded,
        report.completed_step_count,
        report.step_count,
        report.verified_step_count
    ));
    if report.start_step_index > 0 {
        lines.push(format!("Started at step: {}", report.start_step_index + 1));
    }
    if let Some(index) = report.stopped_at_step_index {
        lines.push(format!("Stopped at step: {}", index + 1));
    }
    lines.push(format!("Script NDA: {}", report.script_nda_path));
    if !report.steps.is_empty() {
        lines.push("Steps:".to_string());
        for step in &report.steps {
            let target = step
                .name
                .as_deref()
                .or(step.node_id.as_deref())
                .unwrap_or("unnamed target");
            let verification_suffix = step
                .verification_status
                .as_deref()
                .map(|status| {
                    format!(
                        "; verify={} ({})",
                        status,
                        step.verification_detail.as_deref().unwrap_or("no detail")
                    )
                })
                .unwrap_or_default();
            lines.push(format!(
                "{}. {} [{}] -> {} ({}{})",
                step.index + 1,
                step.action,
                target,
                step.status,
                step.detail,
                verification_suffix
            ));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::build_script_run_report;
    use std::io::{Error as IoError, ErrorKind};

    fn fake_action_report(
        session_id: &str,
        snapshot_name: &str,
        step: &crate::wa::WaScriptStep,
    ) -> crate::wa::WaWindowsActionReport {
        crate::wa::WaWindowsActionReport {
            source: "windows-uia".to_string(),
            session_id: session_id.to_string(),
            snapshot_name: snapshot_name.to_string(),
            action: step.action.clone(),
            requested_value: step.value.clone(),
            selector: step.clone(),
            matched: crate::wa::WaNode {
                id: step.node_id.clone().unwrap_or_else(|| "node:1".to_string()),
                role: step.role.clone().unwrap_or_else(|| "button".to_string()),
                name: step.name.clone().unwrap_or_else(|| "Target".to_string()),
                value: step.value.clone().unwrap_or_default(),
                actions: vec![step.action.clone()],
                visible: true,
                enabled: true,
                provenance: "native".to_string(),
                confidence: 1.0,
            },
            preconditions: vec!["visible".to_string(), "enabled".to_string()],
            target_process_id: Some(4242),
            target_window_title: "Sign in".to_string(),
            executed_node_id: step.node_id.clone().unwrap_or_else(|| "node:1".to_string()),
            execution_status: "executed".to_string(),
            execution_detail: "condition satisfied".to_string(),
            snapshot_nda_path: ".velocity/wa-snapshots/desktop-auth--login-form.nda".to_string(),
        }
    }

    #[test]
    fn continues_past_optional_script_failures() {
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
                value: "".to_string(),
                actions: vec!["type".to_string()],
                visible: true,
                enabled: true,
                provenance: "native".to_string(),
                confidence: 1.0,
            }],
        )
        .unwrap();
        let saved = crate::wa::save_script_report(
            &root,
            "Sign in flow",
            Some("windows://settings/sign-in"),
            vec![
                crate::wa::WaScriptStep {
                    action: "type".to_string(),
                    node_id: Some("email-field".to_string()),
                    role: Some("textbox".to_string()),
                    name: Some("Email".to_string()),
                    value: Some("agent@example.com".to_string()),
                    required: true,
                },
                crate::wa::WaScriptStep {
                    action: "click".to_string(),
                    node_id: Some("continue-button".to_string()),
                    role: Some("button".to_string()),
                    name: Some("Continue".to_string()),
                    value: None,
                    required: false,
                },
                crate::wa::WaScriptStep {
                    action: "focus".to_string(),
                    node_id: Some("email-field".to_string()),
                    role: Some("textbox".to_string()),
                    name: Some("Email".to_string()),
                    value: None,
                    required: true,
                },
            ],
        )
        .unwrap();
        let script =
            crate::wa::read_script_report(&root, &root.join(saved.relative_file_path)).unwrap();

        let report = build_script_run_report(
            &root,
            "desktop-auth",
            Some("login-form"),
            script,
            0,
            |snapshot_name, step| {
                if step.action == "click" {
                    Err(IoError::new(ErrorKind::NotFound, "optional target missing").into())
                } else {
                    Ok(fake_action_report("desktop-auth", snapshot_name, step))
                }
            },
            |_snapshot_name, step| {
                let wait_report = if step.action == "type" {
                    Some(crate::wa::WaWindowsWaitReport {
                        source: "windows-uia".to_string(),
                        session_id: "desktop-auth".to_string(),
                        snapshot_name: "login-form".to_string(),
                        condition: "value_equals".to_string(),
                        expected_value: step.value.clone(),
                        selector: step.clone(),
                        matched: crate::wa::WaNode {
                            id: step.node_id.clone().unwrap_or_else(|| "node:1".to_string()),
                            role: step.role.clone().unwrap_or_else(|| "textbox".to_string()),
                            name: step.name.clone().unwrap_or_else(|| "Target".to_string()),
                            value: step.value.clone().unwrap_or_default(),
                            actions: vec![step.action.clone()],
                            visible: true,
                            enabled: true,
                            provenance: "native".to_string(),
                            confidence: 1.0,
                        },
                        target_process_id: Some(4242),
                        target_window_title: "Sign in".to_string(),
                        observed_value: step.value.clone(),
                        satisfied: true,
                        elapsed_ms: 25,
                        timeout_ms: 1500,
                        poll_interval_ms: 100,
                        detail: "condition satisfied".to_string(),
                        snapshot_nda_path: ".velocity/wa-snapshots/desktop-auth--login-form.nda"
                            .to_string(),
                    })
                } else if step.action == "focus" {
                    Some(crate::wa::WaWindowsWaitReport {
                        source: "windows-uia".to_string(),
                        session_id: "desktop-auth".to_string(),
                        snapshot_name: "login-form".to_string(),
                        condition: "focused".to_string(),
                        expected_value: None,
                        selector: step.clone(),
                        matched: crate::wa::WaNode {
                            id: step.node_id.clone().unwrap_or_else(|| "node:1".to_string()),
                            role: step.role.clone().unwrap_or_else(|| "textbox".to_string()),
                            name: step.name.clone().unwrap_or_else(|| "Target".to_string()),
                            value: step.value.clone().unwrap_or_default(),
                            actions: vec![step.action.clone()],
                            visible: true,
                            enabled: true,
                            provenance: "native".to_string(),
                            confidence: 1.0,
                        },
                        target_process_id: Some(4242),
                        target_window_title: "Sign in".to_string(),
                        observed_value: Some("true".to_string()),
                        satisfied: true,
                        elapsed_ms: 10,
                        timeout_ms: 1500,
                        poll_interval_ms: 100,
                        detail: "condition satisfied".to_string(),
                        snapshot_nda_path: ".velocity/wa-snapshots/desktop-auth--login-form.nda"
                            .to_string(),
                    })
                } else {
                    None
                };
                Ok(wait_report)
            },
        )
        .unwrap();

        assert!(report.succeeded);
        assert_eq!(report.completed_step_count, 2);
        assert_eq!(report.verified_step_count, 2);
        assert_eq!(report.stopped_at_step_index, None);
        assert_eq!(report.steps.len(), 3);
        assert_eq!(report.steps[0].status, "verified");
        assert_eq!(report.steps[1].status, "optional_failed");
        assert_eq!(report.steps[2].status, "verified");
    }

    #[test]
    fn stops_on_required_verification_failure() {
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
                value: "".to_string(),
                actions: vec!["type".to_string()],
                visible: true,
                enabled: true,
                provenance: "native".to_string(),
                confidence: 1.0,
            }],
        )
        .unwrap();
        let saved = crate::wa::save_script_report(
            &root,
            "Sign in flow",
            Some("windows://settings/sign-in"),
            vec![
                crate::wa::WaScriptStep {
                    action: "type".to_string(),
                    node_id: Some("email-field".to_string()),
                    role: Some("textbox".to_string()),
                    name: Some("Email".to_string()),
                    value: Some("agent@example.com".to_string()),
                    required: true,
                },
                crate::wa::WaScriptStep {
                    action: "focus".to_string(),
                    node_id: Some("email-field".to_string()),
                    role: Some("textbox".to_string()),
                    name: Some("Email".to_string()),
                    value: None,
                    required: true,
                },
            ],
        )
        .unwrap();
        let script =
            crate::wa::read_script_report(&root, &root.join(saved.relative_file_path)).unwrap();

        let report = build_script_run_report(
            &root,
            "desktop-auth",
            Some("login-form"),
            script,
            0,
            |snapshot_name, step| Ok(fake_action_report("desktop-auth", snapshot_name, step)),
            |_snapshot_name, step| {
                if step.action == "type" {
                    Ok(Some(crate::wa::WaWindowsWaitReport {
                        source: "windows-uia".to_string(),
                        session_id: "desktop-auth".to_string(),
                        snapshot_name: "login-form".to_string(),
                        condition: "value_equals".to_string(),
                        expected_value: step.value.clone(),
                        selector: step.clone(),
                        matched: crate::wa::WaNode {
                            id: step.node_id.clone().unwrap_or_else(|| "node:1".to_string()),
                            role: step.role.clone().unwrap_or_else(|| "textbox".to_string()),
                            name: step.name.clone().unwrap_or_else(|| "Target".to_string()),
                            value: "wrong@example.com".to_string(),
                            actions: vec![step.action.clone()],
                            visible: true,
                            enabled: true,
                            provenance: "native".to_string(),
                            confidence: 1.0,
                        },
                        target_process_id: Some(4242),
                        target_window_title: "Sign in".to_string(),
                        observed_value: Some("wrong@example.com".to_string()),
                        satisfied: false,
                        elapsed_ms: 1500,
                        timeout_ms: 1500,
                        poll_interval_ms: 100,
                        detail: "condition not yet satisfied (observed 'wrong@example.com')"
                            .to_string(),
                        snapshot_nda_path: ".velocity/wa-snapshots/desktop-auth--login-form.nda"
                            .to_string(),
                    }))
                } else {
                    Ok(None)
                }
            },
        )
        .unwrap();

        assert!(!report.succeeded);
        assert_eq!(report.completed_step_count, 1);
        assert_eq!(report.verified_step_count, 0);
        assert_eq!(report.stopped_at_step_index, Some(0));
        assert_eq!(report.steps.len(), 1);
        assert_eq!(report.steps[0].status, "verification_failed");
        assert_eq!(
            report.steps[0].verification_status.as_deref(),
            Some("verification_failed")
        );
    }

    #[test]
    fn stops_on_required_script_failure() {
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
                value: "".to_string(),
                actions: vec!["type".to_string()],
                visible: true,
                enabled: true,
                provenance: "native".to_string(),
                confidence: 1.0,
            }],
        )
        .unwrap();
        let saved = crate::wa::save_script_report(
            &root,
            "Sign in flow",
            Some("windows://settings/sign-in"),
            vec![
                crate::wa::WaScriptStep {
                    action: "type".to_string(),
                    node_id: Some("email-field".to_string()),
                    role: Some("textbox".to_string()),
                    name: Some("Email".to_string()),
                    value: Some("agent@example.com".to_string()),
                    required: true,
                },
                crate::wa::WaScriptStep {
                    action: "click".to_string(),
                    node_id: Some("continue-button".to_string()),
                    role: Some("button".to_string()),
                    name: Some("Continue".to_string()),
                    value: None,
                    required: true,
                },
                crate::wa::WaScriptStep {
                    action: "focus".to_string(),
                    node_id: Some("email-field".to_string()),
                    role: Some("textbox".to_string()),
                    name: Some("Email".to_string()),
                    value: None,
                    required: true,
                },
            ],
        )
        .unwrap();
        let script =
            crate::wa::read_script_report(&root, &root.join(saved.relative_file_path)).unwrap();

        let report = build_script_run_report(
            &root,
            "desktop-auth",
            Some("login-form"),
            script,
            0,
            |snapshot_name, step| {
                if step.action == "click" {
                    Err(IoError::other("required action failed").into())
                } else {
                    Ok(fake_action_report("desktop-auth", snapshot_name, step))
                }
            },
            |_snapshot_name, step| {
                if step.action == "type" {
                    Ok(Some(crate::wa::WaWindowsWaitReport {
                        source: "windows-uia".to_string(),
                        session_id: "desktop-auth".to_string(),
                        snapshot_name: "login-form".to_string(),
                        condition: "value_equals".to_string(),
                        expected_value: step.value.clone(),
                        selector: step.clone(),
                        matched: crate::wa::WaNode {
                            id: step.node_id.clone().unwrap_or_else(|| "node:1".to_string()),
                            role: step.role.clone().unwrap_or_else(|| "textbox".to_string()),
                            name: step.name.clone().unwrap_or_else(|| "Target".to_string()),
                            value: step.value.clone().unwrap_or_default(),
                            actions: vec![step.action.clone()],
                            visible: true,
                            enabled: true,
                            provenance: "native".to_string(),
                            confidence: 1.0,
                        },
                        target_process_id: Some(4242),
                        target_window_title: "Sign in".to_string(),
                        observed_value: step.value.clone(),
                        satisfied: true,
                        elapsed_ms: 20,
                        timeout_ms: 1500,
                        poll_interval_ms: 100,
                        detail: "condition satisfied".to_string(),
                        snapshot_nda_path: ".velocity/wa-snapshots/desktop-auth--login-form.nda"
                            .to_string(),
                    }))
                } else {
                    Ok(None)
                }
            },
        )
        .unwrap();

        assert!(!report.succeeded);
        assert_eq!(report.completed_step_count, 1);
        assert_eq!(report.verified_step_count, 1);
        assert_eq!(report.stopped_at_step_index, Some(1));
        assert_eq!(report.steps.len(), 2);
        assert_eq!(report.steps[0].status, "verified");
        assert_eq!(report.steps[1].status, "failed");
    }
}
