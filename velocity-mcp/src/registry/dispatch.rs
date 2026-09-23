use super::browser_tools::handle_browser_tool;
use super::custom_tools;
use super::event_store;
use super::generation_tools::handle_generation_tool;
use super::system_tools::handle_system_tool;
use super::team_tools::handle_team_tool;
use super::wa_tools::handle_wa_tool;
use crate::agent::drone_bridge::handle_drone_tool;
use crate::errors::ToolError;
use crate::security::audit::{self, ToolAuditOutcome};
use serde_json::Value;
use std::error::Error;
use std::path::Path;
use std::time::Instant;

pub fn call_tool_in_workspace(
    root: &Path,
    name: &str,
    arguments: &Value,
) -> Result<String, Box<dyn Error>> {
    let root = root.canonicalize().map_err(|e| {
        log::error!(
            "tool dispatch: failed to canonicalize workspace root: {}",
            e
        );
        e
    })?;

    // Governance gate: deny or park-for-approval per the workspace policy. With
    // no policy configured this allows everything (no behavior change).
    crate::editor::governance::gate_tool_call(&root, name, arguments).map_err(|e| {
        log::warn!("tool dispatch: governance denied '{}': {}", name, e);
        e
    })?;

    let start = Instant::now();

    // Capture the site map Merkle root *before* the tool call so we can
    // detect state changes and record codebase events.
    let merkle_before = read_sitemap_merkle_root(&root);

    // Try built-in tool handlers first (in priority order).
    let builtin = try_builtin_tools(&root, name, arguments);

    match &builtin {
        Ok(Some(output)) => {
            // Capture the site map Merkle root after the tool call.
            let merkle_after = read_sitemap_merkle_root(&root);
            // Bug #42: `Ok` only means the handler ran. Every tool that returns
            // a payload saying `"success": false` - including the bug #40
            // session refusals - was logged as a success, so the one record of
            // what touched a live session claimed the opposite of what happened.
            let refused = in_band_failure(output);
            let audit_outcome = match &refused {
                None => ToolAuditOutcome::Success,
                Some(note) => ToolAuditOutcome::Error(note.clone()),
            };
            audit::record_tool_call_full(
                "stdio",
                name,
                start,
                audit_outcome,
                merkle_after.map(|r| format!("{:016x}", r)),
            );
            // ── Universal codebase event recording ─────────────────────
            // Every tool call is recorded in the event store, with the verdict
            // the tool actually gave. Context (the *why*) is read generically
            // from the tool call arguments so agents cannot forget to declare
            // their changes. Useless events (grep, list_dir) can be pruned
            // later; the important ones (write, delete, index, edit) are kept.
            record_dispatch_event(
                &root,
                name,
                arguments,
                merkle_before,
                merkle_after,
                match &refused {
                    None => event_store::EventOutcome::Success,
                    Some(_) => event_store::EventOutcome::Failure,
                },
                refused,
            );
            // Flush audit log after site-map-mutating tools.
            if is_sitemap_tool(name) {
                let _ = audit::flush_all_sessions_to_dir(&root.join(".velocity"));
            }
            // Enrich read_file responses with the decision trail so the
            // agent sees *why* code exists, not just *what* it contains.
            if name == "read_file" {
                if let Some(rel_path) = arguments["relativeFilePath"].as_str() {
                    let enriched = event_store::enrich_read_response(&root, rel_path, output);
                    return Ok(enriched);
                }
            }
            Ok(output.clone())
        }
        Ok(None) => {
            // No built-in handler — try custom tools.
            let custom_result = try_custom_tool(&root, name, arguments);
            let outcome = match &custom_result {
                Ok(output) => match in_band_failure(output) {
                    None => ToolAuditOutcome::Success,
                    Some(note) => ToolAuditOutcome::Error(note),
                },
                Err(e) => ToolAuditOutcome::Error(e.to_string()),
            };
            audit::record_tool_call("stdio", name, start, outcome);
            let merkle_after = read_sitemap_merkle_root(&root);
            match &custom_result {
                Ok(output) => {
                    let refused = in_band_failure(output);
                    record_dispatch_event(
                        &root,
                        name,
                        arguments,
                        merkle_before,
                        merkle_after,
                        match &refused {
                            None => event_store::EventOutcome::Success,
                            Some(_) => event_store::EventOutcome::Failure,
                        },
                        refused,
                    )
                }
                Err(e) => record_dispatch_event(
                    &root,
                    name,
                    arguments,
                    merkle_before,
                    merkle_after,
                    event_store::EventOutcome::Failure,
                    Some(e.to_string()),
                ),
            }
            custom_result
        }
        Err(e) => {
            audit::record_tool_call("stdio", name, start, ToolAuditOutcome::Error(e.to_string()));
            // Failed calls belong in the decision trail too: an agent that only
            // sees successes will repeat experiments that already broke.
            let merkle_after = read_sitemap_merkle_root(&root);
            record_dispatch_event(
                &root,
                name,
                arguments,
                merkle_before,
                merkle_after,
                event_store::EventOutcome::Failure,
                Some(e.to_string()),
            );
            Err(e.to_string().into())
        }
    }
}

