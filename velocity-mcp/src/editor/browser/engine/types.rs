use crate::editor::browser::models::*;
use crate::editor::browser::truncate_string;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;


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

pub struct BrowserHttpResponse {
    pub html: String,
    pub final_url: String,
    pub cookies: Vec<BrowserCookie>,
    pub local_storage_updates: HashMap<String, String>,
    pub session_storage_updates: HashMap<String, String>,
    pub mutations: Vec<String>,
    pub requests: Vec<BrowserRequestRecord>,
    pub settle_signals: Vec<String>,
    pub runtime_state: Vec<BrowserRuntimeState>,
    pub protocol_events: Vec<BrowserProtocolEvent>,
}


#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RuntimeCaptureApiRequestRecord {
    pub method: String,
    pub url: String,
    pub status_code: u16,
    pub resource: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RuntimeCaptureApiState {
    pub scope: String,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RuntimeCaptureApiProtocolEvent {
    pub kind: String,
    pub phase: String,
    pub target: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCaptureApiFrameEntry {
    #[serde(default)]
    pub selector: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub same_origin: bool,
    #[serde(default)]
    pub accessible: bool,
    #[serde(default)]
    pub semantic_node_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCaptureApiShadowHostEntry {
    #[serde(default)]
    pub selector: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub semantic_node_count: usize,
    #[serde(default)]
    pub text_sample: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCaptureApiCanvasEntry {
    #[serde(default)]
    pub selector: String,
    #[serde(default)]
    pub width: usize,
    #[serde(default)]
    pub height: usize,
    #[serde(default)]
    pub context_kinds: Vec<String>,
    #[serde(default)]
    pub text_op_count: usize,
    #[serde(default)]
    pub image_op_count: usize,
    #[serde(default)]
    pub webgl_draw_count: usize,
    #[serde(default)]
    pub readback_count: usize,
    #[serde(default)]
    pub likely_animated: bool,
    #[serde(default)]
    pub runtime_evidence: bool,
    #[serde(default)]
    pub text_sample: String,
}



#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RuntimeCaptureApiResponse {
    pub final_url: String,
    pub title: String,
    pub html: String,
    pub aom_summary: String,
    pub page_text: String,
    #[serde(default)]
    pub scripts: Vec<String>,
    #[serde(default)]
    pub fields: HashMap<String, String>,
    #[serde(default)]
    pub cookies: Vec<RuntimeBrowserCookie>,
    #[serde(default)]
    pub local_storage: HashMap<String, String>,
    #[serde(default)]
    pub session_storage: HashMap<String, String>,
    #[serde(default)]
    pub settle_signals: Vec<String>,
    #[serde(default)]
    pub runtime_state: Vec<RuntimeCaptureApiState>,
    #[serde(default)]
    pub protocol_events: Vec<RuntimeCaptureApiProtocolEvent>,
    #[serde(default)]
    pub requests: Vec<RuntimeCaptureApiRequestRecord>,
    #[serde(default)]
    pub frames: Vec<RuntimeCaptureApiFrameEntry>,
    #[serde(default)]
    pub shadow_hosts: Vec<RuntimeCaptureApiShadowHostEntry>,
    #[serde(default)]
    pub canvases: Vec<RuntimeCaptureApiCanvasEntry>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub action: Option<RuntimeActionApiResult>,
}

#[derive(Debug, Serialize)]
pub struct RuntimeCaptureApiRequest<'a> {
    pub url: &'a str,
    pub timeout_ms: u64,
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

pub fn render_html_fallback_line(html_fallback_path: Option<&str>) -> String {
    html_fallback_path
        .map(|path| format!("\nHTML fallback: {}", path))
        .unwrap_or_default()
}

pub fn browser_runtime_api_base() -> String {
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

pub fn resolve_browser_runtime_api_base(api_base: Option<&str>) -> String {
    api_base
        .map(str::trim)
        .map(|value| value.trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(browser_runtime_api_base)
}

pub fn format_runtime_api_error(err: ureq::Error) -> String {
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

pub fn runtime_api_request(
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

pub fn runtime_capture_response_from_value(
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

pub fn parse_runtime_session_cookie_value(raw: &str) -> RuntimeBrowserCookie {
    let trimmed = raw.trim();
    let (name, value) = trimmed.split_once('=').unwrap_or((trimmed, ""));
    RuntimeBrowserCookie {
        name: name.trim().to_string(),
        value: value.trim().to_string(),
        ..RuntimeBrowserCookie::default()
    }
}

pub fn parse_runtime_string_map(value: Option<&serde_json::Value>) -> HashMap<String, String> {
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

pub fn parse_runtime_string_list(value: Option<&serde_json::Value>) -> Vec<String> {
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

pub fn parse_runtime_session_capture_response(
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
                            
                            parse_runtime_session_cookie_value(raw)
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
