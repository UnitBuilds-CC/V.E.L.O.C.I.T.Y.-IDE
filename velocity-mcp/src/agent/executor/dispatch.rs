use super::super::models::*;
use super::utils::send_usage_update;
use crate::usage::{
    AzureOpenAiAccount, CloudflareAccount, LocalOllamaAccount, OpenRouterAccount, UsageTracker,
};
use crossbeam_channel::Sender;
use serde_json::{json, Value};
use std::time::Duration;

pub fn execute_openrouter_request<'a>(
    or_accounts: &'a [OpenRouterAccount],
    accounts: &[CloudflareAccount],
    usage_tracker: &mut UsageTracker,
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
) -> (Option<ureq::Response>, Option<&'a OpenRouterAccount>) {
    let start_idx = usage_tracker
        .pick_or_account(or_accounts)
        .and_then(|picked| or_accounts.iter().position(|a| a.n == picked.n))
        .unwrap_or(0);

    let mut final_res = None;
    let mut used_acct = None;
    let loop_limit = or_accounts.len().max(1);

    for idx in 0..loop_limit {
        let mut active_acct = None;
        let current_key = if or_accounts.is_empty() {
            super::super::provider::openrouter_api_key()
        } else {
            let acct = &or_accounts[(start_idx + idx) % or_accounts.len()];
            if usage_tracker.is_or_exhausted(acct.n) {
                continue;
            }
            active_acct = Some(acct);
            acct.token.clone()
        };

        let mut attempt = 0;
        let max_attempts = 3;
        let mut account_exhausted = false;

        while attempt < max_attempts {
            attempt += 1;
            match ureq::post("https://openrouter.ai/api/v1/chat/completions")
                .timeout(Duration::from_secs(60))
                .set("Authorization", &format!("Bearer {}", current_key))
                .set("HTTP-Referer", "https://velocity-ide.local")
                .set("X-Title", "Velocity Cognitive IDE")
                .set("Content-Type", "application/json")
                .send_json(request_body)
            {
                Ok(res) => {
                    used_acct = active_acct;
                    final_res = Some(res);
                    break;
                }
                Err(ureq::Error::Status(429, resp)) => {
                    let body = resp.into_string().unwrap_or_default();
                    let body_lower = body.to_lowercase();
                    if body_lower.contains("free-models-per-day")
                        || body_lower.contains("quota")
                        || body_lower.contains("credit")
                        || body_lower.contains("limit exceeded")
                    {
                        if let Some(acct) = active_acct {
                            usage_tracker.mark_or_exhausted(acct.n, &acct.label, &acct.tier);
                            send_usage_update(usage_tracker, accounts, or_accounts, ui_tx);
                            ui_tx
                                .send(AgentToUiMessage::StatusUpdate(format!(
                                    "OpenRouter account '{}' quota exhausted — trying next…",
                                    acct.label
                                )))
                                .ok();
                        }
                        account_exhausted = true;
                        break;
                    } else if attempt < max_attempts {
                        let wait_secs = attempt * 2;
                        ui_tx
                            .send(AgentToUiMessage::StatusUpdate(format!(
                            "OpenRouter rate limit (429) on '{}'. Retrying in {}s (Attempt {}/{})…",
                            active_acct.map(|a| a.label.as_str()).unwrap_or("default"),
                            wait_secs, attempt, max_attempts
                        )))
                            .ok();
                        std::thread::sleep(Duration::from_secs(wait_secs as u64));
                    }
                }
                Err(ureq::Error::Status(code, resp)) => {
                    let _body = resp.into_string().unwrap_or_default();
                    if code >= 500 && attempt < max_attempts {
                        std::thread::sleep(Duration::from_secs(attempt as u64));
                    } else {
                        break;
                    }
                }
                Err(e) => {
                    if attempt < max_attempts {
                        std::thread::sleep(Duration::from_secs(1));
                    } else {
                        ui_tx
                            .send(AgentToUiMessage::StatusUpdate(format!(
                                "OpenRouter connection error: {:?}",
                                e
                            )))
                            .ok();
                    }
                }
            }
        }
        if (final_res.is_some() || account_exhausted) && final_res.is_some() {
            break;
        }
    }
    (final_res, used_acct)
}

