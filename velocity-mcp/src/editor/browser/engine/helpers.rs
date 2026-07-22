use super::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};


pub fn browser_workflow_suite_json_path(workspace_root: &Path, suite_name: &str) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-suites")
        .join(format!("{}.suite.json", sanitize_file_stem(suite_name)))
}

pub fn browser_workflow_suite_run_path(workspace_root: &Path, suite_name: &str) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-suite-runs")
        .join(format!("{}.suite-run.json", sanitize_file_stem(suite_name)))
}

pub fn browser_session_checkpoint_path(
    workspace_root: &Path,
    session_id: &str,
    checkpoint_name: &str,
) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-session-checkpoints")
        .join(sanitize_file_stem(session_id))
        .join(format!(
            "{}.checkpoint.json",
            sanitize_file_stem(checkpoint_name)
        ))
}

pub fn browser_auth_profile_json_path(workspace_root: &Path, profile_name: &str) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-auth-profiles")
        .join(format!("{}.auth.json", sanitize_file_stem(profile_name)))
}

pub fn parse_cookie_header(value: &str) -> Option<BrowserCookie> {
    let cookie_part = value.split(';').next()?.trim();
    let mut parts = cookie_part.splitn(2, '=');
    let name = parts.next()?.trim();
    let cookie_value = parts.next().unwrap_or("").trim();
    if name.is_empty() {
        return None;
    }
    Some(BrowserCookie {
        name: name.to_string(),
        value: cookie_value.to_string(),
    })
}

pub fn merge_cookie(cookies: &mut Vec<BrowserCookie>, cookie: BrowserCookie) {
    if let Some(existing) = cookies.iter_mut().find(|entry| entry.name == cookie.name) {
        *existing = cookie;
    } else {
        cookies.push(cookie);
    }
}

pub fn sync_runtime_cookies_from_browser_cookies(session: &mut BrowserSessionState) {
    session.runtime_cookies = session
        .cookies
        .iter()
        .map(browser_cookie_as_runtime_cookie)
        .collect();
}

pub fn auth_runtime_cookies_for_source(source: &BrowserSessionState) -> Vec<RuntimeBrowserCookie> {
    if !source.runtime_cookies.is_empty() {
        source
            .runtime_cookies
            .iter()
            .filter(|cookie| is_auth_cookie_name(&cookie.name) || is_csrf_key(&cookie.name))
            .cloned()
            .collect()
    } else {
        filter_auth_cookies(&source.cookies)
            .iter()
            .map(browser_cookie_as_runtime_cookie)
            .collect()
    }
}

pub fn cookie_header(cookies: &[BrowserCookie]) -> Option<String> {
    if cookies.is_empty() {
        None
    } else {
        Some(
            cookies
                .iter()
                .map(|cookie| format!("{}={}", cookie.name, cookie.value))
                .collect::<Vec<_>>()
                .join("; "),
        )
    }
}

