#![allow(dead_code)]

use super::*;
use std::fs;
use std::path::{Path, PathBuf};

fn write_session_checkpoint(
    workspace_root: &Path,
    checkpoint: &BrowserSessionCheckpoint,
) -> Result<PathBuf, String> {
    let path =
        browser_session_checkpoint_path(workspace_root, &checkpoint.session.id, &checkpoint.name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create checkpoint dir: {err}"))?;
    }
    let json = serde_json::to_vec_pretty(checkpoint)
        .map_err(|err| format!("serialise browser checkpoint: {err}"))?;
    fs::write(&path, json).map_err(|err| format!("write browser checkpoint: {err}"))?;
    Ok(path)
}

pub fn persist_checkpoint_from_replay_state(
    workspace_root: &Path,
    state: &BrowserReplayState,
    checkpoint_name: &str,
) -> Result<PathBuf, String> {
    save_session_state(workspace_root, &state.session)?;
    let checkpoint = BrowserSessionCheckpoint {
        name: checkpoint_name.to_string(),
        session: state.session.clone(),
        snapshot: Some(state.snapshot.clone()),
    };
    write_session_checkpoint(workspace_root, &checkpoint)
}

pub fn render_session_transcript_report(report: &BrowserSessionTranscriptReadReport) -> String {
    let mut lines = vec![
        format!("Browser session transcript for '{}'", report.session.id),
        format!("Entries: {}", report.entry_count),
        format!("Transcript JSON: {}", report.transcript_json_path),
    ];
    if let Some(sequence) = report.latest_sequence {
        lines.push(format!("Latest sequence: {}", sequence));
    }
    for entry in &report.entries {
        lines.push(format!(
            "#{} [{}] {} - {}",
            entry.sequence, entry.event_kind, entry.outcome, entry.summary
        ));
    }
    lines.join("\n")
}

pub fn render_checkpoint_save_report(report: &BrowserCheckpointSaveReport) -> String {
    format!(
        "Saved browser checkpoint '{}' for session '{}'\nCheckpoint JSON: {}",
        report.checkpoint.name, report.checkpoint.session_id, report.checkpoint_json_path,
    )
}

pub fn save_session_checkpoint_report(
    workspace_root: &Path,
    session_id: &str,
    checkpoint_name: &str,
    sitemap_path: &Path,
) -> Result<BrowserCheckpointSaveReport, String> {
    let session = load_session_state(workspace_root, session_id)?;
    let snapshot = match session.current_url.as_deref() {
        Some(url) => load_snapshot_json(url, sitemap_path).ok(),
        None => None,
    };
    let checkpoint = BrowserSessionCheckpoint {
        name: checkpoint_name.to_string(),
        session,
        snapshot,
    };
    let path = write_session_checkpoint(workspace_root, &checkpoint)?;
    let report = BrowserCheckpointSaveReport {
        checkpoint: summarize_session_checkpoint(checkpoint),
        checkpoint_json_path: path.display().to_string(),
    };
    append_session_transcript_entry(
        workspace_root,
        session_id,
        BrowserSessionTranscriptEntry {
            sequence: 0,
            timestamp_ms: current_timestamp_ms(),
            event_kind: "save_checkpoint".to_string(),
            outcome: "ok".to_string(),
            summary: format!("Saved checkpoint '{}'", report.checkpoint.name),
            session_id: report.checkpoint.session_id.clone(),
            url: report.checkpoint.current_url.clone(),
            title: report.checkpoint.title.clone(),
            target: Some(report.checkpoint.name.clone()),
            diff_summary: None,
            request_count: report.checkpoint.request_count,
            settle_signal_count: report.checkpoint.settle_signal_count,
            runtime_state_count: report.checkpoint.runtime_state_count,
            protocol_event_count: report.checkpoint.protocol_event_count,
            network_summary: report.checkpoint.network_summary.clone(),
            session_json_path: session_file_path(workspace_root, session_id)
                .display()
                .to_string(),
            snapshot_json_path: report.checkpoint.current_url.as_deref().map(|url| {
                browser_snapshot_path(url, sitemap_path)
                    .display()
                    .to_string()
            }),
            checkpoint_json_path: Some(report.checkpoint_json_path.clone()),
            nda_facts_path: report
                .checkpoint
                .current_url
                .as_deref()
                .map(|url| crawl_facts_path(url, sitemap_path).display().to_string()),
            html_fallback_path: report.checkpoint.current_url.as_deref().and_then(|url| {
                let path = browser_html_fallback_path(url, sitemap_path);
                path.exists().then(|| path.display().to_string())
            }),
        },
    )?;
    Ok(report)
}

