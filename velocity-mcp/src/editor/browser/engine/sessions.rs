use super::*;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn render_session_create_report(report: &BrowserSessionCreateReport) -> String {
    format!(
        "Created browser session '{}'\nSession JSON: {}",
        report.session.id, report.session_json_path,
    )
}

pub fn render_session_network_read_report(report: &BrowserSessionNetworkReadReport) -> String {
    format!(
        "Browser session network config for '{}'\nUser-Agent: {}\nHeaders: {}\nTimeout ms: {}\nFollow redirects: {}\nAllow prefixes: {}\nBlock prefixes: {}\nSession JSON: {}",
        report.session.id,
        report.network.user_agent.as_deref().unwrap_or(default_browser_user_agent()),
        report.network.headers.len(),
        report.network.timeout_ms.map(|value: u64| value.to_string()).unwrap_or_else(|| "default".to_string()),
        report.network.follow_redirects.map(|value: bool| value.to_string()).unwrap_or_else(|| "default".to_string()),
        report.network.allowed_url_prefixes.len(),
        report.network.blocked_url_prefixes.len(),
        report.session_json_path,
    )
}

pub fn render_session_network_update_report(report: &BrowserSessionNetworkUpdateReport) -> String {
    format!(
        "Updated browser session network config for '{}'\nUser-Agent: {}\nHeaders: {}\nTimeout ms: {}\nFollow redirects: {}\nAllow prefixes: {}\nBlock prefixes: {}\nSession JSON: {}",
        report.session.id,
        report.network.user_agent.as_deref().unwrap_or(default_browser_user_agent()),
        report.network.headers.len(),
        report.network.timeout_ms.map(|value: u64| value.to_string()).unwrap_or_else(|| "default".to_string()),
        report.network.follow_redirects.map(|value: bool| value.to_string()).unwrap_or_else(|| "default".to_string()),
        report.network.allowed_url_prefixes.len(),
        report.network.blocked_url_prefixes.len(),
        report.session_json_path,
    )
}

pub fn create_session_report(
    workspace_root: &Path,
    session_id: &str,
) -> Result<BrowserSessionCreateReport, String> {
    let session = empty_browser_session_state(session_id);
    let path = save_session_state(workspace_root, &session)?;
    Ok(BrowserSessionCreateReport {
        session: summarize_session(session),
        session_json_path: path.display().to_string(),
    })
}

#[allow(dead_code)]
pub fn create_session(workspace_root: &Path, session_id: &str) -> Result<PathBuf, String> {
    let report = create_session_report(workspace_root, session_id)?;
    Ok(PathBuf::from(report.session_json_path))
}

pub fn runtime_session_state_to_json(
    session: &RuntimeBrowserSessionState,
) -> Result<String, String> {
    serde_json::to_string_pretty(session)
        .map_err(|err| format!("serialise runtime browser session: {err}"))
}

pub fn save_runtime_session_state(
    workspace_root: &Path,
    session: &RuntimeBrowserSessionState,
) -> Result<PathBuf, String> {
    let path = runtime_session_file_path(workspace_root, &session.id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create runtime session dir: {err}"))?;
    }
    let json = serde_json::to_vec_pretty(session)
        .map_err(|err| format!("serialise runtime browser session: {err}"))?;
    fs::write(&path, json).map_err(|err| format!("write runtime browser session: {err}"))?;
    Ok(path)
}

pub fn load_runtime_session_state(
    workspace_root: &Path,
    session_id: &str,
) -> Result<RuntimeBrowserSessionState, String> {
    let path = runtime_session_file_path(workspace_root, session_id);
    let raw = fs::read(&path).map_err(|err| format!("read runtime browser session: {err}"))?;
    serde_json::from_slice(&raw).map_err(|err| format!("parse runtime browser session: {err}"))
}

pub fn runtime_cookie_as_browser_cookie(cookie: &RuntimeBrowserCookie) -> BrowserCookie {
    BrowserCookie {
        name: cookie.name.clone(),
        value: cookie.value.clone(),
    }
}

