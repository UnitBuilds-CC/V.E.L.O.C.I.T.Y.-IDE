//! Read-only inspection tools of the native browser family: page reads,
//! structure digests, search, navigation history, checkpoints, memory
//! recall and form validation.

use serde_json::Value;
use std::error::Error;
use std::path::Path;

use crate::editor::browser::native_bridge::NativeBrowserBridge;

use super::*;

pub(super) fn handle_inspect_tool(
    bridge: &mut NativeBrowserBridge,
    root: &Path,
    name: &str,
    arguments: &Value,
    compact: bool,
) -> Result<Option<String>, Box<dyn Error>> {
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
    // collapsed, scripts/styles skipped. format switches to the engine's
    // distilled projections (markdown structure, tables, page summary) and
    // maxChars keeps huge pages bounded.
    if name == "browser_native_page_text" {
        let format = arguments["format"].as_str().unwrap_or("text");
        let (text, empty_msg) = match format {
        "text" => (bridge.page_text(), "(no visible text on page)"),
        "markdown" => (bridge.page_markdown(), "(no content to render as markdown)"),
        "content" => (bridge.page_content_markdown(), "(no main content on page)"),
        "tables" => (bridge.page_tables_text(), "(no tables on page)"),
        "summary" => (bridge.page_summary_text(), "(nothing to summarize)"),
        other => {
            return Err(format!(
                "unknown page_text format '{other}' (expected text, markdown, content, tables or summary)"
            )
            .into())
        }
    };
        if text.trim().is_empty() {
            return Ok(Some(empty_msg.to_string()));
        }
        let max_chars = arguments["maxChars"].as_u64().unwrap_or(0) as usize;
        if max_chars > 0 && text.chars().count() > max_chars {
            let truncated: String = text.chars().take(max_chars).collect();
            return Ok(Some(format!(
                "{truncated}\u{2026}\n(truncated to {max_chars} of {} chars)",
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
                "captured frame {idx} ({elements} elements, hash {hash:016x}) \u{2014} {total} frame{} in timeline\n",
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
                            f.frame_idx,
                            f.width,
                            f.height,
                            f.element_count,
                            f.frame_hash,
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
                out.push_str(&format!(
                    "  \u{2026} {} more (raise limit)\n",
                    matched - hits.len()
                ));
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
                    if text.is_empty() {
                        "(no text)"
                    } else {
                        text.as_str()
                    },
                    href,
                ));
            }
            if matched > links.len() {
                out.push_str(&format!(
                    "  \u{2026} {} more (raise limit)\n",
                    matched - links.len()
                ));
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
                return Err(
                    format!("unknown checkpoint action '{other}' (save, diff, list, drop)").into(),
                )
            }
        }));
    }

    // Failure-pattern lessons scored from real observations: what has the
    // agent been trying that keeps not working, and what to try instead.
    if name == "browser_native_reflect" {
        let recent_n = arguments["recent"].as_u64().unwrap_or(5) as usize;
        let reflections = bridge.reflect();
        if compact {
            let items: Vec<serde_json::Value> = reflections
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "category": format!("{:?}", r.category),
                        "message": r.message,
                        "confidence": r.confidence,
                        "strategy": r.suggested_strategy,
                    })
                })
                .collect();
            let outcomes: Vec<serde_json::Value> = bridge
                .scorer
                .recent_context(recent_n)
                .iter()
                .map(|o| {
                    serde_json::json!({
                        "action": o.action_kind.label(),
                        "role": o.target_role,
                        "target": o.target_selector,
                        "url": o.page_url,
                        "score": (o.score * 100.0).round() / 100.0,
                        "error": o.signals.error_thrown,
                    })
                })
                .collect();
            return Ok(Some(
                serde_json::to_string_pretty(&serde_json::json!({
                    "reflections": items,
                    "outcomes": outcomes,
                }))
                .map_err(|e| format!("serialise reflect report: {e}"))?,
            ));
        }
        let mut out = match bridge.reflector.format_as_system_message(&reflections) {
            Some(msg) => format!("{msg}\n"),
            None => "(no failure patterns detected)\n".to_string(),
        };
        let context = bridge.scorer.format_for_context(recent_n);
        if !context.is_empty() {
            out.push_str("---\n");
            out.push_str(&context);
        }
        return Ok(Some(out));
    }

    // "What should I try next?" — the learned per-domain confidence ranks the
    // page's actionable elements; before any history exists it falls back to
    // a conservative default instead of a hardcoded optimism.
    if name == "browser_native_predict" {
        let suggestion = bridge.predict_learned();
        let patterns = bridge.confidence_report();
        let view = bridge.current_view();
        // Enrich the node_N selector with the element's role and name.
        let detail = suggestion
            .as_ref()
            .and_then(|p| view.elements.iter().find(|e| e.aom_id == p.target_selector));
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
            return Ok(Some(
                serde_json::to_string_pretty(&serde_json::json!({
                    "suggestion": sugg_json,
                    "patterns": pattern_json,
                }))
                .map_err(|e| format!("serialise predict report: {e}"))?,
            ));
        }
        let mut out = match (&suggestion, detail) {
            (Some(p), Some(e)) => format!(
                "suggested next action: {} {} [{}] \"{}\" (confidence {:.2})\n",
                p.action_type, p.target_selector, e.role, e.name, p.confidence_score
            ),
            (Some(p), None) => format!(
                "suggested next action: {} {} (confidence {:.2})\n",
                p.action_type, p.target_selector, p.confidence_score
            ),
            (None, _) => "(no actionable elements to predict from)\n".to_string(),
        };
        if !patterns.is_empty() {
            out.push_str("learned patterns on this domain:\n");
            for (role, action, conf, obs) in &patterns {
                out.push_str(&format!("  {action} on {role}: {conf:.2} ({obs} obs)\n"));
            }
        }
        return Ok(Some(out));
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
                out.push_str(&format!("  [{}] \"{}\": {}\n", id, name, failed.join(", ")));
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
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
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
            "remembered page as '{}' ({} chars from {}, tags [{}], outcome {:.2}) \u{2014} {} memor{} stored\n",
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
        let min_outcome = arguments["minOutcome"]
            .as_f64()
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
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
                let score = sim
                    .map(|s| format!("{s:.3}"))
                    .unwrap_or_else(|| "-".to_string());
                out.push_str(&format!(
                    "  [{}] {} {} tags [{}] outcome {:.2}\n      {}\n",
                    score,
                    n.id,
                    if n.url.is_empty() {
                        "(no url)"
                    } else {
                        n.url.as_str()
                    },
                    n.tags.join(", "),
                    n.outcome_score,
                    memory_snippet(&n.text),
                ));
            }
            out
        }));
    }
    Ok(None)
}
