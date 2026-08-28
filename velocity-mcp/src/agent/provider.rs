use super::models::*;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

/// Cached model catalog entry with expiration.
struct CachedCatalog {
    models: Vec<ModelInfo>,
    fetched_at: Instant,
}

/// Global model catalog cache (provider -> cached catalog).
/// Reduces API calls when users switch between providers or refresh model lists.
static MODEL_CATALOG_CACHE: LazyLock<Mutex<std::collections::HashMap<String, CachedCatalog>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// Cache TTL for model catalogs (10 minutes).
const CATALOG_CACHE_TTL: Duration = Duration::from_secs(600);

/// Get cached models for a provider, or None if cache miss/expired.
fn get_cached_catalog(provider_key: &str) -> Option<Vec<ModelInfo>> {
    let cache = MODEL_CATALOG_CACHE.lock().ok()?;
    let entry = cache.get(provider_key)?;
    if entry.fetched_at.elapsed() < CATALOG_CACHE_TTL {
        Some(entry.models.clone())
    } else {
        None
    }
}

/// Store models in the cache for a provider.
fn set_cached_catalog(provider_key: &str, models: Vec<ModelInfo>) {
    if let Ok(mut cache) = MODEL_CATALOG_CACHE.lock() {
        cache.insert(
            provider_key.to_string(),
            CachedCatalog {
                models,
                fetched_at: Instant::now(),
            },
        );
    }
}

pub fn openrouter_api_key() -> String {
    std::env::var("OPENROUTER_API_KEY").unwrap_or_default()
}

pub fn infer_model_info(id: String, item: &serde_json::Value) -> Option<ModelInfo> {
    let lower = id.to_lowercase();
    let task = item["task"].as_str().unwrap_or("").to_lowercase();
    let description = item["description"].as_str().unwrap_or("").to_lowercase();
    let non_chat = task.contains("embedding")
        || task.contains("image")
        || task.contains("speech")
        || task.contains("audio")
        || lower.contains("embedding")
        || lower.contains("rerank")
        || lower.contains("stable-diffusion")
        || lower.contains("whisper");
    if non_chat {
        return None;
    }

    let metadata = serde_json::to_string(item)
        .unwrap_or_default()
        .to_lowercase();
    let supports_tools = lower.contains("function-calling")
        || lower.contains("tool-use")
        || lower.contains("kimi")
        || lower.contains("llama-3.1")
        || lower.contains("llama-3.2")
        || lower.contains("llama-3.3")
        || lower.contains("qwen2.5")
        || lower.contains("qwen3")
        || lower.contains("nemotron")
        || lower.contains("mistral")
        || lower.contains("mixtral")
        || lower.contains("gemma-3")
        || lower.contains("command-r")
        || lower.contains("deepseek-v3")
        || lower.contains("deepseek-r1")
        || lower.contains("gpt-4")
        || lower.contains("gpt-3.5")
        || lower.contains("claude-3")
        || lower.contains("claude-opus")
        || lower.contains("claude-sonnet")
        || lower.contains("claude-haiku")
        || description.contains("function calling")
        || description.contains("tool calling")
        || description.contains("tool use");
    let prompt_only = (task.contains("text-generation") || metadata.contains("text-generation"))
        && !lower.contains("instruct")
        && !metadata.contains("chat")
        && !supports_tools;
    let supports_thinking = lower.contains("thinking")
        || lower.contains("reasoning")
        || lower.contains("kimi")
        || lower.contains("deepseek-r1")
        || lower.contains("o1-")
        || lower.contains("o3-")
        || lower.contains("qwq");
    Some(ModelInfo {
        label: id.rsplit('/').next().unwrap_or(&id).to_string(),
        id,
        api_style: if prompt_only {
            ApiStyle::PromptCompletion
        } else if supports_tools {
            ApiStyle::OpenAiTools
        } else {
            ApiStyle::OpenAiChat
        },
        supports_tools,
        supports_thinking,
    })
}

