use super::*;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub fn runtime_capture_report(
    workspace_root: &Path,
    session_id: &str,
    url: &str,
    timeout_ms: u64,
    api_base: Option<&str>,
    sitemap_path: &Path,
) -> Result<BrowserRuntimeCaptureReport, String> {
    browser_runtime_capture_report_internal(
        workspace_root,
        session_id,
        url,
        timeout_ms,
        api_base,
        sitemap_path,
    )
}

pub fn runtime_capture(
    workspace_root: &Path,
    session_id: &str,
    url: &str,
    timeout_ms: u64,
    api_base: Option<&str>,
    sitemap_path: &Path,
) -> Result<String, String> {
    match browser_runtime_capture_report_internal(
        workspace_root,
        session_id,
        url,
        timeout_ms,
        api_base,
        sitemap_path,
    ) {
        Ok(report) => Ok(render_runtime_capture_report(&report)),
        Err(err) => {
            append_session_failure_transcript_entry(
                workspace_root,
                session_id,
                "runtime_capture",
                Some(url.to_string()),
                format!("Failed runtime capture for '{}': {}", url, err),
                session_file_path(workspace_root, session_id)
                    .display()
                    .to_string(),
            );
            Err(err)
        }
    }
}

fn runtime_action_target(node_id: Option<&str>, selector: Option<&str>) -> Result<String, String> {
    node_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .or_else(|| {
            selector
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string())
        })
        .ok_or_else(|| "either nodeId or selector is required".to_string())
}

