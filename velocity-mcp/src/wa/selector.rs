use std::error::Error;
use std::io::{Error as IoError, ErrorKind};
use std::path::Path;

use crate::wa::model::{
    WaNode, WaPlanActionReport, WaResolveSelectorReport, WaScriptStep,
};

fn contains_case_insensitive(haystack: &str, needle: &str) -> bool {
    haystack
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

fn action_supported(node: &WaNode, action: &str) -> bool {
    node.actions
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(action))
}

fn score_node(
    node: &WaNode,
    node_id: Option<&str>,
    role: Option<&str>,
    name: Option<&str>,
    action: Option<&str>,
) -> Option<i32> {
    let mut score = 0i32;
    if let Some(expected) = node_id {
        if !node.id.eq_ignore_ascii_case(expected) {
            return None;
        }
        score += 10_000;
    }
    if let Some(expected_role) = role {
        if !node.role.eq_ignore_ascii_case(expected_role) {
            return None;
        }
        score += 500;
    }
    if let Some(expected_name) = name {
        if node.name.eq_ignore_ascii_case(expected_name) {
            score += 250;
        } else if contains_case_insensitive(&node.name, expected_name) {
            score += 100;
        } else {
            return None;
        }
    }
    if let Some(expected_action) = action {
        if !action_supported(node, expected_action) {
            return None;
        }
        score += 400;
    }
    if node.visible {
        score += 50;
    }
    if node.enabled {
        score += 50;
    }
    score += (node.confidence.clamp(0.0, 1.0) * 100.0).round() as i32;
    Some(score)
}

fn resolve_snapshot_name(
    root: &Path,
    session_id: &str,
    snapshot_name: Option<&str>,
) -> Result<String, Box<dyn Error>> {
    if let Some(snapshot_name) = snapshot_name {
        return Ok(snapshot_name.to_string());
    }
    let session = crate::wa::storage::load_session(root, session_id)?;
    session.latest_snapshot_name.ok_or_else(|| {
        IoError::new(
            ErrorKind::NotFound,
            format!("session '{session_id}' has no saved WA snapshot"),
        )
        .into()
    })
}

pub fn resolve_selector(
    root: &Path,
    session_id: &str,
    snapshot_name: Option<&str>,
    node_id: Option<&str>,
    role: Option<&str>,
    name: Option<&str>,
    action: Option<&str>,
) -> Result<WaResolveSelectorReport, Box<dyn Error>> {
    let resolved_snapshot_name = resolve_snapshot_name(root, session_id, snapshot_name)?;
    let snapshot = crate::wa::storage::load_snapshot(root, session_id, &resolved_snapshot_name)?;
    let mut candidates = snapshot
        .nodes
        .iter()
        .filter_map(|node| {
            score_node(node, node_id, role, name, action).map(|score| (score, node.clone()))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then(left.1.id.cmp(&right.1.id))
    });
    let (_, matched) = candidates.first().cloned().ok_or_else(|| {
        IoError::new(
            ErrorKind::NotFound,
            format!(
                "no WA node matched selector for session '{session_id}' snapshot '{}'",
                resolved_snapshot_name
            ),
        )
    })?;
    let read_report = crate::wa::storage::read_snapshot_report(root, session_id, &resolved_snapshot_name)?;
    Ok(WaResolveSelectorReport {
        session_id: session_id.to_string(),
        snapshot_name: resolved_snapshot_name,
        action: action.map(|value| value.to_string()),
        selector: WaScriptStep {
            action: action.unwrap_or("inspect").to_string(),
            node_id: node_id.map(|value| value.to_string()),
            role: role.map(|value| value.to_string()),
            name: name.map(|value| value.to_string()),
            value: None,
            required: true,
        },
        matched,
        candidate_count: candidates.len(),
        snapshot_nda_path: read_report.snapshot_nda_path,
    })
}

pub fn plan_action(
    root: &Path,
    session_id: &str,
    snapshot_name: Option<&str>,
    action: &str,
    node_id: Option<&str>,
    role: Option<&str>,
    name: Option<&str>,
    input_value: Option<&str>,
) -> Result<WaPlanActionReport, Box<dyn Error>> {
    let resolve = resolve_selector(root, session_id, snapshot_name, node_id, role, name, Some(action))?;
    let mut preconditions = Vec::new();
    if resolve.matched.visible {
        preconditions.push("visible".to_string());
    }
    if resolve.matched.enabled {
        preconditions.push("enabled".to_string());
    }
    if action_supported(&resolve.matched, action) {
        preconditions.push(format!("supports:{action}"));
    }
    if let Some(value) = input_value {
        preconditions.push(format!("input-bytes:{}", value.len()));
    }
    let planned_step = WaScriptStep {
        action: action.to_string(),
        node_id: Some(resolve.matched.id.clone()),
        role: Some(resolve.matched.role.clone()),
        name: Some(resolve.matched.name.clone()),
        value: input_value.map(|value| value.to_string()),
        required: true,
    };
    Ok(WaPlanActionReport {
        session_id: resolve.session_id,
        snapshot_name: resolve.snapshot_name,
        action: action.to_string(),
        input_value: input_value.map(|value| value.to_string()),
        selector: resolve.selector,
        matched: resolve.matched,
        preconditions,
        planned_step,
        snapshot_nda_path: resolve.snapshot_nda_path,
    })
}

pub fn render_resolve_selector_report(report: &WaResolveSelectorReport) -> String {
    format!(
        "Resolved WA selector in session '{}' snapshot '{}'.\nMatched node: {} [{}] '{}'\nCandidates: {}\nSnapshot NDA: {}",
        report.session_id,
        report.snapshot_name,
        report.matched.id,
        report.matched.role,
        report.matched.name,
        report.candidate_count,
        report.snapshot_nda_path,
    )
}

pub fn render_plan_action_report(report: &WaPlanActionReport) -> String {
    let value_line = report
        .input_value
        .as_deref()
        .map(|value| format!("\nInput value: {}", value))
        .unwrap_or_default();
    format!(
        "Planned WA action '{}' in session '{}' snapshot '{}'.\nTarget node: {} [{}] '{}'\nPreconditions: {}{}\nPlanned script step: {}\nSnapshot NDA: {}",
        report.action,
        report.session_id,
        report.snapshot_name,
        report.matched.id,
        report.matched.role,
        report.matched.name,
        report.preconditions.join(", "),
        value_line,
        serde_json::to_string(&report.planned_step).unwrap_or_else(|_| "{}".to_string()),
        report.snapshot_nda_path,
    )
}