pub fn save_session_checkpoint(
    workspace_root: &Path,
    session_id: &str,
    checkpoint_name: &str,
    sitemap_path: &Path,
) -> Result<PathBuf, String> {
    match save_session_checkpoint_report(workspace_root, session_id, checkpoint_name, sitemap_path)
    {
        Ok(report) => Ok(PathBuf::from(report.checkpoint_json_path)),
        Err(err) => {
            append_session_failure_transcript_entry(
                workspace_root,
                session_id,
                "save_checkpoint",
                Some(checkpoint_name.to_string()),
                format!("Failed to save checkpoint '{}': {}", checkpoint_name, err),
                session_file_path(workspace_root, session_id)
                    .display()
                    .to_string(),
            );
            Err(err)
        }
    }
}

pub fn read_session_checkpoint(
    workspace_root: &Path,
    session_id: &str,
    checkpoint_name: &str,
) -> Result<BrowserSessionCheckpoint, String> {
    let path = browser_session_checkpoint_path(workspace_root, session_id, checkpoint_name);
    let raw = fs::read(&path).map_err(|err| format!("read browser checkpoint: {err}"))?;
    serde_json::from_slice(&raw).map_err(|err| format!("parse browser checkpoint: {err}"))
}

pub fn read_session_checkpoint_report(
    workspace_root: &Path,
    session_id: &str,
    checkpoint_name: &str,
) -> Result<BrowserCheckpointReadReport, String> {
    let checkpoint = read_session_checkpoint(workspace_root, session_id, checkpoint_name)?;
    Ok(BrowserCheckpointReadReport {
        checkpoint: summarize_session_checkpoint(checkpoint),
        checkpoint_json_path: browser_session_checkpoint_path(
            workspace_root,
            session_id,
            checkpoint_name,
        )
        .display()
        .to_string(),
    })
}

pub fn diff_session_checkpoints(
    workspace_root: &Path,
    session_id: &str,
    before_checkpoint_name: &str,
    after_checkpoint_name: &str,
) -> Result<BrowserSnapshotDiffReport, String> {
    let before = read_session_checkpoint(workspace_root, session_id, before_checkpoint_name)?;
    let after = read_session_checkpoint(workspace_root, session_id, after_checkpoint_name)?;
    let before_snapshot = before.snapshot.ok_or_else(|| {
        format!(
            "checkpoint '{}' does not include a snapshot",
            before_checkpoint_name
        )
    })?;
    let after_snapshot = after.snapshot.ok_or_else(|| {
        format!(
            "checkpoint '{}' does not include a snapshot",
            after_checkpoint_name
        )
    })?;
    let diff = diff_snapshots(&before_snapshot, &after_snapshot);
    Ok(BrowserSnapshotDiffReport {
        before_url: before_snapshot.url,
        after_url: after_snapshot.url,
        summary: summarize_snapshot_diff(&diff),
        diff,
    })
}

pub fn read_checkpoint_diff_report(
    workspace_root: &Path,
    session_id: &str,
    before_checkpoint_name: &str,
    after_checkpoint_name: &str,
) -> Result<BrowserSnapshotDiffReadReport, String> {
    let report = diff_session_checkpoints(
        workspace_root,
        session_id,
        before_checkpoint_name,
        after_checkpoint_name,
    )?;
    Ok(BrowserSnapshotDiffReadReport {
        diff: summarize_snapshot_diff_report(report),
        before_json_path: browser_session_checkpoint_path(
            workspace_root,
            session_id,
            before_checkpoint_name,
        )
        .display()
        .to_string(),
        after_json_path: browser_session_checkpoint_path(
            workspace_root,
            session_id,
            after_checkpoint_name,
        )
        .display()
        .to_string(),
    })
}

pub fn list_session_checkpoints(
    workspace_root: &Path,
    session_id: &str,
    checkpoint_name_contains: Option<&str>,
    title_contains: Option<&str>,
    limit: Option<usize>,
    sort_direction: BrowserListSortDirection,
) -> Result<Vec<BrowserSessionCheckpointSummary>, String> {
    let dir = workspace_root
        .join(".velocity")
        .join("browser-session-checkpoints")
        .join(sanitize_file_stem(session_id));
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|err| format!("read checkpoint dir: {err}"))? {
        let entry: fs::DirEntry = match entry {
            Ok(e) => e,
            Err(err) => return Err(format!("read checkpoint dir entry: {err}")),
        };
        let path = entry.path();
        if path
            .extension()
            .and_then(|ext: &std::ffi::OsStr| ext.to_str())
            != Some("json")
        {
            continue;
        }
        let raw = fs::read(&path).map_err(|err| format!("read browser checkpoint: {err}"))?;
        let checkpoint: BrowserSessionCheckpoint = serde_json::from_slice(&raw)
            .map_err(|err| format!("parse browser checkpoint: {err}"))?;
        let mut summary = summarize_session_checkpoint(checkpoint);
        summary.checkpoint_json_path = Some(path.display().to_string());
        if checkpoint_name_contains
            .map(|needle| contains_case_insensitive(&summary.name, needle))
            .unwrap_or(true)
            && title_contains
                .map(|needle| {
                    summary
                        .title
                        .as_deref()
                        .map(|title| contains_case_insensitive(title, needle))
                        .unwrap_or(false)
                })
                .unwrap_or(true)
        {
            items.push(summary);
        }
    }
    finalize_list(&mut items, sort_direction, limit, |left, right| {
        left.name.cmp(&right.name)
    });
    Ok(items)
}

