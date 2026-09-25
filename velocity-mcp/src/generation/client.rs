//! Generation client for DashScope-native job APIs.
//!
//! Supports the two invocation styles the real endpoints use:
//! * **Synchronous** (`multimodal-generation/generation`, e.g. wan2.7-image):
//!   the POST response embeds the finished artifact URL. Plans that do not
//!   permit asynchronous calls require this mode.
//! * **Asynchronous job submission** (`video-generation/video-synthesis`, e.g.
//!   happyhorse-1.1-t2v): the POST returns a `task_id` which is polled at
//!   `/api/v1/tasks/{id}` until SUCCEEDED.
//!
//! The request `input` shape also varies by model (`prompt` vs `messages`), so
//! both are driven by the model spec rather than hard-coded here.

use super::registry::{GenerationModelSpec, InputFormat, InvocationMode};
use serde_json::Value;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Public result type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct GenerationResult {
    pub task_id: String,
    pub success: bool,
    pub message: String,
    /// Local path to the downloaded artifact (None if download failed).
    pub local_path: Option<PathBuf>,
    /// Remote URL returned by the API (always available on success).
    pub remote_url: Option<String>,
    pub duration: Duration,
}

// ---------------------------------------------------------------------------
// Native base URL derivation
// ---------------------------------------------------------------------------

/// Strip the `/compatible-mode` suffix from an OpenAI-compatible base URL to
/// recover the native DashScope API base (where generation endpoints live).
pub fn derive_native_base(compatible_base: &str) -> String {
    let trimmed = compatible_base.trim_end_matches('/');
    if let Some(stripped) = trimmed.strip_suffix("/compatible-mode") {
        stripped.to_string()
    } else {
        // Already native or unexpected shape — use as-is.
        trimmed.to_string()
    }
}

// ---------------------------------------------------------------------------
// Request construction
// ---------------------------------------------------------------------------

/// Build the request `input` object in the shape the endpoint expects.
fn build_input(input_format: InputFormat, user_input: &Value) -> Value {
    match input_format {
        InputFormat::Prompt => user_input.clone(),
        InputFormat::Messages => {
            // Respect caller-supplied messages verbatim.
            if user_input.get("messages").is_some() {
                return user_input.clone();
            }
            let prompt = user_input
                .get("prompt")
                .and_then(|p| p.as_str())
                .unwrap_or("");
            serde_json::json!({
                "messages": [
                    { "role": "user", "content": [ { "text": prompt } ] }
                ]
            })
        }
    }
}

/// Merge caller overrides over the spec's default parameters.
fn merge_parameters(spec: &GenerationModelSpec, overrides: Option<&Value>) -> Value {
    match overrides {
        Some(o) if !o.is_null() => {
            let mut base = spec
                .default_parameters
                .as_object()
                .cloned()
                .unwrap_or_default();
            if let Some(obj) = o.as_object() {
                for (k, v) in obj {
                    base.insert(k.clone(), v.clone());
                }
            }
            Value::Object(base)
        }
        _ => spec.default_parameters.clone(),
    }
}

// ---------------------------------------------------------------------------
// Error surfacing
// ---------------------------------------------------------------------------

