
use super::models::*;
use serde::{Serialize, Deserialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use sha2::{Digest, Sha256};
use velocity_ide::site_map::verifier::NdaNode;
use velocity_ide::site_map::{SiteMap, VcTriple};
pub fn summarize_workflow(workflow: BrowserWorkflow) -> BrowserWorkflowSummary {
    BrowserWorkflowSummary {
        name: workflow.name,
        start_url: workflow.start_url,
        variable_count: workflow.variables.len(),
        step_count: workflow.steps.len(),
        json_path: None,
        nda_path: None,
    }
}

pub fn summarize_workflow_run(report: BrowserWorkflowRunReport) -> BrowserWorkflowRunSummary {
    BrowserWorkflowRunSummary {
        workflow_name: report.workflow_name,
        session_id: report.session_id,
        final_url: report.final_url,
        final_title: report.final_title,
        step_count: report.step_count,
        cookie_count: report.cookie_count,
        local_storage_count: report.local_storage_count,
        session_storage_count: report.session_storage_count,
        request_count: report.request_count,
        settle_signal_count: report.settle_signal_count,
        runtime_state_count: report.runtime_state_count,
        protocol_event_count: report.protocol_event_count,
        network_summary: report.network_summary,
        run_report_path: None,
    }
}

pub fn summarize_workflow_suite_run(
    report: BrowserWorkflowSuiteRunReport,
) -> BrowserWorkflowSuiteRunSummary {
    BrowserWorkflowSuiteRunSummary {
        suite_name: report.suite_name,
        total: report.total,
        passed: report.passed,
        failed: report.failed,
        suite_report_path: None,
    }
}

pub fn summarize_workflow_suite(suite: BrowserWorkflowSuite) -> BrowserWorkflowSuiteSummary {
    BrowserWorkflowSuiteSummary {
        name: suite.name,
        workflow_count: suite.workflows.len(),
        json_path: None,
    }
}

struct BrowserHttpResponse {
    html: String,
    final_url: String,
    cookies: Vec<BrowserCookie>,
    local_storage_updates: HashMap<String, String>,
    session_storage_updates: HashMap<String, String>,
    mutations: Vec<String>,
    requests: Vec<BrowserRequestRecord>,
    settle_signals: Vec<String>,
    runtime_state: Vec<BrowserRuntimeState>,
    protocol_events: Vec<BrowserProtocolEvent>,
}


#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
struct RuntimeCaptureApiRequestRecord {
    method: String,
    url: String,
    status_code: u16,
    resource: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
struct RuntimeCaptureApiState {
    scope: String,
    key: String,
    value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
struct RuntimeCaptureApiProtocolEvent {
    kind: String,
    phase: String,
    target: String,
    detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
struct RuntimeCaptureApiFrameEntry {
    #[serde(default)]
    selector: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    same_origin: bool,
    #[serde(default)]
    accessible: bool,
    #[serde(default)]
    semantic_node_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
struct RuntimeCaptureApiShadowHostEntry {
    #[serde(default)]
    selector: String,
    #[serde(default)]
    tag: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    mode: String,
    #[serde(default)]
    semantic_node_count: usize,
    #[serde(default)]
    text_sample: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
struct RuntimeCaptureApiCanvasEntry {
    #[serde(default)]
    selector: String,
    #[serde(default)]
    width: usize,
    #[serde(default)]
    height: usize,
    #[serde(default)]
    context_kinds: Vec<String>,
    #[serde(default)]
    text_op_count: usize,
    #[serde(default)]
    image_op_count: usize,
    #[serde(default)]
    webgl_draw_count: usize,
    #[serde(default)]
    readback_count: usize,
    #[serde(default)]
    likely_animated: bool,
    #[serde(default)]
    runtime_evidence: bool,
    #[serde(default)]
    text_sample: String,
}



#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
struct RuntimeCaptureApiResponse {
    final_url: String,
    title: String,
    html: String,
    aom_summary: String,
    page_text: String,
    #[serde(default)]
    scripts: Vec<String>,
    #[serde(default)]
    fields: HashMap<String, String>,
    #[serde(default)]
    cookies: Vec<RuntimeBrowserCookie>,
    #[serde(default)]
    local_storage: HashMap<String, String>,
    #[serde(default)]
    session_storage: HashMap<String, String>,
    #[serde(default)]
    settle_signals: Vec<String>,
    #[serde(default)]
    runtime_state: Vec<RuntimeCaptureApiState>,
    #[serde(default)]
    protocol_events: Vec<RuntimeCaptureApiProtocolEvent>,
    #[serde(default)]
    requests: Vec<RuntimeCaptureApiRequestRecord>,
    #[serde(default)]
    frames: Vec<RuntimeCaptureApiFrameEntry>,
    #[serde(default)]
    shadow_hosts: Vec<RuntimeCaptureApiShadowHostEntry>,
    #[serde(default)]
    canvases: Vec<RuntimeCaptureApiCanvasEntry>,
    #[serde(default)]
    warnings: Vec<String>,
    #[serde(default)]
    action: Option<RuntimeActionApiResult>,
}

#[derive(Debug, Serialize)]
struct RuntimeCaptureApiRequest<'a> {
    url: &'a str,
    timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct BrowserRuntimeVisualArtifact {
    pub artifact_id: String,
    pub artifact_kind: String,
    pub requested_url: String,
    pub captured_url: String,
    pub mime_type: String,
    pub byte_length: usize,
    pub captured_at_ms: u64,
    pub png_path: String,
    pub metadata_json_path: String,
}

fn render_html_fallback_line(html_fallback_path: Option<&str>) -> String {
    html_fallback_path
        .map(|path| format!("\nHTML fallback: {}", path))
        .unwrap_or_default()
}

fn browser_runtime_api_base() -> String {
    [
        "VELOCITY_BROWSER_RUNTIME_API_BASE",
        "VELOCITY_BROWSER_API_BASE",
    ]
    .into_iter()
    .find_map(|key| {
        std::env::var(key)
            .ok()
            .map(|value| value.trim().trim_end_matches('/').to_string())
            .filter(|value| !value.is_empty())
    })
    .unwrap_or_else(|| "http://127.0.0.1:8080".to_string())
}

fn resolve_browser_runtime_api_base(api_base: Option<&str>) -> String {
    api_base
        .map(str::trim)
        .map(|value| value.trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(browser_runtime_api_base)
}

fn format_runtime_api_error(err: ureq::Error) -> String {
    match err {
        ureq::Error::Status(code, response) => {
            let body = response.into_string().unwrap_or_default();
            if body.trim().is_empty() {
                format!("runtime api request failed with status {code}")
            } else {
                format!(
                    "runtime api request failed with status {code}: {}",
                    truncate_string(body.trim(), 500)
                )
            }
        }
        other => format!("runtime api request failed: {other}"),
    }
}

fn runtime_api_request(
    method: &str,
    url: &str,
    body: Option<&serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let response = match method {
        "GET" => ureq::get(url).call().map_err(format_runtime_api_error)?,
        "DELETE" => ureq::delete(url).call().map_err(format_runtime_api_error)?,
        "POST" => {
            let request = ureq::post(url).set("Content-Type", "application/json");
            match body {
                Some(value) => {
                    let payload = serde_json::to_string(value)
                        .map_err(|err| format!("serialise runtime api request: {err}"))?;
                    request
                        .send_string(&payload)
                        .map_err(format_runtime_api_error)?
                }
                None => request.call().map_err(format_runtime_api_error)?,
            }
        }
        other => return Err(format!("unsupported runtime api method '{other}'")),
    };
    let raw = response
        .into_string()
        .map_err(|err| format!("read runtime api response: {err}"))?;
    if raw.trim().is_empty() {
        Ok(serde_json::json!({}))
    } else {
        serde_json::from_str(&raw).map_err(|err| format!("parse runtime api response: {err}"))
    }
}

fn runtime_capture_response_from_value(
    value: serde_json::Value,
) -> Result<RuntimeCaptureApiResponse, String> {
    let candidates = [
        Some(value.clone()),
        value.get("capture").cloned(),
        value
            .get("result")
            .and_then(|result| result.get("capture"))
            .cloned(),
    ];
    for candidate in candidates.into_iter().flatten() {
        if let Ok(response) = serde_json::from_value::<RuntimeCaptureApiResponse>(candidate) {
            return Ok(response);
        }
    }
    Err("runtime capture response did not match a supported payload shape".to_string())
}

fn parse_runtime_session_cookie_value(raw: &str) -> RuntimeBrowserCookie {
    let trimmed = raw.trim();
    let (name, value) = trimmed.split_once('=').unwrap_or((trimmed, ""));
    RuntimeBrowserCookie {
        name: name.trim().to_string(),
        value: value.trim().to_string(),
        ..RuntimeBrowserCookie::default()
    }
}

fn parse_runtime_string_map(value: Option<&serde_json::Value>) -> HashMap<String, String> {
    value
        .and_then(serde_json::Value::as_object)
        .map(|entries| {
            entries
                .iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        value
                            .as_str()
                            .map(|item| item.to_string())
                            .unwrap_or_else(|| value.to_string()),
                    )
                })
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default()
}

fn parse_runtime_string_list(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(|item| item.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn parse_runtime_session_capture_response(
    value: serde_json::Value,
) -> Result<RuntimeCaptureApiResponse, String> {
    if let Ok(response) = runtime_capture_response_from_value(value.clone()) {
        return Ok(response);
    }

    let final_url = value
        .get("finalUrl")
        .or_else(|| value.get("final_url"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "runtime session capture response missing finalUrl".to_string())?
        .to_string();
    let title = value
        .get("title")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let html = value
        .get("html")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let aom_summary = value
        .get("aom")
        .or_else(|| value.get("aom_summary"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let page_text = value
        .get("pageText")
        .or_else(|| value.get("page_text"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let scripts = parse_runtime_string_list(value.get("scripts"));
    let fields = parse_runtime_string_map(value.get("fields"));

    let cookies = value
        .get("cookies")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    if let Some(object) = item.as_object() {
                        Some(RuntimeBrowserCookie {
                            name: object
                                .get("name")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            value: object
                                .get("value")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            domain: object
                                .get("domain")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_string),
                            path: object
                                .get("path")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_string),
                            secure: object
                                .get("secure")
                                .and_then(serde_json::Value::as_bool)
                                .unwrap_or(false),
                            http_only: object
                                .get("httpOnly")
                                .or_else(|| object.get("http_only"))
                                .and_then(serde_json::Value::as_bool)
                                .unwrap_or(false),
                            same_site: object
                                .get("sameSite")
                                .or_else(|| object.get("same_site"))
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_string),
                            expires_unix: object
                                .get("expiresUnix")
                                .or_else(|| object.get("expires_unix"))
                                .or_else(|| object.get("expires"))
                                .and_then(|value| value.as_i64().or_else(|| value.as_f64().map(|v| v as i64))),
                            session: object
                                .get("session")
                                .and_then(serde_json::Value::as_bool)
                                .unwrap_or(false),
                            source_scheme: object
                                .get("sourceScheme")
                                .or_else(|| object.get("source_scheme"))
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_string),
                            source_port: object
                                .get("sourcePort")
                                .or_else(|| object.get("source_port"))
                                .and_then(serde_json::Value::as_i64),
                        })
                    } else {
                        item.as_str().map(|raw| {
                            let parsed = parse_runtime_session_cookie_value(raw);
                            parsed
                        })
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let local_storage = value
        .get("local_storage")
        .map(Some)
        .map(parse_runtime_string_map)
        .unwrap_or_else(|| {
            parse_runtime_string_map(
                value
                    .get("storage")
                    .and_then(|storage| storage.get("local")),
            )
        });
    let session_storage = value
        .get("session_storage")
        .map(Some)
        .map(parse_runtime_string_map)
        .unwrap_or_else(|| {
            parse_runtime_string_map(
                value
                    .get("storage")
                    .and_then(|storage| storage.get("session")),
            )
        });

    let action = value
        .get("action")
        .and_then(serde_json::Value::as_object)
        .map(|action| RuntimeActionApiResult {
            action: action
                .get("action")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            target: action
                .get("target")
                .and_then(serde_json::Value::as_str)
                .map(|value| value.to_string()),
            value: action
                .get("value")
                .and_then(serde_json::Value::as_str)
                .map(|value| value.to_string()),
            key: action
                .get("key")
                .and_then(serde_json::Value::as_str)
                .map(|value| value.to_string()),
            script: action
                .get("script")
                .and_then(serde_json::Value::as_str)
                .map(|value| value.to_string()),
            result: action
                .get("result")
                .and_then(serde_json::Value::as_str)
                .map(|value| value.to_string()),
            wait_applied_ms: action
                .get("waitAppliedMs")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default() as usize,
            warnings: parse_runtime_string_list(action.get("warnings")),
        });

    let warnings = {
        let mut warnings = parse_runtime_string_list(value.get("warnings"));
        if let Some(action) = &action {
            warnings.extend(action.warnings.iter().cloned());
        }
        warnings
    };
    let frames = value
        .get("frames")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    serde_json::from_value::<RuntimeCaptureApiFrameEntry>(item.clone()).ok()
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let shadow_hosts = value
        .get("shadowHosts")
        .or_else(|| value.get("shadow_hosts"))
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    serde_json::from_value::<RuntimeCaptureApiShadowHostEntry>(item.clone()).ok()
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let canvases = value
        .get("canvases")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    serde_json::from_value::<RuntimeCaptureApiCanvasEntry>(item.clone()).ok()
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut runtime_state = Vec::new();
    if let Some(state) = value
        .get("runtimeState")
        .and_then(serde_json::Value::as_object)
    {
        if let Some(session_id) = state.get("sessionId").and_then(serde_json::Value::as_str) {
            runtime_state.push(RuntimeCaptureApiState {
                scope: "runtime_session".to_string(),
                key: "session_id".to_string(),
                value: session_id.to_string(),
            });
        }
        if let Some(alive) = state.get("alive").and_then(serde_json::Value::as_bool) {
            runtime_state.push(RuntimeCaptureApiState {
                scope: "runtime_session".to_string(),
                key: "alive".to_string(),
                value: alive.to_string(),
            });
        }
        if let Some(mode) = state.get("mode").and_then(serde_json::Value::as_str) {
            runtime_state.push(RuntimeCaptureApiState {
                scope: "runtime_session".to_string(),
                key: "mode".to_string(),
                value: mode.to_string(),
            });
        }
        if let Some(last_action) = state.get("lastAction").and_then(serde_json::Value::as_str) {
            runtime_state.push(RuntimeCaptureApiState {
                scope: "runtime_session".to_string(),
                key: "last_action".to_string(),
                value: last_action.to_string(),
            });
        }
        if let Some(active_target) = state
            .get("activeTargetId")
            .and_then(serde_json::Value::as_str)
        {
            runtime_state.push(RuntimeCaptureApiState {
                scope: "runtime_session".to_string(),
                key: "active_target_id".to_string(),
                value: active_target.to_string(),
            });
        }
        if let Some(main_target) = state
            .get("mainTargetId")
            .and_then(serde_json::Value::as_str)
        {
            runtime_state.push(RuntimeCaptureApiState {
                scope: "runtime_session".to_string(),
                key: "main_target_id".to_string(),
                value: main_target.to_string(),
            });
        }
        if let Some(debug_port) = state.get("debugPort").and_then(serde_json::Value::as_i64) {
            runtime_state.push(RuntimeCaptureApiState {
                scope: "runtime_session".to_string(),
                key: "debug_port".to_string(),
                value: debug_port.to_string(),
            });
        }
        if let Some(last_aom_nodes) = state
            .get("lastAomNodeCount")
            .and_then(serde_json::Value::as_i64)
        {
            runtime_state.push(RuntimeCaptureApiState {
                scope: "runtime_session".to_string(),
                key: "last_aom_node_count".to_string(),
                value: last_aom_nodes.to_string(),
            });
        }
        if let Some(created_at) = state.get("createdAt").and_then(serde_json::Value::as_str) {
            runtime_state.push(RuntimeCaptureApiState {
                scope: "runtime_session".to_string(),
                key: "created_at".to_string(),
                value: created_at.to_string(),
            });
        }
        if let Some(frame_count) = state.get("frameCount").and_then(serde_json::Value::as_u64) {
            runtime_state.push(RuntimeCaptureApiState {
                scope: "runtime_session".to_string(),
                key: "frame_count".to_string(),
                value: frame_count.to_string(),
            });
        }
        if let Some(shadow_host_count) = state
            .get("shadowHostCount")
            .and_then(serde_json::Value::as_u64)
        {
            runtime_state.push(RuntimeCaptureApiState {
                scope: "runtime_session".to_string(),
                key: "shadow_host_count".to_string(),
                value: shadow_host_count.to_string(),
            });
        }
        if let Some(canvas_count) = state.get("canvasCount").and_then(serde_json::Value::as_u64) {
            runtime_state.push(RuntimeCaptureApiState {
                scope: "runtime_session".to_string(),
                key: "canvas_count".to_string(),
                value: canvas_count.to_string(),
            });
        }
        if let Some(webgl_canvas_count) = state
            .get("webglCanvasCount")
            .and_then(serde_json::Value::as_u64)
        {
            runtime_state.push(RuntimeCaptureApiState {
                scope: "runtime_session".to_string(),
                key: "webgl_canvas_count".to_string(),
                value: webgl_canvas_count.to_string(),
            });
        }
    }
    if let Some(protocol) = value
        .get("protocolEvidence")
        .and_then(serde_json::Value::as_object)
    {
        if let Some(backend) = protocol.get("backend").and_then(serde_json::Value::as_str) {
            runtime_state.push(RuntimeCaptureApiState {
                scope: "runtime".to_string(),
                key: "backend".to_string(),
                value: backend.to_string(),
            });
        }
        if let Some(transport) = protocol
            .get("transport")
            .and_then(serde_json::Value::as_str)
        {
            runtime_state.push(RuntimeCaptureApiState {
                scope: "runtime".to_string(),
                key: "transport".to_string(),
                value: transport.to_string(),
            });
        }
        if let Some(session_mode) = protocol
            .get("sessionMode")
            .and_then(serde_json::Value::as_str)
        {
            runtime_state.push(RuntimeCaptureApiState {
                scope: "runtime".to_string(),
                key: "session_mode".to_string(),
                value: session_mode.to_string(),
            });
        }
        if let Some(actions) = protocol
            .get("supportsActions")
            .and_then(serde_json::Value::as_array)
        {
            let supported_actions = actions
                .iter()
                .filter_map(|action| action.as_str())
                .collect::<Vec<_>>()
                .join(",");
            if !supported_actions.is_empty() {
                runtime_state.push(RuntimeCaptureApiState {
                    scope: "runtime".to_string(),
                    key: "supports_actions".to_string(),
                    value: supported_actions,
                });
            }
        }
    }
    if !frames.is_empty() {
        runtime_state.push(RuntimeCaptureApiState {
            scope: "runtime_frames".to_string(),
            key: "count".to_string(),
            value: frames.len().to_string(),
        });
        let accessible_count = frames.iter().filter(|frame| frame.accessible).count();
        runtime_state.push(RuntimeCaptureApiState {
            scope: "runtime_frames".to_string(),
            key: "accessible_count".to_string(),
            value: accessible_count.to_string(),
        });
        let same_origin_count = frames.iter().filter(|frame| frame.same_origin).count();
        runtime_state.push(RuntimeCaptureApiState {
            scope: "runtime_frames".to_string(),
            key: "same_origin_count".to_string(),
            value: same_origin_count.to_string(),
        });
    }
    if !shadow_hosts.is_empty() {
        runtime_state.push(RuntimeCaptureApiState {
            scope: "runtime_shadow".to_string(),
            key: "host_count".to_string(),
            value: shadow_hosts.len().to_string(),
        });
        let semantic_count = shadow_hosts
            .iter()
            .map(|host| host.semantic_node_count)
            .sum::<usize>();
        runtime_state.push(RuntimeCaptureApiState {
            scope: "runtime_shadow".to_string(),
            key: "semantic_node_count".to_string(),
            value: semantic_count.to_string(),
        });
    }
    if !canvases.is_empty() {
        runtime_state.push(RuntimeCaptureApiState {
            scope: "runtime_canvas".to_string(),
            key: "count".to_string(),
            value: canvases.len().to_string(),
        });
        let webgl_count = canvases
            .iter()
            .filter(|canvas| {
                canvas
                    .context_kinds
                    .iter()
                    .any(|kind| kind.starts_with("webgl"))
            })
            .count();
        runtime_state.push(RuntimeCaptureApiState {
            scope: "runtime_canvas".to_string(),
            key: "webgl_count".to_string(),
            value: webgl_count.to_string(),
        });
        let evidence_count = canvases
            .iter()
            .filter(|canvas| canvas.runtime_evidence)
            .count();
        runtime_state.push(RuntimeCaptureApiState {
            scope: "runtime_canvas".to_string(),
            key: "runtime_evidence_count".to_string(),
            value: evidence_count.to_string(),
        });
        let animated_count = canvases
            .iter()
            .filter(|canvas| canvas.likely_animated)
            .count();
        runtime_state.push(RuntimeCaptureApiState {
            scope: "runtime_canvas".to_string(),
            key: "animated_count".to_string(),
            value: animated_count.to_string(),
        });
    }
    if let Some(action_result) = &action {
        runtime_state.push(RuntimeCaptureApiState {
            scope: "runtime_action".to_string(),
            key: "action".to_string(),
            value: action_result.action.clone(),
        });
        if let Some(target) = &action_result.target {
            runtime_state.push(RuntimeCaptureApiState {
                scope: "runtime_action".to_string(),
                key: "target".to_string(),
                value: target.clone(),
            });
        }
        if let Some(value) = &action_result.value {
            runtime_state.push(RuntimeCaptureApiState {
                scope: "runtime_action".to_string(),
                key: "value".to_string(),
                value: (value as &String).clone(),
            });
        }
        if let Some(key) = &action_result.key {
            runtime_state.push(RuntimeCaptureApiState {
                scope: "runtime_action".to_string(),
                key: "key".to_string(),
                value: key.clone(),
            });
        }
        if let Some(script) = &action_result.script {
            runtime_state.push(RuntimeCaptureApiState {
                scope: "runtime_action".to_string(),
                key: "script".to_string(),
                value: script.clone(),
            });
        }
        if let Some(result) = &action_result.result {
            runtime_state.push(RuntimeCaptureApiState {
                scope: "runtime_action".to_string(),
                key: "result".to_string(),
                value: result.clone(),
            });
        }
        runtime_state.push(RuntimeCaptureApiState {
            scope: "runtime_action".to_string(),
            key: "wait_applied_ms".to_string(),
            value: action_result.wait_applied_ms.to_string(),
        });
    }

    Ok(RuntimeCaptureApiResponse {
        final_url,
        title,
        html,
        aom_summary,
        page_text,
        scripts,
        fields,
        cookies,
        local_storage,
        session_storage,
        settle_signals: Vec::new(),
        runtime_state,
        protocol_events: Vec::new(),
        requests: Vec::new(),
        frames,
        shadow_hosts,
        canvases,
        warnings,
        action,
    })
}

fn empty_browser_session_state(session_id: &str) -> BrowserSessionState {
    BrowserSessionState {
        id: session_id.to_string(),
        current_url: None,
        cookies: Vec::new(),
        runtime_cookies: Vec::new(),
        local_storage: HashMap::new(),
        session_storage: HashMap::new(),
        network: BrowserSessionNetworkConfig::default(),
        last_html: None,
    }
}

fn default_browser_user_agent() -> &'static str {
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.7727.138 Safari/537.36"
}

fn normalize_network_config(config: &mut BrowserSessionNetworkConfig) {
    config.headers.retain(|key, _| !key.trim().is_empty());
    config.allowed_url_prefixes = config
        .allowed_url_prefixes
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    config.blocked_url_prefixes = config
        .blocked_url_prefixes
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
}

fn network_policy_allows_url(
    config: &BrowserSessionNetworkConfig,
    url: &str,
) -> Result<(), String> {
    if config
        .blocked_url_prefixes
        .iter()
        .any(|prefix| url.starts_with(prefix))
    {
        return Err(format!("network policy blocked url '{url}'"));
    }
    if !config.allowed_url_prefixes.is_empty()
        && !config
            .allowed_url_prefixes
            .iter()
            .any(|prefix| url.starts_with(prefix))
    {
        return Err(format!("network policy disallowed url '{url}'"));
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct BrowserReplayState {
    session: BrowserSessionState,
    snapshot: BrowserPageSnapshot,
    filled_fields: HashMap<String, String>,
    variables: HashMap<String, String>,
    outputs: HashMap<String, String>,
}

const DEFAULT_WAIT_TIMEOUT_MS: u64 = 5_000;
const DEFAULT_WAIT_INTERVAL_MS: u64 = 250;
const DEFAULT_STABLE_POLLS: u32 = 2;

fn replay_lookup<'a>(state: &'a BrowserReplayState, key: &str) -> Option<&'a str> {
    state
        .outputs
        .get(key)
        .map(|value| value.as_str())
        .or_else(|| state.variables.get(key).map(|value| value.as_str()))
}

fn resolve_template(input: &str, state: &BrowserReplayState) -> String {
    if !input.contains("{{") {
        return input.to_string();
    }

    let mut out = String::with_capacity(input.len());
    let mut remaining = input;
    loop {
        let Some(start) = remaining.find("{{") else {
            out.push_str(remaining);
            break;
        };
        out.push_str(&remaining[..start]);
        let after_start = &remaining[start + 2..];
        let Some(end) = after_start.find("}}") else {
            out.push_str(&remaining[start..]);
            break;
        };
        let key = after_start[..end].trim();
        if let Some(value) = replay_lookup(state, key) {
            out.push_str(value);
        } else {
            out.push_str(&remaining[start..start + end + 4]);
        }
        remaining = &after_start[end + 2..];
    }
    out
}

fn content_hash_id(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    digest[..8]
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect::<String>()
}

fn extract_attr(tag: &str, attr_name: &str) -> Option<String> {
    let search = format!("{}=", attr_name);
    let lower = tag.to_ascii_lowercase();
    let idx = lower.find(&search)?;
    let after_eq = &tag[idx + search.len()..];
    if after_eq.is_empty() {
        return None;
    }
    let quote_char = after_eq.chars().next()?;
    if quote_char == '"' || quote_char == '\'' {
        let val_part = &after_eq[1..];
        let end_idx = val_part.find(quote_char)?;
        Some(val_part[..end_idx].to_string())
    } else {
        let end_idx = after_eq.find(|c: char| c.is_whitespace() || c == '/' || c == '>');
        Some(match end_idx {
            Some(end) => after_eq[..end].to_string(),
            None => after_eq.to_string(),
        })
    }
}

fn resolve_relative_url(base: &str, relative: &str) -> String {
    if relative.starts_with("http://") || relative.starts_with("https://") {
        return relative.to_string();
    }

    let base_trimmed = base.trim_end_matches('/');
    if relative.starts_with('/') {
        if let Some(domain_end) = base_trimmed.find("://") {
            let domain_part = &base_trimmed[domain_end + 3..];
            if let Some(slash_idx) = domain_part.find('/') {
                let domain = &base_trimmed[..domain_end + 3 + slash_idx];
                return format!("{}{}", domain, relative);
            }
        }
        return format!("{}{}", base_trimmed, relative);
    }

    if let Some(last_slash) = base_trimmed.rfind('/') {
        if last_slash > 8 {
            return format!("{}/{}", &base_trimmed[..last_slash], relative);
        }
    }
    format!("{}/{}", base_trimmed, relative)
}

fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

fn strip_html_tags(fragment: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    let mut last_was_space = true;
    for ch in fragment.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => {
                let normalized = if ch.is_whitespace() { ' ' } else { ch };
                if normalized == ' ' {
                    if !last_was_space {
                        text.push(' ');
                        last_was_space = true;
                    }
                } else {
                    text.push(normalized);
                    last_was_space = false;
                }
            }
            _ => {}
        }
    }
    text.trim().to_string()
}

fn extract_element_body_text(html: &str, start_index: usize, closing_tag: &str) -> String {
    let body_start = start_index.min(html.len());
    let lower_tail = html[body_start..].to_ascii_lowercase();
    if let Some(close_rel) = lower_tail.find(closing_tag) {
        strip_html_tags(&html[body_start..body_start + close_rel])
    } else {
        String::new()
    }
}

fn encode_nda_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}

fn sanitize_file_stem(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "workflow".to_string()
    } else {
        trimmed.to_string()
    }
}

fn session_file_path(workspace_root: &Path, session_id: &str) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-sessions")
        .join(format!("{}.json", sanitize_file_stem(session_id)))
}

fn runtime_session_file_path(workspace_root: &Path, session_id: &str) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("runtime-browser-sessions")
        .join(format!("{}.json", sanitize_file_stem(session_id)))
}

fn browser_runtime_visual_dir(workspace_root: &Path) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-runtime-visuals")
}

fn browser_runtime_visual_png_path(workspace_root: &Path, artifact_id: &str) -> PathBuf {
    browser_runtime_visual_dir(workspace_root)
        .join(format!("{}.png", sanitize_file_stem(artifact_id)))
}

fn browser_runtime_visual_metadata_path(workspace_root: &Path, artifact_id: &str) -> PathBuf {
    browser_runtime_visual_dir(workspace_root)
        .join(format!("{}.json", sanitize_file_stem(artifact_id)))
}

fn runtime_visual_artifact_id(url: &str) -> String {
    content_hash_id(url)
}

fn crawl_facts_path(url: &str, sitemap_path: &Path) -> PathBuf {
    sitemap_path
        .parent()
        .unwrap_or(sitemap_path)
        .join("browser-captures")
        .join(format!("{}.nda", content_hash_id(url)))
}

fn browser_snapshot_path(url: &str, sitemap_path: &Path) -> PathBuf {
    sitemap_path
        .parent()
        .unwrap_or(sitemap_path)
        .join("browser-snapshots")
        .join(format!("{}.json", content_hash_id(url)))
}

fn browser_html_fallback_path(url: &str, sitemap_path: &Path) -> PathBuf {
    sitemap_path
        .parent()
        .unwrap_or(sitemap_path)
        .join("browser-html-fallbacks")
        .join(format!("{}.html", content_hash_id(url)))
}

fn browser_session_transcript_path(workspace_root: &Path, session_id: &str) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-session-transcripts")
        .join(format!("{}.json", sanitize_file_stem(session_id)))
}

fn browser_workflow_json_path(workspace_root: &Path, workflow_name: &str) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-workflows")
        .join(format!(
            "{}.browser.json",
            sanitize_file_stem(workflow_name)
        ))
}