pub fn browser_cookie_as_runtime_cookie(cookie: &BrowserCookie) -> RuntimeBrowserCookie {
    RuntimeBrowserCookie {
        name: cookie.name.clone(),
        value: cookie.value.clone(),
        ..RuntimeBrowserCookie::default()
    }
}

fn runtime_session_as_browser_session(
    runtime_session: &RuntimeBrowserSessionState,
) -> BrowserSessionState {
    BrowserSessionState {
        id: runtime_session.id.clone(),
        current_url: runtime_session.current_url.clone(),
        cookies: runtime_session
            .cookies
            .iter()
            .map(runtime_cookie_as_browser_cookie)
            .collect(),
        runtime_cookies: runtime_session.cookies.clone(),
        local_storage: runtime_session.local_storage.clone(),
        session_storage: runtime_session.session_storage.clone(),
        network: BrowserSessionNetworkConfig::default(),
        last_html: None,
    }
}

pub fn build_runtime_auth_diagnostics_report(
    workspace_root: &Path,
    session: &RuntimeBrowserSessionState,
    sitemap_path: &Path,
) -> BrowserAuthDiagnosticsReport {
    let snapshot = match session.current_url.as_deref() {
        Some(url) => load_snapshot_json(url, sitemap_path).ok(),
        None => None,
    };
    let snapshot_json_path = snapshot.as_ref().map(|snapshot| {
        browser_snapshot_path(&snapshot.url, sitemap_path)
            .display()
            .to_string()
    });
    let mut report = build_auth_diagnostics_report(
        workspace_root,
        runtime_session_as_browser_session(session),
        snapshot,
        snapshot_json_path,
    );
    report.session_json_path = runtime_session_file_path(workspace_root, &session.id)
        .display()
        .to_string();
    report
}

pub fn save_session_state(
    workspace_root: &Path,
    session: &BrowserSessionState,
) -> Result<PathBuf, String> {
    let path = session_file_path(workspace_root, &session.id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create session dir: {err}"))?;
    }
    let mut serializable = session.clone();
    if serializable.runtime_cookies.is_empty() && !serializable.cookies.is_empty() {
        sync_runtime_cookies_from_browser_cookies(&mut serializable);
    }
    let json = serde_json::to_vec_pretty(&serializable)
        .map_err(|err| format!("serialise browser session: {err}"))?;
    fs::write(&path, json).map_err(|err| format!("write browser session: {err}"))?;
    Ok(path)
}

pub fn load_session_state(
    workspace_root: &Path,
    session_id: &str,
) -> Result<BrowserSessionState, String> {
    let path = session_file_path(workspace_root, session_id);
    let raw = fs::read(&path).map_err(|err| format!("read browser session: {err}"))?;
    let mut session: BrowserSessionState =
        serde_json::from_slice(&raw).map_err(|err| format!("parse browser session: {err}"))?;
    if session.runtime_cookies.is_empty() && !session.cookies.is_empty() {
        sync_runtime_cookies_from_browser_cookies(&mut session);
    }
    Ok(session)
}

pub fn read_session_report(
    workspace_root: &Path,
    session_id: &str,
) -> Result<BrowserSessionReadReport, String> {
    let session = load_session_state(workspace_root, session_id)?;
    Ok(BrowserSessionReadReport {
        session: summarize_session(session),
        session_json_path: session_file_path(workspace_root, session_id)
            .display()
            .to_string(),
    })
}

pub fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration: Duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn load_session_transcript_entries(
    workspace_root: &Path,
    session_id: &str,
) -> Result<Vec<BrowserSessionTranscriptEntry>, String> {
    let path = browser_session_transcript_path(workspace_root, session_id);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read(&path).map_err(|err| format!("read browser session transcript: {err}"))?;
    serde_json::from_slice(&raw).map_err(|err| format!("parse browser session transcript: {err}"))
}

