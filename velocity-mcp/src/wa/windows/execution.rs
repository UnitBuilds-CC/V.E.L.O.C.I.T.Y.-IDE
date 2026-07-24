use super::payloads::*;
use super::reports::*;
use super::scripts::*;
use crate::wa::{WaWindowsActionReport, WaWindowsCaptureReport, WaWindowsWaitReport};
use std::error::Error;
use std::io::{Error as IoError, ErrorKind, Write};
use std::path::Path;
use std::process::{Command, Stdio};

pub fn capture_windows_snapshot_report(
    root: &Path,
    session_id: &str,
    snapshot_name: &str,
    title_override: Option<&str>,
    process_id: Option<u32>,
    window_name_contains: Option<&str>,
    max_depth: u32,
    max_children_per_node: usize,
) -> Result<WaWindowsCaptureReport, Box<dyn Error>> {
    if !cfg!(target_os = "windows") {
        return Err(IoError::new(
            ErrorKind::Unsupported,
            "WA Windows capture is only supported on Windows hosts",
        )
        .into());
    }

    let mut child = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "-",
        ])
        .env("WA_CAPTURE_MAX_DEPTH", max_depth.to_string())
        .env("WA_CAPTURE_MAX_CHILDREN", max_children_per_node.to_string())
        .env(
            "WA_CAPTURE_PROCESS_ID",
            process_id.map(|value| value.to_string()).unwrap_or_default(),
        )
        .env(
            "WA_CAPTURE_WINDOW_NAME_CONTAINS",
            window_name_contains.unwrap_or_default(),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(build_capture_script().as_bytes())?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        return Err(IoError::other(
            format!("Windows UIAutomation capture failed: {detail}"),
        )
        .into());
    }

    let payload = parse_capture_payload(&String::from_utf8_lossy(&output.stdout))?;
    save_windows_capture_payload(root, session_id, snapshot_name, title_override, payload)
}

pub fn execute_windows_action_report(
    root: &Path,
    session_id: &str,
    snapshot_name: Option<&str>,
    action: &str,
    node_id: Option<&str>,
    role: Option<&str>,
    name: Option<&str>,
    input_value: Option<&str>,
) -> Result<WaWindowsActionReport, Box<dyn Error>> {
    if !cfg!(target_os = "windows") {
        return Err(IoError::new(
            ErrorKind::Unsupported,
            "WA Windows action execution is only supported on Windows hosts",
        )
        .into());
    }

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
    let snapshot = crate::wa::load_snapshot(root, session_id, &plan.snapshot_name)?;
    let process_id = snapshot
        .url
        .strip_prefix("windows://uia/process/")
        .and_then(|value| value.parse::<u32>().ok());

    let mut child = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "-",
        ])
        .env(
            "WA_ACTION_PROCESS_ID",
            process_id.map(|value| value.to_string()).unwrap_or_default(),
        )
        .env("WA_ACTION_WINDOW_NAME_CONTAINS", snapshot.title.clone())
        .env("WA_ACTION_NODE_ID", plan.matched.id.clone())
        .env("WA_ACTION_NAME", action)
        .env("WA_ACTION_VALUE", input_value.unwrap_or_default())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(build_action_script().as_bytes())?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        return Err(IoError::other(
            format!("Windows UIAutomation action execution failed: {detail}"),
        )
        .into());
    }

    let payload = parse_action_payload(&String::from_utf8_lossy(&output.stdout))?;
    build_action_report_from_payload(
        root,
        session_id,
        snapshot_name,
        action,
        node_id,
        role,
        name,
        input_value,
        payload,
    )
}

pub fn wait_for_windows_condition_report(
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
) -> Result<WaWindowsWaitReport, Box<dyn Error>> {
    if !cfg!(target_os = "windows") {
        return Err(IoError::new(
            ErrorKind::Unsupported,
            "WA Windows wait execution is only supported on Windows hosts",
        )
        .into());
    }

    let resolve = crate::wa::resolve_selector(
        root,
        session_id,
        snapshot_name,
        node_id,
        role,
        name,
        None,
    )?;
    let snapshot = crate::wa::load_snapshot(root, session_id, &resolve.snapshot_name)?;
    let process_id = snapshot
        .url
        .strip_prefix("windows://uia/process/")
        .and_then(|value| value.parse::<u32>().ok());

    let mut child = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "-",
        ])
        .env(
            "WA_WAIT_PROCESS_ID",
            process_id.map(|value| value.to_string()).unwrap_or_default(),
        )
        .env("WA_WAIT_WINDOW_NAME_CONTAINS", snapshot.title.clone())
        .env("WA_WAIT_NODE_ID", resolve.matched.id.clone())
        .env("WA_WAIT_CONDITION", condition)
        .env("WA_WAIT_EXPECTED_VALUE", expected_value.unwrap_or_default())
        .env("WA_WAIT_TIMEOUT_MS", timeout_ms.to_string())
        .env("WA_WAIT_POLL_MS", poll_interval_ms.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(build_wait_script().as_bytes())?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        return Err(IoError::other(
            format!("Windows UIAutomation wait failed: {detail}"),
        )
        .into());
    }

    let payload = parse_wait_payload(&String::from_utf8_lossy(&output.stdout))?;
    build_wait_report_from_payload(
        root,
        session_id,
        snapshot_name,
        condition,
        node_id,
        role,
        name,
        expected_value,
        timeout_ms,
        poll_interval_ms,
        payload,
    )
}
