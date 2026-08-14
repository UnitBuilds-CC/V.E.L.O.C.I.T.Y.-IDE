use super::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn render_cookie_read_report(report: &BrowserCookieReadReport) -> String {
    format!(
        "Read browser cookies for session '{}'\nCookies: {}\nSession JSON: {}",
        report.session.id, report.cookie_count, report.session_json_path,
    )
}

pub fn render_cookie_update_report(report: &BrowserCookieUpdateReport) -> String {
    format!(
        "Updated browser cookies for session '{}'\nUpdated: {}\nCookies: {}\nSession JSON: {}",
        report.session.id,
        report.updated_cookie_count,
        report.cookie_count,
        report.session_json_path,
    )
}

pub fn render_auth_reseed_report(report: &BrowserAuthReseedReport) -> String {
    let mut details = vec![
        format!(
            "Reseeded auth state into session '{}'",
            report.target_session.id
        ),
        format!("Source kind: {}", report.source_kind),
        format!("Source session: {}", report.source_session_id),
        format!("Copied auth cookies: {}", report.copied_cookie_count),
        format!(
            "Copied local storage entries: {}",
            report.copied_local_storage_count
        ),
        format!(
            "Copied session storage entries: {}",
            report.copied_session_storage_count
        ),
        format!("Session JSON: {}", report.session_json_path),
        format!("Auth diagnosis: {}", report.auth_diagnostics.diagnosis),
        format!(
            "Auth recommendation: {}",
            report.auth_diagnostics.recommended_action
        ),
    ];
    if let Some(checkpoint_name) = report.source_checkpoint_name.as_deref() {
        details.push(format!("Source checkpoint: {}", checkpoint_name));
    }
    if !report.copied_cookie_names.is_empty() {
        details.push(format!(
            "Cookie names: {}",
            report.copied_cookie_names.join(", ")
        ));
    }
    if !report.copied_local_storage_keys.is_empty() {
        details.push(format!(
            "Local storage keys: {}",
            report.copied_local_storage_keys.join(", ")
        ));
    }
    if !report.copied_session_storage_keys.is_empty() {
        details.push(format!(
            "Session storage keys: {}",
            report.copied_session_storage_keys.join(", ")
        ));
    }
    details.join("\n")
}

pub fn render_runtime_auth_reseed_report(report: &RuntimeAuthReseedReport) -> String {
    let mut details = vec![
        format!(
            "Reseeded auth state into runtime session '{}'",
            report.target_runtime_session.id
        ),
        format!(
            "Runtime session id: {}",
            report.target_runtime_session.runtime_session_id
        ),
        format!("Source kind: {}", report.source_kind),
        format!("Source session: {}", report.source_session_id),
        format!("Copied auth cookies: {}", report.copied_cookie_count),
        format!(
            "Copied local storage entries: {}",
            report.copied_local_storage_count
        ),
        format!(
            "Copied session storage entries: {}",
            report.copied_session_storage_count
        ),
        format!("Session JSON: {}", report.session_json_path),
        format!("Auth diagnosis: {}", report.auth_diagnostics.diagnosis),
        format!(
            "Auth recommendation: {}",
            report.auth_diagnostics.recommended_action
        ),
    ];
    if let Some(checkpoint_name) = report.source_checkpoint_name.as_deref() {
        details.push(format!("Source checkpoint: {}", checkpoint_name));
    }
    if !report.copied_cookie_names.is_empty() {
        details.push(format!(
            "Cookie names: {}",
            report.copied_cookie_names.join(", ")
        ));
    }
    if !report.copied_local_storage_keys.is_empty() {
        details.push(format!(
            "Local storage keys: {}",
            report.copied_local_storage_keys.join(", ")
        ));
    }
    if !report.copied_session_storage_keys.is_empty() {
        details.push(format!(
            "Session storage keys: {}",
            report.copied_session_storage_keys.join(", ")
        ));
    }
    if !report.warnings.is_empty() {
        details.push(format!(
            "Warnings ({}): {}",
            report.warning_count,
            report.warnings.join(" | ")
        ));
    }
    details.join("\n")
}

pub fn render_auth_profile_save_report(report: &BrowserAuthProfileSaveReport) -> String {
    let mut details = vec![
        format!("Saved browser auth profile '{}'", report.profile.name),
        format!("Source kind: {}", report.profile.source_kind),
        format!("Source session: {}", report.profile.source_session_id),
        format!("Auth cookies: {}", report.profile.cookie_count),
        format!(
            "Local storage entries: {}",
            report.profile.local_storage_count
        ),
        format!(
            "Session storage entries: {}",
            report.profile.session_storage_count
        ),
        format!("Profile JSON: {}", report.profile_json_path),
        format!("Auth diagnosis: {}", report.profile.diagnosis),
        format!("Auth recommendation: {}", report.profile.recommended_action),
    ];
    if let Some(checkpoint_name) = report.profile.source_checkpoint_name.as_deref() {
        details.push(format!("Source checkpoint: {}", checkpoint_name));
    }
    details.join("\n")
}

