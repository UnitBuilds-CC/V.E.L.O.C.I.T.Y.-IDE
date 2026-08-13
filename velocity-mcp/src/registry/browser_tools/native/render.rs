//! Report shapes and render helpers shared by the native browser tools.
//!
//! Every tool output is either one of these serde reports (compact mode) or
//! a readable rendering produced here, so acting and observing stay formatted
//! by one code path.

use serde::Serialize;
use serde_json::Value;

use crate::editor::browser::native_bridge::{NativeBrowserBridge, NativeBrowserView};
use velocity_browser::NdaDelta;

#[derive(Serialize)]
pub(super) struct ElementReport {
    node_id: usize,
    aom_id: String,
    role: String,
    name: String,
    value: String,
    actionability: u8,
    focused: bool,
    expanded: bool,
}

#[derive(Serialize)]
pub(super) struct ViewReport {
    url: String,
    title: String,
    element_count: usize,
    elements: Vec<ElementReport>,
}

#[derive(Serialize)]
pub(super) struct FactReport {
    subject: String,
    predicate: String,
    object: String,
}

#[derive(Serialize)]
pub(super) struct ChangeReport {
    subject: String,
    predicate: String,
    old: String,
    new: String,
}

#[derive(Serialize)]
pub(super) struct DeltaReport {
    added: Vec<FactReport>,
    removed: Vec<FactReport>,
    changed: Vec<ChangeReport>,
}

#[derive(Serialize)]
pub(super) struct ActionReport {
    pub(super) status: String,
    pub(super) delta: DeltaReport,
    pub(super) view: ViewReport,
    /// `(chars_before, chars_after)` of the distilled content fact — present
    /// only when the action actually changed the page's readable core.
    #[serde(rename = "contentChange", skip_serializing_if = "Option::is_none")]
    pub(super) content_change: Option<(usize, usize)>,
}

pub(super) fn predicate_name(p: u16) -> String {
    // Delegate to the engine's canonical registry so tool-side names never
    // drift from the predicate ids they label.
    velocity_browser::predicates::predicate_name(p).to_string()
}

pub(super) fn view_report(view: &NativeBrowserView) -> ViewReport {
    ViewReport {
        url: view.url.clone(),
        title: view.title.clone(),
        element_count: view.elements.len(),
        elements: view
            .elements
            .iter()
            .map(|e| ElementReport {
                node_id: e.node_id,
                aom_id: e.aom_id.clone(),
                role: e.role.clone(),
                name: e.name.clone(),
                value: e.value.clone(),
                actionability: e.actionability,
                focused: e.is_focused,
                expanded: e.is_expanded,
            })
            .collect(),
    }
}

pub(super) fn delta_report(delta: &NdaDelta) -> DeltaReport {
    DeltaReport {
        added: delta
            .added
            .iter()
            .map(|(s, p, o)| FactReport {
                subject: s.clone(),
                predicate: predicate_name(*p),
                object: fact_snippet(o),
            })
            .collect(),
        removed: delta
            .removed
            .iter()
            .map(|(s, p, o)| FactReport {
                subject: s.clone(),
                predicate: predicate_name(*p),
                object: fact_snippet(o),
            })
            .collect(),
        changed: delta
            .changed
            .iter()
            .map(|c| ChangeReport {
                subject: c.subject.clone(),
                predicate: predicate_name(c.predicate),
                old: fact_snippet(&c.old),
                new: fact_snippet(&c.new),
            })
            .collect(),
    }
}

pub(super) fn render_view(view: &NativeBrowserView) -> String {
    let mut out = String::new();
    out.push_str(&format!("URL: {}\nTitle: {}\n", view.url, view.title));
    out.push_str(&format!("Actionable elements: {}\n", view.elements.len()));
    for e in &view.elements {
        out.push_str(&format!(
            "  [{}] {} \"{}\"{}{} (act {})\n",
            e.node_id,
            e.role,
            e.name,
            if e.value.is_empty() {
                String::new()
            } else {
                format!(" value=\"{}\"", e.value)
            },
            if e.is_focused { " *focused*" } else { "" },
            e.actionability,
        ));
    }
    out
}

/// Diff lines are summaries, not state dumps: long fact values (like the
/// distilled 8000-char content fact) collapse to a snippet so a content
/// change reports its size instead of flooding the output.
pub(super) fn fact_snippet(value: &str) -> String {
    const LIMIT: usize = 160;
    // Diff lines must stay one line: collapse newlines from markdown-shaped
    // values before truncating.
    let flat = if value.contains('\n') {
        value.split_whitespace().collect::<Vec<_>>().join(" ")
    } else {
        value.to_string()
    };
    let count = flat.chars().count();
    if count <= LIMIT {
        return flat;
    }
    let mut snippet: String = flat.chars().take(LIMIT).collect();
    snippet.push_str(&format!(" \u{2026}(+{} chars)", count - LIMIT));
    snippet
}

