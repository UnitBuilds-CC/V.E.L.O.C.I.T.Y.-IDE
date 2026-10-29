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
/// long reasoning streams can run as long as bytes keep arriving. The read
/// bound is 90 s, not 300 s: a healthy stream emits bytes every few seconds,
/// so silence that long means the connection is dead — and a 300 s bound let
/// one frozen read stall an entire turn for five minutes before the retry
/// path ever engaged.
pub(crate) fn stream_agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(15))
            .timeout_read(Duration::from_secs(90))
            .build()
    })
}

/// Same bounds, but the read window is 600 s. Local llama.cpp servers on
/// CPU-offloaded hardware can sit silent for minutes *before the first token*
/// (model load + prefill of a multi-KB prompt emit nothing on the socket), and
/// the 90 s window aborted healthy write-forced turns at the status line.
/// A stream that starts emitting is still expected to keep emitting; the wide
/// window only forgives the pre-token silence, not mid-stream freezes beyond
/// ten minutes. Used exclusively by `execute_ollama_request`.
pub(crate) fn slow_stream_agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(15))
            .timeout_read(Duration::from_secs(600))
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

/// Why a provider call produced no response.
///
/// Every `execute_*_request` returns `Option<ureq::Response>`, so on its own a
/// `None` carries no information: the reasoning loop could only fall back to a
/// per-provider guess string ("Alibaba Qwen request failed or DASHSCOPE_API_KEY
/// missing.") which is *wrong* whenever the key is present and the real cause is
/// a 404 model name, a 429, or a dead socket. That guess sent a live debugging
/// session hunting for a missing key that was configured.
///
/// The concrete cause is recorded here as it happens, and the loop reads it back
/// with [`take_last_request_error`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderRequestError {
    /// No API key or account is configured for this provider at all.
    NoCredentials { detail: String },
    /// The endpoint rejected the credentials (HTTP 401).
    AuthRejected { detail: String },
    /// A deterministic 4xx for *this request* — in practice a model name the
    /// endpoint does not serve (404) or a body it refuses (400/403). Another
    /// model on the same provider is worth trying.
    Rejected { status: u16, detail: String },
    /// Rate limit or upstream 5xx that outlived its retries.
    Exhausted { detail: String },
    /// Transport-level failure: DNS, TLS, connect/read timeout, reset.
    Transport { detail: String },
    /// The server generated a tool call and could not parse its arguments.
    ///
    /// This is the shape a local server takes when our own output ceiling stops
    /// generation inside the arguments string: llama-server builds the call as it
    /// decodes and parses it only when decoding ends, so an unclosed string is an
    /// HTTP error quoting the half-written file back at us. Recorded as its own
    /// cause because it is none of the things the other variants mean: the model
    /// answered, the socket is fine, and the credentials are not the problem. The
    /// ceiling is ours, so the fix is ours — which is exactly why resending the
    /// byte-identical request is listed as futile and switching provider is not
    /// what this is asking for.
    ToolCallArgsUnparsed { detail: String },
}

impl ProviderRequestError {
    /// The operator-facing sentence. This is what both the status line and the
    /// loop's blocked reason are built from, so the two cannot drift apart.
    pub fn describe(&self) -> String {
        match self {
            ProviderRequestError::NoCredentials { detail } => detail.clone(),
            ProviderRequestError::AuthRejected { detail } => detail.clone(),
            ProviderRequestError::Rejected { detail, .. } => detail.clone(),
            ProviderRequestError::Exhausted { detail } => detail.clone(),
            ProviderRequestError::Transport { detail } => detail.clone(),
            ProviderRequestError::ToolCallArgsUnparsed { detail } => detail.clone(),
        }
    }

    /// Whether the failure covers the whole provider rather than one model.
    /// A caller that is choosing between "try the next model here" and "move to
    /// the next provider" branches on this, so it must be a property of the
    /// variant and never of the message text.
    ///
    /// A tool call the server could not parse is not provider-wide: a model that
    /// emits a shorter call answers the same server fine, and another provider
    /// with the same ceiling cuts the call at the same token.
    pub fn is_provider_wide(&self) -> bool {
        !matches!(
            self,
            ProviderRequestError::Rejected { .. }
                | ProviderRequestError::ToolCallArgsUnparsed { .. }
        )
    }

    /// Whether sending this exact request again could produce a different answer.
    ///
    /// The loop sleeps 5s and resends a failed request up to twice when no other
    /// provider is configured, which is right for a dead socket and wrong for
    /// everything the endpoint decided from the request alone. gateprobe4 showed
    /// the cost: it printed "client errors are deterministic" for a 403 and then
    /// resent that identical request twice anyway, ten wasted seconds per model
    /// candidate on the way to BLOCKED. So the decision needs the cause, and the
    /// cause must be a property of the variant, not a string match.
    pub fn is_resend_futile(&self) -> bool {
        matches!(
            self,
            ProviderRequestError::Rejected { .. }
                | ProviderRequestError::AuthRejected { .. }
                | ProviderRequestError::NoCredentials { .. }
                // Futile *as sent*: the same ceiling cuts the same call at the
                // same token. The one move that is not a resend — lifting the
                // ceiling before reissuing — lives in the loop, and it is what
                // this variant exists to make reachable.
                | ProviderRequestError::ToolCallArgsUnparsed { .. }
        )
    }
}

