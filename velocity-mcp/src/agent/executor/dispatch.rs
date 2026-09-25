use super::super::models::*;
use super::utils::send_usage_update;
use crate::usage::{
    AzureOpenAiAccount, CloudflareAccount, LocalOllamaAccount, OpenRouterAccount, UsageTracker,
};
use crossbeam_channel::Sender;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

/// Shared HTTP agent for provider calls whose response body is streamed.
/// ureq's `.timeout()` sets a *total* deadline that also covers reading the
/// body, so an SSE stream still emitting tokens after 60s gets killed mid
/// tool-call — and the truncated JSON arguments then executed as real calls.
/// This agent bounds only the connect and each individual socket read, so
/// long reasoning streams can run as long as bytes keep arriving.
pub(crate) fn stream_agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(15))
            .timeout_read(Duration::from_secs(300))
            .build()
    })
}

/// Resolve an API key by checking provider-settings.json first (where the
/// Settings UI saves keys), then falling back to the environment variable.
/// Provider settings are stored at the user-level config directory
/// (`%APPDATA%/Velocity/`), not per-workspace, for security.
///
/// When a provider supports a pay-per-token plan (e.g. Alibaba DashScope
/// Token Plan), the token-plan key is preferred when `use_token_plan` is true.
pub(crate) fn resolve_api_key(
    workspace_root: &PathBuf,
    settings_field: &str,
    env_var: &str,
) -> String {
    let settings_path = crate::usage::provider_settings_path(workspace_root);
    if let Ok(contents) = std::fs::read_to_string(&settings_path) {
        // Strip UTF-8 BOM if present (Windows editors often insert it)
        let stripped = contents.strip_prefix('\u{feff}').unwrap_or(&contents);
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(stripped) {
            if let Some(section) = json.get(settings_field) {
                // Prefer token-plan key when the switch is on and the key is set.
                let use_token = section
                    .get("use_token_plan")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if use_token {
                    if let Some(tk) = section.get("token_plan_api_key").and_then(|k| k.as_str()) {
                        if !tk.trim().is_empty() {
                            return tk.to_string();
                        }
                    }
                }
                if let Some(key) = section.get("api_key").and_then(|k| k.as_str()) {
                    if !key.trim().is_empty() {
                        return key.to_string();
                    }
                }
            }
        }
    }
    std::env::var(env_var).unwrap_or_default()
}

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
            match stream_agent()
                .post("https://openrouter.ai/api/v1/chat/completions")
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
                                    "OpenRouter account '{}' quota exhausted \u{2014} trying next\u{2026}",
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
                            "OpenRouter rate limit (429) on '{}'. Retrying in {}s (Attempt {}/{})\u{2026}",
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
            match stream_agent()
                .post(&api_url)
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
    // Select account by priority (.n): lower = higher priority.
    let account = azure_accounts
        .iter()
        .min_by_key(|a| a.n)
        .expect("azure_accounts is non-empty");
    // Tier-aware retry budget: paid tiers get more attempts than free.
    let max_attempts = if account.tier == "paid" { 3 } else { 2 };
    let endpoint = account.endpoint.trim_end_matches('/');
    let api_url = format!(
        "{}/openai/deployments/{}/chat/completions?api-version={}",
        endpoint, account.deployment, account.api_version
    );
    let mut attempt = 0;
    let mut azure_response = None;
    while attempt < max_attempts {
        attempt += 1;
        match stream_agent()
            .post(&api_url)
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
    let account = ollama_accounts.first();
    let host = account
        .map(|a| a.host.as_str())
        .unwrap_or("http://localhost:11434");
    let label = account.map(|a| a.label.as_str()).unwrap_or("Local-Ollama");
    let api_url = ollama_chat_url(host);
    match stream_agent()
        .post(&api_url)
        .set("Content-Type", "application/json")
        .send_json(request_body)
    {
        Ok(res) => Some(res),
        Err(e) => {
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(format!(
                    "Ollama '{label}' connection error at {host}: {e:?}",
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
/// Retries up to 3 times with exponential backoff on 429/5xx errors.
fn execute_openai_compatible_request(
    api_url: &str,
    settings_field: &str,
    api_key_env_var: &str,
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
    provider_name: &str,
    workspace_root: &PathBuf,
) -> Option<ureq::Response> {
    let api_key = resolve_api_key(workspace_root, settings_field, api_key_env_var);
    if api_key.trim().is_empty() {
        ui_tx
            .send(AgentToUiMessage::StatusUpdate(format!(
                "{provider_name} API key not set. Configure it in Settings or export {api_key_env_var}."
            )))
            .ok();
        return None;
    }

    let max_attempts = 3u32;
    for attempt in 1..=max_attempts {
        match stream_agent()
            .post(api_url)
            .set("Authorization", &format!("Bearer {}", api_key))
            .set("Content-Type", "application/json")
            .send_json(request_body)
        {
            Ok(res) => return Some(res),
            Err(ureq::Error::Status(401, _)) => {
                // Auth errors are not retryable.
                ui_tx
                    .send(AgentToUiMessage::StatusUpdate(format!(
                        "{provider_name} authentication failed. Check your API key in Settings."
                    )))
                    .ok();
                return None;
            }
            Err(ureq::Error::Status(429, _)) => {
                if attempt < max_attempts {
                    let wait_secs = attempt * 2; // 2s, 4s exponential
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "{provider_name} rate limit (429). Retrying in {wait_secs}s (attempt {attempt}/{max_attempts})..."
                        )))
                        .ok();
                    std::thread::sleep(Duration::from_secs(wait_secs as u64));
                } else {
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "{provider_name} rate limit exceeded after {max_attempts} retries."
                        )))
                        .ok();
                    return None;
                }
            }
            Err(ureq::Error::Status(code, _)) if code >= 500 => {
                if attempt < max_attempts {
                    let wait_secs = attempt; // 1s, 2s for server errors
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "{provider_name} server error ({code}). Retrying in {wait_secs}s (attempt {attempt}/{max_attempts})..."
                        )))
                        .ok();
                    std::thread::sleep(Duration::from_secs(wait_secs as u64));
                } else {
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "{provider_name} server error after {max_attempts} retries."
                        )))
                        .ok();
                    return None;
                }
            }
            Err(e) => {
                if attempt < max_attempts {
                    std::thread::sleep(Duration::from_secs(1));
                } else {
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "{provider_name} request error: {:?}",
                            e
                        )))
                        .ok();
                    return None;
                }
            }
        }
    }
    None
}

