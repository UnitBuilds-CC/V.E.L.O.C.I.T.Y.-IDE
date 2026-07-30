//! Native-engine browser tools.
//!
//! Unlike the static-crawl and runtime-capture tool families (which fetch a
//! page and hand back a metadata snapshot), this family drives the pure-Rust
//! [`velocity_browser::BrowserSession`] directly: it navigates over rustls
//! HTTPS, exposes the *live* Agentic Object Model, and lets the agent act on
//! elements by node id or by role + accessible name. Every action returns the
//! readable NDA delta it produced alongside the refreshed page view, so acting
//! and observing are inseparable.

use serde::Serialize;
use serde_json::Value;
use std::error::Error;
use std::path::Path;

use crate::editor::browser::native_bridge::{
    encode_nda_triples, get_or_create_native_bridge, persist_browser_artifact,
    NativeBrowserBridge, NativeBrowserView,
};
use velocity_browser::NdaDelta;

#[derive(Serialize)]
struct ElementReport {
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
struct ViewReport {
    url: String,
    title: String,
    element_count: usize,
    elements: Vec<ElementReport>,
}

#[derive(Serialize)]
struct FactReport {
    subject: String,
    predicate: String,
    object: String,
}

#[derive(Serialize)]
struct ChangeReport {
    subject: String,
    predicate: String,
    old: String,
    new: String,
}

#[derive(Serialize)]
struct DeltaReport {
    added: Vec<FactReport>,
    removed: Vec<FactReport>,
    changed: Vec<ChangeReport>,
}

#[derive(Serialize)]
struct ActionReport {
    status: String,
    delta: DeltaReport,
    view: ViewReport,
}

fn predicate_name(p: u16) -> String {
    use velocity_browser::predicates::*;
    let s = match p {
        AOM_ROLE => "role",
        AOM_NAME => "name",
        AOM_VALUE => "value",
        AOM_ACTIONABILITY => "actionability",
        AOM_FOCUSED => "focused",
        AOM_EXPANDED => "expanded",
        LAYOUT_BOUNDS => "bounds",
        LAYOUT_VISIBILITY => "visibility",
        LAYOUT_DISPLAY => "display",
        LAYOUT_IN_VIEWPORT => "inViewport",
        SESSION_URL => "url",
        SESSION_TITLE => "title",
        SESSION_COOKIE => "cookie",
        SESSION_STORAGE => "storage",
        SESSION_SCROLL => "scroll",
        SESSION_LINK_COUNT => "links",
        SESSION_FORM_COUNT => "forms",
        SESSION_INTERACTIVE_COUNT => "interactive",
        SESSION_TEXT_LENGTH => "textLength",
        SESSION_HEADING => "heading",
        other => return format!("predicate_{other}"),
    };
    s.to_string()
}

fn view_report(view: &NativeBrowserView) -> ViewReport {
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

fn delta_report(delta: &NdaDelta) -> DeltaReport {
    DeltaReport {
        added: delta
            .added
            .iter()
            .map(|(s, p, o)| FactReport {
                subject: s.clone(),
                predicate: predicate_name(*p),
                object: o.clone(),
            })
            .collect(),
        removed: delta
            .removed
            .iter()
            .map(|(s, p, o)| FactReport {
                subject: s.clone(),
                predicate: predicate_name(*p),
                object: o.clone(),
            })
            .collect(),
        changed: delta
            .changed
            .iter()
            .map(|c| ChangeReport {
                subject: c.subject.clone(),
                predicate: predicate_name(c.predicate),
                old: c.old.clone(),
                new: c.new.clone(),
            })
            .collect(),
    }
}

fn render_view(view: &NativeBrowserView) -> String {
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

fn render_delta(delta: &NdaDelta) -> String {
    if delta.is_empty() {
        return "  (no state change)\n".to_string();
    }
    let mut out = String::new();
    for (s, p, o) in &delta.added {
        out.push_str(&format!("  + {} {} = {}\n", s, predicate_name(*p), o));
    }
    for (s, p, o) in &delta.removed {
        out.push_str(&format!("  - {} {} = {}\n", s, predicate_name(*p), o));
    }
    for c in &delta.changed {
        out.push_str(&format!(
            "  ~ {} {} : {} -> {}\n",
            c.subject,
            predicate_name(c.predicate),
            c.old,
            c.new
        ));
    }
    out
}

/// Readable one-line-per-tab listing; the active tab is starred.
fn tab_lines(bridge: &NativeBrowserBridge) -> String {
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

fn tab_json(bridge: &NativeBrowserBridge) -> Value {
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
fn memory_snippet(text: &str) -> String {
    let mut s: String = text.chars().take(160).collect();
    if text.chars().count() > 160 {
        s.push('…');
    }
    s
}

/// Resolve the target node id from either an explicit `nodeId` (accepts a raw
/// integer, `"5"`, or `"node_5"`) or a semantic `role` + `name` lookup.
fn resolve_node(bridge: &NativeBrowserBridge, arguments: &Value) -> Result<usize, Box<dyn Error>> {
    if let Some(n) = arguments.get("nodeId") {
        if let Some(u) = n.as_u64() {
            return Ok(u as usize);
        }
        if let Some(s) = n.as_str() {
            let trimmed = s.strip_prefix("node_").unwrap_or(s);
            if let Ok(u) = trimmed.parse::<usize>() {
                return Ok(u);
            }
        }
    }
    let name = arguments["name"]
        .as_str()
        .ok_or("either nodeId or name is required")?;
    let role = arguments["role"].as_str();
    bridge
        .resolve_target(role, name)
        .ok_or_else(|| format!("no element matched role={role:?} name={name:?}").into())
}

pub fn handle_native_tool(
    root: &Path,
    name: &str,
    arguments: &Value,
) -> Result<Option<String>, Box<dyn Error>> {
    let session_id = match name {
        "browser_native_navigate"
        | "browser_native_read"
        | "browser_native_click"
        | "browser_native_type"
        | "browser_native_select"
        | "browser_native_submit"
        | "browser_native_scroll"
        | "browser_native_scroll_into_view"
        | "browser_native_back"
        | "browser_native_forward"
        | "browser_native_eval"
        | "browser_native_wait_for"
        | "browser_native_extract"
        | "browser_native_cookies"
        | "browser_native_storage"
        | "browser_native_network"
        | "browser_native_screenshot"
        | "browser_native_hover"
        | "browser_native_press_key"
        | "browser_native_click_text"
        | "browser_native_fill_label"
        | "browser_native_check_label"
        | "browser_native_select_label"
        | "browser_native_focus_label"
        | "browser_native_press"
        | "browser_native_read_form"
        | "browser_native_observe"
        | "browser_native_export_nda"
        | "browser_native_remember"
        | "browser_native_recall"
        | "browser_native_page_text"
        | "browser_native_screencast"
        | "browser_native_find"
        | "browser_native_validate"
        | "browser_native_links"
        | "browser_native_history"
        | "browser_native_checkpoint"
        | "browser_native_tab_open"
        | "browser_native_tab_list"
        | "browser_native_tab_switch"
        | "browser_native_tab_close"
        | "browser_native_settle" => arguments["sessionId"]
            .as_str()
            .ok_or("sessionId is required")?,
        _ => return Ok(None),
    };
    let compact = arguments["compact"].as_bool().unwrap_or(false);
    let bridge = get_or_create_native_bridge(session_id);
    let mut bridge = bridge
        .lock()
        .map_err(|_| "native browser bridge lock poisoned")?;

    // Read is view-only; everything else is an action producing a delta.
    if name == "browser_native_read" {
        let view = bridge.current_view();
        return Ok(Some(if compact {
            serde_json::to_string_pretty(&view_report(&view))
                .map_err(|e| format!("serialise native view: {e}"))?
        } else {
            render_view(&view)
        }));
    }

    // Form summary and full fact dump are view-only readable text.
    if name == "browser_native_read_form" {
        let form = bridge.agent_read_form();
        return Ok(Some(if form.is_empty() {
            "(no form controls on page)".to_string()
        } else {
            form
        }));
    }

    if name == "browser_native_observe" {
        return Ok(Some(bridge.agent_observe()));
    }

    // The token-cheapest full read: title + visible body text, whitespace
    // collapsed, scripts/styles skipped. maxChars keeps huge pages bounded.
    if name == "browser_native_page_text" {
        let text = bridge.page_text();
        if text.is_empty() {
            return Ok(Some("(no visible text on page)".to_string()));
        }
        let max_chars = arguments["maxChars"].as_u64().unwrap_or(0) as usize;
        if max_chars > 0 && text.chars().count() > max_chars {
            let truncated: String = text.chars().take(max_chars).collect();
            return Ok(Some(format!(
                "{truncated}…\n(truncated to {max_chars} of {} chars)",
                text.chars().count()
            )));
        }
        return Ok(Some(text));
    }

    // Structural screencast: frames record the page's shape (viewport, AOM
    // element count, content hash) instead of pixels — a diffable timeline of
    // how the page evolved across the agent's actions.
    if name == "browser_native_screencast" {
        let action = arguments["action"].as_str().unwrap_or("capture");
        return Ok(Some(match action {
            "capture" => {
                let (idx, elements, hash) = bridge.screencast_capture();
                let total = bridge.screencast_frames().len();
                format!(
                    "captured frame {idx} ({elements} elements, hash {hash:016x}) — {total} frame{} in timeline\n",
                    if total == 1 { "" } else { "s" },
                )
            }
            "list" => {
                let frames = bridge.screencast_frames();
                if frames.is_empty() {
                    "(no frames captured)".to_string()
                } else {
                    let mut out = format!(
                        "{} frame{} in timeline:\n",
                        frames.len(),
                        if frames.len() == 1 { "" } else { "s" },
                    );
                    for f in frames {
                        out.push_str(&format!(
                            "  frame {}: {}x{}, {} elements, hash {:016x}, t={}ms\n",
                            f.frame_idx, f.width, f.height, f.element_count, f.frame_hash,
                            f.timestamp_ms,
                        ));
                    }
                    out
                }
            }
            "save" => {
                let path = bridge.screencast_save(root)?;
                format!(
                    "saved {} frame(s) to {}\n",
                    bridge.screencast_frames().len(),
                    path.display()
                )
            }
            other => {
                return Err(format!(
                    "unknown screencast action '{other}' (expected capture, list, or save)"
                )
                .into())
            }
        }));
    }

    // Query the live AOM by role and/or text instead of dumping the whole
    // element view — targeted reads keep big pages token-cheap.
    if name == "browser_native_find" {
        let role = arguments["role"].as_str();
        let text = arguments["text"].as_str().unwrap_or("");
        if role.is_none() && text.is_empty() {
            return Err("at least one of role or text is required".into());
        }
        let limit = arguments["limit"].as_u64().unwrap_or(20) as usize;
        let (mut hits, total) = bridge.find_elements(role, text);
        let matched = hits.len();
        hits.truncate(limit);
        return Ok(Some(if compact {
            let items: Vec<serde_json::Value> = hits
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "nodeId": e.node_id,
                        "role": e.role,
                        "name": e.name,
                        "value": e.value,
                        "actionability": e.actionability,
                        "focused": e.is_focused,
                    })
                })
                .collect();
            serde_json::to_string_pretty(&serde_json::json!({
                "matched": matched,
                "total": total,
                "hits": items,
            }))
            .map_err(|e| format!("serialise find report: {e}"))?
        } else if hits.is_empty() {
            format!(
                "no elements matched role={} text=\"{}\" ({} elements on page)\n",
                role.unwrap_or("*"),
                text,
                total
            )
        } else {
            let mut out = format!(
                "{matched} of {total} elements matched role={} text=\"{}\":\n",
                role.unwrap_or("*"),
                text
            );
            for e in &hits {
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
            if matched > hits.len() {
                out.push_str(&format!("  … {} more (raise limit)\n", matched - hits.len()));
            }
            out
        }));
    }

    // The page's navigation map: every link's text and target in document
    // order — the AOM view names links but never shows their hrefs.
    if name == "browser_native_links" {
        let filter = arguments["filter"].as_str().unwrap_or("");
        let limit = arguments["limit"].as_u64().unwrap_or(50) as usize;
        let mut links = bridge.links(filter);
        let matched = links.len();
        links.truncate(limit);
        return Ok(Some(if compact {
            let items: Vec<serde_json::Value> = links
                .iter()
                .map(|(id, text, href)| {
                    serde_json::json!({ "nodeId": id, "text": text, "href": href })
                })
                .collect();
            serde_json::to_string_pretty(&serde_json::json!({
                "matched": matched,
                "filter": filter,
                "links": items,
            }))
            .map_err(|e| format!("serialise links report: {e}"))?
        } else if links.is_empty() {
            if filter.is_empty() {
                "(no links on page)".to_string()
            } else {
                format!("no links matched \"{filter}\"\n")
            }
        } else {
            let mut out = format!(
                "{matched} link{}{}:\n",
                if matched == 1 { "" } else { "s" },
                if filter.is_empty() {
                    String::new()
                } else {
                    format!(" matching \"{filter}\"")
                },
            );
            for (id, text, href) in &links {
                out.push_str(&format!(
                    "  [{}] \"{}\" -> {}\n",
                    id,
                    if text.is_empty() { "(no text)" } else { text.as_str() },
                    href,
                ));
            }
            if matched > links.len() {
                out.push_str(&format!("  … {} more (raise limit)\n", matched - links.len()));
            }
            out
        }));
    }

    // The session's navigation history: where the agent has been, in stack
    // order, with a marker on the entry it currently points at.
    if name == "browser_native_history" {
        let (entries, current) = bridge.history();
        return Ok(Some(if compact {
            let items: Vec<serde_json::Value> = entries
                .iter()
                .enumerate()
                .map(|(i, (url, title))| {
                    serde_json::json!({
                        "index": i,
                        "url": url,
                        "title": title,
                        "current": i == current,
                    })
                })
                .collect();
            serde_json::to_string_pretty(&serde_json::json!({
                "entries": entries.len(),
                "current": current,
                "history": items,
            }))
            .map_err(|e| format!("serialise history report: {e}"))?
        } else {
            let mut out = format!(
                "{} history entr{} (at #{current}):\n",
                entries.len(),
                if entries.len() == 1 { "y" } else { "ies" },
            );
            for (i, (url, title)) in entries.iter().enumerate() {
                out.push_str(&format!(
                    "  {}#{i} {}{}\n",
                    if i == current { "> " } else { "  " },
                    url,
                    if title.is_empty() {
                        String::new()
                    } else {
                        format!(" \"{title}\"")
                    },
                ));
            }
            out
        }));
    }

    // Named page-state checkpoints: snapshot now, act freely, then ask "what
    // changed since?" — one delta spanning any number of actions.
    if name == "browser_native_checkpoint" {
        let action = arguments["action"].as_str().unwrap_or("save");
        let ckpt_name = arguments["name"].as_str();
        return Ok(Some(match action {
            "save" => {
                let ckpt_name = ckpt_name.ok_or("name is required for save")?;
                let (facts, replaced) = bridge.checkpoint_save(ckpt_name);
                if compact {
                    serde_json::to_string_pretty(&serde_json::json!({
                        "action": "save", "name": ckpt_name,
                        "facts": facts, "replaced": replaced,
                    }))
                    .map_err(|e| format!("serialise checkpoint report: {e}"))?
                } else {
                    format!(
                        "checkpoint '{ckpt_name}' {} ({facts} facts)\n",
                        if replaced { "replaced" } else { "saved" },
                    )
                }
            }
            "diff" => {
                let ckpt_name = ckpt_name.ok_or("name is required for diff")?;
                let delta = bridge
                    .checkpoint_diff(ckpt_name)
                    .ok_or_else(|| format!("no checkpoint '{ckpt_name}'"))?;
                if compact {
                    serde_json::to_string_pretty(&serde_json::json!({
                        "action": "diff", "name": ckpt_name,
                        "delta": delta_report(&delta),
                    }))
                    .map_err(|e| format!("serialise checkpoint report: {e}"))?
                } else {
                    format!(
                        "changes since checkpoint '{ckpt_name}':\n{}",
                        render_delta(&delta),
                    )
                }
            }
            "list" => {
                let ckpts = bridge.checkpoint_list();
                if compact {
                    let items: Vec<serde_json::Value> = ckpts
                        .iter()
                        .map(|(n, f)| serde_json::json!({ "name": n, "facts": f }))
                        .collect();
                    serde_json::to_string_pretty(&serde_json::json!({
                        "action": "list", "checkpoints": items,
                    }))
                    .map_err(|e| format!("serialise checkpoint report: {e}"))?
                } else if ckpts.is_empty() {
                    "(no checkpoints)".to_string()
                } else {
                    let mut out = format!(
                        "{} checkpoint{}:\n",
                        ckpts.len(),
                        if ckpts.len() == 1 { "" } else { "s" },
                    );
                    for (n, f) in &ckpts {
                        out.push_str(&format!("  {n} ({f} facts)\n"));
                    }
                    out
                }
            }
            "drop" => {
                let ckpt_name = ckpt_name.ok_or("name is required for drop")?;
                if !bridge.checkpoint_drop(ckpt_name) {
                    return Err(format!("no checkpoint '{ckpt_name}'").into());
                }
                if compact {
                    serde_json::to_string_pretty(&serde_json::json!({
                        "action": "drop", "name": ckpt_name,
                    }))
                    .map_err(|e| format!("serialise checkpoint report: {e}"))?
                } else {
                    format!("checkpoint '{ckpt_name}' dropped\n")
                }
            }
            other => {
                return Err(format!(
                    "unknown checkpoint action '{other}' (save, diff, list, drop)"
                )
                .into())
            }
        }));
    }

    // Pre-flight HTML5 constraint validation: know why a submit would fail
    // (required, type, pattern, length, range) before spending it.
    if name == "browser_native_validate" {
        let controls = bridge.validate_forms();
        if controls.is_empty() {
            return Ok(Some("(no form controls on page)".to_string()));
        }
        let invalid: Vec<_> = controls.iter().filter(|(_, _, f)| !f.is_empty()).collect();
        return Ok(Some(if compact {
            let items: Vec<serde_json::Value> = controls
                .iter()
                .map(|(id, name, failed)| {
                    serde_json::json!({
                        "nodeId": id,
                        "name": name,
                        "valid": failed.is_empty(),
                        "failed": failed,
                    })
                })
                .collect();
            serde_json::to_string_pretty(&serde_json::json!({
                "controls": controls.len(),
                "invalid": invalid.len(),
                "results": items,
            }))
            .map_err(|e| format!("serialise validate report: {e}"))?
        } else if invalid.is_empty() {
            format!("form is valid ({} control(s) checked)\n", controls.len())
        } else {
            let mut out = format!(
                "{} of {} control(s) invalid:\n",
                invalid.len(),
                controls.len()
            );
            for (id, name, failed) in &invalid {
                out.push_str(&format!(
                    "  [{}] \"{}\": {}\n",
                    id,
                    name,
                    failed.join(", ")
                ));
            }
            out
        }));
    }

    // Vector memory: remember indexes the current page's visible text so a
    // later recall (in this or another tab of the session) finds it by
    // meaning, keyword, or tag — no re-crawl, far fewer tokens than a page
    // dump. Remember reports exactly what was indexed; recall is read-only.
    if name == "browser_native_remember" {
        let tags: Vec<String> = arguments["tags"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let outcome = arguments["outcome"].as_f64().unwrap_or(0.0);
        let note = arguments["note"].as_str();
        let (memory_id, url, chars) = bridge.remember_page(tags.clone(), outcome, note);
        let total = bridge.memory_count();
        return Ok(Some(if compact {
            serde_json::to_string_pretty(&serde_json::json!({
                "memoryId": memory_id,
                "url": url,
                "indexedChars": chars,
                "tags": tags,
                "outcome": outcome,
                "memoryCount": total,
            }))
            .map_err(|e| format!("serialise remember report: {e}"))?
        } else {
            format!(
                "remembered page as '{}' ({} chars from {}, tags [{}], outcome {:.2}) — {} memor{} stored\n",
                memory_id,
                chars,
                if url.is_empty() { "(no url)" } else { url.as_str() },
                tags.join(", "),
                outcome,
                total,
                if total == 1 { "y" } else { "ies" },
            )
        }));
    }

    if name == "browser_native_recall" {
        let query = arguments["query"].as_str().ok_or("query is required")?;
        let mode = arguments["mode"].as_str().unwrap_or("semantic");
        if !matches!(mode, "semantic" | "keyword" | "tag" | "similar") {
            return Err(format!(
                "unknown recall mode '{mode}' (expected semantic, keyword, tag, or similar)"
            )
            .into());
        }
        let limit = arguments["limit"].as_u64().unwrap_or(5) as usize;
        let min_outcome = arguments["minOutcome"].as_f64().unwrap_or(0.0).clamp(0.0, 1.0);
        let hits = bridge.recall_pages(query, mode, limit, min_outcome);
        return Ok(Some(if compact {
            let items: Vec<serde_json::Value> = hits
                .iter()
                .map(|(n, sim)| {
                    serde_json::json!({
                        "memoryId": n.id,
                        "url": n.url,
                        "similarity": sim,
                        "tags": n.tags,
                        "outcome": n.outcome_score,
                        "snippet": memory_snippet(&n.text),
                    })
                })
                .collect();
            serde_json::to_string_pretty(&serde_json::json!({
                "mode": mode,
                "query": query,
                "minOutcome": min_outcome,
                "hits": items,
            }))
            .map_err(|e| format!("serialise recall report: {e}"))?
        } else if hits.is_empty() {
            if min_outcome > 0.0 {
                format!("no memories matched '{query}' ({mode}, outcome >= {min_outcome:.2})\n")
            } else {
                format!("no memories matched '{query}' ({mode})\n")
            }
        } else {
            let filter = if min_outcome > 0.0 {
                format!(", outcome >= {min_outcome:.2}")
            } else {
                String::new()
            };
            let mut out = format!(
                "{} memor{} matched '{}' ({}{}):\n",
                hits.len(),
                if hits.len() == 1 { "y" } else { "ies" },
                query,
                mode,
                filter
            );
            for (n, sim) in &hits {
                let score = sim.map(|s| format!("{s:.3}")).unwrap_or_else(|| "-".to_string());
                out.push_str(&format!(
                    "  [{}] {} {} tags [{}] outcome {:.2}\n      {}\n",
                    score,
                    n.id,
                    if n.url.is_empty() { "(no url)" } else { n.url.as_str() },
                    n.tags.join(", "),
                    n.outcome_score,
                    memory_snippet(&n.text),
                ));
            }
            out
        }));
    }

    // NDA export persists the session state as an on-disk artifact another
    // agent (or a later run) can consume without re-crawling the page.
    // binary = 18-byte hashed triple stream, readable = lossless fact text,
    // trace = console/mutation/performance/network traces (predicates 120-123).
    if name == "browser_native_export_nda" {
        let format = arguments["format"].as_str().unwrap_or("binary");
        let (path, fact_count, facts) = match format {
            "readable" => {
                let facts = bridge.capture_document().facts_text();
                let path = persist_browser_artifact(
                    root,
                    &format!("{session_id}_facts.txt"),
                    facts.as_bytes(),
                )?;
                (path, facts.lines().count(), Some(facts))
            }
            "trace" => {
                let triples = bridge.export_traces_nda();
                let path = persist_browser_artifact(
                    root,
                    &format!("{session_id}_trace.nda"),
                    &encode_nda_triples(&triples),
                )?;
                (path, triples.len(), None)
            }
            "binary" => {
                let triples = bridge.capture_nda();
                let path = persist_browser_artifact(
                    root,
                    &format!("{session_id}_native.nda"),
                    &encode_nda_triples(&triples),
                )?;
                (path, triples.len(), None)
            }
            other => return Err(format!(
                "unknown export format '{other}' (expected binary, readable, or trace)"
            ).into()),
        };
        return Ok(Some(if compact {
            serde_json::to_string_pretty(&serde_json::json!({
                "format": format,
                "path": path.display().to_string(),
                "factCount": fact_count,
                "facts": facts,
            }))
            .map_err(|e| format!("serialise native export report: {e}"))?
        } else {
            let mut out = format!(
                "Exported {} {} fact(s) to {}\n",
                fact_count,
                format,
                path.display()
            );
            if let Some(facts) = facts {
                out.push_str("---\n");
                out.push_str(&facts);
            }
            out
        }));
    }

    // Tab management: one foreground tab plus background tabs parked in the
    // bridge's swarm. Every tab tool answers with the refreshed tab list so
    // acting and observing stay inseparable; switching also returns the view
    // of the tab that just came to the foreground.
    if name.starts_with("browser_native_tab_") {
        let status = match name {
            "browser_native_tab_open" => {
                let tab_id = arguments["tabId"].as_str().ok_or("tabId is required")?;
                bridge.tab_open(tab_id)?;
                format!("opened background tab '{tab_id}'")
            }
            "browser_native_tab_switch" => {
                let tab_id = arguments["tabId"].as_str().ok_or("tabId is required")?;
                bridge.tab_switch(tab_id)?;
                format!("switched to tab '{tab_id}'")
            }
            "browser_native_tab_close" => {
                let tab_id = arguments["tabId"].as_str().ok_or("tabId is required")?;
                bridge.tab_close(tab_id)?;
                format!("closed tab '{tab_id}'")
            }
            _ => format!("{} open tab(s)", bridge.tab_list().len()),
        };
        let switched = name == "browser_native_tab_switch";
        return Ok(Some(if compact {
            let mut report = serde_json::json!({ "status": status, "tabs": tab_json(&bridge) });
            if switched {
                report["view"] = serde_json::to_value(view_report(&bridge.current_view()))
                    .map_err(|e| format!("serialise tab view: {e}"))?;
            }
            serde_json::to_string_pretty(&report)
                .map_err(|e| format!("serialise tab report: {e}"))?
        } else {
            let mut out = format!("{status}\n");
            out.push_str(&tab_lines(&bridge));
            if switched {
                out.push_str("---\n");
                out.push_str(&render_view(&bridge.current_view()));
            }
            out
        }));
    }

    // Eval returns a JS result, not an NDA delta.
    if name == "browser_native_eval" {
        let expr = arguments["expression"].as_str().ok_or("expression is required")?;
        let result = bridge
            .eval_js(expr)
            .map_err(|e| format!("JS eval failed: {e}"))?;
        let view = bridge.current_view();
        if compact {
            let report = serde_json::json!({
                "result": result,
                "view": view_report(&view),
            });
            return Ok(Some(
                serde_json::to_string_pretty(&report)
                    .map_err(|e| format!("serialise native eval report: {e}"))?,
            ));
        } else {
            let mut out = format!("eval result: {}\n---\n", result);
            out.push_str(&render_view(&view));
            return Ok(Some(out));
        }
    }

    // -- Phase 5: Enhanced tools --

    if name == "browser_native_wait_for" {
        let target_name = arguments["name"].as_str().ok_or("name is required for wait_for")?;
        let role = arguments["role"].as_str();
        let timeout = arguments["timeout"].as_u64().unwrap_or(5000);
        let found = bridge.agent_wait_for(role, target_name, timeout);
        return Ok(Some(match found {
            Some(node_id) => {
                let view = bridge.current_view();
                if compact {
                    serde_json::to_string_pretty(&serde_json::json!({
                        "found": true,
                        "nodeId": node_id,
                        "view": view_report(&view)
                    })).unwrap_or_default()
                } else {
                    format!("Found element at node_{}\n---\n{}", node_id, render_view(&view))
                }
            }
            None => {
                if compact {
                    serde_json::to_string_pretty(&serde_json::json!({ "found": false })).unwrap_or_default()
                } else {
                    format!("Element with role={:?} name=\"{}\" not found within {}ms", role, target_name, timeout)
                }
            }
        }));
    }

    if name == "browser_native_extract" {
        let node_id = resolve_node(&bridge, arguments)?;
        let what = arguments["what"].as_str().unwrap_or("text");
        let content = bridge.agent_extract(node_id, what);
        return Ok(Some(if compact {
            serde_json::to_string_pretty(&serde_json::json!({
                "nodeId": node_id,
                "what": what,
                "content": content
            })).unwrap_or_default()
        } else {
            format!("node_{} [{}]:\n{}", node_id, what, content)
        }));
    }

    if name == "browser_native_cookies" {
        let op = arguments["operation"].as_str().unwrap_or("get");
        let cookie_name = arguments["name"].as_str().unwrap_or("");
        match op {
            "set" => {
                let value = arguments["value"].as_str().unwrap_or("");
                let domain = arguments["domain"].as_str().unwrap_or("");
                bridge.set_cookie(cookie_name, value, domain);
                return Ok(Some(format!("Cookie '{}' set", cookie_name)));
            }
            "delete" => {
                bridge.delete_cookie(cookie_name);
                return Ok(Some(format!("Cookie '{}' deleted", cookie_name)));
            }
            _ => {
                let val = bridge.get_cookie(cookie_name);
                return Ok(Some(match val {
                    Some(v) => format!("{}={}", cookie_name, v),
                    None => format!("Cookie '{}' not found", cookie_name),
                }));
            }
        }
    }

    if name == "browser_native_storage" {
        let storage_type = arguments["storageType"].as_str().unwrap_or("local");
        let op = arguments["operation"].as_str().unwrap_or("get");
        let key = arguments["key"].as_str().unwrap_or("");
        match op {
            "set" => {
                let value = arguments["value"].as_str().unwrap_or("");
                bridge.set_storage(storage_type, key, value);
                return Ok(Some(format!("{}Storage['{}'] set", storage_type, key)));
            }
            "clear" => {
                bridge.clear_storage(storage_type);
                return Ok(Some(format!("{}Storage cleared", storage_type)));
            }
            _ => {
                let val = bridge.get_storage(storage_type, key);
                return Ok(Some(match val {
                    Some(v) => v,
                    None => "null".to_string(),
                }));
            }
        }
    }

    if name == "browser_native_network" {
        let requests = bridge.list_network_requests();
        if compact {
            let items: Vec<serde_json::Value> = requests.iter().map(|(url, method, status, rt)| {
                serde_json::json!({ "url": url, "method": method, "status": status, "type": rt })
            }).collect();
            return Ok(Some(serde_json::to_string_pretty(&items).unwrap_or_default()));
        } else {
            let mut out = format!("Network requests ({}):\n", requests.len());
            for (url, method, status, rt) in &requests {
                out.push_str(&format!("  {} {} -> {} [{}]\n", method, url, status, rt));
            }
            return Ok(Some(out));
        }
    }

    if name == "browser_native_screenshot" {
        return Ok(Some(bridge.dom_snapshot()));
    }

    if name == "browser_native_hover" {
        let node_id = resolve_node(&bridge, arguments)?;
        let result = bridge.agent_hover(node_id);
        let view = bridge.current_view();
        if compact {
            let report = ActionReport {
                status: result.status.clone(),
                delta: delta_report(&result.delta),
                view: view_report(&view),
            };
            return Ok(Some(serde_json::to_string_pretty(&report).unwrap_or_default()));
        } else {
            let mut out = format!("{}\nChanges:\n{}", result.status, render_delta(&result.delta));
            out.push_str(&render_view(&view));
            return Ok(Some(out));
        }
    }

    if name == "browser_native_press_key" {
        let key = arguments["key"].as_str().ok_or("key is required")?;
        let result = bridge.agent_press_key(key);
        let view = bridge.current_view();
        if compact {
            let report = ActionReport {
                status: result.status.clone(),
                delta: delta_report(&result.delta),
                view: view_report(&view),
            };
            return Ok(Some(serde_json::to_string_pretty(&report).unwrap_or_default()));
        } else {
            let mut out = format!("{}\nChanges:\n{}", result.status, render_delta(&result.delta));
            out.push_str(&render_view(&view));
            return Ok(Some(out));
        }
    }

    let result = match name {
        "browser_native_navigate" => {
            let url = arguments["url"].as_str().ok_or("url is required")?;
            bridge.agent_navigate(url)
        }
        "browser_native_click" => {
            let node_id = resolve_node(&bridge, arguments)?;
            bridge.agent_click(node_id)
        }
        "browser_native_type" => {
            let node_id = resolve_node(&bridge, arguments)?;
            let text = arguments["text"].as_str().ok_or("text is required")?;
            bridge.agent_type(node_id, text)
        }
        "browser_native_select" => {
            let node_id = resolve_node(&bridge, arguments)?;
            let value = arguments["value"].as_str().ok_or("value is required")?;
            bridge.agent_select(node_id, value)
        }
        "browser_native_submit" => {
            let node_id = resolve_node(&bridge, arguments)?;
            bridge.agent_submit(node_id)
        }
        "browser_native_scroll" => {
            let dx = arguments["deltaX"].as_i64().unwrap_or(0) as i32;
            let dy = arguments["deltaY"].as_i64().unwrap_or(0) as i32;
            bridge.agent_scroll(dx, dy)
        }
        "browser_native_scroll_into_view" => {
            let label = arguments["label"].as_str().ok_or("label is required")?;
            bridge.agent_scroll_into_view(label)
        }
        "browser_native_back" => bridge.agent_back(),
        "browser_native_forward" => bridge.agent_forward(),
        "browser_native_click_text" => {
            let text = arguments["text"].as_str().ok_or("text is required")?;
            bridge.agent_click_by_text(text)
        }
        "browser_native_fill_label" => {
            let label = arguments["label"].as_str().ok_or("label is required")?;
            let text = arguments["text"].as_str().ok_or("text is required")?;
            bridge.agent_fill_by_label(label, text)
        }
        "browser_native_check_label" => {
            let label = arguments["label"].as_str().ok_or("label is required")?;
            let checked = arguments["checked"].as_bool().unwrap_or(true);
            bridge.agent_check_by_label(label, checked)
        }
        "browser_native_select_label" => {
            let label = arguments["label"].as_str().ok_or("label is required")?;
            let option = arguments["option"].as_str().ok_or("option is required")?;
            bridge.agent_select_by_label(label, option)
        }
        "browser_native_focus_label" => {
            let label = arguments["label"].as_str().ok_or("label is required")?;
            bridge.agent_focus_by_label(label)
        }
        "browser_native_press" => {
            let key = arguments["key"].as_str().ok_or("key is required")?;
            bridge.agent_press(key)
        }
        "browser_native_settle" => bridge.agent_settle(),
        _ => unreachable!("native tool name already matched above"),
    };

    let view = bridge.current_view();
    if compact {
        let report = ActionReport {
            status: result.status.clone(),
            delta: delta_report(&result.delta),
            view: view_report(&view),
        };
        Ok(Some(
            serde_json::to_string_pretty(&report)
                .map_err(|e| format!("serialise native action report: {e}"))?,
        ))
    } else {
        let mut out = String::new();
        out.push_str(&format!("{}\n", result.status));
        out.push_str("Changes:\n");
        out.push_str(&render_delta(&result.delta));
        out.push_str("---\n");
        out.push_str(&render_view(&view));
        Ok(Some(out))
    }
}

#[cfg(test)]
mod native_label_tool_tests {
    use super::*;
    use serde_json::json;

    const FORM_HTML: &str = r#"<html><head><title>Signup</title></head><body>
        <form id="f">
          <input type="text" placeholder="Email" name="email" />
          <input type="checkbox" aria-label="Subscribe" />
          <select aria-label="Plan">
            <option value="free">Free</option>
            <option value="pro">Pro</option>
          </select>
          <button type="submit">Log In</button>
        </form>
    </body></html>"#;

    /// Each test uses its own session id: bridges are process-global by id.
    fn load(session: &str) {
        let bridge = get_or_create_native_bridge(session);
        bridge.lock().unwrap().load_html("http://local.test/form", FORM_HTML);
    }

    fn call(name: &str, args: serde_json::Value) -> String {
        handle_native_tool(Path::new("."), name, &args)
            .expect("tool call succeeds")
            .expect("native tool name is handled")
    }

    #[test]
    fn click_text_tool_acts_and_reports_observation() {
        load("t17-click");
        let out = call(
            "browser_native_click_text",
            json!({ "sessionId": "t17-click", "text": "Log In" }),
        );
        assert!(out.contains("clicked"), "status should report the click: {out}");
        assert!(out.contains("Changes:"), "action output must include the delta section");
        assert!(out.contains("URL:"), "action output must include the refreshed view");
    }

    #[test]
    fn fill_label_then_read_form_shows_typed_value() {
        load("t17-fill");
        let out = call(
            "browser_native_fill_label",
            json!({ "sessionId": "t17-fill", "label": "Email", "text": "a@b.c" }),
        );
        assert!(out.contains("node_"), "fill should resolve a concrete node: {out}");
        let form = call("browser_native_read_form", json!({ "sessionId": "t17-fill" }));
        assert!(form.contains("a@b.c"), "read_form must show the typed value: {form}");
        assert!(form.contains("unchecked"), "read_form must show checkbox state: {form}");
    }

    #[test]
    fn check_and_select_label_tools_update_form_state() {
        load("t17-check");
        let out = call(
            "browser_native_check_label",
            json!({ "sessionId": "t17-check", "label": "Subscribe" }),
        );
        assert!(out.contains("checked"), "check status: {out}");
        let out = call(
            "browser_native_select_label",
            json!({ "sessionId": "t17-check", "label": "Plan", "option": "Pro" }),
        );
        assert!(out.contains("selected 'pro'"), "select status: {out}");
        let form = call("browser_native_read_form", json!({ "sessionId": "t17-check" }));
        assert!(form.contains("checked"), "form shows checked state: {form}");
        assert!(form.contains("pro"), "form shows selected value: {form}");
    }

    #[test]
    fn focus_label_and_press_drive_session_keyboard() {
        load("t17-press");
        let miss = call(
            "browser_native_press",
            json!({ "sessionId": "t17-press", "key": "x" }),
        );
        assert!(miss.contains("nothing focused"), "press without focus: {miss}");
        let out = call(
            "browser_native_focus_label",
            json!({ "sessionId": "t17-press", "label": "Email" }),
        );
        assert!(out.contains("focused"), "focus status: {out}");
        let out = call(
            "browser_native_press",
            json!({ "sessionId": "t17-press", "key": "z" }),
        );
        assert!(out.contains("pressed"), "press status: {out}");
        let form = call("browser_native_read_form", json!({ "sessionId": "t17-press" }));
        assert!(form.contains('z'), "pressed character lands in the control: {form}");
    }

    #[test]
    fn observe_and_settle_tools_return_readable_state() {
        load("t17-observe");
        let facts = call("browser_native_observe", json!({ "sessionId": "t17-observe" }));
        assert!(facts.contains("http://local.test/form"), "observe includes url: {facts}");
        assert!(facts.contains("button"), "observe includes AOM roles: {facts}");
        let out = call("browser_native_settle", json!({ "sessionId": "t17-observe" }));
        assert!(out.contains("settled"), "settle status: {out}");
    }

    #[test]
    fn compact_flag_returns_json_action_report() {
        load("t17-compact");
        let out = call(
            "browser_native_click_text",
            json!({ "sessionId": "t17-compact", "text": "Log In", "compact": true }),
        );
        let report: serde_json::Value =
            serde_json::from_str(&out).expect("compact output is valid JSON");
        assert!(report["status"].as_str().unwrap().contains("clicked"));
        assert!(report.get("delta").is_some(), "report carries the delta");
        assert!(report["view"]["url"].as_str().unwrap().contains("local.test"));
    }

    /// Export tests write real artifacts, so they root themselves in the OS
    /// temp dir instead of the workspace.
    fn call_rooted(root: &Path, name: &str, args: serde_json::Value) -> String {
        handle_native_tool(root, name, &args)
            .expect("tool call succeeds")
            .expect("native tool name is handled")
    }

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("velocity_export_{tag}"));
        let _ = std::fs::create_dir_all(&root);
        root
    }

    #[test]
    fn export_nda_binary_writes_triple_stream_artifact() {
        load("t18-bin");
        let root = temp_root("bin");
        let out = call_rooted(
            &root,
            "browser_native_export_nda",
            json!({ "sessionId": "t18-bin" }),
        );
        assert!(out.contains("binary"), "default format is binary: {out}");
        assert!(out.contains("t18-bin_native.nda"), "output names the artifact: {out}");
        let path = root
            .join(".velocity")
            .join("browser_artifacts")
            .join("t18-bin_native.nda");
        let bytes = std::fs::read(&path).expect("binary artifact exists");
        assert!(!bytes.is_empty(), "a loaded page produces state triples");
        assert_eq!(bytes.len() % 18, 0, "stream is whole 18-byte triple records");
    }

    #[test]
    fn export_nda_readable_returns_and_persists_fact_text() {
        load("t18-read");
        let root = temp_root("read");
        let out = call_rooted(
            &root,
            "browser_native_export_nda",
            json!({ "sessionId": "t18-read", "format": "readable" }),
        );
        assert!(
            out.contains("http://local.test/form"),
            "readable export returns the fact text inline: {out}"
        );
        let path = root
            .join(".velocity")
            .join("browser_artifacts")
            .join("t18-read_facts.txt");
        let persisted = std::fs::read_to_string(&path).expect("facts artifact exists");
        assert!(persisted.contains("http://local.test/form"), "persisted facts match: {persisted}");
    }

    #[test]
    fn export_nda_trace_persists_trace_stream() {
        load("t18-trace");
        // Act first so the trace collector has something to export.
        call(
            "browser_native_fill_label",
            json!({ "sessionId": "t18-trace", "label": "Email", "text": "t@e.st" }),
        );
        let root = temp_root("trace");
        let out = call_rooted(
            &root,
            "browser_native_export_nda",
            json!({ "sessionId": "t18-trace", "format": "trace" }),
        );
        assert!(out.contains("t18-trace_trace.nda"), "output names the artifact: {out}");
        let path = root
            .join(".velocity")
            .join("browser_artifacts")
            .join("t18-trace_trace.nda");
        let bytes = std::fs::read(&path).expect("trace artifact exists");
        assert_eq!(bytes.len() % 18, 0, "trace stream is whole triple records");
    }

    #[test]
    fn export_nda_compact_reports_path_and_fact_count() {
        load("t18-compact");
        let root = temp_root("compact");
        let out = call_rooted(
            &root,
            "browser_native_export_nda",
            json!({ "sessionId": "t18-compact", "compact": true }),
        );
        let report: serde_json::Value =
            serde_json::from_str(&out).expect("compact export output is valid JSON");
        assert_eq!(report["format"], "binary");
        assert!(report["factCount"].as_u64().unwrap() > 0, "fact count reported");
        assert!(report["path"].as_str().unwrap().contains("t18-compact_native.nda"));
    }

    #[test]
    fn export_nda_rejects_unknown_format() {
        load("t18-badfmt");
        let root = temp_root("badfmt");
        let err = handle_native_tool(
            &root,
            "browser_native_export_nda",
            &json!({ "sessionId": "t18-badfmt", "format": "yaml" }),
        )
        .expect_err("unknown format must be rejected");
        assert!(err.to_string().contains("unknown export format"), "{err}");
    }

    #[test]
    fn tab_tools_open_switch_and_close_with_observed_state() {
        load("t19-tabs");
        let out = call(
            "browser_native_tab_open",
            json!({ "sessionId": "t19-tabs", "tabId": "t19-tabs-bg" }),
        );
        assert!(out.contains("opened background tab 't19-tabs-bg'"), "{out}");
        assert!(out.contains("* t19-tabs \""), "original tab stays active: {out}");

        let out = call(
            "browser_native_tab_switch",
            json!({ "sessionId": "t19-tabs", "tabId": "t19-tabs-bg" }),
        );
        assert!(out.contains("switched to tab 't19-tabs-bg'"), "{out}");
        assert!(out.contains("* t19-tabs-bg \""), "new tab becomes active: {out}");
        assert!(out.contains("URL:"), "switch returns the newly active view: {out}");

        // Switching back must restore the parked tab with its page intact.
        let out = call(
            "browser_native_tab_switch",
            json!({ "sessionId": "t19-tabs", "tabId": "t19-tabs" }),
        );
        assert!(
            out.contains("http://local.test/form"),
            "foreground state survives parking: {out}"
        );

        let out = call(
            "browser_native_tab_close",
            json!({ "sessionId": "t19-tabs", "tabId": "t19-tabs-bg" }),
        );
        assert!(out.contains("closed tab 't19-tabs-bg'"), "{out}");
        assert!(out.contains("Tabs (1):"), "closed tab leaves the list: {out}");
    }

    #[test]
    fn tab_close_active_and_duplicate_open_are_rejected() {
        load("t19-taberr");
        let err = handle_native_tool(
            Path::new("."),
            "browser_native_tab_close",
            &json!({ "sessionId": "t19-taberr", "tabId": "t19-taberr" }),
        )
        .expect_err("closing the active tab must fail");
        assert!(err.to_string().contains("cannot close the active tab"), "{err}");

        call(
            "browser_native_tab_open",
            json!({ "sessionId": "t19-taberr", "tabId": "t19-taberr-bg" }),
        );
        let err = handle_native_tool(
            Path::new("."),
            "browser_native_tab_open",
            &json!({ "sessionId": "t19-taberr", "tabId": "t19-taberr-bg" }),
        )
        .expect_err("duplicate tab id must be rejected");
        assert!(err.to_string().contains("already exists"), "{err}");
    }

    #[test]
    fn tab_list_compact_reports_active_flag() {
        load("t19-tablist");
        call(
            "browser_native_tab_open",
            json!({ "sessionId": "t19-tablist", "tabId": "t19-tablist-bg" }),
        );
        let out = call(
            "browser_native_tab_list",
            json!({ "sessionId": "t19-tablist", "compact": true }),
        );
        let report: serde_json::Value =
            serde_json::from_str(&out).expect("compact tab list is valid JSON");
        let tabs = report["tabs"].as_array().expect("tabs array");
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[0]["tabId"], "t19-tablist");
        assert_eq!(tabs[0]["active"], true);
        assert_eq!(tabs[1]["tabId"], "t19-tablist-bg");
        assert_eq!(tabs[1]["active"], false);
    }

