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
        // Zhipu GLM family (covers chatglm): the token-plan endpoint accepts
        // native tool_calls, verified live. Without this the profile fell
        // back to OpenAiChat and the agent had to be prompted to hand-write
        // tool markup, which streams unreliably.
        || lower.contains("glm")
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
        AiProvider::LocalOllama => "qwen2.5-coder:0.5b".to_string(),
        AiProvider::Deepseek => "deepseek-chat".to_string(),
        AiProvider::AlibabaQwen => "qwen-max".to_string(),
        AiProvider::AwsBedrock => "anthropic.claude-3-sonnet-20240229-v1:0".to_string(),
        AiProvider::Groq => "llama-3.3-70b-versatile".to_string(),
        AiProvider::Mistral => "mistral-large-latest".to_string(),
        AiProvider::TogetherAi => "meta-llama/Meta-Llama-3.1-405B-Instruct-Turbo".to_string(),
        AiProvider::FireworksAi => "accounts/fireworks/models/llama-v3p3-70b-instruct".to_string(),
        AiProvider::Perplexity => "sonar-pro".to_string(),
        AiProvider::Cerebras => "llama-3.3-70b".to_string(),
    }
}

/// Parse an Ollama `/api/tags` JSON body into `(id, supports_tools,
/// supports_thinking)` tuples. Split out as a pure function so the catalog is
/// testable without a running server and so cross-node merging has a single
/// place to reason about the wire shape.
///
/// Tool support is read from the server-advertised `capabilities` list rather
/// than assumed: a random GGUF pulled onto a host must not be silently offered
/// into the agent loop as if it could emit tool calls.
pub fn parse_ollama_tags(body: &serde_json::Value) -> Vec<(String, bool, bool)> {
    body["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let name = m["name"].as_str()?;
            let supports_tools = m["capabilities"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|c| c.as_str() == Some("tools"));
            let lower = name.to_lowercase();
            let supports_thinking = ["r1", "qwq", "thinking", "qwen3"]
                .iter()
                .any(|tag| lower.contains(tag));
            Some((name.to_string(), supports_tools, supports_thinking))
        })
        .collect()
}

/// Build the Ollama `/api/tags` URL from a host, tolerating a trailing slash.
pub fn ollama_tags_url(host: &str) -> String {
    format!("{}/api/tags", host.trim_end_matches('/'))
}