/// Deepseek API — OpenAI-compatible endpoint.
/// API docs: https://platform.deepseek.com/api-docs/
pub fn execute_deepseek_request(
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
    workspace_root: &PathBuf,
) -> Option<ureq::Response> {
    execute_openai_compatible_request(
        "https://api.deepseek.com/chat/completions",
        "deepseek",
        "DEEPSEEK_API_KEY",
        request_body,
        ui_tx,
        "Deepseek",
        workspace_root,
    )
}

/// Alibaba Cloud Qwen (DashScope) — OpenAI-compatible endpoint.
/// API docs: https://www.alibabacloud.com/help/en/model-studio/
///
/// When the caller's provider-settings has `alibaba.use_token_plan = true`
/// and a non-empty `token_plan_api_key`, dispatch is routed to the Token Plan
/// base URL instead of the standard DashScope compatible-mode endpoint.
pub fn execute_alibaba_qwen_request(
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
    workspace_root: &PathBuf,
) -> Option<ureq::Response> {
    let api_url = alibaba_chat_completions_url(workspace_root);
    execute_openai_compatible_request(
        &api_url,
        "alibaba",
        "DASHSCOPE_API_KEY",
        request_body,
        ui_tx,
        "Alibaba Qwen",
        workspace_root,
    )
}

/// Default base URL for the Alibaba Qwen Token Plan (pay-per-token) endpoint,
/// Singapore region. Callers can override via provider-settings
/// `alibaba.token_plan_base_url`.
///
/// Stored WITHOUT the trailing `/v1` (matches the convention used by
/// `fetch_openai_compatible_models` which appends `/v1/models`).
pub const ALIBABA_TOKEN_PLAN_DEFAULT_BASE_URL: &str =
    "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode";

/// Standard DashScope international compatible-mode base URL (Coding Plan or
/// workspace keys), also without `/v1`.
pub const ALIBABA_DASHSCOPE_INTL_BASE_URL: &str =
    "https://dashscope-intl.aliyuncs.com/compatible-mode";

/// Returns the full `/v1/chat/completions` URL to use for the caller's
/// currently active Alibaba plan. Reads provider-settings.json for the
/// `use_token_plan` flag and optional `token_plan_base_url` override.
pub fn alibaba_chat_completions_url(workspace_root: &PathBuf) -> String {
    format!("{}/v1/chat/completions", alibaba_base_url(workspace_root))
}

/// Returns the base URL (WITHOUT the trailing `/v1`) for Alibaba Qwen
/// dispatch, honouring the token-plan switch. Any trailing `/v1` in a user
/// override is normalised away so downstream URL joins stay consistent.
pub fn alibaba_base_url(workspace_root: &PathBuf) -> String {
    let settings_path = crate::usage::provider_settings_path(workspace_root);
    if let Ok(contents) = std::fs::read_to_string(&settings_path) {
        let stripped = contents.strip_prefix('\u{feff}').unwrap_or(&contents);
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(stripped) {
            if let Some(section) = json.get("alibaba") {
                let use_token = section
                    .get("use_token_plan")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let tp_key_set = section
                    .get("token_plan_api_key")
                    .and_then(|k| k.as_str())
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false);
                if use_token && tp_key_set {
                    let override_url = section
                        .get("token_plan_base_url")
                        .and_then(|v| v.as_str())
                        .map(normalise_alibaba_base)
                        .filter(|s| !s.is_empty());
                    if let Some(u) = override_url {
                        return u;
                    }
                    return ALIBABA_TOKEN_PLAN_DEFAULT_BASE_URL.to_string();
                }
            }
        }
    }
    ALIBABA_DASHSCOPE_INTL_BASE_URL.to_string()
}

/// Trim trailing slashes and a trailing `/v1` so callers can uniformly
/// append `/v1/...` themselves.
fn normalise_alibaba_base(raw: &str) -> String {
    let mut s = raw.trim().trim_end_matches('/').to_string();
    if s.ends_with("/v1") {
        s.truncate(s.len() - 3);
        s = s.trim_end_matches('/').to_string();
    }
    s
}