pub fn render_auth_profile_apply_report(report: &BrowserAuthProfileApplyReport) -> String {
    let mut details = vec![
        format!(
            "Applied browser auth profile '{}' to session '{}'",
            report.profile_name, report.target_session.id
        ),
        format!("Copied auth cookies: {}", report.copied_cookie_count),
        format!(
            "Copied local storage entries: {}",
            report.copied_local_storage_count
        ),
        format!(
            "Copied session storage entries: {}",
            report.copied_session_storage_count
        ),
        format!("Session JSON: {}", report.session_json_path),
        format!("Profile JSON: {}", report.profile_json_path),
        format!("Auth diagnosis: {}", report.auth_diagnostics.diagnosis),
        format!(
            "Auth recommendation: {}",
            report.auth_diagnostics.recommended_action
        ),
    ];
    if !report.copied_cookie_names.is_empty() {
        details.push(format!(
            "Cookie names: {}",
            report.copied_cookie_names.join(", ")
        ));
    }
    if !report.copied_local_storage_keys.is_empty() {
        details.push(format!(
            "Local storage keys: {}",
            report.copied_local_storage_keys.join(", ")
        ));
    }
    if !report.copied_session_storage_keys.is_empty() {
        details.push(format!(
            "Session storage keys: {}",
            report.copied_session_storage_keys.join(", ")
        ));
    }
    details.join("\n")
}

pub fn render_access_diagnostics_report(report: &BrowserAccessDiagnosticsReport) -> String {
    let mut details = vec![
        format!("Access diagnosis for session '{}'", report.session.id),
        format!("Diagnosis: {}", report.diagnosis),
        format!("Recommended action: {}", report.recommended_action),
        format!("Challenge signals: {}", report.challenge_signal_count),
        format!("Session JSON: {}", report.session_json_path),
    ];
    if let Some(router_name) = report.router_name.as_deref() {
        details.push(format!("Router: {}", router_name));
    }
    if !report.challenge_signals.is_empty() {
        details.push(format!("Signals: {}", report.challenge_signals.join(", ")));
    }
    if let Some(snapshot_json_path) = report.snapshot_json_path.as_deref() {
        details.push(format!("Snapshot JSON: {}", snapshot_json_path));
    }
    details.join("\n")
}

pub fn render_session_health_report(report: &BrowserSessionHealthReport) -> String {
    let mut details = vec![
        format!("Browser session health for '{}'", report.session.id),
        format!("Recovery posture: {}", report.recovery_posture),
        format!("Recommended action: {}", report.recommended_action),
        format!(
            "Current URL: {}",
            report.session.current_url.as_deref().unwrap_or("(none)")
        ),
        format!("Auth diagnosis: {}", report.auth_diagnostics.diagnosis),
        format!("Access diagnosis: {}", report.access_diagnostics.diagnosis),
        format!("Compatibility: {}", report.compatibility.level),
        format!("Compatibility cause: {}", report.compatibility.cause),
        format!("Compatibility summary: {}", report.compatibility.summary),
        format!(
            "Compatibility action: {}",
            report.compatibility.recommended_action
        ),
        format!("Checkpoint count: {}", report.checkpoint_count),
        format!(
            "Recent transcript failures: {}",
            report.recent_failure_count
        ),
        format!(
            "User-Agent: {}",
            report
                .network
                .user_agent
                .as_deref()
                .unwrap_or(default_browser_user_agent())
        ),
        format!("Network headers: {}", report.network.headers.len()),
        format!(
            "Allow prefixes: {}",
            report.network.allowed_url_prefixes.len()
        ),
        format!(
            "Block prefixes: {}",
            report.network.blocked_url_prefixes.len()
        ),
        format!("Session JSON: {}", report.session_json_path),
    ];
    if let Some(snapshot) = report.snapshot.as_ref() {
        details.push(format!("Snapshot title: {}", snapshot.title));
        if let Some(json_path) = snapshot.json_path.as_deref() {
            details.push(format!("Snapshot JSON: {}", json_path));
        }
    }
    if let Some(html_fallback_path) = report.html_fallback_path.as_deref() {
        details.push(format!("HTML fallback: {}", html_fallback_path));
    }
    if let Some(checkpoint) = report.latest_checkpoint.as_ref() {
        details.push(format!("Latest checkpoint: {}", checkpoint.name));
        if let Some(checkpoint_json_path) = checkpoint.checkpoint_json_path.as_deref() {
            details.push(format!("Latest checkpoint JSON: {}", checkpoint_json_path));
        }
    }
    if let Some(latest_failure) = report.latest_failure.as_ref() {
        details.push(format!(
            "Latest failure: #{} [{}] {}",
            latest_failure.sequence, latest_failure.event_kind, latest_failure.summary
        ));
    }
    if !report.recent_failures.is_empty() {
        for failure in &report.recent_failures {
            details.push(format!(
                "Recent failure #{} [{}] {}",
                failure.sequence, failure.event_kind, failure.summary
            ));
        }
    }
    if !report.evidence_signals.is_empty() {
        details.push(format!("Evidence: {}", report.evidence_signals.join(", ")));
    }
    details.join("\n")
}

