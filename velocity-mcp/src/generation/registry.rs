//! Generation model registry: specs describing how to call each multimodal
//! generation endpoint, persisted at `.velocity/generation_models.json`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Output type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputType {
    Image,
    Video,
    Audio,
    Text,
}

impl OutputType {
    pub fn file_extension(self) -> &'static str {
        match self {
            OutputType::Image => "png",
            OutputType::Video => "mp4",
            OutputType::Audio => "wav",
            OutputType::Text => "txt",
        }
    }

    pub fn default_poll_timeout_secs(self) -> u64 {
        match self {
            OutputType::Image => 120,
            OutputType::Video => 300,
            OutputType::Audio => 60,
            OutputType::Text => 30,
        }
    }
}

// ---------------------------------------------------------------------------
// Invocation mode & input format
// ---------------------------------------------------------------------------

/// How the endpoint returns its artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InvocationMode {
    /// Synchronous: the POST response embeds the finished artifact URL. Used by
    /// `multimodal-generation/generation` (e.g. wan2.7-image) on plans that do
    /// not permit asynchronous calls.
    Sync,
    /// Asynchronous job submission: POST returns a `task_id`, which must be
    /// polled at `/api/v1/tasks/{id}` until SUCCEEDED. Used by video synthesis.
    Async,
}

/// Shape of the request `input` object the endpoint expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputFormat {
    /// `{ "prompt": "..." }` — classic text2image / video-synthesis shape.
    Prompt,
    /// `{ "messages": [{ "role": "user", "content": [{ "text": "..." }] }] }`
    /// — the multimodal-generation chat-style shape.
    Messages,
}

fn default_mode() -> InvocationMode {
    InvocationMode::Async
}

fn default_input_format() -> InputFormat {
    InputFormat::Prompt
}

// ---------------------------------------------------------------------------
// Model spec
// ---------------------------------------------------------------------------

/// Describes one generation-capable model: where to POST the request, what
/// input shape it expects, and what kind of artifact comes back.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationModelSpec {
    /// Model identifier as sent in the API request body (e.g. "wan2.7-image").
    pub model_id: String,
    /// Human-readable label shown in tool output.
    pub label: String,
    /// Task category (e.g. "text2image", "video-generation", "multimodal-generation").
    pub task_type: String,
    /// Full endpoint path appended to the native base URL
    /// (e.g. "/api/v1/services/aigc/text2image/image-synthesis").
    pub endpoint_path: String,
    /// JSON Schema describing the `input` object the model accepts.
    pub input_schema: Value,
    /// Default `parameters` merged into every request unless overridden.
    #[serde(default)]
    pub default_parameters: Value,
    /// What kind of artifact the model produces.
    pub output_type: OutputType,
    /// Synchronous vs asynchronous job submission.
    #[serde(default = "default_mode")]
    pub mode: InvocationMode,
    /// Shape of the `input` object the endpoint expects.
    #[serde(default = "default_input_format")]
    pub input_format: InputFormat,
    /// Origin of this entry: "builtin", "user", or "discovered".
    #[serde(default = "default_source")]
    pub source: String,
}

fn default_source() -> String {
    "builtin".to_string()
}

impl GenerationModelSpec {
    /// Built-in specs shipped with the binary.
    pub fn builtins() -> Vec<Self> {
        vec![
            GenerationModelSpec {
                model_id: "wan2.7-image".to_string(),
                label: "Wan 2.7 Text-to-Image".to_string(),
                task_type: "multimodal-generation".to_string(),
                endpoint_path: "/api/v1/services/aigc/multimodal-generation/generation".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "prompt": { "type": "string", "description": "Text description of the image to generate." },
                        "negative_prompt": { "type": "string", "description": "Things to avoid in the image (optional)." }
                    },
                    "required": ["prompt"]
                }),
                default_parameters: serde_json::json!({
                    "size": "1024*1024",
                    "n": 1
                }),
                output_type: OutputType::Image,
                mode: InvocationMode::Sync,
                input_format: InputFormat::Messages,
                source: "builtin".to_string(),
            },
            GenerationModelSpec {
                model_id: "happyhorse-1.1-t2v".to_string(),
                label: "HappyHorse 1.1 Text-to-Video".to_string(),
                task_type: "video-generation".to_string(),
                endpoint_path: "/api/v1/services/aigc/video-generation/video-synthesis".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "prompt": { "type": "string", "description": "Text description of the video to generate." }
                    },
                    "required": ["prompt"]
                }),
                default_parameters: serde_json::json!({}),
                output_type: OutputType::Video,
                mode: InvocationMode::Async,
                input_format: InputFormat::Prompt,
                source: "builtin".to_string(),
            },
        ]
    }
}

// ---------------------------------------------------------------------------
// Registry file format
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GenerationRegistryFile {
    #[serde(default)]
    pub models: HashMap<String, GenerationModelSpec>,
}