/// How long a recorded refusal note may be.
const MAX_FAILURE_NOTE_CHARS: usize = 200;

/// The failure verdict inside a handler's own successful return value.
///
/// A tool can be *run* without the operation it was asked for happening: the
/// payload carries `"success": false` plus the reason. `None` means the output
/// asserts no failure, which covers plain-text tools, JSON without a `success`
/// field, and output that cannot be parsed at all - guessing at a failure from
/// unparseable text would be the mirror image of bug #42.
fn in_band_failure(output: &str) -> Option<String> {
    let trimmed = output.trim();
    if !trimmed.starts_with('{') {
        return None;
    }
    let parsed: Value = serde_json::from_str(trimmed).ok()?;
    // Only an explicit boolean `false` is a verdict. A missing or differently
    // typed `success` leaves the historic behaviour untouched.
    match parsed.get("success") {
        Some(Value::Bool(false)) => {
            let note = ["detail", "error", "message", "reason"]
                .iter()
                .find_map(|key| parsed.get(*key).and_then(Value::as_str))
                .unwrap_or("tool reported success:false");
            Some(clip(note, MAX_FAILURE_NOTE_CHARS))
        }
        _ => None,
    }
}

/// Append an automatic dispatch event to the codebase event store, resolving
/// its outcome in the same write.
fn record_dispatch_event(
    root: &Path,
    name: &str,
    arguments: &Value,
    merkle_before: Option<u64>,
    merkle_after: Option<u64>,
    outcome: event_store::EventOutcome,
    failure_reason: Option<String>,
) {
    let ctx = arguments["context"].as_str();
    let affected = extract_affected_files(name, arguments);
    let description = build_event_description(name, arguments);
    let store = event_store::EventStore::open(root);
    let _ = store.record_with_outcome(
        name,
        &description,
        merkle_before.map(|r| format!("{:016x}", r)),
        merkle_after.map(|r| format!("{:016x}", r)),
        ctx.map(|s| s.to_string()),
        affected,
        outcome,
        failure_reason,
    );
}

/// Try all built-in tool handlers. Returns `Ok(None)` if no handler recognizes
/// the tool name.
fn try_builtin_tools(
    root: &Path,
    name: &str,
    arguments: &Value,
) -> Result<Option<String>, Box<dyn Error>> {
    if let Some(result) = handle_system_tool(root, name, arguments)? {
        return Ok(Some(result));
    }
    if let Some(result) = handle_team_tool(root, name, arguments)? {
        return Ok(Some(result));
    }
    if let Some(result) = handle_browser_tool(root, name, arguments)? {
        return Ok(Some(result));
    }
    if let Some(result) = handle_wa_tool(root, name, arguments)? {
        return Ok(Some(result));
    }
    if let Some(result) = handle_drone_tool(root, name, arguments)? {
        return Ok(Some(result));
    }
    if let Some(result) = handle_generation_tool(root, name, arguments)? {
        return Ok(Some(result));
    }
    Ok(None)
}