pub fn render_visual_fallback_report(report: &BrowserVisualFallbackReadReport) -> String {
    format!(
        "Browser HTML fallback available.\nURL: {}\nBytes: {}\nHTML path: {}",
        report.url, report.byte_count, report.html_path,
    )
}

pub fn render_checkpoint_restore_report(report: &BrowserCheckpointRestoreReport) -> String {
    let mut details = vec![
        format!(
            "Restored browser session checkpoint '{}'",
            report.checkpoint_name
        ),
        format!("Session: {}", report.session_id),
        format!("Session JSON: {}", report.session_json_path),
        format!("Auth diagnosis: {}", report.auth_diagnostics.diagnosis),
        format!(
            "Auth recommendation: {}",
            report.auth_diagnostics.recommended_action
        ),
    ];

    if let Some(url) = report.url.as_deref() {
        details.push(format!("URL: {}", url));
    }
    if let Some(title) = report.title.as_deref() {
        details.push(format!("Title: {}", title));
    }
    details.push(format!("Requests: {}", report.request_count));
    details.push(format!("Settle signals: {}", report.settle_signal_count));
    details.push(format!("Runtime state: {}", report.runtime_state_count));
    details.push(format!("Protocol events: {}", report.protocol_event_count));
    details.push(format!(
        "Has login form: {}",
        report.auth_diagnostics.has_login_form
    ));
    details.push(format!(
        "Has auth cookie: {}",
        report.auth_diagnostics.has_auth_cookie
    ));
    details.push(format!(
        "Has CSRF token: {}",
        report.auth_diagnostics.has_csrf_token
    ));
    if let Some(auth_state) = report.auth_diagnostics.auth_state.as_deref() {
        details.push(format!("Auth state: {}", auth_state));
    }
    if let Some(router_name) = report.auth_diagnostics.router_name.as_deref() {
        details.push(format!("Router: {}", router_name));
    }
    if let Some(network) = render_network_summary(&report.network_summary) {
        details.push(format!("Network summary: {}", network));
    }
    if !report.auth_diagnostics.auth_signals.is_empty() {
        details.push(format!(
            "Auth signals: {}",
            report.auth_diagnostics.auth_signals.join(", ")
        ));
    }
    if let Some(snapshot_json_path) = report.snapshot_json_path.as_deref() {
        details.push(format!("Snapshot JSON: {}", snapshot_json_path));
    }
    if let Some(html_fallback_path) = report.html_fallback_path.as_deref() {
        details.push(format!("HTML fallback: {}", html_fallback_path));
    }
    if let Some(nda_facts_path) = report.nda_facts_path.as_deref() {
        details.push(format!("NDA Facts: {}", nda_facts_path));
    }

    details.join("\n")
}

