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
        SESSION_URL => "url",
        SESSION_TITLE => "title",
        SESSION_COOKIE => "cookie",
        SESSION_STORAGE => "storage",
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
}
