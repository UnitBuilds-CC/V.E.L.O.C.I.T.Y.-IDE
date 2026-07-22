use super::*;
use std::path::{Path, PathBuf};

fn build_session_action_report(
    workspace_root: &Path,
    sitemap_path: &Path,
    action: &str,
    target: &str,
    target_actionability: Option<BrowserTargetActionability>,
    before: &BrowserPageSnapshot,
    state: &BrowserReplayState,
) -> Result<BrowserSessionActionReport, String> {
    let diff = diff_snapshots(before, &state.snapshot);
    let (snapshot_path, session_path, facts_path, html_fallback_path): (PathBuf, PathBuf, PathBuf, Option<PathBuf>) =
        persist_replay_state(workspace_root, state, sitemap_path)?;
    let report = BrowserSessionActionReport {
        action: action.to_string(),
        target: target.to_string(),
        session_id: state.session.id.clone(),
        url: state.snapshot.url.clone(),
        title: state.snapshot.title.clone(),
        diff_summary: render_snapshot_diff(&diff),
        form_count: state.snapshot.forms.len(),
        cookie_count: state.session.cookies.len(),
        request_count: state.snapshot.requests.len(),
        settle_signal_count: state.snapshot.settle_signals.len(),
        runtime_state_count: state.snapshot.runtime_state.len(),
        protocol_event_count: state.snapshot.protocol_events.len(),
        network_summary: summarize_network_activity(&state.snapshot.protocol_events),
        local_storage_count: state.session.local_storage.len(),
        session_storage_count: state.session.session_storage.len(),
        snapshot_json_path: snapshot_path.display().to_string(),
        session_json_path: session_path.display().to_string(),
        nda_facts_path: facts_path.display().to_string(),
        html_fallback_path: html_fallback_path.map(|path: PathBuf| path.display().to_string()),
        target_actionability,
    };
    append_session_transcript_entry(
        workspace_root,
        &report.session_id,
        BrowserSessionTranscriptEntry {
            sequence: 0,
            timestamp_ms: current_timestamp_ms(),
            event_kind: report.action.clone(),
            outcome: "ok".to_string(),
            summary: format!("{} -> {}", report.action, report.target),
            session_id: report.session_id.clone(),
            url: Some(report.url.clone()),
            title: Some(report.title.clone()),
            target: Some(report.target.clone()),
            diff_summary: Some(report.diff_summary.clone()),
            request_count: report.request_count,
            settle_signal_count: report.settle_signal_count,
            runtime_state_count: report.runtime_state_count,
            protocol_event_count: report.protocol_event_count,
            network_summary: report.network_summary.clone(),
            session_json_path: report.session_json_path.clone(),
            snapshot_json_path: Some(report.snapshot_json_path.clone()),
            checkpoint_json_path: None,
            nda_facts_path: Some(report.nda_facts_path.clone()),
            html_fallback_path: report.html_fallback_path.clone(),
        },
    )?;
    Ok(report)
}

pub fn session_click_report(
    workspace_root: &Path,
    session_id: &str,
    role: &str,
    name: &str,
    sitemap_path: &Path,
) -> Result<BrowserSessionActionReport, String> {
    let mut state = load_session_replay_state(workspace_root, session_id, sitemap_path)?;
    let before = state.snapshot.clone();
    let target = find_element(&state.snapshot, role, name).ok_or_else(|| {
        format!(
            "session click target not found: role='{}' name='{}'",
            role, name
        )
    })?;
    let target_actionability = describe_element_actionability(target);
    let matched_name = target.name.clone();
    let target_url = target.target_url.clone().ok_or_else(|| {
        format!(
            "session click target '{}' is not actionable: {}",
            matched_name, target_actionability.reason
        )
    })?;
    state.snapshot = crawl_page_snapshot_with_session(&mut state.session, &target_url)?;
    let bridge_arc = crate::editor::browser::native_bridge::get_or_create_native_bridge(session_id);
    if let Ok(mut bridge) = bridge_arc.lock() {
        let _ = bridge.click(&format!("{}:{}", role, matched_name));
        let triples = bridge.capture_nda();
        let _ = crate::editor::browser::native_bridge::persist_native_nda_triples(
            workspace_root,
            session_id,
            &triples,
        );
    }
    build_session_action_report(
        workspace_root,
        sitemap_path,
        "click",
        &format!("{}:{}", role, matched_name),
        Some(target_actionability),
        &before,
        &state,
    )
}