pub fn infer_openrouter_model_info(item: &serde_json::Value) -> Option<ModelInfo> {
    let id = item["id"].as_str()?.to_string();
    let lower = id.to_lowercase();
    let arch = item["architecture"]["tokenizer"]
        .as_str()
        .unwrap_or("")
        .to_lowercase();
    if arch.contains("embed") || lower.contains("embed") || lower.contains("stable-diffusion") {
        return None;
    }
    let name = item["name"].as_str().unwrap_or(&id).to_string();
    let label = name.clone();
    let supports_thinking = lower.contains("think")
        || lower.contains("reason")
        || lower.contains("kimi")
        || lower.contains("deepseek-r1")
        || lower.contains("qwq")
        || lower.contains("/o1")
        || lower.contains("/o3");

    let supports_tools = true;

    Some(ModelInfo {
        label,
        id,
        api_style: ApiStyle::OpenAiTools,
        supports_tools,
        supports_thinking,
    })
}

pub fn fetch_model_catalog(
    accounts: &[crate::usage::CloudflareAccount],
) -> Result<Vec<ModelInfo>, String> {
    for account in accounts {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/ai/models/search",
            account.id
        );
        let response = ureq::get(&url)
            .timeout(std::time::Duration::from_secs(15))
            .set("Authorization", &format!("Bearer {}", account.token))
            .set("Accept", "application/json")
            .call()
            .map_err(|e| format!("Workers AI model catalog request failed: {e}"))?;
        let body: serde_json::Value = response
            .into_json()
            .map_err(|e| format!("Workers AI model catalog response was invalid: {e}"))?;
        let mut models = body["result"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| {
                ["name", "model", "id"]
                    .iter()
                    .find_map(|key| item[*key].as_str())
                    .map(str::to_string)
                    .and_then(|id| infer_model_info(id, item))
            })
            .filter(|model| model.id.contains('/'))
            .collect::<Vec<_>>();
        models.sort_by(|a, b| a.id.cmp(&b.id));
        models.dedup_by(|a, b| a.id == b.id);
        if !models.is_empty() {
            return Ok(models);
        }
    }
    Err("No Workers AI models were returned for the configured accounts.".into())
}

pub fn fetch_openrouter_models(
    or_accounts: &[crate::usage::OpenRouterAccount],
    usage_tracker: &crate::usage::UsageTracker,
) -> Result<Vec<ModelInfo>, String> {
    let mut response = None;
    let mut last_err = String::new();

    let start_idx = usage_tracker
        .pick_or_account(or_accounts)
        .and_then(|picked| or_accounts.iter().position(|a| a.n == picked.n))
        .unwrap_or(0);

    let loop_limit = or_accounts.len().max(1);
    for idx in 0..loop_limit {
        let current_key = if or_accounts.is_empty() {
            openrouter_api_key()
        } else {
            let acct = &or_accounts[(start_idx + idx) % or_accounts.len()];
            if usage_tracker.is_or_exhausted(acct.n) {
                continue;
            }
            acct.token.clone()
        };

        if current_key.trim().is_empty() {
            continue;
        }

        match ureq::get("https://openrouter.ai/api/v1/models")
            .timeout(std::time::Duration::from_secs(15))
            .set("Authorization", &format!("Bearer {}", current_key))
            .set("HTTP-Referer", "https://velocity-ide.local")
            .set("X-Title", "Velocity Cognitive IDE")
            .set("Accept", "application/json")
            .call()
        {
            Ok(res) => {
                response = Some(res);
                break;
            }
            Err(e) => {
                last_err = format!("OpenRouter model catalog request failed: {e}");
            }
        }
    }

    let res_unwrapped = match response {
        Some(r) => r,
        None => {
            return Err(if last_err.is_empty() {
                "No available OpenRouter accounts to fetch models.".to_string()
            } else {
                last_err
            })
        }
    };

    let body: serde_json::Value = res_unwrapped
        .into_json()
        .map_err(|e| format!("OpenRouter model catalog response invalid: {e}"))?;

    let goal = ModelInfo {
        id: "tencent/hy3:free".to_string(),
        label: "HunyuanLarge (hy3) Free".to_string(),
        api_style: ApiStyle::OpenAiChat,
        supports_tools: false,
        supports_thinking: false,
    };

    let mut models: Vec<ModelInfo> = body["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(infer_openrouter_model_info)
        .collect();

    models.retain(|m| m.id != goal.id);
    models.insert(0, goal);

    models.sort_by(|a, b| {
        let a_free = a.id.ends_with(":free");
        let b_free = b.id.ends_with(":free");
        match (a_free, b_free) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.id.cmp(&b.id),
        }
    });

    if let Some(pos) = models.iter().position(|m| m.id == "tencent/hy3:free") {
        if pos != 0 {
            let entry = models.remove(pos);
            models.insert(0, entry);
        }
    }

    if models.is_empty() {
        return Err("No OpenRouter models returned.".into());
    }
    Ok(models)
}

