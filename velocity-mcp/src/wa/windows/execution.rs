use super::payloads::*;
use super::reports::*;
use super::scripts::*;
use crate::wa::{WaWindowsActionReport, WaWindowsCaptureReport, WaWindowsWaitReport};
use std::error::Error;
use std::io::{Error as IoError, ErrorKind};
use std::path::Path;
use std::time::Duration;

/// Budget for a UIA capture: the walk is bounded by the tree it is given, not
/// by a caller-supplied deadline, so it gets the default script budget.
const CAPTURE_BUDGET: Duration = crate::wa::ps::DEFAULT_BUDGET;

/// Budget for an action: one element lookup plus one pattern call.
const ACTION_BUDGET: Duration = crate::wa::ps::DEFAULT_BUDGET;

/// These three calls used to spawn `powershell` inline with `-Command -` and
/// pipe a multi-line script into stdin. Windows PowerShell 5.1 consumes piped
/// command input statement by statement, so the here-strings behind
/// `Add-Type @"..."` were discarded while the process still exited 0 printing
/// nothing - which `serde_json` then reported as
/// "EOF while parsing a value at line 1 column 0" (bug #33). They now go
/// through the shared `-File` runner, which also bounds them with a deadline
/// the old unbounded `wait_with_output()` never had.
fn run_uia_script(
    label: &str,
    script: &str,
    budget: Duration,
    envs: &[(&str, &str)],
) -> Result<String, Box<dyn Error>> {
    crate::wa::ps::run_ps_script_env(script, budget, envs)
        .map_err(|err| IoError::other(format!("{label}: {err}")).into())
}

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

    let stdout = run_uia_script(
        "Windows UIAutomation capture failed",
        build_capture_script(),
        CAPTURE_BUDGET,
        &[
            ("WA_CAPTURE_MAX_DEPTH", &max_depth.to_string()),
            (
                "WA_CAPTURE_MAX_CHILDREN",
                &max_children_per_node.to_string(),
            ),
            (
                "WA_CAPTURE_PROCESS_ID",
                &process_id
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
            ),
            (
                "WA_CAPTURE_WINDOW_NAME_CONTAINS",
                window_name_contains.unwrap_or_default(),
            ),
        ],
    )?;

    let payload = parse_capture_payload(&stdout)?;
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

    let stdout = run_uia_script(
        "Windows UIAutomation action execution failed",
        build_action_script(),
        ACTION_BUDGET,
        &[
            (
                "WA_ACTION_PROCESS_ID",
                &process_id
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
            ),
            ("WA_ACTION_WINDOW_NAME_CONTAINS", &snapshot.title),
            ("WA_ACTION_NODE_ID", &plan.matched.id),
            ("WA_ACTION_NAME", action),
            ("WA_ACTION_VALUE", input_value.unwrap_or_default()),
        ],
    )?;

    let payload = parse_action_payload(&stdout)?;
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

    let resolve =
        crate::wa::resolve_selector(root, session_id, snapshot_name, node_id, role, name, None)?;
    let snapshot = crate::wa::load_snapshot(root, session_id, &resolve.snapshot_name)?;
    let process_id = snapshot
        .url
        .strip_prefix("windows://uia/process/")
        .and_then(|value| value.parse::<u32>().ok());

    // The wait script polls until its own WA_WAIT_TIMEOUT_MS deadline, so the
    // process budget has to sit above that or a healthy long wait gets killed.
    let wait_budget = Duration::from_millis(timeout_ms) + crate::wa::ps::SLACK;
    let node_id_text = resolve.matched.id.clone();
    let title_text = snapshot.title.clone();
    let process_id_text = process_id
        .map(|value| value.to_string())
        .unwrap_or_default();
    let timeout_text = timeout_ms.to_string();
    let poll_text = poll_interval_ms.to_string();
    let stdout = run_uia_script(
        "Windows UIAutomation wait failed",
        build_wait_script(),
        wait_budget,
        &[
            ("WA_WAIT_PROCESS_ID", &process_id_text),
            ("WA_WAIT_WINDOW_NAME_CONTAINS", &title_text),
            ("WA_WAIT_NODE_ID", &node_id_text),
            ("WA_WAIT_CONDITION", condition),
            ("WA_WAIT_EXPECTED_VALUE", expected_value.unwrap_or_default()),
            ("WA_WAIT_TIMEOUT_MS", &timeout_text),
            ("WA_WAIT_POLL_MS", &poll_text),
        ],
    )?;

    let payload = parse_wait_payload(&stdout)?;
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