fn sync_runtime_capture_session(
    workspace_root: &Path,
    runtime_session: &mut RuntimeBrowserSessionState,
    captured: &RuntimeCaptureApiResponse,
    target: Option<String>,
    event_kind: &str,
    sitemap_path: &Path,
) -> Result<BrowserRuntimeCaptureReport, String> {
    let mut session = load_session_state(workspace_root, &runtime_session.id)
        .unwrap_or_else(|_| empty_browser_session_state(&runtime_session.id));
    let report =
        persist_runtime_capture_artifacts(workspace_root, &mut session, captured, sitemap_path)?;
    runtime_session.current_url = Some(report.url.clone());
    runtime_session.last_title = Some(report.title.clone());
    runtime_session.cookies = captured.cookies.clone();
    runtime_session.local_storage = session.local_storage.clone();
    runtime_session.session_storage = session.session_storage.clone();
    save_runtime_session_state(workspace_root, runtime_session)?;
    append_session_transcript_entry(
        workspace_root,
        &report.session_id,
        BrowserSessionTranscriptEntry {
            sequence: 0,
            timestamp_ms: current_timestamp_ms(),
            event_kind: event_kind.to_string(),
            outcome: "ok".to_string(),
            summary: format!("{} -> {}", event_kind, report.url),
            session_id: report.session_id.clone(),
            url: Some(report.url.clone()),
            title: Some(report.title.clone()),
            target,
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

pub fn create_runtime_session(
    workspace_root: &Path,
    session_id: &str,
    start_url: Option<&str>,
    wait_timeout_ms: Option<u64>,
    api_base: Option<&str>,
    compact: bool,
) -> Result<String, String> {
    let api_base_resolved = resolve_browser_runtime_api_base(api_base);
    let body = serde_json::json!({
        "startUrl": start_url,
        "waitTimeoutMs": wait_timeout_ms.unwrap_or(2_000),
    });
    let value = runtime_api_request(
        "POST",
        &format!("{}/api/runtime/session", api_base_resolved),
        Some(&body),
    )?;
    let runtime_session_id = value
        .get("sessionId")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "runtime create session response missing sessionId".to_string())?
        .to_string();
    let warnings = parse_runtime_string_list(value.get("warnings"));
    let session = RuntimeBrowserSessionState {
        id: session_id.to_string(),
        runtime_session_id,
        api_base: api_base_resolved,
        current_url: start_url.map(|value| value.to_string()),
        last_title: None,
        cookies: Vec::new(),
        local_storage: HashMap::new(),
        session_storage: HashMap::new(),
    };
    let session_path = save_runtime_session_state(workspace_root, &session)?;
    if compact {
        let report = RuntimeBrowserSessionReadReport {
            session,
            session_json_path: session_path.display().to_string(),
            warning_count: warnings.len(),
            warnings,
        };
        serde_json::to_string_pretty(&report)
            .map_err(|err| format!("serialise runtime session create summary: {err}"))
    } else {
        let mut lines = vec![
            format!("Created runtime browser session '{}'", session_id),
            format!("Runtime session id: {}", session.runtime_session_id),
            format!("API base: {}", session.api_base),
            format!("Session JSON: {}", session_path.display()),
        ];
        if let Some(url) = session.current_url.as_deref() {
            lines.push(format!("Start URL: {}", url));
        }
        if !warnings.is_empty() {
            lines.push(format!(
                "Warnings ({}): {}",
                warnings.len(),
                warnings.join(" | ")
            ));
        }
        Ok(lines.join("\n"))
    }
}

pub fn get_runtime_session(
    workspace_root: &Path,
    session_id: &str,
    compact: bool,
) -> Result<String, String> {
    let session = load_runtime_session_state(workspace_root, session_id)?;
    if compact {
        let report = RuntimeBrowserSessionReadReport {
            session,
            session_json_path: runtime_session_file_path(workspace_root, session_id)
                .display()
                .to_string(),
            warning_count: 0,
            warnings: Vec::new(),
        };
        serde_json::to_string_pretty(&report)
            .map_err(|err| format!("serialise runtime session summary: {err}"))
    } else {
        runtime_session_state_to_json(&session)
    }
}

pub fn close_runtime_session(
    workspace_root: &Path,
    session_id: &str,
    compact: bool,
) -> Result<String, String> {
    let session = load_runtime_session_state(workspace_root, session_id)?;
    runtime_api_request(
        "DELETE",
        &format!(
            "{}/api/runtime/session/{}",
            session.api_base, session.runtime_session_id
        ),
        None,
    )?;
    let path = runtime_session_file_path(workspace_root, session_id);
    if path.exists() {
        fs::remove_file(&path).map_err(|err| format!("remove runtime browser session: {err}"))?;
    }
    if compact {
        let report = RuntimeBrowserSessionCloseReport {
            session_id: session.id,
            runtime_session_id: session.runtime_session_id,
            removed_session_json_path: path.display().to_string(),
        };
        serde_json::to_string_pretty(&report)
            .map_err(|err| format!("serialise runtime session close summary: {err}"))
    } else {
        Ok(format!(
            "Closed runtime browser session '{}'\nRuntime session id: {}\nRemoved session JSON: {}",
            session.id,
            session.runtime_session_id,
            path.display()
        ))
    }
}

pub fn capture_runtime_session(
    workspace_root: &Path,
    session_id: &str,
    sitemap_path: &Path,
    compact: bool,
) -> Result<String, String> {
    let mut session = load_runtime_session_state(workspace_root, session_id)?;
    let value = runtime_api_request(
        "POST",
        &format!(
            "{}/api/runtime/session/{}/capture",
            session.api_base, session.runtime_session_id
        ),
        Some(&serde_json::json!({})),
    )?;
    let captured = parse_runtime_session_capture_response(value)?;
    let target = session.current_url.clone();
    let report = sync_runtime_capture_session(
        workspace_root,
        &mut session,
        &captured,
        target,
        "runtime_session_capture",
        sitemap_path,
    )?;
    if compact {
        serde_json::to_string_pretty(&report)
            .map_err(|err| format!("serialise runtime session capture summary: {err}"))
    } else {
        Ok(render_runtime_capture_report(&report))
    }
}

fn runtime_session_action(
    workspace_root: &Path,
    session_id: &str,
    action: &str,
    node_id: Option<&str>,
    selector: Option<&str>,
    value: Option<&str>,
    key: Option<&str>,
    url: Option<&str>,
    script: Option<&str>,
    natural: bool,
    clear: bool,
    wait_timeout_ms: Option<u64>,
    sitemap_path: &Path,
    compact: bool,
) -> Result<String, String> {
    let mut session = load_runtime_session_state(workspace_root, session_id)?;
    let body = serde_json::json!({
        "action": action,
        "nodeId": node_id,
        "selector": selector,
        "value": value,
        "key": key,
        "url": url,
        "script": script,
        "natural": natural,
        "clear": clear,
        "waitTimeoutMs": wait_timeout_ms.unwrap_or(1_500),
    });
    let value = runtime_api_request(
        "POST",
        &format!(
            "{}/api/runtime/session/{}/action",
            session.api_base, session.runtime_session_id
        ),
        Some(&body),
    )?;
    let captured = parse_runtime_session_capture_response(value)?;
    let target = match action {
        "navigate" => url.map(|value| value.to_string()),
        "press_key" => key.map(|value| value.to_string()),
        "evaluate" => script.map(|value| value.to_string()),
        _ => Some(runtime_action_target(node_id, selector)?),
    };
    let report = sync_runtime_capture_session(
        workspace_root,
        &mut session,
        &captured,
        target,
        action,
        sitemap_path,
    )?;
    if compact {
        serde_json::to_string_pretty(&report)
            .map_err(|err| format!("serialise runtime session action summary: {err}"))
    } else {
        Ok(render_runtime_capture_report(&report))
    }
}

pub fn runtime_navigate_session(
    workspace_root: &Path,
    session_id: &str,
    url: &str,
    wait_timeout_ms: Option<u64>,
    sitemap_path: &Path,
    compact: bool,
) -> Result<String, String> {
    runtime_session_action(
        workspace_root,
        session_id,
        "navigate",
        None,
        None,
        None,
        None,
        Some(url),
        None,
        false,
        false,
        wait_timeout_ms,
        sitemap_path,
        compact,
    )
}

pub fn runtime_click_session(
    workspace_root: &Path,
    session_id: &str,
    node_id: Option<&str>,
    selector: Option<&str>,
    wait_timeout_ms: Option<u64>,
    sitemap_path: &Path,
    compact: bool,
) -> Result<String, String> {
    runtime_session_action(
        workspace_root,
        session_id,
        "click",
        node_id,
        selector,
        None,
        None,
        None,
        None,
        false,
        false,
        wait_timeout_ms,
        sitemap_path,
        compact,
    )
}

pub fn runtime_evaluate_session(
    workspace_root: &Path,
    session_id: &str,
    script: &str,
    wait_timeout_ms: Option<u64>,
    sitemap_path: &Path,
    compact: bool,
) -> Result<String, String> {
    runtime_session_action(
        workspace_root,
        session_id,
        "evaluate",
        None,
        None,
        None,
        None,
        None,
        Some(script),
        false,
        false,
        wait_timeout_ms,
        sitemap_path,
        compact,
    )
}

pub fn runtime_js_click_session(
    workspace_root: &Path,
    session_id: &str,
    node_id: &str,
    wait_timeout_ms: Option<u64>,
    sitemap_path: &Path,
    compact: bool,
) -> Result<String, String> {
    runtime_session_action(
        workspace_root,
        session_id,
        "js_click",
        Some(node_id),
        None,
        None,
        None,
        None,
        None,
        false,
        false,
        wait_timeout_ms,
        sitemap_path,
        compact,
    )
}

pub fn render_trace_summary(report: &TraceSummaryReport) -> String {
    format!(
        "Browser Trace Summary:\nTotal Entries: {}\nConsole Messages: {}\nNetwork Activity: {}\nDOM Mutations: {}\nScreenshots: {}\nWarnings: {}\nHealth Impact: {}\nLatest Screenshot: {}",
        report.total_entries,
        report.console_count,
        report.network_count,
        report.mutation_count,
        report.screenshot_count,
        report.warning_count,
        report.health_impact.as_deref().unwrap_or("healthy"),
        report.latest_screenshot.as_deref().unwrap_or("none")
    )
}

pub fn get_trace_summary(workspace_root: &Path, compact: bool) -> Result<String, String> {
    let trace_path = workspace_root
        .join(".velocity")
        .join("browser_artifacts")
        .join("trace_summary.json");

    let fallback_path = workspace_root
        .join("data")
        .join("artifacts")
        .join("trace_summary.json");

    let final_path = if trace_path.exists() {
        trace_path
    } else if fallback_path.exists() {
        fallback_path
    } else {
        return Ok(if compact {
            serde_json::to_string_pretty(&TraceSummaryReport {
                total_entries: 0,
                console_count: 0,
                network_count: 0,
                mutation_count: 0,
                screenshot_count: 0,
                warning_count: 0,
                recent_entries: Vec::new(),
                latest_screenshot: None,
                health_impact: Some("healthy".to_string()),
            })
            .unwrap_or_default()
        } else {
            "No active browser traces captured yet.\nHealth impact: healthy\nTotal entries: 0"
                .to_string()
        });
    };

    let contents = fs::read_to_string(&final_path)
        .map_err(|err| format!("read trace summary from {}: {}", final_path.display(), err))?;

    if compact {
        Ok(contents)
    } else {
        let report: TraceSummaryReport = serde_json::from_str(&contents)
            .map_err(|err| format!("parse trace summary json: {}", err))?;
        Ok(render_trace_summary(&report))
    }
}

pub fn get_trace_logs(workspace_root: &Path, compact: bool) -> Result<String, String> {
    let log_path = workspace_root
        .join(".velocity")
        .join("browser_artifacts")
        .join("trace_log.json");

    let fallback_path = workspace_root
        .join("data")
        .join("artifacts")
        .join("trace_log.json");

    let final_path = if log_path.exists() {
        log_path
    } else if fallback_path.exists() {
        fallback_path
    } else {
        return Ok(if compact {
            "[]".to_string()
        } else {
            "No trace entries recorded.".to_string()
        });
    };

    let contents = fs::read_to_string(&final_path)
        .map_err(|err| format!("read trace log from {}: {}", final_path.display(), err))?;

    if compact {
        Ok(contents)
    } else {
        let entries: Vec<TraceEntry> = serde_json::from_str(&contents)
            .map_err(|err| format!("parse trace log json: {}", err))?;
        let mut out = format!("Trace Entries ({})\n", entries.len());
        for entry in entries {
            out.push_str(&format!(
                "[{}] [{}] {} - {}\n",
                entry.timestamp,
                entry.level.as_deref().unwrap_or("info"),
                entry.entry_type,
                entry.message
            ));
        }
        Ok(out)
    }
}

pub fn runtime_fill_session(
    workspace_root: &Path,
    session_id: &str,
    node_id: Option<&str>,
    selector: Option<&str>,
    value: &str,
    natural: bool,
    clear: bool,
    wait_timeout_ms: Option<u64>,
    sitemap_path: &Path,
    compact: bool,
) -> Result<String, String> {
    runtime_session_action(
        workspace_root,
        session_id,
        "fill",
        node_id,
        selector,
        Some(value),
        None,
        None,
        None,
        natural,
        clear,
        wait_timeout_ms,
        sitemap_path,
        compact,
    )
}

pub fn runtime_submit_session(
    workspace_root: &Path,
    session_id: &str,
    node_id: Option<&str>,
    selector: Option<&str>,
    wait_timeout_ms: Option<u64>,
    sitemap_path: &Path,
    compact: bool,
) -> Result<String, String> {
    runtime_session_action(
        workspace_root,
        session_id,
        "submit",
        node_id,
        selector,
        None,
        None,
        None,
        None,
        false,
        false,
        wait_timeout_ms,
        sitemap_path,
        compact,
    )
}

pub fn runtime_press_key_session(
    workspace_root: &Path,
    session_id: &str,
    key: &str,
    wait_timeout_ms: Option<u64>,
    sitemap_path: &Path,
    compact: bool,
) -> Result<String, String> {
    runtime_session_action(
        workspace_root,
        session_id,
        "press_key",
        None,
        None,
        None,
        Some(key),
        None,
        None,
        false,
        false,
        wait_timeout_ms,
        sitemap_path,
        compact,
    )
}

pub fn browser_runtime_visual_capture(
    workspace_root: &Path,
    url: &str,
    api_base: Option<&str>,
    compact: bool,
) -> Result<String, String> {
    let api_base_resolved = resolve_browser_runtime_api_base(api_base);
    let endpoint = format!("{}/api/runtime/visual-artifact", api_base_resolved);
    let response = ureq::post(&endpoint)
        .set("Content-Type", "application/json")
        .send_string(&serde_json::json!({"url": url}).to_string())
        .map_err(format_runtime_api_error)?;
    let artifact_kind = response
        .header("X-Runtime-Artifact-Kind")
        .unwrap_or("runtime_screenshot")
        .to_string();
    let captured_url = response
        .header("X-Runtime-Page-Url")
        .unwrap_or(url)
        .to_string();
    let mut reader = response.into_reader();
    let mut png = Vec::new();
    reader
        .read_to_end(&mut png)
        .map_err(|err| format!("read runtime visual artifact response: {err}"))?;
    let artifact_id = runtime_visual_artifact_id(url);
    let png_path = browser_runtime_visual_png_path(workspace_root, &artifact_id);
    let metadata_path = browser_runtime_visual_metadata_path(workspace_root, &artifact_id);
    if let Some(parent) = png_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create runtime visual artifact dir: {err}"))?;
    }
    fs::write(&png_path, &png).map_err(|err| format!("write runtime visual png: {err}"))?;
    let artifact = BrowserRuntimeVisualArtifact {
        artifact_id,
        artifact_kind,
        requested_url: url.to_string(),
        captured_url,
        mime_type: "image/png".to_string(),
        byte_length: png.len(),
        captured_at_ms: current_timestamp_ms(),
        png_path: png_path.display().to_string(),
        metadata_json_path: metadata_path.display().to_string(),
    };
    fs::write(
        &metadata_path,
        serde_json::to_vec_pretty(&artifact)
            .map_err(|err| format!("serialise runtime visual artifact metadata: {err}"))?,
    )
    .map_err(|err| format!("write runtime visual artifact metadata: {err}"))?;
    if compact {
        serde_json::to_string_pretty(&artifact)
            .map_err(|err| format!("serialise runtime visual artifact summary: {err}"))
    } else {
        Ok(format!(
            "Runtime visual artifact captured.\nKind: {}\nRequested URL: {}\nCaptured URL: {}\nMIME type: {}\nBytes: {}\nPNG path: {}\nMetadata JSON: {}",
            artifact.artifact_kind,
            artifact.requested_url,
            artifact.captured_url,
            artifact.mime_type,
            artifact.byte_length,
            artifact.png_path,
            artifact.metadata_json_path
        ))
    }
}