pub fn default_model_info(id: &str) -> ModelInfo {
    let lower = id.to_lowercase();
    infer_model_info(id.to_string(), &serde_json::Value::Null).unwrap_or(ModelInfo {
        id: id.to_string(),
        label: id.rsplit('/').next().unwrap_or(id).to_string(),
        api_style: ApiStyle::OpenAiChat,
        supports_tools: false,
        supports_thinking: lower.contains("think")
            || lower.contains("reason")
            || lower.contains("kimi")
            || lower.contains("deepseek-r1")
            || lower.contains("qwq")
            || lower.contains("o1-")
            || lower.contains("o3-"),
    })
}

pub fn enrich_model_profile(
    accounts: &[crate::usage::CloudflareAccount],
    profile: &ModelInfo,
) -> ModelInfo {
    let Some(account) = accounts.first() else {
        return profile.clone();
    };
    let encoded_model = profile
        .id
        .replace('%', "%25")
        .replace('/', "%2F")
        .replace('@', "%40");
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/ai/models/schema?model={}",
        account.id, encoded_model
    );
    let Ok(response) = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .set("Authorization", &format!("Bearer {}", account.token))
        .set("Accept", "application/json")
        .call()
    else {
        return profile.clone();
    };
    let Ok(body) = response.into_json::<serde_json::Value>() else {
        return profile.clone();
    };
    let input_description = body["result"]["input"]["description"]
        .as_str()
        .unwrap_or("")
        .to_lowercase();
    if input_description.is_empty() {
        return profile.clone();
    }
    let mut enriched = profile.clone();
    enriched.supports_tools =
        input_description.contains("tool") || input_description.contains("function");
    enriched.supports_thinking =
        input_description.contains("thinking") || input_description.contains("reasoning");
    if input_description.contains("prompt") && !input_description.contains("message") {
        enriched.api_style = ApiStyle::PromptCompletion;
    } else if enriched.supports_tools {
        enriched.api_style = ApiStyle::OpenAiTools;
    } else {
        enriched.api_style = ApiStyle::OpenAiChat;
    }
    enriched
}

pub fn fallback_provider(current: AiProvider) -> AiProvider {
    match current {
        AiProvider::CloudflareWorkersAi => AiProvider::OpenRouter,
        AiProvider::OpenRouter => AiProvider::AzureOpenAi,
        AiProvider::AzureOpenAi => AiProvider::LocalOllama,
        AiProvider::LocalOllama => AiProvider::CloudflareWorkersAi,
        AiProvider::OpenAI => AiProvider::CloudflareWorkersAi,
        AiProvider::Anthropic => AiProvider::CloudflareWorkersAi,
        AiProvider::GoogleVertex => AiProvider::CloudflareWorkersAi,
        AiProvider::Deepseek => AiProvider::OpenRouter,
        AiProvider::AlibabaQwen => AiProvider::OpenRouter,
        AiProvider::AwsBedrock => AiProvider::OpenRouter,
        AiProvider::Groq => AiProvider::OpenRouter,
        AiProvider::Mistral => AiProvider::OpenRouter,
        AiProvider::TogetherAi => AiProvider::OpenRouter,
        AiProvider::FireworksAi => AiProvider::OpenRouter,
        AiProvider::Perplexity => AiProvider::OpenRouter,
        AiProvider::Cerebras => AiProvider::OpenRouter,
    }
}

pub fn default_provider_model(provider: AiProvider) -> String {
    match provider {
        AiProvider::CloudflareWorkersAi => "@cf/moonshotai/kimi-k2.7-code".to_string(),
        AiProvider::OpenRouter => "tencent/hy3:free".to_string(),
        AiProvider::OpenAI => "gpt-4o".to_string(),
        AiProvider::Anthropic => "claude-3-5-sonnet-20241022".to_string(),
        AiProvider::GoogleVertex => "gemini-1.5-pro".to_string(),
        AiProvider::AzureOpenAi => "gpt-4o".to_string(),
        AiProvider::LocalOllama => "llama3.2".to_string(),
        AiProvider::Deepseek => "deepseek-chat".to_string(),
        AiProvider::AlibabaQwen => "qwen-plus".to_string(),
        AiProvider::AwsBedrock => "anthropic.claude-3-sonnet-20240229-v1:0".to_string(),
        AiProvider::Groq => "llama-3.3-70b-versatile".to_string(),
        AiProvider::Mistral => "mistral-large-latest".to_string(),
        AiProvider::TogetherAi => "meta-llama/Meta-Llama-3.1-405B-Instruct-Turbo".to_string(),
        AiProvider::FireworksAi => "accounts/fireworks/models/llama-v3p3-70b-instruct".to_string(),
        AiProvider::Perplexity => "sonar-pro".to_string(),
        AiProvider::Cerebras => "llama-3.3-70b".to_string(),
    }
}

