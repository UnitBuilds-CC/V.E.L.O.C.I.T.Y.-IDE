use super::payloads::*;
use crate::wa::{WaWindowsActionReport, WaWindowsCaptureReport, WaWindowsWaitReport};
use std::error::Error;
use std::path::Path;

pub fn build_action_report_from_payload(
    root: &Path,
    session_id: &str,
    snapshot_name: Option<&str>,
    action: &str,
    node_id: Option<&str>,
    role: Option<&str>,
    name: Option<&str>,
    input_value: Option<&str>,
    payload: WindowsActionPayload,
) -> Result<WaWindowsActionReport, Box<dyn Error>> {
    let plan = crate::wa::plan_action(
        root,
        session_id,
        snapshot_name,
        action,
        node_id,
        role,
        name,
        input_value,
    )?;
    Ok(WaWindowsActionReport {
        source: "windows-uia".to_string(),
        session_id: session_id.to_string(),
        snapshot_name: plan.snapshot_name,
        action: action.to_string(),
        requested_value: input_value.map(|value| value.to_string()),
        selector: plan.selector,
        matched: plan.matched,
        preconditions: plan.preconditions,
        target_process_id: payload.process_id,
        target_window_title: payload.window_title,
        executed_node_id: payload.executed_node_id,
        execution_status: payload.status,
        execution_detail: payload.detail,
        snapshot_nda_path: plan.snapshot_nda_path,
    })
}

pub fn build_wait_report_from_payload(
    root: &Path,
    session_id: &str,
    snapshot_name: Option<&str>,
    condition: &str,
    node_id: Option<&str>,
    role: Option<&str>,
    name: Option<&str>,
    expected_value: Option<&str>,
    timeout_ms: u64,
    poll_interval_ms: u64,
    payload: WindowsWaitPayload,
) -> Result<WaWindowsWaitReport, Box<dyn Error>> {
    let probe_action = match condition {
        "focused" => "focus",
        "value_equals" => "type",
        _ => "inspect",
    };
    let resolve = crate::wa::resolve_selector(
        root,
        session_id,
        snapshot_name,
        node_id,
        role,
        name,
        if probe_action == "inspect" { None } else { Some(probe_action) },
    )?;
    Ok(WaWindowsWaitReport {
        source: "windows-uia".to_string(),
        session_id: session_id.to_string(),
        snapshot_name: resolve.snapshot_name,
        condition: condition.to_string(),
        expected_value: expected_value.map(|value| value.to_string()),
        selector: resolve.selector,
        matched: resolve.matched,
        target_process_id: payload.process_id,
        target_window_title: payload.window_title,
        observed_value: payload.observed_value,
        satisfied: payload.satisfied,
        elapsed_ms: payload.elapsed_ms,
        timeout_ms,
        poll_interval_ms,
        detail: payload.detail,
        snapshot_nda_path: resolve.snapshot_nda_path,
    })
}

pub fn save_windows_capture_payload(
    root: &Path,
    session_id: &str,
    snapshot_name: &str,
    title_override: Option<&str>,
    payload: WindowsCapturePayload,
) -> Result<WaWindowsCaptureReport, Box<dyn Error>> {
    let window_title = if payload.window_title.trim().is_empty() {
        "Windows UIA capture".to_string()
    } else {
        payload.window_title
    };
    let title = title_override.unwrap_or(&window_title);
    let url = match payload.process_id {
        Some(process_id) => format!("windows://uia/process/{process_id}"),
        None => "windows://uia/window".to_string(),
    };
    let save_report = crate::wa::save_snapshot_report(
        root,
        session_id,
        snapshot_name,
        &url,
        title,
        payload.focus_node_id.as_deref(),
        payload.nodes,
    )?;
    Ok(WaWindowsCaptureReport {
        source: "windows-uia".to_string(),
        target_process_id: payload.process_id,
        target_window_title: window_title,
        snapshot: save_report.snapshot,
        snapshot_nda_path: save_report.snapshot_nda_path,
        session_nda_path: save_report.session_nda_path,
    })
}

pub fn render_windows_wait_report(report: &WaWindowsWaitReport) -> String {
    format!(
        "Waited for Windows WA condition '{}' in session '{}' snapshot '{}'.\nTarget window: {}\nProcess id: {}\nNode: {} [{}] '{}'\nSatisfied: {}\nObserved: {}\nElapsed: {}ms / {}ms\nDetail: {}\nSnapshot NDA: {}",
        report.condition,
        report.session_id,
        report.snapshot_name,
        report.target_window_title,
        report
            .target_process_id
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        report.matched.id,
        report.matched.role,
        report.matched.name,
        report.satisfied,
        report.observed_value.as_deref().unwrap_or("unknown"),
        report.elapsed_ms,
        report.timeout_ms,
        report.detail,
        report.snapshot_nda_path,
    )
}

pub fn render_windows_action_report(report: &WaWindowsActionReport) -> String {
    format!(
        "Executed Windows WA action '{}' in session '{}' snapshot '{}'.\nTarget window: {}\nProcess id: {}\nNode: {} [{}] '{}'\nExecution: {} ({})\nSnapshot NDA: {}",
        report.action,
        report.session_id,
        report.snapshot_name,
        report.target_window_title,
        report
            .target_process_id
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        report.matched.id,
        report.matched.role,
        report.matched.name,
        report.execution_status,
        report.execution_detail,
        report.snapshot_nda_path,
    )
}

pub fn render_windows_capture_report(report: &WaWindowsCaptureReport) -> String {
    format!(
        "Captured Windows WA snapshot '{}' for session '{}'.\nTarget window: {}\nProcess id: {}\nNodes: {}\nFocused node: {}\nSnapshot NDA: {}",
        report.snapshot.snapshot_name,
        report.snapshot.session_id,
        report.target_window_title,
        report
            .target_process_id
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        report.snapshot.nodes.len(),
        report
            .snapshot
            .focus_node_id
            .as_deref()
            .unwrap_or("unknown"),
        report.snapshot_nda_path,
    )
}