    #[test]
    fn scroll_tool_reports_offset_and_scroll_fact_delta() {
        load("t20-scroll");
        let out = call(
            "browser_native_scroll",
            json!({ "sessionId": "t20-scroll", "deltaY": 120 }),
        );
        assert!(out.contains("to offset (0, 120)"), "status carries the new offset: {out}");
        assert!(
            out.contains("scroll : 0,0 -> 0,120"),
            "delta shows the scroll fact moving: {out}"
        );
    }

    #[test]
    fn scroll_into_view_tool_resolves_element_by_label() {
        load("t20-inview");
        // The default 1920x1080 viewport already shows the whole form.
        let out = call(
            "browser_native_scroll_into_view",
            json!({ "sessionId": "t20-inview", "label": "Log In" }),
        );
        assert!(out.contains("already in view"), "{out}");
        assert!(out.contains("URL:"), "action output includes the refreshed view: {out}");

        // Shrink the viewport so the submit button starts below the fold.
        get_or_create_native_bridge("t20-inview")
            .lock()
            .unwrap()
            .active_session
            .viewport_height = 10.0;
        let out = call(
            "browser_native_scroll_into_view",
            json!({ "sessionId": "t20-inview", "label": "Log In" }),
        );
        assert!(out.contains("into view (offset"), "{out}");
        assert!(
            out.contains("inViewport"),
            "delta shows in-viewport facts flipping: {out}"
        );
    }