pub fn fetch_local_ollama_models(
    accounts: &[crate::usage::LocalOllamaAccount],
) -> Result<Vec<ModelInfo>, String> {
    let host = accounts
        .first()
        .map(|a| a.host.as_str())
        .unwrap_or("http://localhost:11434");
    Ok(vec![
        ModelInfo {
            id: "llama3.2".to_string(),
            label: format!("llama3.2 ({host})"),
            api_style: ApiStyle::OpenAiTools,
            supports_tools: true,
            supports_thinking: false,
        },
        ModelInfo {
            id: "qwen2.5-coder".to_string(),
            label: format!("qwen2.5-coder ({host})"),
            api_style: ApiStyle::OpenAiTools,
            supports_tools: true,
            supports_thinking: false,
        },
        ModelInfo {
            id: "deepseek-r1".to_string(),
            label: format!("deepseek-r1 ({host})"),
            api_style: ApiStyle::OpenAiTools,
            supports_tools: true,
            supports_thinking: true,
        },
    ])
}

pub fn fetch_azure_models(
    accounts: &[crate::usage::AzureOpenAiAccount],
) -> Result<Vec<ModelInfo>, String> {
    if accounts.is_empty() {
        return Ok(vec![
            ModelInfo {
                id: "gpt-4o".to_string(),
                label: "GPT-4o (Azure)".to_string(),
                api_style: ApiStyle::OpenAiTools,
                supports_tools: true,
                supports_thinking: false,
            },
            ModelInfo {
                id: "gpt-4o-mini".to_string(),
                label: "GPT-4o Mini (Azure)".to_string(),
                api_style: ApiStyle::OpenAiTools,
                supports_tools: true,
                supports_thinking: false,
            },
            ModelInfo {
                id: "o1".to_string(),
                label: "o1 (Azure)".to_string(),
                api_style: ApiStyle::OpenAiTools,
                supports_tools: true,
                supports_thinking: true,
            },
        ]);
    }
    let mut catalog = Vec::new();
    for acct in accounts {
        catalog.push(ModelInfo {
            id: acct.deployment.clone(),
            label: format!("{} ({})", acct.deployment, acct.label),
            api_style: ApiStyle::OpenAiTools,
            supports_tools: true,
            supports_thinking: acct.deployment.contains("o1") || acct.deployment.contains("o3"),
        });
    }
    Ok(catalog)
}

/// Generic fetcher for any OpenAI-compatible `/v1/models` endpoint.
/// Used by OpenAI, Groq, Mistral, Deepseek, AlibabaQwen, Together, Fireworks, Perplexity, Cerebras.
fn fetch_openai_compatible_models(
    base_url: &str,
    api_key: &str,
    provider_label: &str,
) -> Result<Vec<ModelInfo>, String> {
    if api_key.trim().is_empty() {
        return Err(format!("No API key configured for {provider_label}"));
    }
    // Check cache first
    let cache_key = format!("{}:{}", provider_label, base_url);
    if let Some(cached) = get_cached_catalog(&cache_key) {
        return Ok(cached);
    }
    let url = format!("{base_url}/v1/models");
    let response = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(15))
        .set("Authorization", &format!("Bearer {api_key}"))
        .set("Accept", "application/json")
        .call()
        .map_err(|e| format!("{provider_label} model catalog request failed: {e}"))?;
    let body: serde_json::Value = response
        .into_json()
        .map_err(|e| format!("{provider_label} model catalog response invalid: {e}"))?;
    let mut models: Vec<ModelInfo> = body["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let id = item["id"].as_str()?.to_string();
            let lower = id.to_lowercase();
            // Filter out embedding/non-chat models
            if lower.contains("embed")
                || lower.contains("whisper")
                || lower.contains("tts")
                || lower.contains("image")
            {
                return None;
            }
            let supports_thinking = lower.contains("thinking")
                || lower.contains("reasoning")
                || lower.contains("deepseek-r1")
                || lower.contains("qwq")
                || lower.contains("o1")
                || lower.contains("o3");
            let supports_tools = lower.contains("function")
                || lower.contains("tool")
                || lower.contains("llama-3")
                || lower.contains("llama-4")
                || lower.contains("qwen")
                || lower.contains("gpt-4")
                || lower.contains("gpt-3.5")
                || lower.contains("mistral")
                || lower.contains("mixtral")
                || lower.contains("deepseek")
                || lower.contains("claude")
                || lower.contains("gemini")
                || lower.contains("sonar")
                || lower.contains("meta-llama");
            Some(ModelInfo {
                label: id.rsplit('/').next().unwrap_or(&id).to_string(),
                id: id.clone(),
                api_style: if supports_tools {
                    ApiStyle::OpenAiTools
                } else {
                    ApiStyle::OpenAiChat
                },
                supports_tools,
                supports_thinking,
            })
        })
        .collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    if models.is_empty() {
        return Err(format!("No models returned by {provider_label}"));
    }
    // Cache the result for future calls
    set_cached_catalog(&cache_key, models.clone());
    Ok(models)
}

