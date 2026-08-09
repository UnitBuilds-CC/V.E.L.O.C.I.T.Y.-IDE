use super::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn render_session_navigation_report(report: &BrowserSessionNavigationReport) -> String {
    let network_summary = render_network_summary(&report.network_summary)
        .map(|value| format!("\nNetwork summary: {}", value))
        .unwrap_or_default();
    let html_fallback = render_html_fallback_line(report.html_fallback_path.as_deref());
    format!(
        "Session navigate complete.\nSession: {}\n{}\nTitle: {}\nForms: {}\nCookies: {}\nRequests: {}\nSettle signals: {}\nRuntime state: {}\nProtocol events: {}{}\nLocal storage: {}\nSession storage: {}\nSnapshot JSON: {}\nSession JSON: {}{}\nNDA Facts: {}",
        report.session_id,
        describe_url_resolution(&report.requested_url, &report.url),
        report.title,
        report.form_count,
        report.cookie_count,
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

pub fn render_runtime_capture_report(report: &BrowserRuntimeCaptureReport) -> String {
    let network_summary = render_network_summary(&report.network_summary)
        .map(|value| format!("\nNetwork summary: {}", value))
        .unwrap_or_default();
    let html_fallback = render_html_fallback_line(report.html_fallback_path.as_deref());
    let action_summary = report
        .action
        .as_ref()
        .map(|action| {
            let mut parts = vec![format!(
                "{} (wait {}ms)",
                action.action, action.wait_applied_ms
            )];
            if let Some(target) = &action.target {
                parts.push(format!("target={target}"));
            }
            if let Some(key) = &action.key {
                parts.push(format!("key={key}"));
            }
            if let Some(value) = &action.value {
                parts.push(format!("value={value}"));
            }
            if let Some(script) = &action.script {
                parts.push(format!("script={script}"));
            }
            if let Some(result) = &action.result {
                parts.push(format!("result={result}"));
            }
            format!("\nAction: {}", parts.join(" | "))
        })
        .unwrap_or_default();
    let frame_summary = if report.frame_count == 0 {
        String::new()
    } else {
        let accessible = report
            .frames
            .iter()
            .filter(|frame| frame.accessible)
            .count();
        let same_origin = report
            .frames
            .iter()
            .filter(|frame| frame.same_origin)
            .count();
        format!(
            "\nFrames: {} (accessible {}, same-origin {})",
            report.frame_count, accessible, same_origin
        )
    };
    let shadow_summary = if report.shadow_host_count == 0 {
        String::new()
    } else {
        let semantic_nodes = report
            .shadow_hosts
            .iter()
            .map(|host| host.semantic_node_count)
            .sum::<usize>();
        format!(
            "\nShadow hosts: {} (semantic nodes {})",
            report.shadow_host_count, semantic_nodes
        )
    };
    let canvas_summary = if report.canvas_count == 0 {
        String::new()
    } else {
        let runtime_evidence = report
            .canvases
            .iter()
            .filter(|canvas| canvas.runtime_evidence)
            .count();
        let animated = report
            .canvases
            .iter()
            .filter(|canvas| canvas.likely_animated)
            .count();
        format!(
            "\nCanvases: {} (webgl {}, runtime evidence {}, likely animated {})",
            report.canvas_count, report.webgl_canvas_count, runtime_evidence, animated
        )
    };
    let warnings = if report.warnings.is_empty() {
        String::new()
    } else {
        format!(
            "\nWarnings ({}): {}",
            report.warning_count,
            report.warnings.join(" | ")
        )
    };
    format!(
        "Runtime capture complete.\nSession: {}\nURL: {}\nTitle: {}\nBackend: {}\nForms: {}\nCookies: {}\nRequests: {}\nSettle signals: {}\nRuntime state: {}\nProtocol events: {}{}{}{}{}\nLocal storage: {}\nSession storage: {}\nAOM summary chars: {}{}{}\nSnapshot JSON: {}\nSession JSON: {}{}\nNDA Facts: {}",
        report.session_id,
        report.url,
        report.title,
        report.capture_backend,
        report.form_count,
        report.cookie_count,
        report.request_count,
        report.settle_signal_count,
        report.runtime_state_count,
        report.protocol_event_count,
        network_summary,
        frame_summary,
        shadow_summary,
        canvas_summary,
        report.local_storage_count,
        report.session_storage_count,
        report.aom_summary_chars,
        action_summary,
        warnings,
        report.snapshot_json_path,
        report.session_json_path,
        html_fallback,
        report.nda_facts_path,
    )
}

pub fn render_session_action_report(report: &BrowserSessionActionReport) -> String {
    let network_summary = render_network_summary(&report.network_summary)
        .map(|value| format!("\nNetwork summary: {}", value))
        .unwrap_or_default();
    let html_fallback = render_html_fallback_line(report.html_fallback_path.as_deref());
    let target_actionability = report
        .target_actionability
        .as_ref()
        .map(|target| {
            format!(
                "\nTarget actionability: {} (score {}) - {}",
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
        "Session action complete.\nAction: {}\nTarget: {}{}\nSession: {}\nURL: {}\nTitle: {}\nDiff: {}\nForms: {}\nCookies: {}\nRequests: {}\nSettle signals: {}\nRuntime state: {}\nProtocol events: {}{}\nLocal storage: {}\nSession storage: {}\nSnapshot JSON: {}\nSession JSON: {}{}\nNDA Facts: {}",
        report.action,
        report.target,
        target_actionability,
        report.session_id,
        report.url,
        report.title,
        report.diff_summary,
        report.form_count,
        report.cookie_count,
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

pub fn load_session_replay_state(
    workspace_root: &Path,
    session_id: &str,
    sitemap_path: &Path,
) -> Result<BrowserReplayState, String> {
    let mut session = load_session_state(workspace_root, session_id)?;
    let current_url = session
        .current_url
        .clone()
        .ok_or_else(|| format!("browser session '{}' has no current URL", session_id))?;
    let snapshot = load_snapshot_json(&current_url, sitemap_path)
        .or_else(|_| crawl_page_snapshot_with_session(&mut session, &current_url))?;
    Ok(BrowserReplayState {
        session,
        snapshot,
        filled_fields: HashMap::new(),
        variables: HashMap::new(),
        outputs: HashMap::new(),
    })
}

pub fn persist_runtime_capture_artifacts(
    workspace_root: &Path,
    session: &mut BrowserSessionState,
    captured: &RuntimeCaptureApiResponse,
    sitemap_path: &Path,
) -> Result<BrowserRuntimeCaptureReport, String> {
    session.current_url = Some(captured.final_url.clone());
    session.cookies = captured
        .cookies
        .iter()
        .map(runtime_cookie_as_browser_cookie)
        .collect();
    session.runtime_cookies = captured.cookies.clone();
    session.local_storage = captured.local_storage.clone();
    session.session_storage = captured.session_storage.clone();
    session.last_html = Some(captured.html.clone());

    let storage = vec![
        BrowserStorageBucket {
            scope: "local".to_string(),
            entries: session.local_storage.clone(),
        },
        BrowserStorageBucket {
            scope: "session".to_string(),
            entries: session.session_storage.clone(),
        },
    ];
    let requests = captured
        .requests
        .iter()
        .map(|request| BrowserRequestRecord {
            method: request.method.clone(),
            url: request.url.clone(),
            status_code: request.status_code,
            resource: request.resource.clone(),
        })
        .collect::<Vec<_>>();
    let runtime_state = captured
        .runtime_state
        .iter()
        .map(|entry| BrowserRuntimeState {
            scope: entry.scope.clone(),
            key: entry.key.clone(),
            value: entry.value.clone(),
        })
        .collect::<Vec<_>>();
    let protocol_events = captured
        .protocol_events
        .iter()
        .map(|event| BrowserProtocolEvent {
            kind: event.kind.clone(),
            phase: event.phase.clone(),
            target: event.target.clone(),
            detail: event.detail.clone(),
        })
        .collect::<Vec<_>>();
    let frames = captured
        .frames
        .iter()
        .map(|frame| BrowserFrameInventoryEntry {
            selector: frame.selector.clone(),
            name: frame.name.clone(),
            title: frame.title.clone(),
            source: frame.source.clone(),
            same_origin: frame.same_origin,
            accessible: frame.accessible,
            semantic_node_count: frame.semantic_node_count,
        })
        .collect::<Vec<_>>();
    let shadow_hosts = captured
        .shadow_hosts
        .iter()
        .map(|host| BrowserShadowHostInventoryEntry {
            selector: host.selector.clone(),
            tag: host.tag.clone(),
            role: host.role.clone(),
            mode: host.mode.clone(),
            semantic_node_count: host.semantic_node_count,
            text_sample: host.text_sample.clone(),
        })
        .collect::<Vec<_>>();

    let canvases = captured
        .canvases
        .iter()
        .map(|canvas| BrowserCanvasInventoryEntry {
            selector: canvas.selector.clone(),
            width: canvas.width,
            height: canvas.height,
            context_kinds: canvas.context_kinds.clone(),
            text_op_count: canvas.text_op_count,
            image_op_count: canvas.image_op_count,
            webgl_draw_count: canvas.webgl_draw_count,
            readback_count: canvas.readback_count,
            likely_animated: canvas.likely_animated,
            runtime_evidence: canvas.runtime_evidence,
            text_sample: canvas.text_sample.clone(),
        })
        .collect::<Vec<_>>();

    let mut snapshot = parse_html_to_snapshot_with_runtime_state(
        &captured.final_url,
        &captured.html,
        &session.cookies,
        &storage,
        &[],
        &requests,
        &captured.settle_signals,
        &runtime_state,
        &protocol_events,
    );
    if snapshot.title.trim().is_empty() || snapshot.title == "Untitled Page" {
        snapshot.title = captured.title.clone();
    }
    let summary_source = if !captured.page_text.trim().is_empty() {
        captured.page_text.trim()
    } else {
        captured.aom_summary.trim()
    };
    if !summary_source.is_empty() {
        snapshot.summary = truncate_string(summary_source, 1000);
    }
    for (name_key, val) in &captured.fields {
        let field_name = name_key.trim();
        if field_name.is_empty() {
            continue;
        }
        let already_present = snapshot
            .forms
            .iter()
            .flat_map(|form| form.fields.iter())
            .any(|field| {
                field.name.eq_ignore_ascii_case(field_name)
                    || field.label.eq_ignore_ascii_case(field_name)
            });
        if !already_present {
            snapshot.elements.push(AomElement {
                role: "textbox".to_string(),
                name: field_name.to_string(),
                value: val.clone(),
                target_url: None,
                supported_actions: vec!["fill".to_string()],
                provenance: "runtime-capture-derived".to_string(),
                actionability: role_actionability("textbox"),
            });
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
    let snapshot_path = write_snapshot_json(&snapshot, sitemap_path)?;
    let html_fallback_path = write_html_fallback(&snapshot.url, &captured.html, sitemap_path)?;
    let session_path = save_session_state(workspace_root, session)?;

    Ok(BrowserRuntimeCaptureReport {
        session_id: session.id.clone(),
        url: snapshot.url.clone(),
        title: snapshot.title.clone(),
        form_count: snapshot.forms.len(),
        cookie_count: session.cookies.len(),
        request_count: snapshot.requests.len(),
        settle_signal_count: snapshot.settle_signals.len(),
        runtime_state_count: snapshot.runtime_state.len(),
        protocol_event_count: snapshot.protocol_events.len(),
        frame_count: frames.len(),
        shadow_host_count: shadow_hosts.len(),
        canvas_count: canvases.len(),
        webgl_canvas_count: canvases
            .iter()
            .filter(|canvas| {
                canvas
                    .context_kinds
                    .iter()
                    .any(|kind: &String| kind.starts_with("webgl"))
            })
            .count(),
        frames,
        shadow_hosts,
        canvases,
        network_summary: summarize_network_activity(&snapshot.protocol_events),
        local_storage_count: session.local_storage.len(),
        session_storage_count: session.session_storage.len(),
        snapshot_json_path: snapshot_path.display().to_string(),
        session_json_path: session_path.display().to_string(),
        nda_facts_path: facts_path.display().to_string(),
        html_fallback_path: html_fallback_path.map(|path: PathBuf| path.display().to_string()),
        capture_backend: runtime_state
            .iter()
            .find(|entry| entry.scope == "runtime" && entry.key == "backend")
            .map(|entry| entry.value.clone())
            .unwrap_or_else(|| "go-runtime".to_string()),
        aom_summary_chars: captured.aom_summary.chars().count(),
        warning_count: captured.warnings.len(),
        warnings: captured.warnings.clone(),
        action: captured.action.clone(),
    })
}

pub fn browser_runtime_capture_report_internal(
    workspace_root: &Path,
    session_id: &str,
    url: &str,
    timeout_ms: u64,
    api_base: Option<&str>,
    sitemap_path: &Path,
) -> Result<BrowserRuntimeCaptureReport, String> {
    let mut session = load_session_state(workspace_root, session_id)
        .unwrap_or_else(|_| empty_browser_session_state(session_id));
    let endpoint = format!(
        "{}/api/runtime/capture",
        resolve_browser_runtime_api_base(api_base)
    );
    let request_body = serde_json::to_string(&RuntimeCaptureApiRequest { url, timeout_ms })
        .map_err(|err| format!("serialise runtime capture request: {err}"))?;
    let response = ureq::post(&endpoint)
        .set("Content-Type", "application/json")
        .send_string(&request_body)
        .map_err(|err| format!("runtime capture request failed: {err}"))?;
    if response.status() >= 400 {
        return Err(format!(
            "runtime capture request failed with status {}",
            response.status()
        ));
    }
    let raw = response
        .into_string()
        .map_err(|err| format!("read runtime capture response: {err}"))?;
    let captured: RuntimeCaptureApiResponse = serde_json::from_str(&raw)
        .map_err(|err| format!("parse runtime capture response: {err}"))?;

    let report =
        persist_runtime_capture_artifacts(workspace_root, &mut session, &captured, sitemap_path)?;
    append_session_transcript_entry(
        workspace_root,
        &report.session_id,
        BrowserSessionTranscriptEntry {
            sequence: 0,
            timestamp_ms: current_timestamp_ms(),
            event_kind: "runtime_capture".to_string(),
            outcome: "ok".to_string(),
            summary: format!("Runtime captured {}", report.url),
            session_id: report.session_id.clone(),
            url: Some(report.url.clone()),
            title: Some(report.title.clone()),
            target: Some(url.to_string()),
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