/// Groq — OpenAI-compatible endpoint for LPU inference.
/// API docs: https://console.groq.com/docs/api-reference
pub fn execute_groq_request(
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
    workspace_root: &PathBuf,
) -> Option<ureq::Response> {
    execute_openai_compatible_request(
        "https://api.groq.com/openai/v1/chat/completions",
        "groq",
        "GROQ_API_KEY",
        request_body,
        ui_tx,
        "Groq",
        workspace_root,
    )
}

/// Mistral AI (La Plateforme) — OpenAI-compatible endpoint.
/// API docs: https://docs.mistral.ai/api/
pub fn execute_mistral_request(
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
    workspace_root: &PathBuf,
) -> Option<ureq::Response> {
    execute_openai_compatible_request(
        "https://api.mistral.ai/v1/chat/completions",
        "mistral",
        "MISTRAL_API_KEY",
        request_body,
        ui_tx,
        "Mistral AI",
        workspace_root,
    )
}

/// OpenAI Direct — standard API endpoint.
/// API docs: https://platform.openai.com/docs/api-reference
pub fn execute_openai_request(
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
    workspace_root: &PathBuf,
) -> Option<ureq::Response> {
    execute_openai_compatible_request(
        "https://api.openai.com/v1/chat/completions",
        "openai",
        "OPENAI_API_KEY",
        request_body,
        ui_tx,
        "OpenAI",
        workspace_root,
    )
}