pub fn fetch_openai_models(api_key: &str) -> Result<Vec<ModelInfo>, String> {
    fetch_openai_compatible_models("https://api.openai.com", api_key, "OpenAI")
}

pub fn fetch_groq_models(api_key: &str) -> Result<Vec<ModelInfo>, String> {
    fetch_openai_compatible_models("https://api.groq.com", api_key, "Groq")
}

pub fn fetch_mistral_models(api_key: &str) -> Result<Vec<ModelInfo>, String> {
    fetch_openai_compatible_models("https://api.mistral.ai", api_key, "Mistral")
}

pub fn fetch_deepseek_models(api_key: &str) -> Result<Vec<ModelInfo>, String> {
    fetch_openai_compatible_models("https://api.deepseek.com", api_key, "Deepseek")
}

pub fn fetch_alibaba_models(api_key: &str) -> Result<Vec<ModelInfo>, String> {
    fetch_openai_compatible_models(
        "https://dashscope.aliyuncs.com/compatible-mode",
        api_key,
        "Alibaba Qwen",
    )
}

pub fn fetch_google_models(api_key: &str) -> Result<Vec<ModelInfo>, String> {
    // Google Gemini API uses a different endpoint format
    if api_key.trim().is_empty() {
        return Err("No API key configured for Google Vertex AI".to_string());
    }
    let url = format!("https://generativelanguage.googleapis.com/v1beta/models?key={api_key}");
    let response = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(15))
        .set("Accept", "application/json")
        .call()
        .map_err(|e| format!("Google Vertex model catalog request failed: {e}"))?;
    let body: serde_json::Value = response
        .into_json()
        .map_err(|e| format!("Google Vertex model catalog response invalid: {e}"))?;
    let mut models: Vec<ModelInfo> = body["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let name = item["name"].as_str().unwrap_or("").to_string();
            let id = name.strip_prefix("models/").unwrap_or(&name).to_string();
            let lower = id.to_lowercase();
            if lower.contains("embed") || lower.contains("image") || lower.contains("tts") {
                return None;
            }
            let supports_thinking = lower.contains("thinking")
                || lower.contains("reasoning")
                || lower.contains("flash");
            let supports_tools =
                lower.contains("gemini") || lower.contains("flash") || lower.contains("pro");
            Some(ModelInfo {
                label: id.replace('-', " ").replace("gemini ", "Gemini "),
                id: id.clone(),
                api_style: if supports_tools {
                    ApiStyle::OpenAiTools
                } else {
                    ApiStyle::OpenAiChat
                },
                supports_tools,
                supports_thinking,
            })
        })
        .collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    if models.is_empty() {
        return Err("No models returned by Google Vertex AI".into());
    }
    Ok(models)
}

pub fn fetch_together_models(api_key: &str) -> Result<Vec<ModelInfo>, String> {
    fetch_openai_compatible_models("https://api.together.xyz", api_key, "Together AI")
}

pub fn fetch_fireworks_models(api_key: &str) -> Result<Vec<ModelInfo>, String> {
    fetch_openai_compatible_models(
        "https://api.fireworks.ai/inference",
        api_key,
        "Fireworks AI",
    )
}

