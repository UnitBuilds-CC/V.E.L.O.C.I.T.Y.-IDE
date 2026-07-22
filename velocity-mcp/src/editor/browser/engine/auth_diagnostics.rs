use super::*;
use std::path::Path;

pub fn build_auth_diagnostics_report(
    workspace_root: &Path,
    session: BrowserSessionState,
    snapshot: Option<BrowserPageSnapshot>,
    snapshot_json_path: Option<String>,
) -> BrowserAuthDiagnosticsReport {
    let session_id = session.id.clone();
    let has_login_form = snapshot
        .as_ref()
        .map(snapshot_has_login_form)
        .unwrap_or(false);
    let has_auth_cookie = session
        .cookies
        .iter()
        .any(|cookie| is_auth_cookie_name(&cookie.name));
    let has_csrf_token = session
        .local_storage
        .keys()
        .chain(session.session_storage.keys())
        .any(|key| is_csrf_key(key))
        || session
            .cookies
            .iter()
            .any(|cookie| is_csrf_key(&cookie.name));
    let auth_state = snapshot.as_ref().and_then(snapshot_auth_state);
    let router_name = snapshot.as_ref().and_then(snapshot_router_name);
    let auth_ready = snapshot
        .as_ref()
        .map(|snapshot: &BrowserPageSnapshot| {
            snapshot
                .settle_signals
                .iter()
                .any(|signal: &String| signal.eq_ignore_ascii_case("auth_ready"))
                || auth_state
                    .as_deref()
                    .map(|value: &str| value.eq_ignore_ascii_case("ready"))
                    .unwrap_or(false)
        })
        .unwrap_or(false);
    let session_expired = snapshot
        .as_ref()
        .map(snapshot_has_expired_marker)
        .unwrap_or(false);
    let diagnosis = if auth_ready {
        "auth_ready"
    } else if session_expired {
        "session_expired"
    } else if has_login_form && has_auth_cookie && !has_csrf_token {
        "csrf_missing"
    } else if has_login_form {
        "login_required"
    } else {
        "unknown"
    };
    let recommended_action = match diagnosis {
        "auth_ready" => "Continue with the authenticated workflow using the persisted session.".to_string(),
        "session_expired" => "Reauthenticate or restore a fresher authenticated checkpoint before continuing.".to_string(),
        "csrf_missing" => "Restore or reseed the CSRF token from storage/checkpoint before submitting the login form.".to_string(),
        "login_required" => "Complete the login flow or restore an authenticated checkpoint before continuing.".to_string(),
        _ => "Inspect the current snapshot, cookies, and storage to confirm the next recovery step.".to_string(),
    };
    let auth_signals = collect_auth_signals(
        &session,
        snapshot.as_ref(),
        has_login_form,
        has_auth_cookie,
        has_csrf_token,
        auth_state.as_deref(),
        router_name.as_deref(),
    );
    BrowserAuthDiagnosticsReport {
        session: summarize_session(session),
        diagnosis: diagnosis.to_string(),
        recommended_action,
        snapshot_available: snapshot.is_some(),
        has_login_form,
        has_auth_cookie,
        has_csrf_token,
        auth_state,
        router_name,
        auth_signal_count: auth_signals.len(),
        auth_signals,
        session_json_path: session_file_path(workspace_root, &session_id)
            .display()
            .to_string(),
        snapshot_json_path,
    }
}

pub fn auth_diagnostics_report(
    workspace_root: &Path,
    session_id: &str,
    sitemap_path: &Path,
) -> Result<BrowserAuthDiagnosticsReport, String> {
    let session = load_session_state(workspace_root, session_id)?;
    let snapshot = match session.current_url.as_deref() {
        Some(url) => load_snapshot_json(url, sitemap_path).ok(),
        None => None,
    };
    let snapshot_json_path = snapshot.as_ref().map(|snapshot: &BrowserPageSnapshot| {
        browser_snapshot_path(&snapshot.url, sitemap_path)
            .display()
            .to_string()
    });
    Ok(build_auth_diagnostics_report(
        workspace_root,
        session,
        snapshot,
        snapshot_json_path,
    ))
}