/// Render a ureq error, extracting the API's own `message` field from a non-2xx
/// response body so callers see *why* a submission was rejected (bad endpoint,
/// model not on plan, quota, etc.) rather than a bare status code.
fn describe_ureq_err(err: ureq::Error) -> String {
    match err {
        ureq::Error::Status(code, resp) => {
            let mut body = String::new();
            let _ = resp.into_reader().read_to_string(&mut body);
            let detail = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| {
                    v.get("message")
                        .and_then(|m| m.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| clip(&body, 300));
            format!("HTTP {code}: {detail}")
        }
        ureq::Error::Transport(t) => format!("transport error: {t}"),
    }
}

fn clip(value: &str, max: usize) -> String {
    let count = value.chars().count();
    if count <= max {
        return value.to_string();
    }
    let kept: String = value.chars().take(max).collect();
    format!("{kept}…")
}

// ---------------------------------------------------------------------------
// Submission (sync or async)
// ---------------------------------------------------------------------------

/// POST a generation request. When `async_call` is true the DashScope async
/// header is added (required for job submission; rejected by plans that only
/// allow synchronous calls). Returns the parsed JSON response.
fn post_request(
    native_base: &str,
    api_key: &str,
    spec: &GenerationModelSpec,
    body: &Value,
    async_call: bool,
) -> Result<Value, String> {
    let url = format!("{}{}", native_base, spec.endpoint_path);
    // Synchronous generation holds the connection open for the whole render, so
    // it needs a longer timeout than the async submit (which returns a task id).
    let timeout = if async_call {
        Duration::from_secs(30)
    } else {
        Duration::from_secs(spec.output_type.default_poll_timeout_secs() + 30)
    };

    let mut req = ureq::post(&url)
        .timeout(timeout)
        .set("Authorization", &format!("Bearer {api_key}"))
        .set("Content-Type", "application/json");
    if async_call {
        req = req.set("X-DashScope-Async", "enable");
    }

    let response = req.send_json(body).map_err(describe_ureq_err)?;

    response
        .into_json()
        .map_err(|e| format!("failed to parse response: {e}"))
}

// ---------------------------------------------------------------------------
// Polling (async only)
// ---------------------------------------------------------------------------

/// Poll the task status endpoint until SUCCEEDED, FAILED, or timeout.
fn poll_job(
    native_base: &str,
    api_key: &str,
    task_id: &str,
    timeout: Duration,
) -> Result<Value, String> {
    let url = format!("{}/api/v1/tasks/{}", native_base, task_id);
    let start = Instant::now();
    let mut interval = Duration::from_secs(3);

    loop {
        if start.elapsed() > timeout {
            return Err(format!(
                "timed out after {}s waiting for task",
                start.elapsed().as_secs()
            ));
        }

        let response = ureq::get(&url)
            .timeout(Duration::from_secs(15))
            .set("Authorization", &format!("Bearer {api_key}"))
            .call()
            .map_err(describe_ureq_err)?;

        let body: Value = response
            .into_json()
            .map_err(|e| format!("failed to parse poll response: {e}"))?;

        let task_status = body
            .get("output")
            .and_then(|o| o.get("task_status"))
            .and_then(|s| s.as_str())
            .unwrap_or("UNKNOWN");

        match task_status {
            "SUCCEEDED" => return Ok(body),
            "FAILED" | "CANCELED" | "UNKNOWN_STATE" => {
                let code = body
                    .get("output")
                    .and_then(|o| o.get("code"))
                    .and_then(|c| c.as_str())
                    .unwrap_or("unknown");
                let msg = body
                    .get("output")
                    .and_then(|o| o.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("");
                return Err(format!("job {task_status} [{code}]: {msg}"));
            }
            // PENDING, RUNNING — keep polling with bounded backoff.
            _ => {
                thread::sleep(interval);
                interval = (interval * 2).min(Duration::from_secs(10));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Artifact URL extraction
// ---------------------------------------------------------------------------

fn extract_task_id(body: &Value) -> Option<String> {
    body.get("output")
        .and_then(|o| o.get("task_id"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
}

/// Find the first artifact URL in a generation `output` object across the known
/// DashScope response shapes.
fn extract_artifact_url(output: &Value) -> Option<String> {
    // 1. multimodal-generation: output.choices[].message.content[].{image|video|audio|url}
    if let Some(choices) = output.get("choices").and_then(|c| c.as_array()) {
        for choice in choices {
            if let Some(contents) = choice
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                for item in contents {
                    if let Some(u) = first_url(item, &["image", "video", "audio", "url"]) {
                        return Some(u);
                    }
                }
            }
        }
    }
    // 2. top-level single-field shapes (video synthesis, some image models).
    if let Some(u) = first_url(output, &["video_url", "image_url", "url"]) {
        return Some(u);
    }
    // 3. legacy results array.
    if let Some(results) = output.get("results").and_then(|r| r.as_array()) {
        for r in results {
            if let Some(u) = first_url(r, &["url", "video_url", "image_url"]) {
                return Some(u);
            }
        }
    }
    None
}

fn first_url(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| {
        value
            .get(*k)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    })
}

// ---------------------------------------------------------------------------
// Artifact download
// ---------------------------------------------------------------------------

/// Download a remote URL to a local file.
fn download_artifact(url: &str, dest: &Path) -> Result<(), String> {
    let response = ureq::get(url)
        .timeout(Duration::from_secs(120))
        .call()
        .map_err(describe_ureq_err)?;

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create output dir: {e}"))?;
    }

    let mut bytes = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("read download stream: {e}"))?;

    fs::write(dest, &bytes).map_err(|e| format!("write artifact: {e}"))?;
    Ok(())
}

/// Identifier used for the output directory when the endpoint is synchronous
/// and returns no task id.
fn local_job_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("sync-{nanos}")
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Execute a full generation job for the spec's mode: submit (and poll, when
/// asynchronous), then download the artifact.
///
/// * `native_base` — the DashScope native API base URL (no /compatible-mode).
/// * `api_key` — authentication token.
/// * `spec` — the model specification from the registry.
/// * `input` — the semantic input object (e.g. `{"prompt": "..."}`).
/// * `parameters` — optional overrides merged over `spec.default_parameters`.
/// * `workspace_root` — used to derive the output directory.
pub fn generate(
    native_base: &str,
    api_key: &str,
    spec: &GenerationModelSpec,
    input: &Value,
    parameters: Option<&Value>,
    workspace_root: &Path,
) -> GenerationResult {
    let start = Instant::now();
    let async_call = matches!(spec.mode, InvocationMode::Async);

    let body = serde_json::json!({
        "model": spec.model_id,
        "input": build_input(spec.input_format, input),
        "parameters": merge_parameters(spec, parameters),
    });

    // Step 1: Submit.
    let submitted = match post_request(native_base, api_key, spec, &body, async_call) {
        Ok(v) => v,
        Err(msg) => {
            return GenerationResult {
                task_id: String::new(),
                success: false,
                message: format!("Submission failed: {msg}"),
                local_path: None,
                remote_url: None,
                duration: start.elapsed(),
            };
        }
    };

    // Step 2: obtain the final `output` object (poll for async, else use as-is).
    let (task_id, output) = if async_call {
        let task_id = match extract_task_id(&submitted) {
            Some(t) => t,
            None => {
                return GenerationResult {
                    task_id: String::new(),
                    success: false,
                    message: format!(
                        "No task_id in async response: {}",
                        clip(&submitted.to_string(), 300)
                    ),
                    local_path: None,
                    remote_url: None,
                    duration: start.elapsed(),
                };
            }
        };
        let timeout = Duration::from_secs(spec.output_type.default_poll_timeout_secs());
        match poll_job(native_base, api_key, &task_id, timeout) {
            Ok(body) => {
                let output = body.get("output").cloned().unwrap_or(Value::Null);
                (task_id, output)
            }
            Err(msg) => {
                return GenerationResult {
                    task_id,
                    success: false,
                    message: format!("Polling failed: {msg}"),
                    local_path: None,
                    remote_url: None,
                    duration: start.elapsed(),
                };
            }
        }
    } else {
        let output = submitted.get("output").cloned().unwrap_or(Value::Null);
        (String::new(), output)
    };

    // Step 3: extract the artifact URL.
    let remote_url = extract_artifact_url(&output);

    let dir_id = if task_id.is_empty() {
        local_job_id()
    } else {
        task_id.clone()
    };
    let local_path = match &remote_url {
        Some(url) => {
            let ext = spec.output_type.file_extension();
            let dest = workspace_root
                .join(".velocity")
                .join("generations")
                .join(&dir_id)
                .join(format!("output.{ext}"));
            match download_artifact(url, &dest) {
                Ok(()) => Some(dest),
                Err(e) => {
                    log::warn!("generation: download failed for {dir_id}: {e}");
                    None
                }
            }
        }
        None => None,
    };

    let success = remote_url.is_some();
    if success {
        write_generation_meta(workspace_root, &dir_id, spec, input, remote_url.as_deref());
    }

    GenerationResult {
        task_id,
        success,
        message: if success {
            format!(
                "Generated {} via model '{}' ({} mode).",
                local_path
                    .as_ref()
                    .map(|p| format!("artifact at {}", p.display()))
                    .unwrap_or_else(|| "artifact (download pending)".to_string()),
                spec.model_id,
                if async_call { "async" } else { "sync" },
            )
        } else {
            format!(
                "No artifact URL found in response: {}",
                clip(&output.to_string(), 300)
            )
        },
        local_path,
        remote_url,
        duration: start.elapsed(),
    }
}

/// Write a meta.json sidecar for each generation, useful for history.
fn write_generation_meta(
    workspace_root: &Path,
    dir_id: &str,
    spec: &GenerationModelSpec,
    input: &Value,
    remote_url: Option<&str>,
) {
    let dir = workspace_root
        .join(".velocity")
        .join("generations")
        .join(dir_id);
    let _ = fs::create_dir_all(&dir);
    let meta = serde_json::json!({
        "id": dir_id,
        "model_id": spec.model_id,
        "label": spec.label,
        "mode": format!("{:?}", spec.mode).to_lowercase(),
        "input": input,
        "remote_url": remote_url,
        "timestamp": format!("{:?}", SystemTime::now()),
    });
    let _ = fs::write(
        dir.join("meta.json"),
        serde_json::to_string_pretty(&meta).unwrap_or_default(),
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generation::registry::OutputType;
    use serde_json::json;

    #[test]
    fn derive_native_base_strips_compatible_mode() {
        assert_eq!(
            derive_native_base(
                "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode"
            ),
            "https://token-plan.ap-southeast-1.maas.aliyuncs.com"
        );
    }

    #[test]
    fn derive_native_base_passthrough_for_non_compatible() {
        assert_eq!(
            derive_native_base("https://dashscope.aliyuncs.com"),
            "https://dashscope.aliyuncs.com"
        );
    }

    #[test]
    fn derive_native_base_handles_trailing_slash() {
        assert_eq!(
            derive_native_base("https://example.com/compatible-mode/"),
            "https://example.com"
        );
    }

    #[test]
    fn build_input_prompt_shape_passes_through() {
        let out = build_input(InputFormat::Prompt, &json!({ "prompt": "hi" }));
        assert_eq!(out, json!({ "prompt": "hi" }));
    }

    #[test]
    fn build_input_messages_shape_wraps_prompt() {
        let out = build_input(InputFormat::Messages, &json!({ "prompt": "a cat" }));
        assert_eq!(
            out["messages"][0]["content"][0]["text"].as_str(),
            Some("a cat")
        );
        assert_eq!(out["messages"][0]["role"].as_str(), Some("user"));
    }

    #[test]
    fn build_input_messages_preserves_explicit_messages() {
        let input = json!({ "messages": [{ "role": "user", "content": [{ "text": "x" }] }] });
        let out = build_input(InputFormat::Messages, &input);
        assert_eq!(out, input);
    }

    #[test]
    fn extract_artifact_url_reads_multimodal_choices() {
        let output = json!({
            "choices": [{
                "message": { "content": [{ "type": "image", "image": "https://x/y.png" }] }
            }]
        });
        assert_eq!(
            extract_artifact_url(&output).as_deref(),
            Some("https://x/y.png")
        );
    }

    #[test]
    fn extract_artifact_url_reads_video_url_and_results() {
        assert_eq!(
            extract_artifact_url(&json!({ "video_url": "https://x/v.mp4" })).as_deref(),
            Some("https://x/v.mp4")
        );
        assert_eq!(
            extract_artifact_url(&json!({ "results": [{ "url": "https://x/r.png" }] })).as_deref(),
            Some("https://x/r.png")
        );
    }

    #[test]
    fn extract_task_id_reads_output() {
        assert_eq!(
            extract_task_id(&json!({ "output": { "task_id": "abc" } })).as_deref(),
            Some("abc")
        );
    }

    #[test]
    fn merge_parameters_overrides_defaults() {
        let spec = GenerationModelSpec::builtins()
            .into_iter()
            .find(|s| s.model_id == "wan2.7-image")
            .unwrap();
        let merged = merge_parameters(&spec, Some(&json!({ "n": 3 })));
        assert_eq!(merged["n"], json!(3));
        assert_eq!(merged["size"], json!("1024*1024")); // default preserved
    }

    #[test]
    fn builtin_image_is_sync_messages_video_is_async_prompt() {
        let builtins = GenerationModelSpec::builtins();
        let img = builtins
            .iter()
            .find(|s| s.model_id == "wan2.7-image")
            .unwrap();
        assert_eq!(img.mode, InvocationMode::Sync);
        assert_eq!(img.input_format, InputFormat::Messages);
        assert_eq!(
            img.endpoint_path,
            "/api/v1/services/aigc/multimodal-generation/generation"
        );
        assert_eq!(img.output_type, OutputType::Image);

        let vid = builtins
            .iter()
            .find(|s| s.model_id == "happyhorse-1.1-t2v")
            .unwrap();
        assert_eq!(vid.mode, InvocationMode::Async);
        assert_eq!(vid.input_format, InputFormat::Prompt);
        assert_eq!(vid.output_type, OutputType::Video);
    }
}
