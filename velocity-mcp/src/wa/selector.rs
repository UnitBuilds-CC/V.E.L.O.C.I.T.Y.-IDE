//! CSS/XPath selector resolution for Windows Automation node trees.
//!
//! NOTE: The CSS/XPath parsing and scoring API is built out ahead of its
//! wiring into the WA action pipeline, so several parsers and helpers read as
//! dead until the resolver is invoked from tool dispatch.
#![allow(dead_code)] // selector resolver API awaiting WA-pipeline integration

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

// ── CSS Selector Engine ──────────────────────────────────────────────

/// Parsed CSS selector component.
#[derive(Debug, Clone)]
pub struct CssSelectorPart {
    pub role: Option<String>,
    pub name_contains: Option<String>,
    pub name_equals: Option<String>,
    pub id: Option<String>,
    pub visible: Option<bool>,
    pub enabled: Option<bool>,
    pub action: Option<String>,
}

/// Parse a CSS-like selector string into selector parts.
/// Supports: `[role=X]`, `[name=Y]`, `[name*="Z"]`, `#id`, `[visible]`, `[enabled]`, `[action=click]`
pub fn parse_css_selector(selector: &str) -> Vec<CssSelectorPart> {
    let mut parts = Vec::new();
    let mut current = CssSelectorPart {
        role: None, name_contains: None, name_equals: None,
        id: None, visible: None, enabled: None, action: None,
    };
    let mut has_any = false;
    let chars: Vec<char> = selector.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '#' => {
                // ID selector: #someId
                i += 1;
                let start = i;
                while i < chars.len() && chars[i] != '[' && chars[i] != '.' && chars[i] != ' ' {
                    i += 1;
                }
                current.id = Some(selector[start..i].to_string());
                has_any = true;
            }
            '[' => {
                // Attribute selector: [attr=value] or [attr*="value"]
                i += 1;
                let start = i;
                while i < chars.len() && chars[i] != ']' {
                    i += 1;
                }
                let attr_str = &selector[start..i];
                if i < chars.len() { i += 1; } // skip ']'
                parse_attribute_selector(attr_str, &mut current);
                has_any = true;
            }
            '.' => {
                // Class-like selector (mapped to action)
                i += 1;
                let start = i;
                while i < chars.len() && chars[i] != '[' && chars[i] != '.' && chars[i] != ' ' {
                    i += 1;
                }
                current.action = Some(selector[start..i].to_string());
                has_any = true;
            }
            ' ' => {
                // Combinator: push current and start new part
                if has_any {
                    parts.push(std::mem::replace(&mut current, CssSelectorPart {
                        role: None, name_contains: None, name_equals: None,
                        id: None, visible: None, enabled: None, action: None,
                    }));
                    has_any = false;
                }
                i += 1;
            }
            _ => { i += 1; }
        }
    }
    if has_any {
        parts.push(current);
    }
    if parts.is_empty() {
        parts.push(CssSelectorPart {
            role: None, name_contains: None, name_equals: None,
            id: None, visible: None, enabled: None, action: None,
        });
    }
    parts
}

fn parse_attribute_selector(attr: &str, part: &mut CssSelectorPart) {
    if let Some(eq_pos) = attr.find('*') {
        // [name*="value"] — contains
        let key = attr[..eq_pos].trim();
        let val = attr[eq_pos+1..].trim().trim_matches('"').trim_matches('\'');
        match key {
            "name" => part.name_contains = Some(val.to_string()),
            "role" => part.role = Some(val.to_string()),
            _ => {}
        }
    } else if let Some(eq_pos) = attr.find('=') {
        let key = attr[..eq_pos].trim();
        let val = attr[eq_pos+1..].trim().trim_matches('"').trim_matches('\'');
        match key {
            "role" => part.role = Some(val.to_string()),
            "name" => part.name_equals = Some(val.to_string()),
            "id" => part.id = Some(val.to_string()),
            "action" => part.action = Some(val.to_string()),
            "visible" => part.visible = Some(val == "true"),
            "enabled" => part.enabled = Some(val == "true"),
            _ => {}
        }
    } else {
        // Boolean attribute: [visible], [enabled]
        match attr.trim() {
            "visible" => part.visible = Some(true),
            "enabled" => part.enabled = Some(true),
            _ => {}
        }
    }
}

/// Score a node against a CSS selector part.
fn score_css(node: &WaNode, part: &CssSelectorPart) -> Option<i32> {
    let mut score = 0i32;
    if let Some(ref expected_id) = part.id {
        if !node.id.eq_ignore_ascii_case(expected_id) { return None; }
        score += 10_000;
    }
    if let Some(ref expected_role) = part.role {
        if !node.role.eq_ignore_ascii_case(expected_role) { return None; }
        score += 500;
    }
    if let Some(ref expected_name) = part.name_equals {
        if node.name.eq_ignore_ascii_case(expected_name) {
            score += 250;
        } else { return None; }
    }
    if let Some(ref needle) = part.name_contains {
        if contains_case_insensitive(&node.name, needle) {
            score += 100;
        } else { return None; }
    }
    if let Some(expected_action) = &part.action {
        if !action_supported(node, expected_action) { return None; }
        score += 400;
    }
    if let Some(vis) = part.visible {
        if vis && !node.visible { return None; }
        if !vis && node.visible { return None; }
        score += 50;
    }
    if let Some(en) = part.enabled {
        if en && !node.enabled { return None; }
        if !en && node.enabled { return None; }
        score += 50;
    }
    score += (node.confidence.clamp(0.0, 1.0) * 100.0).round() as i32;
    Some(score)
}