pub fn set_session_storage_entries_report(
    workspace_root: &Path,
    session_id: &str,
    scope: &str,
    entries: &HashMap<String, String>,
) -> Result<BrowserStorageUpdateReport, String> {
    let mut session = load_session_state(workspace_root, session_id)?;
    match scope {
        "local" => apply_storage_updates(&mut session.local_storage, entries),
        "session" => apply_storage_updates(&mut session.session_storage, entries),
        _ => return Err(format!("unsupported browser storage scope: '{}'", scope)),
    }
    let path = save_session_state(workspace_root, &session)?;
    Ok(BrowserStorageUpdateReport {
        session: summarize_session(session),
        scope: scope.to_string(),
        updated_entry_count: entries.len(),
        session_json_path: path.display().to_string(),
    })
}

pub fn set_session_storage_entries(
    workspace_root: &Path,
    session_id: &str,
    scope: &str,
    entries: &HashMap<String, String>,
) -> Result<PathBuf, String> {
    let report = set_session_storage_entries_report(workspace_root, session_id, scope, entries)?;
    Ok(PathBuf::from(report.session_json_path))
}

pub fn get_session_storage_entries_report(
    workspace_root: &Path,
    session_id: &str,
    scope: &str,
) -> Result<BrowserStorageReadReport, String> {
    let session = load_session_state(workspace_root, session_id)?;
    let entries = match scope {
        "local" => session.local_storage.clone(),
        "session" => session.session_storage.clone(),
        _ => return Err(format!("unsupported browser storage scope: '{}'", scope)),
    };
    let entry_count = entries.len();
    let session_json_path = session_file_path(workspace_root, session_id)
        .display()
        .to_string();
    Ok(BrowserStorageReadReport {
        session: summarize_session(session),
        scope: scope.to_string(),
        entry_count,
        entries,
        session_json_path,
    })
}

pub fn get_session_storage_entries(
    workspace_root: &Path,
    session_id: &str,
    scope: &str,
) -> Result<String, String> {
    let report = get_session_storage_entries_report(workspace_root, session_id, scope)?;
    serde_json::to_string_pretty(&report.entries)
        .map_err(|err| format!("serialise browser storage state: {err}"))
}

pub fn get_session_cookies_report(
    workspace_root: &Path,
    session_id: &str,
) -> Result<BrowserCookieReadReport, String> {
    let session = load_session_state(workspace_root, session_id)?;
    let cookie_count = session.cookies.len();
    let cookie_names = summarize_cookie_names(&session.cookies);
    let session_json_path = session_file_path(workspace_root, session_id)
        .display()
        .to_string();
    Ok(BrowserCookieReadReport {
        session: summarize_session(session),
        cookie_count,
        cookie_names,
        session_json_path,
    })
}

pub fn get_session_cookies(workspace_root: &Path, session_id: &str) -> Result<String, String> {
    let session = load_session_state(workspace_root, session_id)?;
    serde_json::to_string_pretty(&session.cookies)
        .map_err(|err| format!("serialise browser cookies: {err}"))
}

pub fn set_session_cookies_report(
    workspace_root: &Path,
    session_id: &str,
    cookies: &[BrowserCookie],
) -> Result<BrowserCookieUpdateReport, String> {
    let mut session = load_session_state(workspace_root, session_id)?;
    for cookie in cookies.iter().cloned() {
        merge_cookie(&mut session.cookies, cookie);
    }
    sync_runtime_cookies_from_browser_cookies(&mut session);
    let cookie_count = session.cookies.len();
    let cookie_names = summarize_cookie_names(&session.cookies);
    let path = save_session_state(workspace_root, &session)?;
    Ok(BrowserCookieUpdateReport {
        session: summarize_session(session),
        updated_cookie_count: cookies.len(),
        cookie_count,
        cookie_names,
        session_json_path: path.display().to_string(),
    })
}

pub fn set_session_cookies(
    workspace_root: &Path,
    session_id: &str,
    cookies: &[BrowserCookie],
) -> Result<PathBuf, String> {
    let report = set_session_cookies_report(workspace_root, session_id, cookies)?;
    Ok(PathBuf::from(report.session_json_path))
}