/// Try to execute a dynamically registered custom tool.
fn try_custom_tool(root: &Path, name: &str, arguments: &Value) -> Result<String, Box<dyn Error>> {
    if let Some(tool) = custom_tools::get_tool(root, name) {
        custom_tools::execute_tool(root, &tool, arguments)
    } else {
        log::warn!("tool dispatch: unknown tool '{}'", name);
        // Return the structured variant rather than an opaque string: callers
        // (the JSON-RPC layer, the parity tests, plugin bridges) branch on the
        // failure mode instead of matching message text.
        Err(Box::new(ToolError::ToolNotFound(name.to_string())))
    }
}

/// Read the current site map Merkle root from the workspace's persisted metadata.
/// Returns None if the site map doesn't exist or can't be read.
fn read_sitemap_merkle_root(root: &Path) -> Option<u64> {
    let sitemap_dir = root.join(".velocity").join("site_map");
    let index_path = sitemap_dir.join("index.json");
    if !index_path.exists() {
        return None;
    }
    velocity_ide::site_map::SiteMap::read_persisted_root(&sitemap_dir)
}

/// Returns true if the tool is known to mutate the site map index.
fn is_sitemap_tool(name: &str) -> bool {
    matches!(name, "index_workspace")
}

/// Extract affected file paths from tool arguments (best-effort).
fn extract_affected_files(name: &str, arguments: &Value) -> Vec<String> {
    // Tools that carry a file path in a known argument.
    if let Some(path) = arguments["relativeFilePath"].as_str() {
        return vec![path.to_string()];
    }
    if let Some(path) = arguments["filePath"].as_str() {
        return vec![path.to_string()];
    }
    if let Some(path) = arguments["path"].as_str() {
        // index_workspace, knowledge_ingest, etc.
        if matches!(name, "index_workspace" | "knowledge_ingest") {
            return vec![path.to_string()];
        }
    }
    Vec::new()
}

/// Build a human-readable event description from the tool name and arguments.
fn build_event_description(name: &str, arguments: &Value) -> String {
    match name {
        "write_file" => {
            let path = arguments["relativeFilePath"].as_str().unwrap_or("unknown");
            format!("Wrote {}", path)
        }
        "delete_file" => {
            let path = arguments["relativeFilePath"].as_str().unwrap_or("unknown");
            format!("Deleted {}", path)
        }
        "read_file" => {
            let path = arguments["relativeFilePath"].as_str().unwrap_or("unknown");
            format!("Read {}", path)
        }
        "index_workspace" => {
            let path = arguments["path"].as_str().unwrap_or("entire workspace");
            format!("Indexed {}", path)
        }
        "grep_search" => {
            let query = arguments["query"].as_str().unwrap_or("?");
            format!("Searched for '{}'", query)
        }
        "run_command" => {
            let cmd = arguments["command"].as_str().unwrap_or("?");
            format!("Ran command: {}", clip(cmd, 60))
        }
        "knowledge_ingest" => {
            let source = arguments["source"]
                .as_str()
                .or_else(|| arguments["path"].as_str())
                .unwrap_or("unknown");
            format!("Ingested knowledge from {}", source)
        }
        // Bug #41: everything unlisted was recorded as a bare "Called <name>",
        // which cannot answer the question the log exists for - *what was that
        // call asked to do*. `wa_virtual_desktop_switch` moving the operator to
        // another desktop left a trail saying only that the tool was called.
        _ => {
            let summary = summarize_arguments(arguments);
            if summary.is_empty() {
                format!("Called {name}")
            } else {
                format!("Called {name}: {summary}")
            }
        }
    }
}

