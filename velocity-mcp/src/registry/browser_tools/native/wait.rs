//! Deterministic waiting for the native browser tools.
//!
//! `browser_native_wait` blocks until a predicate holds on the session
//! (content move, element appear/disappear, navigation, or content
//! settlement) so async pages cost the agent one call instead of a
//! poll-and-reread loop.

use serde_json::Value;
use std::error::Error;

use crate::editor::browser::native_bridge::{get_or_create_native_bridge, NativeBrowserBridge};

/// Size of the distilled content projection (readability markdown, with the
/// plain markdown as fallback) — the baseline `browser_native_wait` watches.
pub(super) fn distilled_content_chars(bridge: &NativeBrowserBridge) -> usize {
    let mut content = bridge.page_content_markdown();
    if content.is_empty() {
        content = bridge.page_markdown();
    }
    content.chars().count()
}

/// Whether a content-size move clears the significance threshold. Kept pure
/// so the wait loop's only judgment call is trivially testable.
pub(super) fn content_delta_matches(baseline: usize, now: usize, min_delta: usize) -> bool {
    now.max(baseline) - now.min(baseline) >= min_delta
}

/// Block until a predicate holds on the session or the timeout elapses.
/// Locks are taken per poll so concurrent updates remain observable.
pub(super) fn wait_on_session(
    session_id: &str,
    arguments: &Value,
    compact: bool,
) -> Result<Option<String>, Box<dyn Error>> {
    let mode = arguments["mode"].as_str().unwrap_or("content");
    let timeout_ms = arguments["timeout"]
        .as_u64()
        .unwrap_or(10_000)
        .clamp(1, 60_000);
    let poll_ms = arguments["poll"].as_u64().unwrap_or(100).clamp(20, 1_000);
    // Significance threshold for mode=content: ignore tiny fluctuations
    // (timestamps, counters) and only wake on a real content move.
    let min_delta = arguments["minDelta"].as_u64().unwrap_or(1).max(1) as usize;
    let label = arguments["label"].as_str().unwrap_or("");
    // mode=element with gone=true inverts the predicate: wake when the
    // element is NOT on the page (spinners, overlays, toasts disappearing).
    let gone = arguments["gone"].as_bool().unwrap_or(false);
    // mode=stable: how many consecutive quiet polls (no move >= minDelta)
    // declare the page settled.
    let quiet_needed = arguments["stable"].as_u64().unwrap_or(3).clamp(2, 50) as usize;
    if !matches!(mode, "content" | "element" | "url" | "stable") {
        return Err(format!("unknown wait mode '{mode}'").into());
    }
    if mode == "element" && label.trim().is_empty() {
        return Err("label is required for mode=element".into());
    }
    let arc = get_or_create_native_bridge(session_id);
    let start = std::time::Instant::now();
    let (baseline, baseline_url) = {
        let bridge = arc
            .lock()
            .map_err(|_| "native browser bridge lock poisoned")?;
        (distilled_content_chars(&bridge), bridge.current_view().url)
    };
    let want = label.to_lowercase();
    let mut matched: Option<String> = None;
    // mode=stable bookkeeping: streak of consecutive quiet polls and the
    // content level the streak was last measured against.
    let mut last_chars = baseline;
    let mut quiet = 0usize;
    while start.elapsed().as_millis() < u128::from(timeout_ms) {
        std::thread::sleep(std::time::Duration::from_millis(poll_ms));
        let bridge = arc
            .lock()
            .map_err(|_| "native browser bridge lock poisoned")?;
        if mode == "element" {
            let view = bridge.current_view();
            let found = view
                .elements
                .iter()
                .find(|e| e.name.to_lowercase().contains(&want));
            if gone {
                if found.is_none() {
                    matched = Some(format!("\"{label}\" gone"));
                    break;
                }
            } else if let Some(e) = found {
                matched = Some(format!("{} \"{}\" (aom {})", e.role, e.name, e.aom_id));
                break;
            }
        } else if mode == "url" {
            // With a label: wake once the URL contains it. Without: wake on
            // any navigation away from the baseline (SPA or full loads).
            let now = bridge.current_view().url;
            if label.trim().is_empty() {
                if now != baseline_url {
                    matched = Some(format!("url {now}"));
                    break;
                }
            } else if now.to_lowercase().contains(&want) {
                matched = Some(format!("url {now}"));
                break;
            }
        } else if mode == "stable" {
            // Wake when the content stops moving: N consecutive polls with
            // no move >= minDelta means the page has settled.
            let now = distilled_content_chars(&bridge);
            if content_delta_matches(last_chars, now, min_delta) {
                // A real move: restart the streak from the new level.
                last_chars = now;
                quiet = 0;
            } else {
                quiet += 1;
                if quiet >= quiet_needed {
                    matched = Some(format!(
                        "content stable at {now} chars ({quiet} quiet polls)"
                    ));
                    break;
                }
            }
        } else {
            let now = distilled_content_chars(&bridge);
            if content_delta_matches(baseline, now, min_delta) {
                matched = Some(format!("content {baseline} -> {now} chars"));
                break;
            }
        }
    }
    let elapsed = start.elapsed().as_millis() as u64;
    if compact {
        return Ok(Some(
            serde_json::to_string_pretty(&serde_json::json!({
                "status": if matched.is_some() { "matched" } else { "timeout" },
                "mode": mode,
                "matched": matched,
                "elapsedMs": elapsed,
            }))
            .map_err(|e| format!("serialise wait report: {e}"))?,
        ));
    }
    Ok(Some(match matched {
        Some(m) => format!("matched after {elapsed} ms: {m}\n"),
        None => format!("timeout after {elapsed} ms: no '{mode}' change observed\n"),
    }))
}
