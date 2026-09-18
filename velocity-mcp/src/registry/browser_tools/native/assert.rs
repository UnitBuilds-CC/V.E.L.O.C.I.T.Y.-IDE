//! Page-state assertions (guards) for the native browser tools.
//!
//! `browser_native_assert` checks text/element conditions in one call; an
//! unmet condition is a tool error, and is also recorded in the outcome
//! history so the reflection loop treats repeated misses as a pattern.

use serde_json::Value;
use std::error::Error;
use std::path::Path;

use crate::editor::browser::native_bridge::{get_or_create_native_bridge, NativeBrowserBridge};
use velocity_browser::{AgentActionResult, NdaDelta};

use super::render::fact_snippet;

/// Evaluate the assert conditions against the current bridge state.
/// Returns `(what, expected value, ok, detail-on-failure)` per check.
pub(super) fn evaluate_assert_checks(
    bridge: &NativeBrowserBridge,
    raw_text: &str,
    raw_label: &str,
) -> Vec<(String, String, bool, String)> {
    let text = raw_text.trim().to_lowercase();
    let label = raw_label.trim().to_lowercase();
    let mut checks: Vec<(String, String, bool, String)> = Vec::new();
    if !text.is_empty() {
        // One shared definition of "the content" (bug #53): the old
        // readability-markdown-only chain saw a JSON document as 0 chars, so
        // `assert` reported the text the agent had just submitted as missing.
        let content = bridge.distilled_content();
        let ok = content.to_lowercase().contains(&text);
        let detail = format!(
            "content is {} chars: {}",
            content.chars().count(),
            fact_snippet(&content)
        );
        checks.push(("text".to_string(), raw_text.to_string(), ok, detail));
    }
    if !label.is_empty() {
        let view = bridge.current_view();
        let ok = view
            .elements
            .iter()
            .any(|e| e.name.to_lowercase().contains(&label));
        checks.push((
            "element".to_string(),
            raw_label.to_string(),
            ok,
            format!("{} element(s) in view", view.elements.len()),
        ));
    }
    checks
}

/// Whether every check held. Kept next to the report renderer so the verdict
/// the agent reads and the verdict the tool reports can never disagree.
pub(super) fn asserts_hold(checks: &[(String, String, bool, String)]) -> bool {
    checks.iter().all(|(_, _, ok, _)| *ok)
}

/// Render an assert verdict. `waited_ms` is only set when the call used a
/// waitMs grace period, in which case the report carries the elapsed time.
pub(super) fn render_assert_report(
    checks: &[(String, String, bool, String)],
    waited_ms: Option<u64>,
    compact: bool,
) -> Result<String, Box<dyn Error>> {
    let all_ok = asserts_hold(checks);
    if compact {
        let mut json = serde_json::json!({
            "ok": all_ok,
            "checks": checks.iter().map(|(what, value, ok, detail)| {
                serde_json::json!({ "what": what, "value": value, "ok": ok, "detail": detail })
            }).collect::<Vec<_>>(),
        });
        if let Some(elapsed) = waited_ms {
            json["elapsedMs"] = serde_json::json!(elapsed);
        }
        return serde_json::to_string_pretty(&json)
            .map_err(|e| format!("serialise assert report: {e}").into());
    }
    let prefix = match waited_ms {
        Some(elapsed) => format!("after {elapsed} ms: "),
        None => String::new(),
    };
    let mut out = String::new();
    if all_ok {
        out.push_str(&prefix);
        out.push_str("assert ok: ");
        let parts: Vec<String> = checks
            .iter()
            .map(|(what, value, _, _)| format!("{what} \"{value}\""))
            .collect();
        out.push_str(&parts.join("; "));
        out.push('\n');
    } else {
        out.push_str(&prefix);
        out.push_str("assert FAILED:\n");
        for (what, value, ok, detail) in checks {
            let verdict = if *ok { "ok" } else { "FAILED" };
            out.push_str(&format!("  - {what} \"{value}\": {verdict} ({detail})\n"));
        }
    }
    Ok(out)
}

/// Check page-state conditions in one call. An unmet condition is reported as
/// a tool error, carrying the full per-check diagnosis, because `isError` is
/// the only failure signal that survives into the MCP audit trail: an assert
/// that answered `isError: false` next to its own "assert FAILED" text made a
/// sweep count unverified guards as passes (bug #49). A failed assertion is
/// still recorded in the outcome history, so it stays a cheap guard an agent
/// can probe with rather than a hard stop it must avoid.
/// With waitMs > 0 the checks poll (lock released between polls) until the
/// conditions hold or the grace period elapses.
pub(super) fn assert_on_session(
    root: &Path,
    session_id: &str,
    arguments: &Value,
    compact: bool,
) -> Result<Option<String>, Box<dyn Error>> {
    let raw_text = arguments["text"].as_str().unwrap_or("");
    let raw_label = arguments["label"].as_str().unwrap_or("");
    if raw_text.trim().is_empty() && raw_label.trim().is_empty() {
        return Err("assert needs at least one of: text, label".into());
    }
    // waitMs grants a grace period for async pages to reach the state.
    let wait_ms = arguments["waitMs"].as_u64().unwrap_or(0).min(60_000);
    let poll_ms = arguments["poll"].as_u64().unwrap_or(100).clamp(20, 1_000);
    let arc = get_or_create_native_bridge(session_id);
    // First-touch experience inheritance, matching every other tool: the
    // default bundle is seeded before the first evaluation runs.
    {
        let mut bridge = arc
            .lock()
            .map_err(|_| "native browser bridge lock poisoned")?;
        bridge.seed_default_experience(root);
    }
    let start = std::time::Instant::now();
    let checks = loop {
        let bridge = arc
            .lock()
            .map_err(|_| "native browser bridge lock poisoned")?;
        let checks = evaluate_assert_checks(&bridge, raw_text, raw_label);
        if asserts_hold(&checks) || start.elapsed().as_millis() >= u128::from(wait_ms) {
            break checks;
        }
        drop(bridge);
        std::thread::sleep(std::time::Duration::from_millis(poll_ms));
    };
    let waited = (wait_ms > 0).then(|| start.elapsed().as_millis() as u64);
    // Failed guards are learning signals: record each missed check in the
    // outcome history so browser_native_reflect spots repeated "expected
    // X" misses exactly like repeated dead clicks.
    if !asserts_hold(&checks) {
        let mut bridge = arc
            .lock()
            .map_err(|_| "native browser bridge lock poisoned")?;
        for (what, value, ok, _) in &checks {
            if *ok {
                continue;
            }
            let role = if what == "text" { "content" } else { "element" };
            let result = AgentActionResult::failed(
                format!("assert failed: {what} \"{value}\" not satisfied"),
                NdaDelta::default(),
            );
            bridge.record_outcome("assert", role, value, &result);
        }
    }
    let report = render_assert_report(&checks, waited, compact)?;
    if asserts_hold(&checks) {
        return Ok(Some(report));
    }
    Err(report.into())
}
