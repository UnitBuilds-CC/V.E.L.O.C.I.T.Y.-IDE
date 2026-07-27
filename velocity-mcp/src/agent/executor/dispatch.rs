use super::super::models::*;
use super::utils::send_usage_update;
use crate::usage::{AzureOpenAiAccount, CloudflareAccount, LocalOllamaAccount, OpenRouterAccount, UsageTracker};
use crossbeam_channel::Sender;
use serde_json::Value;
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
                            ui_tx.send(AgentToUiMessage::StatusUpdate(format!(
                                "OpenRouter account '{}' quota exhausted — trying next…",
                                acct.label
                            ))).ok();
                        }
                        account_exhausted = true;
                        break;
                    } else if attempt < max_attempts {
                        let wait_secs = attempt * 2;
                        ui_tx.send(AgentToUiMessage::StatusUpdate(format!(
                            "OpenRouter rate limit (429) on '{}'. Retrying in {}s (Attempt {}/{})…",
                            active_acct.map(|a| a.label.as_str()).unwrap_or("default"),
                            wait_secs, attempt, max_attempts
                        ))).ok();
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
                        ui_tx.send(AgentToUiMessage::StatusUpdate(format!(
                            "OpenRouter connection error: {:?}", e
                        ))).ok();
                    }
                }
            }
        }
        if (final_res.is_some() || account_exhausted)
            && final_res.is_some() {
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
        ui_tx.send(AgentToUiMessage::StatusUpdate("No Cloudflare accounts configured.".to_string())).ok();
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
        ui_tx.send(AgentToUiMessage::StatusUpdate("No Azure OpenAI accounts configured.".to_string())).ok();
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
                ui_tx.send(AgentToUiMessage::StatusUpdate(format!("Azure OpenAI HTTP {code} error: {body}"))).ok();
                break;
            }
            Err(e) => {
                if attempt < max_attempts {
                    std::thread::sleep(Duration::from_secs(1));
                } else {
                    ui_tx.send(AgentToUiMessage::StatusUpdate(format!("Azure OpenAI connection error: {:?}", e))).ok();
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
            ui_tx.send(AgentToUiMessage::StatusUpdate(format!("Local Ollama connection error at {host}: {:?}", e))).ok();
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
}