pub fn fetch_perplexity_models(api_key: &str) -> Result<Vec<ModelInfo>, String> {
    fetch_openai_compatible_models("https://api.perplexity.ai", api_key, "Perplexity")
}

pub fn fetch_cerebras_models(api_key: &str) -> Result<Vec<ModelInfo>, String> {
    fetch_openai_compatible_models("https://api.cerebras.ai", api_key, "Cerebras")
}

/// Anthropic Messages API — GET /v1/models with x-api-key auth.
pub fn fetch_anthropic_models(api_key: &str) -> Result<Vec<ModelInfo>, String> {
    if api_key.trim().is_empty() {
        return Err("No API key configured for Anthropic".to_string());
    }
    // Check cache first
    let cache_key = "Anthropic:https://api.anthropic.com";
    if let Some(cached) = get_cached_catalog(cache_key) {
        return Ok(cached);
    }
    let response = ureq::get("https://api.anthropic.com/v1/models")
        .timeout(std::time::Duration::from_secs(15))
        .set("x-api-key", api_key)
        .set("anthropic-version", "2023-06-01")
        .set("Accept", "application/json")
        .call()
        .map_err(|e| format!("Anthropic model catalog request failed: {e}"))?;
    let body: serde_json::Value = response
        .into_json()
        .map_err(|e| format!("Anthropic model catalog parse failed: {e}"))?;
    let models = body
        .get("data")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();
    let result: Vec<ModelInfo> = models
        .iter()
        .filter_map(|m| {
            let id = m.get("id").and_then(|v| v.as_str())?;
            let display = m.get("display_name").and_then(|v| v.as_str()).unwrap_or(id);
            Some(ModelInfo {
                id: id.to_string(),
                label: display.to_string(),
                api_style: ApiStyle::OpenAiTools,
                supports_tools: id.contains("claude") && !id.contains("haiku"),
                supports_thinking: id.contains("claude")
                    && (id.contains("sonnet") || id.contains("opus")),
            })
        })
        .collect();
    // Cache the result
    set_cached_catalog(cache_key, result.clone());
    Ok(result)
}