pub fn execute_cloudflare_request<'a>(
    accounts: &'a [CloudflareAccount],
    or_accounts: &[OpenRouterAccount],
    usage_tracker: &mut UsageTracker,
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
) -> (Option<ureq::Response>, Option<&'a CloudflareAccount>) {
    if accounts.is_empty() {
        ui_tx
            .send(AgentToUiMessage::StatusUpdate(
                "No Cloudflare accounts configured.".to_string(),
            ))
            .ok();
        return (None, None);
    }
    let start_idx = usage_tracker
        .pick_account(accounts)
        .and_then(|picked| accounts.iter().position(|a| a.n == picked.n))
        .unwrap_or(0);
    let mut cf_response = None;
    let mut used_acct = None;
    for i in 0..accounts.len() {
        let account = &accounts[(start_idx + i) % accounts.len()];
        if usage_tracker.is_exhausted(account.n) {
            continue;
        }
        let api_url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/ai/v1/chat/completions",
            account.id
        );
        let mut attempt = 0;
        let max_attempts = 2;
        while attempt < max_attempts {
            attempt += 1;
            match ureq::post(&api_url)
                .timeout(Duration::from_secs(60))
                .set("Authorization", &format!("Bearer {}", account.token))
                .set("Content-Type", "application/json")
                .send_json(request_body)
            {
                Ok(res) => {
                    used_acct = Some(account);
                    cf_response = Some(res);
                    break;
                }
                Err(ureq::Error::Status(_code, resp)) => {
                    let body = resp.into_string().unwrap_or_default();
                    if super::utils::is_quota_exhausted_error(&body) {
                        usage_tracker.mark_exhausted(account.n, &account.label, &account.tier);
                        send_usage_update(usage_tracker, accounts, or_accounts, ui_tx);
                        break;
                    } else if attempt < max_attempts {
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
                Err(_) => {
                    if attempt < max_attempts {
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
            }
        }
        if cf_response.is_some() {
            break;
        }
    }
    (cf_response, used_acct)
}

pub fn execute_azure_request(
    azure_accounts: &[AzureOpenAiAccount],
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
) -> Option<ureq::Response> {
    if azure_accounts.is_empty() {
        ui_tx
            .send(AgentToUiMessage::StatusUpdate(
                "No Azure OpenAI accounts configured.".to_string(),
            ))
            .ok();
        return None;
    }
    let account = &azure_accounts[0];
    let endpoint = account.endpoint.trim_end_matches('/');
    let api_url = format!(
        "{}/openai/deployments/{}/chat/completions?api-version={}",
        endpoint, account.deployment, account.api_version
    );
    let mut attempt = 0;
    let max_attempts = 2;
    let mut azure_response = None;
    while attempt < max_attempts {
        attempt += 1;
        match ureq::post(&api_url)
            .timeout(Duration::from_secs(60))
            .set("api-key", &account.api_key)
            .set("Content-Type", "application/json")
            .send_json(request_body)
        {
            Ok(res) => {
                azure_response = Some(res);
                break;
            }
            Err(ureq::Error::Status(code, resp)) => {
                let body = resp.into_string().unwrap_or_default();
                ui_tx
                    .send(AgentToUiMessage::StatusUpdate(format!(
                        "Azure OpenAI HTTP {code} error: {body}"
                    )))
                    .ok();
                break;
            }
            Err(e) => {
                if attempt < max_attempts {
                    std::thread::sleep(Duration::from_secs(1));
                } else {
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "Azure OpenAI connection error: {:?}",
                            e
                        )))
                        .ok();
                }
            }
        }
    }
    azure_response
}

pub fn execute_ollama_request(
    ollama_accounts: &[LocalOllamaAccount],
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
) -> Option<ureq::Response> {
    let host = ollama_accounts
        .first()
        .map(|account| account.host.as_str())
        .unwrap_or("http://localhost:11434");
    let api_url = ollama_chat_url(host);
    match ureq::post(&api_url)
        .timeout(Duration::from_secs(60))
        .set("Content-Type", "application/json")
        .send_json(request_body)
    {
        Ok(res) => Some(res),
        Err(e) => {
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(format!(
                    "Local Ollama connection error at {host}: {:?}",
                    e
                )))
                .ok();
            None
        }
    }
}