/// Resolve nodes using a CSS selector string.
pub fn resolve_css_selector(
    root: &Path,
    session_id: &str,
    snapshot_name: Option<&str>,
    css_selector: &str,
) -> Result<Vec<(i32, WaNode)>, Box<dyn Error>> {
    let resolved_snapshot_name = resolve_snapshot_name(root, session_id, snapshot_name)?;
    let snapshot = crate::wa::storage::load_snapshot(root, session_id, &resolved_snapshot_name)?;
    let parts = parse_css_selector(css_selector);
    // Match against the last part (descendant combinator simplified)
    let part = parts.last().unwrap();
    let mut candidates: Vec<(i32, WaNode)> = snapshot.nodes.iter()
        .filter_map(|node| score_css(node, part).map(|s| (s, node.clone())))
        .collect();
    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(candidates)
}

// ── XPath Engine ─────────────────────────────────────────────────────

/// Simple XPath expression.
#[derive(Debug, Clone)]
pub enum XPathExpr {
    /// //node — match all nodes
    DescendantAll,
    /// //role[@name='X'] — match by role and optional attribute
    DescendantByRole { role: String, name_filter: Option<String> },
    /// //*[@id='X'] — match by id
    DescendantById(String),
    /// //*[contains(@name, 'X')] — contains match
    DescendantContains { attr: String, value: String },
}

/// Parse a simple XPath expression.
pub fn parse_xpath(xpath: &str) -> Option<XPathExpr> {
    let xpath = xpath.trim();
    if xpath == "//*" || xpath == "//node" {
        return Some(XPathExpr::DescendantAll);
    }
    // //*[@id='X']
    if let Some(rest) = xpath.strip_prefix("//*[@id='") {
        if let Some(id) = rest.strip_suffix("']") {
            return Some(XPathExpr::DescendantById(id.to_string()));
        }
    }
    if let Some(rest) = xpath.strip_prefix("//*[contains(@") {
        // //*[contains(@name, 'value')]
        if let Some(at_pos) = rest.find(',') {
            let attr = &rest[..at_pos];
            let remainder = &rest[at_pos+1..];
            if let Some(end) = remainder.find("')]") {
                let value_clean = remainder[..end].trim().trim_matches(' ').trim_matches('\'');
                return Some(XPathExpr::DescendantContains {
                    attr: attr.to_string(),
                    value: value_clean.to_string(),
                });
            }
        }
    }
    // //role[@name='X']
    if let Some(rest) = xpath.strip_prefix("//") {
        if let Some(bracket_pos) = rest.find('[') {
            let role = &rest[..bracket_pos];
            let attr_part = &rest[bracket_pos..];
            if let Some(name_val) = extract_attr_value(attr_part, "name") {
                return Some(XPathExpr::DescendantByRole {
                    role: role.to_string(),
                    name_filter: Some(name_val),
                });
            }
            return Some(XPathExpr::DescendantByRole {
                role: role.to_string(),
                name_filter: None,
            });
        } else {
            return Some(XPathExpr::DescendantByRole {
                role: rest.to_string(),
                name_filter: None,
            });
        }
    }
    None
}

fn extract_attr_value(attr_str: &str, attr_name: &str) -> Option<String> {
    let pattern = format!("@{}='", attr_name);
    if let Some(start) = attr_str.find(&pattern) {
        let val_start = start + pattern.len();
        if let Some(end) = attr_str[val_start..].find('\'') {
            return Some(attr_str[val_start..val_start + end].to_string());
        }
    }
    None
}

/// Resolve nodes using an XPath expression.
pub fn resolve_xpath(
    root: &Path,
    session_id: &str,
    snapshot_name: Option<&str>,
    xpath: &str,
) -> Result<Vec<(i32, WaNode)>, Box<dyn Error>> {
    let resolved_snapshot_name = resolve_snapshot_name(root, session_id, snapshot_name)?;
    let snapshot = crate::wa::storage::load_snapshot(root, session_id, &resolved_snapshot_name)?;
    let expr = parse_xpath(xpath).ok_or_else(|| {
        IoError::new(ErrorKind::InvalidInput, format!("unsupported XPath: {}", xpath))
    })?;
    let mut candidates: Vec<(i32, WaNode)> = match expr {
        XPathExpr::DescendantAll => {
            snapshot.nodes.iter().map(|n| (100i32 + (n.confidence * 100.0) as i32, n.clone())).collect()
        }
        XPathExpr::DescendantById(ref id) => {
            snapshot.nodes.iter()
                .filter(|n| n.id.eq_ignore_ascii_case(id))
                .map(|n| (10_000i32 + (n.confidence * 100.0) as i32, n.clone()))
                .collect()
        }
        XPathExpr::DescendantByRole { ref role, ref name_filter } => {
            snapshot.nodes.iter()
                .filter(|n| {
                    n.role.eq_ignore_ascii_case(role)
                        && name_filter.as_ref().is_none_or(|nf| contains_case_insensitive(&n.name, nf))
                })
                .map(|n| (500i32 + (n.confidence * 100.0) as i32, n.clone()))
                .collect()
        }
        XPathExpr::DescendantContains { ref attr, ref value } => {
            snapshot.nodes.iter()
                .filter(|n| {
                    match attr.as_str() {
                        "name" => contains_case_insensitive(&n.name, value),
                        "role" => contains_case_insensitive(&n.role, value),
                        "id" => contains_case_insensitive(&n.id, value),
                        _ => false,
                    }
                })
                .map(|n| (200i32 + (n.confidence * 100.0) as i32, n.clone()))
                .collect()
        }
    };
    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(candidates)
}

// ── Original API ─────────────────────────────────────────────────────

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