pub fn parse_storage_header(raw: &str) -> HashMap<String, String> {
    let mut updates = HashMap::new();
    for pair in raw.split(';') {
        let trimmed = pair.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            updates.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    updates
}

pub fn parse_list_header(raw: &str) -> Vec<String> {
    raw.split(';')
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .collect()
}

pub fn request_records_from_headers(
    method: &str,
    url: &str,
    status_code: u16,
    raw: Option<&str>,
) -> Vec<BrowserRequestRecord> {
    let mut records = raw
        .map(|value| {
            value
                .split(';')
                .filter_map(|entry| {
                    let trimmed = entry.trim();
                    if trimmed.is_empty() {
                        return None;
                    }
                    let mut parts = trimmed.splitn(2, '=');
                    let resource = parts.next().unwrap_or("document").trim();
                    let request_url = parts.next().unwrap_or(url).trim();
                    Some(BrowserRequestRecord {
                        method: method.to_ascii_uppercase(),
                        url: if request_url.is_empty() {
                            url.to_string()
                        } else {
                            request_url.to_string()
                        },
                        status_code,
                        resource: if resource.is_empty() {
                            "document".to_string()
                        } else {
                            resource.to_string()
                        },
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if records.is_empty() {
        records.push(BrowserRequestRecord {
            method: method.to_ascii_uppercase(),
            url: url.to_string(),
            status_code,
            resource: "document".to_string(),
        });
    }
    records
}

pub fn request_record_matches(
    record: &BrowserRequestRecord,
    method: Option<&str>,
    url_contains: Option<&str>,
    status: Option<u16>,
    resource: Option<&str>,
) -> bool {
    if let Some(wait_method) = method {
        if !record.method.eq_ignore_ascii_case(wait_method) {
            return false;
        }
    }
    if let Some(wait_url_fragment) = url_contains {
        if !record.url.contains(wait_url_fragment) {
            return false;
        }
    }
    if let Some(wait_status) = status {
        if record.status_code != wait_status {
            return false;
        }
    }
    if let Some(wait_resource) = resource {
        if !record.resource.eq_ignore_ascii_case(wait_resource) {
            return false;
        }
    }
    true
}

pub fn storage_entry_matches(
    snapshot: &BrowserPageSnapshot,
    scope: &str,
    key: &str,
    value: Option<&str>,
) -> bool {
    snapshot.storage.iter().any(|bucket| {
        bucket.scope.eq_ignore_ascii_case(scope)
            && bucket.entries.iter().any(|(entry_key, entry_value): (&String, &String)| {
                entry_key.eq_ignore_ascii_case(key)
                    && value
                        .map(|needle| entry_value.contains(needle))
                        .unwrap_or(true)
            })
    })
}

pub fn protocol_event_matches(
    event: &BrowserProtocolEvent,
    kind: Option<&str>,
    phase: Option<&str>,
    target: Option<&str>,
    detail: Option<&str>,
) -> bool {
    if let Some(wait_kind) = kind {
        if !event.kind.eq_ignore_ascii_case(wait_kind) {
            return false;
        }
    }
    if let Some(wait_phase) = phase {
        if !event.phase.eq_ignore_ascii_case(wait_phase) {
            return false;
        }
    }
    if let Some(wait_target) = target {
        if !contains_case_insensitive(&event.target, wait_target) {
            return false;
        }
    }
    if let Some(wait_detail) = detail {
        if !contains_case_insensitive(&event.detail, wait_detail) {
            return false;
        }
    }
    true
}

pub fn default_settle_signals(method: &str, status_code: u16) -> Vec<String> {
    let mut signals = vec!["response_complete".to_string()];
    if method.eq_ignore_ascii_case("GET") {
        signals.push("navigation_settled".to_string());
    }
    if (200..400).contains(&status_code) {
        signals.push("network_settled".to_string());
    }
    signals
}

pub fn settle_signals_from_headers(method: &str, status_code: u16, raw: Option<&str>) -> Vec<String> {
    let mut signals = parse_list_header(raw.unwrap_or_default());
    if signals.is_empty() {
        signals = default_settle_signals(method, status_code);
    }
    signals.sort();
    signals.dedup();
    signals
}

pub fn parse_settle_signal_parts(signal: &str) -> Option<(&str, &str)> {
    if let Some((scope, state)) = signal.split_once(':') {
        let scope = scope.trim();
        let state = state.trim();
        if !scope.is_empty() && !state.is_empty() {
            return Some((scope, state));
        }
    }
    if let Some((scope, state)) = signal.rsplit_once('_') {
        let scope = scope.trim();
        let state = state.trim();
        if !scope.is_empty() && !state.is_empty() {
            return Some((scope, state));
        }
    }
    None
}

pub fn settle_signal_matches(
    signal: &str,
    label: Option<&str>,
    scope: Option<&str>,
    state: Option<&str>,
) -> bool {
    if let Some(wait_label) = label {
        return signal
            .to_ascii_lowercase()
            .contains(&wait_label.to_ascii_lowercase());
    }
    if let Some(wait_scope) = scope {
        let Some((signal_scope, signal_state)) = parse_settle_signal_parts(signal) else {
            return false;
        };
        if !signal_scope.eq_ignore_ascii_case(wait_scope) {
            return false;
        }
        return state
            .map(|wait_state| signal_state.eq_ignore_ascii_case(wait_state))
            .unwrap_or(true);
    }
    false
}

pub fn runtime_state_from_headers(raw: Option<&str>) -> Vec<BrowserRuntimeState> {
    let mut state = raw
        .map(|value| {
            value
                .split(';')
                .filter_map(|entry| {
                    let trimmed = entry.trim();
                    if trimmed.is_empty() {
                        return None;
                    }
                    let (scope_and_key, value) = trimmed.split_once('=')?;
                    let (scope, key) = scope_and_key.split_once(':')?;
                    let scope = scope.trim();
                    let key = key.trim();
                    let value = value.trim();
                    if scope.is_empty() || key.is_empty() || value.is_empty() {
                        return None;
                    }
                    Some(BrowserRuntimeState {
                        scope: scope.to_string(),
                        key: key.to_string(),
                        value: value.to_string(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    state.sort_by(|left, right| {
        left.scope
            .cmp(&right.scope)
            .then_with(|| left.key.cmp(&right.key))
            .then_with(|| left.value.cmp(&right.value))
    });
    state.dedup();
    state
}

pub fn protocol_event_signature(event: &BrowserProtocolEvent) -> String {
    format!(
        "{}:{}:{}:{}",
        event.kind, event.phase, event.target, event.detail
    )
}

pub fn protocol_events_from_headers(raw: Option<&str>) -> Vec<BrowserProtocolEvent> {
    let mut events = raw
        .map(|value| {
            value
                .split(';')
                .filter_map(|entry| {
                    let trimmed = entry.trim();
                    if trimmed.is_empty() {
                        return None;
                    }
                    let mut parts = trimmed.splitn(4, '|').map(str::trim);
                    let kind = parts.next()?;
                    let phase = parts.next()?;
                    let target = parts.next()?;
                    let detail = parts.next()?;
                    if kind.is_empty() || phase.is_empty() || target.is_empty() || detail.is_empty()
                    {
                        return None;
                    }
                    Some(BrowserProtocolEvent {
                        kind: kind.to_string(),
                        phase: phase.to_string(),
                        target: target.to_string(),
                        detail: detail.to_string(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    events.sort_by(|left, right| {
        protocol_event_signature(left).cmp(&protocol_event_signature(right))
    });
    events.dedup();
    events
}