// ---------------------------------------------------------------------------
// Load / save
// ---------------------------------------------------------------------------

fn registry_path(workspace_root: &Path) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("generation_models.json")
}

/// Load the merged registry: builtins overlaid by user-registered models.
/// Later entries win when model_ids collide.
pub fn load_registry(workspace_root: &Path) -> HashMap<String, GenerationModelSpec> {
    let mut map: HashMap<String, GenerationModelSpec> = GenerationModelSpec::builtins()
        .into_iter()
        .map(|s| (s.model_id.clone(), s))
        .collect();

    let path = registry_path(workspace_root);
    if let Ok(contents) = fs::read_to_string(&path) {
        let stripped = contents.strip_prefix('\u{feff}').unwrap_or(&contents);
        if let Ok(file) = serde_json::from_str::<GenerationRegistryFile>(stripped) {
            for (id, spec) in file.models {
                map.insert(id, spec);
            }
        }
    }

    map
}

/// Persist user-added/modified specs (excludes builtins that haven't been
/// overridden) back to the registry file.
pub fn save_registry(
    workspace_root: &Path,
    models: &HashMap<String, GenerationModelSpec>,
) -> Result<(), String> {
    let path = registry_path(workspace_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create .velocity dir: {e}"))?;
    }
    // Only persist non-builtin entries (or builtins that were modified).
    let builtin_ids: Vec<String> = GenerationModelSpec::builtins()
        .into_iter()
        .map(|s| s.model_id)
        .collect();
    let user_models: HashMap<String, &GenerationModelSpec> = models
        .iter()
        .filter(|(id, spec)| !builtin_ids.contains(id) || spec.source != "builtin")
        .map(|(id, spec)| (id.clone(), spec))
        .collect();

    let file = GenerationRegistryFile {
        models: user_models
            .into_iter()
            .map(|(id, spec)| (id, spec.clone()))
            .collect(),
    };
    let json =
        serde_json::to_string_pretty(&file).map_err(|e| format!("serialize registry: {e}"))?;
    fs::write(&path, json).map_err(|e| format!("write registry: {e}"))?;
    Ok(())
}

/// Register a new model spec into the workspace registry.
pub fn register_model(workspace_root: &Path, spec: GenerationModelSpec) -> Result<String, String> {
    let mut models = load_registry(workspace_root);
    let id = spec.model_id.clone();
    models.insert(id.clone(), spec);
    save_registry(workspace_root, &models)?;
    Ok(format!("Registered generation model '{id}'."))
}

/// Remove a user-registered model from the workspace registry.
pub fn unregister_model(workspace_root: &Path, model_id: &str) -> Result<String, String> {
    let mut models = load_registry(workspace_root);
    if models.remove(model_id).is_some() {
        save_registry(workspace_root, &models)?;
        Ok(format!("Removed generation model '{model_id}'."))
    } else {
        Err(format!("No generation model '{model_id}' found."))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_load_without_workspace_file() {
        let dir = tempfile::tempdir().unwrap();
        let models = load_registry(dir.path());
        assert!(models.contains_key("wan2.7-image"));
        assert!(models.contains_key("happyhorse-1.1-t2v"));
    }

    #[test]
    fn register_and_persist_user_model() {
        let dir = tempfile::tempdir().unwrap();
        let spec = GenerationModelSpec {
            model_id: "custom-artist".to_string(),
            label: "Custom Artist".to_string(),
            task_type: "text2image".to_string(),
            endpoint_path: "/api/v1/services/aigc/custom/generate".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
            default_parameters: serde_json::json!({}),
            output_type: OutputType::Image,
            mode: InvocationMode::Async,
            input_format: InputFormat::Prompt,
            source: "user".to_string(),
        };
        register_model(dir.path(), spec).unwrap();

        let models = load_registry(dir.path());
        assert!(models.contains_key("custom-artist"));
        assert_eq!(models["custom-artist"].label, "Custom Artist");
    }

    #[test]
    fn unregister_removes_user_model() {
        let dir = tempfile::tempdir().unwrap();
        let spec = GenerationModelSpec {
            model_id: "temp-model".to_string(),
            label: "Temp".to_string(),
            task_type: "text2image".to_string(),
            endpoint_path: "/api/v1/services/aigc/test/gen".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
            default_parameters: serde_json::json!({}),
            output_type: OutputType::Image,
            mode: InvocationMode::Async,
            input_format: InputFormat::Prompt,
            source: "user".to_string(),
        };
        register_model(dir.path(), spec).unwrap();
        assert!(load_registry(dir.path()).contains_key("temp-model"));

        unregister_model(dir.path(), "temp-model").unwrap();
        assert!(!load_registry(dir.path()).contains_key("temp-model"));
    }

    #[test]
    fn output_type_extensions() {
        assert_eq!(OutputType::Image.file_extension(), "png");
        assert_eq!(OutputType::Video.file_extension(), "mp4");
    }
}
