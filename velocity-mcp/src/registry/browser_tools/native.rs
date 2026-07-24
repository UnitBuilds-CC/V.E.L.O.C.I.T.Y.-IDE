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
    get_or_create_native_bridge, NativeBrowserBridge, NativeBrowserView,
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
    _root: &Path,
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
        | "browser_native_press_key" => arguments["sessionId"]
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
