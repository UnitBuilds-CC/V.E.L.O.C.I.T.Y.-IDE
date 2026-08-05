//! Experience persistence: save/load the learned stores as NDA artifacts,
//! export the whole session state, and manage the tab swarm.

use serde_json::Value;
use std::error::Error;
use std::path::Path;

use crate::editor::browser::native_bridge::{
    encode_nda_triples, persist_browser_artifact, NativeBrowserBridge,
};

use super::*;

pub(super) fn handle_learn_tool(
    bridge: &mut NativeBrowserBridge,
    root: &Path,
    session_id: &str,
    name: &str,
    arguments: &Value,
    compact: bool,
) -> Result<Option<String>, Box<dyn Error>> {
    // Persist / restore the session's experience stores as NDA artifacts so
    // they survive across sessions instead of dying with the process:
    // what=confidence is the learned per-domain action confidence,
    // what=memory is the vector page memory. Both artifacts are the lossless
    // NdaDocument binary stream.
    if name == "browser_native_learn" {
        let action = arguments["action"].as_str().unwrap_or("save");
        let what = arguments["what"].as_str().unwrap_or("confidence");
        if !matches!(what, "confidence" | "memory" | "outcomes" | "all") {
            return Err(format!(
                "unknown learn store '{what}' (expected confidence, memory, outcomes or all)"
            )
            .into());
        }
        let default_file = format!("{session_id}_{what}.nda");
        let file_name = arguments["file"].as_str().unwrap_or(&default_file);
        match action {
            "save" => {
                // what=all bundles every experience store into one artifact;
                // the predicate ranges are disjoint so one document carries
                // all three losslessly.
                if what == "all" {
                    let mut doc = bridge.confidence.export_nda();
                    // Each confidence pattern is two facts.
                    let patterns = doc.facts.len() / 2;
                    doc.merge(&bridge.vector_memory.export_nda());
                    doc.merge(&bridge.scorer.export_nda());
                    let memories = bridge.memory_count();
                    let outcomes = bridge.scorer.history.len();
                    let path = persist_browser_artifact(root, file_name, &doc.to_binary_stream())?;
                    return Ok(Some(if compact {
                        serde_json::to_string_pretty(&serde_json::json!({
                            "action": "save",
                            "what": "all",
                            "path": path.display().to_string(),
                            "patterns": patterns,
                            "memories": memories,
                            "outcomes": outcomes,
                        }))
                        .map_err(|e| format!("serialise learn report: {e}"))?
                    } else {
                        format!(
                        "saved {patterns} learned pattern(s), {memories} page memory(ies) and {outcomes} action outcome(s) to {}\n",
                        path.display()
                    )
                    }));
                }
                let (doc, count, noun) = match what {
                    "confidence" => {
                        let doc = bridge.confidence.export_nda();
                        // Each pattern is two facts: confidence + observations.
                        let count = doc.facts.len() / 2;
                        (doc, count, "learned pattern(s)")
                    }
                    "memory" => (
                        bridge.vector_memory.export_nda(),
                        bridge.memory_count(),
                        "page memory(ies)",
                    ),
                    _ => (
                        bridge.scorer.export_nda(),
                        bridge.scorer.history.len(),
                        "action outcome(s)",
                    ),
                };
                let path = persist_browser_artifact(root, file_name, &doc.to_binary_stream())?;
                return Ok(Some(if compact {
                    serde_json::to_string_pretty(&serde_json::json!({
                        "action": "save",
                        "what": what,
                        "path": path.display().to_string(),
                        "count": count,
                    }))
                    .map_err(|e| format!("serialise learn report: {e}"))?
                } else {
                    format!("saved {count} {noun} to {}\n", path.display())
                }));
            }
            "load" => {
                let path = root
                    .join(".velocity")
                    .join("browser_artifacts")
                    .join(file_name);
                let bytes = std::fs::read(&path).map_err(|e| {
                    format!(
                        "failed to read learned patterns from {}: {e}",
                        path.display()
                    )
                })?;
                let doc = velocity_browser::NdaDocument::from_binary_stream(&bytes)
                    .map_err(|e| format!("invalid learned-pattern artifact: {e}"))?;
                if what == "all" {
                    // Each importer only consumes its own predicate range, so
                    // one bundled document restores all three stores.
                    let patterns = bridge.confidence.import_nda(&doc);
                    let memories = bridge.vector_memory.import_nda(&doc);
                    let outcomes = bridge.scorer.import_nda(&doc);
                    return Ok(Some(if compact {
                        serde_json::to_string_pretty(&serde_json::json!({
                            "action": "load",
                            "what": "all",
                            "path": path.display().to_string(),
                            "patterns": patterns,
                            "memories": memories,
                            "outcomes": outcomes,
                        }))
                        .map_err(|e| format!("serialise learn report: {e}"))?
                    } else {
                        format!(
                        "restored {patterns} learned pattern(s), {memories} page memory(ies) and {outcomes} action outcome(s) from {}\n",
                        path.display()
                    )
                    }));
                }
                if what == "outcomes" {
                    let restored = bridge.scorer.import_nda(&doc);
                    let total = bridge.scorer.history.len();
                    return Ok(Some(if compact {
                        serde_json::to_string_pretty(&serde_json::json!({
                            "action": "load",
                            "what": what,
                            "path": path.display().to_string(),
                            "restored": restored,
                            "outcomeCount": total,
                        }))
                        .map_err(|e| format!("serialise learn report: {e}"))?
                    } else {
                        format!(
                            "restored {} action outcome(s) from {}\n{} outcome(s) now recorded\n",
                            restored,
                            path.display(),
                            total
                        )
                    }));
                }
                if what == "memory" {
                    let restored = bridge.vector_memory.import_nda(&doc);
                    let total = bridge.memory_count();
                    return Ok(Some(if compact {
                        serde_json::to_string_pretty(&serde_json::json!({
                            "action": "load",
                            "what": what,
                            "path": path.display().to_string(),
                            "restored": restored,
                            "memoryCount": total,
                        }))
                        .map_err(|e| format!("serialise learn report: {e}"))?
                    } else {
                        format!(
                            "restored {} page memory(ies) from {}\n{} memory(ies) now stored\n",
                            restored,
                            path.display(),
                            total
                        )
                    }));
                }
                let restored = bridge.confidence.import_nda(&doc);
                let patterns = bridge.confidence_report();
                if compact {
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
                            "action": "load",
                            "what": what,
                            "path": path.display().to_string(),
                            "restored": restored,
                            "patterns": pattern_json,
                        }))
                        .map_err(|e| format!("serialise learn report: {e}"))?,
                    ));
                }
                let mut out = format!(
                    "restored {} learned pattern(s) from {}\n",
                    restored,
                    path.display()
                );
                if !patterns.is_empty() {
                    out.push_str("learned patterns on this domain:\n");
                    for (role, action, conf, obs) in &patterns {
                        out.push_str(&format!("  {action} on {role}: {conf:.2} ({obs} obs)\n"));
                    }
                }
                return Ok(Some(out));
            }
            "list" => {
                // Discover inheritable experience: enumerate every artifact in
                // the workspace so an agent can pick a file= to load from a
                // previous session without knowing its id in advance.
                let dir = root.join(".velocity").join("browser_artifacts");
                let mut artifacts: Vec<(String, String, u64)> = Vec::new();
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let meta = match entry.metadata() {
                            Ok(m) if m.is_file() => m,
                            _ => continue,
                        };
                        let file = entry.file_name().to_string_lossy().into_owned();
                        let kind = if file.ends_with("_confidence.nda") {
                            "confidence"
                        } else if file.ends_with("_memory.nda") {
                            "memory"
                        } else if file.ends_with("_outcomes.nda") {
                            "outcomes"
                        } else if file.ends_with("_all.nda") {
                            "all"
                        } else if file.ends_with("_native.nda") {
                            "state"
                        } else if file.ends_with("_trace.nda") {
                            "trace"
                        } else if file.ends_with("_facts.txt") {
                            "facts"
                        } else {
                            "other"
                        };
                        artifacts.push((file, kind.to_string(), meta.len()));
                    }
                }
                artifacts.sort_by(|a, b| a.0.cmp(&b.0));
                if compact {
                    let artifact_json: Vec<serde_json::Value> = artifacts
                        .iter()
                        .map(|(file, kind, bytes)| {
                            serde_json::json!({
                                "file": file,
                                "kind": kind,
                                "bytes": bytes,
                            })
                        })
                        .collect();
                    return Ok(Some(
                        serde_json::to_string_pretty(&serde_json::json!({
                            "action": "list",
                            "path": dir.display().to_string(),
                            "artifacts": artifact_json,
                        }))
                        .map_err(|e| format!("serialise learn report: {e}"))?,
                    ));
                }
                if artifacts.is_empty() {
                    return Ok(Some("(no browser artifacts saved yet)\n".to_string()));
                }
                let mut out = format!("{} artifact(s) in {}:\n", artifacts.len(), dir.display());
                for (file, kind, bytes) in &artifacts {
                    out.push_str(&format!("  {file} ({kind}, {bytes} bytes)\n"));
                }
                out.push_str("load one with action=load file=<name>\n");
                return Ok(Some(out));
            }
            other => {
                return Err(
                    format!("unknown learn action '{other}' (expected save, load or list)").into(),
                )
            }
        }
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
            other => {
                return Err(format!(
                    "unknown export format '{other}' (expected binary, readable, or trace)"
                )
                .into())
            }
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
            let mut report = serde_json::json!({ "status": status, "tabs": tab_json(bridge) });
            if switched {
                report["view"] = serde_json::to_value(view_report(&bridge.current_view()))
                    .map_err(|e| format!("serialise tab view: {e}"))?;
            }
            serde_json::to_string_pretty(&report)
                .map_err(|e| format!("serialise tab report: {e}"))?
        } else {
            let mut out = format!("{status}\n");
            out.push_str(&tab_lines(bridge));
            if switched {
                out.push_str("---\n");
                out.push_str(&render_view(&bridge.current_view()));
            }
            out
        }));
    }
    Ok(None)
}
