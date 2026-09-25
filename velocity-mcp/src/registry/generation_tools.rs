//! MCP tool handlers for the multimodal generation subsystem.
//!
//! These route the `generate_*`, `list_generation_models`, and
//! `register_generation_model` tools onto the generic submit → poll → download
//! client in [`crate::generation::client`], resolving credentials and the native
//! base URL from the workspace's Alibaba/Qwen provider settings.

use super::super::generation::client::{derive_native_base, generate};
use super::super::generation::registry::{
    load_registry, register_model, GenerationModelSpec, InputFormat, InvocationMode, OutputType,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::error::Error;
use std::path::Path;

/// Dispatch a generation tool call. Returns `Ok(None)` when `name` is not one
/// of the generation tools, so the caller can fall through to other handlers.
pub fn handle_generation_tool(
    root: &Path,
    name: &str,
    arguments: &Value,
) -> Result<Option<String>, Box<dyn Error>> {
    let out = match name {
        "generate_image" => handle_generate_image(root, arguments),
        "generate_video" => handle_generate_video(root, arguments),
        "generate_media" => handle_generate_media(root, arguments),
        "list_generation_models" => handle_list_models(root),
        "register_generation_model" => handle_register_model(root, arguments),
        _ => return Ok(None),
    };
    Ok(Some(out))
}

// ---------------------------------------------------------------------------
// Shortcut handlers (image / video)
// ---------------------------------------------------------------------------

const DEFAULT_VIDEO_MODEL: &str = "happyhorse-1.1-t2v";

/// Which backend an image request is routed to after provider inference.
#[derive(Debug, PartialEq, Eq)]
enum ImageBackend {
    /// A model registered in the generation registry: async native endpoint.
    Native,
    /// Cloudflare Workers AI (an '@cf/...' id or no model at all).
    Cloudflare,
}

/// Decide the image backend purely from the requested model and the registry.
/// A model that resolves to a registered generation spec uses its native
/// endpoint; everything else falls back to Cloudflare, preserving the
/// pre-existing default behaviour.
fn infer_image_backend(
    model_arg: Option<&str>,
    registry: &HashMap<String, GenerationModelSpec>,
) -> ImageBackend {
    match model_arg.and_then(|m| registry.get(m)) {
        Some(_) => ImageBackend::Native,
        None => ImageBackend::Cloudflare,
    }
}

fn handle_generate_image(root: &Path, args: &Value) -> String {
    let prompt = match args["prompt"].as_str() {
        Some(p) => p,
        None => return missing_arg("prompt"),
    };
    let registry = load_registry(root);
    let model_arg = args["model"].as_str();

    // Provider inference (see infer_image_backend).
    match infer_image_backend(model_arg, &registry) {
        ImageBackend::Native => {
            let spec = registry.get(model_arg.unwrap()).cloned().unwrap();
            let mut input = json!({ "prompt": prompt });
            if let Some(np) = args["negative_prompt"].as_str() {
                input["negative_prompt"] = json!(np);
            }
            let mut params = serde_json::Map::new();
            if let Some(size) = args["size"].as_str() {
                params.insert("size".to_string(), json!(size));
            }
            if let Some(n) = args["n"].as_i64() {
                params.insert("n".to_string(), json!(n));
            }
            let parameters = if params.is_empty() {
                None
            } else {
                Some(Value::Object(params))
            };
            run_generation(root, &spec, &input, parameters.as_ref())
        }
        ImageBackend::Cloudflare => {
            run_cloudflare_image(root, prompt, model_arg, args["output"].as_str())
        }
    }
}

/// Cloudflare Workers AI synchronous image path (delegates to the existing
/// editor::multimodal implementation). Returned as an in-band JSON verdict so
/// the dispatch audit records failures correctly.
fn run_cloudflare_image(
    root: &Path,
    prompt: &str,
    model: Option<&str>,
    output: Option<&str>,
) -> String {
    match crate::editor::multimodal::generate_image(root, prompt, model, output) {
        Ok(path) => json!({
            "success": true,
            "provider": "cloudflare",
            "local_path": path.display().to_string(),
            "message": format!("Saved generated image to {}", path.display()),
        })
        .to_string(),
        Err(e) => json!({
            "success": false,
            "provider": "cloudflare",
            "error": e,
        })
        .to_string(),
    }
}

fn handle_generate_video(root: &Path, args: &Value) -> String {
    let prompt = match args["prompt"].as_str() {
        Some(p) => p,
        None => return missing_arg("prompt"),
    };
    let registry = load_registry(root);
    let model_id = args["model"].as_str().unwrap_or(DEFAULT_VIDEO_MODEL);
    let spec = match registry.get(model_id) {
        Some(s) => s.clone(),
        None => return unknown_model(model_id, &registry),
    };

    let input = json!({ "prompt": prompt });

    let mut params = serde_json::Map::new();
    if let Some(d) = args["duration"].as_f64() {
        params.insert("duration".to_string(), json!(d));
    }
    if let Some(r) = args["resolution"].as_str() {
        params.insert("resolution".to_string(), json!(r));
    }
    let parameters = if params.is_empty() {
        None
    } else {
        Some(Value::Object(params))
    };

    run_generation(root, &spec, &input, parameters.as_ref())
}

// ---------------------------------------------------------------------------
// Generic / registry handlers
// ---------------------------------------------------------------------------

fn handle_generate_media(root: &Path, args: &Value) -> String {
    let model_id = match args["model_id"].as_str() {
        Some(m) => m,
        None => return missing_arg("model_id"),
    };
    let registry = load_registry(root);
    let spec = match registry.get(model_id) {
        Some(s) => s.clone(),
        None => return unknown_model(model_id, &registry),
    };

    let input = args["input"].clone();
    if !input.is_object() {
        return json!({
            "success": false,
            "error": "'input' must be a JSON object matching the model's input schema.",
        })
        .to_string();
    }
    if let Some(missing) = missing_required_fields(&spec, &input) {
        return json!({
            "success": false,
            "error": format!(
                "input is missing required field(s): {}",
                missing.join(", ")
            ),
            "input_schema": spec.input_schema,
        })
        .to_string();
    }

    let parameters = if args["parameters"].is_object() {
        Some(args["parameters"].clone())
    } else {
        None
    };

    run_generation(root, &spec, &input, parameters.as_ref())
}

fn handle_list_models(root: &Path) -> String {
    let registry = load_registry(root);
    let mut specs: Vec<&GenerationModelSpec> = registry.values().collect();
    specs.sort_by(|a, b| a.model_id.cmp(&b.model_id));

    let models: Vec<Value> = specs
        .iter()
        .map(|s| {
            json!({
                "model_id": s.model_id,
                "label": s.label,
                "task_type": s.task_type,
                "endpoint_path": s.endpoint_path,
                "output_type": output_type_str(s.output_type),
                "mode": format!("{:?}", s.mode).to_lowercase(),
                "input_format": format!("{:?}", s.input_format).to_lowercase(),
                "source": s.source,
                "input_schema": s.input_schema,
                "default_parameters": s.default_parameters,
            })
        })
        .collect();

    json!({ "success": true, "count": models.len(), "models": models }).to_string()
}

fn handle_register_model(root: &Path, args: &Value) -> String {
    let model_id = match args["model_id"].as_str() {
        Some(v) => v,
        None => return missing_arg("model_id"),
    };
    let label = match args["label"].as_str() {
        Some(v) => v,
        None => return missing_arg("label"),
    };
    let task_type = match args["task_type"].as_str() {
        Some(v) => v,
        None => return missing_arg("task_type"),
    };
    let endpoint_path = match args["endpoint_path"].as_str() {
        Some(v) => v,
        None => return missing_arg("endpoint_path"),
    };
    let output_type: OutputType = match args["output_type"].as_str() {
        Some("image") => OutputType::Image,
        Some("video") => OutputType::Video,
        Some("audio") => OutputType::Audio,
        Some("text") => OutputType::Text,
        _ => {
            return json!({
                "success": false,
                "error": "'output_type' must be one of: image, video, audio, text.",
            })
            .to_string()
        }
    };

    let input_schema = if args["input_schema"].is_object() {
        args["input_schema"].clone()
    } else {
        json!({ "type": "object" })
    };
    let default_parameters = if args["default_parameters"].is_object() {
        args["default_parameters"].clone()
    } else {
        json!({})
    };

    // Optional invocation semantics; default to async/prompt for generic models.
    let mode = match args["mode"].as_str() {
        Some("sync") => InvocationMode::Sync,
        _ => InvocationMode::Async,
    };
    let input_format = match args["input_format"].as_str() {
        Some("messages") => InputFormat::Messages,
        _ => InputFormat::Prompt,
    };

    let spec = GenerationModelSpec {
        model_id: model_id.to_string(),
        label: label.to_string(),
        task_type: task_type.to_string(),
        endpoint_path: endpoint_path.to_string(),
        input_schema,
        default_parameters,
        output_type,
        mode,
        input_format,
        source: "user".to_string(),
    };

    match register_model(root, spec) {
        Ok(msg) => json!({ "success": true, "message": msg }).to_string(),
        Err(e) => json!({ "success": false, "error": e }).to_string(),
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Resolve the API key and native (non-compatible-mode) base URL, then run the
/// full generation lifecycle. Operational failures are returned as an in-band
/// `{"success": false, ...}` payload so the dispatch audit records them.
fn run_generation(
    root: &Path,
    spec: &GenerationModelSpec,
    input: &Value,
    parameters: Option<&Value>,
) -> String {
    let root_buf = root.to_path_buf();
    let api_key = crate::agent::executor::dispatch::resolve_api_key(
        &root_buf,
        "alibaba",
        "DASHSCOPE_API_KEY",
    );
    if api_key.trim().is_empty() {
        return json!({
            "success": false,
            "error": "No Alibaba Qwen API key configured. Add your DASHSCOPE API key in \
                Settings → Providers (or set the DASHSCOPE_API_KEY environment variable) \
                and enable the Token Plan.",
        })
        .to_string();
    }
    let base = crate::agent::executor::dispatch::alibaba_base_url(&root_buf);
    let native_base = derive_native_base(&base);

    let result = generate(&native_base, &api_key, spec, input, parameters, root);
    json!({
        "success": result.success,
        "task_id": result.task_id,
        "model_id": spec.model_id,
        "output_type": output_type_str(spec.output_type),
        "message": result.message,
        "local_path": result.local_path.map(|p| p.display().to_string()),
        "remote_url": result.remote_url,
        "duration_secs": (result.duration.as_secs_f64() * 100.0).round() / 100.0,
    })
    .to_string()
}

/// Return the required `input` properties (from the spec's JSON Schema) that
/// are absent or null in the supplied input. `None` when everything is present
/// or the schema declares no required fields.
fn missing_required_fields(spec: &GenerationModelSpec, input: &Value) -> Option<Vec<String>> {
    let required = spec.input_schema.get("required")?.as_array()?;
    let missing: Vec<String> = required
        .iter()
        .filter_map(|r| r.as_str())
        .filter(|key| match input.get(*key) {
            None => true,
            Some(v) => v.is_null(),
        })
        .map(|s| s.to_string())
        .collect();
    if missing.is_empty() {
        None
    } else {
        Some(missing)
    }
}

fn output_type_str(ot: OutputType) -> &'static str {
    match ot {
        OutputType::Image => "image",
        OutputType::Video => "video",
        OutputType::Audio => "audio",
        OutputType::Text => "text",
    }
}

fn missing_arg(field: &str) -> String {
    json!({ "success": false, "error": format!("'{field}' is required.") }).to_string()
}

fn unknown_model(model_id: &str, registry: &HashMap<String, GenerationModelSpec>) -> String {
    let mut available: Vec<&String> = registry.keys().collect();
    available.sort();
    json!({
        "success": false,
        "error": format!("unknown generation model '{model_id}'"),
        "available": available,
        "hint": "Call list_generation_models to see the full registry or \
            register_generation_model to add one.",
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_tool_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let out = handle_generation_tool(dir.path(), "not_a_generation_tool", &json!({}));
        assert!(matches!(out, Ok(None)));
    }

    #[test]
    fn list_models_includes_builtins() {
        let dir = tempfile::tempdir().unwrap();
        let out = handle_list_models(dir.path());
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["success"], json!(true));
        let models = parsed["models"].as_array().unwrap();
        let ids: Vec<&str> = models
            .iter()
            .map(|m| m["model_id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"wan2.7-image"));
        assert!(ids.contains(&"happyhorse-1.1-t2v"));
    }

    #[test]
    fn register_then_list_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let reg = handle_register_model(
            dir.path(),
            &json!({
                "model_id": "my-model",
                "label": "My Model",
                "task_type": "text2image",
                "endpoint_path": "/api/v1/services/aigc/text2image/image-synthesis",
                "output_type": "image",
                "input_schema": {"type": "object", "properties": {"prompt": {"type": "string"}}, "required": ["prompt"]}
            }),
        );
        let parsed: Value = serde_json::from_str(&reg).unwrap();
        assert_eq!(parsed["success"], json!(true), "{reg}");

        let listed: Value = serde_json::from_str(&handle_list_models(dir.path())).unwrap();
        let ids: Vec<&str> = listed["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["model_id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"my-model"));
    }

    #[test]
    fn generate_media_rejects_missing_required_input() {
        let dir = tempfile::tempdir().unwrap();
        let out = handle_generate_media(
            dir.path(),
            &json!({ "model_id": "wan2.7-image", "input": {} }),
        );
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["success"], json!(false));
        assert!(
            parsed["error"].as_str().unwrap().contains("required field"),
            "{out}"
        );
    }

    #[test]
    fn generate_media_unknown_model_lists_available() {
        let dir = tempfile::tempdir().unwrap();
        let out = handle_generate_media(
            dir.path(),
            &json!({ "model_id": "nope", "input": {"prompt": "x"} }),
        );
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["success"], json!(false));
        assert!(parsed["available"].as_array().unwrap().len() >= 2);
    }

    #[test]
    fn image_backend_infers_native_for_registered_model() {
        let dir = tempfile::tempdir().unwrap();
        let registry = load_registry(dir.path());
        assert_eq!(
            infer_image_backend(Some("wan2.7-image"), &registry),
            ImageBackend::Native
        );
    }

    #[test]
    fn image_backend_falls_back_to_cloudflare() {
        let dir = tempfile::tempdir().unwrap();
        let registry = load_registry(dir.path());
        // No model, a Cloudflare id, and an unknown id all route to Cloudflare.
        assert_eq!(
            infer_image_backend(None, &registry),
            ImageBackend::Cloudflare
        );
        assert_eq!(
            infer_image_backend(
                Some("@cf/stabilityai/stable-diffusion-xl-base-1.0"),
                &registry
            ),
            ImageBackend::Cloudflare
        );
        assert_eq!(
            infer_image_backend(Some("not-a-registered-model"), &registry),
            ImageBackend::Cloudflare
        );
    }
}