pub fn restore_session_checkpoint_report(
    workspace_root: &Path,
    session_id: &str,
    checkpoint_name: &str,
    target_session_id: Option<&str>,
    sitemap_path: &Path,
) -> Result<BrowserCheckpointRestoreReport, String> {
    let mut checkpoint = read_session_checkpoint(workspace_root, session_id, checkpoint_name)?;
    if let Some(target) = target_session_id {
        checkpoint.session.id = target.to_string();
    }
    let session_path = save_session_state(workspace_root, &checkpoint.session)?;
    let local_storage_count = checkpoint.session.local_storage.len();
    let session_storage_count = checkpoint.session.session_storage.len();
    let session_id = checkpoint.session.id.clone();
    let checkpoint_name = checkpoint.name.clone();

    let (
        url,
        title,
        request_count,
        settle_signal_count,
        runtime_state_count,
        protocol_event_count,
        network_summary,
        restored_snapshot,
        snapshot_json_path,
        nda_facts_path,
        html_fallback_path,
    ): (
        Option<String>,
        Option<String>,
        usize,
        usize,
        usize,
        usize,
        BrowserNetworkSummary,
        Option<BrowserPageSnapshot>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = if let Some(snapshot) = checkpoint.snapshot {
        let network_summary = summarize_network_activity(&snapshot.protocol_events);
        persist_snapshot_to_sitemap(&snapshot, sitemap_path)?;
        let facts_path = write_crawl_facts(
            &snapshot.url,
            &snapshot.title,
            &snapshot.summary,
            &snapshot.elements,
            &snapshot.forms,
            &snapshot.cookies,
            &snapshot.storage,
            &snapshot.mutations,
            &snapshot.requests,
            &snapshot.settle_signals,
            &snapshot.runtime_state,
            &snapshot.protocol_events,
            sitemap_path,
        )?;
        let snapshot_path = write_snapshot_json(&snapshot, sitemap_path)?;
        let html_fallback_path = browser_html_fallback_path(&snapshot.url, sitemap_path);
        (
            Some(snapshot.url.clone()),
            Some(snapshot.title.clone()),
            snapshot.requests.len(),
            snapshot.settle_signals.len(),
            snapshot.runtime_state.len(),
            snapshot.protocol_events.len(),
            network_summary,
            Some(snapshot),
            Some(snapshot_path.display().to_string()),
            Some(facts_path.display().to_string()),
            html_fallback_path
                .exists()
                .then(|| html_fallback_path.display().to_string()),
        )
    } else {
        (
            None,
            None,
            0,
            0,
            0,
            0,
            BrowserNetworkSummary::default(),
            None,
            None,
            None,
            None,
        )
    };
    let auth_diagnostics = build_auth_diagnostics_report(
        workspace_root,
        checkpoint.session.clone(),
        restored_snapshot,
        snapshot_json_path.clone(),
    );

    let report = BrowserCheckpointRestoreReport {
        checkpoint_name,
        session_id: session_id.clone(),
        url,
        title,
        request_count,
        settle_signal_count,
        runtime_state_count,
        protocol_event_count,
        network_summary,
        local_storage_count,
        session_storage_count,
        session_json_path: session_path.display().to_string(),
        snapshot_json_path,
        nda_facts_path,
        html_fallback_path,
        auth_diagnostics,
    };
    append_session_transcript_entry(
        workspace_root,
        &report.session_id,
        BrowserSessionTranscriptEntry {
            sequence: 0,
            timestamp_ms: current_timestamp_ms(),
            event_kind: "restore_checkpoint".to_string(),
            outcome: "ok".to_string(),
            summary: format!("Restored checkpoint '{}'", report.checkpoint_name),
            session_id: report.session_id.clone(),
            url: report.url.clone(),
            title: report.title.clone(),
            target: Some(report.checkpoint_name.clone()),
            diff_summary: None,
            request_count: report.request_count,
            settle_signal_count: report.settle_signal_count,
            runtime_state_count: report.runtime_state_count,
            protocol_event_count: report.protocol_event_count,
            network_summary: report.network_summary.clone(),
            session_json_path: report.session_json_path.clone(),
            snapshot_json_path: report.snapshot_json_path.clone(),
            checkpoint_json_path: Some(
                browser_session_checkpoint_path(
                    workspace_root,
                    session_id.as_str(),
                    report.checkpoint_name.as_str(),
                )
                .display()
                .to_string(),
            ),
            nda_facts_path: report.nda_facts_path.clone(),
            html_fallback_path: report.html_fallback_path.clone(),
        },
    )?;
    Ok(report)
}

pub fn restore_session_checkpoint(
    workspace_root: &Path,
    session_id: &str,
    checkpoint_name: &str,
    target_session_id: Option<&str>,
    sitemap_path: &Path,
) -> Result<String, String> {
    match restore_session_checkpoint_report(
        workspace_root,
        session_id,
        checkpoint_name,
        target_session_id,
        sitemap_path,
    ) {
        Ok(report) => Ok(render_checkpoint_restore_report(&report)),
        Err(err) => {
            append_session_failure_transcript_entry(
                workspace_root,
                target_session_id.unwrap_or(session_id),
                "restore_checkpoint",
                Some(checkpoint_name.to_string()),
                format!(
                    "Failed to restore checkpoint '{}': {}",
                    checkpoint_name, err
                ),
                session_file_path(workspace_root, target_session_id.unwrap_or(session_id))
                    .display()
                    .to_string(),
            );
            Err(err)
        }
    }
}

pub fn describe_url_resolution(requested_url: &str, resolved_url: &str) -> String {
    if requested_url == resolved_url {
        format!("URL: {}", resolved_url)
    } else {
        format!(
            "Requested URL: {}\nResolved URL: {}",
            requested_url, resolved_url
        )
    }
}