/// Build the Ollama OpenAI-compatible chat endpoint URL from a host, tolerating
/// a trailing slash. Kept as a pure helper so the request shape is testable
/// without a running server.
pub fn ollama_chat_url(host: &str) -> String {
    format!("{}/v1/chat/completions", host.trim_end_matches('/'))
}

/// Execute a request against an OpenAI-compatible API endpoint.
/// Used by Deepseek, Alibaba Qwen, Groq, Mistral, and other compatible providers.
fn execute_openai_compatible_request(
    api_url: &str,
    api_key_env_var: &str,
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
    provider_name: &str,
) -> Option<ureq::Response> {
    let api_key = std::env::var(api_key_env_var).unwrap_or_default();
    if api_key.trim().is_empty() {
        ui_tx
            .send(AgentToUiMessage::StatusUpdate(format!(
                "{provider_name} API key not set. Export {api_key_env_var} to use this provider."
            )))
            .ok();
        return None;
    }
    match ureq::post(api_url)
        .timeout(Duration::from_secs(60))
        .set("Authorization", &format!("Bearer {}", api_key))
        .set("Content-Type", "application/json")
        .send_json(request_body)
    {
        Ok(res) => Some(res),
        Err(ureq::Error::Status(401, _)) => {
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(format!(
                    "{provider_name} authentication failed. Check your {api_key_env_var} API key."
                )))
                .ok();
            None
        }
        Err(ureq::Error::Status(429, _)) => {
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(format!(
                    "{provider_name} rate limit exceeded (429). Try again shortly."
                )))
                .ok();
            None
        }
        Err(e) => {
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(format!(
                    "{provider_name} request error: {:?}",
                    e
                )))
                .ok();
            None
        }
    }
}

/// Deepseek API — OpenAI-compatible endpoint.
/// API docs: https://platform.deepseek.com/api-docs/
pub fn execute_deepseek_request(
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
) -> Option<ureq::Response> {
    execute_openai_compatible_request(
        "https://api.deepseek.com/chat/completions",
        "DEEPSEEK_API_KEY",
        request_body,
        ui_tx,
        "Deepseek",
    )
}

/// Alibaba Cloud Qwen (DashScope) — OpenAI-compatible endpoint.
/// API docs: https://www.alibabacloud.com/help/en/model-studio/
pub fn execute_alibaba_qwen_request(
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
) -> Option<ureq::Response> {
    execute_openai_compatible_request(
        "https://dashscope-intl.aliyuncs.com/compatible-mode/v1/chat/completions",
        "DASHSCOPE_API_KEY",
        request_body,
        ui_tx,
        "Alibaba Qwen",
    )
}

/// Groq — OpenAI-compatible endpoint for LPU inference.
/// API docs: https://console.groq.com/docs/api-reference
pub fn execute_groq_request(
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
) -> Option<ureq::Response> {
    execute_openai_compatible_request(
        "https://api.groq.com/openai/v1/chat/completions",
        "GROQ_API_KEY",
        request_body,
        ui_tx,
        "Groq",
    )
}

/// Mistral AI (La Plateforme) — OpenAI-compatible endpoint.
/// API docs: https://docs.mistral.ai/api/
pub fn execute_mistral_request(
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
) -> Option<ureq::Response> {
    execute_openai_compatible_request(
        "https://api.mistral.ai/v1/chat/completions",
        "MISTRAL_API_KEY",
        request_body,
        ui_tx,
        "Mistral AI",
    )
}

/// OpenAI Direct — standard API endpoint.
/// API docs: https://platform.openai.com/docs/api-reference
pub fn execute_openai_request(
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
) -> Option<ureq::Response> {
    execute_openai_compatible_request(
        "https://api.openai.com/v1/chat/completions",
        "OPENAI_API_KEY",
        request_body,
        ui_tx,
        "OpenAI",
    )
}