    #[test]
    fn scroll_into_view_tool_reports_missing_label() {
        load("t20-miss");
        let out = call(
            "browser_native_scroll_into_view",
            json!({ "sessionId": "t20-miss", "label": "Nonexistent Widget" }),
        );
        assert!(out.contains("no element matching"), "{out}");
        assert!(out.contains("(no state change)"), "miss produces an empty delta: {out}");
    }

    #[test]
    fn remember_tool_indexes_page_and_recall_finds_it_semantically() {
        load("t21-mem");
        let out = call(
            "browser_native_remember",
            json!({ "sessionId": "t21-mem", "tags": ["signup"], "outcome": 0.9 }),
        );
        assert!(out.contains("remembered page as 't21-mem:0'"), "{out}");
        assert!(out.contains("http://local.test/form"), "report carries the url: {out}");
        assert!(out.contains("1 memory stored"), "{out}");

        let out = call(
            "browser_native_recall",
            json!({ "sessionId": "t21-mem", "query": "signup" }),
        );
        assert!(out.contains("1 memory matched 'signup' (semantic):"), "{out}");
        assert!(out.contains("t21-mem:0"), "hit lists the memory id: {out}");
        assert!(out.contains("http://local.test/form"), "hit lists the url: {out}");

        let out = call(
            "browser_native_recall",
            json!({ "sessionId": "t21-mem", "query": "signup", "compact": true }),
        );
        let report: serde_json::Value =
            serde_json::from_str(&out).expect("compact recall is valid JSON");
        let hits = report["hits"].as_array().expect("hits array");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0]["memoryId"], "t21-mem:0");
        assert!(hits[0]["similarity"].as_f64().expect("semantic score") > 0.0);
    }

    #[test]
    fn recall_tool_supports_keyword_tag_and_empty_results() {
        load("t21-modes");
        call(
            "browser_native_remember",
            json!({
                "sessionId": "t21-modes",
                "tags": ["checkout"],
                "outcome": 0.7,
                "note": "special discount pricing page"
            }),
        );

        let out = call(
            "browser_native_recall",
            json!({ "sessionId": "t21-modes", "query": "discount", "mode": "keyword" }),
        );
        assert!(out.contains("(keyword)"), "{out}");
        assert!(out.contains("discount"), "note text is indexed and recallable: {out}");

        let out = call(
            "browser_native_recall",
            json!({ "sessionId": "t21-modes", "query": "checkout", "mode": "tag" }),
        );
        assert!(out.contains("(tag)"), "{out}");
        assert!(out.contains("tags [checkout]"), "{out}");
        assert!(out.contains("outcome 0.70"), "{out}");

        let out = call(
            "browser_native_recall",
            json!({ "sessionId": "t21-modes", "query": "quantum blockchain", "mode": "semantic" }),
        );
        assert!(out.contains("no memories matched 'quantum blockchain'"), "{out}");

        let err = handle_native_tool(
            Path::new("."),
            "browser_native_recall",
            &json!({ "sessionId": "t21-modes", "query": "x", "mode": "psychic" }),
        )
        .expect_err("unknown recall mode must be rejected");
        assert!(err.to_string().contains("unknown recall mode"), "{err}");
    }

    #[test]
    fn recall_tool_finds_similar_memories_and_filters_by_outcome() {
        load("t22-sim");
        call(
            "browser_native_remember",
            json!({ "sessionId": "t22-sim", "tags": ["attempt"], "outcome": 0.9, "note": "first pass" }),
        );
        call(
            "browser_native_remember",
            json!({ "sessionId": "t22-sim", "tags": ["attempt"], "outcome": 0.2, "note": "second pass" }),
        );

        // Same page indexed twice: similar mode on the first memory id must
        // surface the second one with a high embedding score.
        let out = call(
            "browser_native_recall",
            json!({ "sessionId": "t22-sim", "query": "t22-sim:0", "mode": "similar" }),
        );
        assert!(out.contains("(similar)"), "{out}");
        assert!(out.contains("t22-sim:1"), "sibling memory is found: {out}");
        assert!(!out.contains("t22-sim:0 http"), "source memory excludes itself: {out}");

        // Unknown memory id is a miss, not an error.
        let out = call(
            "browser_native_recall",
            json!({ "sessionId": "t22-sim", "query": "t22-sim:99", "mode": "similar" }),
        );
        assert!(out.contains("no memories matched 't22-sim:99' (similar)"), "{out}");

        // minOutcome keeps only the successful attempt across any mode.
        let out = call(
            "browser_native_recall",
            json!({ "sessionId": "t22-sim", "query": "attempt", "mode": "tag", "minOutcome": 0.8 }),
        );
        assert!(out.contains("1 memory matched 'attempt' (tag, outcome >= 0.80):"), "{out}");
        assert!(out.contains("t22-sim:0"), "{out}");
        assert!(!out.contains("t22-sim:1"), "low-outcome memory filtered: {out}");

        // Filter that excludes everything reports the threshold in the miss.
        let out = call(
            "browser_native_recall",
            json!({ "sessionId": "t22-sim", "query": "attempt", "mode": "tag", "minOutcome": 0.95 }),
        );
        assert!(out.contains("no memories matched 'attempt' (tag, outcome >= 0.95)"), "{out}");

        // Compact report carries the filter value.
        let out = call(
            "browser_native_recall",
            json!({ "sessionId": "t22-sim", "query": "attempt", "mode": "tag", "minOutcome": 0.8, "compact": true }),
        );
        let report: serde_json::Value =
            serde_json::from_str(&out).expect("compact recall is valid JSON");
        assert_eq!(report["minOutcome"], 0.8);
        assert_eq!(report["hits"].as_array().expect("hits array").len(), 1);
    }

    #[test]
    fn page_text_tool_reads_visible_text_with_truncation() {
        load("t24-text");
        let out = call("browser_native_page_text", json!({ "sessionId": "t24-text" }));
        assert!(out.starts_with("Signup"), "title leads the text: {out}");
        assert!(out.contains("Log In"), "button text is visible: {out}");

        let out = call(
            "browser_native_page_text",
            json!({ "sessionId": "t24-text", "maxChars": 6 }),
        );
        assert!(out.starts_with("Signup…"), "{out}");
        assert!(out.contains("(truncated to 6 of"), "{out}");
    }

    #[test]
    fn screencast_tool_captures_lists_and_saves_frames() {
        load("t24-cast");
        let out = call(
            "browser_native_screencast",
            json!({ "sessionId": "t24-cast", "action": "capture" }),
        );
        assert!(out.contains("captured frame 0"), "{out}");
        assert!(out.contains("1 frame in timeline"), "{out}");

        // Default action is capture.
        let out = call("browser_native_screencast", json!({ "sessionId": "t24-cast" }));
        assert!(out.contains("captured frame 1"), "{out}");

        let out = call(
            "browser_native_screencast",
            json!({ "sessionId": "t24-cast", "action": "list" }),
        );
        assert!(out.contains("2 frames in timeline:"), "{out}");
        assert!(out.contains("frame 0: 1920x1080"), "{out}");
        assert!(out.contains("frame 1: 1920x1080"), "{out}");

        let tmp = std::env::temp_dir();
        let out = handle_native_tool(
            &tmp,
            "browser_native_screencast",
            &json!({ "sessionId": "t24-cast", "action": "save" }),
        )
        .expect("save succeeds")
        .expect("screencast tool is handled");
        assert!(out.contains("saved 2 frame(s) to"), "{out}");
        assert!(out.contains("t24-cast_screencast.json"), "{out}");

        let err = handle_native_tool(
            Path::new("."),
            "browser_native_screencast",
            &json!({ "sessionId": "t24-cast", "action": "explode" }),
        )
        .expect_err("unknown screencast action must be rejected");
        assert!(err.to_string().contains("unknown screencast action"), "{err}");
    }

    #[test]
    fn find_tool_filters_aom_by_role_and_text() {
        load("t25-find");
        let out = call(
            "browser_native_find",
            json!({ "sessionId": "t25-find", "role": "button" }),
        );
        assert!(out.contains("Log In"), "button hit is listed: {out}");
        assert!(out.contains("elements matched role=button"), "{out}");

        let out = call(
            "browser_native_find",
            json!({ "sessionId": "t25-find", "text": "plan" }),
        );
        assert!(out.contains("\"Plan\""), "select matched by label text: {out}");

        let out = call(
            "browser_native_find",
            json!({ "sessionId": "t25-find", "text": "zzz-nope" }),
        );
        assert!(out.contains("no elements matched"), "{out}");

        let err = handle_native_tool(
            Path::new("."),
            "browser_native_find",
            &json!({ "sessionId": "t25-find" }),
        )
        .expect_err("find without role or text must be rejected");
        assert!(err.to_string().contains("at least one of role or text"), "{err}");
    }

    #[test]
    fn validate_tool_reports_constraint_failures_then_valid() {
        let html = r#"<html><head><title>Join</title></head><body>
            <form id="j">
              <input type="email" placeholder="Email" name="email" required />
              <input type="text" name="nick" value="ok" />
              <button type="submit">Join</button>
            </form>
        </body></html>"#;
        get_or_create_native_bridge("t25-valid")
            .lock()
            .unwrap()
            .load_html("http://local.test/join", html);

        let out = call("browser_native_validate", json!({ "sessionId": "t25-valid" }));
        assert!(out.contains("1 of 2 control(s) invalid"), "{out}");
        assert!(out.contains("valueMissing"), "empty required email: {out}");

        call(
            "browser_native_fill_label",
            json!({ "sessionId": "t25-valid", "label": "Email", "text": "not-an-email" }),
        );
        let out = call("browser_native_validate", json!({ "sessionId": "t25-valid" }));
        assert!(out.contains("typeMismatch"), "bad email flagged: {out}");

        call(
            "browser_native_fill_label",
            json!({ "sessionId": "t25-valid", "label": "Email", "text": "a@b.com" }),
        );
        let out = call("browser_native_validate", json!({ "sessionId": "t25-valid" }));
        assert!(out.contains("form is valid (2 control(s) checked)"), "{out}");

        let compact = call(
            "browser_native_validate",
            json!({ "sessionId": "t25-valid", "compact": true }),
        );
        let report: serde_json::Value =
            serde_json::from_str(&compact).expect("compact validate is valid JSON");
        assert_eq!(report["controls"], 2);
        assert_eq!(report["invalid"], 0);
    }

    #[test]
    fn links_tool_lists_navigation_map_with_filter_and_limit() {
        let html = r#"<html><head><title>Nav</title></head><body>
            <a href="/pricing">Pricing</a>
            <a href="/docs">Docs</a>
            <a href="https://ext.example/x">External <b>Deal</b></a>
            <a name="top">Bare anchor</a>
        </body></html>"#;
        get_or_create_native_bridge("t26-links")
            .lock()
            .unwrap()
            .load_html("http://local.test/nav", html);

        let out = call("browser_native_links", json!({ "sessionId": "t26-links" }));
        assert!(out.starts_with("3 links:"), "bare anchor is excluded: {out}");
        assert!(out.contains("\"Pricing\" -> /pricing"), "{out}");
        assert!(
            out.contains("\"ExternalDeal\" -> https://ext.example/x"),
            "nested text is included: {out}"
        );

        let out = call(
            "browser_native_links",
            json!({ "sessionId": "t26-links", "filter": "docs" }),
        );
        assert!(out.starts_with("1 link matching \"docs\":"), "{out}");
        assert!(out.contains("-> /docs"), "{out}");

        let out = call(
            "browser_native_links",
            json!({ "sessionId": "t26-links", "filter": "zzz-nope" }),
        );
        assert!(out.contains("no links matched"), "{out}");

        let out = call(
            "browser_native_links",
            json!({ "sessionId": "t26-links", "limit": 1 }),
        );
        assert!(out.contains("… 2 more"), "truncation is reported: {out}");

        let compact = call(
            "browser_native_links",
            json!({ "sessionId": "t26-links", "compact": true }),
        );
        let report: serde_json::Value =
            serde_json::from_str(&compact).expect("compact links is valid JSON");
        assert_eq!(report["matched"], 3);
        assert_eq!(report["links"].as_array().expect("links array").len(), 3);
    }

    #[test]
    fn history_tool_lists_stack_and_traversal_keeps_forward_entries() {
        load("t27-hist");
        let two = r#"<html><head><title>Two</title></head><body><p>second</p></body></html>"#;
        get_or_create_native_bridge("t27-hist")
            .lock()
            .unwrap()
            .load_html("http://local.test/two", two);

        let out = call("browser_native_history", json!({ "sessionId": "t27-hist" }));
        assert!(out.starts_with("3 history entries (at #2):"), "{out}");
        assert!(out.contains("> #2 http://local.test/two \"Two\""), "{out}");
        assert!(out.contains("#1 http://local.test/form \"Signup\""), "titles are backfilled: {out}");
        assert!(out.contains("#0 about:blank\n"), "seed entry has no title: {out}");

        // Reloading the current entry must not grow the stack.
        get_or_create_native_bridge("t27-hist")
            .lock()
            .unwrap()
            .load_html("http://local.test/two", two);
        let out = call("browser_native_history", json!({ "sessionId": "t27-hist" }));
        assert!(out.starts_with("3 history entries (at #2):"), "reload does not duplicate: {out}");

        // Going back then re-loading that entry (what agent_back does after
        // a successful fetch) must keep the forward entry intact.
        {
            let bridge = get_or_create_native_bridge("t27-hist");
            let mut b = bridge.lock().unwrap();
            let url = b
                .active_session
                .history_stack
                .back()
                .expect("has a previous entry")
                .url
                .clone();
            b.load_html(&url, FORM_HTML);
        }
        let out = call("browser_native_history", json!({ "sessionId": "t27-hist" }));
        assert!(out.starts_with("3 history entries (at #1):"), "forward entry survives: {out}");
        assert!(out.contains("> #1 http://local.test/form"), "{out}");
        assert!(out.contains("  #2 http://local.test/two"), "{out}");

        let compact = call(
            "browser_native_history",
            json!({ "sessionId": "t27-hist", "compact": true }),
        );
        let report: serde_json::Value =
            serde_json::from_str(&compact).expect("compact history is valid JSON");
        assert_eq!(report["entries"], 3);
        assert_eq!(report["current"], 1);
        assert_eq!(report["history"][1]["current"], true);
    }

    #[test]
    fn checkpoint_tool_saves_diffs_lists_and_drops() {
        load("t28-ckpt");
        let out = call(
            "browser_native_checkpoint",
            json!({ "sessionId": "t28-ckpt", "action": "save", "name": "start" }),
        );
        assert!(out.contains("checkpoint 'start' saved"), "{out}");

        // Nothing happened yet: the diff is empty.
        let out = call(
            "browser_native_checkpoint",
            json!({ "sessionId": "t28-ckpt", "action": "diff", "name": "start" }),
        );
        assert!(out.contains("changes since checkpoint 'start':"), "{out}");
        assert!(out.contains("(no state change)"), "{out}");

        // Two actions later, one diff reports the accumulated change.
        call(
            "browser_native_fill_label",
            json!({ "sessionId": "t28-ckpt", "label": "Email", "text": "x@y.example" }),
        );
        call(
            "browser_native_check_label",
            json!({ "sessionId": "t28-ckpt", "label": "Subscribe", "checked": true }),
        );
        let out = call(
            "browser_native_checkpoint",
            json!({ "sessionId": "t28-ckpt", "action": "diff", "name": "start" }),
        );
        assert!(out.contains("x@y.example"), "fill shows in the delta: {out}");
        assert!(!out.contains("(no state change)"), "{out}");

        // Saving under the same name replaces the snapshot.
        let out = call(
            "browser_native_checkpoint",
            json!({ "sessionId": "t28-ckpt", "action": "save", "name": "start" }),
        );
        assert!(out.contains("checkpoint 'start' replaced"), "{out}");

        // list shows the snapshot, drop removes it.
        let out = call(
            "browser_native_checkpoint",
            json!({ "sessionId": "t28-ckpt", "action": "list" }),
        );
        assert!(out.starts_with("1 checkpoint:"), "{out}");
        assert!(out.contains("start ("), "{out}");
        let out = call(
            "browser_native_checkpoint",
            json!({ "sessionId": "t28-ckpt", "action": "drop", "name": "start" }),
        );
        assert!(out.contains("checkpoint 'start' dropped"), "{out}");
        let out = call(
            "browser_native_checkpoint",
            json!({ "sessionId": "t28-ckpt", "action": "list" }),
        );
        assert!(out.contains("(no checkpoints)"), "{out}");

        // Missing checkpoint and unknown action are errors.
        let err = handle_native_tool(
            Path::new("."),
            "browser_native_checkpoint",
            &json!({ "sessionId": "t28-ckpt", "action": "diff", "name": "gone" }),
        )
        .expect_err("diff against a missing checkpoint must fail");
        assert!(err.to_string().contains("no checkpoint 'gone'"), "{err}");
        let err = handle_native_tool(
            Path::new("."),
            "browser_native_checkpoint",
            &json!({ "sessionId": "t28-ckpt", "action": "teleport" }),
        )
        .expect_err("unknown checkpoint action must be rejected");
        assert!(err.to_string().contains("unknown checkpoint action"), "{err}");

        // Compact save carries the fact count.
        let compact = call(
            "browser_native_checkpoint",
            json!({ "sessionId": "t28-ckpt", "action": "save", "name": "s2", "compact": true }),
        );
        let report: serde_json::Value =
            serde_json::from_str(&compact).expect("compact checkpoint is valid JSON");
        assert_eq!(report["action"], "save");
        assert_eq!(report["replaced"], false);
        assert!(report["facts"].as_u64().expect("facts count") > 0);
    }
}