/// Query every configured Ollama host's `/api/tags` and merge the results into
/// one catalog. This is what makes cross-node model sharing real: each host
/// contributes the models it actually serves, labelled with the host that owns
/// them, and one unreachable host degrades gracefully instead of blanking the
/// whole set.
pub fn fetch_local_ollama_models(
    accounts: &[crate::usage::LocalOllamaAccount],
) -> Result<Vec<ModelInfo>, String> {
    // No accounts configured: still probe the default local host so a freshly
    // started Ollama is discoverable without any settings.
    let hosts: Vec<(String, String)> = if accounts.is_empty() {
        vec![(
            "http://localhost:11434".to_string(),
            "Local-Ollama".to_string(),
        )]
    } else {
        accounts
            .iter()
            .map(|a| {
                let display = if a.label.trim().is_empty() {
                    a.host.clone()
                } else {
                    a.label.clone()
                };
                (a.host.clone(), display)
            })
            .collect()
    };

    use std::collections::HashSet;
    let mut catalog = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut any_reachable = false;
    for (host, display) in hosts {
        let url = ollama_tags_url(&host);
        let body: Option<serde_json::Value> = match ureq::get(&url)
            .timeout(std::time::Duration::from_secs(8))
            .call()
        {
            Ok(r) => r.into_json().ok(),
            Err(_) => None,
        };
        let body = match body {
            Some(b) => b,
            None => continue,
        };
        any_reachable = true;
        for (name, supports_tools, supports_thinking) in parse_ollama_tags(&body) {
            if !seen.insert((host.clone(), name.clone())) {
                continue;
            }
            catalog.push(ModelInfo {
                id: name.clone(),
                label: format!("{name} ({display})"),
                api_style: ApiStyle::OpenAiTools,
                supports_tools,
                supports_thinking,
            });
        }
    }
    if !any_reachable {
        return Err(
            "No reachable Ollama host: every configured host failed its /api/tags probe."
                .to_string(),
        );
    }
    Ok(catalog)
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

/// Model families whose OpenAI-compatible endpoints accept a native `tools`
/// array. The catalog listing carries no capability field, so the model id is
/// the only signal available before the first request.
///
/// Keeping this as a named predicate (rather than an inline chain) matters:
/// the same table is consulted by the headless worker profile path via
/// `default_model_info`, and a family missing here silently degrades that
/// worker onto hand-written tool markup. glm-5.2 was missing on both tables
/// and the LiteLLM crucible stalled on turn 1 three runs in a row.
fn openai_compat_tool_family(lower: &str) -> bool {
    lower.contains("function")
        || lower.contains("tool")
        || lower.contains("llama-3")
        || lower.contains("llama-4")
        || lower.contains("meta-llama")
        || lower.contains("qwen")
        || lower.contains("glm")
        || lower.contains("gpt-4")
        || lower.contains("gpt-3.5")
        || lower.contains("mistral")
        || lower.contains("mixtral")
        || lower.contains("deepseek")
        || lower.contains("claude")
        || lower.contains("gemini")
        || lower.contains("sonar")
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
            // Filter out embedding/non-chat/routing models
            if lower.contains("embed")
                || lower.contains("whisper")
                || lower.contains("tts")
                || lower.contains("image")
                || lower.contains("audio")
                || lower == "auto"
            {
                return None;
            }
            let supports_thinking = lower.contains("thinking")
                || lower.contains("reasoning")
                || lower.contains("deepseek-r1")
                || lower.contains("qwq")
                || lower.contains("o1")
                || lower.contains("o3");
            let supports_tools = openai_compat_tool_family(&lower);
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
        "https://dashscope-intl.aliyuncs.com/compatible-mode",
        api_key,
        "Alibaba Qwen",
    )
}

/// Fetch the Alibaba Qwen model catalog from a caller-supplied base URL.
/// Used to route token-plan keys to the dedicated `token-plan.ap-southeast-1`
/// endpoint (which lists a different model set than the standard DashScope
/// compatible-mode endpoint).
///
/// The Token Plan endpoint exposes a superset list on `/v1/models` that
/// includes multimodal and non-chat variants which the chat-completions
/// route refuses. This function therefore **probes each listed model with a
/// minimal chat request against the SAME base URL** and prunes those the
/// endpoint rejects. It never falls back to the pay-as-you-go DashScope
/// endpoint — a model that works there but not here is NOT considered
/// available on the Token Plan.
pub fn fetch_alibaba_models_at(base_url: &str, api_key: &str) -> Result<Vec<ModelInfo>, String> {
    if api_key.trim().is_empty() {
        return Err("No API key configured for Alibaba Qwen".to_string());
    }
    // Distinct cache key so probed results never mix with the unprobed
    // generic listing, and Token Plan / DashScope-intl entries never share.
    let cache_key = format!("Alibaba Qwen (probed):{base_url}");
    if let Some(cached) = get_cached_catalog(&cache_key) {
        return Ok(cached);
    }
    let listed = fetch_openai_compatible_models(base_url, api_key, "Alibaba Qwen")?;
    let probed = probe_models_parallel(base_url, api_key, &listed);
    if probed.is_empty() {
        // Never return an empty list on probe failure — fall back to the
        // raw listing so the user still sees the models that responded 200
        // to /v1/models. Log a warning via the error string path if we
        // suspect a systemic auth issue.
        return Err(format!(
            "Token Plan probe rejected every listed model at {base_url} — check API key and plan activation"
        ));
    }
    set_cached_catalog(&cache_key, probed.clone());
    Ok(probed)
}