thread_local! {
    /// Cause of the most recent failed provider request on this thread.
    ///
    /// Thread-local because the request is issued synchronously by the same
    /// thread that runs the reasoning loop, and because a shared global would
    /// let two concurrent sub-agents overwrite each other's diagnosis.
    static LAST_REQUEST_ERROR: std::cell::RefCell<Option<ProviderRequestError>> =
        const { std::cell::RefCell::new(None) };
}

/// Report a failed request: announce it on the status channel *and* keep the
/// structured cause for the loop to read. Using this at every failure site is
/// what keeps the human-visible sentence and the machine-visible reason in step.
fn fail_request(
    ui_tx: &Sender<AgentToUiMessage>,
    err: ProviderRequestError,
) -> Option<ureq::Response> {
    ui_tx
        .send(AgentToUiMessage::StatusUpdate(err.describe()))
        .ok();
    LAST_REQUEST_ERROR.with(|slot| *slot.borrow_mut() = Some(err));
    None
}

/// Take the cause of the most recent failed provider request, clearing it.
///
/// Taking (rather than peeking) matters: a stale error from an earlier turn
/// must never be reported as the reason a later, unrelated `None` happened.
pub fn take_last_request_error() -> Option<ProviderRequestError> {
    LAST_REQUEST_ERROR.with(|slot| slot.borrow_mut().take())
}

/// Drop any recorded cause. Called before a request is issued so the value read
/// afterwards can only describe that request.
pub fn clear_last_request_error() {
    LAST_REQUEST_ERROR.with(|slot| *slot.borrow_mut() = None);
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
    if final_res.is_none() {
        // Every account was quota-exhausted, rejected the request, or could not
        // be reached. Recorded here rather than at each break so the reason
        // survives whichever path gave up first.
        let _ = fail_request(
            ui_tx,
            ProviderRequestError::Exhausted {
                detail: format!(
                    "OpenRouter request failed across all {} account(s).",
                    or_accounts.len()
                ),
            },
        );
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
        return (
            fail_request(
                ui_tx,
                ProviderRequestError::NoCredentials {
                    detail: "No Cloudflare accounts configured.".to_string(),
                },
            ),
            None,
        );
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
    if cf_response.is_none() {
        let _ = fail_request(
            ui_tx,
            ProviderRequestError::Exhausted {
                detail: format!(
                    "All {} Cloudflare Workers AI account(s) exhausted or failed.",
                    accounts.len()
                ),
            },
        );
    }
    (cf_response, used_acct)
}

pub fn execute_azure_request(
    azure_accounts: &[AzureOpenAiAccount],
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
) -> Option<ureq::Response> {
    if azure_accounts.is_empty() {
        return fail_request(
            ui_tx,
            ProviderRequestError::NoCredentials {
                detail: "No Azure OpenAI accounts configured.".to_string(),
            },
        );
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
                // 401 is the key; any other 4xx names the deployment, so it is
                // model-specific and another deployment is worth a try.
                let err = if code == 401 {
                    ProviderRequestError::AuthRejected {
                        detail: format!("Azure OpenAI authentication failed (401): {body}"),
                    }
                } else if (400..=499).contains(&code) {
                    ProviderRequestError::Rejected {
                        status: code,
                        detail: format!("Azure OpenAI HTTP {code} error: {body}"),
                    }
                } else {
                    ProviderRequestError::Exhausted {
                        detail: format!("Azure OpenAI HTTP {code} error: {body}"),
                    }
                };
                let _ = fail_request(ui_tx, err);
                break;
            }
            Err(e) => {
                if attempt < max_attempts {
                    std::thread::sleep(Duration::from_secs(1));
                } else {
                    let _ = fail_request(
                        ui_tx,
                        ProviderRequestError::Transport {
                            detail: format!("Azure OpenAI connection error: {e:?}"),
                        },
                    );
                }
            }
        }
    }
    azure_response
}

/// Pick which Ollama host should serve a request. A single account is trivial.
/// With cross-node accounts the requested model may live on only one host, so
/// probe each host's `/api/tags` and return the first that actually advertises
/// the model; fall back to the first account when none claim it (or none
/// answer), which keeps the historic single-host behaviour intact.
fn choose_ollama_account<'a>(
    accounts: &'a [crate::usage::LocalOllamaAccount],
    model: &str,
) -> Option<&'a crate::usage::LocalOllamaAccount> {
    let first = accounts.first()?;
    if accounts.len() == 1 || model.is_empty() {
        return Some(first);
    }
    for account in accounts {
        let url = crate::agent::provider::ollama_tags_url(&account.host);
        let body: Option<serde_json::Value> = match ureq::get(&url)
            .timeout(std::time::Duration::from_secs(5))
            .call()
        {
            Ok(r) => r.into_json().ok(),
            Err(_) => None,
        };
        if let Some(body) = body {
            let serves = crate::agent::provider::parse_ollama_tags(&body)
                .iter()
                .any(|(name, _, _)| name == model);
            if serves {
                return Some(account);
            }
        }
    }
    Some(first)
}