fn save_session_transcript_entries(
    workspace_root: &Path,
    session_id: &str,
    entries: &[BrowserSessionTranscriptEntry],
) -> Result<PathBuf, String> {
    let path = browser_session_transcript_path(workspace_root, session_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create browser session transcript dir: {err}"))?;
    }
    let json = serde_json::to_vec_pretty(entries)
        .map_err(|err| format!("serialise browser session transcript: {err}"))?;
    fs::write(&path, json).map_err(|err| format!("write browser session transcript: {err}"))?;
    Ok(path)
}

pub fn append_session_transcript_entry(
    workspace_root: &Path,
    session_id: &str,
    mut entry: BrowserSessionTranscriptEntry,
) -> Result<PathBuf, String> {
    let mut entries = load_session_transcript_entries(workspace_root, session_id)?;
    entry.sequence = entries.last().map(|value| value.sequence + 1).unwrap_or(1);
    entries.push(entry);
    save_session_transcript_entries(workspace_root, session_id, &entries)
}

pub fn append_session_failure_transcript_entry(
    workspace_root: &Path,
    session_id: &str,
    event_kind: &str,
    target: Option<String>,
    summary: String,
    session_json_path: String,
) {
    let _ = append_session_transcript_entry(
        workspace_root,
        session_id,
        BrowserSessionTranscriptEntry {
            sequence: 0,
            timestamp_ms: current_timestamp_ms(),
            event_kind: event_kind.to_string(),
            outcome: "error".to_string(),
            summary,
            session_id: session_id.to_string(),
            url: None,
            title: None,
            target,
            diff_summary: None,
            request_count: 0,
            settle_signal_count: 0,
            runtime_state_count: 0,
            protocol_event_count: 0,
            network_summary: BrowserNetworkSummary::default(),
            session_json_path,
            snapshot_json_path: None,
            checkpoint_json_path: None,
            nda_facts_path: None,
            html_fallback_path: None,
        },
    );
}

pub fn recent_failed_session_transcript_entries(
    workspace_root: &Path,
    session_id: &str,
    limit: usize,
) -> Result<Vec<BrowserSessionTranscriptEntrySummary>, String> {
    let mut entries = load_session_transcript_entries(workspace_root, session_id)?
        .into_iter()
        .filter(|entry| !entry.outcome.eq_ignore_ascii_case("ok"))
        .collect::<Vec<_>>();
    entries.sort_by_key(|e| std::cmp::Reverse(e.sequence));
    entries.truncate(limit);
    Ok(entries
        .into_iter()
        .map(summarize_session_transcript_entry)
        .collect())
}

pub fn read_session_transcript_report(
    workspace_root: &Path,
    session_id: &str,
    limit: Option<usize>,
    sort_direction: BrowserListSortDirection,
) -> Result<BrowserSessionTranscriptReadReport, String> {
    let session = load_session_state(workspace_root, session_id)?;
    let mut entries = load_session_transcript_entries(workspace_root, session_id)?;
    finalize_list(&mut entries, sort_direction, limit, |left, right| {
        left.sequence.cmp(&right.sequence)
    });
    let latest_sequence = entries.iter().map(|entry| entry.sequence).max();
    Ok(BrowserSessionTranscriptReadReport {
        session: summarize_session(session),
        entry_count: entries.len(),
        latest_sequence,
        transcript_json_path: browser_session_transcript_path(workspace_root, session_id)
            .display()
            .to_string(),
        entries: entries
            .into_iter()
            .map(summarize_session_transcript_entry)
            .collect(),
    })
}

pub fn read_session_transcript_entry(
    workspace_root: &Path,
    session_id: &str,
    sequence: u64,
) -> Result<BrowserSessionTranscriptEntry, String> {
    load_session_transcript_entries(workspace_root, session_id)?
        .into_iter()
        .find(|entry| entry.sequence == sequence)
        .ok_or_else(|| {
            format!(
                "browser session transcript entry {} not found for '{}'",
                sequence, session_id
            )
        })
}