/// AWS Bedrock — uses BEDROCK_PROXY_URL as an OpenAI-compatible gateway.
pub fn fetch_bedrock_models() -> Result<Vec<ModelInfo>, String> {
    if let Ok(proxy_url) = std::env::var("BEDROCK_PROXY_URL") {
        if !proxy_url.trim().is_empty() {
            let api_key = std::env::var("BEDROCK_API_KEY").unwrap_or_default();
            return fetch_openai_compatible_models(
                proxy_url.trim_end_matches('/'),
                &api_key,
                "AWS Bedrock",
            );
        }
    }
    Err("AWS Bedrock requires BEDROCK_PROXY_URL env var pointing to an OpenAI-compatible proxy. Configure in Settings > Provider Settings.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── infer_model_info ───────────────────────────────────────────────

    #[test]
    fn infer_model_info_filters_embeddings() {
        let item = serde_json::json!({"task": "text-embedding"});
        assert!(infer_model_info("my-embedding-model".into(), &item).is_none());
    }

    #[test]
    fn infer_model_info_filters_image_models() {
        let item = serde_json::json!({"task": "image-generation"});
        assert!(infer_model_info("stable-diffusion-xl".into(), &item).is_none());
    }

    #[test]
    fn infer_model_info_filters_rerank() {
        assert!(infer_model_info("bge-reranker-v2".into(), &serde_json::json!(null)).is_none());
    }

    #[test]
    fn infer_model_info_detects_tool_support() {
        let item = serde_json::json!({});
        let info = infer_model_info("gpt-4-turbo".into(), &item).unwrap();
        assert!(info.supports_tools);
        assert_eq!(info.api_style, ApiStyle::OpenAiTools);
    }

    #[test]
    fn infer_model_info_detects_thinking() {
        let item = serde_json::json!({});
        let info = infer_model_info("deepseek-r1".into(), &item).unwrap();
        assert!(info.supports_thinking);
    }

    #[test]
    fn infer_model_info_plain_chat_model() {
        let item = serde_json::json!({});
        let info = infer_model_info("llama-2-7b".into(), &item).unwrap();
        assert!(!info.supports_tools);
        assert!(!info.supports_thinking);
        assert_eq!(info.api_style, ApiStyle::OpenAiChat);
    }

    #[test]
    fn infer_model_info_extracts_label_from_path() {
        let item = serde_json::json!({});
        let info = infer_model_info("org/namespace/my-model".into(), &item).unwrap();
        assert_eq!(info.label, "my-model");
    }

    #[test]
    fn infer_model_info_prompt_only_api_style() {
        let item = serde_json::json!({"task": "text-generation"});
        let info = infer_model_info("some-legacy-model".into(), &item).unwrap();
        assert_eq!(info.api_style, ApiStyle::PromptCompletion);
    }

    // ── infer_openrouter_model_info ────────────────────────────────────

    #[test]
    fn openrouter_infers_chat_model() {
        let item = serde_json::json!({
            "id": "anthropic/claude-3.5-sonnet",
            "name": "Claude 3.5 Sonnet",
            "architecture": {"tokenizer": "claude"}
        });
        let info = infer_openrouter_model_info(&item).unwrap();
        assert_eq!(info.id, "anthropic/claude-3.5-sonnet");
        assert_eq!(info.label, "Claude 3.5 Sonnet");
        assert!(info.supports_tools);
    }

    #[test]
    fn openrouter_filters_embedding_models() {
        let item = serde_json::json!({
            "id": "openai/text-embedding-3-small",
            "name": "Embedding",
            "architecture": {"tokenizer": "embed"}
        });
        assert!(infer_openrouter_model_info(&item).is_none());
    }

    #[test]
    fn openrouter_detects_thinking_models() {
        let item = serde_json::json!({
            "id": "deepseek/deepseek-r1",
            "name": "DeepSeek R1",
            "architecture": {"tokenizer": "llama"}
        });
        let info = infer_openrouter_model_info(&item).unwrap();
        assert!(info.supports_thinking);
    }

    #[test]
    fn openrouter_returns_none_for_missing_id() {
        let item = serde_json::json!({"name": "No ID model"});
        assert!(infer_openrouter_model_info(&item).is_none());
    }

    // ── default_model_info ─────────────────────────────────────────────

    #[test]
    fn default_model_info_known_model() {
        let info = default_model_info("gpt-4-turbo");
        assert!(info.supports_tools);
    }

    #[test]
    fn default_model_info_unknown_model() {
        let info = default_model_info("totally-unknown-model-xyz");
        assert_eq!(info.id, "totally-unknown-model-xyz");
        assert!(!info.supports_tools);
        assert!(!info.supports_thinking);
    }

    #[test]
    fn default_model_info_thinking_model() {
        let info = default_model_info("kimi-k2");
        assert!(info.supports_thinking);
    }

    // ── fallback_provider ──────────────────────────────────────────────

    #[test]
    fn fallback_chain_cloudflare_to_openrouter() {
        assert_eq!(
            fallback_provider(AiProvider::CloudflareWorkersAi),
            AiProvider::OpenRouter
        );
    }

    #[test]
    fn fallback_chain_openrouter_to_azure() {
        assert_eq!(
            fallback_provider(AiProvider::OpenRouter),
            AiProvider::AzureOpenAi
        );
    }

    #[test]
    fn fallback_chain_azure_to_ollama() {
        assert_eq!(
            fallback_provider(AiProvider::AzureOpenAi),
            AiProvider::LocalOllama
        );
    }

    #[test]
    fn fallback_chain_ollama_wraps_to_cloudflare() {
        assert_eq!(
            fallback_provider(AiProvider::LocalOllama),
            AiProvider::CloudflareWorkersAi
        );
    }

    #[test]
    fn fallback_chain_all_providers_have_fallback() {
        // Verify every provider has a fallback (doesn't panic)
        let providers = vec![
            AiProvider::CloudflareWorkersAi,
            AiProvider::OpenRouter,
            AiProvider::AzureOpenAi,
            AiProvider::LocalOllama,
            AiProvider::OpenAI,
            AiProvider::Anthropic,
            AiProvider::GoogleVertex,
            AiProvider::Deepseek,
            AiProvider::AlibabaQwen,
            AiProvider::AwsBedrock,
            AiProvider::Groq,
            AiProvider::Mistral,
            AiProvider::TogetherAi,
            AiProvider::FireworksAi,
            AiProvider::Perplexity,
            AiProvider::Cerebras,
        ];
        for p in providers {
            let fb = fallback_provider(p);
            assert_ne!(fb, p, "fallback for {:?} should not be itself", p);
        }
    }

    // ── default_provider_model ─────────────────────────────────────────

    #[test]
    fn default_model_per_provider_non_empty() {
        let providers = vec![
            AiProvider::CloudflareWorkersAi,
            AiProvider::OpenRouter,
            AiProvider::OpenAI,
            AiProvider::Anthropic,
            AiProvider::GoogleVertex,
            AiProvider::AzureOpenAi,
            AiProvider::LocalOllama,
            AiProvider::Deepseek,
            AiProvider::AlibabaQwen,
            AiProvider::AwsBedrock,
            AiProvider::Groq,
            AiProvider::Mistral,
            AiProvider::TogetherAi,
            AiProvider::FireworksAi,
            AiProvider::Perplexity,
            AiProvider::Cerebras,
        ];
        for p in providers {
            let model = default_provider_model(p);
            assert!(!model.is_empty(), "default model for {:?} should not be empty", p);
        }
    }

    #[test]
    fn default_openrouter_model_is_hy3_free() {
        assert_eq!(default_provider_model(AiProvider::OpenRouter), "tencent/hy3:free");
    }

    #[test]
    fn default_openai_model_is_gpt4o() {
        assert_eq!(default_provider_model(AiProvider::OpenAI), "gpt-4o");
    }

    // ── fetch_local_ollama_models ──────────────────────────────────────

    #[test]
    fn ollama_models_always_returns_three() {
        let models = fetch_local_ollama_models(&[]).unwrap();
        assert_eq!(models.len(), 3);
    }

    #[test]
    fn ollama_models_contain_expected_ids() {
        let models = fetch_local_ollama_models(&[]).unwrap();
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"llama3.2"));
        assert!(ids.contains(&"qwen2.5-coder"));
        assert!(ids.contains(&"deepseek-r1"));
    }

    #[test]
    fn ollama_models_all_support_tools() {
        let models = fetch_local_ollama_models(&[]).unwrap();
        for m in &models {
            assert!(m.supports_tools);
        }
    }

    #[test]
    fn ollama_deepseek_r1_supports_thinking() {
        let models = fetch_local_ollama_models(&[]).unwrap();
        let ds = models.iter().find(|m| m.id == "deepseek-r1").unwrap();
        assert!(ds.supports_thinking);
    }

    #[test]
    fn ollama_models_use_custom_host() {
        let accounts = vec![crate::usage::LocalOllamaAccount {
            host: "http://myhost:1234".to_string(),
            default_model: String::new(),
            label: String::new(),
        }];
        let models = fetch_local_ollama_models(&accounts).unwrap();
        assert!(models[0].label.contains("myhost:1234"));
    }

    // ── fetch_azure_models ─────────────────────────────────────────────

    #[test]
    fn azure_empty_accounts_returns_defaults() {
        let models = fetch_azure_models(&[]).unwrap();
        assert_eq!(models.len(), 3);
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"gpt-4o"));
        assert!(ids.contains(&"gpt-4o-mini"));
        assert!(ids.contains(&"o1"));
    }

    #[test]
    fn azure_o1_supports_thinking() {
        let models = fetch_azure_models(&[]).unwrap();
        let o1 = models.iter().find(|m| m.id == "o1").unwrap();
        assert!(o1.supports_thinking);
    }

    #[test]
    fn azure_custom_deployment() {
        let accounts = vec![crate::usage::AzureOpenAiAccount {
            n: 0,
            api_key: "tok".to_string(),
            endpoint: "https://example.com".to_string(),
            deployment: "my-custom-gpt4".to_string(),
            api_version: "2024-01-01".to_string(),
            tier: "standard".to_string(),
            label: "My GPT-4".to_string(),
        }];
        let models = fetch_azure_models(&accounts).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "my-custom-gpt4");
        assert!(models[0].label.contains("My GPT-4"));
    }

    #[test]
    fn azure_o1_deployment_supports_thinking() {
        let accounts = vec![crate::usage::AzureOpenAiAccount {
            n: 0,
            api_key: "tok".to_string(),
            endpoint: "https://example.com".to_string(),
            deployment: "o1-preview".to_string(),
            api_version: "2024-01-01".to_string(),
            tier: "standard".to_string(),
            label: "O1".to_string(),
        }];
        let models = fetch_azure_models(&accounts).unwrap();
        assert!(models[0].supports_thinking);
    }

    // ── openrouter_api_key ─────────────────────────────────────────────

    #[test]
    fn openrouter_api_key_returns_string() {
        // Just verify it doesn't panic; value depends on env
        let _key = openrouter_api_key();
    }
}