/// Classify an HTTP status answered by the local Ollama server.
///
/// A 4xx from it is the same class of answer as a 4xx from a cloud endpoint:
/// the body the message describes is the body this process will send again, so
/// resending cannot change it. Recording these as `Transport` made the loop's
/// own retry gate (`is_resend_futile`) call them transient, and the stubbed
/// `reject4xx` probe measured what that costs — three identical turns with 5s
/// of sleep between them, for a server that had already said no. Only 408 and
/// 429 are left retryable, matching the cloud path: a slow model load or a
/// rate limit can both clear on their own.
fn ollama_status_error(
    code: u16,
    label: &str,
    model: &str,
    api_url: &str,
    snippet: &str,
) -> ProviderRequestError {
    if (400..=499).contains(&code) && code != 408 && code != 429 {
        ProviderRequestError::Rejected {
            status: code,
            detail: format!(
                "Ollama '{label}' rejected the request ({code}) for model '{model}' \
                 at {api_url}: {snippet}"
            ),
        }
    } else {
        ProviderRequestError::Transport {
            detail: format!("Ollama '{label}' returned HTTP {code} at {api_url}: {snippet}"),
        }
    }
}

pub fn execute_ollama_request(
    ollama_accounts: &[LocalOllamaAccount],
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
    workspace_root: &PathBuf,
) -> Option<ureq::Response> {
    let model = request_body["model"].as_str().unwrap_or("");
    let account = choose_ollama_account(ollama_accounts, model);
    let host = account
        .map(|a| a.host.as_str())
        .unwrap_or("http://localhost:11434");
    let label = account.map(|a| a.label.as_str()).unwrap_or("Local-Ollama");
    let api_url = ollama_chat_url(host);

    // The internal request is OpenAI-shaped; convert it to the body the native
    // /api/chat endpoint accepts (assistant tool_calls.arguments as an object,
    // tool-result messages re-emitted as user turns) before posting.
    let request_body = &to_native_ollama_body(request_body);

    // Durable retry logic for transient connection errors (timeouts, connection refused, etc.)
    // Exponential backoff: 2s, 4s, 8s (3 attempts total)
    const MAX_RETRIES: u32 = 3;
    const BASE_DELAY_MS: u64 = 2000;

    for attempt in 0..MAX_RETRIES {
        match slow_stream_agent()
            .post(&api_url)
            .set("Content-Type", "application/json")
            .send_json(request_body)
        {
            Ok(res) => return Some(res),
            // A Status response carries Ollama's actual rejection reason in the
            // body (e.g. the 400 "Value looks like object..." history-shape
            // error). Without it, a turn-2 rejection is undiagnosable from the
            // agent log alone — the status code says only "Bad Request".
            Err(ureq::Error::Status(code, resp)) => {
                let why = resp.into_string().unwrap_or_default();
                let snippet: String = why.chars().take(400).collect();
                // `last_request.json` is re-written by every later fallback
                // turn, so by the time a run ends it no longer shows what the
                // Ollama endpoint actually received. Persist the rejected
                // native body alongside the server's reason for offline replay.
                dump_rejected_request(workspace_root, &api_url, code, &snippet, request_body);
                let err = ollama_status_error(code, label, model, &api_url, &snippet);
                return fail_request(ui_tx, err);
            }
            Err(e) => {
                let is_last_attempt = attempt == MAX_RETRIES - 1;
                let error_str = format!("{:?}", e);

                // Check if this is a transient error worth retrying
                let is_transient = error_str.contains("TimedOut")
                    || error_str.contains("ConnectionRefused")
                    || error_str.contains("ConnectionReset")
                    || error_str.contains("BrokenPipe")
                    || error_str.contains("NetworkUnreachable")
                    || error_str.contains("HostUnreachable");

                if is_transient && !is_last_attempt {
                    let delay_ms = BASE_DELAY_MS * (1 << attempt); // Exponential: 2s, 4s, 8s
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "Ollama connection error (attempt {}/{}), retrying in {}ms...",
                            attempt + 1,
                            MAX_RETRIES,
                            delay_ms
                        )))
                        .ok();
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                    continue;
                }

                // Non-transient error or last attempt: fail
                return fail_request(
                    ui_tx,
                    ProviderRequestError::Transport {
                        detail: format!("Ollama '{label}' connection error at {host}: {e:?}"),
                    },
                );
            }
        }
    }

    // Should never reach here, but satisfy the compiler
    None
}