pub fn build_access_diagnostics_report(
    workspace_root: &Path,
    session: BrowserSessionState,
    snapshot: Option<BrowserPageSnapshot>,
    snapshot_json_path: Option<String>,
) -> BrowserAccessDiagnosticsReport {
    let session_id = session.id.clone();
    let router_name = snapshot.as_ref().and_then(snapshot_router_name);
    let captcha_needles = ["captcha", "recaptcha", "hcaptcha", "turnstile"];
    let challenge_needles = [
        "challenge",
        "verify you are human",
        "bot check",
        "cloudflare",
        "attention required",
    ];
    let rate_limit_needles = [
        "rate limit",
        "too many requests",
        "retry later",
        "slow down",
    ];
    let block_needles = ["access denied", "request blocked", "blocked", "forbidden"];

    let diagnosis = if snapshot
        .as_ref()
        .map(|snapshot| snapshot_has_access_marker(snapshot, &captcha_needles))
        .unwrap_or(false)
    {
        "captcha_required"
    } else if snapshot
        .as_ref()
        .map(|snapshot| snapshot_has_access_marker(snapshot, &challenge_needles))
        .unwrap_or(false)
    {
        "anti_bot_challenge"
    } else if snapshot
        .as_ref()
        .map(|snapshot| snapshot_has_access_marker(snapshot, &rate_limit_needles))
        .unwrap_or(false)
    {
        "rate_limited"
    } else if snapshot
        .as_ref()
        .map(|snapshot| snapshot_has_access_marker(snapshot, &block_needles))
        .unwrap_or(false)
    {
        "access_blocked"
    } else {
        "clear"
    };

    let recommended_action = match diagnosis {
        "captcha_required" => "Escalate for manual or external captcha solving; the current browser runtime cannot truthfully solve captcha challenges on its own.".to_string(),
        "anti_bot_challenge" => "Wait for the challenge to clear, use a fresher session, or move to a less restricted path before retrying.".to_string(),
        "rate_limited" => "Back off and retry later, reduce request frequency, or resume from a cooler session/checkpoint.".to_string(),
        "access_blocked" => "Inspect the target site policy, credentials, and network identity before retrying; the current session appears blocked.".to_string(),
        _ => "No explicit access blocker was detected from the persisted snapshot evidence.".to_string(),
    };

    let mut challenge_signals = Vec::new();
    if let Some(snapshot) = snapshot.as_ref() {
        for signal in snapshot.settle_signals.iter().filter(|signal| {
            contains_any_case_insensitive(
                signal,
                &[
                    "captcha",
                    "challenge",
                    "blocked",
                    "forbidden",
                    "rate limit",
                    "too many requests",
                    "cloudflare",
                    "human",
                ],
            )
        }) {
            challenge_signals.push(format!("settle:{}", signal));
        }
        for event in snapshot.protocol_events.iter().filter(|event| {
            contains_any_case_insensitive(
                &event.kind,
                &[
                    "captcha",
                    "challenge",
                    "blocked",
                    "forbidden",
                    "rate",
                    "cloudflare",
                ],
            ) || contains_any_case_insensitive(
                &event.phase,
                &[
                    "captcha",
                    "challenge",
                    "blocked",
                    "forbidden",
                    "rate",
                    "cloudflare",
                ],
            ) || contains_any_case_insensitive(
                &event.target,
                &[
                    "captcha",
                    "challenge",
                    "blocked",
                    "forbidden",
                    "rate",
                    "cloudflare",
                ],
            ) || contains_any_case_insensitive(
                &event.detail,
                &[
                    "captcha",
                    "challenge",
                    "blocked",
                    "forbidden",
                    "rate",
                    "cloudflare",
                    "human",
                ],
            )
        }) {
            challenge_signals.push(format!("protocol:{}:{}", event.kind, event.phase));
        }
        if snapshot_has_access_marker(snapshot, &captcha_needles) {
            challenge_signals.push("page:captcha".to_string());
        }
        if snapshot_has_access_marker(snapshot, &challenge_needles) {
            challenge_signals.push("page:challenge".to_string());
        }
        if snapshot_has_access_marker(snapshot, &rate_limit_needles) {
            challenge_signals.push("page:rate_limit".to_string());
        }
        if snapshot_has_access_marker(snapshot, &block_needles) {
            challenge_signals.push("page:blocked".to_string());
        }
    }
    if let Some(router_name) = router_name.as_deref() {
        challenge_signals.push(format!("router:{}", router_name));
    }
    challenge_signals.sort();
    challenge_signals.dedup();

    BrowserAccessDiagnosticsReport {
        session: summarize_session(session),
        diagnosis: diagnosis.to_string(),
        recommended_action,
        snapshot_available: snapshot.is_some(),
        challenge_signal_count: challenge_signals.len(),
        challenge_signals,
        router_name,
        session_json_path: session_file_path(workspace_root, &session_id)
            .display()
            .to_string(),
        snapshot_json_path,
    }
}

pub fn access_diagnostics_report(
    workspace_root: &Path,
    session_id: &str,
    sitemap_path: &Path,
) -> Result<BrowserAccessDiagnosticsReport, String> {
    let session = load_session_state(workspace_root, session_id)?;
    let snapshot = match session.current_url.as_deref() {
        Some(url) => load_snapshot_json(url, sitemap_path).ok(),
        None => None,
    };
    let snapshot_json_path = snapshot.as_ref().map(|snapshot: &BrowserPageSnapshot| {
        browser_snapshot_path(&snapshot.url, sitemap_path)
            .display()
            .to_string()
    });
    Ok(build_access_diagnostics_report(
        workspace_root,
        session,
        snapshot,
        snapshot_json_path,
    ))
}