/// Classification of a single-model probe against an OpenAI-compatible
/// chat-completions endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// Endpoint returned HTTP 2xx — the model is callable via chat.
    Confirmed,
    /// Endpoint returned an explicit "model not found / unsupported" style
    /// error. The model should be pruned from this plan's catalog.
    Unsupported,
    /// Auth, rate-limit, network or ambiguous error — do NOT prune. A
    /// transient failure during probing must not be mistaken for
    /// "model unavailable".
    Inconclusive,
}

/// Decide how to classify a probe response given its HTTP status and a
/// short snippet of the error body. Extracted as a pure function so it
/// can be unit-tested without an HTTP server.
pub fn classify_probe(status: u16, body_snippet: &str) -> ProbeOutcome {
    match status {
        200..=299 => ProbeOutcome::Confirmed,
        400 | 404 | 422 => {
            let lower = body_snippet.to_lowercase();
            // DashScope / OpenAI-compatible markers that mean "this model
            // is not available on this endpoint / plan".
            const UNSUPPORTED_MARKERS: &[&str] = &[
                "model_not_found",
                "model not found",
                "model not exist",
                "model does not exist",
                "no such model",
                "not supported",
                "unsupported model",
                "not available",
                "invalid model",
                "unknown model",
            ];
            if UNSUPPORTED_MARKERS.iter().any(|m| lower.contains(m)) {
                ProbeOutcome::Unsupported
            } else {
                // 4xx on chat without a clear "model" reason: could be a
                // bad request shape (e.g. audio model needs different
                // params) — treat as Unsupported because the standard chat
                // request we send is documented and any deviation means
                // the endpoint does not accept this model via chat.
                ProbeOutcome::Unsupported
            }
        }
        401 | 403 => {
            // Auth / entitlement failure at the endpoint level. If the
            // message specifically says the model is disallowed for the
            // plan we can prune; otherwise leave Inconclusive so a
            // transient key/permission hiccup does not hide a valid model.
            let lower = body_snippet.to_lowercase();
            if lower.contains("model")
                && (lower.contains("not allowed") || lower.contains("not authorized"))
            {
                ProbeOutcome::Unsupported
            } else {
                ProbeOutcome::Inconclusive
            }
        }
        408 | 429 | 500..=599 => ProbeOutcome::Inconclusive,
        _ => ProbeOutcome::Inconclusive,
    }
}

/// Issue a single minimal chat-completion probe against `base_url` for
/// `model_id`. Never reaches a fallback endpoint; failures are classified
/// by [classify_probe].
pub fn probe_chat_model(base_url: &str, api_key: &str, model_id: &str) -> ProbeOutcome {
    let trimmed = base_url.trim_end_matches('/');
    let url = format!("{trimmed}/v1/chat/completions");
    let body = serde_json::json!({
        "model": model_id,
        "messages": [{"role": "user", "content": "ping"}],
        "max_tokens": 1,
        "temperature": 0.0,
    });
    match ureq::post(&url)
        .timeout(Duration::from_secs(10))
        .set("Authorization", &format!("Bearer {api_key}"))
        .set("Content-Type", "application/json")
        .send_json(&body)
    {
        Ok(_) => ProbeOutcome::Confirmed,
        Err(ureq::Error::Status(code, resp)) => {
            let mut snippet = String::new();
            // Error bodies from OpenAI-compatible endpoints are tiny
            // (well under a KB), so reading the whole stream is safe.
            let _ = resp.into_reader().read_to_string(&mut snippet);
            if snippet.len() > 4096 {
                snippet.truncate(4096);
            }
            classify_probe(code, &snippet)
        }
        Err(_) => ProbeOutcome::Inconclusive,
    }
}

