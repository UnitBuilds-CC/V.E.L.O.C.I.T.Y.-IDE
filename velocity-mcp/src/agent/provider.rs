use super::models::*;

pub fn openrouter_api_key() -> String {
    const OPENROUTER_API_KEY: &str =
        "[REDACTED_OPENROUTER_API_KEY]";
    std::env::var("OPENROUTER_API_KEY").unwrap_or_else(|_| OPENROUTER_API_KEY.to_string())
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

    let supports_tools = false;

    Some(ModelInfo {
        label,
        id,
        api_style: ApiStyle::OpenAiChat,
        supports_tools,
        supports_thinking,
    })
}

pub fn fetch_model_catalog(accounts: &[crate::usage::CloudflareAccount]) -> Result<Vec<ModelInfo>, String> {
    for account in accounts {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/ai/models/search",
            account.id
        );
        let response = ureq::get(&url)
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

        match ureq::get("https://openrouter.ai/api/v1/models")
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

pub fn enrich_model_profile(accounts: &[crate::usage::CloudflareAccount], profile: &ModelInfo) -> ModelInfo {
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