/// How many argument pairs and how many characters of each reach the log.
///
/// Bounded because the event store is append-only and read in full by the
/// health and coverage passes; an unbounded argument dump turns every audit
/// read into a content review.
const MAX_SUMMARISED_ARGS: usize = 6;
const MAX_ARG_VALUE_CHARS: usize = 80;
const MAX_SUMMARY_CHARS: usize = 400;

/// Keys whose values must never be written to the event log.
///
/// Two different reasons, same handling: credentials (`token`, `api_key`,
/// `password`, ...) because the log is plaintext and shared, and bulk payloads
/// (`content`, `text`, `diff`, ...) because a truncated copy is neither
/// attributable nor free.
fn elide_argument(key: &str) -> Option<&'static str> {
    const SECRET_MARKERS: [&str; 7] = [
        "token",
        "password",
        "secret",
        "apikey",
        "api_key",
        "credential",
        "private",
    ];
    const BULK_MARKERS: [&str; 6] = ["content", "text", "body", "diff", "patch", "base64"];
    let lowered = key.to_ascii_lowercase();
    if SECRET_MARKERS.iter().any(|m| lowered.contains(m)) {
        Some("[redacted]")
    } else if BULK_MARKERS.iter().any(|m| lowered.contains(m)) {
        Some("[elided]")
    } else {
        None
    }
}

/// Clip to `max` *characters*, not bytes.
///
/// The previous byte-slicing preview panicked mid-codepoint on any non-ASCII
/// command, which is an ordinary thing to run.
fn clip(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let kept: String = value.chars().take(max).collect();
    format!("{kept}~")
}

