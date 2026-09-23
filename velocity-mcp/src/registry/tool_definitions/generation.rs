use crate::registry::types::Tool;
use serde_json::json;

pub fn get_generation_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "generate_image".to_string(),
            description: "Generate an image from a text prompt. The provider is inferred from the selected model: a model \
                registered in the generation registry (see list_generation_models, e.g. 'wan2.7-image') is routed through \
                its native async generation endpoint; an '@cf/...' model id or no model at all falls back to Cloudflare \
                Workers AI (default stable-diffusion-xl). Returns the saved file path, plus the remote URL for native models."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "prompt": { "type": "string", "description": "Text description of the image to generate." },
                    "model": { "type": "string", "description": "Model id. If it matches a registered generation model the native provider is used; a '@cf/...' id or omission uses Cloudflare Workers AI." },
                    "negative_prompt": { "type": "string", "description": "Things to avoid in the image (native models only; optional)." },
                    "size": { "type": "string", "description": "Image size as 'WxH' (e.g. '1024*1024'); native models only, optional." },
                    "n": { "type": "integer", "description": "Number of images to generate (1-4); native models only, optional." },
                    "output": { "type": "string", "description": "Workspace-relative output path for the Cloudflare path (e.g. generated/logo.png). Ignored for native models." }
                },
                "required": ["prompt"]
            }),
        },
        Tool {
            name: "generate_video".to_string(),
            description: "Generate a video from a text prompt using a text-to-video model (e.g. happyhorse-1.1-t2v). \
                Submits an async job, polls until the video is ready, and downloads the MP4. Video generation may \
                take 1-5 minutes. Returns the local file path and remote URL."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "prompt": { "type": "string", "description": "Text description of the video to generate." },
                    "model": { "type": "string", "description": "Override model ID. Defaults to 'happyhorse-1.1-t2v'." },
                    "duration": { "type": "number", "description": "Desired video duration in seconds (if model supports it). Optional." },
                    "resolution": { "type": "string", "description": "Video resolution (e.g. '1280x720'). Optional." }
                },
                "required": ["prompt"]
            }),
        },
        Tool {
            name: "generate_media".to_string(),
            description: "Generic multimodal generation: submit a generation request to any registered model with \
                arbitrary input/parameters. Use list_generation_models first to discover available models and \
                their expected input schemas. Supports future models without code changes."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "model_id": { "type": "string", "description": "The generation model to use (must be in the registry)." },
                    "input": { "type": "object", "description": "The model's input object (prompt, etc.). Shape depends on model." },
                    "parameters": { "type": "object", "description": "Optional parameter overrides merged over model defaults." }
                },
                "required": ["model_id", "input"]
            }),
        },
        Tool {
            name: "list_generation_models".to_string(),
            description: "List all available multimodal generation models with their input schemas, default \
                parameters, and output types. Use this to discover what models are available before calling \
                generate_media."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        Tool {
            name: "register_generation_model".to_string(),
            description: "Register a new multimodal generation model in the workspace registry so it becomes \
                available via generate_media. Provide the model_id, API endpoint path, input schema, and output type."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "model_id": { "type": "string", "description": "The model identifier as the API expects it." },
                    "label": { "type": "string", "description": "Human-readable display name." },
                    "task_type": { "type": "string", "description": "Task category (e.g. 'text2image', 'video-generation', 'multimodal-generation')." },
                    "endpoint_path": { "type": "string", "description": "Full API path (e.g. '/api/v1/services/aigc/text2image/image-synthesis')." },
                    "output_type": { "type": "string", "enum": ["image", "video", "audio", "text"], "description": "Type of artifact produced." },
                    "mode": { "type": "string", "enum": ["sync", "async"], "description": "Invocation mode. 'sync' returns the artifact URL inline (multimodal-generation); 'async' submits a job and polls (video-synthesis). Defaults to 'async'." },
                    "input_format": { "type": "string", "enum": ["prompt", "messages"], "description": "Shape of the input object: 'prompt' = {prompt}; 'messages' = chat-style {messages:[...]}. Defaults to 'prompt'." },
                    "input_schema": { "type": "object", "description": "JSON Schema for the model's input object." },
                    "default_parameters": { "type": "object", "description": "Default parameters sent with every request (optional)." }
                },
                "required": ["model_id", "label", "task_type", "endpoint_path", "output_type", "input_schema"]
            }),
        },
    ]
}
