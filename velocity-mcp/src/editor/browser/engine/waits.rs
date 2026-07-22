use super::*;
use std::path::{Path, PathBuf};

pub fn render_session_wait_report(report: &BrowserSessionWaitReport) -> String {
    let network_summary = render_network_summary(&report.network_summary)
        .map(|value| format!("\nNetwork summary: {}", value))
        .unwrap_or_default();
    let html_fallback = render_html_fallback_line(report.html_fallback_path.as_deref());
    let matched_target_actionability = report
        .matched_target_actionability
        .as_ref()
        .map(|target| {
            format!(
                "\nMatched target actionability: {} (score {}) - {}",
                if target.actionable {
                    "actionable"
                } else {
                    "not actionable"
                },
                target.score,
                target.reason
            )
        })
        .unwrap_or_default();
    format!(
        "Session wait complete.\nSession: {}\n{}\nTitle: {}\nDiff: {}{}\nRequests: {}\nSettle signals: {}\nRuntime state: {}\nProtocol events: {}{}\nLocal storage: {}\nSession storage: {}\nSnapshot JSON: {}\nSession JSON: {}{}\nNDA Facts: {}",
        report.session_id,
        describe_url_resolution(&report.requested_url, &report.url),
        report.title,
        report.diff_summary,
        matched_target_actionability,
        report.request_count,
        report.settle_signal_count,
        report.runtime_state_count,
        report.protocol_event_count,
        network_summary,
        report.local_storage_count,
        report.session_storage_count,
        report.snapshot_json_path,
        report.session_json_path,
        html_fallback,
        report.nda_facts_path,
    )
}