/// One-line, bounded rendering of the arguments a tool was called with.
fn summarize_arguments(arguments: &Value) -> String {
    let Value::Object(map) = arguments else {
        return String::new();
    };
    let mut parts: Vec<String> = Vec::new();
    let mut dropped = 0usize;
    for (key, value) in map {
        if parts.len() == MAX_SUMMARISED_ARGS {
            dropped += 1;
            continue;
        }
        let rendered = match elide_argument(key) {
            Some(marker) => (*marker).to_string(),
            None => clip(&value.to_string(), MAX_ARG_VALUE_CHARS),
        };
        parts.push(format!("{key}={rendered}"));
    }
    let mut summary = clip(&parts.join(" "), MAX_SUMMARY_CHARS);
    // Admitting that arguments were left out outranks the character cap, so the
    // marker is appended after clipping instead of competing for the budget.
    if dropped > 0 {
        summary.push_str(&format!(" (+{dropped} more)"));
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Bug #41: the event log is the only record of what ran when the operator's
    /// desktop moved, and until now it said nothing beyond the tool name.
    #[test]
    fn an_unlisted_call_records_the_arguments_it_was_given() {
        let desc = build_event_description(
            "wa_virtual_desktop_switch",
            &json!({ "index": 7, "name": "sweep" }),
        );
        assert!(
            desc.starts_with("Called wa_virtual_desktop_switch:"),
            "{desc}"
        );
        assert!(desc.contains("index=7"), "{desc}");
        assert!(desc.contains("name=\"sweep\""), "{desc}");
    }

    #[test]
    fn a_call_with_no_arguments_still_reads_as_a_call() {
        assert_eq!(
            build_event_description("wa_screenshot", &json!({})),
            "Called wa_screenshot"
        );
    }

    /// Credentials never reach a plaintext append-only log, and a file body is
    /// neither attributable nor free to store.
    #[test]
    fn secrets_and_bulk_payloads_are_not_recorded() {
        let desc = build_event_description(
            "some_tool",
            &json!({
                "apiKey": "sk-live-abcdef",
                "password": "hunter2",
                "content": "the entire file, line by line",
                "target": "Desktop 3",
            }),
        );
        assert!(desc.contains("apiKey=[redacted]"), "{desc}");
        assert!(desc.contains("password=[redacted]"), "{desc}");
        assert!(desc.contains("content=[elided]"), "{desc}");
        assert!(desc.contains("target=\"Desktop 3\""), "{desc}");
        for leaked in ["sk-live-abcdef", "hunter2", "line by line"] {
            assert!(
                !desc.contains(leaked),
                "secret reached the audit trail: {desc}"
            );
        }
    }

    #[test]
    fn the_summary_is_bounded_and_single_line() {
        let mut args = serde_json::Map::new();
        for i in 0..30 {
            args.insert(format!("arg{i}"), json!("x".repeat(300)));
        }
        let desc = build_event_description("flood_tool", &Value::Object(args));
        let budget = "Called flood_tool: ".chars().count() + MAX_SUMMARY_CHARS + 16;
        assert!(
            desc.chars().count() <= budget,
            "unbounded audit entry: {} chars over a {budget} budget",
            desc.chars().count()
        );
        assert!(
            desc.contains("(+24 more)"),
            "dropped args must be admitted: {desc}"
        );
        assert!(!desc.contains('\n') && !desc.contains('\r'), "{desc}");
    }

    /// A long non-ASCII command is ordinary input; slicing it by byte count
    /// panicked mid-codepoint, which took the whole dispatch down with it.
    #[test]
    fn clipping_never_splits_a_character() {
        assert_eq!(clip("héllo wörld", 5), "héllo~");
        assert_eq!(clip("日本語のテキスト", 4), "日本語の~");
        assert_eq!(clip("short", 60), "short");
        let desc = build_event_description("run_command", &json!({ "command": "echo 日本語" }));
        assert_eq!(desc, "Ran command: echo 日本語");
    }

    // ─── Bug #42: a refusal must not be logged as a success ───────────────

    #[test]
    fn an_in_band_refusal_is_a_failure() {
        let note = in_band_failure(
            r#"{"success":false,"detail":"bring_to_front was refused: opt-in only"}"#,
        )
        .expect("a payload that says success:false is not a success");
        assert!(note.contains("was refused"), "{note}");
    }

    #[test]
    fn the_reason_is_taken_from_whichever_field_the_tool_uses() {
        for (payload, expected) in [
            (
                r#"{"success":false,"error":"no such window"}"#,
                "no such window",
            ),
            (r#"{"success":false,"message":"timed out"}"#, "timed out"),
            (
                r#"{"success":false,"reason":"daemon offline"}"#,
                "daemon offline",
            ),
            (
                r#"{"success":false,"code":42}"#,
                "tool reported success:false",
            ),
        ] {
            let note = in_band_failure(payload).expect("explicit false must be a failure");
            assert_eq!(note, expected, "for {payload}");
        }
    }

    /// Only an explicit boolean false counts. Reading a failure into plain text,
    /// a missing field, or unparseable output would invert bug #42 rather than
    /// fix it.
    #[test]
    fn nothing_is_inferred_where_no_verdict_exists() {
        for benign in [
            r#"{"success":true,"detail":"moved"}"#,
            r#"{"count":0,"windows":[]}"#,
            r#"{"success":null}"#,
            r#"{"success":"false-ish"}"#,
            r#"{"success":true,"results":[{"ok":false}]}"#,
            "plain text result, not JSON at all",
            r#"{ this is not valid json"#,
            "",
        ] {
            assert!(
                in_band_failure(benign).is_none(),
                "misread verdict in: {benign}"
            );
        }
    }

    #[test]
    fn a_long_refusal_note_is_bounded_before_it_reaches_the_log() {
        let payload = format!(
            r#"{{"success":false,"detail":"{}"}}"#,
            "x".repeat(MAX_FAILURE_NOTE_CHARS * 3)
        );
        let note = in_band_failure(&payload).expect("refusal must be recorded");
        assert!(
            note.chars().count() <= MAX_FAILURE_NOTE_CHARS + 1,
            "{} chars",
            note.chars().count()
        );
    }
}