/// Keep at most this many lines per predicate before folding the rest.
const DELTA_LINES_PER_PREDICATE: usize = 4;

/// Fold a list of rendered diff lines per predicate: the first few lines stay
/// explicit, the remainder collapses into one "(N more ...)" tail line.
pub(super) fn fold_by_predicate(mut lines: Vec<(u16, String)>) -> Vec<String> {
    use std::collections::HashMap;
    let mut grouped: HashMap<u16, Vec<String>> = HashMap::new();
    let mut order: Vec<u16> = Vec::new();
    for (p, text) in lines.drain(..) {
        if !grouped.contains_key(&p) {
            order.push(p);
        }
        grouped.entry(p).or_default().push(text);
    }
    let mut out = Vec::new();
    for p in order {
        let texts = grouped.remove(&p).unwrap_or_default();
        let total = texts.len();
        let keep = total.min(DELTA_LINES_PER_PREDICATE);
        out.extend(texts.iter().take(keep).cloned());
        if total > keep {
            out.push(format!(
                "    ({} more {} change(s))",
                total - keep,
                predicate_name(p)
            ));
        }
    }
    out
}

/// Distilled-content change as `(chars_before, chars_after)` — the one signal
/// that tells an agent whether re-reading the page after an action is worth
/// a page_text call.
pub(super) fn content_change_signal(delta: &NdaDelta) -> Option<(usize, usize)> {
    use velocity_browser::predicates::SESSION_CONTENT;
    if let Some(c) = delta
        .changed
        .iter()
        .find(|c| c.predicate == SESSION_CONTENT)
    {
        return Some((c.old.chars().count(), c.new.chars().count()));
    }
    let added = delta
        .added
        .iter()
        .find(|(_, p, _)| *p == SESSION_CONTENT)
        .map(|(_, _, o)| o.chars().count());
    let removed = delta
        .removed
        .iter()
        .find(|(_, p, _)| *p == SESSION_CONTENT)
        .map(|(_, _, o)| o.chars().count());
    match (removed, added) {
        (Some(a), Some(b)) => Some((a, b)),
        (Some(a), None) => Some((a, 0)),
        (None, Some(b)) => Some((0, b)),
        (None, None) => None,
    }
}

/// Readable one-liner for [`content_change_signal`]; empty when unchanged.
pub(super) fn content_change_note(delta: &NdaDelta) -> String {
    match content_change_signal(delta) {
        Some((from, to)) => format!("Content changed: {from} -> {to} chars\n"),
        None => String::new(),
    }
}

pub(super) fn render_delta(delta: &NdaDelta) -> String {
    if delta.is_empty() {
        return "  (no state change)\n".to_string();
    }
    let mut out = String::new();
    let added: Vec<(u16, String)> = delta
        .added
        .iter()
        .map(|(s, p, o)| {
            (
                *p,
                format!("  + {} {} = {}", s, predicate_name(*p), fact_snippet(o)),
            )
        })
        .collect();
    for line in fold_by_predicate(added) {
        out.push_str(&line);
        out.push('\n');
    }
    let removed: Vec<(u16, String)> = delta
        .removed
        .iter()
        .map(|(s, p, o)| {
            (
                *p,
                format!("  - {} {} = {}", s, predicate_name(*p), fact_snippet(o)),
            )
        })
        .collect();
    for line in fold_by_predicate(removed) {
        out.push_str(&line);
        out.push('\n');
    }
    let changed: Vec<(u16, String)> = delta
        .changed
        .iter()
        .map(|c| {
            (
                c.predicate,
                format!(
                    "  ~ {} {} : {} -> {}",
                    c.subject,
                    predicate_name(c.predicate),
                    fact_snippet(&c.old),
                    fact_snippet(&c.new)
                ),
            )
        })
        .collect();
    for line in fold_by_predicate(changed) {
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// Readable one-line-per-tab listing; the active tab is starred.
pub(super) fn tab_lines(bridge: &NativeBrowserBridge) -> String {
    let tabs = bridge.tab_list();
    let mut out = format!("Tabs ({}):\n", tabs.len());
    for (id, url, title, active) in &tabs {
        out.push_str(&format!(
            "  {}{} \"{}\" {}\n",
            if *active { "* " } else { "  " },
            id,
            title,
            if url.is_empty() { "(blank)" } else { url },
        ));
    }
    out
}

pub(super) fn tab_json(bridge: &NativeBrowserBridge) -> Value {
    Value::Array(
        bridge
            .tab_list()
            .into_iter()
            .map(|(id, url, title, active)| {
                serde_json::json!({ "tabId": id, "url": url, "title": title, "active": active })
            })
            .collect(),
    )
}

/// First 160 characters of a stored page text, ellipsised, so recall
/// listings stay token-cheap even when whole articles were indexed.
pub(super) fn memory_snippet(text: &str) -> String {
    let mut s: String = text.chars().take(160).collect();
    if text.chars().count() > 160 {
        s.push('\u{2026}');
    }
    s
}
