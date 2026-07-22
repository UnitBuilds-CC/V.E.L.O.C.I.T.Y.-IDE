use super::*;
use std::collections::HashMap;

pub fn render_storage_read_report(report: &BrowserStorageReadReport) -> String {
    format!(
        "Read browser storage for session '{}' scope '{}'\nEntries: {}\nSession JSON: {}",
        report.session.id, report.scope, report.entry_count, report.session_json_path,
    )
}

pub fn render_storage_update_report(report: &BrowserStorageUpdateReport) -> String {
    format!(
        "Updated browser storage for session '{}' scope '{}'\nSession JSON: {}",
        report.session.id, report.scope, report.session_json_path,
    )
}

fn summarize_cookie_names(cookies: &[BrowserCookie]) -> Vec<String> {
    let mut names = cookies
        .iter()
        .map(|cookie| cookie.name.clone())
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

fn summarize_runtime_cookie_names(cookies: &[RuntimeBrowserCookie]) -> Vec<String> {
    let mut names = cookies
        .iter()
        .map(|cookie| cookie.name.clone())
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

pub fn contains_any_case_insensitive(haystack: &str, needles: &[&str]) -> bool {
    needles
        .iter()
        .any(|needle| contains_case_insensitive(haystack, needle))
}

pub fn is_auth_cookie_name(name: &str) -> bool {
    contains_any_case_insensitive(name, &["session", "auth", "token", "sid", "jwt", "refresh"])
}

pub fn is_csrf_key(name: &str) -> bool {
    contains_any_case_insensitive(name, &["csrf", "xsrf"])
}

fn summarize_sorted_keys(entries: &HashMap<String, String>) -> Vec<String> {
    let mut keys = entries.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    keys
}

pub fn filter_auth_cookies(cookies: &[BrowserCookie]) -> Vec<BrowserCookie> {
    cookies
        .iter()
        .filter(|cookie| is_auth_cookie_name(&cookie.name) || is_csrf_key(&cookie.name))
        .cloned()
        .collect()
}

pub fn merge_runtime_cookie(cookies: &mut Vec<RuntimeBrowserCookie>, cookie: RuntimeBrowserCookie) {
    if let Some(existing) = cookies.iter_mut().find(|existing| {
        existing.name == cookie.name
            && existing.domain == cookie.domain
            && existing.path == cookie.path
    }) {
        *existing = cookie;
    } else {
        cookies.push(cookie);
    }
}

pub fn filter_csrf_storage(entries: &HashMap<String, String>) -> HashMap<String, String> {
    entries
        .iter()
        .filter(|(key, _)| is_csrf_key(key))
        .map(|(key, value): (&String, &String)| (key.clone(), value.clone()))
        .collect()
}

pub fn snapshot_has_login_form(snapshot: &BrowserPageSnapshot) -> bool {
    contains_any_case_insensitive(&snapshot.title, &["login", "sign in", "signin", "reauth"])
        || contains_any_case_insensitive(
            &snapshot.summary,
            &["login", "sign in", "signin", "reauth"],
        )
        || snapshot.forms.iter().any(|form| {
            contains_any_case_insensitive(&form.id, &["login", "signin", "auth"])
                || contains_any_case_insensitive(&form.action, &["login", "signin", "auth"])
                || form
                    .submit_label
                    .as_deref()
                    .map(|label| {
                        contains_any_case_insensitive(
                            label,
                            &["login", "sign in", "signin", "continue"],
                        )
                    })
                    .unwrap_or(false)
        })
        || snapshot.elements.iter().any(|element| {
            contains_any_case_insensitive(&element.name, &["login", "sign in", "signin", "reauth"])
        })
}

pub fn snapshot_has_expired_marker(snapshot: &BrowserPageSnapshot) -> bool {
    let expired_needles = [
        "expired",
        "reauth",
        "login_required",
        "unauthorized",
        "forbidden",
        "signed out",
    ];
    contains_any_case_insensitive(&snapshot.title, &expired_needles)
        || contains_any_case_insensitive(&snapshot.summary, &expired_needles)
        || snapshot
            .settle_signals
            .iter()
            .any(|signal| contains_any_case_insensitive(signal, &expired_needles))
        || snapshot.runtime_state.iter().any(|entry| {
            contains_any_case_insensitive(&entry.scope, &expired_needles)
                || contains_any_case_insensitive(&entry.key, &expired_needles)
                || contains_any_case_insensitive(&entry.value, &expired_needles)
        })
        || snapshot.protocol_events.iter().any(|event| {
            contains_any_case_insensitive(&event.kind, &expired_needles)
                || contains_any_case_insensitive(&event.phase, &expired_needles)
                || contains_any_case_insensitive(&event.target, &expired_needles)
                || contains_any_case_insensitive(&event.detail, &expired_needles)
        })
}

pub fn snapshot_has_access_marker(snapshot: &BrowserPageSnapshot, needles: &[&str]) -> bool {
    contains_any_case_insensitive(&snapshot.title, needles)
        || contains_any_case_insensitive(&snapshot.summary, needles)
        || snapshot.elements.iter().any(|element| {
            contains_any_case_insensitive(&element.name, needles)
                || contains_any_case_insensitive(&element.value, needles)
        })
        || snapshot.forms.iter().any(|form| {
            contains_any_case_insensitive(&form.id, needles)
                || contains_any_case_insensitive(&form.action, needles)
                || form
                    .submit_label
                    .as_deref()
                    .map(|label| contains_any_case_insensitive(label, needles))
                    .unwrap_or(false)
                || form.fields.iter().any(|field| {
                    contains_any_case_insensitive(&field.name, needles)
                        || contains_any_case_insensitive(&field.label, needles)
                        || contains_any_case_insensitive(&field.value, needles)
                })
        })
        || snapshot
            .settle_signals
            .iter()
            .any(|signal| contains_any_case_insensitive(signal, needles))
        || snapshot.runtime_state.iter().any(|entry| {
            contains_any_case_insensitive(&entry.scope, needles)
                || contains_any_case_insensitive(&entry.key, needles)
                || contains_any_case_insensitive(&entry.value, needles)
        })
        || snapshot.protocol_events.iter().any(|event| {
            contains_any_case_insensitive(&event.kind, needles)
                || contains_any_case_insensitive(&event.phase, needles)
                || contains_any_case_insensitive(&event.target, needles)
                || contains_any_case_insensitive(&event.detail, needles)
        })
}

pub fn snapshot_auth_state(snapshot: &BrowserPageSnapshot) -> Option<String> {
    snapshot
        .runtime_state
        .iter()
        .find(|entry| entry.key.eq_ignore_ascii_case("auth"))
        .map(|entry| entry.value.clone())
}

pub fn snapshot_router_name(snapshot: &BrowserPageSnapshot) -> Option<String> {
    snapshot
        .runtime_state
        .iter()
        .find(|entry| {
            entry.scope.eq_ignore_ascii_case("router") && entry.key.eq_ignore_ascii_case("name")
        })
        .map(|entry| entry.value.clone())
}

pub fn collect_auth_signals(
    session: &BrowserSessionState,
    snapshot: Option<&BrowserPageSnapshot>,
    has_login_form: bool,
    has_auth_cookie: bool,
    has_csrf_token: bool,
    auth_state: Option<&str>,
    router_name: Option<&str>,
) -> Vec<String> {
    let mut signals = Vec::new();
    for cookie in session
        .cookies
        .iter()
        .filter(|cookie| is_auth_cookie_name(&cookie.name))
    {
        signals.push(format!("cookie:{}", cookie.name));
    }
    if has_csrf_token {
        signals.push("csrf:present".to_string());
    }
    if has_login_form {
        signals.push("page:login_form".to_string());
    }
    if let Some(value) = auth_state {
        signals.push(format!("runtime_auth:{}", value));
    }
    if let Some(value) = router_name {
        signals.push(format!("router:{}", value));
    }
    if let Some(snapshot) = snapshot {
        for signal in snapshot.settle_signals.iter().filter(|signal| {
            contains_any_case_insensitive(signal, &["auth", "login", "session", "csrf", "expired"])
        }) {
            signals.push(format!("settle:{}", signal));
        }
        for event in snapshot.protocol_events.iter().filter(|event| {
            contains_any_case_insensitive(
                &event.kind,
                &["auth", "login", "session", "csrf", "expired"],
            ) || contains_any_case_insensitive(
                &event.phase,
                &["auth", "login", "session", "csrf", "expired"],
            ) || contains_any_case_insensitive(
                &event.target,
                &["auth", "login", "session", "csrf", "expired"],
            ) || contains_any_case_insensitive(
                &event.detail,
                &["auth", "login", "session", "csrf", "expired"],
            )
        }) {
            signals.push(format!("protocol:{}:{}", event.kind, event.phase));
        }
    }
    if has_auth_cookie && signals.is_empty() {
        signals.push("cookie:present".to_string());
    }
    signals.sort();
    signals.dedup();
    signals
}