/// Run chat-model probes against `base_url` for every candidate in parallel
/// (bounded worker pool) and return only those NOT classified as
/// [ProbeOutcome::Unsupported]. Uses the SAME `base_url` for every request
/// — no cross-endpoint fallback path exists in this function.
fn probe_models_parallel(
    base_url: &str,
    api_key: &str,
    candidates: &[ModelInfo],
) -> Vec<ModelInfo> {
    if candidates.is_empty() {
        return Vec::new();
    }
    use std::sync::atomic::{AtomicUsize, Ordering};
    let next_idx = AtomicUsize::new(0);
    let outcomes: Mutex<Vec<Option<ProbeOutcome>>> = Mutex::new(vec![None; candidates.len()]);
    let worker_count = candidates.len().min(6);
    let base_owned = base_url.to_string();
    let key_owned = api_key.to_string();
    let cands_owned: Vec<ModelInfo> = candidates.to_vec();
    std::thread::scope(|s| {
        for _ in 0..worker_count {
            let base_ref = &base_owned;
            let key_ref = &key_owned;
            let cands_ref = &cands_owned;
            let next_ref = &next_idx;
            let out_ref = &outcomes;
            s.spawn(move || loop {
                let i = next_ref.fetch_add(1, Ordering::SeqCst);
                if i >= cands_ref.len() {
                    break;
                }
                let outcome = probe_chat_model(base_ref, key_ref, &cands_ref[i].id);
                if let Ok(mut guard) = out_ref.lock() {
                    guard[i] = Some(outcome);
                }
            });
        }
    });
    let guard = outcomes.lock().unwrap_or_else(|p| p.into_inner());
    cands_owned
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !matches!(guard[*i], Some(ProbeOutcome::Unsupported)))
        .map(|(_, m)| m)
        .collect()
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

    // ── classify_probe ────────────────────────────────────

    #[test]
    fn classify_probe_200_is_confirmed() {
        assert_eq!(classify_probe(200, ""), ProbeOutcome::Confirmed);
        assert_eq!(classify_probe(201, "{}"), ProbeOutcome::Confirmed);
    }

    #[test]
    fn classify_probe_model_not_found_is_unsupported() {
        let body = r#"{"error":{"code":"model_not_found","message":"Model not exist."}}"#;
        assert_eq!(classify_probe(404, body), ProbeOutcome::Unsupported);
    }

    #[test]
    fn classify_probe_400_with_message_is_unsupported() {
        let body =
            r#"{"error":{"message":"The model `qwen-audio-3.0-realtime-plus` is not supported"}}"#;
        assert_eq!(classify_probe(400, body), ProbeOutcome::Unsupported);
    }

    #[test]
    fn classify_probe_400_without_markers_still_unsupported_for_chat() {
        // A 400 on our canonical chat body means the endpoint does not accept
        // this model through /v1/chat/completions.
        let body = r#"{"error":{"message":"Invalid request"}}"#;
        assert_eq!(classify_probe(400, body), ProbeOutcome::Unsupported);
    }

    #[test]
    fn classify_probe_auth_failure_is_inconclusive() {
        // Endpoint-wide auth problem — must not hide a valid model from the
        // catalog just because the API key happened to be transiently denied.
        let body = r#"{"error":{"code":"invalid_api_key","message":"token expired"}}"#;
        assert_eq!(classify_probe(401, body), ProbeOutcome::Inconclusive);
        assert_eq!(classify_probe(403, body), ProbeOutcome::Inconclusive);
    }

    #[test]
    fn classify_probe_403_model_not_allowed_is_unsupported() {
        let body = r#"{"error":{"message":"Model qwen-max not allowed for this plan"}}"#;
        assert_eq!(classify_probe(403, body), ProbeOutcome::Unsupported);
    }

    #[test]
    fn classify_probe_transient_errors_are_inconclusive() {
        assert_eq!(
            classify_probe(429, "rate limited"),
            ProbeOutcome::Inconclusive
        );
        assert_eq!(
            classify_probe(500, "server error"),
            ProbeOutcome::Inconclusive
        );
        assert_eq!(
            classify_probe(503, "upstream unavailable"),
            ProbeOutcome::Inconclusive
        );
        assert_eq!(classify_probe(408, "timeout"), ProbeOutcome::Inconclusive);
    }

    #[test]
    fn probe_models_parallel_empty_returns_empty() {
        let out = probe_models_parallel("http://example.invalid", "sk-test", &[]);
        assert!(out.is_empty());
    }

    #[test]
    fn probe_models_parallel_prunes_unsupported_keeps_inconclusive() {
        // Exercise the pure filtering path by driving it against an
        // unroutable host: every probe must be Inconclusive (network err),
        // so nothing is pruned. Proves the function does not silently
        // discard transient failures.
        let cands = vec![
            ModelInfo {
                id: "a".into(),
                label: "a".into(),
                api_style: ApiStyle::OpenAiChat,
                supports_tools: false,
                supports_thinking: false,
            },
            ModelInfo {
                id: "b".into(),
                label: "b".into(),
                api_style: ApiStyle::OpenAiChat,
                supports_tools: false,
                supports_thinking: false,
            },
        ];
        let out = probe_models_parallel("http://127.0.0.1:1", "sk-test", &cands);
        assert_eq!(
            out.len(),
            2,
            "unreachable host must not prune any candidate"
        );
    }

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
    fn infer_model_info_marks_glm_family_tool_capable() {
        // Live defect: the token-plan glm-5.2 profile fell to OpenAiChat, so
        // the agent was prompted to hand-write tool markup instead of
        // receiving native tool schemas. The endpoint does support them.
        for id in ["glm-5.2", "zhipu/glm-4-plus", "chatglm3-6b"] {
            let info = default_model_info(id);
            assert!(info.supports_tools, "{id} must be tool-capable");
            assert_eq!(info.api_style, ApiStyle::OpenAiTools, "{id}");
        }
    }

    #[test]
    fn openai_compat_table_agrees_with_the_headless_profile() {
        // The two capability tables drifting apart was the real bug: the
        // catalog listed glm as tool-incapable while the endpoint streams
        // native delta.tool_calls. Any family the worker path marks
        // tool-capable must also be marked here, or a model is presented to
        // the user as chat-only and then hand-writes markup under the hood.
        for id in [
            "glm-5.2",
            "qwen3-max",
            "deepseek-v3",
            "claude-3-5-sonnet",
            "llama-3.3-70b",
        ] {
            assert!(
                openai_compat_tool_family(id),
                "catalog table rejects tool-capable {id}"
            );
            assert!(
                default_model_info(id).supports_tools,
                "worker profile rejects tool-capable {id}"
            );
        }
        // Non-chat leftovers must not be swept in by the widening.
        assert!(!openai_compat_tool_family("my-plain-chat-model"));
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
            assert!(
                !model.is_empty(),
                "default model for {:?} should not be empty",
                p
            );
        }
    }

    #[test]
    fn default_openrouter_model_is_hy3_free() {
        assert_eq!(
            default_provider_model(AiProvider::OpenRouter),
            "tencent/hy3:free"
        );
    }

    #[test]
    fn default_openai_model_is_gpt4o() {
        assert_eq!(default_provider_model(AiProvider::OpenAI), "gpt-4o");
    }

    // ── ollama catalog ─────────────────────────────────────────────────
    // The catalog is discovered live from each host's /api/tags, so the
    // content-bearing logic (parse) is tested as a pure function; only the
    // graceful-failure path touches the network, against a guaranteed-closed
    // port so it is deterministic and fast.

    fn tags_body(json: &str) -> serde_json::Value {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn parse_ollama_tags_reads_names_and_per_model_tool_capability() {
        let body = tags_body(
            r#"{"models":[
                {"name":"qwen2.5-coder:14b","capabilities":["completion","tools"]},
                {"name":"some-legacy-gguf","capabilities":["completion"]}
            ]}"#,
        );
        let parsed = parse_ollama_tags(&body);
        let ids: Vec<&str> = parsed.iter().map(|(n, _, _)| n.as_str()).collect();
        assert_eq!(ids, vec!["qwen2.5-coder:14b", "some-legacy-gguf"]);
        // Tool support is read per model, never assumed for the whole batch.
        assert!(parsed[0].1);
        assert!(!parsed[1].1);
    }

    #[test]
    fn parse_ollama_tags_flags_thinking_models() {
        let body =
            tags_body(r#"{"models":[{"name":"deepseek-r1:8b","capabilities":["completion"]}]}"#);
        let parsed = parse_ollama_tags(&body);
        assert!(parsed[0].2, "an r1 model must be recognized as thinking");
    }

    #[test]
    fn parse_ollama_tags_missing_capabilities_is_not_tools() {
        let body = tags_body(r#"{"models":[{"name":"llama3.2:3b"}]}"#);
        let parsed = parse_ollama_tags(&body);
        assert_eq!(parsed.len(), 1);
        assert!(
            !parsed[0].1,
            "no advertised capability means no tool support"
        );
    }

    #[test]
    fn parse_ollama_tags_empty_or_malformed_is_empty() {
        assert!(parse_ollama_tags(&tags_body(r#"{"models":[]}"#)).is_empty());
        assert!(parse_ollama_tags(&tags_body(r#"{}"#)).is_empty());
    }

    #[test]
    fn ollama_tags_url_tolerates_trailing_slash() {
        assert_eq!(ollama_tags_url("http://h:11434"), "http://h:11434/api/tags");
        assert_eq!(
            ollama_tags_url("http://h:11434/"),
            "http://h:11434/api/tags"
        );
    }

    #[test]
    fn unreachable_hosts_surface_an_error_not_an_empty_success() {
        // Port 1 is closed, so every probe fails fast. A catalog that silently
        // returns "no models" would masquerade as success and strand a dispatch
        // on a model nothing can serve; the error keeps the fallback chain honest.
        let accounts = vec![crate::usage::LocalOllamaAccount {
            host: "http://127.0.0.1:1".to_string(),
            default_model: String::new(),
            label: "dead".to_string(),
        }];
        assert!(fetch_local_ollama_models(&accounts).is_err());
    }

    /// Live, opt-in proof of cross-node sharing: reads the configured host
    /// list (set `OLLAMA_HOSTS` to two or more reachable Ollama servers) and
    /// asserts the merged catalog is a superset of every host's solo catalog,
    /// so no host's models are dropped by the merge. Ignored by default
    /// because it needs running servers.
    #[test]
    #[ignore = "requires live Ollama hosts configured in OLLAMA_HOSTS"]
    fn live_multi_host_ollama_discovery_merges_all_hosts() {
        let accounts = crate::usage::load_local_ollama_accounts(std::path::Path::new("."));
        assert!(
            accounts.len() >= 2,
            "expected OLLAMA_HOSTS to configure at least two hosts, got {}",
            accounts.len()
        );
        let merged = fetch_local_ollama_models(&accounts).expect("at least one host reachable");
        for acct in &accounts {
            let solo = fetch_local_ollama_models(std::slice::from_ref(acct)).unwrap_or_default();
            for m in solo {
                assert!(
                    merged.iter().any(|x| x.id == m.id && x.label == m.label),
                    "model {:?} from host {:?} was dropped by the multi-host merge",
                    m.id,
                    acct.host
                );
            }
        }
        assert!(
            !merged.is_empty(),
            "merged catalog must list discovered models"
        );
        let listing: Vec<String> = merged.iter().map(|m| format!("  {}", m.label)).collect();
        println!(
            "merged ollama catalog across {} hosts:\n{}",
            accounts.len(),
            listing.join("\n")
        );
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