/// Persist a rejected Ollama request (exact body + server reason) for offline
/// replay. Written best-effort; a failure to dump must never mask the error
/// the caller is about to return.
fn dump_rejected_request(
    workspace_root: &PathBuf,
    api_url: &str,
    code: u16,
    reason: &str,
    request_body: &Value,
) {
    let dir = workspace_root.join(".velocity");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let dump = json!({
        "url": api_url,
        "status": code,
        "server_reason": reason,
        "rejected_body": request_body,
    });
    let _ = std::fs::write(
        dir.join("ollama_rejected.json"),
        serde_json::to_string_pretty(&dump).unwrap_or_default(),
    );
}

/// Build the Ollama NATIVE chat endpoint URL from a host, tolerating a
/// trailing slash. Kept as a pure helper so the request shape is testable
/// without a running server.
///
/// We use `/api/chat` (native) rather than `/v1/chat/completions` (OpenAI
/// compatibility shim) because the shim silently ignores the `think:false`
/// flag for Qwen3-family models: reasoning always runs and, on complex
/// convert-phase tasks, exhausts the entire output budget before any tool
/// call is emitted (finish_reason=length with empty content). The native
/// endpoint honours `think:false`, suppressing reasoning so the full budget
/// is available for large write_file tool calls. The native streaming reply
/// is JSONL (one JSON object per line, `message.content` deltas, complete
/// `message.tool_calls`, terminal `done:true`) rather than SSE — see the
/// native-format branch in `run_agent_reasoning_loop`.
pub fn ollama_chat_url(host: &str) -> String {
    format!("{}/api/chat", host.trim_end_matches('/'))
}

/// Transform an OpenAI-style request body into the shape Ollama's NATIVE
/// `/api/chat` endpoint accepts. The internal history is built OpenAI-style
/// (shared with the cloud providers), but native rejects it with a 400
/// "Value looks like object, but can't find closing '}'" because it tries to
/// parse the assistant `tool_calls[].function.arguments` string as an object
/// and chokes on the JSON escapes. Native wants `arguments` as a parsed object
/// and drops the `id`/`type` wrapper. A `role:"tool"` result message is also
/// not a native role — native feeds the observation back as a `user` turn.
pub fn to_native_ollama_body(body: &Value) -> Value {
    let mut out = body.clone();
    let Some(msgs) = out.get_mut("messages").and_then(|m| m.as_array_mut()) else {
        return out;
    };
    for msg in msgs.iter_mut() {
        let role = msg
            .get("role")
            .and_then(|r| r.as_str())
            .unwrap_or("")
            .to_string();
        if role == "assistant" {
            if let Some(tcs) = msg.get_mut("tool_calls").and_then(|t| t.as_array_mut()) {
                for tc in tcs.iter_mut() {
                    if let Some(func) = tc.get("function").cloned() {
                        let name = func.get("name").cloned().unwrap_or(Value::Null);
                        let args = match func.get("arguments") {
                            Some(Value::String(s)) => serde_json::from_str::<Value>(s)
                                .unwrap_or(Value::Object(serde_json::Map::new())),
                            Some(v) => v.clone(),
                            None => Value::Object(serde_json::Map::new()),
                        };
                        *tc = json!({ "function": { "name": name, "arguments": args } });
                    }
                }
            }
        } else if role == "tool" {
            let content = msg
                .get("content")
                .cloned()
                .unwrap_or(Value::String(String::new()));
            *msg = json!({ "role": "user", "content": content });
        }
    }
    out
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
        return fail_request(
            ui_tx,
            ProviderRequestError::NoCredentials {
                detail: format!(
                    "{provider_name} API key not set. Configure it in Settings or export {api_key_env_var}."
                ),
            },
        );
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
                return fail_request(
                    ui_tx,
                    ProviderRequestError::AuthRejected {
                        detail: format!(
                            "{provider_name} authentication failed. Check your API key in Settings."
                        ),
                    },
                );
            }
            Err(ureq::Error::Status(code, _)) if (400..=499).contains(&code) && code != 408 => {
                // Client errors other than rate-limit are deterministic: a bad
                // model name (404), a malformed body (400), a forbidden model
                // (403). Repeating the identical request cannot change the
                // answer — the LiteLLM crucible burned nine attempts and ~45s
                // of sleeps on a token-plan 404 for a legacy model name before
                // giving up. Fail fast and name the model so the operator can
                // see *which* candidate the chain exhausted on.
                let model = request_body
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or("?");
                return fail_request(
                    ui_tx,
                    ProviderRequestError::Rejected {
                        status: code,
                        detail: format!(
                            "{provider_name} rejected the request ({code}) for model '{model}'. Not retrying: client errors are deterministic."
                        ),
                    },
                );
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
                    return fail_request(
                        ui_tx,
                        ProviderRequestError::Exhausted {
                            detail: format!(
                                "{provider_name} rate limit exceeded after {max_attempts} retries."
                            ),
                        },
                    );
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
                    return fail_request(
                        ui_tx,
                        ProviderRequestError::Exhausted {
                            detail: format!(
                                "{provider_name} server error ({code}) after {max_attempts} retries."
                            ),
                        },
                    );
                }
            }
            Err(e) => {
                if attempt < max_attempts {
                    std::thread::sleep(Duration::from_secs(1));
                } else {
                    return fail_request(
                        ui_tx,
                        ProviderRequestError::Transport {
                            detail: format!("{provider_name} request error: {e:?}"),
                        },
                    );
                }
            }
        }
    }
    // Unreachable in practice: every arm above either returns or retries, and
    // the last retry returns. Recorded rather than silently `None` so the loop
    // never has to invent a reason.
    fail_request(
        ui_tx,
        ProviderRequestError::Exhausted {
            detail: format!("{provider_name} gave up after {max_attempts} attempts."),
        },
    )
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
        return fail_request(
            ui_tx,
            ProviderRequestError::NoCredentials {
                detail:
                    "Google API key not set. Configure it in Settings or export GOOGLE_API_KEY."
                        .to_string(),
            },
        );
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
        Err(ureq::Error::Status(401, _)) => fail_request(
            ui_tx,
            ProviderRequestError::AuthRejected {
                detail: "Google authentication failed. Check your API key in Settings.".to_string(),
            },
        ),
        Err(ureq::Error::Status(code, resp)) if (400..=499).contains(&code) => {
            let body = resp.into_string().unwrap_or_default();
            fail_request(
                ui_tx,
                ProviderRequestError::Rejected {
                    status: code,
                    detail: format!("Google rejected the request ({code}): {body}"),
                },
            )
        }
        Err(e) => fail_request(
            ui_tx,
            ProviderRequestError::Transport {
                detail: format!("Google request error: {e:?}"),
            },
        ),
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
    fail_request(
        ui_tx,
        ProviderRequestError::NoCredentials {
            detail: "AWS Bedrock requires BEDROCK_PROXY_URL env var pointing to an OpenAI-compatible proxy. \
                     Alternatively, use OpenRouter which routes to Bedrock models."
                .to_string(),
        },
    )
}