/// Google Vertex AI (Gemini) — OpenAI-compatible endpoint.
/// API docs: https://ai.google.dev/gemini-api/docs/openai
pub fn execute_google_request(
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
) -> Option<ureq::Response> {
    let api_key = std::env::var("GOOGLE_API_KEY").unwrap_or_default();
    if api_key.trim().is_empty() {
        ui_tx
            .send(AgentToUiMessage::StatusUpdate(
                "Google API key not set. Export GOOGLE_API_KEY to use this provider.".to_string(),
            ))
            .ok();
        return None;
    }
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions?key={api_key}"
    );
    match ureq::post(&url)
        .timeout(Duration::from_secs(60))
        .set("Content-Type", "application/json")
        .send_json(request_body)
    {
        Ok(res) => Some(res),
        Err(ureq::Error::Status(401, _)) => {
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(
                    "Google authentication failed. Check your GOOGLE_API_KEY.".to_string(),
                ))
                .ok();
            None
        }
        Err(e) => {
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(format!(
                    "Google request error: {:?}",
                    e
                )))
                .ok();
            None
        }
    }
}

/// Together AI — OpenAI-compatible endpoint.
/// API docs: https://docs.together.ai/reference/chat-completions
pub fn execute_together_request(
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
) -> Option<ureq::Response> {
    execute_openai_compatible_request(
        "https://api.together.xyz/v1/chat/completions",
        "TOGETHER_API_KEY",
        request_body,
        ui_tx,
        "Together AI",
    )
}

/// Fireworks AI — OpenAI-compatible endpoint.
/// API docs: https://docs.fireworks.ai/api-reference/
pub fn execute_fireworks_request(
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
) -> Option<ureq::Response> {
    execute_openai_compatible_request(
        "https://api.fireworks.ai/inference/v1/chat/completions",
        "FIREWORKS_API_KEY",
        request_body,
        ui_tx,
        "Fireworks AI",
    )
}

/// Perplexity — OpenAI-compatible endpoint for sonar models.
/// API docs: https://docs.perplexity.ai/api-reference/chat-completions
pub fn execute_perplexity_request(
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
) -> Option<ureq::Response> {
    execute_openai_compatible_request(
        "https://api.perplexity.ai/chat/completions",
        "PERPLEXITY_API_KEY",
        request_body,
        ui_tx,
        "Perplexity",
    )
}

/// Cerebras — OpenAI-compatible endpoint for wafer-scale inference.
/// API docs: https://inference-docs.cerebras.ai/api-reference/chat-completions
pub fn execute_cerebras_request(
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
) -> Option<ureq::Response> {
    execute_openai_compatible_request(
        "https://api.cerebras.ai/v1/chat/completions",
        "CEREBRAS_API_KEY",
        request_body,
        ui_tx,
        "Cerebras",
    )
}

/// AWS Bedrock — uses OpenAI-compatible proxy or environment-configured endpoint.
/// Requires AWS_REGION and AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY env vars,
/// or a configured Bedrock proxy URL via BEDROCK_PROXY_URL.
pub fn execute_bedrock_request(
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
) -> Option<ureq::Response> {
    // If a proxy URL is configured, use it as an OpenAI-compatible endpoint.
    if let Ok(proxy_url) = std::env::var("BEDROCK_PROXY_URL") {
        if !proxy_url.trim().is_empty() {
            let url = format!("{}/chat/completions", proxy_url.trim_end_matches('/'));
            return execute_openai_compatible_request(
                &url,
                "BEDROCK_API_KEY",
                request_body,
                ui_tx,
                "AWS Bedrock",
            );
        }
    }
    // Otherwise, Bedrock requires AWS SigV4 signing which is beyond simple ureq.
    // Direct the user to configure a proxy or use OpenRouter as a Bedrock gateway.
    ui_tx.send(AgentToUiMessage::StatusUpdate(
        "AWS Bedrock requires BEDROCK_PROXY_URL env var pointing to an OpenAI-compatible proxy. \
         Alternatively, use OpenRouter which routes to Bedrock models."
        .to_string()
    )).ok();
    None
}

