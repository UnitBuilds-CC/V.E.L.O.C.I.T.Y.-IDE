//! Unified API client supporting multiple LLM providers.

use anyhow::{Context, Result};
use std::io::{BufRead, BufReader};

use super::config::{AppConfig, Provider, ProviderConfig};

/// A chat message.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Send a chat completion request to the active provider.
pub fn chat_completion(config: &AppConfig, messages: &[ChatMessage]) -> Result<String> {
    let provider_cfg = config
        .active_provider_config()
        .context("No active provider configured")?;

    match &provider_cfg.provider {
        p if p.is_anthropic_format() => anthropic_request(provider_cfg, messages),
        p if p.is_cloudflare_format() => cloudflare_request(provider_cfg, messages),
        p if p.is_google_format() => google_request(provider_cfg, messages),
        _ => openai_compatible_request(provider_cfg, messages),
    }
}

/// Stream a chat completion, calling `on_chunk` for each text fragment.
pub fn chat_completion_stream(
    config: &AppConfig,
    messages: &[ChatMessage],
    mut on_chunk: impl FnMut(&str),
) -> Result<String> {
    let provider_cfg = config
        .active_provider_config()
        .context("No active provider configured")?;

    let full = match &provider_cfg.provider {
        p if p.is_anthropic_format() => anthropic_stream(provider_cfg, messages, &mut on_chunk)?,
        p if p.is_cloudflare_format() => cloudflare_stream(provider_cfg, messages, &mut on_chunk)?,
        p if p.is_google_format() => google_request(provider_cfg, messages)?,
        _ => openai_compatible_stream(provider_cfg, messages, &mut on_chunk)?,
    };
    Ok(full)
}

// ─── OpenAI-compatible (OpenAI, OpenRouter, Moonshot, Alibaba, Mistral, Groq, Together, DeepSeek)

fn openai_compatible_request(cfg: &ProviderConfig, messages: &[ChatMessage]) -> Result<String> {
    let base = cfg
        .base_url
        .as_deref()
        .or_else(|| cfg.provider.default_base_url())
        .context("No base URL configured for this provider")?;
    let url = format!("{}/chat/completions", base.trim_end_matches('/'));

    let body = serde_json::json!({
        "model": cfg.model,
        "messages": messages,
        "stream": false,
    });

    let resp: serde_json::Value = ureq::post(&url)
        .set("Authorization", &format!("Bearer {}", cfg.api_key))
        .set("Content-Type", "application/json")
        .send_json(&body)
        .context("API request failed")?
        .into_json()
        .context("Failed to parse API response")?;

    resp["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .context("Missing content in API response")
}

fn openai_compatible_stream(
    cfg: &ProviderConfig,
    messages: &[ChatMessage],
    on_chunk: &mut dyn FnMut(&str),
) -> Result<String> {
    let base = cfg
        .base_url
        .as_deref()
        .or_else(|| cfg.provider.default_base_url())
        .context("No base URL configured for this provider")?;
    let url = format!("{}/chat/completions", base.trim_end_matches('/'));

    let body = serde_json::json!({
        "model": cfg.model,
        "messages": messages,
        "stream": true,
    });

    let resp = ureq::post(&url)
        .set("Authorization", &format!("Bearer {}", cfg.api_key))
        .set("Content-Type", "application/json")
        .send_json(&body)
        .context("Streaming request failed")?;

    parse_sse_stream(resp, on_chunk, |val| {
        val["choices"]
            .get(0)
            .and_then(|c| c["delta"].as_object())
            .and_then(|d| d.get("content"))
            .and_then(|c| c.as_str())
            .map(|s| s.to_string())
    })
}

// ─── Anthropic ─────────────────────────────────────────────────────────────

fn anthropic_request(cfg: &ProviderConfig, messages: &[ChatMessage]) -> Result<String> {
    let base = cfg
        .base_url
        .as_deref()
        .unwrap_or("https://api.anthropic.com/v1");
    let url = format!("{}/messages", base.trim_end_matches('/'));

    // Separate system message
    let system_msg = messages
        .iter()
        .find(|m| m.role == "system")
        .map(|m| m.content.as_str());
    let non_system: Vec<_> = messages.iter().filter(|m| m.role != "system").collect();

    let mut body = serde_json::json!({
        "model": cfg.model,
        "max_tokens": 4096,
        "messages": non_system,
    });

    if let Some(sys) = system_msg {
        body["system"] = serde_json::Value::String(sys.to_string());
    }

    let resp: serde_json::Value = ureq::post(&url)
        .set("x-api-key", &cfg.api_key)
        .set("anthropic-version", "2023-06-01")
        .set("Content-Type", "application/json")
        .send_json(&body)
        .context("Anthropic API request failed")?
        .into_json()
        .context("Failed to parse Anthropic response")?;

    resp["content"][0]["text"]
        .as_str()
        .map(|s| s.to_string())
        .context("Missing content in Anthropic response")
}

fn anthropic_stream(
    cfg: &ProviderConfig,
    messages: &[ChatMessage],
    on_chunk: &mut dyn FnMut(&str),
) -> Result<String> {
    // Anthropic streaming uses SSE with content_block_delta events
    // For simplicity, fall back to non-streaming
    anthropic_request(cfg, messages)
}

// ─── Cloudflare Workers AI ─────────────────────────────────────────────────

fn cloudflare_request(cfg: &ProviderConfig, messages: &[ChatMessage]) -> Result<String> {
    let account_id = cfg
        .account_id
        .as_deref()
        .context("Cloudflare requires an Account ID")?;
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/ai/v1/chat/completions",
        account_id
    );

    let body = serde_json::json!({
        "model": cfg.model,
        "messages": messages,
        "stream": false,
    });

    let resp: serde_json::Value = ureq::post(&url)
        .set("Authorization", &format!("Bearer {}", cfg.api_key))
        .set("Content-Type", "application/json")
        .send_json(&body)
        .context("Cloudflare API request failed")?
        .into_json()
        .context("Failed to parse Cloudflare response")?;

    // Cloudflare can return result in different formats
    if let Some(content) = resp["result"]["response"].as_str() {
        return Ok(content.to_string());
    }
    resp["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .context("Missing content in Cloudflare response")
}

fn cloudflare_stream(
    cfg: &ProviderConfig,
    messages: &[ChatMessage],
    on_chunk: &mut dyn FnMut(&str),
) -> Result<String> {
    let account_id = cfg
        .account_id
        .as_deref()
        .context("Cloudflare requires an Account ID")?;
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/ai/v1/chat/completions",
        account_id
    );

    let body = serde_json::json!({
        "model": cfg.model,
        "messages": messages,
        "stream": true,
    });

    let resp = ureq::post(&url)
        .set("Authorization", &format!("Bearer {}", cfg.api_key))
        .set("Content-Type", "application/json")
        .send_json(&body)
        .context("Cloudflare streaming request failed")?;

    parse_sse_stream(resp, on_chunk, |val| {
        // Try multiple response formats
        val["choices"]
            .get(0)
            .and_then(|c| c["delta"].as_object())
            .and_then(|d| d.get("content"))
            .and_then(|c| c.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                val.get("response")
                    .and_then(|r| r.as_str())
                    .map(|s| s.to_string())
            })
            .or_else(|| {
                val["result"]
                    .get("response")
                    .and_then(|r| r.as_str())
                    .map(|s| s.to_string())
            })
    })
}