/// Whether a server's error body complains about the tool call it generated
/// instead of about anything the caller could have got wrong.
///
/// The live convert of `litellm/_service_logger.py` is the case this reads: the
/// turn was capped at 18000 output tokens, the model spent most of them
/// reasoning, and the `write_file` call ran out of ceiling inside its arguments
/// string. llama-server then answered
/// `Failed to parse tool call arguments as JSON: … invalid string: missing
/// closing quote` at column 18575 — quoting the arguments it had half-written,
/// which the loop threw away as a transport error and resent twice.
///
/// Matching here, at the failure site, is what keeps the loop's decision a
/// property of the variant rather than a string match on a message some other
/// layer formatted.
fn is_tool_call_parse_rejection(body: &str) -> bool {
    let lowered = body.to_lowercase();
    lowered.contains("tool call arguments")
        && (lowered.contains("parse") || lowered.contains("invalid json"))
}

/// Classify an HTTP status answered by the local llama-server, from its body.
///
/// Three stories, not the two the status code alone tells. A tool call the
/// server could not parse is neither a dead socket (it answered) nor a refusal
/// of the request (it ran it and lost the answer at the end), and each of the
/// three gets a different move from the loop: resend, refuse, or lift the
/// ceiling. Split out from [`execute_llama_request`] so the live convert's own
/// error body — the one the loop resent twice — can be fed to it in a test.
fn llama_status_error(code: u16, model: &str, url: &str, why: &str) -> ProviderRequestError {
    const LABEL: &str = "Local llama.cpp";
    let snippet: String = why.chars().take(400).collect();
    if is_tool_call_parse_rejection(why) {
        return ProviderRequestError::ToolCallArgsUnparsed {
            detail: format!(
                "{LABEL} generated a tool call it could not parse ({code}) for model \
                 '{model}' at {url}: {snippet}"
            ),
        };
    }
    // 4xx (bar rate-limit/timeout) are deterministic for a local server too — an
    // unknown model or a body it rejects on its own content will not change on
    // retry, so name the cause and fail fast.
    if (400..=499).contains(&code) && code != 408 && code != 429 {
        ProviderRequestError::Rejected {
            status: code,
            detail: format!(
                "{LABEL} rejected the request ({code}) for model '{model}' at {url}: {snippet}"
            ),
        }
    } else {
        ProviderRequestError::Transport {
            detail: format!("{LABEL} returned HTTP {code} at {url}: {snippet}"),
        }
    }
}