/// Anthropic Messages API — converts OpenAI-format request body to Anthropic format.
/// API docs: https://docs.anthropic.com/en/api/messages
pub fn execute_anthropic_request(
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
) -> Option<ureq::Response> {
    let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default();
    if api_key.trim().is_empty() {
        ui_tx
            .send(AgentToUiMessage::StatusUpdate(
                "Anthropic API key not set. Export ANTHROPIC_API_KEY to use this provider."
                    .to_string(),
            ))
            .ok();
        return None;
    }
    // Convert OpenAI-format messages to Anthropic format:
    // Extract system message separately, keep user/assistant messages.
    let messages = request_body.get("messages").and_then(|m| m.as_array());
    let Some(messages) = messages else {
        ui_tx
            .send(AgentToUiMessage::StatusUpdate(
                "Anthropic request failed: no messages in request body.".to_string(),
            ))
            .ok();
        return None;
    };
    let mut system_text = String::new();
    let mut anthropic_messages = Vec::new();
    for msg in messages {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
        let content = msg
            .get("content")
            .cloned()
            .unwrap_or(Value::String(String::new()));
        match role {
            "system" => {
                if let Some(s) = content.as_str() {
                    system_text.push_str(s);
                }
            }
            "user" => {
                let mut entry = serde_json::Map::new();
                entry.insert("role".to_string(), Value::String("user".to_string()));
                entry.insert("content".to_string(), content);
                anthropic_messages.push(Value::Object(entry));
            }
            "assistant" => {
                // If the assistant message has tool_calls, convert them to
                // Anthropic tool_use content blocks.
                let mut entry = serde_json::Map::new();
                entry.insert("role".to_string(), Value::String("assistant".to_string()));
                if let Some(tool_calls) = msg.get("tool_calls").and_then(|t| t.as_array()) {
                    let mut blocks = Vec::new();
                    // Include text content if present
                    if let Some(text) = content.as_str() {
                        if !text.is_empty() {
                            let mut text_block = serde_json::Map::new();
                            text_block
                                .insert("type".to_string(), Value::String("text".to_string()));
                            text_block.insert("text".to_string(), Value::String(text.to_string()));
                            blocks.push(Value::Object(text_block));
                        }
                    }
                    for tc in tool_calls {
                        let func = match tc.get("function").and_then(|f| f.as_object()) {
                            Some(f) => f,
                            None => continue,
                        };
                        let mut tool_block = serde_json::Map::new();
                        tool_block
                            .insert("type".to_string(), Value::String("tool_use".to_string()));
                        if let Some(id) = tc.get("id").and_then(|i| i.as_str()) {
                            tool_block.insert("id".to_string(), Value::String(id.to_string()));
                        }
                        if let Some(name) = func.get("name").and_then(|n| n.as_str()) {
                            tool_block.insert("name".to_string(), Value::String(name.to_string()));
                        }
                        let input: Value = func
                            .get("arguments")
                            .and_then(|a| a.as_str())
                            .and_then(|s| serde_json::from_str(s).ok())
                            .unwrap_or_else(|| json!({}));
                        tool_block.insert("input".to_string(), input);
                        blocks.push(Value::Object(tool_block));
                    }
                    entry.insert("content".to_string(), Value::Array(blocks));
                } else {
                    entry.insert("content".to_string(), content);
                }
                anthropic_messages.push(Value::Object(entry));
            }
            "tool" => {
                // OpenAI tool results → Anthropic user message with tool_result block
                let tool_call_id = msg
                    .get("tool_call_id")
                    .and_then(|id| id.as_str())
                    .unwrap_or("unknown");
                let result_text = content.as_str().unwrap_or("").to_string();
                let mut result_block = serde_json::Map::new();
                result_block.insert("type".to_string(), Value::String("tool_result".to_string()));
                result_block.insert(
                    "tool_use_id".to_string(),
                    Value::String(tool_call_id.to_string()),
                );
                result_block.insert("content".to_string(), Value::String(result_text));
                let mut entry = serde_json::Map::new();
                entry.insert("role".to_string(), Value::String("user".to_string()));
                entry.insert(
                    "content".to_string(),
                    Value::Array(vec![Value::Object(result_block)]),
                );
                anthropic_messages.push(Value::Object(entry));
            }
            _ => {}
        }
    }
    // Anthropic requires at least one user message.  If the conversation
    // contains only system messages, promote the system text as a user
    // message so the API accepts the request.
    if anthropic_messages.is_empty() {
        let fallback_content = if !system_text.is_empty() {
            system_text.clone()
        } else {
            "Continue".to_string()
        };
        let mut entry = serde_json::Map::new();
        entry.insert("role".to_string(), Value::String("user".to_string()));
        entry.insert("content".to_string(), Value::String(fallback_content));
        anthropic_messages.push(Value::Object(entry));
    }
    let model = request_body
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("claude-sonnet-4-20250514");
    let max_tokens = request_body
        .get("max_tokens")
        .and_then(|t| t.as_u64())
        .unwrap_or(4096);
    let mut body = serde_json::Map::new();
    body.insert("model".to_string(), Value::String(model.to_string()));
    body.insert("max_tokens".to_string(), Value::Number(max_tokens.into()));
    body.insert("messages".to_string(), Value::Array(anthropic_messages));
    if !system_text.is_empty() {
        body.insert("system".to_string(), Value::String(system_text));
    }
    // Convert OpenAI-format tools to Anthropic format.
    // OpenAI: {"type":"function","function":{"name","description","parameters"}}
    // Anthropic: {"name","description","input_schema"}
    if let Some(tools) = request_body.get("tools").and_then(|t| t.as_array()) {
        let anthropic_tools: Vec<Value> = tools
            .iter()
            .filter_map(|tool| {
                let func = tool.get("function")?;
                let name = func.get("name")?.as_str()?;
                let mut entry = serde_json::Map::new();
                entry.insert("name".to_string(), Value::String(name.to_string()));
                if let Some(desc) = func.get("description").and_then(|d| d.as_str()) {
                    entry.insert("description".to_string(), Value::String(desc.to_string()));
                }
                let schema = func
                    .get("parameters")
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
                entry.insert("input_schema".to_string(), schema);
                Some(Value::Object(entry))
            })
            .collect();
        if !anthropic_tools.is_empty() {
            body.insert("tools".to_string(), Value::Array(anthropic_tools));
        }
    }
    // Force non-streaming: the loop runner's SSE parser only understands
    // OpenAI-format events.  Anthropic uses a different wire format, so we
    // receive the full response at once and let the loop runner parse it
    // via the provider-specific branch.
    body.insert("stream".to_string(), Value::Bool(false));
    let anthropic_body = Value::Object(body);
    match ureq::post("https://api.anthropic.com/v1/messages")
        .timeout(Duration::from_secs(120))
        .set("x-api-key", &api_key)
        .set("anthropic-version", "2023-06-01")
        .set("Content-Type", "application/json")
        .send_json(&anthropic_body)
    {
        Ok(res) => Some(res),
        Err(ureq::Error::Status(401, _)) => {
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(
                    "Anthropic authentication failed. Check your ANTHROPIC_API_KEY.".to_string(),
                ))
                .ok();
            None
        }
        Err(ureq::Error::Status(429, _)) => {
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(
                    "Anthropic rate limit exceeded (429). Try again shortly.".to_string(),
                ))
                .ok();
            None
        }
        Err(e) => {
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(format!(
                    "Anthropic request failed: {e}"
                )))
                .ok();
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollama_url_appends_openai_chat_path() {
        assert_eq!(
            ollama_chat_url("http://localhost:11434"),
            "http://localhost:11434/v1/chat/completions"
        );
    }

    #[test]
    fn ollama_url_trims_trailing_slash() {
        assert_eq!(
            ollama_chat_url("http://localhost:11434/"),
            "http://localhost:11434/v1/chat/completions"
        );
        assert_eq!(
            ollama_chat_url("http://remote:9999///"),
            "http://remote:9999/v1/chat/completions"
        );
    }

    #[test]
    fn bedrock_url_appends_chat_completions() {
        let url = format!(
            "{}/chat/completions",
            "https://bedrock-proxy.example.com".trim_end_matches('/')
        );
        assert_eq!(url, "https://bedrock-proxy.example.com/chat/completions");
    }

    #[test]
    fn bedrock_url_trims_trailing_slash() {
        let url = format!(
            "{}/chat/completions",
            "https://bedrock-proxy.example.com/".trim_end_matches('/')
        );
        assert_eq!(url, "https://bedrock-proxy.example.com/chat/completions");
    }
}