// ─── Google Gemini ──────────────────────────────────────────────────────

fn google_request(cfg: &ProviderConfig, messages: &[ChatMessage]) -> Result<String> {
    let base = cfg
        .base_url
        .as_deref()
        .or_else(|| cfg.provider.default_base_url())
        .context("No base URL configured for Google Gemini")?;
    let url = format!(
        "{}/models/{}:generateContent?key={}",
        base.trim_end_matches('/'),
        cfg.model,
        cfg.api_key
    );

    // Convert messages to Gemini format
    let contents: Vec<serde_json::Value> = messages
        .iter()
        .filter(|m| m.role != "system")
        .map(|m| {
            serde_json::json!({
                "role": if m.role == "assistant" { "model" } else { "user" },
                "parts": [{ "text": m.content }]
            })
        })
        .collect();

    let mut body = serde_json::json!({ "contents": contents });

    // Add system instruction if present
    if let Some(sys) = messages.iter().find(|m| m.role == "system") {
        body["systemInstruction"] = serde_json::json!({
            "parts": [{ "text": sys.content }]
        });
    }

    let resp: serde_json::Value = ureq::post(&url)
        .set("Content-Type", "application/json")
        .send_json(&body)
        .context("Google Gemini API request failed")?
        .into_json()
        .context("Failed to parse Google Gemini response")?;

    resp["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .map(|s| s.to_string())
        .context("Missing content in Google Gemini response")
}

// ─── SSE stream parser ─────────────────────────────────────────────────────

fn parse_sse_stream(
    resp: ureq::Response,
    on_chunk: &mut dyn FnMut(&str),
    extract: impl Fn(&serde_json::Value) -> Option<String>,
) -> Result<String> {
    let mut reader = BufReader::new(resp.into_reader());
    let mut full_response = String::new();
    let mut line = String::new();

    while reader.read_line(&mut line).is_ok() {
        if line.is_empty() {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "data: [DONE]" {
            line.clear();
            continue;
        }
        if trimmed.starts_with("data: ") {
            let data_str = &trimmed[6..];
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(data_str) {
                if let Some(chunk) = extract(&val) {
                    if !chunk.is_empty() {
                        on_chunk(&chunk);
                        full_response.push_str(&chunk);
                    }
                }
            }
        }
        line.clear();
    }

    Ok(full_response)
}