pub fn wait_for_session_report(
    workspace_root: &Path,
    session_id: &str,
    text: Option<&str>,
    title: Option<&str>,
    url_contains: Option<&str>,
    mutation: Option<&str>,
    request_method: Option<&str>,
    request_url_contains: Option<&str>,
    request_status: Option<u16>,
    request_resource: Option<&str>,
    storage_scope: Option<&str>,
    storage_key: Option<&str>,
    storage_value: Option<&str>,
    settle: Option<&str>,
    settle_scope: Option<&str>,
    settle_state: Option<&str>,
    runtime_scope: Option<&str>,
    runtime_key: Option<&str>,
    runtime_value: Option<&str>,
    protocol_kind: Option<&str>,
    protocol_phase: Option<&str>,
    protocol_target: Option<&str>,
    protocol_detail: Option<&str>,
    network_idle: bool,
    app_ready: bool,
    mutation_settled: bool,
    stream_complete: bool,
    role: Option<&str>,
    name: Option<&str>,
    require_actionable: bool,
    stable_polls: Option<u32>,
    timeout_ms: Option<u64>,
    interval_ms: Option<u64>,
    sitemap_path: &Path,
) -> Result<BrowserSessionWaitReport, String> {
    let mut session = load_session_state(workspace_root, session_id)?;
    let current_url = session
        .current_url
        .clone()
        .ok_or_else(|| format!("browser session '{}' has no current URL", session_id))?;
    let mut snapshot = load_snapshot_json(&current_url, sitemap_path).unwrap_or_else(|_| {
        crawl_page_snapshot_with_session(&mut session, &current_url).unwrap_or(
            BrowserPageSnapshot {
                url: current_url.clone(),
                title: "Untitled Page".to_string(),
                summary: String::new(),
                elements: Vec::new(),
                forms: Vec::new(),
                cookies: session.cookies.clone(),
                storage: storage_buckets(&session),
                mutations: Vec::new(),
                requests: Vec::new(),
                settle_signals: Vec::new(),
                runtime_state: Vec::new(),
                protocol_events: Vec::new(),
            },
        )
    });
    if session.last_html.is_none() {
        session.last_html = browser_html_fallback_path(&snapshot.url, sitemap_path)
            .exists()
            .then(|| load_html_fallback(&snapshot.url, sitemap_path).ok())
            .flatten();
    }
    let diff = if let Some(wait_text) = text {
        wait_for_condition(
            &mut session,
            &mut snapshot,
            timeout_ms,
            interval_ms,
            |candidate| snapshot_contains_text(candidate, wait_text),
        )?
    } else if let Some(wait_title) = title {
        wait_for_condition(
            &mut session,
            &mut snapshot,
            timeout_ms,
            interval_ms,
            |candidate| {
                candidate
                    .title
                    .to_ascii_lowercase()
                    .contains(&wait_title.to_ascii_lowercase())
            },
        )?
    } else if let Some(wait_fragment) = url_contains {
        wait_for_condition(
            &mut session,
            &mut snapshot,
            timeout_ms,
            interval_ms,
            |candidate| candidate.url.contains(wait_fragment),
        )?
    } else if let Some(wait_mutation) = mutation {
        let lowered = wait_mutation.to_ascii_lowercase();
        wait_for_condition(
            &mut session,
            &mut snapshot,
            timeout_ms,
            interval_ms,
            |candidate| {
                candidate
                    .mutations
                    .iter()
                    .any(|entry: &String| entry.to_ascii_lowercase().contains(lowered.as_str()))
            },
        )?
    } else if request_method.is_some()
        || request_url_contains.is_some()
        || request_status.is_some()
        || request_resource.is_some()
    {
        wait_for_condition(
            &mut session,
            &mut snapshot,
            timeout_ms,
            interval_ms,
            |candidate| {
                candidate.requests.iter().any(|entry| {
                    request_record_matches(
                        entry,
                        request_method,
                        request_url_contains,
                        request_status,
                        request_resource,
                    )
                })
            },
        )?
    } else if let (Some(wait_scope), Some(wait_key)) = (storage_scope, storage_key) {
        wait_for_condition(
            &mut session,
            &mut snapshot,
            timeout_ms,
            interval_ms,
            |candidate| storage_entry_matches(candidate, wait_scope, wait_key, storage_value),
        )?
    } else if settle.is_some() || settle_scope.is_some() {
        wait_for_condition(
            &mut session,
            &mut snapshot,
            timeout_ms,
            interval_ms,
            |candidate| {
                candidate
                    .settle_signals
                    .iter()
                    .any(|entry| settle_signal_matches(entry, settle, settle_scope, settle_state))
            },
        )?
    } else if let (Some(wait_scope), Some(wait_key)) = (runtime_scope, runtime_key) {
        let lowered_scope = wait_scope.to_ascii_lowercase();
        let lowered_key = wait_key.to_ascii_lowercase();
        let lowered_value = runtime_value.map(|value| value.to_ascii_lowercase());
        wait_for_condition(
            &mut session,
            &mut snapshot,
            timeout_ms,
            interval_ms,
            |candidate| {
                candidate.runtime_state.iter().any(|entry| {
                    entry.scope.eq_ignore_ascii_case(&lowered_scope)
                        && entry.key.eq_ignore_ascii_case(&lowered_key)
                        && lowered_value
                            .as_ref()
                            .map(|value| entry.value.to_ascii_lowercase().contains(value))
                            .unwrap_or(true)
                })
            },
        )?
    } else if protocol_kind.is_some()
        || protocol_phase.is_some()
        || protocol_target.is_some()
        || protocol_detail.is_some()
    {
        wait_for_condition(
            &mut session,
            &mut snapshot,
            timeout_ms,
            interval_ms,
            |candidate| {
                candidate.protocol_events.iter().any(|entry| {
                    protocol_event_matches(
                        entry,
                        protocol_kind,
                        protocol_phase,
                        protocol_target,
                        protocol_detail,
                    )
                })
            },
        )?
    } else if network_idle {
        wait_for_condition(
            &mut session,
            &mut snapshot,
            timeout_ms,
            interval_ms,
            |candidate| {
                candidate.settle_signals.iter().any(|entry| {
                    settle_signal_matches(entry, None, Some("network"), Some("settled"))
                })
            },
        )?
    } else if app_ready {
        wait_for_condition(
            &mut session,
            &mut snapshot,
            timeout_ms,
            interval_ms,
            |candidate| {
                candidate.settle_signals.iter().any(|entry| {
                    settle_signal_matches(entry, None, Some("navigation"), Some("settled"))
                }) && candidate.runtime_state.iter().any(|entry| {
                    (entry.scope.eq_ignore_ascii_case("router")
                        && entry.key.eq_ignore_ascii_case("name")
                        && !entry.value.trim().is_empty())
                        || (entry.scope.eq_ignore_ascii_case("store")
                            && contains_case_insensitive(&entry.value, "ready"))
                        || (entry.scope.eq_ignore_ascii_case("app")
                            && contains_case_insensitive(&entry.value, "ready"))
                })
            },
        )?
    } else if mutation_settled {
        wait_for_condition(
            &mut session,
            &mut snapshot,
            timeout_ms,
            interval_ms,
            |candidate| {
                candidate
                    .settle_signals
                    .iter()
                    .any(|entry| settle_signal_matches(entry, Some("settled"), None, None))
                    && candidate.mutations.iter().any(|entry| {
                        contains_case_insensitive(entry, "hydration")
                            || contains_case_insensitive(entry, "settled")
                            || contains_case_insensitive(entry, "complete")
                    })
            },
        )?
    } else if stream_complete {
        wait_for_condition(
            &mut session,
            &mut snapshot,
            timeout_ms,
            interval_ms,
            |candidate| {
                candidate.protocol_events.iter().any(|entry| {
                    (entry.kind.eq_ignore_ascii_case("stream")
                        || entry.kind.eq_ignore_ascii_case("event_stream")
                        || entry.kind.eq_ignore_ascii_case("websocket"))
                        && (entry.phase.eq_ignore_ascii_case("complete")
                            || entry.phase.eq_ignore_ascii_case("closed")
                            || contains_case_insensitive(&entry.detail, "complete")
                            || contains_case_insensitive(&entry.detail, "closed"))
                })
            },
        )?
    } else if let (Some(wait_role), Some(wait_name)) = (role, name) {
        wait_for_condition(
            &mut session,
            &mut snapshot,
            timeout_ms,
            interval_ms,
            |candidate| {
                find_element(candidate, wait_role, wait_name)
                    .map(|element| {
                        !require_actionable || describe_element_actionability(element).actionable
                    })
                    .unwrap_or(false)
            },
        )?
    } else if stable_polls.is_some() {
        wait_for_stable_snapshot(
            &mut session,
            &mut snapshot,
            stable_polls,
            timeout_ms,
            interval_ms,
        )?
    } else {
        return Err("browser_session_wait requires text, title, urlContains, mutation, requestMethod/requestUrlContains/requestStatus/requestResource, storageScope+storageKey, settle/settleScope, runtimeScope+runtimeKey, protocolKind/protocolPhase/protocolTarget/protocolDetail, networkIdle, appReady, mutationSettled, streamComplete, stablePolls, or both role and name".to_string());
    };

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
    let html_fallback_path = write_html_fallback(
        &snapshot.url,
        session.last_html.as_deref().unwrap_or_default(),
        sitemap_path,
    )?;
    let session_path = save_session_state(workspace_root, &session)?;

    let matched_target_actionability = if let (Some(wait_role), Some(wait_name)) = (role, name) {
        find_element(&snapshot, wait_role, wait_name).map(describe_element_actionability)
    } else {
        None
    };
    let report = BrowserSessionWaitReport {
        session_id: session.id,
        requested_url: current_url,
        url: snapshot.url,
        title: snapshot.title,
        diff_summary: render_snapshot_diff(&diff),
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
        matched_target_actionability,
    };
    append_session_transcript_entry(
        workspace_root,
        &report.session_id,
        BrowserSessionTranscriptEntry {
            sequence: 0,
            timestamp_ms: current_timestamp_ms(),
            event_kind: "wait".to_string(),
            outcome: "ok".to_string(),
            summary: format!("Wait satisfied on {}", report.url),
            session_id: report.session_id.clone(),
            url: Some(report.url.clone()),
            title: Some(report.title.clone()),
            target: None,
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

pub fn wait_for_session(
    workspace_root: &Path,
    session_id: &str,
    text: Option<&str>,
    title: Option<&str>,
    url_contains: Option<&str>,
    mutation: Option<&str>,
    request_method: Option<&str>,
    request_url_contains: Option<&str>,
    request_status: Option<u16>,
    request_resource: Option<&str>,
    storage_scope: Option<&str>,
    storage_key: Option<&str>,
    storage_value: Option<&str>,
    settle: Option<&str>,
    settle_scope: Option<&str>,
    settle_state: Option<&str>,
    runtime_scope: Option<&str>,
    runtime_key: Option<&str>,
    runtime_value: Option<&str>,
    protocol_kind: Option<&str>,
    protocol_phase: Option<&str>,
    protocol_target: Option<&str>,
    protocol_detail: Option<&str>,
    network_idle: bool,
    app_ready: bool,
    mutation_settled: bool,
    stream_complete: bool,
    role: Option<&str>,
    name: Option<&str>,
    require_actionable: bool,
    stable_polls: Option<u32>,
    timeout_ms: Option<u64>,
    interval_ms: Option<u64>,
    sitemap_path: &Path,
) -> Result<String, String> {
    match wait_for_session_report(
        workspace_root,
        session_id,
        text,
        title,
        url_contains,
        mutation,
        request_method,
        request_url_contains,
        request_status,
        request_resource,
        storage_scope,
        storage_key,
        storage_value,
        settle,
        settle_scope,
        settle_state,
        runtime_scope,
        runtime_key,
        runtime_value,
        protocol_kind,
        protocol_phase,
        protocol_target,
        protocol_detail,
        network_idle,
        app_ready,
        mutation_settled,
        stream_complete,
        role,
        name,
        require_actionable,
        stable_polls,
        timeout_ms,
        interval_ms,
        sitemap_path,
    ) {
        Ok(report) => Ok(render_session_wait_report(&report)),
        Err(err) => {
            append_session_failure_transcript_entry(
                workspace_root,
                session_id,
                "wait",
                None,
                format!("Failed to satisfy wait condition: {}", err),
                session_file_path(workspace_root, session_id)
                    .display()
                    .to_string(),
            );
            Err(err)
        }
    }
}

#[allow(dead_code)]
fn extract_snapshot_value(
    snapshot: &BrowserPageSnapshot,
    source: &str,
    role: Option<&str>,
    name: Option<&str>,
    field: Option<&str>,
) -> Result<String, String> {
    if source.eq_ignore_ascii_case("title") {
        return Ok(snapshot.title.clone());
    }
    if source.eq_ignore_ascii_case("summary") {
        return Ok(snapshot.summary.clone());
    }
    if source.eq_ignore_ascii_case("field_value") {
        let field_name = field
            .ok_or_else(|| "extract_text field is required for source=field_value".to_string())?;
        let matched = find_form_field(snapshot, field_name)
            .ok_or_else(|| format!("workflow extract field not found: '{}'", field_name))?;
        return Ok(matched.value.clone());
    }

    let role =
        role.ok_or_else(|| format!("extract_text role is required for source='{}'", source))?;
    let name =
        name.ok_or_else(|| format!("extract_text name is required for source='{}'", source))?;
    let matched = find_element(snapshot, role, name).ok_or_else(|| {
        format!(
            "workflow extract target not found: role='{}' name='{}'",
            role, name
        )
    })?;
    if source.eq_ignore_ascii_case("element_name") {
        Ok(matched.name.clone())
    } else if source.eq_ignore_ascii_case("element_value") {
        Ok(matched.value.clone())
    } else if source.eq_ignore_ascii_case("element_url") {
        Ok(matched.target_url.clone().unwrap_or_default())
    } else {
        Err(format!("unsupported workflow extract source: '{}'", source))
    }
}

pub fn apply_fill_field(
    state: &mut BrowserReplayState,
    field_name: &str,
    value: &str,
) -> Result<(), String> {
    let best_field = find_form_field(&state.snapshot, field_name);
    if let Some(field) = best_field {
        let field_actionability = describe_form_field_actionability(field);
        if !field_actionability.actionable {
            return Err(format!(
                "workflow fill target '{}' is not actionable: {}",
                field_name, field_actionability.reason
            ));
        }
    }
    let best_field_name = best_field.map(|field| field.name.clone());
    let best_element_name =
        find_textbox_element(&state.snapshot, field_name).map(|element| element.name.clone());

    let mut matched = false;
    for form in &mut state.snapshot.forms {
        for field in &mut form.fields {
            if best_field_name.as_deref() == Some(field.name.as_str()) {
                field.value = value.to_string();
                state
                    .filled_fields
                    .insert(field.name.clone(), value.to_string());
                matched = true;
            }
        }
    }
    for element in &mut state.snapshot.elements {
        if element.role.eq_ignore_ascii_case("textbox")
            && best_element_name.as_deref() == Some(element.name.as_str())
        {
            element.value = value.to_string();
            matched = true;
        }
    }
    if matched {
        Ok(())
    } else {
        Err(format!("workflow fill target not found: '{}'", field_name))
    }
}

pub fn submit_current_form(
    state: &mut BrowserReplayState,
    form_id: Option<&str>,
) -> Result<(), String> {
    let form = find_form(&state.snapshot, form_id)
        .cloned()
        .ok_or_else(|| match form_id {
            Some(id) => format!("workflow submit target form not found: '{}'", id),
            None => "workflow submit target form not found".to_string(),
        })?;

    let mut encoded_pairs = Vec::new();
    for field in &form.fields {
        let value = state
            .filled_fields
            .get(&field.name)
            .cloned()
            .unwrap_or_else(|| field.value.clone());
        encoded_pairs.push(format!(
            "{}={}",
            url_encode(&field.name),
            url_encode(&value)
        ));
    }
    let payload = encoded_pairs.join("&");
    let target_url = if form.method.eq_ignore_ascii_case("GET") && !payload.is_empty() {
        if form.action.contains('?') {
            format!("{}&{}", form.action, payload)
        } else {
            format!("{}?{}", form.action, payload)
        }
    } else {
        form.action.clone()
    };

    let response = if form.method.eq_ignore_ascii_case("POST") {
        fetch_with_session(
            &target_url,
            "POST",
            Some(&payload),
            &state.session.cookies,
            &state.session.network,
        )?
    } else {
        fetch_with_session(
            &target_url,
            "GET",
            None,
            &state.session.cookies,
            &state.session.network,
        )?
    };

    for cookie in response.cookies.iter().cloned() {
        merge_cookie(&mut state.session.cookies, cookie);
    }
    apply_storage_updates(
        &mut state.session.local_storage,
        &response.local_storage_updates,
    );
    apply_storage_updates(
        &mut state.session.session_storage,
        &response.session_storage_updates,
    );
    state.session.current_url = Some(response.final_url.clone());
    state.session.last_html = Some(response.html.clone());
    let storage = storage_buckets(&state.session);
    state.snapshot = parse_html_to_snapshot_with_runtime_state(
        &response.final_url,
        &response.html,
        &state.session.cookies,
        &storage,
        &response.mutations,
        &response.requests,
        &response.settle_signals,
        &response.runtime_state,
        &response.protocol_events,
    );
    state.filled_fields.clear();
    Ok(())
}