pub fn session_click(
    workspace_root: &Path,
    session_id: &str,
    role: &str,
    name: &str,
    sitemap_path: &Path,
) -> Result<String, String> {
    match session_click_report(workspace_root, session_id, role, name, sitemap_path) {
        Ok(report) => Ok(render_session_action_report(&report)),
        Err(err) => {
            append_session_failure_transcript_entry(
                workspace_root,
                session_id,
                "click",
                Some(format!("{}:{}", role, name)),
                format!("Failed to click {}:{}: {}", role, name, err),
                session_file_path(workspace_root, session_id)
                    .display()
                    .to_string(),
            );
            Err(err)
        }
    }
}

pub fn session_fill_report(
    workspace_root: &Path,
    session_id: &str,
    field: &str,
    value: &str,
    sitemap_path: &Path,
) -> Result<BrowserSessionActionReport, String> {
    let mut state = load_session_replay_state(workspace_root, session_id, sitemap_path)?;
    let before = state.snapshot.clone();
    let target_actionability = if let Some(form_field) = find_form_field(&state.snapshot, field) {
        Some(describe_form_field_actionability(form_field))
    } else {
        find_textbox_element(&state.snapshot, field).map(describe_element_actionability)
    };
    apply_fill_field(&mut state, field, value)?;
    let bridge_arc = crate::editor::browser::native_bridge::get_or_create_native_bridge(session_id);
    if let Ok(mut bridge) = bridge_arc.lock() {
        let _ = bridge.fill(field, value);
        let triples = bridge.capture_nda();
        let _ = crate::editor::browser::native_bridge::persist_native_nda_triples(
            workspace_root,
            session_id,
            &triples,
        );
    }
    build_session_action_report(
        workspace_root,
        sitemap_path,
        "fill_field",
        field,
        target_actionability,
        &before,
        &state,
    )
}

pub fn session_fill(
    workspace_root: &Path,
    session_id: &str,
    field: &str,
    value: &str,
    sitemap_path: &Path,
) -> Result<String, String> {
    match session_fill_report(workspace_root, session_id, field, value, sitemap_path) {
        Ok(report) => Ok(render_session_action_report(&report)),
        Err(err) => {
            append_session_failure_transcript_entry(
                workspace_root,
                session_id,
                "fill_field",
                Some(field.to_string()),
                format!("Failed to fill field '{}': {}", field, err),
                session_file_path(workspace_root, session_id)
                    .display()
                    .to_string(),
            );
            Err(err)
        }
    }
}

pub fn session_submit_report(
    workspace_root: &Path,
    session_id: &str,
    form_id: Option<&str>,
    sitemap_path: &Path,
) -> Result<BrowserSessionActionReport, String> {
    let mut state = load_session_replay_state(workspace_root, session_id, sitemap_path)?;
    let before = state.snapshot.clone();
    let form = find_form(&state.snapshot, form_id).ok_or_else(|| match form_id {
        Some(id) => format!("session submit target form not found: '{}'", id),
        None => "session submit target form not found".to_string(),
    })?;
    let target = if form.id.is_empty() {
        form_id.unwrap_or("default").to_string()
    } else {
        form.id.clone()
    };
    let target_actionability = describe_form_actionability(form);
    submit_current_form(&mut state, form_id)?;
    build_session_action_report(
        workspace_root,
        sitemap_path,
        "submit_form",
        &target,
        Some(target_actionability),
        &before,
        &state,
    )
}

pub fn session_submit(
    workspace_root: &Path,
    session_id: &str,
    form_id: Option<&str>,
    sitemap_path: &Path,
) -> Result<String, String> {
    match session_submit_report(workspace_root, session_id, form_id, sitemap_path) {
        Ok(report) => Ok(render_session_action_report(&report)),
        Err(err) => {
            append_session_failure_transcript_entry(
                workspace_root,
                session_id,
                "submit_form",
                Some(form_id.unwrap_or("default").to_string()),
                format!(
                    "Failed to submit form '{}': {}",
                    form_id.unwrap_or("default"),
                    err
                ),
                session_file_path(workspace_root, session_id)
                    .display()
                    .to_string(),
            );
            Err(err)
        }
    }
}