/// Google Vertex AI (Gemini) — OpenAI-compatible endpoint.
/// API docs: https://ai.google.dev/gemini-api/docs/openai
pub fn execute_google_request(
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
    workspace_root: &PathBuf,
) -> Option<ureq::Response> {
    let api_key = resolve_api_key(workspace_root, "google", "GOOGLE_API_KEY");
    if api_key.trim().is_empty() {
        ui_tx
            .send(AgentToUiMessage::StatusUpdate(
                "Google API key not set. Configure it in Settings or export GOOGLE_API_KEY."
                    .to_string(),
            ))
            .ok();
        return None;
    }
    let url =
        "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions".to_string();
    match stream_agent()
        .post(&url)
        .set("x-goog-api-key", &api_key)
        .set("Content-Type", "application/json")
        .send_json(request_body)
    {
        Ok(res) => Some(res),
        Err(ureq::Error::Status(401, _)) => {
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(
                    "Google authentication failed. Check your API key in Settings.".to_string(),
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
    workspace_root: &PathBuf,
) -> Option<ureq::Response> {
    execute_openai_compatible_request(
        "https://api.together.xyz/v1/chat/completions",
        "together",
        "TOGETHER_API_KEY",
        request_body,
        ui_tx,
        "Together AI",
        workspace_root,
    )
}

/// Fireworks AI — OpenAI-compatible endpoint.
/// API docs: https://docs.fireworks.ai/api-reference/
pub fn execute_fireworks_request(
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
    workspace_root: &PathBuf,
) -> Option<ureq::Response> {
    execute_openai_compatible_request(
        "https://api.fireworks.ai/inference/v1/chat/completions",
        "fireworks",
        "FIREWORKS_API_KEY",
        request_body,
        ui_tx,
        "Fireworks AI",
        workspace_root,
    )
}

/// Perplexity — OpenAI-compatible endpoint for sonar models.
/// API docs: https://docs.perplexity.ai/api-reference/chat-completions
pub fn execute_perplexity_request(
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
    workspace_root: &PathBuf,
) -> Option<ureq::Response> {
    execute_openai_compatible_request(
        "https://api.perplexity.ai/chat/completions",
        "perplexity",
        "PERPLEXITY_API_KEY",
        request_body,
        ui_tx,
        "Perplexity",
        workspace_root,
    )
}

/// Cerebras — OpenAI-compatible endpoint for wafer-scale inference.
/// API docs: https://inference-docs.cerebras.ai/api-reference/chat-completions
pub fn execute_cerebras_request(
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
    workspace_root: &PathBuf,
) -> Option<ureq::Response> {
    execute_openai_compatible_request(
        "https://api.cerebras.ai/v1/chat/completions",
        "cerebras",
        "CEREBRAS_API_KEY",
        request_body,
        ui_tx,
        "Cerebras",
        workspace_root,
    )
}

/// AWS Bedrock — uses OpenAI-compatible proxy or environment-configured endpoint.
/// Requires AWS_REGION and AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY env vars,
/// or a configured Bedrock proxy URL via BEDROCK_PROXY_URL.
pub fn execute_bedrock_request(
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
    workspace_root: &PathBuf,
) -> Option<ureq::Response> {
    // If a proxy URL is configured, use it as an OpenAI-compatible endpoint.
    if let Ok(proxy_url) = std::env::var("BEDROCK_PROXY_URL") {
        if !proxy_url.trim().is_empty() {
            let url = format!("{}/chat/completions", proxy_url.trim_end_matches('/'));
            return execute_openai_compatible_request(
                &url,
                "bedrock",
                "BEDROCK_API_KEY",
                request_body,
                ui_tx,
                "AWS Bedrock",
                workspace_root,
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

/// Convert an OpenAI-format request body into an Anthropic Messages API body.
///
/// Returns `Ok(body)` ready to POST, or `Err(message)` when the input is
/// missing required fields (e.g. no `messages` array).  This is a pure
/// function with no side-effects so it can be thoroughly unit-tested.
pub fn build_anthropic_body(request_body: &Value) -> Result<Value, String> {
    let messages = request_body.get("messages").and_then(|m| m.as_array());
    let Some(messages) = messages else {
        return Err("no messages in request body".to_string());
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
                let mut entry = serde_json::Map::new();
                entry.insert("role".to_string(), Value::String("assistant".to_string()));
                if let Some(tool_calls) = msg.get("tool_calls").and_then(|t| t.as_array()) {
                    let mut blocks = Vec::new();
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
    // Anthropic requires at least one user message.
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
    body.insert("stream".to_string(), Value::Bool(false));
    Ok(Value::Object(body))
}

/// Anthropic Messages API — converts OpenAI-format request body to Anthropic format.
/// API docs: https://docs.anthropic.com/en/api/messages
pub fn execute_anthropic_request(
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
    workspace_root: &PathBuf,
) -> Option<ureq::Response> {
    let api_key = resolve_api_key(workspace_root, "anthropic", "ANTHROPIC_API_KEY");
    if api_key.trim().is_empty() {
        ui_tx
            .send(AgentToUiMessage::StatusUpdate(
                "Anthropic API key not set. Configure it in Settings or export ANTHROPIC_API_KEY."
                    .to_string(),
            ))
            .ok();
        return None;
    }
    let anthropic_body = match build_anthropic_body(request_body) {
        Ok(body) => body,
        Err(msg) => {
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(format!(
                    "Anthropic request failed: {msg}"
                )))
                .ok();
            return None;
        }
    };
    match stream_agent()
        .post("https://api.anthropic.com/v1/messages")
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
    use crate::agent::executor::utils::{build_request, estimate_tokens, is_quota_exhausted_error};
    use crate::agent::models::{ApiStyle, ModelInfo};

    /// Global lock serialising tests that mutate `VELOCITY_CONFIG_DIR` (or any
    /// other process-wide env var) so parallel test threads cannot observe a
    /// half-set environment from a sibling test.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        match LOCK.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    // ── ollama_chat_url ──────────────────────────────────────────────

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

    // ── bedrock URL shape ────────────────────────────────────────────

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

    // ── resolve_api_key ──────────────────────────────────────────────

    #[test]
    fn resolve_api_key_reads_from_provider_settings_json() {
        let _lock = env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("VELOCITY_CONFIG_DIR", dir.path().to_str().unwrap());
        let settings = json!({
            "anthropic": {
                "api_key": "sk-ant-from-file"
            }
        });
        std::fs::write(
            dir.path().join("provider-settings.json"),
            settings.to_string(),
        )
        .unwrap();

        let root = tempfile::tempdir().unwrap().path().to_path_buf();
        let key = resolve_api_key(&root, "anthropic", "NONEXISTENT_ENV_VAR_FOR_TEST");
        assert_eq!(key, "sk-ant-from-file");
        std::env::remove_var("VELOCITY_CONFIG_DIR");
    }

    #[test]
    fn resolve_api_key_falls_back_to_env_var() {
        let _lock = env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("VELOCITY_CONFIG_DIR", dir.path().to_str().unwrap());
        // No provider-settings.json → must fall through to env var.
        let root = tempfile::tempdir().unwrap().path().to_path_buf();
        std::env::set_var("VELOCITY_TEST_RESOLVE_KEY", "sk-from-env");
        let key = resolve_api_key(&root, "anthropic", "VELOCITY_TEST_RESOLVE_KEY");
        assert_eq!(key, "sk-from-env");
        std::env::remove_var("VELOCITY_TEST_RESOLVE_KEY");
        std::env::remove_var("VELOCITY_CONFIG_DIR");
    }

    #[test]
    fn resolve_api_key_returns_empty_when_nothing_configured() {
        let _lock = env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("VELOCITY_CONFIG_DIR", dir.path().to_str().unwrap());
        let root = tempfile::tempdir().unwrap().path().to_path_buf();
        let key = resolve_api_key(
            &root,
            "nonexistent_provider",
            "VELOCITY_NONEXISTENT_ENV_VAR_XYZ",
        );
        assert_eq!(key, "");
        std::env::remove_var("VELOCITY_CONFIG_DIR");
    }

    #[test]
    fn resolve_api_key_ignores_empty_key_in_settings() {
        let _lock = env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("VELOCITY_CONFIG_DIR", dir.path().to_str().unwrap());
        let settings = json!({
            "openai": {
                "api_key": "   "
            }
        });
        std::fs::write(
            dir.path().join("provider-settings.json"),
            settings.to_string(),
        )
        .unwrap();

        let root = tempfile::tempdir().unwrap().path().to_path_buf();
        // Whitespace-only key should be treated as empty → fall back to env.
        let key = resolve_api_key(&root, "openai", "VELOCITY_NONEXISTENT_ENV_VAR_XYZ");
        assert_eq!(key, "");
        std::env::remove_var("VELOCITY_CONFIG_DIR");
    }

    #[test]
    fn resolve_api_key_prefers_file_over_env() {
        let _lock = env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("VELOCITY_CONFIG_DIR", dir.path().to_str().unwrap());
        let settings = json!({
            "groq": {
                "api_key": "sk-from-file"
            }
        });
        std::fs::write(
            dir.path().join("provider-settings.json"),
            settings.to_string(),
        )
        .unwrap();

        std::env::set_var("VELOCITY_TEST_GROQ_KEY", "sk-from-env");
        let root = tempfile::tempdir().unwrap().path().to_path_buf();
        let key = resolve_api_key(&root, "groq", "VELOCITY_TEST_GROQ_KEY");
        assert_eq!(key, "sk-from-file");
        std::env::remove_var("VELOCITY_TEST_GROQ_KEY");
        std::env::remove_var("VELOCITY_CONFIG_DIR");
    }

    #[test]
    fn resolve_api_key_prefers_token_plan_when_enabled() {
        let _lock = env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("VELOCITY_CONFIG_DIR", dir.path().to_str().unwrap());
        let settings = json!({
            "alibaba": {
                "api_key": "sk-ws-default",
                "token_plan_api_key": "sk-sp-tokenplan",
                "use_token_plan": true
            }
        });
        std::fs::write(
            dir.path().join("provider-settings.json"),
            settings.to_string(),
        )
        .unwrap();
        let root = tempfile::tempdir().unwrap().path().to_path_buf();
        let key = resolve_api_key(&root, "alibaba", "DASHSCOPE_NONEXISTENT_ENV_XYZ");
        assert_eq!(key, "sk-sp-tokenplan");
        std::env::remove_var("VELOCITY_CONFIG_DIR");
    }

    #[test]
    fn resolve_api_key_uses_default_when_token_plan_disabled() {
        let _lock = env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("VELOCITY_CONFIG_DIR", dir.path().to_str().unwrap());
        let settings = json!({
            "alibaba": {
                "api_key": "sk-ws-default",
                "token_plan_api_key": "sk-sp-tokenplan",
                "use_token_plan": false
            }
        });
        std::fs::write(
            dir.path().join("provider-settings.json"),
            settings.to_string(),
        )
        .unwrap();
        let root = tempfile::tempdir().unwrap().path().to_path_buf();
        let key = resolve_api_key(&root, "alibaba", "DASHSCOPE_NONEXISTENT_ENV_XYZ");
        assert_eq!(key, "sk-ws-default");
        std::env::remove_var("VELOCITY_CONFIG_DIR");
    }

    #[test]
    fn resolve_api_key_token_plan_falls_back_when_key_empty() {
        let _lock = env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("VELOCITY_CONFIG_DIR", dir.path().to_str().unwrap());
        let settings = json!({
            "alibaba": {
                "api_key": "sk-ws-default",
                "token_plan_api_key": "",
                "use_token_plan": true
            }
        });
        std::fs::write(
            dir.path().join("provider-settings.json"),
            settings.to_string(),
        )
        .unwrap();
        let root = tempfile::tempdir().unwrap().path().to_path_buf();
        // Token plan enabled but no key → fall through to normal api_key.
        let key = resolve_api_key(&root, "alibaba", "DASHSCOPE_NONEXISTENT_ENV_XYZ");
        assert_eq!(key, "sk-ws-default");
        std::env::remove_var("VELOCITY_CONFIG_DIR");
    }

    // ── alibaba_base_url / chat URL routing ──────────────────

    #[test]
    fn alibaba_base_url_defaults_to_dashscope_intl_when_token_plan_off() {
        let _lock = env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("VELOCITY_CONFIG_DIR", dir.path().to_str().unwrap());
        let settings = json!({
            "alibaba": { "api_key": "sk-ws-..." }
        });
        std::fs::write(
            dir.path().join("provider-settings.json"),
            settings.to_string(),
        )
        .unwrap();
        let root = tempfile::tempdir().unwrap().path().to_path_buf();
        assert_eq!(alibaba_base_url(&root), ALIBABA_DASHSCOPE_INTL_BASE_URL);
        assert_eq!(
            alibaba_chat_completions_url(&root),
            "https://dashscope-intl.aliyuncs.com/compatible-mode/v1/chat/completions"
        );
        std::env::remove_var("VELOCITY_CONFIG_DIR");
    }

    #[test]
    fn alibaba_base_url_switches_to_token_plan_when_enabled() {
        let _lock = env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("VELOCITY_CONFIG_DIR", dir.path().to_str().unwrap());
        let settings = json!({
            "alibaba": {
                "api_key": "sk-ws-...",
                "token_plan_api_key": "sk-sp-...",
                "use_token_plan": true
            }
        });
        std::fs::write(
            dir.path().join("provider-settings.json"),
            settings.to_string(),
        )
        .unwrap();
        let root = tempfile::tempdir().unwrap().path().to_path_buf();
        assert_eq!(alibaba_base_url(&root), ALIBABA_TOKEN_PLAN_DEFAULT_BASE_URL);
        assert_eq!(
            alibaba_chat_completions_url(&root),
            "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1/chat/completions"
        );
        std::env::remove_var("VELOCITY_CONFIG_DIR");
    }

    #[test]
    fn alibaba_base_url_respects_user_override_and_strips_trailing_v1() {
        let _lock = env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("VELOCITY_CONFIG_DIR", dir.path().to_str().unwrap());
        let settings = json!({
            "alibaba": {
                "api_key": "sk-ws-...",
                "token_plan_api_key": "sk-sp-...",
                "use_token_plan": true,
                "token_plan_base_url": "https://token-plan.eu-central-1.maas.aliyuncs.com/compatible-mode/v1/"
            }
        });
        std::fs::write(
            dir.path().join("provider-settings.json"),
            settings.to_string(),
        )
        .unwrap();
        let root = tempfile::tempdir().unwrap().path().to_path_buf();
        assert_eq!(
            alibaba_base_url(&root),
            "https://token-plan.eu-central-1.maas.aliyuncs.com/compatible-mode"
        );
        assert_eq!(
            alibaba_chat_completions_url(&root),
            "https://token-plan.eu-central-1.maas.aliyuncs.com/compatible-mode/v1/chat/completions"
        );
        std::env::remove_var("VELOCITY_CONFIG_DIR");
    }

    #[test]
    fn alibaba_token_plan_disabled_uses_dashscope_even_with_tp_key_present() {
        let _lock = env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("VELOCITY_CONFIG_DIR", dir.path().to_str().unwrap());
        let settings = json!({
            "alibaba": {
                "api_key": "sk-ws-...",
                "token_plan_api_key": "sk-sp-...",
                "use_token_plan": false
            }
        });
        std::fs::write(
            dir.path().join("provider-settings.json"),
            settings.to_string(),
        )
        .unwrap();
        let root = tempfile::tempdir().unwrap().path().to_path_buf();
        assert_eq!(alibaba_base_url(&root), ALIBABA_DASHSCOPE_INTL_BASE_URL);
        std::env::remove_var("VELOCITY_CONFIG_DIR");
    }

    // ── build_anthropic_body ─────────────────────────────────────────

    #[test]
    fn anthropic_body_basic_user_message() {
        let req = json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "Hello Claude"}
            ]
        });
        let body = build_anthropic_body(&req).unwrap();
        assert_eq!(body["model"], "claude-sonnet-4-20250514");
        assert_eq!(body["max_tokens"], 1024);
        assert_eq!(body["stream"], false);
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "Hello Claude");
        // No system key when there's no system message.
        assert!(body.get("system").is_none());
    }

    #[test]
    fn anthropic_body_extracts_system_message() {
        let req = json!({
            "messages": [
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": "Hi"}
            ]
        });
        let body = build_anthropic_body(&req).unwrap();
        assert_eq!(body["system"], "You are a helpful assistant.");
        let msgs = body["messages"].as_array().unwrap();
        // System message should NOT appear in the messages array.
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "Hi");
    }

    #[test]
    fn anthropic_body_system_only_promotes_to_user() {
        // When only a system message exists, Anthropic requires at least one
        // user message. The converter should promote the system text.
        let req = json!({
            "messages": [
                {"role": "system", "content": "You are a pirate."}
            ]
        });
        let body = build_anthropic_body(&req).unwrap();
        assert_eq!(body["system"], "You are a pirate.");
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "You are a pirate.");
    }

    #[test]
    fn anthropic_body_empty_messages_falls_back_to_continue() {
        // No messages at all → should produce a "Continue" fallback.
        let req = json!({
            "messages": []
        });
        let body = build_anthropic_body(&req).unwrap();
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "Continue");
    }

    #[test]
    fn anthropic_body_no_messages_key_returns_error() {
        let req = json!({"model": "claude-sonnet-4-20250514"});
        let err = build_anthropic_body(&req).unwrap_err();
        assert!(err.contains("no messages"), "got: {err}");
    }

    #[test]
    fn anthropic_body_converts_tool_calls_to_tool_use_blocks() {
        let req = json!({
            "messages": [
                {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [
                        {
                            "id": "call_123",
                            "function": {
                                "name": "read_file",
                                "arguments": "{\"path\":\"/tmp/x.rs\"}"
                            }
                        }
                    ]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_123",
                    "content": "file contents here"
                }
            ]
        });
        let body = build_anthropic_body(&req).unwrap();
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);

        // Assistant message should have tool_use block.
        let assistant_content = msgs[0]["content"].as_array().unwrap();
        assert_eq!(assistant_content.len(), 1);
        assert_eq!(assistant_content[0]["type"], "tool_use");
        assert_eq!(assistant_content[0]["id"], "call_123");
        assert_eq!(assistant_content[0]["name"], "read_file");
        assert_eq!(assistant_content[0]["input"]["path"], "/tmp/x.rs");

        // Tool result should be a user message with tool_result block.
        assert_eq!(msgs[1]["role"], "user");
        let tool_result_content = msgs[1]["content"].as_array().unwrap();
        assert_eq!(tool_result_content.len(), 1);
        assert_eq!(tool_result_content[0]["type"], "tool_result");
        assert_eq!(tool_result_content[0]["tool_use_id"], "call_123");
        assert_eq!(tool_result_content[0]["content"], "file contents here");
    }

    #[test]
    fn anthropic_body_assistant_with_text_and_tool_calls() {
        let req = json!({
            "messages": [
                {
                    "role": "assistant",
                    "content": "Let me check that.",
                    "tool_calls": [
                        {
                            "id": "call_abc",
                            "function": {
                                "name": "grep_search",
                                "arguments": "{\"pattern\":\"fn main\"}"
                            }
                        }
                    ]
                }
            ]
        });
        let body = build_anthropic_body(&req).unwrap();
        let msgs = body["messages"].as_array().unwrap();
        let blocks = msgs[0]["content"].as_array().unwrap();
        // Should have a text block AND a tool_use block.
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[0]["text"], "Let me check that.");
        assert_eq!(blocks[1]["type"], "tool_use");
        assert_eq!(blocks[1]["name"], "grep_search");
    }

    #[test]
    fn anthropic_body_converts_tools_to_anthropic_format() {
        let req = json!({
            "messages": [
                {"role": "user", "content": "Use a tool"}
            ],
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "read_file",
                        "description": "Read a file from disk",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "path": {"type": "string"}
                            },
                            "required": ["path"]
                        }
                    }
                }
            ]
        });
        let body = build_anthropic_body(&req).unwrap();
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        // Anthropic format: name, description, input_schema (not "function" wrapper).
        assert_eq!(tools[0]["name"], "read_file");
        assert_eq!(tools[0]["description"], "Read a file from disk");
        assert!(tools[0].get("input_schema").is_some());
        assert!(tools[0].get("function").is_none());
        assert_eq!(tools[0]["input_schema"]["required"][0], "path");
    }

    #[test]
    fn anthropic_body_default_model_and_max_tokens() {
        // When model and max_tokens are absent, defaults should apply.
        let req = json!({
            "messages": [
                {"role": "user", "content": "Hi"}
            ]
        });
        let body = build_anthropic_body(&req).unwrap();
        assert_eq!(body["model"], "claude-sonnet-4-20250514");
        assert_eq!(body["max_tokens"], 4096);
    }

    #[test]
    fn anthropic_body_ignores_unknown_roles() {
        let req = json!({
            "messages": [
                {"role": "system", "content": "sys"},
                {"role": "function", "content": "should be ignored"},
                {"role": "user", "content": "real message"}
            ]
        });
        let body = build_anthropic_body(&req).unwrap();
        let msgs = body["messages"].as_array().unwrap();
        // "function" role is ignored; only user message passes through.
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["content"], "real message");
    }

    #[test]
    fn anthropic_body_tool_with_missing_tool_call_id() {
        // tool_call_id missing → defaults to "unknown".
        let req = json!({
            "messages": [
                {"role": "tool", "content": "result data"}
            ]
        });
        let body = build_anthropic_body(&req).unwrap();
        let msgs = body["messages"].as_array().unwrap();
        let block = &msgs[0]["content"][0];
        assert_eq!(block["tool_use_id"], "unknown");
    }

    #[test]
    fn anthropic_body_tool_with_invalid_json_arguments() {
        // If arguments is not valid JSON, input should default to {}.
        let req = json!({
            "messages": [
                {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [
                        {
                            "id": "call_bad",
                            "function": {
                                "name": "broken",
                                "arguments": "not valid json {"
                            }
                        }
                    ]
                }
            ]
        });
        let body = build_anthropic_body(&req).unwrap();
        let blocks = body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(blocks[0]["input"], json!({}));
    }

    #[test]
    fn anthropic_body_multiple_user_messages_preserved() {
        let req = json!({
            "messages": [
                {"role": "user", "content": "first"},
                {"role": "assistant", "content": "reply"},
                {"role": "user", "content": "second"}
            ]
        });
        let body = build_anthropic_body(&req).unwrap();
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["content"], "first");
        assert_eq!(msgs[1]["content"], "reply");
        assert_eq!(msgs[2]["content"], "second");
    }

    // ── is_quota_exhausted_error (from utils) ────────────────────────

    #[test]
    fn quota_exhausted_detects_4006_code() {
        assert!(is_quota_exhausted_error("Error 4006: daily limit reached"));
    }

    #[test]
    fn quota_exhausted_detects_quota_keyword() {
        assert!(is_quota_exhausted_error("Your quota has been exceeded."));
    }

    #[test]
    fn quota_exhausted_negative_for_generic_error() {
        assert!(!is_quota_exhausted_error("Internal server error"));
        assert!(!is_quota_exhausted_error(""));
    }

    // ── estimate_tokens (from utils) ─────────────────────────────────

    #[test]
    fn estimate_tokens_empty_string_returns_one() {
        assert_eq!(estimate_tokens(""), 1);
    }

    #[test]
    fn estimate_tokens_short_text_returns_at_least_one() {
        let est = estimate_tokens("Hi");
        assert!(est >= 1);
    }

    #[test]
    fn estimate_tokens_code_higher_than_prose() {
        // Code has more symbols → higher token estimate per char.
        let prose = "This is a simple sentence that a user might type in a chat window.";
        let code = "fn main() { let x: Vec<u32> = (0..100).filter(|n| n % 2 == 0).collect(); }";
        let prose_est = estimate_tokens(prose);
        let code_est = estimate_tokens(code);
        // Same length roughly, but code should estimate more tokens.
        assert!(
            code_est >= prose_est,
            "code_est={code_est} should be >= prose_est={prose_est}"
        );
    }

    // ── build_request (from utils) ───────────────────────────────────

    fn test_model_info(
        api_style: ApiStyle,
        supports_tools: bool,
        supports_thinking: bool,
    ) -> ModelInfo {
        ModelInfo {
            id: "test-model".to_string(),
            label: "Test Model".to_string(),
            api_style,
            supports_tools,
            supports_thinking,
        }
    }

    #[test]
    fn build_request_openai_chat_includes_messages() {
        let profile = test_model_info(ApiStyle::OpenAiChat, true, false);
        let messages = vec![crate::agent::models::ChatMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }];
        let req = build_request(
            &profile,
            "gpt-4o",
            &messages,
            &[],
            false,
            AiProvider::OpenRouter,
        );
        assert_eq!(req["model"], "gpt-4o");
        assert_eq!(req["stream"], true);
        assert!(req["messages"].is_array());
        assert_eq!(req["messages"][0]["content"], "Hello");
        // No tools → no tools key.
        assert!(req.get("tools").is_none());
    }

    #[test]
    fn build_request_openai_chat_with_tools() {
        let profile = test_model_info(ApiStyle::OpenAiTools, true, false);
        let messages = vec![crate::agent::models::ChatMessage {
            role: "user".to_string(),
            content: "Search".to_string(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }];
        let tools = vec![json!({"type": "function", "function": {"name": "search"}})];
        let req = build_request(
            &profile,
            "gpt-4o",
            &messages,
            &tools,
            false,
            AiProvider::OpenRouter,
        );
        assert!(req["tools"].is_array());
        assert_eq!(req["tools"][0]["function"]["name"], "search");
    }

    #[test]
    fn build_request_prompt_completion_uses_prompt_field() {
        let profile = test_model_info(ApiStyle::PromptCompletion, false, false);
        let messages = vec![crate::agent::models::ChatMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }];
        let req = build_request(
            &profile,
            "llama3",
            &messages,
            &[],
            false,
            AiProvider::CloudflareWorkersAi,
        );
        assert!(req.get("prompt").is_some());
        assert!(req.get("messages").is_none());
        assert!(req["prompt"].as_str().unwrap().contains("user: Hello"));
    }

    #[test]
    fn build_request_thinking_cloudflare() {
        let profile = test_model_info(ApiStyle::OpenAiChat, false, true);
        let messages = vec![crate::agent::models::ChatMessage {
            role: "user".to_string(),
            content: "Think".to_string(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }];
        let req = build_request(
            &profile,
            "model",
            &messages,
            &[],
            true,
            AiProvider::CloudflareWorkersAi,
        );
        assert_eq!(req["thinking"], true);
    }

    #[test]
    fn build_request_thinking_openrouter() {
        let profile = test_model_info(ApiStyle::OpenAiChat, false, true);
        let messages = vec![crate::agent::models::ChatMessage {
            role: "user".to_string(),
            content: "Think".to_string(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }];
        let req = build_request(
            &profile,
            "model",
            &messages,
            &[],
            true,
            AiProvider::OpenRouter,
        );
        assert_eq!(req["reasoning"]["effort"], "high");
        assert_eq!(req["reasoning"]["exclude"], false);
    }

    #[test]
    fn build_request_thinking_azure() {
        let profile = test_model_info(ApiStyle::OpenAiChat, false, true);
        let messages = vec![crate::agent::models::ChatMessage {
            role: "user".to_string(),
            content: "Think".to_string(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }];
        let req = build_request(
            &profile,
            "model",
            &messages,
            &[],
            true,
            AiProvider::AzureOpenAi,
        );
        assert_eq!(req["reasoning_effort"], "high");
    }

    #[test]
    fn build_request_thinking_ollama() {
        let profile = test_model_info(ApiStyle::OpenAiChat, false, true);
        let messages = vec![crate::agent::models::ChatMessage {
            role: "user".to_string(),
            content: "Think".to_string(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }];
        let req = build_request(
            &profile,
            "model",
            &messages,
            &[],
            true,
            AiProvider::LocalOllama,
        );
        assert_eq!(req["think"], true);
    }

    #[test]
    fn build_request_thinking_disabled_no_thinking_key() {
        let profile = test_model_info(ApiStyle::OpenAiChat, false, true);
        let messages = vec![crate::agent::models::ChatMessage {
            role: "user".to_string(),
            content: "No think".to_string(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }];
        let req = build_request(
            &profile,
            "model",
            &messages,
            &[],
            false,
            AiProvider::OpenRouter,
        );
        assert!(req.get("reasoning").is_none());
        assert!(req.get("thinking").is_none());
        assert!(req.get("think").is_none());
    }

    #[test]
    fn build_request_no_tools_when_unsupported() {
        let profile = test_model_info(ApiStyle::OpenAiChat, false, false);
        let messages = vec![crate::agent::models::ChatMessage {
            role: "user".to_string(),
            content: "Hi".to_string(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }];
        let tools = vec![json!({"type": "function", "function": {"name": "search"}})];
        let req = build_request(
            &profile,
            "model",
            &messages,
            &tools,
            false,
            AiProvider::OpenRouter,
        );
        // Model doesn't support tools → tools key should be absent.
        assert!(req.get("tools").is_none());
    }
}