pub fn read_session_network_report(
    workspace_root: &Path,
    session_id: &str,
) -> Result<BrowserSessionNetworkReadReport, String> {
    let mut session = load_session_state(workspace_root, session_id)?;
    normalize_network_config(&mut session.network);
    Ok(BrowserSessionNetworkReadReport {
        session: summarize_session(session.clone()),
        network: session.network,
        session_json_path: session_file_path(workspace_root, session_id)
            .display()
            .to_string(),
    })
}

pub fn update_session_network_report(
    workspace_root: &Path,
    session_id: &str,
    user_agent: Option<&str>,
    headers: Option<HashMap<String, String>>,
    timeout_ms: Option<u64>,
    clear_timeout: bool,
    follow_redirects: Option<bool>,
    clear_follow_redirects: bool,
    allowed_url_prefixes: Option<Vec<String>>,
    blocked_url_prefixes: Option<Vec<String>>,
    replace_headers: bool,
) -> Result<BrowserSessionNetworkUpdateReport, String> {
    let mut session = load_session_state(workspace_root, session_id)?;
    if let Some(user_agent) = user_agent {
        let trimmed = user_agent.trim();
        session.network.user_agent = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
    }
    if let Some(headers) = headers {
        if replace_headers {
            session.network.headers = headers;
        } else {
            for (key, value) in headers {
                session.network.headers.insert(key, value);
            }
        }
    }
    if clear_timeout {
        session.network.timeout_ms = None;
    } else if let Some(timeout_ms) = timeout_ms {
        session.network.timeout_ms = Some(timeout_ms);
    }
    if clear_follow_redirects {
        session.network.follow_redirects = None;
    } else if let Some(follow_redirects) = follow_redirects {
        session.network.follow_redirects = Some(follow_redirects);
    }
    if let Some(allowed_url_prefixes) = allowed_url_prefixes {
        session.network.allowed_url_prefixes = allowed_url_prefixes;
    }
    if let Some(blocked_url_prefixes) = blocked_url_prefixes {
        session.network.blocked_url_prefixes = blocked_url_prefixes;
    }
    normalize_network_config(&mut session.network);
    let updated_header_count = session.network.headers.len();
    let path = save_session_state(workspace_root, &session)?;
    Ok(BrowserSessionNetworkUpdateReport {
        session: summarize_session(session.clone()),
        network: session.network,
        updated_header_count,
        session_json_path: path.display().to_string(),
    })
}

pub fn list_sessions(
    workspace_root: &Path,
    session_id_contains: Option<&str>,
    url_contains: Option<&str>,
    limit: Option<usize>,
    sort_direction: BrowserListSortDirection,
) -> Result<Vec<BrowserSessionSummary>, String> {
    let dir = workspace_root.join(".velocity").join("browser-sessions");
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|err| format!("read browser session dir: {err}"))? {
        let entry: fs::DirEntry = match entry {
            Ok(e) => e,
            Err(err) => return Err(format!("read browser session dir entry: {err}")),
        };
        let path = entry.path();
        if path
            .extension()
            .and_then(|ext: &std::ffi::OsStr| ext.to_str())
            != Some("json")
        {
            continue;
        }
        let raw = fs::read(&path).map_err(|err| format!("read browser session: {err}"))?;
        let session: BrowserSessionState =
            serde_json::from_slice(&raw).map_err(|err| format!("parse browser session: {err}"))?;
        let mut summary = summarize_session(session);
        summary.session_json_path = Some(path.display().to_string());
        if session_id_contains
            .map(|needle| contains_case_insensitive(&summary.id, needle))
            .unwrap_or(true)
            && url_contains
                .map(|needle| {
                    summary
                        .current_url
                        .as_deref()
                        .map(|url| contains_case_insensitive(url, needle))
                        .unwrap_or(false)
                })
                .unwrap_or(true)
        {
            items.push(summary);
        }
    }
    finalize_list(&mut items, sort_direction, limit, |left, right| {
        left.id.cmp(&right.id)
    });
    Ok(items)
}

pub fn session_state_to_json(session: &BrowserSessionState) -> Result<String, String> {
    serde_json::to_string_pretty(session)
        .map_err(|err| format!("serialise browser session state: {err}"))
}