pub fn navigate_session_report(
    workspace_root: &Path,
    session_id: &str,
    url: &str,
    sitemap_path: &Path,
) -> Result<BrowserSessionNavigationReport, String> {
    let mut session = load_session_state(workspace_root, session_id)
        .unwrap_or_else(|_| empty_browser_session_state(session_id));
    let snapshot = crawl_page_snapshot_with_session(&mut session, url)?;
    
    let bridge_arc = crate::editor::browser::native_bridge::get_or_create_native_bridge(session_id);
    if let Ok(mut bridge) = bridge_arc.lock() {
        if let Ok(triples) = bridge.navigate(url) {
            let _ = crate::editor::browser::native_bridge::persist_native_nda_triples(
                workspace_root,
                session_id,
                &triples,
            );
        }
    }
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
    let snapshot_path: PathBuf = write_snapshot_json(&snapshot, sitemap_path)?;
    let html_fallback_path: Option<PathBuf> = write_html_fallback(
        &snapshot.url,
        session.last_html.as_deref().unwrap_or_default(),
        sitemap_path,
    )?;
    let session_path: PathBuf = save_session_state(workspace_root, &session)?;

    let report = BrowserSessionNavigationReport {
        session_id: session.id,
        requested_url: url.to_string(),
        url: snapshot.url,
        title: snapshot.title,
        form_count: snapshot.forms.len(),
        cookie_count: snapshot.cookies.len(),
        request_count: snapshot.requests.len(),
        settle_signal_count: snapshot.settle_signals.len(),
        runtime_state_count: snapshot.runtime_state.len(),
        protocol_event_count: snapshot.protocol_events.len(),
        network_summary: summarize_network_activity(&snapshot.protocol_events),
        local_storage_count: session.local_storage.len(),
        session_storage_count: session.session_storage.len(),
        snapshot_json_path: snapshot_path.display().to_string(),
        session_json_path: session_path.display().to_string(),
        nda_facts_path: facts_path.display().to_string(),
        html_fallback_path: html_fallback_path.map(|path: PathBuf| path.display().to_string()),
    };
    append_session_transcript_entry(
        workspace_root,
        &report.session_id,
        BrowserSessionTranscriptEntry {
            sequence: 0,
            timestamp_ms: current_timestamp_ms(),
            event_kind: "navigate".to_string(),
            outcome: "ok".to_string(),
            summary: format!("Navigated to {}", report.url),
            session_id: report.session_id.clone(),
            url: Some(report.url.clone()),
            title: Some(report.title.clone()),
            target: Some(report.url.clone()),
            diff_summary: None,
            request_count: report.request_count,
            settle_signal_count: report.settle_signal_count,
            runtime_state_count: report.runtime_state_count,
            protocol_event_count: report.protocol_event_count,
            network_summary: report.network_summary.clone(),
            session_json_path: report.session_json_path.clone(),
            snapshot_json_path: Some(report.snapshot_json_path.clone()),
            checkpoint_json_path: None,
            nda_facts_path: Some(report.nda_facts_path.clone()),
            html_fallback_path: report.html_fallback_path.clone(),
        },
    )?;
    Ok(report)
}

pub fn navigate_session(
    workspace_root: &Path,
    session_id: &str,
    url: &str,
    sitemap_path: &Path,
) -> Result<String, String> {
    match navigate_session_report(workspace_root, session_id, url, sitemap_path) {
        Ok(report) => Ok(render_session_navigation_report(&report)),
        Err(err) => {
            append_session_failure_transcript_entry(
                workspace_root,
                session_id,
                "navigate",
                Some(url.to_string()),
                format!("Failed to navigate to '{}': {}", url, err),
                session_file_path(workspace_root, session_id)
                    .display()
                    .to_string(),
            );
            Err(err)
        }
    }
}