/// Local llama.cpp server (llama-server) — OpenAI-compatible
/// `/v1/chat/completions` endpoint with no API key. Base URL comes from
/// `LLAMA_CPP_HOST` (default `http://localhost:8080`).
///
/// The request body is already the streaming OpenAI shape the loop builds for
/// every compatible provider (`stream: true` + `messages`/`tools`), so nothing
/// is transformed here — llama-server emits SSE that the loop's parser and the
/// heartbeat stall window already understand. It reuses `slow_stream_agent()`
/// because 3-tier MoE offload keeps experts on CPU/RAM: prefill of a multi-KB
/// prompt can sit silent for minutes before the first token, well past the
/// 90 s window that would otherwise abort a healthy turn.
pub fn execute_llama_request(
    request_body: &Value,
    ui_tx: &Sender<AgentToUiMessage>,
) -> Option<ureq::Response> {
    let host =
        std::env::var("LLAMA_CPP_HOST").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let url = format!("{}/v1/chat/completions", host.trim_end_matches('/'));
    let label = "Local llama.cpp";

    // Connection errors are retried like Ollama's: a server still warming up
    // (loading a 21 GB GGUF into the tiered memory) refuses the socket for a
    // while before it is ready, and that is transient, not a dead provider.
    const MAX_RETRIES: u32 = 3;
    const BASE_DELAY_MS: u64 = 2000;
    for attempt in 0..MAX_RETRIES {
        match slow_stream_agent()
            .post(&url)
            // llama-server ignores auth; a fixed dummy bearer keeps the
            // OpenAI client shape uniform and satisfies a server run behind
            // `--api-key`. It is never a secret.
            .set("Authorization", "Bearer llama-cpp-local")
            .set("Content-Type", "application/json")
            .send_json(request_body)
        {
            Ok(res) => return Some(res),
            Err(ureq::Error::Status(code, resp)) => {
                let why = resp.into_string().unwrap_or_default();
                let model = request_body
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or("?");
                return fail_request(ui_tx, llama_status_error(code, model, &url, &why));
            }
            Err(e) => {
                let is_last = attempt == MAX_RETRIES - 1;
                let error_str = format!("{:?}", e);
                let is_transient = error_str.contains("TimedOut")
                    || error_str.contains("ConnectionRefused")
                    || error_str.contains("ConnectionReset")
                    || error_str.contains("BrokenPipe")
                    || error_str.contains("NetworkUnreachable")
                    || error_str.contains("HostUnreachable");
                if is_transient && !is_last {
                    let delay_ms = BASE_DELAY_MS * (1 << attempt);
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "llama.cpp connection error (attempt {}/{}), retrying in {}ms...",
                            attempt + 1,
                            MAX_RETRIES,
                            delay_ms
                        )))
                        .ok();
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                    continue;
                }
                return fail_request(
                    ui_tx,
                    ProviderRequestError::Transport {
                        detail: format!("{label} connection error at {url}: {e:?}"),
                    },
                );
            }
        }
    }
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
        return fail_request(
            ui_tx,
            ProviderRequestError::NoCredentials {
                detail: "Anthropic API key not set. Configure it in Settings or export ANTHROPIC_API_KEY."
                    .to_string(),
            },
        );
    }
    let anthropic_body = match build_anthropic_body(request_body) {
        Ok(body) => body,
        Err(msg) => {
            return fail_request(
                ui_tx,
                ProviderRequestError::Rejected {
                    // Not an HTTP status: the conversion refused before any
                    // request went out, and no retry can fix the payload.
                    status: 400,
                    detail: format!("Anthropic request failed: {msg}"),
                },
            );
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
        Err(ureq::Error::Status(401, _)) => fail_request(
            ui_tx,
            ProviderRequestError::AuthRejected {
                detail: "Anthropic authentication failed. Check your ANTHROPIC_API_KEY."
                    .to_string(),
            },
        ),
        Err(ureq::Error::Status(429, _)) => fail_request(
            ui_tx,
            ProviderRequestError::Exhausted {
                detail: "Anthropic rate limit exceeded (429). Try again shortly.".to_string(),
            },
        ),
        Err(ureq::Error::Status(code, resp)) if (400..=499).contains(&code) => {
            let body = resp.into_string().unwrap_or_default();
            fail_request(
                ui_tx,
                ProviderRequestError::Rejected {
                    status: code,
                    detail: format!("Anthropic rejected the request ({code}): {body}"),
                },
            )
        }
        Err(e) => fail_request(
            ui_tx,
            ProviderRequestError::Transport {
                detail: format!("Anthropic request failed: {e}"),
            },
        ),
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

    // ── ProviderRequestError ─────────────────────────────────

    /// The loop reports this text to the operator, so it has to be the real
    /// cause. The bug it replaces: a token-plan 404 on a stale model name was
    /// reported as "Alibaba Qwen request failed or DASHSCOPE_API_KEY missing."
    /// while the key was configured, which sent a debugging session after the
    /// wrong thing entirely.
    #[test]
    fn a_rejected_model_keeps_its_status_and_name_in_the_reason() {
        let err = ProviderRequestError::Rejected {
            status: 404,
            detail: "Alibaba Qwen rejected the request (404) for model 'qwen-plus'.".to_string(),
        };
        assert!(err.describe().contains("404"));
        assert!(err.describe().contains("qwen-plus"));
        assert!(
            !err.describe().to_lowercase().contains("api_key missing"),
            "a present key must not be blamed: {}",
            err.describe()
        );
        // Model-specific: the next model on this provider is worth an attempt.
        assert!(!err.is_provider_wide());
    }

    /// Everything except a per-model rejection covers the whole provider, so
    /// `team_dispatch` moves to the next provider instead of burning attempts
    /// on model names that cannot help. A tool call the server could not parse
    /// is in the same exemption: a model that emits a shorter call answers the
    /// same server fine, so the provider is not what failed.
    #[test]
    fn only_a_model_rejection_leaves_the_provider_worth_retrying() {
        let provider_wide = [
            ProviderRequestError::NoCredentials {
                detail: "no key".to_string(),
            },
            ProviderRequestError::AuthRejected {
                detail: "401".to_string(),
            },
            ProviderRequestError::Exhausted {
                detail: "429 after 3".to_string(),
            },
            ProviderRequestError::Transport {
                detail: "dns".to_string(),
            },
        ];
        for err in provider_wide {
            assert!(err.is_provider_wide(), "{err:?} covers the whole provider");
        }
    }

    /// gateprobe4's blocked run printed "Not retrying: client errors are
    /// deterministic" and then, one line later, "Retrying same request in 5s
    /// (attempt 1/2)" — the loop had the structured cause but only consulted it
    /// after the sleep. A request the endpoint refused on its own content
    /// answers identically forever; a socket or a rate limit may recover.
    #[test]
    fn a_refused_request_is_never_resent_and_a_blip_still_is() {
        let futile = [
            ProviderRequestError::Rejected {
                status: 403,
                detail: "glm-5.2 forbidden".to_string(),
            },
            ProviderRequestError::Rejected {
                status: 404,
                detail: "model not found".to_string(),
            },
            ProviderRequestError::AuthRejected {
                detail: "401".to_string(),
            },
            ProviderRequestError::NoCredentials {
                detail: "no key".to_string(),
            },
        ];
        for err in futile {
            assert!(err.is_resend_futile(), "{err:?} cannot change by resending");
        }

        let transient = [
            ProviderRequestError::Transport {
                detail: "connection reset".to_string(),
            },
            ProviderRequestError::Exhausted {
                detail: "429 after 3 retries".to_string(),
            },
        ];
        for err in transient {
            assert!(!err.is_resend_futile(), "{err:?} may recover on its own");
        }
    }

    /// The exact body that ended the live convert of `litellm/_service_logger.py`
    /// on the llama.cpp route. The server had decoded the model's `write_file`
    /// call, run out of the 18000-token output ceiling inside its arguments
    /// string, and answered by quoting the half-written file back. Nothing about
    /// that is a dead socket, but it was recorded as `Transport` because nothing
    /// else fitted — and `Transport` is the one class `is_resend_futile` calls
    /// retryable. So the identical request went out twice more, was cut at the
    /// identical token both times, and the run blocked on a 0-byte artifact with
    /// the port already generated.
    #[test]
    fn a_tool_call_the_server_could_not_parse_is_not_a_transport_blip() {
        const LIVE_BODY: &str = r##"{"error":{"code":500,"message":"Failed to parse tool call arguments as JSON: [json.exception.parse_error.101] parse error at line 1, column 18575: syntax error while parsing value - invalid string: missing closing quote; last read: \"//! Port of `litellm/_service_logger.py`"##;
        let err = llama_status_error(
            500,
            "qwen3.6-35b-a3b",
            "http://192.168.0.4:8080/v1/chat/completions",
            LIVE_BODY,
        );
        assert!(
            matches!(err, ProviderRequestError::ToolCallArgsUnparsed { .. }),
            "{err:?} is the server losing an answer it generated, not a socket"
        );
        assert!(
            err.is_resend_futile(),
            "the same ceiling cuts the call at the same token"
        );
        assert!(
            !err.is_provider_wide(),
            "a model that emits a shorter call answers this same server"
        );
        // The operator has to see the column the server stopped at: it is the
        // difference between "the model failed" and "our number failed".
        assert!(err.describe().contains("18575"), "{}", err.describe());
    }

    /// The classifier must not be a catch-all, or every server hiccup reads as a
    /// truncated call and skips the resends that a real blip recovers from. A 500
    /// that says nothing about tool call arguments is still a blip, and a 4xx
    /// still a refusal.
    #[test]
    fn an_ordinary_llama_error_is_still_a_blip_and_a_refusal_still_a_refusal() {
        let blip = llama_status_error(500, "m", "http://localhost:8080", "server is overloaded");
        assert!(
            matches!(blip, ProviderRequestError::Transport { .. }),
            "{blip:?}"
        );
        assert!(!blip.is_resend_futile(), "a busy server may free up");

        let refusal = llama_status_error(
            400,
            "qwen3.6-35b-a3b",
            "http://localhost:8080",
            "unknown model: qwen3.6-35b-a3b",
        );
        assert!(
            matches!(refusal, ProviderRequestError::Rejected { status: 400, .. }),
            "{refusal:?}"
        );
        assert!(refusal.is_resend_futile());
    }

    /// The native `/api/chat` path classified *every* HTTP status as
    /// `Transport`, so a deterministic 400 from Ollama — the history-shape
    /// rejection, an unknown model, a bad `think` value — was resent twice
    /// with 5s sleeps before the run gave up. `reject4xx` measured it against
    /// a stub that refuses on one line: 3 chat calls, 2 "Retrying same request
    /// in 5s" status lines, ~10s spent proving a refusal is still a refusal.
    #[test]
    fn a_local_ollama_4xx_is_a_refusal_and_a_blip_is_not() {
        for code in [400u16, 401, 403, 404, 415, 422] {
            let err = ollama_status_error(code, "Local-Ollama", "qwen3.6:35b-a3b", "u", "nope");
            assert!(
                err.is_resend_futile(),
                "HTTP {code} answers from the request body alone; resending it cannot help"
            );
            // The operator has to see which model the local server refused,
            // and Ollama's own reason — a bare "Bad Request" sent a previous
            // debugging session after the wrong thing.
            assert!(err.describe().contains("nope"), "{}", err.describe());
            assert!(err.describe().contains("qwen3.6:35b-a3b"));
        }

        for code in [408u16, 429, 500, 502, 503] {
            let err = ollama_status_error(code, "Local-Ollama", "qwen3.6:35b-a3b", "u", "later");
            assert!(
                !err.is_resend_futile(),
                "HTTP {code} may clear on its own; the loop's resend is correct there"
            );
        }
    }

    /// The sink is take-once: a stale cause from an earlier turn must never be
    /// reported as the reason a later, unrelated failure happened.
    #[test]
    fn the_error_sink_is_cleared_by_reading_it() {
        clear_last_request_error();
        assert_eq!(take_last_request_error(), None);

        let (ui_tx, ui_rx) = crossbeam_channel::unbounded();
        let returned = fail_request(
            &ui_tx,
            ProviderRequestError::Transport {
                detail: "connection reset".to_string(),
            },
        );
        assert!(returned.is_none(), "fail_request never yields a response");
        // The operator sees the same sentence the loop will report.
        match ui_rx.try_recv() {
            Ok(AgentToUiMessage::StatusUpdate(text)) => {
                assert_eq!(text, "connection reset");
            }
            other => panic!("expected a status update, got {other:?}"),
        }

        assert_eq!(
            take_last_request_error(),
            Some(ProviderRequestError::Transport {
                detail: "connection reset".to_string(),
            })
        );
        assert_eq!(take_last_request_error(), None, "reading must clear it");
    }

    // ── to_native_ollama_body ────────────────────────────────────────

    #[test]
    fn native_body_rewrites_assistant_tool_call_arguments_to_object() {
        let body = json!({
            "messages": [
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": "", "tool_calls": [
                    {"id": "c1", "type": "function", "function": {
                        "name": "write_file",
                        "arguments": "{\"path\":\"a.txt\"}"}}]},
                {"role": "tool", "tool_call_id": "c1", "content": "ok"}
            ]
        });
        let out = to_native_ollama_body(&body);
        let msgs = out["messages"].as_array().unwrap();
        let tc = &msgs[1]["tool_calls"][0];
        assert!(tc.get("id").is_none(), "native drops the id wrapper");
        assert!(tc.get("type").is_none(), "native drops the type wrapper");
        assert_eq!(tc["function"]["name"], json!("write_file"));
        assert!(
            tc["function"]["arguments"].is_object(),
            "arguments must be an object, not a JSON string"
        );
        assert_eq!(tc["function"]["arguments"]["path"], json!("a.txt"));
        assert_eq!(
            msgs[2]["role"],
            json!("user"),
            "tool result becomes a user turn"
        );
        assert_eq!(msgs[2]["content"], json!("ok"));
    }

    #[test]
    fn native_body_leaves_plain_messages_untouched() {
        let body = json!({"messages": [
            {"role": "system", "content": "s"},
            {"role": "user", "content": "u"}
        ]});
        let out = to_native_ollama_body(&body);
        assert_eq!(out["messages"][0]["role"], json!("system"));
        assert_eq!(out["messages"][1]["content"], json!("u"));
    }

    // ── ollama_chat_url ──────────────────────────────────────────────

    #[test]
    fn ollama_url_appends_native_chat_path() {
        assert_eq!(
            ollama_chat_url("http://localhost:11434"),
            "http://localhost:11434/api/chat"
        );
    }

    #[test]
    fn ollama_url_trims_trailing_slash() {
        assert_eq!(
            ollama_chat_url("http://localhost:11434/"),
            "http://localhost:11434/api/chat"
        );
        assert_eq!(
            ollama_chat_url("http://remote:9999///"),
            "http://remote:9999/api/chat"
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
