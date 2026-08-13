//! Native-engine browser tools.
//!
//! Unlike the static-crawl and runtime-capture tool families (which fetch a
//! page and hand back a metadata snapshot), this family drives the pure-Rust
//! [`velocity_browser::BrowserSession`] directly: it navigates over rustls
//! HTTPS, exposes the *live* Agentic Object Model, and lets the agent act on
//! elements by node id or by role + accessible name. Every action returns the
//! readable NDA delta it produced alongside the refreshed page view, so acting
//! and observing are inseparable.

use serde_json::Value;
use std::error::Error;
use std::path::Path;

use crate::editor::browser::native_bridge::{get_or_create_native_bridge, NativeBrowserBridge};

mod assert;
mod render;
mod wait;

// The glob imports keep the dispatcher and the test module referring to the
// helpers by their plain names, exactly as before the module split.
use assert::*;
use render::*;
use wait::*;

mod actions;
mod inspect;
mod learn;

/// similar targets so the reflection engine can spot repeated failures
/// ("fill on textbox keeps failing") instead of one-off misses.
fn outcome_descriptor(name: &str, arguments: &Value) -> (&'static str, &'static str, String) {
    let text = |k: &str| arguments[k].as_str().unwrap_or("").to_string();
    let node_target = arguments["nodeId"]
        .as_u64()
        .map(|n| format!("node_{n}"))
        .unwrap_or_else(|| text("name"));
    match name {
        "browser_native_navigate" => ("navigate", "page", text("url")),
        "browser_native_click" => ("click", "node", node_target),
        "browser_native_type" => ("fill", "node", node_target),
        "browser_native_select" => ("select", "node", node_target),
        "browser_native_submit" => ("submit", "node", node_target),
        "browser_native_scroll" | "browser_native_scroll_into_view" => {
            ("scroll", "viewport", text("label"))
        }
        "browser_native_back" | "browser_native_forward" => ("navigate", "history", String::new()),
        "browser_native_click_text" => ("click", "clickable", text("text")),
        "browser_native_fill_label" => ("fill", "textbox", text("label")),
        "browser_native_check_label" => ("check", "checkbox", text("label")),
        "browser_native_select_label" => ("select", "combobox", text("label")),
        "browser_native_focus_label" => ("focus", "control", text("label")),
        "browser_native_press" => ("press", "keyboard", text("key")),
        _ => ("settle", "page", String::new()),
    }
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
        | "browser_native_reflect"
        | "browser_native_predict"
        | "browser_native_brief"
        | "browser_native_learn"
        | "browser_native_tab_open"
        | "browser_native_tab_list"
        | "browser_native_tab_switch"
        | "browser_native_tab_close"
        | "browser_native_settle"
        | "browser_native_assert"
        | "browser_native_wait" => arguments["sessionId"]
            .as_str()
            .ok_or("sessionId is required")?,
        _ => return Ok(None),
    };
    let compact = arguments["compact"].as_bool().unwrap_or(false);
    // Wait polls the session with the lock released between checks, so it
    // runs before the handler-wide guard is taken.
    if name == "browser_native_wait" {
        return wait_on_session(session_id, arguments, compact);
    }
    // Assert can poll with a waitMs grace period, so it likewise bypasses
    // the handler-wide guard and re-locks per poll.
    if name == "browser_native_assert" {
        return assert_on_session(root, session_id, arguments, compact);
    }
    let bridge = get_or_create_native_bridge(session_id);
    let mut bridge = bridge
        .lock()
        .map_err(|_| "native browser bridge lock poisoned")?;
    // First touch of a session inherits the workspace-default experience
    // bundle (default_all.nda) if one was saved, so learned patterns, page
    // memories and outcome lessons carry over without an explicit load call.
    bridge.seed_default_experience(root);

    if let Some(res) = inspect::handle_inspect_tool(&mut bridge, root, name, arguments, compact)? {
        return Ok(Some(res));
    }

    // One call that assembles everything the agent should know before acting:
    // the page identity, this domain's learned patterns, the suggested next
    // action, similar remembered pages, failure lessons and recent outcomes.
    // Replaces a predict + recall + reflect round-trip, saving tokens.
    if name == "browser_native_brief" {
        let memory_limit = arguments["memories"].as_u64().unwrap_or(3) as usize;
        let recent_n = arguments["recent"].as_u64().unwrap_or(5) as usize;
        let view = bridge.current_view();
        // Structure digest: element counts + heading outline, minus the
        // "Page:" identity line the brief header already carries.
        let digest: String = bridge
            .page_summary_text()
            .lines()
            .skip(1)
            .collect::<Vec<_>>()
            .join("\n");
        let patterns = bridge.confidence_report();
        let suggestion = bridge.predict_learned();
        let detail = suggestion
            .as_ref()
            .and_then(|p| view.elements.iter().find(|e| e.aom_id == p.target_selector));
        // Similar past pages: the current page text is the semantic query.
        let page_query = bridge.page_text();
        // Distilled content size: lets the agent notice silent page growth
        // between turns without diffing or re-reading the whole page.
        let mut content = bridge.page_content_markdown();
        if content.is_empty() {
            content = bridge.page_markdown();
        }
        let content_chars = content.chars().count();
        let memories = if page_query.trim().is_empty() {
            Vec::new()
        } else {
            bridge.recall_pages(&page_query, "semantic", memory_limit, 0.0)
        };
        let reflections = bridge.reflect();
        // What the most recent action changed: the rolling `_pre` checkpoint
        // exists once any action has run, so a returning agent learns the
        // effect of its previous turn in the same call it re-orients with.
        let last_change = bridge.checkpoint_diff("_pre").map(|d| {
            (
                d.added.len(),
                d.removed.len(),
                d.changed.len(),
                content_change_signal(&d),
            )
        });
        // Assert-guard health: pass/fail counts over the outcome history,
        // plus the most-missed target, so a returning agent sees whether
        // its recent guards are drifting — and which one — without
        // re-reading the raw outcome lines.
        let mut guards_passed = 0usize;
        let mut guards_failed = 0usize;
        let mut failed_targets: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for o in &bridge.scorer.history {
            if o.action_kind.label() != "assert" {
                continue;
            }
            if o.score < 0.3 {
                guards_failed += 1;
                *failed_targets.entry(&o.target_selector).or_default() += 1;
            } else {
                guards_passed += 1;
            }
        }
        let most_missed: Option<(&str, usize)> = failed_targets
            .iter()
            .max_by_key(|(_, n)| **n)
            .map(|(target, n)| (*target, *n));
        if compact {
            let sugg_json = suggestion.as_ref().map(|p| {
                serde_json::json!({
                    "target": p.target_selector,
                    "action": p.action_type,
                    "confidence": ((p.confidence_score as f64) * 100.0).round() / 100.0,
                    "role": detail.map(|e| e.role.clone()),
                    "name": detail.map(|e| e.name.clone()),
                })
            });
            let pattern_json: Vec<serde_json::Value> = patterns
                .iter()
                .map(|(role, action, conf, obs)| {
                    serde_json::json!({
                        "role": role,
                        "action": action,
                        "confidence": (conf * 100.0).round() / 100.0,
                        "observations": obs,
                    })
                })
                .collect();
            let memory_json: Vec<serde_json::Value> = memories
                .iter()
                .map(|(n, sim)| {
                    serde_json::json!({
                        "url": n.url,
                        "similarity": sim,
                        "outcome": n.outcome_score,
                        "snippet": memory_snippet(&n.text),
                    })
                })
                .collect();
            let reflection_json: Vec<serde_json::Value> = reflections
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "category": format!("{:?}", r.category),
                        "message": r.message,
                        "strategy": r.suggested_strategy,
                    })
                })
                .collect();
            let outcome_json: Vec<serde_json::Value> = bridge
                .scorer
                .recent_context(recent_n)
                .iter()
                .map(|o| {
                    serde_json::json!({
                        "action": o.action_kind.label(),
                        "role": o.target_role,
                        "score": (o.score * 100.0).round() / 100.0,
                        "error": o.signals.error_thrown,
                    })
                })
                .collect();
            return Ok(Some(
                serde_json::to_string_pretty(&serde_json::json!({
                    "url": view.url,
                    "title": view.title,
                    "elements": view.elements.len(),
                    "contentChars": (content_chars > 0).then_some(content_chars),
                    "lastChange": last_change.as_ref().map(|(added, removed, changed, cc)| {
                        serde_json::json!({
                            "added": added,
                            "removed": removed,
                            "changed": changed,
                            "contentChange": cc.map(|(from, to)| serde_json::json!([from, to])),
                        })
                    }),
                    "guards": ((guards_passed + guards_failed) > 0).then(|| {
                        serde_json::json!({
                            "passed": guards_passed,
                            "failed": guards_failed,
                            "mostMissed": most_missed.map(|(target, n)| {
                                serde_json::json!({ "target": target, "count": n })
                            }),
                        })
                    }),
                    "digest": (!digest.is_empty()).then_some(digest.as_str()),
                    "suggestion": sugg_json,
                    "patterns": pattern_json,
                    "memories": memory_json,
                    "reflections": reflection_json,
                    "outcomes": outcome_json,
                }))
                .map_err(|e| format!("serialise brief report: {e}"))?,
            ));
        }
        let mut out = format!(
            "brief for {} \u{2014} \"{}\" ({} interactive element(s))\n",
            view.url,
            view.title,
            view.elements.len()
        );
        if !digest.is_empty() {
            out.push_str(&digest);
            out.push('\n');
        }
        if content_chars > 0 {
            out.push_str(&format!("Content: {} chars\n", content_chars));
        }
        if let Some((added, removed, changed, cc)) = last_change {
            let mut line = format!(
                "Last action: {} added, {} removed, {} changed fact(s)",
                added, removed, changed
            );
            if let Some((from, to)) = cc {
                line.push_str(&format!("; content {} -> {} chars", from, to));
            }
            out.push_str(&line);
            out.push('\n');
        }
        if guards_passed + guards_failed > 0 {
            let mut line = format!(
                "Guards: {} passed, {} failed assert(s)",
                guards_passed, guards_failed
            );
            if let Some((target, n)) = most_missed {
                line.push_str(&format!("; most missed: \"{target}\" ({n}x)"));
            }
            out.push_str(&line);
            out.push('\n');
        }
        match (&suggestion, detail) {
            (Some(p), Some(e)) => out.push_str(&format!(
                "suggested next action: {} {} [{}] \"{}\" (confidence {:.2})\n",
                p.action_type, p.target_selector, e.role, e.name, p.confidence_score
            )),
            (Some(p), None) => out.push_str(&format!(
                "suggested next action: {} {} (confidence {:.2})\n",
                p.action_type, p.target_selector, p.confidence_score
            )),
            (None, _) => {}
        }
        if !patterns.is_empty() {
            out.push_str("learned patterns on this domain:\n");
            for (role, action, conf, obs) in &patterns {
                out.push_str(&format!("  {action} on {role}: {conf:.2} ({obs} obs)\n"));
            }
        }
        if !memories.is_empty() {
            out.push_str("similar remembered pages:\n");
            for (n, sim) in &memories {
                let score = sim
                    .map(|s| format!("{s:.3}"))
                    .unwrap_or_else(|| "-".to_string());
                out.push_str(&format!(
                    "  [{}] {} (outcome {:.2}) {}\n",
                    score,
                    if n.url.is_empty() {
                        "(no url)"
                    } else {
                        n.url.as_str()
                    },
                    n.outcome_score,
                    memory_snippet(&n.text),
                ));
            }
        }
        if let Some(msg) = bridge.reflector.format_as_system_message(&reflections) {
            out.push_str(&format!("{msg}\n"));
        }
        let context = bridge.scorer.format_for_context(recent_n);
        if !context.is_empty() {
            out.push_str(&context);
        }
        return Ok(Some(out));
    }

    if let Some(res) =
        learn::handle_learn_tool(&mut bridge, root, session_id, name, arguments, compact)?
    {
        return Ok(Some(res));
    }

    actions::handle_action_tool(&mut bridge, name, arguments, compact)
}

#[cfg(test)]
#[path = "native/tests.rs"]
mod native_label_tool_tests;