fn browser_workflow_nda_path(workspace_root: &Path, workflow_name: &str) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-workflows")
        .join(format!("{}.browser.nda", sanitize_file_stem(workflow_name)))
}

fn browser_workflow_run_path(
    workspace_root: &Path,
    workflow_name: &str,
    session_id: &str,
) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-runs")
        .join(format!(
            "{}--{}.run.json",
            sanitize_file_stem(workflow_name),
            sanitize_file_stem(session_id)
        ))
}

fn browser_workflow_suite_json_path(workspace_root: &Path, suite_name: &str) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-suites")
        .join(format!("{}.suite.json", sanitize_file_stem(suite_name)))
}

fn browser_workflow_suite_run_path(workspace_root: &Path, suite_name: &str) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-suite-runs")
        .join(format!("{}.suite-run.json", sanitize_file_stem(suite_name)))
}

fn browser_session_checkpoint_path(
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

fn browser_auth_profile_json_path(workspace_root: &Path, profile_name: &str) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-auth-profiles")
        .join(format!("{}.auth.json", sanitize_file_stem(profile_name)))
}

fn parse_cookie_header(value: &str) -> Option<BrowserCookie> {
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

fn merge_cookie(cookies: &mut Vec<BrowserCookie>, cookie: BrowserCookie) {
    if let Some(existing) = cookies.iter_mut().find(|entry| entry.name == cookie.name) {
        *existing = cookie;
    } else {
        cookies.push(cookie);
    }
}

fn sync_runtime_cookies_from_browser_cookies(session: &mut BrowserSessionState) {
    session.runtime_cookies = session
        .cookies
        .iter()
        .map(browser_cookie_as_runtime_cookie)
        .collect();
}

fn auth_runtime_cookies_for_source(source: &BrowserSessionState) -> Vec<RuntimeBrowserCookie> {
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

fn cookie_header(cookies: &[BrowserCookie]) -> Option<String> {
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

fn parse_storage_header(raw: &str) -> HashMap<String, String> {
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

fn parse_list_header(raw: &str) -> Vec<String> {
    raw.split(';')
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .collect()
}

fn request_records_from_headers(
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

fn request_record_matches(
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

fn storage_entry_matches(
    snapshot: &BrowserPageSnapshot,
    scope: &str,
    key: &str,
    value: Option<&str>,
) -> bool {
    snapshot.storage.iter().any(|bucket| {
        bucket.scope.eq_ignore_ascii_case(scope)
            && bucket.entries.iter().any(|(entry_key, entry_value)| {
                entry_key.eq_ignore_ascii_case(key)
                    && value
                        .map(|needle| entry_value.contains(needle))
                        .unwrap_or(true)
            })
    })
}

fn protocol_event_matches(
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

fn default_settle_signals(method: &str, status_code: u16) -> Vec<String> {
    let mut signals = vec!["response_complete".to_string()];
    if method.eq_ignore_ascii_case("GET") {
        signals.push("navigation_settled".to_string());
    }
    if (200..400).contains(&status_code) {
        signals.push("network_settled".to_string());
    }
    signals
}

fn settle_signals_from_headers(method: &str, status_code: u16, raw: Option<&str>) -> Vec<String> {
    let mut signals = parse_list_header(raw.unwrap_or_default());
    if signals.is_empty() {
        signals = default_settle_signals(method, status_code);
    }
    signals.sort();
    signals.dedup();
    signals
}

fn parse_settle_signal_parts(signal: &str) -> Option<(&str, &str)> {
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

fn settle_signal_matches(
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

fn runtime_state_from_headers(raw: Option<&str>) -> Vec<BrowserRuntimeState> {
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

fn protocol_event_signature(event: &BrowserProtocolEvent) -> String {
    format!(
        "{}:{}:{}:{}",
        event.kind, event.phase, event.target, event.detail
    )
}

fn protocol_events_from_headers(raw: Option<&str>) -> Vec<BrowserProtocolEvent> {
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

pub fn summarize_network_activity(events: &[BrowserProtocolEvent]) -> BrowserNetworkSummary {
    let mut summary = BrowserNetworkSummary {
        event_count: events.len(),
        ..BrowserNetworkSummary::default()
    };
    for event in events {
        if event.kind.eq_ignore_ascii_case("redirect") {
            summary.redirect_count += 1;
            summary.last_redirect_target = Some(event.target.clone());
        } else if event.kind.eq_ignore_ascii_case("download") {
            summary.download_count += 1;
            summary.last_download_target = Some(event.target.clone());
        } else if event.kind.eq_ignore_ascii_case("upload") {
            summary.upload_count += 1;
            summary.last_upload_target = Some(event.target.clone());
        } else if event.kind.eq_ignore_ascii_case("event_stream")
            || (event.kind.eq_ignore_ascii_case("stream")
                && (event.phase.eq_ignore_ascii_case("sse")
                    || event.detail.to_ascii_lowercase().contains("event-stream")
                    || event.target.to_ascii_lowercase().contains("/events")))
        {
            summary.event_stream_count += 1;
            summary.stream_count += 1;
            summary.last_event_stream_target = Some(event.target.clone());
            summary.last_stream_target = Some(event.target.clone());
        } else if event.kind.eq_ignore_ascii_case("websocket")
            || (event.kind.eq_ignore_ascii_case("stream")
                && (event.phase.eq_ignore_ascii_case("websocket")
                    || event.phase.eq_ignore_ascii_case("ws")
                    || event.target.to_ascii_lowercase().starts_with("ws://")
                    || event.target.to_ascii_lowercase().starts_with("wss://")))
        {
            summary.websocket_count += 1;
            summary.stream_count += 1;
            summary.last_websocket_target = Some(event.target.clone());
            summary.last_stream_target = Some(event.target.clone());
        } else if event.kind.eq_ignore_ascii_case("stream") {
            summary.stream_count += 1;
            summary.last_stream_target = Some(event.target.clone());
        } else {
            summary.other_count += 1;
        }
    }
    summary
}

fn render_network_summary(summary: &BrowserNetworkSummary) -> Option<String> {
    if summary.event_count == 0 {
        return None;
    }
    let mut parts = vec![
        format!("redirects={}", summary.redirect_count),
        format!("downloads={}", summary.download_count),
        format!("uploads={}", summary.upload_count),
        format!("streams={}", summary.stream_count),
    ];
    if summary.event_stream_count > 0 {
        parts.push(format!("event_streams={}", summary.event_stream_count));
    }
    if summary.websocket_count > 0 {
        parts.push(format!("websockets={}", summary.websocket_count));
    }
    if summary.other_count > 0 {
        parts.push(format!("other={}", summary.other_count));
    }
    if let Some(target) = summary.last_redirect_target.as_deref() {
        parts.push(format!("last_redirect={}", target));
    }
    if let Some(target) = summary.last_download_target.as_deref() {
        parts.push(format!("last_download={}", target));
    }
    if let Some(target) = summary.last_upload_target.as_deref() {
        parts.push(format!("last_upload={}", target));
    }
    if let Some(target) = summary.last_stream_target.as_deref() {
        parts.push(format!("last_stream={}", target));
    }
    if let Some(target) = summary.last_event_stream_target.as_deref() {
        parts.push(format!("last_event_stream={}", target));
    }
    if let Some(target) = summary.last_websocket_target.as_deref() {
        parts.push(format!("last_websocket={}", target));
    }
    Some(parts.join(", "))
}

fn storage_buckets(session: &BrowserSessionState) -> Vec<BrowserStorageBucket> {
    let mut buckets = Vec::new();
    if !session.local_storage.is_empty() {
        buckets.push(BrowserStorageBucket {
            scope: "local".to_string(),
            entries: session.local_storage.clone(),
        });
    }
    if !session.session_storage.is_empty() {
        buckets.push(BrowserStorageBucket {
            scope: "session".to_string(),
            entries: session.session_storage.clone(),
        });
    }
    buckets
}

fn storage_signature(bucket: &BrowserStorageBucket) -> Vec<String> {
    let mut entries = bucket
        .entries
        .iter()
        .map(|(key, value)| format!("{}:{}={}", bucket.scope, key, value))
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

fn apply_storage_updates(target: &mut HashMap<String, String>, updates: &HashMap<String, String>) {
    for (key, value) in updates {
        target.insert(key.clone(), value.clone());
    }
}

fn fetch_with_session(
    url: &str,
    method: &str,
    body: Option<&str>,
    cookies: &[BrowserCookie],
    network: &BrowserSessionNetworkConfig,
) -> Result<BrowserHttpResponse, String> {
    network_policy_allows_url(network, url)?;

    let mut agent_builder = ureq::AgentBuilder::new();
    if let Some(timeout_ms) = network.timeout_ms {
        agent_builder = agent_builder.timeout(Duration::from_millis(timeout_ms));
    }
    if let Some(follow_redirects) = network.follow_redirects {
        agent_builder = if follow_redirects {
            agent_builder.redirects(10)
        } else {
            agent_builder.redirects(0)
        };
    }
    let agent = agent_builder.build();
    let mut request = agent.request(method, url).set(
        "User-Agent",
        network
            .user_agent
            .as_deref()
            .unwrap_or(default_browser_user_agent()),
    );
    for (key, value) in &network.headers {
        request = request.set(key, value);
    }
    if let Some(header) = cookie_header(cookies) {
        request = request.set("Cookie", &header);
    }

    let response = if method.eq_ignore_ascii_case("POST") {
        request
            .set("Content-Type", "application/x-www-form-urlencoded")
            .send_string(body.unwrap_or_default())
            .map_err(|e| format!("HTTP request failed: {:?}", e))?
    } else {
        request
            .call()
            .map_err(|e| format!("HTTP request failed: {:?}", e))?
    };

    let mut response_cookies = Vec::new();
    for header in response.all("Set-Cookie") {
        if let Some(cookie) = parse_cookie_header(header) {
            merge_cookie(&mut response_cookies, cookie);
        }
    }
    let status_code = response.status();
    let local_storage_updates = response
        .header("X-Velocity-Local-Storage")
        .map(parse_storage_header)
        .unwrap_or_default();
    let session_storage_updates = response
        .header("X-Velocity-Session-Storage")
        .map(parse_storage_header)
        .unwrap_or_default();
    let mutations = response
        .header("X-Velocity-Mutations")
        .map(parse_list_header)
        .unwrap_or_default();
    let requests = request_records_from_headers(
        method,
        url,
        status_code,
        response.header("X-Velocity-Requests"),
    );
    let settle_signals =
        settle_signals_from_headers(method, status_code, response.header("X-Velocity-Settle"));
    let runtime_state = runtime_state_from_headers(response.header("X-Velocity-Runtime-State"));
    let mut protocol_events =
        protocol_events_from_headers(response.header("X-Velocity-Protocol-Events"));
    let final_url = response.get_url().to_string();
    if final_url != url {
        protocol_events.push(BrowserProtocolEvent {
            kind: "navigation".to_string(),
            phase: "redirected".to_string(),
            target: final_url.clone(),
            detail: url.to_string(),
        });
        protocol_events.sort_by(|left, right| {
            protocol_event_signature(left).cmp(&protocol_event_signature(right))
        });
        protocol_events.dedup();
    }

    let html = response
        .into_string()
        .map_err(|e| format!("Failed to read HTTP body: {:?}", e))?;
    Ok(BrowserHttpResponse {
        html,
        final_url,
        cookies: response_cookies,
        local_storage_updates,
        session_storage_updates,
        mutations,
        requests,
        settle_signals,
        runtime_state,
        protocol_events,
    })
}

fn scan_tags(fragment: &str) -> Vec<String> {
    let chars: Vec<char> = fragment.chars().collect();
    let mut tags = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            i += 1;
            let mut tag = String::new();
            while i < chars.len() && chars[i] != '>' {
                tag.push(chars[i]);
                i += 1;
            }
            tags.push(tag);
        }
        i += 1;
    }
    tags
}

fn parse_forms(url: &str, html: &str) -> Vec<BrowserForm> {
    let lower_html = html.to_ascii_lowercase();
    let mut forms = Vec::new();
    let mut search_from = 0;

    while let Some(form_start_rel) = lower_html[search_from..].find("<form") {
        let form_start = search_from + form_start_rel;
        let tag_end_rel = lower_html[form_start..].find('>');
        let Some(tag_end_rel) = tag_end_rel else {
            break;
        };
        let tag_end = form_start + tag_end_rel;
        let form_tag = &html[form_start + 1..tag_end];
        let body_start = tag_end + 1;
        let close_rel = lower_html[body_start..].find("</form>");
        let Some(close_rel) = close_rel else {
            break;
        };
        let body_end = body_start + close_rel;
        let form_body = &html[body_start..body_end];

        let form_id = extract_attr(form_tag, "id")
            .or_else(|| extract_attr(form_tag, "name"))
            .unwrap_or_else(|| format!("form-{}", forms.len()));
        let action = extract_attr(form_tag, "action")
            .map(|value| resolve_relative_url(url, &value))
            .unwrap_or_else(|| url.to_string());
        let method = extract_attr(form_tag, "method")
            .unwrap_or_else(|| "GET".to_string())
            .to_ascii_uppercase();

        let mut fields = Vec::new();
        let mut submit_label = None;
        for raw_tag in scan_tags(form_body) {
            let trimmed = raw_tag.trim();
            let lower = trimmed.to_ascii_lowercase();
            if lower.starts_with("input") {
                let input_type =
                    extract_attr(trimmed, "type").unwrap_or_else(|| "text".to_string());
                let name = extract_attr(trimmed, "name")
                    .or_else(|| extract_attr(trimmed, "id"))
                    .unwrap_or_else(|| format!("field-{}", fields.len()));
                let label = extract_attr(trimmed, "placeholder")
                    .or_else(|| extract_attr(trimmed, "aria-label"))
                    .unwrap_or_else(|| name.clone());
                let value = extract_attr(trimmed, "value").unwrap_or_default();

                if matches!(input_type.as_str(), "submit" | "button") {
                    if submit_label.is_none() {
                        submit_label = Some(if !value.is_empty() { value } else { label });
                    }
                } else {
                    fields.push(BrowserFormField {
                        name,
                        label,
                        input_type,
                        value,
                    });
                }
            } else if lower.starts_with("textarea") {
                let name = extract_attr(trimmed, "name")
                    .or_else(|| extract_attr(trimmed, "id"))
                    .unwrap_or_else(|| format!("field-{}", fields.len()));
                let label = extract_attr(trimmed, "placeholder")
                    .or_else(|| extract_attr(trimmed, "aria-label"))
                    .unwrap_or_else(|| name.clone());
                fields.push(BrowserFormField {
                    name,
                    label,
                    input_type: "textarea".to_string(),
                    value: String::new(),
                });
            } else if lower.starts_with("button") && submit_label.is_none() {
                submit_label = extract_attr(trimmed, "aria-label")
                    .or_else(|| extract_attr(trimmed, "name"))
                    .or_else(|| extract_attr(trimmed, "value"));
            }
        }

        forms.push(BrowserForm {
            id: form_id,
            action,
            method,
            fields,
            submit_label,
        });
        search_from = body_end + "</form>".len();
    }

    forms
}

fn parse_html_to_snapshot(
    url: &str,
    html: &str,
    cookies: &[BrowserCookie],
    storage: &[BrowserStorageBucket],
    mutations: &[String],
    requests: &[BrowserRequestRecord],
    settle_signals: &[String],
) -> BrowserPageSnapshot {
    parse_html_to_snapshot_with_runtime_state(
        url,
        html,
        cookies,
        storage,
        mutations,
        requests,
        settle_signals,
        &[],
        &[],
    )
}

fn parse_html_to_snapshot_with_runtime_state(
    url: &str,
    html: &str,
    cookies: &[BrowserCookie],
    storage: &[BrowserStorageBucket],
    mutations: &[String],
    requests: &[BrowserRequestRecord],
    settle_signals: &[String],
    runtime_state: &[BrowserRuntimeState],
    protocol_events: &[BrowserProtocolEvent],
) -> BrowserPageSnapshot {
    let forms = parse_forms(url, html);
    let mut elements = Vec::new();
    let mut title = "Untitled Page".to_string();
    let mut page_text = String::new();

    let chars: Vec<char> = html.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            let tag_start = i;
            let mut tag_content = String::new();
            i += 1;
            while i < chars.len() && chars[i] != '>' {
                tag_content.push(chars[i]);
                i += 1;
            }
            let body_start = (i + 1).min(chars.len());
            let trimmed = tag_content.trim();
            let lower = trimmed.to_ascii_lowercase();
            if lower.starts_with("title") {
                i += 1;
                let mut t = String::new();
                while i < chars.len() && chars[i] != '<' {
                    t.push(chars[i]);
                    i += 1;
                }
                title = t.trim().to_string();
            } else if lower.starts_with("a ") || lower.starts_with("a>") {
                let href = extract_attr(trimmed, "href");
                let clean_text = extract_element_body_text(html, body_start, "</a>");
                if let Some(href_value) = href {
                    let absolute_href = resolve_relative_url(url, &href_value);
                    elements.push(AomElement {
                        role: "link".to_string(),
                        name: if clean_text.is_empty() {
                            absolute_href.clone()
                        } else {
                            clean_text
                        },
                        value: absolute_href.clone(),
                        target_url: Some(absolute_href),
                        supported_actions: vec!["open".to_string(), "click".to_string()],
                        provenance: "native-static".to_string(),
                        actionability: role_actionability("link"),
                    });
                }
            } else if lower.starts_with("button") {
                let label = extract_element_body_text(html, body_start, "</button>");
                let fallback = extract_attr(trimmed, "aria-label")
                    .or_else(|| extract_attr(trimmed, "name"))
                    .or_else(|| extract_attr(trimmed, "value"))
                    .unwrap_or_default();
                let final_name = if label.is_empty() { fallback } else { label };
                if !final_name.is_empty() {
                    elements.push(AomElement {
                        role: "button".to_string(),
                        name: final_name,
                        value: String::new(),
                        target_url: None,
                        supported_actions: vec!["click".to_string()],
                        provenance: "native-static".to_string(),
                        actionability: role_actionability("button"),
                    });
                }
            } else if lower.starts_with("input") {
                let input_type =
                    extract_attr(trimmed, "type").unwrap_or_else(|| "text".to_string());
                let placeholder = extract_attr(trimmed, "placeholder").unwrap_or_default();
                let aria_label = extract_attr(trimmed, "aria-label").unwrap_or_default();
                let name_attr = extract_attr(trimmed, "name").unwrap_or_default();
                let value_attr = extract_attr(trimmed, "value").unwrap_or_default();
                let name = if !placeholder.is_empty() {
                    placeholder
                } else if !aria_label.is_empty() {
                    aria_label
                } else if !name_attr.is_empty() {
                    name_attr
                } else {
                    "Input Field".to_string()
                };
                let role = match input_type.as_str() {
                    "button" | "submit" => "button",
                    _ => "textbox",
                };
                let supported_actions = if role == "button" {
                    vec!["click".to_string()]
                } else {
                    vec!["focus".to_string(), "type".to_string()]
                };
                elements.push(AomElement {
                    role: role.to_string(),
                    name,
                    value: value_attr,
                    target_url: None,
                    supported_actions,
                    provenance: "native-static".to_string(),
                    actionability: role_actionability(role),
                });
            }
            let _ = tag_start;
        } else {
            if chars[i] != '\r' && chars[i] != '\n' && chars[i] != '\t' {
                page_text.push(chars[i]);
            }
            i += 1;
        }
    }

    for form in &forms {
        for field in &form.fields {
            if elements.iter().any(|element| {
                element.role.eq_ignore_ascii_case("textbox")
                    && (element.name.eq_ignore_ascii_case(&field.label)
                        || element.name.eq_ignore_ascii_case(&field.name))
            }) {
                continue;
            }
            elements.push(AomElement {
                role: "textbox".to_string(),
                name: if field.label.is_empty() {
                    field.name.clone()
                } else {
                    field.label.clone()
                },
                value: field.value.clone(),
                target_url: None,
                supported_actions: vec!["focus".to_string(), "type".to_string()],
                provenance: "native-static-repaired".to_string(),
                actionability: if field.input_type.eq_ignore_ascii_case("hidden") {
                    0
                } else {
                    role_actionability("textbox")
                },
            });
        }
        if let Some(label) = form
            .submit_label
            .as_ref()
            .filter(|label| !label.trim().is_empty())
        {
            if !elements.iter().any(|element| {
                element.role.eq_ignore_ascii_case("button")
                    && element.name.eq_ignore_ascii_case(label)
            }) {
                elements.push(AomElement {
                    role: "button".to_string(),
                    name: label.trim().to_string(),
                    value: form.id.clone(),
                    target_url: None,
                    supported_actions: vec!["click".to_string(), "submit".to_string()],
                    provenance: "native-static-repaired".to_string(),
                    actionability: role_actionability("button"),
                });
            }
        }
    }

    BrowserPageSnapshot {
        url: url.to_string(),
        title,
        summary: truncate_string(page_text.trim(), 1000),
        elements,
        forms,
        cookies: cookies.to_vec(),
        storage: storage.to_vec(),
        mutations: mutations.to_vec(),
        requests: requests.to_vec(),
        settle_signals: settle_signals.to_vec(),
        runtime_state: runtime_state.to_vec(),
        protocol_events: protocol_events.to_vec(),
    }
}

fn write_snapshot_json(
    snapshot: &BrowserPageSnapshot,
    sitemap_path: &Path,
) -> Result<PathBuf, String> {
    let snapshot_path = browser_snapshot_path(&snapshot.url, sitemap_path);
    if let Some(parent) = snapshot_path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create browser snapshot dir: {err}"))?;
    }
    let json = serde_json::to_vec_pretty(snapshot)
        .map_err(|err| format!("serialise browser snapshot: {err}"))?;
    fs::write(&snapshot_path, json).map_err(|err| format!("write browser snapshot: {err}"))?;
    Ok(snapshot_path)
}

fn write_html_fallback(
    url: &str,
    html: &str,
    sitemap_path: &Path,
) -> Result<Option<PathBuf>, String> {
    if html.trim().is_empty() {
        return Ok(None);
    }
    let path = browser_html_fallback_path(url, sitemap_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create browser html fallback dir: {err}"))?;
    }
    fs::write(&path, html.as_bytes())
        .map_err(|err| format!("write browser html fallback: {err}"))?;
    Ok(Some(path))
}

fn load_html_fallback(url: &str, sitemap_path: &Path) -> Result<String, String> {
    let path = browser_html_fallback_path(url, sitemap_path);
    fs::read_to_string(&path).map_err(|err| format!("read browser html fallback: {err}"))
}

fn load_snapshot_json(url: &str, sitemap_path: &Path) -> Result<BrowserPageSnapshot, String> {
    let snapshot_path = browser_snapshot_path(url, sitemap_path);
    let raw = fs::read(&snapshot_path).map_err(|err| format!("read browser snapshot: {err}"))?;
    serde_json::from_slice(&raw).map_err(|err| format!("parse browser snapshot: {err}"))
}

pub fn read_snapshot(url: &str, sitemap_path: &Path) -> Result<BrowserPageSnapshot, String> {
    load_snapshot_json(url, sitemap_path)
}

pub fn read_visual_fallback_report(
    url: &str,
    sitemap_path: &Path,
) -> Result<BrowserVisualFallbackReadReport, String> {
    let html = load_html_fallback(url, sitemap_path)?;
    let path = browser_html_fallback_path(url, sitemap_path);
    Ok(BrowserVisualFallbackReadReport {
        url: url.to_string(),
        html_path: path.display().to_string(),
        byte_count: html.len(),
    })
}

pub fn read_visual_fallback(url: &str, sitemap_path: &Path) -> Result<String, String> {
    load_html_fallback(url, sitemap_path)
}

pub fn read_snapshot_report(
    url: &str,
    sitemap_path: &Path,
) -> Result<BrowserSnapshotReadReport, String> {
    let snapshot = read_snapshot(url, sitemap_path)?;
    let html_fallback_path = browser_html_fallback_path(url, sitemap_path);
    Ok(BrowserSnapshotReadReport {
        snapshot: summarize_snapshot(snapshot),
        json_path: browser_snapshot_path(url, sitemap_path)
            .display()
            .to_string(),
        html_fallback_path: html_fallback_path
            .exists()
            .then(|| html_fallback_path.display().to_string()),
    })
}

pub fn summarize_snapshot(snapshot: BrowserPageSnapshot) -> BrowserSnapshotSummary {
    BrowserSnapshotSummary {
        network_summary: summarize_network_activity(&snapshot.protocol_events),
        url: snapshot.url,
        title: snapshot.title,
        element_count: snapshot.elements.len(),
        form_count: snapshot.forms.len(),
        cookie_count: snapshot.cookies.len(),
        request_count: snapshot.requests.len(),
        settle_signal_count: snapshot.settle_signals.len(),
        runtime_state_count: snapshot.runtime_state.len(),
        protocol_event_count: snapshot.protocol_events.len(),
        json_path: None,
    }
}

pub fn summarize_snapshot_diff(diff: &BrowserSnapshotDiff) -> String {
    render_snapshot_diff(diff)
}

pub fn summarize_snapshot_diff_report(
    report: BrowserSnapshotDiffReport,
) -> BrowserSnapshotDiffSummary {
    BrowserSnapshotDiffSummary {
        before_url: report.before_url,
        after_url: report.after_url,
        summary: report.summary,
    }
}

pub fn read_snapshot_diff_report(
    before_url: &str,
    after_url: &str,
    sitemap_path: &Path,
) -> Result<BrowserSnapshotDiffReadReport, String> {
    let report = diff_saved_snapshots(before_url, after_url, sitemap_path)?;
    Ok(BrowserSnapshotDiffReadReport {
        diff: summarize_snapshot_diff_report(report),
        before_json_path: browser_snapshot_path(before_url, sitemap_path)
            .display()
            .to_string(),
        after_json_path: browser_snapshot_path(after_url, sitemap_path)
            .display()
            .to_string(),
    })
}

pub fn diff_saved_snapshots(
    before_url: &str,
    after_url: &str,
    sitemap_path: &Path,
) -> Result<BrowserSnapshotDiffReport, String> {
    let before = load_snapshot_json(before_url, sitemap_path)?;
    let after = load_snapshot_json(after_url, sitemap_path)?;
    let diff = diff_snapshots(&before, &after);
    Ok(BrowserSnapshotDiffReport {
        before_url: before.url,
        after_url: after.url,
        summary: summarize_snapshot_diff(&diff),
        diff,
    })
}

pub fn list_snapshots(
    sitemap_path: &Path,
    url_contains: Option<&str>,
    title_contains: Option<&str>,
    limit: Option<usize>,
    sort_direction: BrowserListSortDirection,
) -> Result<Vec<BrowserSnapshotSummary>, String> {
    let dir = sitemap_path
        .parent()
        .unwrap_or(sitemap_path)
        .join("browser-snapshots");
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|err| format!("read browser snapshot dir: {err}"))? {
        let entry = entry.map_err(|err| format!("read browser snapshot dir entry: {err}"))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read(&path).map_err(|err| format!("read browser snapshot: {err}"))?;
        let snapshot: BrowserPageSnapshot =
            serde_json::from_slice(&raw).map_err(|err| format!("parse browser snapshot: {err}"))?;
        let mut summary = summarize_snapshot(snapshot);
        summary.json_path = Some(path.display().to_string());
        if url_contains
            .map(|needle| contains_case_insensitive(&summary.url, needle))
            .unwrap_or(true)
            && title_contains
                .map(|needle| contains_case_insensitive(&summary.title, needle))
                .unwrap_or(true)
        {
            items.push(summary);
        }
    }
    finalize_list(&mut items, sort_direction, limit, |left, right| {
        left.url.cmp(&right.url)
    });
    Ok(items)
}

fn write_crawl_facts(
    url: &str,
    title: &str,
    summary: &str,
    elements: &[AomElement],
    forms: &[BrowserForm],
    cookies: &[BrowserCookie],
    storage: &[BrowserStorageBucket],
    mutations: &[String],
    requests: &[BrowserRequestRecord],
    settle_signals: &[String],
    runtime_state: &[BrowserRuntimeState],
    protocol_events: &[BrowserProtocolEvent],
    sitemap_path: &Path,
) -> Result<PathBuf, String> {
    let facts_path = crawl_facts_path(url, sitemap_path);
    if let Some(parent) = facts_path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create browser capture dir: {err}"))?;
    }

    let storage_entry_count = storage
        .iter()
        .map(|bucket| bucket.entries.len())
        .sum::<usize>();
    let mut facts = vec![
        "browser-capture version 9".to_string(),
        "field_count 10".to_string(),
        "field\tkind\tpage-crawl".to_string(),
        format!("field\telement_count\t{}", elements.len()),
        format!("field\tform_count\t{}", forms.len()),
        format!("field\tcookie_count\t{}", cookies.len()),
        format!("field\tstorage_entry_count\t{}", storage_entry_count),
        format!("field\tmutation_count\t{}", mutations.len()),
        format!("field\trequest_count\t{}", requests.len()),
        format!("field\tsettle_signal_count\t{}", settle_signals.len()),
        format!("field\truntime_state_count\t{}", runtime_state.len()),
        format!("field\tprotocol_event_count\t{}", protocol_events.len()),
        "page_field_count 3".to_string(),
        format!("page_field\turl\t{}", encode_nda_text(url)),
        format!("page_field\ttitle\t{}", encode_nda_text(title)),
        format!("page_field\tsummary\t{}", encode_nda_text(summary)),
    ];

    for (idx, element) in elements.iter().enumerate() {
        facts.push(format!("element\t{}", idx));
        facts.push(format!(
            "element_field\t{}\trole\t{}",
            idx,
            encode_nda_text(&element.role)
        ));
        facts.push(format!(
            "element_field\t{}\tname\t{}",
            idx,
            encode_nda_text(&element.name)
        ));
        facts.push(format!(
            "element_field\t{}\tvalue\t{}",
            idx,
            encode_nda_text(&element.value)
        ));
        facts.push(format!(
            "element_field\t{}\ttarget_url\t{}",
            idx,
            encode_nda_text(element.target_url.as_deref().unwrap_or("-")),
        ));
    }

    for (form_idx, form) in forms.iter().enumerate() {
        facts.push(format!("form\t{}", form_idx));
        facts.push(format!(
            "form_field\t{}\tid\t{}",
            form_idx,
            encode_nda_text(&form.id)
        ));
        facts.push(format!(
            "form_field\t{}\taction\t{}",
            form_idx,
            encode_nda_text(&form.action)
        ));
        facts.push(format!(
            "form_field\t{}\tmethod\t{}",
            form_idx,
            encode_nda_text(&form.method)
        ));
        if let Some(submit_label) = &form.submit_label {
            facts.push(format!(
                "form_field\t{}\tsubmit_label\t{}",
                form_idx,
                encode_nda_text(submit_label)
            ));
        }
        for (field_idx, field) in form.fields.iter().enumerate() {
            facts.push(format!("form_input\t{}\t{}", form_idx, field_idx));
            facts.push(format!(
                "form_input_field\t{}\t{}\tname\t{}",
                form_idx,
                field_idx,
                encode_nda_text(&field.name)
            ));
            facts.push(format!(
                "form_input_field\t{}\t{}\tlabel\t{}",
                form_idx,
                field_idx,
                encode_nda_text(&field.label)
            ));
            facts.push(format!(
                "form_input_field\t{}\t{}\ttype\t{}",
                form_idx,
                field_idx,
                encode_nda_text(&field.input_type)
            ));
        }
    }

    for (idx, cookie) in cookies.iter().enumerate() {
        facts.push(format!("cookie\t{}", idx));
        facts.push(format!(
            "cookie_field\t{}\tname\t{}",
            idx,
            encode_nda_text(&cookie.name)
        ));
        facts.push(format!(
            "cookie_field\t{}\tvalue\t{}",
            idx,
            encode_nda_text(&cookie.value)
        ));
    }

    for (bucket_idx, bucket) in storage.iter().enumerate() {
        facts.push(format!("storage\t{}", bucket_idx));
        facts.push(format!(
            "storage_field\t{}\tscope\t{}",
            bucket_idx,
            encode_nda_text(&bucket.scope)
        ));
        for (entry_idx, (key, value)) in bucket.entries.iter().enumerate() {
            facts.push(format!("storage_entry\t{}\t{}", bucket_idx, entry_idx));
            facts.push(format!(
                "storage_entry_field\t{}\t{}\tkey\t{}",
                bucket_idx,
                entry_idx,
                encode_nda_text(key)
            ));
            facts.push(format!(
                "storage_entry_field\t{}\t{}\tvalue\t{}",
                bucket_idx,
                entry_idx,
                encode_nda_text(value)
            ));
        }
    }

    for (idx, mutation) in mutations.iter().enumerate() {
        facts.push(format!("mutation\t{}", idx));
        facts.push(format!(
            "mutation_field\t{}\tlabel\t{}",
            idx,
            encode_nda_text(mutation)
        ));
    }

    for (idx, request) in requests.iter().enumerate() {
        facts.push(format!("request\t{}", idx));
        facts.push(format!(
            "request_field\t{}\tmethod\t{}",
            idx,
            encode_nda_text(&request.method)
        ));
        facts.push(format!(
            "request_field\t{}\turl\t{}",
            idx,
            encode_nda_text(&request.url)
        ));
        facts.push(format!(
            "request_field\t{}\tstatus_code\t{}",
            idx, request.status_code
        ));
        facts.push(format!(
            "request_field\t{}\tresource\t{}",
            idx,
            encode_nda_text(&request.resource)
        ));
    }

    for (idx, settle) in settle_signals.iter().enumerate() {
        facts.push(format!("settle_signal\t{}", idx));
        facts.push(format!(
            "settle_signal_field\t{}\tlabel\t{}",
            idx,
            encode_nda_text(settle)
        ));
    }

    for (idx, entry) in runtime_state.iter().enumerate() {
        facts.push(format!("runtime_state\t{}", idx));
        facts.push(format!(
            "runtime_state_field\t{}\tscope\t{}",
            idx,
            encode_nda_text(&entry.scope)
        ));
        facts.push(format!(
            "runtime_state_field\t{}\tkey\t{}",
            idx,
            encode_nda_text(&entry.key)
        ));
        facts.push(format!(
            "runtime_state_field\t{}\tvalue\t{}",
            idx,
            encode_nda_text(&entry.value)
        ));
    }

    for (idx, event) in protocol_events.iter().enumerate() {
        facts.push(format!("protocol_event\t{}", idx));
        facts.push(format!(
            "protocol_event_field\t{}\tkind\t{}",
            idx,
            encode_nda_text(&event.kind)
        ));
        facts.push(format!(
            "protocol_event_field\t{}\tphase\t{}",
            idx,
            encode_nda_text(&event.phase)
        ));
        facts.push(format!(
            "protocol_event_field\t{}\ttarget\t{}",
            idx,
            encode_nda_text(&event.target)
        ));
        facts.push(format!(
            "protocol_event_field\t{}\tdetail\t{}",
            idx,
            encode_nda_text(&event.detail)
        ));
    }

    fs::write(&facts_path, facts.join("\n") + "\n")
        .map_err(|err| format!("write browser capture facts: {err}"))?;
    Ok(facts_path)
}

fn persist_snapshot_to_sitemap(
    snapshot: &BrowserPageSnapshot,
    sitemap_path: &Path,
) -> Result<(), String> {
    let mut sm =
        SiteMap::open(sitemap_path, 0).map_err(|e| format!("Failed to open SiteMap: {:?}", e))?;
    let page_hash = sm
        .register_string(&snapshot.url)
        .map_err(|e| e.to_string())?;
    let title_hash = sm
        .register_string(&snapshot.title)
        .map_err(|e| e.to_string())?;
    let summary_hash = sm
        .register_string(&snapshot.summary)
        .map_err(|e| e.to_string())?;

    let mut live_triples = vec![
        VcTriple {
            subject_hash: page_hash,
            predicate_id: 10,
            object_hash: page_hash,
        },
        VcTriple {
            subject_hash: page_hash,
            predicate_id: 11,
            object_hash: title_hash,
        },
        VcTriple {
            subject_hash: page_hash,
            predicate_id: 12,
            object_hash: summary_hash,
        },
    ];

    for triple in &live_triples {
        sm.put_node(&NdaNode::Triple {
            subject_hash: triple.subject_hash,
            predicate_id: triple.predicate_id,
            object_hash: triple.object_hash,
        })
        .map_err(|e| e.to_string())?;
    }

    let mut aom_node_hashes = Vec::new();
    for el in &snapshot.elements {
        let el_role_hash = sm.register_string(&el.role).map_err(|e| e.to_string())?;
        let el_name_hash = sm.register_string(&el.name).map_err(|e| e.to_string())?;
        let el_val_hash = sm.register_string(&el.value).map_err(|e| e.to_string())?;

        let mut hasher = Sha256::new();
        hasher.update(page_hash.to_le_bytes());
        hasher.update(el.role.as_bytes());
        hasher.update(el.name.as_bytes());
        let digest = hasher.finalize();
        let el_hash = u64::from_le_bytes(digest[0..8].try_into().unwrap());

        for triple in [
            VcTriple {
                subject_hash: el_hash,
                predicate_id: 16,
                object_hash: el_role_hash,
            },
            VcTriple {
                subject_hash: el_hash,
                predicate_id: 17,
                object_hash: el_name_hash,
            },
            VcTriple {
                subject_hash: el_hash,
                predicate_id: 18,
                object_hash: el_val_hash,
            },
        ] {
            sm.put_node(&NdaNode::Triple {
                subject_hash: triple.subject_hash,
                predicate_id: triple.predicate_id,
                object_hash: triple.object_hash,
            })
            .map_err(|e| e.to_string())?;
            live_triples.push(triple);
        }

        if let Some(target) = &el.target_url {
            let target_hash = sm.register_string(target).map_err(|e| e.to_string())?;
            let triple = VcTriple {
                subject_hash: page_hash,
                predicate_id: 1,
                object_hash: target_hash,
            };
            sm.put_node(&NdaNode::Triple {
                subject_hash: triple.subject_hash,
                predicate_id: triple.predicate_id,
                object_hash: triple.object_hash,
            })
            .map_err(|e| e.to_string())?;
            live_triples.push(triple);
        }

        aom_node_hashes.push(el_hash);
    }

    if !aom_node_hashes.is_empty() {
        let aom_root_node = NdaNode::Scope {
            children: aom_node_hashes
                .iter()
                .copied()
                .map(|target| NdaNode::Call { target })
                .collect(),
        };
        let root_hash = sm.put_node(&aom_root_node).map_err(|e| e.to_string())?;
        let triple = VcTriple {
            subject_hash: page_hash,
            predicate_id: 6,
            object_hash: root_hash,
        };
        sm.put_node(&NdaNode::Triple {
            subject_hash: triple.subject_hash,
            predicate_id: triple.predicate_id,
            object_hash: triple.object_hash,
        })
        .map_err(|e| e.to_string())?;
        live_triples.push(triple);
    }

    sm.put_file_snapshot(&format!("browser:{}", snapshot.url), &live_triples)
        .map_err(|e| e.to_string())?;
    sm.flush().map_err(|e| e.to_string())
}

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
        report.network.timeout_ms.map(|value| value.to_string()).unwrap_or_else(|| "default".to_string()),
        report.network.follow_redirects.map(|value| value.to_string()).unwrap_or_else(|| "default".to_string()),
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
        report.network.timeout_ms.map(|value| value.to_string()).unwrap_or_else(|| "default".to_string()),
        report.network.follow_redirects.map(|value| value.to_string()).unwrap_or_else(|| "default".to_string()),
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

fn runtime_cookie_as_browser_cookie(cookie: &RuntimeBrowserCookie) -> BrowserCookie {
    BrowserCookie {
        name: cookie.name.clone(),
        value: cookie.value.clone(),
    }
}

fn browser_cookie_as_runtime_cookie(cookie: &BrowserCookie) -> RuntimeBrowserCookie {
    RuntimeBrowserCookie {
        name: cookie.name.clone(),
        value: cookie.value.clone(),
        ..RuntimeBrowserCookie::default()
    }
}

fn runtime_session_as_browser_session(runtime_session: &RuntimeBrowserSessionState) -> BrowserSessionState {
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

fn build_runtime_auth_diagnostics_report(
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

fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
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

fn append_session_transcript_entry(
    workspace_root: &Path,
    session_id: &str,
    mut entry: BrowserSessionTranscriptEntry,
) -> Result<PathBuf, String> {
    let mut entries = load_session_transcript_entries(workspace_root, session_id)?;
    entry.sequence = entries.last().map(|value| value.sequence + 1).unwrap_or(1);
    entries.push(entry);
    save_session_transcript_entries(workspace_root, session_id, &entries)
}

fn append_session_failure_transcript_entry(
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

fn recent_failed_session_transcript_entries(
    workspace_root: &Path,
    session_id: &str,
    limit: usize,
) -> Result<Vec<BrowserSessionTranscriptEntrySummary>, String> {
    let mut entries = load_session_transcript_entries(workspace_root, session_id)?
        .into_iter()
        .filter(|entry| !entry.outcome.eq_ignore_ascii_case("ok"))
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| right.sequence.cmp(&left.sequence));
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
        let entry = entry.map_err(|err| format!("read browser session dir entry: {err}"))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
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

fn contains_any_case_insensitive(haystack: &str, needles: &[&str]) -> bool {
    needles
        .iter()
        .any(|needle| contains_case_insensitive(haystack, needle))
}

fn is_auth_cookie_name(name: &str) -> bool {
    contains_any_case_insensitive(name, &["session", "auth", "token", "sid", "jwt", "refresh"])
}

fn is_csrf_key(name: &str) -> bool {
    contains_any_case_insensitive(name, &["csrf", "xsrf"])
}

fn summarize_sorted_keys(entries: &HashMap<String, String>) -> Vec<String> {
    let mut keys = entries.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    keys
}

fn filter_auth_cookies(cookies: &[BrowserCookie]) -> Vec<BrowserCookie> {
    cookies
        .iter()
        .filter(|cookie| is_auth_cookie_name(&cookie.name) || is_csrf_key(&cookie.name))
        .cloned()
        .collect()
}

fn merge_runtime_cookie(cookies: &mut Vec<RuntimeBrowserCookie>, cookie: RuntimeBrowserCookie) {
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

fn filter_csrf_storage(entries: &HashMap<String, String>) -> HashMap<String, String> {
    entries
        .iter()
        .filter(|(key, _)| is_csrf_key(key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn snapshot_has_login_form(snapshot: &BrowserPageSnapshot) -> bool {
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

fn snapshot_has_expired_marker(snapshot: &BrowserPageSnapshot) -> bool {
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

fn snapshot_has_access_marker(snapshot: &BrowserPageSnapshot, needles: &[&str]) -> bool {
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

fn snapshot_auth_state(snapshot: &BrowserPageSnapshot) -> Option<String> {
    snapshot
        .runtime_state
        .iter()
        .find(|entry| entry.key.eq_ignore_ascii_case("auth"))
        .map(|entry| entry.value.clone())
}

fn snapshot_router_name(snapshot: &BrowserPageSnapshot) -> Option<String> {
    snapshot
        .runtime_state
        .iter()
        .find(|entry| {
            entry.scope.eq_ignore_ascii_case("router") && entry.key.eq_ignore_ascii_case("name")
        })
        .map(|entry| entry.value.clone())
}

fn collect_auth_signals(
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

fn build_auth_diagnostics_report(
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
        .map(|snapshot| {
            snapshot
                .settle_signals
                .iter()
                .any(|signal| signal.eq_ignore_ascii_case("auth_ready"))
                || auth_state
                    .as_deref()
                    .map(|value| value.eq_ignore_ascii_case("ready"))
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
    let snapshot_json_path = snapshot.as_ref().map(|snapshot| {
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

fn build_access_diagnostics_report(
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
    let snapshot_json_path = snapshot.as_ref().map(|snapshot| {
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

fn html_contains_compatibility_marker(html: &str, needles: &[&str]) -> bool {
    contains_any_case_insensitive(html, needles)
}

fn html_count_case_insensitive(html: &str, needle: &str) -> usize {
    html.to_ascii_lowercase()
        .matches(&needle.to_ascii_lowercase())
        .count()
}

fn build_compatibility_report(
    snapshot: Option<&BrowserPageSnapshot>,
    html_fallback: Option<&str>,
    access_diagnostics: &BrowserAccessDiagnosticsReport,
) -> BrowserCompatibilityReport {
    let mut signals = Vec::new();
    let mut script_count = 0usize;
    let mut canvas_count = 0usize;
    let mut spa_shell = false;
    let mut hydration_markers = false;
    let html_mentions_webgl = html_fallback
        .map(|html| {
            contains_any_case_insensitive(html, &["webgl", "webgpu", "three.js", "babylon", "pixi"])
        })
        .unwrap_or(false);
    let html_mentions_device_features = html_fallback
        .map(|html| {
            contains_any_case_insensitive(
                html,
                &[
                    "navigator.webdriver",
                    "deviceorientation",
                    "pointerlock",
                    "getusermedia",
                    "webauthn",
                    "passkey",
                ],
            )
        })
        .unwrap_or(false);

    if let Some(html) = html_fallback {
        script_count = html_count_case_insensitive(html, "<script");
        canvas_count = html_count_case_insensitive(html, "<canvas");
        spa_shell = html_contains_compatibility_marker(
            html,
            &[
                "id=\"app\"",
                "id='app'",
                "id=\"root\"",
                "id='root'",
                "id=\"__next\"",
                "id='__next'",
                "data-reactroot",
                "ng-version",
            ],
        );
        hydration_markers = html_contains_compatibility_marker(
            html,
            &[
                "hydrate",
                "hydration",
                "hydrateroot",
                "__nuxt",
                "webpack",
                "vite",
                "svelte",
            ],
        );
        signals.push(format!("html:script_tags={script_count}"));
        if canvas_count > 0 {
            signals.push(format!("html:canvas_tags={canvas_count}"));
        }
        if spa_shell {
            signals.push("html:spa_shell".to_string());
        }
        if hydration_markers {
            signals.push("html:hydration_markers".to_string());
        }
        if html_mentions_webgl {
            signals.push("html:webgl_markers".to_string());
        }
        if html_mentions_device_features {
            signals.push("html:device_feature_markers".to_string());
        }
    } else {
        signals.push("html:fallback_missing".to_string());
    }

    let challenge_blocked = access_diagnostics.diagnosis != "clear";
    let anti_bot_limited = matches!(
        access_diagnostics.diagnosis.as_str(),
        "captcha_required" | "anti_bot_challenge" | "rate_limited" | "access_blocked"
    );

    let (level, cause, summary, recommended_action) = match snapshot {
        Some(snapshot) => {
            let actionable_count = snapshot
                .elements
                .iter()
                .filter(|element| describe_element_actionability(element).actionable)
                .count();
            let semantic_element_count = snapshot.elements.len();
            let form_count = snapshot.forms.len();
            let field_count = snapshot
                .forms
                .iter()
                .map(|form| form.fields.len())
                .sum::<usize>();
            let runtime_state_count = snapshot.runtime_state.len();
            let network_summary = summarize_network_activity(&snapshot.protocol_events);
            let live_runtime_count =
                network_summary.event_stream_count + network_summary.websocket_count;
            let runtime_heavy = script_count >= 6
                || spa_shell
                || hydration_markers
                || runtime_state_count >= 4
                || live_runtime_count > 0;
            let canvas_only = (canvas_count > 0 || html_mentions_webgl)
                && semantic_element_count == 0
                && form_count == 0;
            let semantic_surface_missing =
                semantic_element_count == 0 && form_count == 0 && field_count == 0;
            let device_or_identity_limited = html_mentions_device_features || anti_bot_limited;

            signals.push(format!("snapshot:elements={semantic_element_count}"));
            signals.push(format!("snapshot:forms={form_count}"));
            signals.push(format!("snapshot:fields={field_count}"));
            signals.push(format!("snapshot:actionable={actionable_count}"));
            if runtime_state_count > 0 {
                signals.push(format!("runtime:state={runtime_state_count}"));
            }
            if live_runtime_count > 0 {
                signals.push(format!("runtime:live_channels={live_runtime_count}"));
            }
            let frame_count = snapshot
                .runtime_state
                .iter()
                .find(|entry| entry.scope == "runtime_session" && entry.key == "frame_count")
                .and_then(|entry| entry.value.parse::<usize>().ok())
                .unwrap_or(0);
            let shadow_host_count = snapshot
                .runtime_state
                .iter()
                .find(|entry| entry.scope == "runtime_session" && entry.key == "shadow_host_count")
                .and_then(|entry| entry.value.parse::<usize>().ok())
                .unwrap_or(0);
            let runtime_canvas_count = snapshot
                .runtime_state
                .iter()
                .find(|entry| entry.scope == "runtime_session" && entry.key == "canvas_count")
                .and_then(|entry| entry.value.parse::<usize>().ok())
                .unwrap_or(0);
            let runtime_webgl_canvas_count = snapshot
                .runtime_state
                .iter()
                .find(|entry| entry.scope == "runtime_session" && entry.key == "webgl_canvas_count")
                .and_then(|entry| entry.value.parse::<usize>().ok())
                .unwrap_or_else(|| {
                    snapshot
                        .runtime_state
                        .iter()
                        .find(|entry| entry.scope == "runtime_canvas" && entry.key == "webgl_count")
                        .and_then(|entry| entry.value.parse::<usize>().ok())
                        .unwrap_or(0)
                });
            let runtime_canvas_evidence_count = snapshot
                .runtime_state
                .iter()
                .find(|entry| {
                    entry.scope == "runtime_canvas" && entry.key == "runtime_evidence_count"
                })
                .and_then(|entry| entry.value.parse::<usize>().ok())
                .unwrap_or(0);
            let runtime_canvas_animated_count = snapshot
                .runtime_state
                .iter()
                .find(|entry| entry.scope == "runtime_canvas" && entry.key == "animated_count")
                .and_then(|entry| entry.value.parse::<usize>().ok())
                .unwrap_or(0);
            let accessible_frame_count = snapshot
                .runtime_state
                .iter()
                .find(|entry| entry.scope == "runtime_frames" && entry.key == "accessible_count")
                .and_then(|entry| entry.value.parse::<usize>().ok())
                .unwrap_or(0);
            if frame_count > 0 {
                signals.push(format!("runtime:frames={frame_count}"));
                signals.push(format!(
                    "runtime:accessible_frames={accessible_frame_count}"
                ));
            }
            if shadow_host_count > 0 {
                signals.push(format!("runtime:shadow_hosts={shadow_host_count}"));
            }
            if runtime_canvas_count > 0 {
                signals.push(format!("runtime:canvases={runtime_canvas_count}"));
                signals.push(format!(
                    "runtime:webgl_canvases={runtime_webgl_canvas_count}"
                ));
                if runtime_canvas_evidence_count > 0 {
                    signals.push(format!(
                        "runtime:canvas_evidence={runtime_canvas_evidence_count}"
                    ));
                }
                if runtime_canvas_animated_count > 0 {
                    signals.push(format!(
                        "runtime:animated_canvases={runtime_canvas_animated_count}"
                    ));
                }
            }
            let runtime_canvas_only = runtime_canvas_count > 0 && semantic_surface_missing;
            let runtime_canvas_heavy = runtime_canvas_count > 0
                && (runtime_webgl_canvas_count > 0
                    || runtime_canvas_evidence_count > 0
                    || runtime_canvas_animated_count > 0);

            if challenge_blocked {
                (
                    "unsupported".to_string(),
                    "challenge_or_policy_block".to_string(),
                    format!(
                        "The current page is blocked by '{}', so the static browser engine cannot usefully continue.",
                        access_diagnostics.diagnosis
                    ),
                    access_diagnostics.recommended_action.clone(),
                )
            } else if canvas_only || runtime_canvas_only {
                (
                    "unsupported".to_string(),
                    "canvas_or_webgl_surface".to_string(),
                    "The page appears canvas- or WebGL-driven without a usable semantic surface, which the current static browser engine cannot operate reliably.".to_string(),
                    "Escalate to a richer browser/runtime with canvas or WebGL understanding, or use a site path that exposes ordinary semantic controls instead of a drawn surface.".to_string(),
                )
            } else if runtime_canvas_heavy {
                (
                    "runtime_limited".to_string(),
                    "canvas_runtime_surface".to_string(),
                    "Runtime capture found active canvas or WebGL surfaces, but the current semantic snapshot still may not expose the underlying controls reliably.".to_string(),
                    "Prefer runtime-backed capture and verification for the needed flow, and treat canvas or WebGL evidence as a sign to verify each interaction outcome instead of assuming the rendered surface is fully represented semantically.".to_string(),
                )
            } else if semantic_surface_missing && runtime_heavy {
                (
                    "unsupported".to_string(),
                    "runtime_only_surface".to_string(),
                    "The page looks runtime-driven but exposes no usable semantic controls in the persisted snapshot, so it is effectively unsupported by the current static engine.".to_string(),
                    "Escalate to a browser/runtime with real JS execution, or capture the same workflow through a server-rendered or less dynamic route if one exists.".to_string(),
                )
            } else if frame_count > 0 && accessible_frame_count < frame_count {
                (
                    "runtime_limited".to_string(),
                    "cross_origin_embeds".to_string(),
                    "The page includes embedded frames that are not all same-origin or script-accessible, so the current browser evidence can only partially inspect the full surface.".to_string(),
                    "Prefer same-origin routes or richer runtime flows that can explicitly operate the embedded experience; treat inaccessible frames as a hard limit instead of assuming their controls are available.".to_string(),
                )
            } else if shadow_host_count > 0 && semantic_surface_missing {
                (
                    "runtime_limited".to_string(),
                    "shadow_dom_surface".to_string(),
                    "The page appears to rely on shadow-DOM components, and the persisted snapshot surface may still be incomplete even though runtime discovery found shadow hosts.".to_string(),
                    "Use runtime-backed capture/action flows for the current page and verify the needed controls become visible before proceeding; do not assume hidden shadow content is already reflected in the persisted snapshot.".to_string(),
                )
            } else if device_or_identity_limited && runtime_heavy {
                (
                    "runtime_limited".to_string(),
                    "device_or_identity_expectations".to_string(),
                    "The page exposes some semantic structure, but its runtime/device expectations suggest only partial support in the current static browser engine.".to_string(),
                    "Try the visible semantic controls that already exist, but expect degraded support; if progress depends on anti-bot checks, passkeys, media capture, or device APIs, move to a richer browser/runtime.".to_string(),
                )
            } else if runtime_heavy {
                (
                    "runtime_limited".to_string(),
                    "spa_or_live_runtime".to_string(),
                    "The page exposes some semantic structure, but runtime-heavy markers suggest only partial support in the current static browser engine.".to_string(),
                    "Proceed only with currently visible semantic controls, save checkpoints aggressively, and escalate to a richer browser/runtime if the flow depends on JS-driven state transitions or live app updates.".to_string(),
                )
            } else if semantic_element_count > 0 || form_count > 0 || actionable_count > 0 {
                (
                    "supported".to_string(),
                    "semantic_static_surface".to_string(),
                    "The persisted snapshot looks compatible with the current static browser engine.".to_string(),
                    "Proceed with semantic browser actions against the current snapshot and use checkpoints/transcripts for recovery if needed.".to_string(),
                )
            } else {
                (
                    "runtime_limited".to_string(),
                    "sparse_semantic_surface".to_string(),
                    "The page lacks enough semantic structure to confirm reliable support in the current static browser engine.".to_string(),
                    "Refresh or navigate to a more semantic page state if possible; otherwise inspect the HTML fallback and escalate if the required control never appears as a semantic element.".to_string(),
                )
            }
        }
        None => {
            if challenge_blocked {
                (
                    "unsupported".to_string(),
                    "challenge_or_policy_block".to_string(),
                    format!(
                        "No usable snapshot is available and the current page is blocked by '{}'.",
                        access_diagnostics.diagnosis
                    ),
                    access_diagnostics.recommended_action.clone(),
                )
            } else if script_count >= 3
                || canvas_count > 0
                || spa_shell
                || hydration_markers
                || html_mentions_webgl
            {
                (
                    "unsupported".to_string(),
                    "html_only_runtime_surface".to_string(),
                    "Only raw HTML fallback is available, and it looks runtime-, canvas-, or WebGL-driven beyond the current static browser engine.".to_string(),
                    "Escalate to a richer browser/runtime or capture the flow through a server-rendered route; the current engine cannot confirm or operate the needed controls from HTML fallback alone.".to_string(),
                )
            } else if html_fallback.is_some() {
                (
                    "runtime_limited".to_string(),
                    "html_only_without_snapshot".to_string(),
                    "Only raw HTML fallback is available, so compatibility remains limited until a semantic snapshot is captured.".to_string(),
                    "Refresh or re-navigate to rebuild a semantic snapshot before continuing; rely on HTML fallback only for inspection, not as proof that interactions are supported.".to_string(),
                )
            } else {
                (
                    "runtime_limited".to_string(),
                    "missing_browser_evidence".to_string(),
                    "No persisted snapshot or HTML fallback is available to confirm browser compatibility.".to_string(),
                    "Navigate or refresh the session to collect fresh browser evidence before attempting more actions.".to_string(),
                )
            }
        }
    };

    signals.sort();
    signals.dedup();
    BrowserCompatibilityReport {
        level,
        cause,
        summary,
        recommended_action,
        signal_count: signals.len(),
        signals,
    }
}

fn latest_session_checkpoint_summary(
    workspace_root: &Path,
    session_id: &str,
) -> Result<(usize, Option<BrowserSessionCheckpointSummary>), String> {
    let dir = workspace_root
        .join(".velocity")
        .join("browser-session-checkpoints")
        .join(sanitize_file_stem(session_id));
    if !dir.exists() {
        return Ok((0, None));
    }

    let mut checkpoint_count = 0usize;
    let mut latest: Option<(Option<SystemTime>, String, BrowserSessionCheckpointSummary)> = None;
    for entry in fs::read_dir(&dir).map_err(|err| format!("read checkpoint dir: {err}"))? {
        let entry = entry.map_err(|err| format!("read checkpoint dir entry: {err}"))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        checkpoint_count += 1;
        let raw = fs::read(&path).map_err(|err| format!("read browser checkpoint: {err}"))?;
        let checkpoint: BrowserSessionCheckpoint = serde_json::from_slice(&raw)
            .map_err(|err| format!("parse browser checkpoint: {err}"))?;
        let mut summary = summarize_session_checkpoint(checkpoint);
        let path_string = path.display().to_string();
        summary.checkpoint_json_path = Some(path_string.clone());
        let modified = entry
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok());
        let replace = match latest.as_ref() {
            None => true,
            Some((best_modified, best_path, _)) => {
                match (modified.as_ref(), best_modified.as_ref()) {
                    (Some(current), Some(best)) => {
                        current > best || (current == best && path_string > *best_path)
                    }
                    (Some(_), None) => true,
                    (None, Some(_)) => false,
                    (None, None) => path_string > *best_path,
                }
            }
        };
        if replace {
            latest = Some((modified, path_string, summary));
        }
    }

    Ok((checkpoint_count, latest.map(|(_, _, summary)| summary)))
}

pub fn session_health_report(
    workspace_root: &Path,
    session_id: &str,
    sitemap_path: &Path,
) -> Result<BrowserSessionHealthReport, String> {
    let mut session = load_session_state(workspace_root, session_id)?;
    normalize_network_config(&mut session.network);
    let snapshot = match session.current_url.as_deref() {
        Some(url) => load_snapshot_json(url, sitemap_path).ok(),
        None => None,
    };
    let snapshot_json_path = snapshot.as_ref().map(|snapshot| {
        browser_snapshot_path(&snapshot.url, sitemap_path)
            .display()
            .to_string()
    });
    let auth_diagnostics = build_auth_diagnostics_report(
        workspace_root,
        session.clone(),
        snapshot.clone(),
        snapshot_json_path.clone(),
    );
    let access_diagnostics = build_access_diagnostics_report(
        workspace_root,
        session.clone(),
        snapshot.clone(),
        snapshot_json_path.clone(),
    );
    let snapshot_summary = snapshot.as_ref().map(|snapshot| {
        let mut summary = summarize_snapshot(snapshot.clone());
        summary.json_path = snapshot_json_path.clone();
        summary
    });
    let html_fallback_path = session.current_url.as_deref().and_then(|url| {
        let path = browser_html_fallback_path(url, sitemap_path);
        path.exists().then(|| path.display().to_string())
    });
    let html_fallback = session
        .current_url
        .as_deref()
        .and_then(|url| load_html_fallback(url, sitemap_path).ok());
    let compatibility = build_compatibility_report(
        snapshot.as_ref(),
        html_fallback.as_deref(),
        &access_diagnostics,
    );
    let (checkpoint_count, latest_checkpoint) =
        latest_session_checkpoint_summary(workspace_root, session_id)?;
    let session_json_path = session_file_path(workspace_root, session_id)
        .display()
        .to_string();
    let mut session_summary = summarize_session(session.clone());
    session_summary.session_json_path = Some(session_json_path.clone());
    let recent_failures = recent_failed_session_transcript_entries(workspace_root, session_id, 3)?;
    let recent_failure_count = recent_failures.len();
    let latest_failure = recent_failures.first().cloned();

    let failure_recovery = latest_failure.as_ref().map(|failure| {
        let posture = match failure.event_kind.as_str() {
            "save_checkpoint" | "restore_checkpoint" => "recover_checkpoint",
            "navigate" => "recover_navigation",
            "wait" => "recover_wait",
            "click" | "fill_field" | "submit_form" => "recover_interaction",
            _ => "investigate",
        }
        .to_string();
        let action = match failure.event_kind.as_str() {
            "save_checkpoint" => "Retry saving the checkpoint or choose a new checkpoint name after confirming the session state is still available.".to_string(),
            "restore_checkpoint" => "Retry restoring the checkpoint or inspect the saved checkpoint artifact before continuing.".to_string(),
            "navigate" => "Retry navigation with the current network/auth settings, or restore a known-good checkpoint before proceeding.".to_string(),
            "wait" => "Refresh the session state or navigate again before retrying the wait condition with updated evidence.".to_string(),
            "click" | "fill_field" | "submit_form" => "Inspect the current snapshot or HTML fallback, then retry the interaction against visible elements or restore a stable checkpoint.".to_string(),
            _ => format!("Investigate the latest browser failure before continuing: {}", failure.summary),
        };
        (posture, action)
    });

    let (recovery_posture, recommended_action) = if access_diagnostics.diagnosis != "clear" {
        (
            "blocked".to_string(),
            access_diagnostics.recommended_action.clone(),
        )
    } else if matches!(
        auth_diagnostics.diagnosis.as_str(),
        "session_expired" | "csrf_missing" | "login_required"
    ) {
        (
            "recover_auth".to_string(),
            auth_diagnostics.recommended_action.clone(),
        )
    } else if session.current_url.is_none() {
        (
            "seed_session".to_string(),
            "Navigate the session to the target page or apply a saved auth profile before continuing.".to_string(),
        )
    } else if compatibility.level == "unsupported" {
        (
            "unsupported_site".to_string(),
            compatibility.recommended_action.clone(),
        )
    } else if snapshot_summary.is_none() && session.current_url.is_some() {
        (
            "recover_snapshot".to_string(),
            "Refresh the current URL or restore a checkpoint to rebuild persisted page evidence before continuing.".to_string(),
        )
    } else if let Some((posture, action)) = failure_recovery {
        (posture, action)
    } else if compatibility.level == "runtime_limited" {
        (
            "runtime_limited".to_string(),
            compatibility.recommended_action.clone(),
        )
    } else if auth_diagnostics.diagnosis == "auth_ready" {
        (
            "ready".to_string(),
            auth_diagnostics.recommended_action.clone(),
        )
    } else {
        (
            "investigate".to_string(),
            auth_diagnostics.recommended_action.clone(),
        )
    };

    let mut evidence_signals = vec![
        format!("auth:{}", auth_diagnostics.diagnosis),
        format!("access:{}", access_diagnostics.diagnosis),
        format!("compatibility:{}", compatibility.level),
        format!("checkpoints:{}", checkpoint_count),
    ];
    evidence_signals.push(if session.current_url.is_some() {
        "session:url_present".to_string()
    } else {
        "session:url_missing".to_string()
    });
    evidence_signals.push(if snapshot_summary.is_some() {
        "snapshot:available".to_string()
    } else {
        "snapshot:missing".to_string()
    });
    if html_fallback_path.is_some() {
        evidence_signals.push("html_fallback:available".to_string());
    }
    if session.network.user_agent.is_some() {
        evidence_signals.push("network:user_agent_override".to_string());
    }
    if !session.network.headers.is_empty() {
        evidence_signals.push(format!("network:headers={}", session.network.headers.len()));
    }
    if !session.network.allowed_url_prefixes.is_empty()
        || !session.network.blocked_url_prefixes.is_empty()
    {
        evidence_signals.push("network:policy".to_string());
    }
    if let Some(checkpoint) = latest_checkpoint.as_ref() {
        evidence_signals.push(format!("checkpoint:latest={}", checkpoint.name));
    }
    if recent_failure_count > 0 {
        evidence_signals.push(format!("transcript:failures={}", recent_failure_count));
    }
    if let Some(latest_failure) = latest_failure.as_ref() {
        evidence_signals.push(format!(
            "transcript:latest_failure_kind={}",
            latest_failure.event_kind
        ));
    }
    evidence_signals.extend(compatibility.signals.iter().cloned());
    evidence_signals.sort();
    evidence_signals.dedup();
    let evidence_signal_count = evidence_signals.len();

    Ok(BrowserSessionHealthReport {
        session: session_summary,
        network: session.network,
        auth_diagnostics,
        access_diagnostics,
        compatibility,
        snapshot: snapshot_summary,
        html_fallback_path,
        checkpoint_count,
        latest_checkpoint,
        recent_failure_count,
        recent_failures,
        latest_failure,
        recovery_posture,
        recommended_action,
        evidence_signal_count,
        evidence_signals,
        session_json_path,
    })
}

fn resolve_auth_profile_source(
    workspace_root: &Path,
    source_session_id: &str,
    source_checkpoint_name: Option<&str>,
) -> Result<(String, BrowserSessionState), String> {
    let source_kind = if source_checkpoint_name.is_some() {
        "checkpoint".to_string()
    } else {
        "session".to_string()
    };
    let source = if let Some(checkpoint_name) = source_checkpoint_name {
        let checkpoint =
            read_session_checkpoint(workspace_root, source_session_id, checkpoint_name)?;
        checkpoint.session
    } else {
        load_session_state(workspace_root, source_session_id)?
    };
    Ok((source_kind, source))
}

fn build_auth_profile_from_source(
    workspace_root: &Path,
    source_kind: &str,
    profile_name: &str,
    source_session_id: &str,
    source_checkpoint_name: Option<&str>,
    source: BrowserSessionState,
    sitemap_path: &Path,
) -> BrowserAuthProfile {
    let cookies = filter_auth_cookies(&source.cookies);
    let local_storage = filter_csrf_storage(&source.local_storage);
    let session_storage = filter_csrf_storage(&source.session_storage);
    let snapshot = match source.current_url.as_deref() {
        Some(url) => load_snapshot_json(url, sitemap_path).ok(),
        None => None,
    };
    let snapshot_json_path = snapshot.as_ref().map(|snapshot| {
        browser_snapshot_path(&snapshot.url, sitemap_path)
            .display()
            .to_string()
    });
    let auth_diagnostics =
        build_auth_diagnostics_report(workspace_root, source.clone(), snapshot, snapshot_json_path);
    BrowserAuthProfile {
        name: profile_name.to_string(),
        source_kind: source_kind.to_string(),
        source_session_id: source_session_id.to_string(),
        source_checkpoint_name: source_checkpoint_name.map(str::to_string),
        current_url: source.current_url,
        cookies,
        local_storage,
        session_storage,
        auth_diagnostics,
    }
}

fn write_auth_profile(
    workspace_root: &Path,
    profile: &BrowserAuthProfile,
) -> Result<PathBuf, String> {
    let path = browser_auth_profile_json_path(workspace_root, &profile.name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create auth profile dir: {err}"))?;
    }
    let json = serde_json::to_vec_pretty(profile)
        .map_err(|err| format!("serialise auth profile: {err}"))?;
    fs::write(&path, json).map_err(|err| format!("write auth profile: {err}"))?;
    Ok(path)
}

pub fn load_auth_profile(
    workspace_root: &Path,
    profile_name: &str,
) -> Result<BrowserAuthProfile, String> {
    let path = browser_auth_profile_json_path(workspace_root, profile_name);
    let raw = fs::read(&path).map_err(|err| format!("read auth profile: {err}"))?;
    serde_json::from_slice(&raw).map_err(|err| format!("parse auth profile: {err}"))
}

pub fn save_auth_profile_report(
    workspace_root: &Path,
    profile_name: &str,
    source_session_id: &str,
    source_checkpoint_name: Option<&str>,
    sitemap_path: &Path,
) -> Result<BrowserAuthProfileSaveReport, String> {
    let (source_kind, source) =
        resolve_auth_profile_source(workspace_root, source_session_id, source_checkpoint_name)?;
    let profile = build_auth_profile_from_source(
        workspace_root,
        &source_kind,
        profile_name,
        source_session_id,
        source_checkpoint_name,
        source,
        sitemap_path,
    );
    let path = write_auth_profile(workspace_root, &profile)?;
    let mut summary = summarize_auth_profile(profile);
    summary.json_path = Some(path.display().to_string());
    Ok(BrowserAuthProfileSaveReport {
        profile: summary,
        profile_json_path: path.display().to_string(),
    })
}

pub fn read_auth_profile_report(
    workspace_root: &Path,
    profile_name: &str,
) -> Result<BrowserAuthProfileReadReport, String> {
    let profile = load_auth_profile(workspace_root, profile_name)?;
    Ok(BrowserAuthProfileReadReport {
        profile: summarize_auth_profile(profile),
        profile_json_path: browser_auth_profile_json_path(workspace_root, profile_name)
            .display()
            .to_string(),
    })
}

pub fn list_auth_profiles(
    workspace_root: &Path,
    profile_name_contains: Option<&str>,
    source_session_id_contains: Option<&str>,
    limit: Option<usize>,
    sort_direction: BrowserListSortDirection,
) -> Result<Vec<BrowserAuthProfileSummary>, String> {
    let dir = workspace_root
        .join(".velocity")
        .join("browser-auth-profiles");
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|err| format!("read auth profile dir: {err}"))? {
        let entry = entry.map_err(|err| format!("read auth profile dir entry: {err}"))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| !name.ends_with(".auth.json"))
            .unwrap_or(true)
        {
            continue;
        }
        let raw = fs::read(&path).map_err(|err| format!("read auth profile: {err}"))?;
        let profile: BrowserAuthProfile =
            serde_json::from_slice(&raw).map_err(|err| format!("parse auth profile: {err}"))?;
        let mut summary = summarize_auth_profile(profile);
        summary.json_path = Some(path.display().to_string());
        if profile_name_contains
            .map(|needle| contains_case_insensitive(&summary.name, needle))
            .unwrap_or(true)
            && source_session_id_contains
                .map(|needle| contains_case_insensitive(&summary.source_session_id, needle))
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

pub fn apply_auth_profile_report(
    workspace_root: &Path,
    profile_name: &str,
    target_session_id: &str,
    sitemap_path: &Path,
) -> Result<BrowserAuthProfileApplyReport, String> {
    let profile = load_auth_profile(workspace_root, profile_name)?;
    let profile_json_path = browser_auth_profile_json_path(workspace_root, profile_name)
        .display()
        .to_string();
    let mut target = load_session_state(workspace_root, target_session_id)?;

    for cookie in profile.cookies.iter().cloned() {
        merge_cookie(&mut target.cookies, cookie);
    }
    apply_storage_updates(&mut target.local_storage, &profile.local_storage);
    apply_storage_updates(&mut target.session_storage, &profile.session_storage);

    let session_path = save_session_state(workspace_root, &target)?;
    let snapshot = match target.current_url.as_deref() {
        Some(url) => load_snapshot_json(url, sitemap_path).ok(),
        None => None,
    };
    let snapshot_json_path = snapshot.as_ref().map(|snapshot| {
        browser_snapshot_path(&snapshot.url, sitemap_path)
            .display()
            .to_string()
    });
    let auth_diagnostics =
        build_auth_diagnostics_report(workspace_root, target.clone(), snapshot, snapshot_json_path);

    Ok(BrowserAuthProfileApplyReport {
        profile_name: profile.name,
        target_session: summarize_session(target),
        copied_cookie_count: profile.cookies.len(),
        copied_cookie_names: summarize_cookie_names(&profile.cookies),
        copied_local_storage_count: profile.local_storage.len(),
        copied_local_storage_keys: summarize_sorted_keys(&profile.local_storage),
        copied_session_storage_count: profile.session_storage.len(),
        copied_session_storage_keys: summarize_sorted_keys(&profile.session_storage),
        session_json_path: session_path.display().to_string(),
        profile_json_path,
        auth_diagnostics,
    })
}

pub fn reseed_auth_state_report(
    workspace_root: &Path,
    target_session_id: &str,
    source_session_id: &str,
    source_checkpoint_name: Option<&str>,
    sitemap_path: &Path,
) -> Result<BrowserAuthReseedReport, String> {
    let (source_kind, source) =
        resolve_auth_profile_source(workspace_root, source_session_id, source_checkpoint_name)?;
    let copied_cookies = filter_auth_cookies(&source.cookies);
    let copied_local_storage = filter_csrf_storage(&source.local_storage);
    let copied_session_storage = filter_csrf_storage(&source.session_storage);
    let mut target = load_session_state(workspace_root, target_session_id)?;

    for cookie in copied_cookies.iter().cloned() {
        merge_cookie(&mut target.cookies, cookie);
    }
    apply_storage_updates(&mut target.local_storage, &copied_local_storage);
    apply_storage_updates(&mut target.session_storage, &copied_session_storage);

    let session_path = save_session_state(workspace_root, &target)?;
    let snapshot = match target.current_url.as_deref() {
        Some(url) => load_snapshot_json(url, sitemap_path).ok(),
        None => None,
    };
    let snapshot_json_path = snapshot.as_ref().map(|snapshot| {
        browser_snapshot_path(&snapshot.url, sitemap_path)
            .display()
            .to_string()
    });
    let auth_diagnostics =
        build_auth_diagnostics_report(workspace_root, target.clone(), snapshot, snapshot_json_path);

    Ok(BrowserAuthReseedReport {
        target_session: summarize_session(target),
        source_kind,
        source_session_id: source_session_id.to_string(),
        source_checkpoint_name: source_checkpoint_name.map(str::to_string),
        copied_cookie_count: copied_cookies.len(),
        copied_cookie_names: summarize_cookie_names(&copied_cookies),
        copied_local_storage_count: copied_local_storage.len(),
        copied_local_storage_keys: summarize_sorted_keys(&copied_local_storage),
        copied_session_storage_count: copied_session_storage.len(),
        copied_session_storage_keys: summarize_sorted_keys(&copied_session_storage),
        session_json_path: session_path.display().to_string(),
        auth_diagnostics,
    })
}

pub fn reseed_runtime_auth_state_report(
    workspace_root: &Path,
    target_session_id: &str,
    source_session_id: &str,
    source_checkpoint_name: Option<&str>,
    sitemap_path: &Path,
    wait_timeout_ms: Option<u64>,
) -> Result<RuntimeAuthReseedReport, String> {
    let (source_kind, source) =
        resolve_auth_profile_source(workspace_root, source_session_id, source_checkpoint_name)?;
    let copied_cookies = filter_auth_cookies(&source.cookies);
    let copied_runtime_cookies = auth_runtime_cookies_for_source(&source);
    let copied_local_storage = filter_csrf_storage(&source.local_storage);
    let copied_session_storage = filter_csrf_storage(&source.session_storage);
    let mut target = load_runtime_session_state(workspace_root, target_session_id)?;
    let request_body = serde_json::json!({
        "url": target.current_url,
        "cookies": copied_runtime_cookies,
        "localStorage": copied_local_storage,
        "sessionStorage": copied_session_storage,
        "waitTimeoutMs": wait_timeout_ms.unwrap_or(1_000),
    });
    let value = runtime_api_request(
        "POST",
        &format!(
            "{}/api/runtime/session/{}/state",
            target.api_base, target.runtime_session_id
        ),
        Some(&request_body),
    )?;
    let warnings = parse_runtime_string_list(value.get("warnings"));

    for cookie in auth_runtime_cookies_for_source(&source) {
        merge_runtime_cookie(&mut target.cookies, cookie);
    }
    apply_storage_updates(&mut target.local_storage, &copied_local_storage);
    apply_storage_updates(&mut target.session_storage, &copied_session_storage);

    let session_path = save_runtime_session_state(workspace_root, &target)?;
    let auth_diagnostics = build_runtime_auth_diagnostics_report(workspace_root, &target, sitemap_path);

    Ok(RuntimeAuthReseedReport {
        target_runtime_session: target,
        source_kind,
        source_session_id: source_session_id.to_string(),
        source_checkpoint_name: source_checkpoint_name.map(str::to_string),
        copied_cookie_count: copied_cookies.len(),
        copied_cookie_names: summarize_cookie_names(&copied_cookies),
        copied_local_storage_count: copied_local_storage.len(),
        copied_local_storage_keys: summarize_sorted_keys(&copied_local_storage),
        copied_session_storage_count: copied_session_storage.len(),
        copied_session_storage_keys: summarize_sorted_keys(&copied_session_storage),
        session_json_path: session_path.display().to_string(),
        auth_diagnostics,
        warning_count: warnings.len(),
        warnings,
    })
}

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

fn persist_checkpoint_from_replay_state(
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
        let entry = entry.map_err(|err| format!("read checkpoint dir entry: {err}"))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
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

fn describe_url_resolution(requested_url: &str, resolved_url: &str) -> String {
    if requested_url == resolved_url {
        format!("URL: {}", resolved_url)
    } else {
        format!(
            "Requested URL: {}\nResolved URL: {}",
            requested_url, resolved_url
        )
    }
}

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

fn load_session_replay_state(
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

fn persist_runtime_capture_artifacts(
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
    for (name, value) in &captured.fields {
        let field_name: &str = name.trim();
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
                value: (value as &String).clone(),
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
                    .any(|kind| kind.starts_with("webgl"))
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
        html_fallback_path: html_fallback_path.map(|path| path.display().to_string()),
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

fn browser_runtime_capture_report_internal(
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
    let (snapshot_path, session_path, facts_path, html_fallback_path) =
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
        html_fallback_path: html_fallback_path.map(|path| path.display().to_string()),
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
        html_fallback_path: html_fallback_path.map(|path| path.display().to_string()),
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

pub fn crawl_page_snapshot(url: &str) -> Result<BrowserPageSnapshot, String> {
    let mut session = BrowserSessionState {
        id: "ephemeral".to_string(),
        current_url: Some(url.to_string()),
        cookies: Vec::new(),
        runtime_cookies: Vec::new(),
        local_storage: HashMap::new(),
        session_storage: HashMap::new(),
        network: BrowserSessionNetworkConfig::default(),
        last_html: None,
    };
    crawl_page_snapshot_with_session(&mut session, url)
}

pub fn crawl_page_snapshot_with_session(
    session: &mut BrowserSessionState,
    url: &str,
) -> Result<BrowserPageSnapshot, String> {
    let response = fetch_with_session(url, "GET", None, &session.cookies, &session.network)?;
    for cookie in response.cookies.iter().cloned() {
        merge_cookie(&mut session.cookies, cookie);
    }
    apply_storage_updates(&mut session.local_storage, &response.local_storage_updates);
    apply_storage_updates(
        &mut session.session_storage,
        &response.session_storage_updates,
    );
    session.current_url = Some(url.to_string());
    let storage = storage_buckets(session);
    session.current_url = Some(response.final_url.clone());
    session.last_html = Some(response.html.clone());
    Ok(parse_html_to_snapshot_with_runtime_state(
        &response.final_url,
        &response.html,
        &session.cookies,
        &storage,
        &response.mutations,
        &response.requests,
        &response.settle_signals,
        &response.runtime_state,
        &response.protocol_events,
    ))
}

fn url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", byte)),
        }
    }
    out
}

fn normalize_match_text(value: &str) -> String {
    value
        .split_whitespace()
        .filter(|segment| !segment.is_empty())
        .map(|segment| segment.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

fn string_match_score(haystack: &str, needle: &str) -> Option<i32> {
    let haystack = normalize_match_text(haystack);
    let needle = normalize_match_text(needle);
    if needle.is_empty() {
        return Some(1);
    }
    if haystack.is_empty() {
        return None;
    }
    if haystack == needle {
        return Some(1_000);
    }
    if haystack.starts_with(&needle) {
        return Some(700);
    }
    if haystack.contains(&needle) {
        return Some(500);
    }

    let needle_terms = needle.split(' ').collect::<Vec<_>>();
    let mut matched_terms = 0;
    for term in &needle_terms {
        if haystack.contains(term) {
            matched_terms += 1;
        }
    }
    if matched_terms == 0 {
        return None;
    }
    Some(200 + matched_terms * 40 - (haystack.len() as i32 - needle.len() as i32).abs().min(120))
}

fn role_actionability(role: &str) -> u8 {
    match role.to_ascii_lowercase().as_str() {
        "link" => 80,
        "button" => 70,
        "textbox" => 40,
        _ => 10,
    }
}

fn element_actionability_score(element: &AomElement) -> i32 {
    let mut score =
        i32::from(role_actionability(&element.role)).max(i32::from(element.actionability));
    if element.target_url.is_some() {
        score += 30;
    }
    if !element.value.is_empty() {
        score += 5;
    }
    score.clamp(0, 255)
}

fn describe_element_actionability(element: &AomElement) -> BrowserTargetActionability {
    let score = element_actionability_score(element) as u8;
    let actionable = match element.role.to_ascii_lowercase().as_str() {
        "link" => element.target_url.is_some(),
        "button" => element.target_url.is_some(),
        "textbox" => score >= 40,
        _ => score >= 40,
    };
    let reason = match element.role.to_ascii_lowercase().as_str() {
        "link" if element.target_url.is_none() => {
            "matched link lacks a navigable target in the current static browser engine".to_string()
        }
        "button" if element.target_url.is_none() => {
            "matched button has no navigable target; use browser_session_submit for forms or a richer runtime for JS buttons".to_string()
        }
        "textbox" if score < 40 => "matched textbox is present but weakly actionable".to_string(),
        _ if actionable => "semantic target is actionable in the current browser model".to_string(),
        _ => "semantic target is present but not actionable in the current browser model".to_string(),
    };
    BrowserTargetActionability {
        kind: "element".to_string(),
        role: element.role.clone(),
        name: element.name.clone(),
        score,
        actionable,
        reason,
        supported_actions: element.supported_actions.clone(),
        provenance: element.provenance.clone(),
        target_url: element.target_url.clone(),
    }
}

fn describe_form_field_actionability(field: &BrowserFormField) -> BrowserTargetActionability {
    let hidden = field.input_type.eq_ignore_ascii_case("hidden");
    BrowserTargetActionability {
        kind: "form_field".to_string(),
        role: field.input_type.clone(),
        name: if field.label.is_empty() {
            field.name.clone()
        } else {
            field.label.clone()
        },
        score: if hidden { 0 } else { 80 },
        actionable: !hidden,
        reason: if hidden {
            "matched form field is hidden and not actionable for browser_session_fill".to_string()
        } else {
            "form field is actionable in the current browser model".to_string()
        },
        supported_actions: vec!["fill".to_string()],
        provenance: "native".to_string(),
        target_url: None,
    }
}

fn describe_form_actionability(form: &BrowserForm) -> BrowserTargetActionability {
    let actionable = !form.action.trim().is_empty();
    BrowserTargetActionability {
        kind: "form".to_string(),
        role: form.method.clone(),
        name: if form.id.is_empty() {
            "default".to_string()
        } else {
            form.id.clone()
        },
        score: if actionable { 75 } else { 35 },
        actionable,
        reason: if actionable {
            "form submit target is actionable in the current browser model".to_string()
        } else {
            "matched form has no explicit action URL, which this static browser engine cannot safely infer".to_string()
        },
        supported_actions: vec!["submit".to_string()],
        provenance: "native".to_string(),
        target_url: if actionable {
            Some(form.action.clone())
        } else {
            None
        },
    }
}

fn element_match_score(element: &AomElement, role: &str, name: &str) -> Option<i32> {
    if !element.role.eq_ignore_ascii_case(role) {
        return None;
    }

    let mut score = element_actionability_score(element);
    if let Some(name_score) = string_match_score(&element.name, name) {
        score += name_score;
    } else {
        let value_score = string_match_score(&element.value, name);
        let target_score = element
            .target_url
            .as_deref()
            .and_then(|target| string_match_score(target, name));
        score += value_score.max(target_score).unwrap_or_default();
        if value_score.is_none() && target_score.is_none() {
            return None;
        }
    }

    if let Some(target_url) = element.target_url.as_deref() {
        if let Some(target_score) = string_match_score(target_url, name) {
            score += target_score / 4;
        }
    }
    Some(score)
}

fn form_field_match_score(field: &BrowserFormField, field_name: &str) -> Option<i32> {
    let label_score = string_match_score(&field.label, field_name);
    let name_score = string_match_score(&field.name, field_name);
    let value_score = string_match_score(&field.value, field_name);
    let best = label_score.max(name_score).max(value_score)?;
    Some(
        best + if field.input_type.eq_ignore_ascii_case("hidden") {
            0
        } else {
            25
        },
    )
}

fn find_element<'a>(
    snapshot: &'a BrowserPageSnapshot,
    role: &str,
    name: &str,
) -> Option<&'a AomElement> {
    snapshot
        .elements
        .iter()
        .filter_map(|element| {
            element_match_score(element, role, name).map(|score| (score, element))
        })
        .max_by(|(left_score, left_element), (right_score, right_element)| {
            left_score
                .cmp(right_score)
                .then_with(|| right_element.name.len().cmp(&left_element.name.len()))
        })
        .map(|(_, element)| element)
}

fn find_form<'a>(
    snapshot: &'a BrowserPageSnapshot,
    form_id: Option<&str>,
) -> Option<&'a BrowserForm> {
    match form_id {
        Some(id) => snapshot
            .forms
            .iter()
            .find(|form| form.id.eq_ignore_ascii_case(id)),
        None => snapshot.forms.first(),
    }
}

fn find_form_field<'a>(
    snapshot: &'a BrowserPageSnapshot,
    field_name: &str,
) -> Option<&'a BrowserFormField> {
    snapshot
        .forms
        .iter()
        .flat_map(|form| form.fields.iter())
        .filter_map(|field| form_field_match_score(field, field_name).map(|score| (score, field)))
        .max_by(|(left_score, left_field), (right_score, right_field)| {
            left_score
                .cmp(right_score)
                .then_with(|| right_field.label.len().cmp(&left_field.label.len()))
        })
        .map(|(_, field)| field)
}

fn find_textbox_element<'a>(
    snapshot: &'a BrowserPageSnapshot,
    field_name: &str,
) -> Option<&'a AomElement> {
    snapshot
        .elements
        .iter()
        .filter(|element| element.role.eq_ignore_ascii_case("textbox"))
        .filter_map(|element| {
            string_match_score(&element.name, field_name).map(|score| (score, element))
        })
        .max_by(|(left_score, left_element), (right_score, right_element)| {
            left_score
                .cmp(right_score)
                .then_with(|| right_element.name.len().cmp(&left_element.name.len()))
        })
        .map(|(_, element)| element)
}

fn snapshot_contains_text(snapshot: &BrowserPageSnapshot, needle: &str) -> bool {
    let needle = needle.to_ascii_lowercase();
    snapshot.title.to_ascii_lowercase().contains(&needle)
        || snapshot.summary.to_ascii_lowercase().contains(&needle)
        || snapshot.forms.iter().any(|form| {
            form.id.to_ascii_lowercase().contains(&needle)
                || form
                    .submit_label
                    .as_deref()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .contains(&needle)
                || form.fields.iter().any(|field| {
                    field.label.to_ascii_lowercase().contains(&needle)
                        || field.name.to_ascii_lowercase().contains(&needle)
                        || field.value.to_ascii_lowercase().contains(&needle)
                })
        })
        || snapshot.elements.iter().any(|element| {
            element.name.to_ascii_lowercase().contains(&needle)
                || element.value.to_ascii_lowercase().contains(&needle)
                || element
                    .target_url
                    .as_deref()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .contains(&needle)
        })
}

fn element_signature(element: &AomElement) -> String {
    format!(
        "{}:{}:{}:{}",
        element.role,
        element.name,
        element.value,
        element.target_url.as_deref().unwrap_or_default()
    )
}

fn form_signature(form: &BrowserForm) -> String {
    format!("{}:{}:{}", form.id, form.method, form.action)
}

fn cookie_signature(cookie: &BrowserCookie) -> String {
    format!("{}={}", cookie.name, cookie.value)
}

fn snapshot_storage_signatures(snapshot: &BrowserPageSnapshot) -> HashSet<String> {
    snapshot
        .storage
        .iter()
        .flat_map(storage_signature)
        .collect::<HashSet<_>>()
}

fn snapshot_mutation_signatures(snapshot: &BrowserPageSnapshot) -> HashSet<String> {
    snapshot.mutations.iter().cloned().collect::<HashSet<_>>()
}

fn request_signature(request: &BrowserRequestRecord) -> String {
    format!(
        "{}:{}:{}:{}",
        request.method, request.url, request.status_code, request.resource
    )
}

fn snapshot_request_signatures(snapshot: &BrowserPageSnapshot) -> HashSet<String> {
    snapshot
        .requests
        .iter()
        .map(request_signature)
        .collect::<HashSet<_>>()
}

fn snapshot_settle_signatures(snapshot: &BrowserPageSnapshot) -> HashSet<String> {
    snapshot
        .settle_signals
        .iter()
        .cloned()
        .collect::<HashSet<_>>()
}

fn runtime_state_signature(entry: &BrowserRuntimeState) -> String {
    format!("{}:{}={}", entry.scope, entry.key, entry.value)
}

fn snapshot_runtime_state_signatures(snapshot: &BrowserPageSnapshot) -> HashSet<String> {
    snapshot
        .runtime_state
        .iter()
        .map(runtime_state_signature)
        .collect::<HashSet<_>>()
}

fn snapshot_protocol_event_signatures(snapshot: &BrowserPageSnapshot) -> HashSet<String> {
    snapshot
        .protocol_events
        .iter()
        .map(protocol_event_signature)
        .collect::<HashSet<_>>()
}

pub fn diff_snapshots(
    before: &BrowserPageSnapshot,
    after: &BrowserPageSnapshot,
) -> BrowserSnapshotDiff {
    let before_elements = before
        .elements
        .iter()
        .map(element_signature)
        .collect::<HashSet<_>>();
    let after_elements = after
        .elements
        .iter()
        .map(element_signature)
        .collect::<HashSet<_>>();
    let before_forms = before
        .forms
        .iter()
        .map(form_signature)
        .collect::<HashSet<_>>();
    let after_forms = after
        .forms
        .iter()
        .map(form_signature)
        .collect::<HashSet<_>>();
    let before_cookies = before
        .cookies
        .iter()
        .map(cookie_signature)
        .collect::<HashSet<_>>();
    let after_cookies = after
        .cookies
        .iter()
        .map(cookie_signature)
        .collect::<HashSet<_>>();
    let before_storage = snapshot_storage_signatures(before);
    let after_storage = snapshot_storage_signatures(after);
    let before_mutations = snapshot_mutation_signatures(before);
    let after_mutations = snapshot_mutation_signatures(after);
    let before_requests = snapshot_request_signatures(before);
    let after_requests = snapshot_request_signatures(after);
    let before_settle_signals = snapshot_settle_signatures(before);
    let after_settle_signals = snapshot_settle_signatures(after);
    let before_runtime_state = snapshot_runtime_state_signatures(before);
    let after_runtime_state = snapshot_runtime_state_signatures(after);
    let before_protocol_events = snapshot_protocol_event_signatures(before);
    let after_protocol_events = snapshot_protocol_event_signatures(after);

    let mut added_elements = after_elements
        .difference(&before_elements)
        .cloned()
        .collect::<Vec<_>>();
    let mut removed_elements = before_elements
        .difference(&after_elements)
        .cloned()
        .collect::<Vec<_>>();
    let mut added_forms = after_forms
        .difference(&before_forms)
        .cloned()
        .collect::<Vec<_>>();
    let mut removed_forms = before_forms
        .difference(&after_forms)
        .cloned()
        .collect::<Vec<_>>();
    let mut added_cookies = after_cookies
        .difference(&before_cookies)
        .cloned()
        .collect::<Vec<_>>();
    let mut removed_cookies = before_cookies
        .difference(&after_cookies)
        .cloned()
        .collect::<Vec<_>>();
    let mut added_storage = after_storage
        .difference(&before_storage)
        .cloned()
        .collect::<Vec<_>>();
    let mut removed_storage = before_storage
        .difference(&after_storage)
        .cloned()
        .collect::<Vec<_>>();
    let mut added_mutations = after_mutations
        .difference(&before_mutations)
        .cloned()
        .collect::<Vec<_>>();
    let mut removed_mutations = before_mutations
        .difference(&after_mutations)
        .cloned()
        .collect::<Vec<_>>();
    let mut added_requests = after_requests
        .difference(&before_requests)
        .cloned()
        .collect::<Vec<_>>();
    let mut removed_requests = before_requests
        .difference(&after_requests)
        .cloned()
        .collect::<Vec<_>>();
    let mut added_settle_signals = after_settle_signals
        .difference(&before_settle_signals)
        .cloned()
        .collect::<Vec<_>>();
    let mut removed_settle_signals = before_settle_signals
        .difference(&after_settle_signals)
        .cloned()
        .collect::<Vec<_>>();
    let mut added_runtime_state = after_runtime_state
        .difference(&before_runtime_state)
        .cloned()
        .collect::<Vec<_>>();
    let mut removed_runtime_state = before_runtime_state
        .difference(&after_runtime_state)
        .cloned()
        .collect::<Vec<_>>();
    let mut added_protocol_events = after_protocol_events
        .difference(&before_protocol_events)
        .cloned()
        .collect::<Vec<_>>();
    let mut removed_protocol_events = before_protocol_events
        .difference(&after_protocol_events)
        .cloned()
        .collect::<Vec<_>>();

    added_elements.sort();
    removed_elements.sort();
    added_forms.sort();
    removed_forms.sort();
    added_cookies.sort();
    removed_cookies.sort();
    added_storage.sort();
    removed_storage.sort();
    added_mutations.sort();
    removed_mutations.sort();
    added_requests.sort();
    removed_requests.sort();
    added_settle_signals.sort();
    removed_settle_signals.sort();
    added_runtime_state.sort();
    removed_runtime_state.sort();
    added_protocol_events.sort();
    removed_protocol_events.sort();

    BrowserSnapshotDiff {
        title_changed: before.title != after.title,
        summary_changed: before.summary != after.summary,
        added_elements,
        removed_elements,
        added_forms,
        removed_forms,
        added_cookies,
        removed_cookies,
        added_storage,
        removed_storage,
        added_mutations,
        removed_mutations,
        added_requests,
        removed_requests,
        added_settle_signals,
        removed_settle_signals,
        added_runtime_state,
        removed_runtime_state,
        added_protocol_events,
        removed_protocol_events,
    }
}

fn render_snapshot_diff(diff: &BrowserSnapshotDiff) -> String {
    let mut parts = Vec::new();
    if diff.title_changed {
        parts.push("title".to_string());
    }
    if diff.summary_changed {
        parts.push("summary".to_string());
    }
    if !diff.added_elements.is_empty() {
        parts.push(format!("elements+{}", diff.added_elements.len()));
    }
    if !diff.removed_elements.is_empty() {
        parts.push(format!("elements-{}", diff.removed_elements.len()));
    }
    if !diff.added_forms.is_empty() {
        parts.push(format!("forms+{}", diff.added_forms.len()));
    }
    if !diff.removed_forms.is_empty() {
        parts.push(format!("forms-{}", diff.removed_forms.len()));
    }
    if !diff.added_cookies.is_empty() {
        parts.push(format!("cookies+{}", diff.added_cookies.len()));
    }
    if !diff.removed_cookies.is_empty() {
        parts.push(format!("cookies-{}", diff.removed_cookies.len()));
    }
    if !diff.added_storage.is_empty() {
        parts.push(format!("storage+{}", diff.added_storage.len()));
    }
    if !diff.removed_storage.is_empty() {
        parts.push(format!("storage-{}", diff.removed_storage.len()));
    }
    if !diff.added_mutations.is_empty() {
        parts.push(format!("mutations+{}", diff.added_mutations.len()));
    }
    if !diff.removed_mutations.is_empty() {
        parts.push(format!("mutations-{}", diff.removed_mutations.len()));
    }
    if !diff.added_requests.is_empty() {
        parts.push(format!("requests+{}", diff.added_requests.len()));
    }
    if !diff.removed_requests.is_empty() {
        parts.push(format!("requests-{}", diff.removed_requests.len()));
    }
    if !diff.added_settle_signals.is_empty() {
        parts.push(format!("settle+{}", diff.added_settle_signals.len()));
    }
    if !diff.removed_settle_signals.is_empty() {
        parts.push(format!("settle-{}", diff.removed_settle_signals.len()));
    }
    if !diff.added_runtime_state.is_empty() {
        parts.push(format!("runtime+{}", diff.added_runtime_state.len()));
    }
    if !diff.removed_runtime_state.is_empty() {
        parts.push(format!("runtime-{}", diff.removed_runtime_state.len()));
    }
    if !diff.added_protocol_events.is_empty() {
        parts.push(format!("protocol+{}", diff.added_protocol_events.len()));
    }
    if !diff.removed_protocol_events.is_empty() {
        parts.push(format!("protocol-{}", diff.removed_protocol_events.len()));
    }

    if parts.is_empty() {
        "no_semantic_change".to_string()
    } else {
        parts.join(",")
    }
}

fn is_semantically_stable(diff: &BrowserSnapshotDiff) -> bool {
    !diff.title_changed
        && !diff.summary_changed
        && diff.added_elements.is_empty()
        && diff.removed_elements.is_empty()
        && diff.added_forms.is_empty()
        && diff.removed_forms.is_empty()
        && diff.added_cookies.is_empty()
        && diff.removed_cookies.is_empty()
        && diff.added_storage.is_empty()
        && diff.removed_storage.is_empty()
        && diff.added_mutations.is_empty()
        && diff.removed_mutations.is_empty()
        && diff.added_requests.is_empty()
        && diff.removed_requests.is_empty()
        && diff.added_settle_signals.is_empty()
        && diff.removed_settle_signals.is_empty()
        && diff.added_runtime_state.is_empty()
        && diff.removed_runtime_state.is_empty()
        && diff.added_protocol_events.is_empty()
        && diff.removed_protocol_events.is_empty()
}

fn wait_for_condition<F>(
    session: &mut BrowserSessionState,
    current_snapshot: &mut BrowserPageSnapshot,
    timeout_ms: Option<u64>,
    interval_ms: Option<u64>,
    mut predicate: F,
) -> Result<BrowserSnapshotDiff, String>
where
    F: FnMut(&BrowserPageSnapshot) -> bool,
{
    if predicate(current_snapshot) {
        return Ok(diff_snapshots(current_snapshot, current_snapshot));
    }

    let timeout = Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS));
    let interval = Duration::from_millis(interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS));
    let started = Instant::now();
    let original = current_snapshot.clone();

    loop {
        if started.elapsed() >= timeout {
            return Err(format!(
                "wait condition not satisfied within {}ms",
                timeout.as_millis()
            ));
        }
        thread::sleep(interval);
        let url = session
            .current_url
            .clone()
            .unwrap_or_else(|| current_snapshot.url.clone());
        let refreshed = crawl_page_snapshot_with_session(session, &url)?;
        if predicate(&refreshed) {
            let diff = diff_snapshots(&original, &refreshed);
            *current_snapshot = refreshed;
            return Ok(diff);
        }
    }
}

fn wait_for_stable_snapshot(
    session: &mut BrowserSessionState,
    current_snapshot: &mut BrowserPageSnapshot,
    stable_polls: Option<u32>,
    timeout_ms: Option<u64>,
    interval_ms: Option<u64>,
) -> Result<BrowserSnapshotDiff, String> {
    let required_stable = stable_polls.unwrap_or(DEFAULT_STABLE_POLLS).max(1);
    let timeout = Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS));
    let interval = Duration::from_millis(interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS));
    let started = Instant::now();
    let original = current_snapshot.clone();
    let mut previous = current_snapshot.clone();
    let mut consecutive_stable = 0u32;

    loop {
        if started.elapsed() >= timeout {
            return Err(format!(
                "wait for stable snapshot not satisfied within {}ms",
                timeout.as_millis()
            ));
        }
        thread::sleep(interval);
        let url = session
            .current_url
            .clone()
            .unwrap_or_else(|| current_snapshot.url.clone());
        let refreshed = crawl_page_snapshot_with_session(session, &url)?;
        let poll_diff = diff_snapshots(&previous, &refreshed);
        if is_semantically_stable(&poll_diff) {
            consecutive_stable += 1;
        } else {
            consecutive_stable = 0;
        }
        previous = refreshed.clone();
        if consecutive_stable >= required_stable {
            let final_diff = diff_snapshots(&original, &refreshed);
            *current_snapshot = refreshed;
            return Ok(final_diff);
        }
    }
}

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
                    .any(|entry| entry.to_ascii_lowercase().contains(&lowered))
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
        html_fallback_path: html_fallback_path.map(|path| path.display().to_string()),
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

fn apply_fill_field(
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

fn submit_current_form(
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

pub fn render_web_navigate_report(report: &BrowserWebNavigateReport) -> String {
    let network_summary = render_network_summary(&report.snapshot.network_summary)
        .map(|value| format!("\nNetwork summary: {}", value))
        .unwrap_or_default();
    let html_fallback = render_html_fallback_line(report.html_fallback_path.as_deref());
    format!(
        "Crawler finished.\nURL: {}\nTitle: {}\nInteractive Elements: {}\nForms: {}\nCookies: {}\nRequests: {}\nSettle signals: {}\nRuntime state: {}\nProtocol events: {}{}\nRegistered in Merkle SiteMap at {}\nSnapshot JSON: {}{}\nNDA Facts: {}",
        report.snapshot.url,
        report.snapshot.title,
        report.snapshot.element_count,
        report.snapshot.form_count,
        report.snapshot.cookie_count,
        report.snapshot.request_count,
        report.snapshot.settle_signal_count,
        report.snapshot.runtime_state_count,
        report.snapshot.protocol_event_count,
        network_summary,
        report.sitemap_path,
        report.snapshot_json_path,
        html_fallback,
        report.nda_facts_path,
    )
}

pub fn crawl_and_sync_sitemap_report(
    url: &str,
    sitemap_path: &Path,
) -> Result<BrowserWebNavigateReport, String> {
    let mut session = BrowserSessionState {
        id: "ephemeral".to_string(),
        current_url: Some(url.to_string()),
        cookies: Vec::new(),
        runtime_cookies: Vec::new(),
        local_storage: HashMap::new(),
        session_storage: HashMap::new(),
        network: BrowserSessionNetworkConfig::default(),
        last_html: None,
    };
    let snapshot = crawl_page_snapshot_with_session(&mut session, url)?;
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
    let snapshot_summary = snapshot.summary.clone();
    let summary = summarize_snapshot(snapshot);

    Ok(BrowserWebNavigateReport {
        snapshot: summary,
        snapshot_summary,
        snapshot_json_path: snapshot_path.display().to_string(),
        nda_facts_path: facts_path.display().to_string(),
        sitemap_path: sitemap_path.display().to_string(),
        html_fallback_path: html_fallback_path.map(|path| path.display().to_string()),
    })
}

pub fn crawl_and_sync_sitemap(url: &str, sitemap_path: &Path) -> Result<String, String> {
    let report = crawl_and_sync_sitemap_report(url, sitemap_path)?;
    Ok(render_web_navigate_report(&report))
}

fn render_workflow_step_lines(lines: &mut Vec<String>, step: &BrowserWorkflowStep, prefix: &str) {
    match step {
        BrowserWorkflowStep::Navigate { url } => {
            lines.push(format!("{}\tnavigate\t{}", prefix, encode_nda_text(url)));
        }
        BrowserWorkflowStep::Click { role, name } => {
            lines.push(format!(
                "{}\tclick\trole={}\tname={}",
                prefix,
                encode_nda_text(role),
                encode_nda_text(name)
            ));
        }
        BrowserWorkflowStep::FillField { field, value } => {
            lines.push(format!(
                "{}\tfill_field\tfield={}\tvalue={}",
                prefix,
                encode_nda_text(field),
                encode_nda_text(value)
            ));
        }
        BrowserWorkflowStep::SubmitForm { form } => {
            lines.push(format!(
                "{}\tsubmit_form\tform={}",
                prefix,
                encode_nda_text(form.as_deref().unwrap_or("default"))
            ));
        }
        BrowserWorkflowStep::WaitForText {
            text,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_text\ttext={}\ttimeout_ms={}\tinterval_ms={}",
                prefix,
                encode_nda_text(text),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::WaitForElement {
            role,
            name,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_element\trole={}\tname={}\ttimeout_ms={}\tinterval_ms={}",
                prefix,
                encode_nda_text(role),
                encode_nda_text(name),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::WaitForTitle {
            title,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_title\ttitle={}\ttimeout_ms={}\tinterval_ms={}",
                prefix,
                encode_nda_text(title),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::WaitForUrlContains {
            fragment,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_url_contains\tfragment={}\ttimeout_ms={}\tinterval_ms={}",
                prefix,
                encode_nda_text(fragment),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::WaitForMutation {
            label,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_mutation\tlabel={}\ttimeout_ms={}\tinterval_ms={}",
                prefix,
                encode_nda_text(label),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::WaitForRequest {
            method,
            url_contains,
            status,
            resource,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_request\tmethod={}\turl_contains={}\tstatus={}\tresource={}\ttimeout_ms={}\tinterval_ms={}"
                ,prefix,
                encode_nda_text(method.as_deref().unwrap_or_default()),
                encode_nda_text(url_contains.as_deref().unwrap_or_default()),
                status.map(|value| value.to_string()).unwrap_or_default(),
                encode_nda_text(resource.as_deref().unwrap_or_default()),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::WaitForStorage {
            scope,
            key,
            value,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_storage\tscope={}\tkey={}\tvalue={}\ttimeout_ms={}\tinterval_ms={}",
                prefix,
                encode_nda_text(scope),
                encode_nda_text(key),
                encode_nda_text(value.as_deref().unwrap_or_default()),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::WaitForSettle {
            label,
            scope,
            state,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_settle\tlabel={}\tscope={}\tstate={}\ttimeout_ms={}\tinterval_ms={}",
                prefix,
                encode_nda_text(label.as_deref().unwrap_or_default()),
                encode_nda_text(scope.as_deref().unwrap_or_default()),
                encode_nda_text(state.as_deref().unwrap_or_default()),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::WaitForRuntimeState {
            scope,
            key,
            value,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_runtime_state\tscope={}\tkey={}\tvalue={}\ttimeout_ms={}\tinterval_ms={}"
                ,prefix,
                encode_nda_text(scope),
                encode_nda_text(key),
                encode_nda_text(value.as_deref().unwrap_or_default()),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::WaitForProtocolEvent {
            event_kind,
            phase,
            target,
            detail,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_protocol_event\tkind={}\tphase={}\ttarget={}\tdetail={}\ttimeout_ms={}\tinterval_ms={}"
                ,prefix,
                encode_nda_text(event_kind.as_deref().unwrap_or_default()),
                encode_nda_text(phase.as_deref().unwrap_or_default()),
                encode_nda_text(target.as_deref().unwrap_or_default()),
                encode_nda_text(detail.as_deref().unwrap_or_default()),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::WaitForStable {
            stable_polls,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_stable\tstable_polls={}\ttimeout_ms={}\tinterval_ms={}",
                prefix,
                stable_polls.unwrap_or(DEFAULT_STABLE_POLLS),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::ExtractText {
            output,
            source,
            role,
            name,
            field,
        } => {
            lines.push(format!(
                "{}\textract_text\toutput={}\tsource={}\trole={}\tname={}\tfield={}",
                prefix,
                encode_nda_text(output),
                encode_nda_text(source),
                encode_nda_text(role.as_deref().unwrap_or_default()),
                encode_nda_text(name.as_deref().unwrap_or_default()),
                encode_nda_text(field.as_deref().unwrap_or_default())
            ));
        }
        BrowserWorkflowStep::SaveCheckpoint { name } => {
            lines.push(format!(
                "{}\tsave_checkpoint\tname={}",
                prefix,
                encode_nda_text(name)
            ));
        }
        BrowserWorkflowStep::RestoreCheckpoint { name } => {
            lines.push(format!(
                "{}\trestore_checkpoint\tname={}",
                prefix,
                encode_nda_text(name)
            ));
        }
        BrowserWorkflowStep::IfTextContains {
            text,
            then_steps,
            else_steps,
        } => {
            lines.push(format!(
                "{}\tif_text_contains\ttext={}",
                prefix,
                encode_nda_text(text)
            ));
            for (idx, nested) in then_steps.iter().enumerate() {
                render_workflow_step_lines(lines, nested, &format!("{}:then:{}", prefix, idx));
            }
            for (idx, nested) in else_steps.iter().enumerate() {
                render_workflow_step_lines(lines, nested, &format!("{}:else:{}", prefix, idx));
            }
        }
        BrowserWorkflowStep::IfOutputEquals {
            output,
            equals,
            then_steps,
            else_steps,
        } => {
            lines.push(format!(
                "{}\tif_output_equals\toutput={}\tequals={}",
                prefix,
                encode_nda_text(output),
                encode_nda_text(equals)
            ));
            for (idx, nested) in then_steps.iter().enumerate() {
                render_workflow_step_lines(lines, nested, &format!("{}:then:{}", prefix, idx));
            }
            for (idx, nested) in else_steps.iter().enumerate() {
                render_workflow_step_lines(lines, nested, &format!("{}:else:{}", prefix, idx));
            }
        }
        BrowserWorkflowStep::AssertElement { role, name } => {
            lines.push(format!(
                "{}\tassert_element\trole={}\tname={}",
                prefix,
                encode_nda_text(role),
                encode_nda_text(name)
            ));
        }
        BrowserWorkflowStep::AssertTextContains { text } => {
            lines.push(format!(
                "{}\tassert_text\t{}",
                prefix,
                encode_nda_text(text)
            ));
        }
        BrowserWorkflowStep::AssertOutput {
            output,
            equals,
            contains,
        } => {
            lines.push(format!(
                "{}\tassert_output\toutput={}\tequals={}\tcontains={}",
                prefix,
                encode_nda_text(output),
                encode_nda_text(equals.as_deref().unwrap_or_default()),
                encode_nda_text(contains.as_deref().unwrap_or_default())
            ));
        }
    }
}

pub fn render_workflow_dsl(workflow: &BrowserWorkflow) -> String {
    let mut lines = vec![
        "browser-workflow version 2".to_string(),
        format!("name\t{}", encode_nda_text(&workflow.name)),
        format!("start_url\t{}", encode_nda_text(&workflow.start_url)),
    ];

    for (idx, step) in workflow.steps.iter().enumerate() {
        let prefix = format!("step\t{}", idx);
        render_workflow_step_lines(&mut lines, step, &prefix);
    }

    lines.join("\n") + "\n"
}

pub fn render_workflow_save_report(report: &BrowserWorkflowSaveReport) -> String {
    format!(
        "Saved browser workflow '{}'\nJSON: {}\nNDA: {}",
        report.workflow.name, report.json_path, report.nda_path,
    )
}

pub fn save_workflow_report(
    workspace_root: &Path,
    workflow: &BrowserWorkflow,
) -> Result<BrowserWorkflowSaveReport, String> {
    let json_path = browser_workflow_json_path(workspace_root, &workflow.name);
    let nda_path = browser_workflow_nda_path(workspace_root, &workflow.name);
    if let Some(parent) = json_path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create workflow dir: {err}"))?;
    }
    let json =
        serde_json::to_vec_pretty(workflow).map_err(|err| format!("serialise workflow: {err}"))?;
    fs::write(&json_path, json).map_err(|err| format!("write workflow json: {err}"))?;
    fs::write(&nda_path, render_workflow_dsl(workflow))
        .map_err(|err| format!("write workflow nda: {err}"))?;
    Ok(BrowserWorkflowSaveReport {
        workflow: summarize_workflow(workflow.clone()),
        json_path: json_path.display().to_string(),
        nda_path: nda_path.display().to_string(),
    })
}

pub fn save_workflow(
    workspace_root: &Path,
    workflow: &BrowserWorkflow,
) -> Result<(PathBuf, PathBuf), String> {
    let report = save_workflow_report(workspace_root, workflow)?;
    Ok((
        PathBuf::from(report.json_path),
        PathBuf::from(report.nda_path),
    ))
}

pub fn load_workflow(path: &Path) -> Result<BrowserWorkflow, String> {
    let raw = fs::read(path).map_err(|err| format!("read workflow: {err}"))?;
    serde_json::from_slice(&raw).map_err(|err| format!("parse workflow: {err}"))
}

pub fn read_workflow_report(path: &Path) -> Result<BrowserWorkflowReadReport, String> {
    let workflow = load_workflow(path)?;
    Ok(BrowserWorkflowReadReport {
        nda_path: browser_workflow_nda_path(
            path.parent()
                .and_then(|parent| parent.parent())
                .and_then(|parent| parent.parent())
                .ok_or("workflow path is not inside a workspace")?,
            &workflow.name,
        )
        .display()
        .to_string(),
        workflow: summarize_workflow(workflow),
        json_path: path.display().to_string(),
    })
}

pub fn list_workflows(
    workspace_root: &Path,
    workflow_name_contains: Option<&str>,
    start_url_contains: Option<&str>,
    limit: Option<usize>,
    sort_direction: BrowserListSortDirection,
) -> Result<Vec<BrowserWorkflowSummary>, String> {
    let dir = workspace_root.join(".velocity").join("browser-workflows");
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|err| format!("read workflow dir: {err}"))? {
        let entry = entry.map_err(|err| format!("read workflow dir entry: {err}"))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| !name.ends_with(".browser.json"))
            .unwrap_or(true)
        {
            continue;
        }
        let raw = fs::read(&path).map_err(|err| format!("read workflow: {err}"))?;
        let workflow: BrowserWorkflow =
            serde_json::from_slice(&raw).map_err(|err| format!("parse workflow: {err}"))?;
        let mut summary = summarize_workflow(workflow);
        summary.json_path = Some(path.display().to_string());
        summary.nda_path = Some(
            browser_workflow_nda_path(workspace_root, &summary.name)
                .display()
                .to_string(),
        );
        if workflow_name_contains
            .map(|needle| contains_case_insensitive(&summary.name, needle))
            .unwrap_or(true)
            && start_url_contains
                .map(|needle| contains_case_insensitive(&summary.start_url, needle))
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

pub fn render_workflow_suite_save_report(report: &BrowserWorkflowSuiteSaveReport) -> String {
    format!(
        "Saved browser workflow suite '{}'\nJSON: {}",
        report.suite.name, report.json_path,
    )
}

pub fn save_workflow_suite_report(
    workspace_root: &Path,
    suite: &BrowserWorkflowSuite,
) -> Result<BrowserWorkflowSuiteSaveReport, String> {
    let path = browser_workflow_suite_json_path(workspace_root, &suite.name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create workflow suite dir: {err}"))?;
    }
    let json = serde_json::to_vec_pretty(suite)
        .map_err(|err| format!("serialise workflow suite: {err}"))?;
    fs::write(&path, json).map_err(|err| format!("write workflow suite: {err}"))?;
    Ok(BrowserWorkflowSuiteSaveReport {
        suite: summarize_workflow_suite(suite.clone()),
        json_path: path.display().to_string(),
    })
}

pub fn save_workflow_suite(
    workspace_root: &Path,
    suite: &BrowserWorkflowSuite,
) -> Result<PathBuf, String> {
    let report = save_workflow_suite_report(workspace_root, suite)?;
    Ok(PathBuf::from(report.json_path))
}

pub fn load_workflow_suite(path: &Path) -> Result<BrowserWorkflowSuite, String> {
    let raw = fs::read(path).map_err(|err| format!("read workflow suite: {err}"))?;
    serde_json::from_slice(&raw).map_err(|err| format!("parse workflow suite: {err}"))
}

pub fn read_workflow_suite_report(path: &Path) -> Result<BrowserWorkflowSuiteReadReport, String> {
    let suite = load_workflow_suite(path)?;
    Ok(BrowserWorkflowSuiteReadReport {
        suite: summarize_workflow_suite(suite),
        json_path: path.display().to_string(),
    })
}

pub fn list_workflow_suites(
    workspace_root: &Path,
    suite_name_contains: Option<&str>,
    limit: Option<usize>,
    sort_direction: BrowserListSortDirection,
) -> Result<Vec<BrowserWorkflowSuiteSummary>, String> {
    let dir = workspace_root.join(".velocity").join("browser-suites");
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|err| format!("read workflow suite dir: {err}"))? {
        let entry = entry.map_err(|err| format!("read workflow suite dir entry: {err}"))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| !name.ends_with(".suite.json"))
            .unwrap_or(true)
        {
            continue;
        }
        let raw = fs::read(&path).map_err(|err| format!("read workflow suite: {err}"))?;
        let suite: BrowserWorkflowSuite =
            serde_json::from_slice(&raw).map_err(|err| format!("parse workflow suite: {err}"))?;
        let mut summary = summarize_workflow_suite(suite);
        summary.json_path = Some(path.display().to_string());
        if suite_name_contains
            .map(|needle| contains_case_insensitive(&summary.name, needle))
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

pub fn read_workflow_run(
    workspace_root: &Path,
    workflow_name: &str,
    session_id: &str,
) -> Result<BrowserWorkflowRunReport, String> {
    let path = browser_workflow_run_path(workspace_root, workflow_name, session_id);
    let raw = fs::read(&path).map_err(|err| format!("read browser run report: {err}"))?;
    serde_json::from_slice(&raw).map_err(|err| format!("parse browser run report: {err}"))
}

pub fn read_workflow_run_report(
    workspace_root: &Path,
    workflow_name: &str,
    session_id: &str,
) -> Result<BrowserWorkflowRunReadReport, String> {
    let report = read_workflow_run(workspace_root, workflow_name, session_id)?;
    Ok(BrowserWorkflowRunReadReport {
        workflow: summarize_workflow_run(report),
        run_report_path: browser_workflow_run_path(workspace_root, workflow_name, session_id)
            .display()
            .to_string(),
    })
}

pub fn list_workflow_runs(
    workspace_root: &Path,
    workflow_name_contains: Option<&str>,
    session_id_contains: Option<&str>,
    final_url_contains: Option<&str>,
    limit: Option<usize>,
    sort_direction: BrowserListSortDirection,
) -> Result<Vec<BrowserWorkflowRunSummary>, String> {
    let dir = workspace_root.join(".velocity").join("browser-runs");
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|err| format!("read browser run dir: {err}"))? {
        let entry = entry.map_err(|err| format!("read browser run dir entry: {err}"))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read(&path).map_err(|err| format!("read browser run report: {err}"))?;
        let report: BrowserWorkflowRunReport = serde_json::from_slice(&raw)
            .map_err(|err| format!("parse browser run report: {err}"))?;
        let mut summary = summarize_workflow_run(report);
        summary.run_report_path = Some(path.display().to_string());
        if workflow_name_contains
            .map(|needle| contains_case_insensitive(&summary.workflow_name, needle))
            .unwrap_or(true)
            && session_id_contains
                .map(|needle| contains_case_insensitive(&summary.session_id, needle))
                .unwrap_or(true)
            && final_url_contains
                .map(|needle| contains_case_insensitive(&summary.final_url, needle))
                .unwrap_or(true)
        {
            items.push(summary);
        }
    }
    finalize_list(&mut items, sort_direction, limit, |left, right| {
        left.workflow_name
            .cmp(&right.workflow_name)
            .then(left.session_id.cmp(&right.session_id))
    });
    Ok(items)
}

pub fn read_workflow_suite_run(
    workspace_root: &Path,
    suite_name: &str,
) -> Result<BrowserWorkflowSuiteRunReport, String> {
    let path = browser_workflow_suite_run_path(workspace_root, suite_name);
    let raw = fs::read(&path).map_err(|err| format!("read browser suite run report: {err}"))?;
    serde_json::from_slice(&raw).map_err(|err| format!("parse browser suite run report: {err}"))
}

pub fn read_workflow_suite_run_report(
    workspace_root: &Path,
    suite_name: &str,
) -> Result<BrowserWorkflowSuiteRunReadReport, String> {
    let report = read_workflow_suite_run(workspace_root, suite_name)?;
    Ok(BrowserWorkflowSuiteRunReadReport {
        suite: summarize_workflow_suite_run(report),
        suite_report_path: browser_workflow_suite_run_path(workspace_root, suite_name)
            .display()
            .to_string(),
    })
}

pub fn list_workflow_suite_runs(
    workspace_root: &Path,
    suite_name_contains: Option<&str>,
    limit: Option<usize>,
    sort_direction: BrowserListSortDirection,
) -> Result<Vec<BrowserWorkflowSuiteRunSummary>, String> {
    let dir = workspace_root.join(".velocity").join("browser-suite-runs");
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|err| format!("read browser suite run dir: {err}"))? {
        let entry = entry.map_err(|err| format!("read browser suite run dir entry: {err}"))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read(&path).map_err(|err| format!("read browser suite run report: {err}"))?;
        let report: BrowserWorkflowSuiteRunReport = serde_json::from_slice(&raw)
            .map_err(|err| format!("parse browser suite run report: {err}"))?;
        let mut summary = summarize_workflow_suite_run(report);
        summary.suite_report_path = Some(path.display().to_string());
        if suite_name_contains
            .map(|needle| contains_case_insensitive(&summary.suite_name, needle))
            .unwrap_or(true)
        {
            items.push(summary);
        }
    }
    finalize_list(&mut items, sort_direction, limit, |left, right| {
        left.suite_name.cmp(&right.suite_name)
    });
    Ok(items)
}

fn execute_workflow_steps(
    steps: &[BrowserWorkflowStep],
    state: &mut BrowserReplayState,
    log: &mut Vec<String>,
    checkpoints: &mut HashMap<String, BrowserReplayState>,
    workspace_root: Option<&Path>,
) -> Result<(), String> {
    for step in steps {
        match step {
            BrowserWorkflowStep::Navigate { url } => {
                let resolved_url = resolve_template(url, state);
                state.snapshot =
                    crawl_page_snapshot_with_session(&mut state.session, &resolved_url)?;
                log.push(format!(
                    "navigate {} -> {}",
                    resolved_url, state.snapshot.title
                ));
            }
            BrowserWorkflowStep::Click { role, name } => {
                let resolved_role = resolve_template(role, state);
                let resolved_name = resolve_template(name, state);
                let target = find_element(&state.snapshot, &resolved_role, &resolved_name)
                    .ok_or_else(|| {
                        format!(
                            "workflow click target not found: role='{}' name='{}'",
                            resolved_role, resolved_name
                        )
                    })?;
                let target_url = target.target_url.clone().ok_or_else(|| {
                    format!(
                        "workflow click target '{}' is not a navigable link in the current static browser engine",
                        resolved_name
                    )
                })?;
                state.snapshot = crawl_page_snapshot_with_session(&mut state.session, &target_url)?;
                log.push(format!(
                    "click {}:{} -> {}",
                    resolved_role, resolved_name, state.snapshot.title
                ));
            }
            BrowserWorkflowStep::FillField { field, value } => {
                let resolved_field = resolve_template(field, state);
                let resolved_value = resolve_template(value, state);
                apply_fill_field(state, &resolved_field, &resolved_value)?;
                log.push(format!("fill_field {} ok", resolved_field));
            }
            BrowserWorkflowStep::SubmitForm { form } => {
                let resolved_form = form.as_ref().map(|value| resolve_template(value, state));
                submit_current_form(state, resolved_form.as_deref())?;
                log.push(format!(
                    "submit_form {} -> {}",
                    resolved_form.as_deref().unwrap_or("default"),
                    state.snapshot.title
                ));
            }
            BrowserWorkflowStep::WaitForText {
                text,
                timeout_ms,
                interval_ms,
            } => {
                let resolved_text = resolve_template(text, state);
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| snapshot_contains_text(candidate, &resolved_text),
                )?;
                log.push(format!(
                    "wait_for_text '{}' -> {}",
                    resolved_text,
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::WaitForElement {
                role,
                name,
                timeout_ms,
                interval_ms,
            } => {
                let resolved_role = resolve_template(role, state);
                let resolved_name = resolve_template(name, state);
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| find_element(candidate, &resolved_role, &resolved_name).is_some(),
                )?;
                log.push(format!(
                    "wait_for_element {}:{} -> {}",
                    resolved_role,
                    resolved_name,
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::WaitForTitle {
                title,
                timeout_ms,
                interval_ms,
            } => {
                let resolved_title = resolve_template(title, state);
                let lowered = resolved_title.to_ascii_lowercase();
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| candidate.title.to_ascii_lowercase().contains(&lowered),
                )?;
                log.push(format!(
                    "wait_for_title '{}' -> {}",
                    resolved_title,
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::WaitForUrlContains {
                fragment,
                timeout_ms,
                interval_ms,
            } => {
                let resolved_fragment = resolve_template(fragment, state);
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| candidate.url.contains(&resolved_fragment),
                )?;
                log.push(format!(
                    "wait_for_url_contains '{}' -> {}",
                    resolved_fragment,
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::WaitForMutation {
                label,
                timeout_ms,
                interval_ms,
            } => {
                let resolved_label = resolve_template(label, state);
                let lowered = resolved_label.to_ascii_lowercase();
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| {
                        candidate
                            .mutations
                            .iter()
                            .any(|entry| entry.to_ascii_lowercase().contains(&lowered))
                    },
                )?;
                log.push(format!(
                    "wait_for_mutation '{}' -> {}",
                    resolved_label,
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::WaitForRequest {
                method,
                url_contains,
                status,
                resource,
                timeout_ms,
                interval_ms,
            } => {
                let resolved_method = method.as_ref().map(|value| resolve_template(value, state));
                let resolved_url_contains = url_contains
                    .as_ref()
                    .map(|value| resolve_template(value, state));
                let resolved_resource = resource
                    .as_ref()
                    .map(|value| resolve_template(value, state));
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| {
                        candidate.requests.iter().any(|entry| {
                            request_record_matches(
                                entry,
                                resolved_method.as_deref(),
                                resolved_url_contains.as_deref(),
                                *status,
                                resolved_resource.as_deref(),
                            )
                        })
                    },
                )?;
                log.push(format!(
                    "wait_for_request method={} url_contains={} status={} resource={} -> {}",
                    resolved_method.as_deref().unwrap_or("*"),
                    resolved_url_contains.as_deref().unwrap_or("*"),
                    status
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "*".to_string()),
                    resolved_resource.as_deref().unwrap_or("*"),
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::WaitForStorage {
                scope,
                key,
                value,
                timeout_ms,
                interval_ms,
            } => {
                let resolved_scope = resolve_template(scope, state);
                let resolved_key = resolve_template(key, state);
                let resolved_value = value.as_ref().map(|entry| resolve_template(entry, state));
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| {
                        storage_entry_matches(
                            candidate,
                            &resolved_scope,
                            &resolved_key,
                            resolved_value.as_deref(),
                        )
                    },
                )?;
                log.push(format!(
                    "wait_for_storage {}:{}={} -> {}",
                    resolved_scope,
                    resolved_key,
                    resolved_value.as_deref().unwrap_or("*"),
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::WaitForSettle {
                label,
                scope,
                state: settle_state,
                timeout_ms,
                interval_ms,
            } => {
                let resolved_label = label.as_ref().map(|value| resolve_template(value, state));
                let resolved_scope = scope.as_ref().map(|value| resolve_template(value, state));
                let resolved_state = settle_state
                    .as_ref()
                    .map(|value| resolve_template(value, state));
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| {
                        candidate.settle_signals.iter().any(|entry| {
                            settle_signal_matches(
                                entry,
                                resolved_label.as_deref(),
                                resolved_scope.as_deref(),
                                resolved_state.as_deref(),
                            )
                        })
                    },
                )?;
                log.push(format!(
                    "wait_for_settle {} -> {}",
                    resolved_label.clone().unwrap_or_else(|| format!(
                        "{}:{}",
                        resolved_scope.as_deref().unwrap_or("*"),
                        resolved_state.as_deref().unwrap_or("*")
                    )),
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::WaitForRuntimeState {
                scope,
                key,
                value,
                timeout_ms,
                interval_ms,
            } => {
                let resolved_scope = resolve_template(scope, state);
                let resolved_key = resolve_template(key, state);
                let resolved_value = value.as_ref().map(|value| resolve_template(value, state));
                let lowered_scope = resolved_scope.to_ascii_lowercase();
                let lowered_key = resolved_key.to_ascii_lowercase();
                let lowered_value = resolved_value
                    .as_ref()
                    .map(|value| value.to_ascii_lowercase());
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
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
                )?;
                log.push(format!(
                    "wait_for_runtime_state {}:{}={} -> {}",
                    resolved_scope,
                    resolved_key,
                    resolved_value.as_deref().unwrap_or("*"),
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::WaitForProtocolEvent {
                event_kind,
                phase,
                target,
                detail,
                timeout_ms,
                interval_ms,
            } => {
                let resolved_kind = event_kind
                    .as_ref()
                    .map(|value| resolve_template(value, state));
                let resolved_phase = phase.as_ref().map(|value| resolve_template(value, state));
                let resolved_target = target.as_ref().map(|value| resolve_template(value, state));
                let resolved_detail = detail.as_ref().map(|value| resolve_template(value, state));
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| {
                        candidate.protocol_events.iter().any(|entry| {
                            protocol_event_matches(
                                entry,
                                resolved_kind.as_deref(),
                                resolved_phase.as_deref(),
                                resolved_target.as_deref(),
                                resolved_detail.as_deref(),
                            )
                        })
                    },
                )?;
                log.push(format!(
                    "wait_for_protocol_event kind={} phase={} target={} detail={} -> {}",
                    resolved_kind.as_deref().unwrap_or("*"),
                    resolved_phase.as_deref().unwrap_or("*"),
                    resolved_target.as_deref().unwrap_or("*"),
                    resolved_detail.as_deref().unwrap_or("*"),
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::WaitForStable {
                stable_polls,
                timeout_ms,
                interval_ms,
            } => {
                let diff = wait_for_stable_snapshot(
                    &mut state.session,
                    &mut state.snapshot,
                    *stable_polls,
                    *timeout_ms,
                    *interval_ms,
                )?;
                log.push(format!(
                    "wait_for_stable polls={} -> {}",
                    stable_polls.unwrap_or(DEFAULT_STABLE_POLLS),
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::ExtractText {
                output,
                source,
                role,
                name,
                field,
            } => {
                let resolved_source = resolve_template(source, state);
                let resolved_role = role.as_ref().map(|value| resolve_template(value, state));
                let resolved_name = name.as_ref().map(|value| resolve_template(value, state));
                let resolved_field = field.as_ref().map(|value| resolve_template(value, state));
                let extracted = extract_snapshot_value(
                    &state.snapshot,
                    &resolved_source,
                    resolved_role.as_deref(),
                    resolved_name.as_deref(),
                    resolved_field.as_deref(),
                )?;
                state.outputs.insert(output.clone(), extracted.clone());
                log.push(format!(
                    "extract_text {}='{}'",
                    output,
                    truncate_string(&extracted, 80)
                ));
            }
            BrowserWorkflowStep::SaveCheckpoint { name } => {
                let resolved_name = resolve_template(name, state);
                checkpoints.insert(resolved_name.clone(), state.clone());
                if let Some(root) = workspace_root {
                    let path = persist_checkpoint_from_replay_state(root, state, &resolved_name)?;
                    log.push(format!(
                        "save_checkpoint {} -> {}",
                        resolved_name,
                        path.display()
                    ));
                } else {
                    log.push(format!("save_checkpoint {} ok", resolved_name));
                }
            }
            BrowserWorkflowStep::RestoreCheckpoint { name } => {
                let resolved_name = resolve_template(name, state);
                let restored = checkpoints.get(&resolved_name).cloned().ok_or_else(|| {
                    format!("workflow restore checkpoint not found: '{}'", resolved_name)
                })?;
                *state = restored;
                log.push(format!(
                    "restore_checkpoint {} -> {}",
                    resolved_name, state.snapshot.title
                ));
            }
            BrowserWorkflowStep::IfTextContains {
                text,
                then_steps,
                else_steps,
            } => {
                let resolved_text = resolve_template(text, state);
                let matched = snapshot_contains_text(&state.snapshot, &resolved_text);
                log.push(format!(
                    "if_text_contains '{}' -> {}",
                    resolved_text,
                    if matched { "then" } else { "else" }
                ));
                let branch = if matched { then_steps } else { else_steps };
                execute_workflow_steps(branch, state, log, checkpoints, workspace_root)?;
            }
            BrowserWorkflowStep::IfOutputEquals {
                output,
                equals,
                then_steps,
                else_steps,
            } => {
                let actual = state.outputs.get(output).cloned().unwrap_or_default();
                let resolved_expected = resolve_template(equals, state);
                let matched = actual == resolved_expected;
                log.push(format!(
                    "if_output_equals {}='{}' -> {}",
                    output,
                    truncate_string(&actual, 80),
                    if matched { "then" } else { "else" }
                ));
                let branch = if matched { then_steps } else { else_steps };
                execute_workflow_steps(branch, state, log, checkpoints, workspace_root)?;
            }
            BrowserWorkflowStep::AssertElement { role, name } => {
                let resolved_role = resolve_template(role, state);
                let resolved_name = resolve_template(name, state);
                find_element(&state.snapshot, &resolved_role, &resolved_name).ok_or_else(|| {
                    format!(
                        "workflow assertion failed: missing element role='{}' name='{}'",
                        resolved_role, resolved_name
                    )
                })?;
                log.push(format!(
                    "assert_element {}:{} ok",
                    resolved_role, resolved_name
                ));
            }
            BrowserWorkflowStep::AssertTextContains { text } => {
                let resolved_text = resolve_template(text, state);
                if !snapshot_contains_text(&state.snapshot, &resolved_text) {
                    return Err(format!(
                        "workflow assertion failed: text '{}' not present",
                        resolved_text
                    ));
                }
                log.push(format!("assert_text '{}' ok", resolved_text));
            }
            BrowserWorkflowStep::AssertOutput {
                output,
                equals,
                contains,
            } => {
                let actual = state.outputs.get(output).cloned().ok_or_else(|| {
                    format!("workflow assertion failed: output '{}' not present", output)
                })?;
                if let Some(expected) = equals {
                    let resolved_expected = resolve_template(expected, state);
                    if actual != resolved_expected {
                        return Err(format!(
                            "workflow assertion failed: output '{}' expected '{}' but was '{}'",
                            output, resolved_expected, actual
                        ));
                    }
                }
                if let Some(expected_fragment) = contains {
                    let resolved_fragment = resolve_template(expected_fragment, state);
                    if !actual.contains(&resolved_fragment) {
                        return Err(format!(
                            "workflow assertion failed: output '{}' does not contain '{}'",
                            output, resolved_fragment
                        ));
                    }
                }
                log.push(format!("assert_output {} ok", output));
            }
        }
    }
    Ok(())
}

pub fn render_workflow_replay_result(
    result: &str,
    snapshot_path: &Path,
    session_path: &Path,
    facts_path: &Path,
    report_path: &Path,
    html_fallback_path: Option<&Path>,
) -> String {
    let html_fallback = html_fallback_path
        .map(|path| format!("\nHTML fallback: {}", path.display()))
        .unwrap_or_default();
    format!(
        "{}\nSnapshot JSON: {}\nSession JSON: {}{}\nNDA Facts: {}\nRun Report: {}",
        result,
        snapshot_path.display(),
        session_path.display(),
        html_fallback,
        facts_path.display(),
        report_path.display()
    )
}

pub fn render_workflow_suite_execution_report(
    suite_name: &str,
    summary: &BrowserWorkflowSuiteRunSummary,
    report_path: &Path,
) -> String {
    format!(
        "Workflow suite '{}' completed.\nTotal: {}\nPassed: {}\nFailed: {}\nSuite Report: {}",
        suite_name,
        summary.total,
        summary.passed,
        summary.failed,
        report_path.display()
    )
}

fn replay_workflow_with_state(
    workflow: &BrowserWorkflow,
    mut state: BrowserReplayState,
    workspace_root: Option<&Path>,
) -> Result<(String, BrowserReplayState, BrowserWorkflowRunReport), String> {
    let mut log = vec![format!(
        "start {} -> {}",
        state.snapshot.url, state.snapshot.title
    )];
    let mut checkpoints = HashMap::new();
    execute_workflow_steps(
        &workflow.steps,
        &mut state,
        &mut log,
        &mut checkpoints,
        workspace_root,
    )?;

    let network_summary = summarize_network_activity(&state.snapshot.protocol_events);
    let report = BrowserWorkflowRunReport {
        workflow_name: workflow.name.clone(),
        session_id: state.session.id.clone(),
        final_url: state.snapshot.url.clone(),
        final_title: state.snapshot.title.clone(),
        step_count: workflow.steps.len(),
        cookie_count: state.session.cookies.len(),
        local_storage_count: state.session.local_storage.len(),
        session_storage_count: state.session.session_storage.len(),
        mutation_count: state.snapshot.mutations.len(),
        request_count: state.snapshot.requests.len(),
        settle_signal_count: state.snapshot.settle_signals.len(),
        runtime_state_count: state.snapshot.runtime_state.len(),
        protocol_event_count: state.snapshot.protocol_events.len(),
        network_summary: network_summary.clone(),
        outputs: state.outputs.clone(),
        log: log.clone(),
    };
    let network_summary_line = render_network_summary(&network_summary)
        .map(|value| format!("\nNetwork summary: {}", value))
        .unwrap_or_default();
    let result = format!(
        "Workflow '{}' completed.\nFinal URL: {}\nFinal title: {}\nSession: {}\nSteps executed: {}\nCookies: {}\nRequests: {}\nSettle signals: {}\nRuntime state: {}\nProtocol events: {}{}\nLocal storage: {}\nSession storage: {}\nMutations: {}\nOutputs: {}\n{}",
        workflow.name,
        state.snapshot.url,
        state.snapshot.title,
        state.session.id,
        workflow.steps.len(),
        state.session.cookies.len(),
        state.snapshot.requests.len(),
        state.snapshot.settle_signals.len(),
        state.snapshot.runtime_state.len(),
        state.snapshot.protocol_events.len(),
        network_summary_line,
        state.session.local_storage.len(),
        state.session.session_storage.len(),
        state.snapshot.mutations.len(),
        state.outputs.len(),
        log.join("\n")
    );
    Ok((result, state, report))
}

fn persist_replay_state(
    workspace_root: &Path,
    state: &BrowserReplayState,
    sitemap_path: &Path,
) -> Result<(PathBuf, PathBuf, PathBuf, Option<PathBuf>), String> {
    persist_snapshot_to_sitemap(&state.snapshot, sitemap_path)?;
    let facts_path = write_crawl_facts(
        &state.snapshot.url,
        &state.snapshot.title,
        &state.snapshot.summary,
        &state.snapshot.elements,
        &state.snapshot.forms,
        &state.snapshot.cookies,
        &state.snapshot.storage,
        &state.snapshot.mutations,
        &state.snapshot.requests,
        &state.snapshot.settle_signals,
        &state.snapshot.runtime_state,
        &state.snapshot.protocol_events,
        sitemap_path,
    )?;
    let snapshot_path = write_snapshot_json(&state.snapshot, sitemap_path)?;
    let html_fallback_path = write_html_fallback(
        &state.snapshot.url,
        state.session.last_html.as_deref().unwrap_or_default(),
        sitemap_path,
    )?;
    let session_path = save_session_state(workspace_root, &state.session)?;
    Ok((snapshot_path, session_path, facts_path, html_fallback_path))
}

fn persist_run_report(
    workspace_root: &Path,
    report: &BrowserWorkflowRunReport,
) -> Result<PathBuf, String> {
    let path = browser_workflow_run_path(workspace_root, &report.workflow_name, &report.session_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create browser run dir: {err}"))?;
    }
    let json = serde_json::to_vec_pretty(report)
        .map_err(|err| format!("serialise browser run report: {err}"))?;
    fs::write(&path, json).map_err(|err| format!("write browser run report: {err}"))?;
    Ok(path)
}

fn persist_suite_run_report(
    workspace_root: &Path,
    report: &BrowserWorkflowSuiteRunReport,
) -> Result<PathBuf, String> {
    let path = browser_workflow_suite_run_path(workspace_root, &report.suite_name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create browser suite run dir: {err}"))?;
    }
    let json = serde_json::to_vec_pretty(report)
        .map_err(|err| format!("serialise browser suite run report: {err}"))?;
    fs::write(&path, json).map_err(|err| format!("write browser suite run report: {err}"))?;
    Ok(path)
}

pub fn replay_workflow(workflow: &BrowserWorkflow) -> Result<String, String> {
    let mut session = BrowserSessionState {
        id: format!("replay-{}", sanitize_file_stem(&workflow.name)),
        current_url: Some(workflow.start_url.clone()),
        cookies: Vec::new(),
        runtime_cookies: Vec::new(),
        local_storage: HashMap::new(),
        session_storage: HashMap::new(),
        network: BrowserSessionNetworkConfig::default(),
        last_html: None,
    };
    let snapshot = crawl_page_snapshot_with_session(&mut session, &workflow.start_url)?;
    let state = BrowserReplayState {
        session,
        snapshot,
        filled_fields: HashMap::new(),
        variables: workflow.variables.clone(),
        outputs: HashMap::new(),
    };
    let (result, _, _) = replay_workflow_with_state(workflow, state, None)?;
    Ok(result)
}

pub fn replay_workflow_with_artifacts_report(
    workspace_root: &Path,
    workflow: &BrowserWorkflow,
    sitemap_path: &Path,
) -> Result<BrowserWorkflowReplayReport, String> {
    let mut session = BrowserSessionState {
        id: format!("replay-{}", sanitize_file_stem(&workflow.name)),
        current_url: Some(workflow.start_url.clone()),
        cookies: Vec::new(),
        runtime_cookies: Vec::new(),
        local_storage: HashMap::new(),
        session_storage: HashMap::new(),
        network: BrowserSessionNetworkConfig::default(),
        last_html: None,
    };
    let snapshot = crawl_page_snapshot_with_session(&mut session, &workflow.start_url)?;
    let state = BrowserReplayState {
        session,
        snapshot,
        filled_fields: HashMap::new(),
        variables: workflow.variables.clone(),
        outputs: HashMap::new(),
    };
    let (result, final_state, report) =
        replay_workflow_with_state(workflow, state, Some(workspace_root))?;
    let (snapshot_path, session_path, facts_path, html_fallback_path) =
        persist_replay_state(workspace_root, &final_state, sitemap_path)?;
    let report_path = persist_run_report(workspace_root, &report)?;
    let workflow = summarize_workflow_run(report);
    let _ = result;
    Ok(BrowserWorkflowReplayReport {
        workflow,
        snapshot_json_path: snapshot_path.display().to_string(),
        session_json_path: session_path.display().to_string(),
        nda_facts_path: facts_path.display().to_string(),
        run_report_path: report_path.display().to_string(),
        html_fallback_path: html_fallback_path.map(|path| path.display().to_string()),
    })
}

pub fn replay_workflow_with_artifacts(
    workspace_root: &Path,
    workflow: &BrowserWorkflow,
    sitemap_path: &Path,
) -> Result<String, String> {
    let report = replay_workflow_with_artifacts_report(workspace_root, workflow, sitemap_path)?;
    let network_summary = render_network_summary(&report.workflow.network_summary)
        .map(|value| format!("\nNetwork summary: {}", value))
        .unwrap_or_default();
    let result = format!(
        "Workflow '{}' completed.\nFinal URL: {}\nFinal title: {}\nSession: {}\nSteps executed: {}\nCookies: {}\nRequests: {}\nSettle signals: {}\nRuntime state: {}\nProtocol events: {}{}\nLocal storage: {}\nSession storage: {}",
        report.workflow.workflow_name,
        report.workflow.final_url,
        report.workflow.final_title,
        report.workflow.session_id,
        report.workflow.step_count,
        report.workflow.cookie_count,
        report.workflow.request_count,
        report.workflow.settle_signal_count,
        report.workflow.runtime_state_count,
        report.workflow.protocol_event_count,
        network_summary,
        report.workflow.local_storage_count,
        report.workflow.session_storage_count,
    );
    Ok(render_workflow_replay_result(
        &result,
        Path::new(&report.snapshot_json_path),
        Path::new(&report.session_json_path),
        Path::new(&report.nda_facts_path),
        Path::new(&report.run_report_path),
        report.html_fallback_path.as_deref().map(Path::new),
    ))
}

pub fn run_workflow_suite_report(
    workspace_root: &Path,
    suite: &BrowserWorkflowSuite,
    sitemap_path: &Path,
) -> Result<BrowserWorkflowSuiteExecutionReport, String> {
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut items = Vec::with_capacity(suite.workflows.len());

    for workflow_path in &suite.workflows {
        let full_path = workspace_root.join(workflow_path);
        match load_workflow(&full_path) {
            Ok(workflow) => {
                match replay_workflow_with_artifacts(workspace_root, &workflow, sitemap_path) {
                    Ok(summary) => {
                        passed += 1;
                        items.push(BrowserWorkflowSuiteRunItem {
                            workflow_path: workflow_path.clone(),
                            workflow_name: workflow.name,
                            status: "passed".to_string(),
                            summary: truncate_string(&summary, 400),
                        });
                    }
                    Err(err) => {
                        failed += 1;
                        items.push(BrowserWorkflowSuiteRunItem {
                            workflow_path: workflow_path.clone(),
                            workflow_name: workflow.name,
                            status: "failed".to_string(),
                            summary: err,
                        });
                    }
                }
            }
            Err(err) => {
                failed += 1;
                items.push(BrowserWorkflowSuiteRunItem {
                    workflow_path: workflow_path.clone(),
                    workflow_name: workflow_path.clone(),
                    status: "failed".to_string(),
                    summary: err,
                });
            }
        }
    }

    let report = BrowserWorkflowSuiteRunReport {
        suite_name: suite.name.clone(),
        total: suite.workflows.len(),
        passed,
        failed,
        items,
    };
    let report_path = persist_suite_run_report(workspace_root, &report)?;
    let suite = summarize_workflow_suite_run(report);
    Ok(BrowserWorkflowSuiteExecutionReport {
        suite,
        suite_report_path: report_path.display().to_string(),
    })
}

pub fn run_workflow_suite(
    workspace_root: &Path,
    suite: &BrowserWorkflowSuite,
    sitemap_path: &Path,
) -> Result<String, String> {
    let report = run_workflow_suite_report(workspace_root, suite, sitemap_path)?;
    Ok(render_workflow_suite_execution_report(
        &report.suite.suite_name,
        &report.suite,
        Path::new(&report.suite_report_path),
    ))
}

pub fn replay_workflow_in_session_report(
    workspace_root: &Path,
    session_id: &str,
    workflow: &BrowserWorkflow,
    sitemap_path: &Path,
) -> Result<BrowserWorkflowReplayReport, String> {
    let mut session = load_session_state(workspace_root, session_id)?;
    let snapshot = match session.current_url.clone() {
        Some(url) => load_snapshot_json(&url, sitemap_path)
            .or_else(|_| crawl_page_snapshot_with_session(&mut session, &url)),
        None => crawl_page_snapshot_with_session(&mut session, &workflow.start_url),
    }?;
    session.id = session_id.to_string();
    let state = BrowserReplayState {
        session,
        snapshot,
        filled_fields: HashMap::new(),
        variables: workflow.variables.clone(),
        outputs: HashMap::new(),
    };
    let (_result, final_state, report) =
        replay_workflow_with_state(workflow, state, Some(workspace_root))?;
    let (snapshot_path, session_path, facts_path, html_fallback_path) =
        persist_replay_state(workspace_root, &final_state, sitemap_path)?;
    let report_path = persist_run_report(workspace_root, &report)?;
    let workflow = summarize_workflow_run(report);
    Ok(BrowserWorkflowReplayReport {
        workflow,
        snapshot_json_path: snapshot_path.display().to_string(),
        session_json_path: session_path.display().to_string(),
        nda_facts_path: facts_path.display().to_string(),
        run_report_path: report_path.display().to_string(),
        html_fallback_path: html_fallback_path.map(|path| path.display().to_string()),
    })
}

pub fn replay_workflow_in_session(
    workspace_root: &Path,
    session_id: &str,
    workflow: &BrowserWorkflow,
    sitemap_path: &Path,
) -> Result<String, String> {
    let report =
        replay_workflow_in_session_report(workspace_root, session_id, workflow, sitemap_path)?;
    let network_summary = render_network_summary(&report.workflow.network_summary)
        .map(|value| format!("\nNetwork summary: {}", value))
        .unwrap_or_default();
    let result = format!(
        "Workflow '{}' completed.\nFinal URL: {}\nFinal title: {}\nSession: {}\nSteps executed: {}\nCookies: {}\nRequests: {}\nSettle signals: {}\nRuntime state: {}\nProtocol events: {}{}\nLocal storage: {}\nSession storage: {}",
        report.workflow.workflow_name,
        report.workflow.final_url,
        report.workflow.final_title,
        report.workflow.session_id,
        report.workflow.step_count,
        report.workflow.cookie_count,
        report.workflow.request_count,
        report.workflow.settle_signal_count,
        report.workflow.runtime_state_count,
        report.workflow.protocol_event_count,
        network_summary,
        report.workflow.local_storage_count,
        report.workflow.session_storage_count,
    );
    Ok(render_workflow_replay_result(
        &result,
        Path::new(&report.snapshot_json_path),
        Path::new(&report.session_json_path),
        Path::new(&report.nda_facts_path),
        Path::new(&report.run_report_path),
        report.html_fallback_path.as_deref().map(Path::new),
    ))
}



