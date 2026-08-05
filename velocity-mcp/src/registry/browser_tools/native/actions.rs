//! State-mutating actions of the native browser family: every branch runs
//! the rolling `_pre` checkpoint, scores the observed outcome, and returns
//! the delta + refreshed view in the same report.

use serde_json::Value;
use std::error::Error;

use crate::editor::browser::native_bridge::NativeBrowserBridge;

use super::*;

pub(super) fn handle_action_tool(
    bridge: &mut NativeBrowserBridge,
    name: &str,
    arguments: &Value,
    compact: bool,
) -> Result<Option<String>, Box<dyn Error>> {
    // Eval returns a JS result, not an NDA delta.
    if name == "browser_native_eval" {
        let expr = arguments["expression"]
            .as_str()
            .ok_or("expression is required")?;
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
        let target_name = arguments["name"]
            .as_str()
            .ok_or("name is required for wait_for")?;
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
                    }))
                    .unwrap_or_default()
                } else {
                    format!(
                        "Found element at node_{}\n---\n{}",
                        node_id,
                        render_view(&view)
                    )
                }
            }
            None => {
                if compact {
                    serde_json::to_string_pretty(&serde_json::json!({ "found": false }))
                        .unwrap_or_default()
                } else {
                    format!(
                        "Element with role={:?} name=\"{}\" not found within {}ms",
                        role, target_name, timeout
                    )
                }
            }
        }));
    }

    if name == "browser_native_extract" {
        let node_id = resolve_node(bridge, arguments)?;
        let what = arguments["what"].as_str().unwrap_or("text");
        let content = bridge.agent_extract(node_id, what);
        return Ok(Some(if compact {
            serde_json::to_string_pretty(&serde_json::json!({
                "nodeId": node_id,
                "what": what,
                "content": content
            }))
            .unwrap_or_default()
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
            return Ok(Some(
                serde_json::to_string_pretty(&items).unwrap_or_default(),
            ));
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
        let node_id = resolve_node(bridge, arguments)?;
        // Rolling auto-checkpoint: `_pre` always holds the state immediately
        // before the most recent action, so diff works without an explicit save.
        bridge.checkpoint_save("_pre");
        let result = bridge.agent_hover(node_id);
        let view = bridge.current_view();
        if compact {
            let report = ActionReport {
                status: result.status.clone(),
                delta: delta_report(&result.delta),
                view: view_report(&view),
                content_change: content_change_signal(&result.delta),
            };
            return Ok(Some(
                serde_json::to_string_pretty(&report).unwrap_or_default(),
            ));
        } else {
            let mut out = format!(
                "{}\nChanges:\n{}",
                result.status,
                render_delta(&result.delta)
            );
            out.push_str(&content_change_note(&result.delta));
            out.push_str(&render_view(&view));
            return Ok(Some(out));
        }
    }

    if name == "browser_native_press_key" {
        let key = arguments["key"].as_str().ok_or("key is required")?;
        // Rolling auto-checkpoint (see browser_native_hover).
        bridge.checkpoint_save("_pre");
        let result = bridge.agent_press_key(key);
        let view = bridge.current_view();
        if compact {
            let report = ActionReport {
                status: result.status.clone(),
                delta: delta_report(&result.delta),
                view: view_report(&view),
                content_change: content_change_signal(&result.delta),
            };
            return Ok(Some(
                serde_json::to_string_pretty(&report).unwrap_or_default(),
            ));
        } else {
            let mut out = format!(
                "{}\nChanges:\n{}",
                result.status,
                render_delta(&result.delta)
            );
            out.push_str(&content_change_note(&result.delta));
            out.push_str(&render_view(&view));
            return Ok(Some(out));
        }
    }

    // Rolling auto-checkpoint (see browser_native_hover): every action path
    // that reaches this dispatch leaves its pre-state diffable as `_pre`.
    bridge.checkpoint_save("_pre");

    let result = match name {
        "browser_native_navigate" => {
            let url = arguments["url"].as_str().ok_or("url is required")?;
            bridge.agent_navigate(url)
        }
        "browser_native_click" => {
            let node_id = resolve_node(bridge, arguments)?;
            bridge.agent_click(node_id)
        }
        "browser_native_type" => {
            let node_id = resolve_node(bridge, arguments)?;
            let text = arguments["text"].as_str().ok_or("text is required")?;
            bridge.agent_type(node_id, text)
        }
        "browser_native_select" => {
            let node_id = resolve_node(bridge, arguments)?;
            let value = arguments["value"].as_str().ok_or("value is required")?;
            bridge.agent_select(node_id, value)
        }
        "browser_native_submit" => {
            let node_id = resolve_node(bridge, arguments)?;
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

    // Score the observed outcome so browser_native_reflect can learn from it:
    // the signals come from the NDA delta the action actually produced.
    let (action, role, target) = outcome_descriptor(name, arguments);
    bridge.record_outcome(action, role, &target, &result);

    let view = bridge.current_view();
    if compact {
        let report = ActionReport {
            status: result.status.clone(),
            delta: delta_report(&result.delta),
            view: view_report(&view),
            content_change: content_change_signal(&result.delta),
        };
        Ok(Some(serde_json::to_string_pretty(&report).map_err(
            |e| format!("serialise native action report: {e}"),
        )?))
    } else {
        let mut out = String::new();
        out.push_str(&format!("{}\n", result.status));
        out.push_str("Changes:\n");
        out.push_str(&render_delta(&result.delta));
        out.push_str(&content_change_note(&result.delta));
        out.push_str("---\n");
        out.push_str(&render_view(&view));
        Ok(Some(out))
    }
}
