use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use crossbeam_channel::{Sender, Receiver};
use serde::{Serialize, Deserialize};
use serde_json::{json, Value};
use crate::registry;

#[derive(Debug, Clone)]
pub enum UiToAgentMessage {
    SetWorkspace(PathBuf),
    RefreshModels,
    RefreshUsage,
    SetModel(String),
    SetThinking(bool),
    SetProvider(AiProvider),
    UserPrompt(String),
    ApproveTool { id: String, tool_name: String, arguments: Value },
    #[allow(dead_code)]
    RejectTool { id: String, tool_name: String },
    RunLocalBuild,
    RunLocalRun,
}

#[derive(Debug, Clone)]
pub enum AgentToUiMessage {
    #[allow(dead_code)]
    ThoughtToken(String),
    OutputToken(String),
    RequestToolApproval { id: String, tool_name: String, arguments: Value },
    ToolExecutionStarted { tool_name: String },
    ToolExecutionFinished { tool_name: String, result: String },
    StatusUpdate(String),
    AgentFinished,
    UpdateFileBuffer { path: PathBuf, content: String },
    ModelCatalog { models: Vec<ModelInfo>, selected: String, thinking: bool },
    AccountUsage { accounts: Vec<crate::usage::AccountUsageView>, date: String },
    ChatHistoryRestored(Vec<(String, String)>),
    ProviderChanged(AiProvider),
}

#[derive(Debug, Clone)]
pub struct HeadlessSubAgentRequest {
    pub workspace_root: PathBuf,
    pub provider: AiProvider,
    pub model: String,
    pub thinking: bool,
    pub prompt: String,
}

#[derive(Debug, Clone, Default)]
pub struct HeadlessSubAgentResult {
    pub status_updates: Vec<String>,
    pub transcript: String,
    pub changed_files: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    pub api_style: ApiStyle,
    pub supports_tools: bool,
    pub supports_thinking: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiStyle {
    OpenAiTools,
    OpenAiChat,
    PromptCompletion,
}

/// Which AI backend to use for inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiProvider {
    CloudflareWorkersAi,
    OpenRouter,
}

impl AiProvider {
    pub fn label(self) -> &'static str {
        match self {
            AiProvider::CloudflareWorkersAi => "Cloudflare Workers AI",
            AiProvider::OpenRouter => "OpenRouter",
        }
    }
}

fn infer_model_info(id: String, item: &Value) -> Option<ModelInfo> {
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

    let metadata = serde_json::to_string(item).unwrap_or_default().to_lowercase();
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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Value>,
}

struct ToolCallAccumulator {
    id: String,
    name: String,
    arguments: String,
}

use crate::usage::{load_accounts_from_env, load_openrouter_accounts_from_env, CloudflareAccount, OpenRouterAccount, UsageTracker};

fn send_usage_update(
    tracker: &mut UsageTracker,
    accounts: &[CloudflareAccount],
    or_accounts: &[OpenRouterAccount],
    ui_tx: &Sender<AgentToUiMessage>,
) {
    let views = tracker.build_views(accounts, or_accounts);
    let date = tracker.current_date();
    ui_tx.send(AgentToUiMessage::AccountUsage { accounts: views, date }).ok();
}

fn is_quota_exhausted_error(body: &str) -> bool {
    body.contains("4006") || body.to_lowercase().contains("quota")
}

fn estimate_tokens(text: &str) -> u64 {
    (text.len() as u64).max(1) / 4
}

fn fetch_model_catalog(accounts: &[CloudflareAccount]) -> Result<Vec<ModelInfo>, String> {
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
        let body: Value = response
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

const OPENROUTER_API_KEY: &str = "[REDACTED_OPENROUTER_API_KEY]";

fn openrouter_api_key() -> String {
    std::env::var("OPENROUTER_API_KEY").unwrap_or_else(|_| OPENROUTER_API_KEY.to_string())
}

fn infer_openrouter_model_info(item: &Value) -> Option<ModelInfo> {
    let id = item["id"].as_str()?.to_string();
    let lower = id.to_lowercase();
    // Skip embedding / image / audio models
    let arch = item["architecture"]["tokenizer"].as_str().unwrap_or("").to_lowercase();
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
    
    // Disable native tools for all OpenRouter models. This forces them to use our
    // highly reliable, pattern-matched inline tool calling system. This bypasses
    // all provider-specific unmarshalling, schema-mismatch, and empty-role errors
    // on OpenRouter.
    let supports_tools = false;

    Some(ModelInfo {
        label,
        id,
        api_style: ApiStyle::OpenAiChat,
        supports_tools,
        supports_thinking,
    })
}

fn fetch_openrouter_models(or_accounts: &[OpenRouterAccount], usage_tracker: &UsageTracker) -> Result<Vec<ModelInfo>, String> {
    let mut response = None;
    let mut last_err = String::new();
    
    let start_idx = usage_tracker.pick_or_account(or_accounts)
        .and_then(|picked| or_accounts.iter().position(|a| a.n == picked.n))
        .unwrap_or(0);
        
    let loop_limit = or_accounts.len().max(1);
    for idx in 0..loop_limit {
        let current_key = if or_accounts.is_empty() {
            openrouter_api_key()
        } else {
            let acct = &or_accounts[(start_idx + idx) % or_accounts.len()];
            if usage_tracker.is_or_exhausted(acct.n) { continue; }
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
        None => return Err(if last_err.is_empty() { "No available OpenRouter accounts to fetch models.".to_string() } else { last_err }),
    };

    let body: Value = res_unwrapped
        .into_json()
        .map_err(|e| format!("OpenRouter model catalog response invalid: {e}"))?;

    // Pre-seed the goal model so it always appears even if filtered
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

    // Always guarantee goal model is present and at the top
    models.retain(|m| m.id != goal.id);
    models.insert(0, goal);

    // Separate free models to top (after goal), then paid
    models.sort_by(|a, b| {
        let a_free = a.id.ends_with(":free");
        let b_free = b.id.ends_with(":free");
        match (a_free, b_free) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.id.cmp(&b.id),
        }
    });
    // Keep goal at absolute front
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
    infer_model_info(id.to_string(), &Value::Null).unwrap_or(ModelInfo {
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

pub fn enrich_model_profile(accounts: &[CloudflareAccount], profile: &ModelInfo) -> ModelInfo {
    let Some(account) = accounts.first() else { return profile.clone() };
    let encoded_model = profile.id.replace('%', "%25").replace('/', "%2F").replace('@', "%40");
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/ai/models/schema?model={}",
        account.id, encoded_model
    );
    let Ok(response) = ureq::get(&url)
        .set("Authorization", &format!("Bearer {}", account.token))
        .set("Accept", "application/json")
        .call() else { return profile.clone() };
    let Ok(body) = response.into_json::<Value>() else { return profile.clone() };
    let input_description = body["result"]["input"]["description"]
        .as_str()
        .unwrap_or("")
        .to_lowercase();
    if input_description.is_empty() {
        return profile.clone();
    }
    let mut enriched = profile.clone();
    enriched.supports_tools = input_description.contains("tool") || input_description.contains("function");
    enriched.supports_thinking = input_description.contains("thinking") || input_description.contains("reasoning");
    if input_description.contains("prompt") && !input_description.contains("message") {
        enriched.api_style = ApiStyle::PromptCompletion;
    } else if enriched.supports_tools {
        enriched.api_style = ApiStyle::OpenAiTools;
    } else {
        enriched.api_style = ApiStyle::OpenAiChat;
    }
    enriched
}

fn render_prompt(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .map(|message| format!("{}: {}", message.role, message.content))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Render all registered tools as a compact inline reference block.
/// Injected into the system prompt for models that use text-based tool calling
/// (i.e. `supports_tools = false`) so the model knows what functions exist and
/// how to invoke them using the `<tool_call>` syntax.
fn build_inline_tool_docs() -> String {
    use crate::registry::get_tools;
    let tools = get_tools();
    let mut doc = String::from(
        "\n\n## Available Tools\n\
        Call tools using this exact syntax (one block per call):\n\
        <tool_call>\n\
        <function=TOOL_NAME>\n\
        <parameter=PARAM_NAME>VALUE</parameter>\n\
        </function>\n\n"
    );
    for t in &tools {
        doc.push_str(&format!("### {}\n{}\n", t.name, t.description));
        if let Some(props) = t.input_schema["properties"].as_object() {
            let required: Vec<&str> = t.input_schema["required"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            for (param, schema) in props {
                let desc = schema["description"].as_str().unwrap_or("");
                let req = if required.contains(&param.as_str()) { " (required)" } else { " (optional)" };
                doc.push_str(&format!("  - `{}`{}: {}\n", param, req, desc));
            }
        }
        doc.push('\n');
    }
    doc.push_str(
        "Always call exactly one tool per <tool_call> block. \
        Wait for the tool result before continuing.\n"
    );
    doc
}

fn build_request(
    profile: &ModelInfo,
    model: &str,
    messages: &[ChatMessage],
    tools: &[Value],
    thinking: bool,
    provider: AiProvider,
) -> Value {
    let mut request = json!({"model": model, "stream": true});
    match profile.api_style {
        ApiStyle::PromptCompletion => {
            request["prompt"] = json!(render_prompt(messages));
        }
        ApiStyle::OpenAiTools | ApiStyle::OpenAiChat => {
            request["messages"] = json!(messages);
            if profile.supports_tools && !tools.is_empty() {
                request["tools"] = json!(tools);
            }
        }
    }
    if profile.supports_thinking && thinking {
        match provider {
            AiProvider::CloudflareWorkersAi => {
                request["thinking"] = json!(true);
            }
            AiProvider::OpenRouter => {
                request["reasoning"] = json!({
                    "effort": "high",
                    "exclude": false
                });
            }
        }
    }
    request
}


fn strip_think_tags(s: &str) -> String {
    // Remove <think>...</think> blocks left by chain-of-thought models (e.g. deepseek-r1)
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        if let Some(end) = rest[start..].find("</think>") {
            rest = &rest[start + end + "</think>".len()..];
        } else {
            // unclosed tag — drop everything from here
            break;
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

fn compress_history(messages: &[ChatMessage], supports_tools: bool) -> Vec<ChatMessage> {
    // ── Step 0: sanitize roles — drop/repair messages with invalid roles ──────
    // History loaded from chatlogs.nda may have messages written by buggy prior
    // sessions with empty or missing roles.  Every provider rejects these.
    const VALID_ROLES: &[&str] = &["system", "user", "assistant", "tool", "function", "developer"];
    let mut messages: Vec<ChatMessage> = messages.iter().filter_map(|m| {
        let trimmed_content = m.content.trim();
        // Drop buggy empty/corrupted tool call residues or dummy loop errors
        if trimmed_content.contains("Tool '' is not registered")
            || trimmed_content == "</tool_call>"
            || trimmed_content == "<tool_call>\n</tool_call>"
            || trimmed_content == "Tool name came through empty. Let me retry with the correct tool:"
            || trimmed_content == "The tool invocation isn't registering the function name. Let me use the proper `read_file` tool:"
            || trimmed_content == "The tool name is still being stripped from my calls, so I can't write the file through the tool right now. But I can give you the exact file to create manually, which will permanently fix the validation error."
            || trimmed_content == "The tool-call parser is consistently dropping the `<function>` tag on my side, so I can't fetch the file right now. However, I already have enough information from the earlier `list_dir` to resolve your build error confidently."
            || trimmed_content == "Apologies — the tool name field is being dropped in my calls. Let me retry explicitly with `read_file`:"
            || trimmed_content == "My `write_file` tool calls are being rejected because the function name isn't being transmitted correctly on my side. I cannot directly create the file through tools right now."
            || trimmed_content == "My tool calls keep getting stripped of the function name, so I can't browse the files right now. Based on the earlier `list_dir` of `velocity-mcp/src/editor/`, the UI panels live there (e.g. `chat_panel.rs`, `status_bar.rs`, `app.rs`, `top_bar.rs` or similar)."
            || trimmed_content.starts_with("[Tool result for '']: ")
            || (m.role == "assistant" && trimmed_content.is_empty() && m.tool_calls.is_none())
        {
            return None;
        }

        if m.role.trim().is_empty() {
            // Recover if content is meaningful, otherwise drop
            if !m.content.trim().is_empty() {
                let mut fixed = m.clone();
                fixed.role = "assistant".to_string();
                Some(fixed)
            } else {
                None // drop entirely
            }
        } else if !VALID_ROLES.contains(&m.role.as_str()) {
            // Unknown role — demote to user so it's preserved as context
            let mut fixed = m.clone();
            fixed.role = "user".to_string();
            Some(fixed)
        } else {
            Some(m.clone())
        }
    }).collect();

    // Dynamically update the system prompt in history to include/exclude the inline
    // tool descriptions depending on the active model's tool capability mode.
    // This handles cases where history was loaded from disk under one model but
    // is now executing under another.
    if let Some(sys_msg) = messages.iter_mut().find(|m| m.role == "system") {
        if sys_msg.content.starts_with("You are Antigravity") {
            let clean_base = "You are Antigravity, a high-performance agent running directly in V.E.L.O.C.I.T.Y.-IDE. You have access to local workspace files and execution sandboxes via tools. Help the user program the workspace. Always output concise, correct, and high-quality responses.";
            if !supports_tools {
                sys_msg.content = format!("{}{}", clean_base, build_inline_tool_docs());
            } else {
                sys_msg.content = clean_base.to_string();
            }
        }
    }

    let messages = messages.as_slice();

    // ── Step 1: per-message compression + tool flattening ────────────────────
    let max_intact_tool_calls = 4;
    let tool_indices: Vec<usize> = messages.iter().enumerate()
        .filter(|(_, m)| m.role == "tool")
        .map(|(idx, _)| idx)
        .collect();

    let mut compressed: Vec<ChatMessage> = Vec::new();
    for (idx, m) in messages.iter().enumerate() {
        let mut m_copy = m.clone();

        // Strip think-tags from assistant messages (leftover CoT pollution)
        if m_copy.role == "assistant" {
            m_copy.content = strip_think_tags(&m_copy.content);
        }

        if m.role == "tool" {
            let has_subsequent_assistant_msg = messages[idx + 1..].iter().any(|msg| msg.role == "assistant");
            if has_subsequent_assistant_msg && m_copy.content.len() > 1000 {
                let tool_name = m_copy.name.clone().unwrap_or_else(|| "unknown_tool".to_string());
                let content_len = m_copy.content.len();
                let content_hash = hash_str(&m_copy.content);

                let mut decls = Vec::new();
                if tool_name == "read_file" || m_copy.content.contains("fn ") || m_copy.content.contains("class ") {
                    for line in m_copy.content.lines() {
                        let line = line.trim();
                        if line.contains("fn ") || line.contains("void ") || line.contains("def ") || line.contains("class ") {
                            let parts: Vec<&str> = line.split_whitespace().collect();
                            for (i, &word) in parts.iter().enumerate() {
                                if word == "fn" || word == "def" || word == "class" || word == "void" {
                                    if let Some(&name) = parts.get(i + 1) {
                                        let name_cleaned = name.split('(').next().unwrap_or(name);
                                        decls.push(format!("{} {}", word, name_cleaned));
                                    }
                                }
                            }
                        }
                    }
                }

                let decl_summary = if decls.is_empty() {
                    String::new()
                } else {
                    format!("\nParsed Declarations:\n  {}", decls.join("\n  "))
                };

                m_copy.content = format!(
                    "[Tool output of '{}' compressed to optimize context budget.\n\
                     Merkle Hash: {:016x}\n\
                     Original Size: {} characters.{}\n\
                     (This data was successfully read and processed in a previous turn. Query site_map to retrieve specific details.)]",
                    tool_name, content_hash, content_len, decl_summary
                );
            }
        }

        if !supports_tools {
            if m_copy.role == "tool" {
                m_copy.role = "user".to_string();
                let tool_name = m_copy.name.clone().unwrap_or_else(|| "unknown_tool".to_string());
                m_copy.content = format!("[Tool result for '{}']: {}", tool_name, m_copy.content);
                m_copy.name = None;
                m_copy.tool_call_id = None;
            } else if m_copy.role == "assistant" && m_copy.tool_calls.is_some() {
                if let Some(tool_calls) = m_copy.tool_calls.take() {
                    let mut desc = String::new();
                    if let Some(arr) = tool_calls.as_array() {
                        for tc in arr {
                            let name = tc["function"]["name"].as_str().unwrap_or("unknown");
                            let args = tc["function"]["arguments"].as_str().unwrap_or("{}");
                            if !desc.is_empty() {
                                desc.push_str("\n");
                            }
                            desc.push_str(&format!("[Calling tool '{}' with arguments '{}']", name, args));
                        }
                    }
                    if m_copy.content.trim().is_empty() {
                        m_copy.content = desc;
                    } else {
                        m_copy.content = format!("{}\n\n{}", m_copy.content, desc);
                    }
                }
            }
        }

        // Drop empty assistant messages (they confuse some providers)
        if m_copy.role == "assistant" && m_copy.content.trim().is_empty() && m_copy.tool_calls.is_none() {
            continue;
        }

        compressed.push(m_copy);
    }

    // ── Step 2: hard context budget cap (~60 K chars ≈ 15 K tokens) ──────────
    // Always keep the system prompt (first message). Then fill from the tail.
    const BUDGET: usize = 60_000;
    let system: Vec<ChatMessage> = compressed.iter().filter(|m| m.role == "system").cloned().collect();
    let non_system: Vec<ChatMessage> = compressed.into_iter().filter(|m| m.role != "system").collect();

    let system_chars: usize = system.iter().map(|m| m.content.len()).sum();
    let remaining_budget = BUDGET.saturating_sub(system_chars);

    // Walk from the tail, accumulate until budget exhausted
    let mut tail: Vec<ChatMessage> = Vec::new();
    let mut used = 0usize;
    for m in non_system.iter().rev() {
        let len = m.content.len();
        if used + len > remaining_budget && !tail.is_empty() {
            break;
        }
        tail.push(m.clone());
        used += len;
    }
    tail.reverse();

    let mut result = system;
    result.extend(tail);
    result
}


fn pack_ndav(filename: &str, payload: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"NDAV");
    let size = payload.len() as u32;
    buf.extend_from_slice(&size.to_le_bytes());
    buf.extend_from_slice(filename.as_bytes());
    buf.push(0);
    buf.extend_from_slice(payload);
    buf
}

fn unpack_ndav(data: &[u8]) -> Option<(String, Vec<u8>)> {
    if data.len() < 9 || &data[0..4] != b"NDAV" {
        return None;
    }
    let size = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let mut name_end = 8;
    while name_end < data.len() && data[name_end] != 0 {
        name_end += 1;
    }
    if name_end >= data.len() {
        return None;
    }
    let filename = String::from_utf8_lossy(&data[8..name_end]).to_string();
    let payload_start = name_end + 1;
    if payload_start + size > data.len() {
        return None;
    }
    let payload = data[payload_start..payload_start + size].to_vec();
    Some((filename, payload))
}

fn generate_sitemap_text(workspace_root: &std::path::Path) -> String {
    let mut entries: Vec<(String, String, Option<u64>)> = Vec::new();

    fn scan_sitemap(dir: &std::path::Path, base: &std::path::Path, entries: &mut Vec<(String, String, Option<u64>)>) {
        if let Ok(read_dir) = std::fs::read_dir(dir) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if file_name == ".git" || file_name == "target" || file_name == "node_modules" || file_name == ".velocity" {
                    continue;
                }

                if let Ok(meta) = entry.metadata() {
                    let rel_path = path.strip_prefix(base).unwrap_or(&path).to_string_lossy().to_string();
                    if meta.is_dir() {
                        entries.push(("dir".to_string(), rel_path.clone(), None));
                        scan_sitemap(&path, base, entries);
                    } else {
                        entries.push(("file".to_string(), rel_path, Some(meta.len())));
                    }
                }
            }
        }
    }

    scan_sitemap(workspace_root, workspace_root, &mut entries);
    entries.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

    let mut lines = vec!["sitemap version 1".to_string()];
    for (kind, rel_path, size) in entries {
        match size {
            Some(size) => lines.push(format!("entry\t{}\t{}\t{}", kind, encode_nda_text(&rel_path), size)),
            None => lines.push(format!("entry\t{}\t{}", kind, encode_nda_text(&rel_path))),
        }
    }
    lines.join("\n") + "\n"
}

fn write_sitemap_nda(workspace_root: &std::path::Path) {
    let sitemap_dir = workspace_root.join(".velocity");
    let _ = std::fs::create_dir_all(&sitemap_dir);
    let sitemap_text = generate_sitemap_text(workspace_root);
    let _ = std::fs::write(sitemap_dir.join("sitemap.nda"), sitemap_text);
}

fn load_chatlogs_nda(workspace_root: &std::path::Path) -> Option<Vec<ChatMessage>> {
    let nda_path = workspace_root.join(".velocity").join("chatlogs.nda");
    if !nda_path.exists() {
        return None;
    }
    let data = std::fs::read(&nda_path).ok()?;
    let text = if let Some((_filename, payload)) = unpack_ndav(&data) {
        String::from_utf8_lossy(&payload).to_string()
    } else {
        String::from_utf8_lossy(&data).to_string()
    };
    let messages = parse_chatlogs_nda(&text);
    if messages.is_empty() { None } else { Some(messages) }
}

fn parse_chatlogs_nda(text: &str) -> Vec<ChatMessage> {
    if text.starts_with("chatlogs version 2\n") {
        let mut messages = std::collections::BTreeMap::new();
        for line in text.lines() {
            if line.trim().is_empty() || line == "chatlogs version 2" {
                continue;
            }
            if line.starts_with("message_count ") || line.starts_with("message\t") {
                continue;
            }
            let Some(rest) = line.strip_prefix("field\t") else {
                continue;
            };
            let parts: Vec<&str> = rest.split('\t').collect();
            if parts.len() != 3 {
                continue;
            }
            let Ok(index) = parts[0].parse::<usize>() else {
                continue;
            };
            let field = parts[1];
            let value = parts[2];
            let message = messages.entry(index).or_insert_with(|| ChatMessage {
                role: String::new(),
                content: String::new(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            });
            match field {
                "role" => message.role = decode_nda_text(value),
                "content" => message.content = decode_nda_text(value),
                "name" => message.name = decode_optional_nda_text(value),
                "tool_call_id" => message.tool_call_id = decode_optional_nda_text(value),
                "tool_calls" => {
                    message.tool_calls = if value == "-" {
                        None
                    } else {
                        serde_json::from_str(&decode_nda_text(value)).ok()
                    };
                }
                _ => {}
            }
        }
        return messages.into_values().collect();
    }
    if text.starts_with("chatlogs version 1\n") {
        let mut messages = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() || line == "chatlogs version 1" {
                continue;
            }
            let Some(rest) = line.strip_prefix("message\t") else {
                continue;
            };
            let parts: Vec<&str> = rest.split('\t').collect();
            if parts.len() != 6 {
                continue;
            }
            let tool_calls = if parts[5] == "-" {
                None
            } else {
                serde_json::from_str(&decode_nda_text(parts[5])).ok()
            };
            messages.push(ChatMessage {
                role: decode_nda_text(parts[1]),
                content: decode_nda_text(parts[4]),
                name: decode_optional_nda_text(parts[2]),
                tool_call_id: decode_optional_nda_text(parts[3]),
                tool_calls,
            });
        }
        return messages;
    }

    let mut messages = Vec::new();
    for msg_block in text.split("\n---\n") {
        if msg_block.trim().is_empty() {
            continue;
        }
        let lines: Vec<&str> = msg_block.lines().collect();
        if lines.len() >= 2 {
            let role = lines[0].to_string();
            let mut content = lines[1..].join("\n");
            let mut name = None;
            let mut tool_call_id = None;
            if role == "tool" {
                if let Some(first_line) = lines.get(1) {
                    let parts: Vec<&str> = first_line.split('\t').collect();
                    if parts.len() == 2 {
                        name = Some(parts[0].to_string());
                        tool_call_id = Some(parts[1].to_string());
                        content = lines[2..].join("\n");
                    }
                }
            }
            messages.push(ChatMessage {
                role,
                content,
                name,
                tool_call_id,
                tool_calls: None,
            });
        }
    }
    messages
}

fn serialize_chatlogs_nda(messages: &[ChatMessage]) -> String {
    let mut lines = vec![
        "chatlogs version 2".to_string(),
        format!("message_count {}", messages.len()),
    ];
    for (index, msg) in messages.iter().enumerate() {
        let tool_calls = msg
            .tool_calls
            .as_ref()
            .and_then(|value| serde_json::to_string(value).ok());
        lines.push(format!("message\t{}", index));
        lines.push(format!("field\t{}\trole\t{}", index, encode_nda_text(&msg.role)));
        lines.push(format!("field\t{}\tname\t{}", index, encode_optional_nda_text(msg.name.as_deref())));
        lines.push(format!("field\t{}\ttool_call_id\t{}", index, encode_optional_nda_text(msg.tool_call_id.as_deref())));
        lines.push(format!("field\t{}\tcontent\t{}", index, encode_nda_text(&msg.content)));
        lines.push(format!(
            "field\t{}\ttool_calls\t{}",
            index,
            encode_optional_nda_text(tool_calls.as_deref()),
        ));
    }
    lines.join("\n") + "\n"
}

fn save_chatlogs_nda(workspace_root: &std::path::Path, messages: &[ChatMessage]) {
    let sitemap_dir = workspace_root.join(".velocity");
    let _ = std::fs::create_dir_all(&sitemap_dir);
    let _ = std::fs::write(sitemap_dir.join("chatlogs.nda"), serialize_chatlogs_nda(messages));
}

fn append_changelog_nda(workspace_root: &std::path::Path, file_path: &str, action: &str) {
    let sitemap_dir = workspace_root.join(".velocity");
    let _ = std::fs::create_dir_all(&sitemap_dir);
    let changelog_path = sitemap_dir.join("changelog.nda");

    let mut entries = load_changelog_entries(&changelog_path);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    entries.push((now, file_path.to_string(), action.to_string()));
    let _ = std::fs::write(changelog_path, serialize_changelog_nda(&entries));
}

fn load_changelog_entries(changelog_path: &std::path::Path) -> Vec<(u64, String, String)> {
    let raw = if let Ok(data) = std::fs::read(changelog_path) {
        if let Some((_filename, payload)) = unpack_ndav(&data) {
            String::from_utf8_lossy(&payload).to_string()
        } else {
            String::from_utf8_lossy(&data).to_string()
        }
    } else {
        String::new()
    };
    parse_changelog_entries(&raw)
}

fn parse_changelog_entries(raw: &str) -> Vec<(u64, String, String)> {
    let mut entries = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line == "changelog version 1" {
            continue;
        }
        let parts: Vec<&str> = if let Some(rest) = line.strip_prefix("entry\t") {
            rest.split('\t').collect()
        } else {
            line.split('\t').collect()
        };
        if parts.len() != 3 {
            continue;
        }
        if let Ok(timestamp) = parts[0].parse() {
            entries.push((timestamp, decode_nda_text(parts[1]), decode_nda_text(parts[2])));
        }
    }
    entries
}

fn serialize_changelog_nda(entries: &[(u64, String, String)]) -> String {
    let mut lines = vec!["changelog version 1".to_string()];
    for (timestamp, file_path, action) in entries {
        lines.push(format!(
            "entry\t{}\t{}\t{}",
            timestamp,
            encode_nda_text(file_path),
            encode_nda_text(action),
        ));
    }
    lines.join("\n") + "\n"
}

fn write_handover_nda(workspace_root: &std::path::Path, task_state: &str, last_active_turn: usize, build_status: &str, interrupted: bool) {
    let sitemap_dir = workspace_root.join(".velocity");
    let _ = std::fs::create_dir_all(&sitemap_dir);
    let handover_path = sitemap_dir.join("handover.nda");

    let payload = format!(
        "handover version 1\nstate {}\nturn {}\nbuild {}\ninterrupted {}\n",
        encode_nda_text(task_state),
        last_active_turn,
        encode_nda_text(build_status),
        interrupted,
    );
    let _ = std::fs::write(handover_path, payload);
}

fn write_last_request_artifacts(
    workspace_root: &std::path::Path,
    profile: &ModelInfo,
    model: &str,
    provider: AiProvider,
    thinking: bool,
    messages: &[ChatMessage],
    tools: &[Value],
    request_body: &Value,
) {
    let velocity_dir = workspace_root.join(".velocity");
    let _ = std::fs::create_dir_all(&velocity_dir);
    let _ = std::fs::write(
        velocity_dir.join("last_request.nda"),
        serialize_last_request_nda(profile, model, provider, thinking, messages, tools, request_body),
    );
    let _ = std::fs::write(
        velocity_dir.join("last_request.json"),
        serde_json::to_string_pretty(request_body).unwrap_or_default(),
    );
}

fn serialize_last_request_nda(
    profile: &ModelInfo,
    model: &str,
    provider: AiProvider,
    thinking: bool,
    messages: &[ChatMessage],
    tools: &[Value],
    request_body: &Value,
) -> String {
    let mut lines = vec![
        "last-request version 1".to_string(),
        format!("provider {}", nda_atom(provider.label())),
        format!("provider_label {}", encode_nda_text(provider.label())),
        format!("model {}", encode_nda_text(model)),
        format!("api_style {}", nda_atom(api_style_name(profile.api_style))),
        format!("supports_tools {}", profile.supports_tools),
        format!("supports_thinking {}", profile.supports_thinking),
        format!("thinking {}", thinking),
        format!("stream {}", request_body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false)),
        format!("message_count {}", messages.len()),
        format!("tool_count {}", tools.len()),
    ];

    if let Some(prompt) = request_body.get("prompt").and_then(|v| v.as_str()) {
        lines.push(format!("prompt {}", encode_nda_text(prompt)));
    }

    if let Some(reasoning) = request_body.get("reasoning") {
        lines.push(format!("reasoning {}", encode_nda_text(&reasoning.to_string())));
    }

    for (index, message) in messages.iter().enumerate() {
        lines.push(format!(
            "message\t{}\t{}\t{}\t{}\t{}\t{}",
            index,
            nda_atom(&message.role),
            encode_optional_nda_text(message.name.as_deref()),
            encode_optional_nda_text(message.tool_call_id.as_deref()),
            encode_nda_text(&message.content),
            encode_optional_nda_text(message.tool_calls.as_ref().map(|v| v.to_string()).as_deref()),
        ));
    }

    for (index, tool) in tools.iter().enumerate() {
        let name = tool
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let description = tool
            .get("function")
            .and_then(|f| f.get("description"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        lines.push(format!(
            "tool\t{}\t{}\t{}",
            index,
            encode_nda_text(name),
            encode_nda_text(description),
        ));
    }

    lines.join("\n") + "\n"
}

fn api_style_name(style: ApiStyle) -> &'static str {
    match style {
        ApiStyle::OpenAiTools => "openai-tools",
        ApiStyle::OpenAiChat => "openai-chat",
        ApiStyle::PromptCompletion => "prompt-completion",
    }
}

fn nda_atom(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };
        if mapped == '-' {
            if !last_dash && !out.is_empty() {
                out.push('-');
            }
            last_dash = true;
        } else {
            out.push(mapped);
            last_dash = false;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "empty".to_string()
    } else {
        trimmed.to_string()
    }
}

fn encode_nda_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn decode_nda_text(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn decode_optional_nda_text(value: &str) -> Option<String> {
    if value == "-" {
        None
    } else {
        Some(decode_nda_text(value))
    }
}

fn encode_optional_nda_text(value: Option<&str>) -> String {
    value.map(encode_nda_text).unwrap_or_else(|| "-".to_string())
}

pub fn run_agent_thread(
    mut workspace_root: PathBuf,
    ui_rx: Receiver<UiToAgentMessage>,
    ui_tx: Sender<AgentToUiMessage>,
) {
    let accounts = load_accounts_from_env();
    let or_accounts = load_openrouter_accounts_from_env();
    let mut usage_tracker = UsageTracker::new(&workspace_root);
    send_usage_update(&mut usage_tracker, &accounts, &or_accounts, &ui_tx);
    let mut provider = match std::env::var("LLM_PROVIDER")
        .or_else(|_| std::env::var("AI_PROVIDER"))
        .map(|s| s.to_lowercase())
        .as_deref()
    {
        Ok("openrouter") => AiProvider::OpenRouter,
        _ => AiProvider::CloudflareWorkersAi,
    };
    let mut model = match provider {
        AiProvider::OpenRouter => std::env::var("OPENROUTER_MODEL")
            .or_else(|_| std::env::var("CF_MODEL"))
            .unwrap_or_else(|_| "tencent/hy3:free".to_string()),
        AiProvider::CloudflareWorkersAi => std::env::var("CF_MODEL")
            .unwrap_or_else(|_| "@cf/moonshotai/kimi-k2.7-code".to_string()),
    };
    let mut thinking = std::env::var("CF_THINKING").map(|v| v != "0").unwrap_or(false);
    let mut selected_profile = match provider {
        AiProvider::OpenRouter => ModelInfo {
            id: model.clone(),
            label: model.rsplit('/').next().unwrap_or(&model).to_string(),
            api_style: ApiStyle::OpenAiChat,
            supports_tools: false,
            supports_thinking: false,
        },
        AiProvider::CloudflareWorkersAi => default_model_info(&model),
    };
    if !selected_profile.supports_thinking {
        thinking = false;
    }
    let mut model_catalog = vec![selected_profile.clone()];

    let mut message_history = match load_chatlogs_nda(&workspace_root) {
        Some(history) => {
            ui_tx.send(AgentToUiMessage::StatusUpdate("Loaded previous chat session context.".to_string())).ok();
            let restored: Vec<(String, String)> = history
                .iter()
                .filter(|m| m.role == "user" || m.role == "assistant")
                .map(|m| (m.role.clone(), m.content.clone()))
                .collect();
            if !restored.is_empty() {
                ui_tx.send(AgentToUiMessage::ChatHistoryRestored(restored)).ok();
            }
            history
        }
        None => {
            // Build system prompt: include inline tool docs only for providers
            // that cannot do native tool_calls (OpenRouter inline mode).
            let use_inline_tools = provider == AiProvider::OpenRouter
                || !selected_profile.supports_tools;
            let sys = format!(
                "You are Antigravity, a high-performance agent running directly in V.E.L.O.C.I.T.Y.-IDE. \
                You have access to local workspace files and execution sandboxes via tools. \
                Help the user program the workspace. Always output concise, correct, and high-quality responses.{}",
                if use_inline_tools { build_inline_tool_docs() } else { String::new() }
            );
            vec![ChatMessage {
                role: "system".to_string(),
                content: sys,
                name: None,
                tool_call_id: None,
                tool_calls: None,
            }]
        }
    };

    // Build the initial sitemap
    write_sitemap_nda(&workspace_root);

    ui_tx.send(AgentToUiMessage::StatusUpdate("Agent thread initialized and idling.".to_string())).ok();
    ui_tx.send(AgentToUiMessage::ProviderChanged(provider)).ok();
    ui_tx.send(AgentToUiMessage::ModelCatalog { models: model_catalog.clone(), selected: model.clone(), thinking }).ok();

    while let Ok(msg) = ui_rx.recv() {
        match msg {
            UiToAgentMessage::RefreshModels => {
                let fetch_result = match provider {
                    AiProvider::CloudflareWorkersAi => fetch_model_catalog(&accounts),
                    AiProvider::OpenRouter => fetch_openrouter_models(&or_accounts, &usage_tracker),
                };
                match fetch_result {
                    Ok(models) => {
                        model_catalog = models;
                        if !model_catalog.iter().any(|candidate| candidate.id == model) {
                            model = model_catalog.first().map(|candidate| candidate.id.clone()).unwrap_or(model);
                        }
                        selected_profile = model_catalog.iter().find(|candidate| candidate.id == model).cloned().unwrap_or_else(|| default_model_info(&model));
                        // Only enrich via Cloudflare schema API when on that provider
                        if provider == AiProvider::CloudflareWorkersAi {
                            selected_profile = enrich_model_profile(&accounts, &selected_profile);
                        }
                        if !selected_profile.supports_thinking {
                            thinking = false;
                        }
                        ui_tx.send(AgentToUiMessage::ModelCatalog { models: model_catalog.clone(), selected: model.clone(), thinking }).ok();
                    }
                    Err(error) => {
                        ui_tx.send(AgentToUiMessage::StatusUpdate(error)).ok();
                    }
                };
            }
            UiToAgentMessage::RefreshUsage => {
                send_usage_update(&mut usage_tracker, &accounts, &or_accounts, &ui_tx);
            }
            UiToAgentMessage::SetModel(selected) => {
                if !selected.trim().is_empty() {
                    model = selected;
                    selected_profile = model_catalog.iter().find(|candidate| candidate.id == model).cloned().unwrap_or_else(|| default_model_info(&model));
                    selected_profile = enrich_model_profile(&accounts, &selected_profile);
                    if let Some(entry) = model_catalog.iter_mut().find(|candidate| candidate.id == model) {
                        *entry = selected_profile.clone();
                    }
                    if !selected_profile.supports_thinking {
                        thinking = false;
                    }
                    ui_tx.send(AgentToUiMessage::ModelCatalog { models: model_catalog.clone(), selected: model.clone(), thinking }).ok();
                    ui_tx.send(AgentToUiMessage::StatusUpdate(format!("Model set to {model}"))).ok();
                }
            }
            UiToAgentMessage::SetThinking(enabled) => {
                thinking = enabled && selected_profile.supports_thinking;
                ui_tx.send(AgentToUiMessage::StatusUpdate(if thinking { "Thinking enabled" } else { "Thinking disabled" }.to_string())).ok();
            }
            UiToAgentMessage::SetProvider(new_provider) => {
                provider = new_provider;
                // Switch default model for the new provider
                match provider {
                    AiProvider::OpenRouter => {
                        model = "tencent/hy3:free".to_string();
                        model_catalog = vec![ModelInfo {
                            id: "tencent/hy3:free".to_string(),
                            label: "HunyuanLarge (hy3) Free".to_string(),
                            api_style: ApiStyle::OpenAiChat,
                            supports_tools: false,
                            supports_thinking: false,
                        }];
                    }
                    AiProvider::CloudflareWorkersAi => {
                        model = std::env::var("CF_MODEL")
                            .unwrap_or_else(|_| "@cf/moonshotai/kimi-k2.7-code".to_string());
                        model_catalog = vec![default_model_info(&model)];
                    }
                }
                selected_profile = model_catalog.first().cloned().unwrap_or_else(|| default_model_info(&model));
                thinking = thinking && selected_profile.supports_thinking;
                ui_tx.send(AgentToUiMessage::ProviderChanged(provider)).ok();
                ui_tx.send(AgentToUiMessage::ModelCatalog { models: model_catalog.clone(), selected: model.clone(), thinking }).ok();
                ui_tx.send(AgentToUiMessage::StatusUpdate(format!("Provider switched to {}", provider.label()))).ok();
                // Immediately kick off model discovery for the new provider
                let _ = ui_tx.send(AgentToUiMessage::StatusUpdate(format!("Fetching {} model catalog…", provider.label())));
            }
            UiToAgentMessage::SetWorkspace(new_root) => {
                if new_root.is_dir() {
                    write_sitemap_nda(&new_root);
                    message_history = load_chatlogs_nda(&new_root).unwrap_or_else(|| {
                        let use_inline = provider == AiProvider::OpenRouter
                            || !selected_profile.supports_tools;
                        vec![ChatMessage {
                            role: "system".to_string(),
                            content: format!(
                                "You are Antigravity, a high-performance agent running directly in V.E.L.O.C.I.T.Y.-IDE. \
                                You have access to local workspace files and execution sandboxes via tools. \
                                Help the user program the workspace. Always output concise, correct, and high-quality responses.{}",
                                if use_inline { build_inline_tool_docs() } else { String::new() }
                            ),
                            name: None,
                            tool_call_id: None,
                            tool_calls: None,
                        }]
                    });
                    workspace_root = new_root.clone();
                    usage_tracker = UsageTracker::new(&new_root);
                    send_usage_update(&mut usage_tracker, &accounts, &or_accounts, &ui_tx);
                    let restored: Vec<(String, String)> = message_history
                        .iter()
                        .filter(|m| m.role == "user" || m.role == "assistant")
                        .map(|m| (m.role.clone(), m.content.clone()))
                        .collect();
                    if !restored.is_empty() {
                        ui_tx.send(AgentToUiMessage::ChatHistoryRestored(restored)).ok();
                    }
                    ui_tx.send(AgentToUiMessage::StatusUpdate("Agent workspace switched.".to_string())).ok();
                }
            }
            UiToAgentMessage::UserPrompt(prompt) => {
                message_history.push(ChatMessage {
                    role: "user".to_string(),
                    content: prompt,
                    name: None,
                    tool_call_id: None,
                    tool_calls: None,
                });
                
                run_agent_reasoning_loop(
                    &workspace_root,
                    &accounts,
                    &or_accounts,
                    &model,
                    &selected_profile,
                    provider,
                    thinking,
                    &mut message_history,
                    &mut usage_tracker,
                    &ui_rx,
                    &ui_tx,
                );
            }
            UiToAgentMessage::RunLocalBuild => {
                ui_tx.send(AgentToUiMessage::StatusUpdate("Running local cargo check...".to_string())).ok();
                ui_tx.send(AgentToUiMessage::OutputToken("\n$ cargo check (Local)\n".to_string())).ok();
                
                let output = std::process::Command::new("cargo")
                    .arg("check")
                    .current_dir(&workspace_root)
                    .output();
                    
                match output {
                    Ok(out) => {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        ui_tx.send(AgentToUiMessage::OutputToken(stdout.into_owned())).ok();
                        ui_tx.send(AgentToUiMessage::OutputToken(stderr.into_owned())).ok();
                        
                        if out.status.success() {
                            ui_tx.send(AgentToUiMessage::StatusUpdate("Local build succeeded!".to_string())).ok();
                        } else {
                            ui_tx.send(AgentToUiMessage::StatusUpdate("Local build failed!".to_string())).ok();
                        }
                    }
                    Err(e) => {
                        ui_tx.send(AgentToUiMessage::OutputToken(format!("Failed to run build: {:?}", e))).ok();
                        ui_tx.send(AgentToUiMessage::StatusUpdate("Local build failed to launch".to_string())).ok();
                    }
                }
                
                let _ = run_compilation_check(&workspace_root);
                ui_tx.send(AgentToUiMessage::AgentFinished).ok();
            }
            UiToAgentMessage::RunLocalRun => {
                ui_tx.send(AgentToUiMessage::StatusUpdate("Running local cargo run...".to_string())).ok();
                ui_tx.send(AgentToUiMessage::OutputToken("\n$ cargo run (Local)\n".to_string())).ok();
                
                let output = std::process::Command::new("cargo")
                    .arg("run")
                    .current_dir(&workspace_root)
                    .output();
                    
                match output {
                    Ok(out) => {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        ui_tx.send(AgentToUiMessage::OutputToken(stdout.into_owned())).ok();
                        ui_tx.send(AgentToUiMessage::OutputToken(stderr.into_owned())).ok();
                        
                        if out.status.success() {
                            ui_tx.send(AgentToUiMessage::StatusUpdate("Local run finished successfully!".to_string())).ok();
                        } else {
                            ui_tx.send(AgentToUiMessage::StatusUpdate("Local run exited with error!".to_string())).ok();
                        }
                    }
                    Err(e) => {
                        ui_tx.send(AgentToUiMessage::OutputToken(format!("Failed to run executable: {:?}", e))).ok();
                        ui_tx.send(AgentToUiMessage::StatusUpdate("Local run failed to launch".to_string())).ok();
                    }
                }
                
                ui_tx.send(AgentToUiMessage::AgentFinished).ok();
            }
            _ => {}
        }
    }
}

fn serialize_transcript_nda(content: &[u8]) -> String {
    let text = String::from_utf8_lossy(content);
    let mut lines = vec!["transcript version 1".to_string(), "source jsonl".to_string()];
    for (index, line) in text.lines().enumerate() {
        lines.push(format!("line\t{}\t{}", index, encode_nda_text(line)));
    }
    if text.ends_with('\n') {
        lines.push("trailing_newline true".to_string());
    }
    lines.join("\n") + "\n"
}

fn write_workspace_transcript_nda(workspace_root: &std::path::Path, content: &[u8]) {
    let velocity_dir = workspace_root.join(".velocity");
    let _ = std::fs::create_dir_all(&velocity_dir);
    let workspace_nda = velocity_dir.join("transcript.nda");
    let _ = std::fs::write(workspace_nda, serialize_transcript_nda(content));
}

fn convert_jsonl_to_nda(workspace_root: &std::path::Path) {
    let conv_id = "17bd30f6-be7a-4829-b5b9-023fa4dd8c59";
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\Users\\visse".to_string());
    let transcript_path = std::path::Path::new(&home)
        .join(".gemini")
        .join("antigravity")
        .join("brain")
        .join(conv_id)
        .join(".system_generated")
        .join("logs")
        .join("transcript.jsonl");

    if let Ok(content) = std::fs::read(&transcript_path) {
        let nda_payload = pack_ndav("transcript.txt", &content);
        let nda_path = transcript_path.with_extension("nda");
        let _ = std::fs::write(nda_path, &nda_payload);
        write_workspace_transcript_nda(workspace_root, &content);
    }
}

fn run_compilation_check(workspace_root: &std::path::Path) -> Result<(), String> {
    let output = std::process::Command::new("cargo")
        .arg("check")
        .current_dir(workspace_root)
        .output();
        
    match output {
        Ok(out) => {
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let mut errors = Vec::new();
                for line in stderr.lines() {
                    let trimmed = line.trim();
                    if trimmed.contains("error[E") || trimmed.contains("error:") || trimmed.starts_with("--> src/") {
                        errors.push(trimmed.to_string());
                    }
                }
                if errors.is_empty() {
                    // Build failed but no parseable errors; return raw tail.
                    let lines: Vec<&str> = stderr.lines().collect();
                    let start = lines.len().saturating_sub(10);
                    return Err(lines[start..].iter().map(|s| s.to_string()).collect::<Vec<_>>().join("\n"));
                }
                return Err(errors.join("\n"));
            }
            Ok(())
        }
        Err(e) => Err(format!("Failed to execute cargo check: {:?}", e))
    }
}

pub fn run_headless_subagent(request: HeadlessSubAgentRequest) -> HeadlessSubAgentResult {
    let accounts = load_accounts_from_env();
    let or_accounts = load_openrouter_accounts_from_env();
    let mut usage_tracker = UsageTracker::new(&request.workspace_root);
    let selected_profile = match request.provider {
        AiProvider::OpenRouter => ModelInfo {
            id: request.model.clone(),
            label: request.model.rsplit('/').next().unwrap_or(&request.model).to_string(),
            api_style: ApiStyle::OpenAiChat,
            supports_tools: false,
            supports_thinking: false,
        },
        AiProvider::CloudflareWorkersAi => {
            let profile = default_model_info(&request.model);
            enrich_model_profile(&accounts, &profile)
        }
    };
    let thinking = request.thinking && selected_profile.supports_thinking;

    let use_inline_tools = request.provider == AiProvider::OpenRouter || !selected_profile.supports_tools;
    let mut message_history = vec![ChatMessage {
        role: "system".to_string(),
        content: format!(
            "You are Antigravity, a high-performance agent running directly in V.E.L.O.C.I.T.Y.-IDE. \
            You have access to local workspace files and execution sandboxes via tools. \
            Help the user program the workspace. Always output concise, correct, and high-quality responses.{}",
            if use_inline_tools { build_inline_tool_docs() } else { String::new() }
        ),
        name: None,
        tool_call_id: None,
        tool_calls: None,
    }];
    message_history.push(ChatMessage {
        role: "user".to_string(),
        content: request.prompt,
        name: None,
        tool_call_id: None,
        tool_calls: None,
    });

    let (agent_event_tx, agent_event_rx) = crossbeam_channel::unbounded();
    let (agent_ui_tx, agent_ui_rx) = crossbeam_channel::unbounded();
    let status_updates = Arc::new(Mutex::new(Vec::new()));
    let transcript = Arc::new(Mutex::new(String::new()));
    let changed_files = Arc::new(Mutex::new(Vec::new()));

    let status_updates_collector = status_updates.clone();
    let transcript_collector = transcript.clone();
    let changed_files_collector = changed_files.clone();
    let auto_approve_tx = agent_ui_tx.clone();

    let collector = std::thread::spawn(move || {
        while let Ok(msg) = agent_event_rx.recv() {
            match msg {
                AgentToUiMessage::StatusUpdate(status) => {
                    status_updates_collector.lock().unwrap().push(status);
                }
                AgentToUiMessage::OutputToken(token) | AgentToUiMessage::ThoughtToken(token) => {
                    transcript_collector.lock().unwrap().push_str(&token);
                }
                AgentToUiMessage::UpdateFileBuffer { path, .. } => {
                    let mut guard = changed_files_collector.lock().unwrap();
                    if !guard.contains(&path) {
                        guard.push(path);
                    }
                }
                AgentToUiMessage::RequestToolApproval { id, tool_name, arguments } => {
                    status_updates_collector
                        .lock()
                        .unwrap()
                        .push(format!("Auto-approving tool: {tool_name}"));
                    let _ = auto_approve_tx.send(UiToAgentMessage::ApproveTool { id, tool_name, arguments });
                }
                AgentToUiMessage::AgentFinished => break,
                _ => {}
            }
        }
    });

    run_agent_reasoning_loop(
        &request.workspace_root,
        &accounts,
        &or_accounts,
        &request.model,
        &selected_profile,
        request.provider,
        thinking,
        &mut message_history,
        &mut usage_tracker,
        &agent_ui_rx,
        &agent_event_tx,
    );

    drop(agent_event_tx);
    let _ = collector.join();

    let status_updates = status_updates.lock().unwrap().clone();
    let transcript = transcript.lock().unwrap().clone();
    let changed_files = changed_files.lock().unwrap().clone();
    HeadlessSubAgentResult {
        status_updates,
        transcript,
        changed_files,
    }
}

fn run_agent_reasoning_loop(
    workspace_root: &PathBuf,
    accounts: &[CloudflareAccount],
    or_accounts: &[OpenRouterAccount],
    model: &str,
    profile: &ModelInfo,
    provider: AiProvider,
    thinking: bool,
    message_history: &mut Vec<ChatMessage>,
    usage_tracker: &mut UsageTracker,
    ui_rx: &Receiver<UiToAgentMessage>,
    ui_tx: &Sender<AgentToUiMessage>,
) {
    let mut sitemap_needed = false;
    let mut loop_count = 0;
    let max_loops = 15;
    
    // Map registered tools to Workers AI schema
    let registered_tools = registry::get_tools();
    let cf_tools: Vec<Value> = registered_tools.iter().map(|t| {
        json!({
            "type": "function",
            "function": {
                "name": t.name,
                "description": t.description,
                "parameters": t.input_schema
            }
        })
    }).collect();

    while loop_count < max_loops {
        loop_count += 1;
        ui_tx.send(AgentToUiMessage::StatusUpdate(format!(
            "Querying {} (Turn {})…",
            provider.label(),
            loop_count
        ))).ok();

        let compressed_history = compress_history(message_history, profile.supports_tools);

        let request_body = build_request(profile, model, &compressed_history, &cf_tools, thinking, provider);
        write_last_request_artifacts(
            workspace_root,
            profile,
            model,
            provider,
            thinking,
            &compressed_history,
            &cf_tools,
            &request_body,
        );

        // --- Provider-forked API call ---
        let mut used_account: Option<&CloudflareAccount> = None;
        let mut used_or_account: Option<&OpenRouterAccount> = None;
        let ureq_response = match provider {
            AiProvider::OpenRouter => {
                let start_idx = usage_tracker.pick_or_account(or_accounts)
                    .and_then(|picked| or_accounts.iter().position(|a| a.n == picked.n))
                    .unwrap_or(0);
                
                let mut final_res = None;
                let loop_limit = or_accounts.len().max(1);
                
                for idx in 0..loop_limit {
                    let mut active_acct = None;
                    let current_key = if or_accounts.is_empty() {
                        openrouter_api_key()
                    } else {
                        let acct = &or_accounts[(start_idx + idx) % or_accounts.len()];
                        if usage_tracker.is_or_exhausted(acct.n) { continue; }
                        active_acct = Some(acct);
                        acct.token.clone()
                    };

                    let mut attempt = 0;
                    let max_attempts = 3;
                    let mut account_exhausted = false;

                    while attempt < max_attempts {
                        attempt += 1;
                        match ureq::post("https://openrouter.ai/api/v1/chat/completions")
                            .set("Authorization", &format!("Bearer {}", current_key))
                            .set("HTTP-Referer", "https://velocity-ide.local")
                            .set("X-Title", "Velocity Cognitive IDE")
                            .set("Content-Type", "application/json")
                            .send_json(&request_body)
                        {
                            Ok(res) => {
                                used_or_account = active_acct;
                                final_res = Some(res);
                                break;
                            }
                            Err(ureq::Error::Status(429, resp)) => {
                                let body = resp.into_string().unwrap_or_default();
                                let body_lower = body.to_lowercase();
                                if body_lower.contains("free-models-per-day") || body_lower.contains("quota") || body_lower.contains("credit") || body_lower.contains("limit exceeded") {
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
                                } else {
                                    if attempt < max_attempts {
                                        let wait_secs = attempt * 2;
                                        ui_tx.send(AgentToUiMessage::StatusUpdate(format!(
                                            "OpenRouter rate limit (429) on '{}'. Retrying in {}s (Attempt {}/{})…",
                                            active_acct.map(|a| a.label.as_str()).unwrap_or("default"),
                                            wait_secs, attempt, max_attempts
                                        ))).ok();
                                        std::thread::sleep(std::time::Duration::from_secs(wait_secs));
                                    } else {
                                        ui_tx.send(AgentToUiMessage::OutputToken(
                                            format!("\n\nOpenRouter rate limit error (429): {}", body)
                                        )).ok();
                                    }
                                }
                            }
                            Err(ureq::Error::Status(code, resp)) => {
                                let body = resp.into_string().unwrap_or_default();
                                ui_tx.send(AgentToUiMessage::OutputToken(
                                    format!("\n\nOpenRouter error ({}): {}", code, body)
                                )).ok();
                                break;
                            }
                            Err(e) => {
                                ui_tx.send(AgentToUiMessage::OutputToken(
                                    format!("\n\nOpenRouter connection error: {:?}", e)
                                )).ok();
                                break;
                            }
                        }
                    }
                    
                    if final_res.is_some() {
                        break;
                    }
                    if !account_exhausted {
                        ui_tx.send(AgentToUiMessage::StatusUpdate("OpenRouter request failed, trying next account key…".to_string())).ok();
                    }
                }
                final_res
            }
            AiProvider::CloudflareWorkersAi => {
                if accounts.is_empty() {
                    ui_tx.send(AgentToUiMessage::OutputToken(
                        "\n\nError: No Cloudflare accounts configured.".to_string()
                    )).ok();
                    break;
                }
                let start_idx = usage_tracker
                    .pick_account(accounts)
                    .and_then(|picked| accounts.iter().position(|a| a.n == picked.n))
                    .unwrap_or(0);
                let mut cf_response = None;
                for i in 0..accounts.len() {
                    let account = &accounts[(start_idx + i) % accounts.len()];
                    if usage_tracker.is_exhausted(account.n) { continue; }
                    let api_url = format!(
                        "https://api.cloudflare.com/client/v4/accounts/{}/ai/v1/chat/completions",
                        account.id
                    );
                    match ureq::post(&api_url)
                        .set("Authorization", &format!("Bearer {}", account.token))
                        .set("Content-Type", "application/json")
                        .send_json(&request_body)
                    {
                        Ok(res) => {
                            used_account = Some(account);
                            cf_response = Some(res);
                            break;
                        }
                        Err(ureq::Error::Status(_code, resp)) => {
                            let body = resp.into_string().unwrap_or_default();
                            if is_quota_exhausted_error(&body) {
                                usage_tracker.mark_exhausted(account.n, &account.label, &account.tier);
                                send_usage_update(usage_tracker, accounts, or_accounts, ui_tx);
                                ui_tx.send(AgentToUiMessage::StatusUpdate(format!(
                                    "Account '{}' quota exhausted — trying next…",
                                    account.label
                                ))).ok();
                            } else {
                                eprintln!("CF account {} HTTP error: {}", account.label, body);
                            }
                        }
                        Err(e) => {
                            eprintln!("CF account {} failed: {:?}", account.label, e);
                        }
                    }
                }
                cf_response
            }
        };

        let response = match ureq_response {
            Some(res) => res,
            None => {
                let err_msg = match provider {
                    AiProvider::OpenRouter => "OpenRouter request failed.",
                    AiProvider::CloudflareWorkersAi => "All Cloudflare Workers AI accounts exhausted or failed.",
                };
                ui_tx.send(AgentToUiMessage::OutputToken(format!("\n\nError: {err_msg}"))).ok();
                break;
            }
        };

        let tokens_in = compressed_history
            .iter()
            .map(|m| estimate_tokens(&m.content))
            .sum::<u64>();

        let mut reader = std::io::BufReader::new(response.into_reader());
        let mut line_buf = String::new();

        let mut assistant_content = String::new();
        let mut reasoning_content = String::new();
        let mut accumulated_tools: Vec<ToolCallAccumulator> = Vec::new();

        // ── Streaming display: suppress <tool_call> blocks regardless of profile ──
        // We detect inline tool-call syntax by content pattern, not by model capability
        // flags, because cached profiles may be stale. Models that don't emit <tool_call>
        // are unaffected.
        //
        // Once we see <tool_call> we flip to suppression mode and stop forwarding tokens
        // to the UI. We track how much of `assistant_content` we've already sent.
        let mut streamed_len: usize = 0;   // chars of assistant_content already sent to UI
        let mut suppressing = false;       // currently inside a <tool_call> block

        loop {
            line_buf.clear();
            match reader.read_line(&mut line_buf) {
                Ok(0) => break,
                Ok(_) => {
                    let cleaned = line_buf.trim();
                    if cleaned.is_empty() { continue; }
                    if cleaned == "data: [DONE]" { break; }

                    if cleaned.starts_with("data: ") {
                        let json_part = &cleaned[6..];
                        if let Ok(parsed) = serde_json::from_str::<Value>(json_part) {
                            if let Some(choices) = parsed["choices"].as_array() {
                                if let Some(first_choice) = choices.get(0) {
                                    let delta = &first_choice["delta"];

                                    // Reasoning tokens — always forward immediately to reasoning bubble
                                    if let Some(r) = delta["reasoning_content"].as_str()
                                        .or_else(|| delta["reasoning"].as_str())
                                    {
                                        reasoning_content.push_str(r);
                                        ui_tx.send(AgentToUiMessage::ThoughtToken(r.to_string())).ok();
                                    }

                                    // Content tokens — suppress <tool_call> blocks
                                    if let Some(tok) = delta["content"].as_str() {
                                        assistant_content.push_str(tok);

                                        // Walk the newly accumulated content and decide
                                        // what to forward, using a simple state machine.
                                        let ac = &assistant_content;
                                        loop {
                                            if suppressing {
                                                // Look for end of tool-call block
                                                let search = &ac[streamed_len..];
                                                let end = search.find("</function>")
                                                    .map(|p| (p, "</function>".len()))
                                                    .or_else(|| search.find("</tool_call>")
                                                        .map(|p| (p, "</tool_call>".len())))
                                                    // [Calling tool ...] ends at the closing ]
                                                    .or_else(|| search.find(']')
                                                        .map(|p| (p, 1)));
                                                if let Some((p, mlen)) = end {
                                                    streamed_len += p + mlen;
                                                    suppressing = false;
                                                } else {
                                                    break;
                                                }
                                            } else {
                                                let search = &ac[streamed_len..];
                                                // Detect either <tool_call> or [Calling tool
                                                let tc1 = search.find("<tool_call>").map(|p| (p, false));
                                                let tc2 = search.find("[Calling tool").map(|p| (p, true));
                                                let detected = match (tc1, tc2) {
                                                    (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
                                                    (Some(a), None) => Some(a),
                                                    (None, Some(b)) => Some(b),
                                                    (None, None) => None,
                                                };
                                                if let Some((p, _is_bracket)) = detected {
                                                    let safe = &search[..p];
                                                    if !safe.is_empty() {
                                                        ui_tx.send(AgentToUiMessage::OutputToken(sanitize_chat_token(safe))).ok();
                                                    }
                                                    streamed_len += p;
                                                    suppressing = true;
                                                } else {
                                                    let total = ac.len();
                                                    let mut safe_end = total.saturating_sub(14); // len("[Calling tool") + 1
                                                    while safe_end > streamed_len && !ac.is_char_boundary(safe_end) {
                                                        safe_end -= 1;
                                                    }
                                                    if safe_end > streamed_len {
                                                        let chunk = &ac[streamed_len..safe_end];
                                                        ui_tx.send(AgentToUiMessage::OutputToken(sanitize_chat_token(chunk))).ok();
                                                        streamed_len = safe_end;
                                                    }
                                                    break;
                                                }
                                            }
                                        }
                                    }

                                    // Standard OpenAI structured tool_calls deltas
                                    if let Some(tool_calls) = delta["tool_calls"].as_array() {
                                        for tc in tool_calls {
                                            let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                                            while accumulated_tools.len() <= idx {
                                                accumulated_tools.push(ToolCallAccumulator {
                                                    id: String::new(), name: String::new(), arguments: String::new(),
                                                });
                                            }
                                            if let Some(id) = tc["id"].as_str() {
                                                accumulated_tools[idx].id.push_str(id);
                                            }
                                            if let Some(func) = tc["function"].as_object() {
                                                if let Some(n) = func.get("name").and_then(|v| v.as_str()) {
                                                    accumulated_tools[idx].name.push_str(n);
                                                }
                                                if let Some(a) = func.get("arguments").and_then(|v| v.as_str()) {
                                                    accumulated_tools[idx].arguments.push_str(a);
                                                }
                                            }
                                        }
                                    }
                                }
                            } else if let Some(content) = parsed["response"].as_str()
                                .or_else(|| parsed["output"].as_str())
                                .or_else(|| parsed["text"].as_str())
                            {
                                assistant_content.push_str(content);
                                streamed_len += content.len();
                                ui_tx.send(AgentToUiMessage::OutputToken(sanitize_chat_token(content))).ok();
                            }
                        }
                    }
                }
                Err(e) => {
                    ui_tx.send(AgentToUiMessage::OutputToken(
                        format!("\nError reading stream: {:?}", e)
                    )).ok();
                    break;
                }
            }
        }

        // Flush any remaining clean content after stream ends
        if !suppressing && streamed_len <= assistant_content.len() {
            // Clamp streamed_len to a valid char boundary (guards against multibyte chars)
            let mut flush_start = streamed_len;
            while flush_start > 0 && !assistant_content.is_char_boundary(flush_start) {
                flush_start -= 1;
            }
            let tail = &assistant_content[flush_start..];
            if !tail.is_empty() {
                ui_tx.send(AgentToUiMessage::OutputToken(sanitize_chat_token(tail))).ok();
            }
        }

        // ── Inline tool-call parsing (Nemotron / Llama-style) ────────────────
        // Nemotron format (actual observed output):
        //   <tool_call>\n<function=NAME>\n<parameter=K>V</parameter>\n</function>
        // The block ends with </function> — there is NO </tool_call> closing tag.
        // Also handle </tool_call> for models that do include it.
        // Detect inline tool calls by content pattern — no profile flag needed.
        // This handles Nemotron and any other model that emits <tool_call> as text.
        if accumulated_tools.is_empty() && assistant_content.contains("<tool_call>") {
            // Helper: find the end of a <tool_call> block — either </function>,
            // </tool_call>, or the start of the NEXT <tool_call> (whichever comes first).
            fn find_block_end(s: &str) -> Option<(&str, &str)> {
                // candidate positions
                let ef = s.find("</function>").map(|p| (p, p + "</function>".len()));
                let et = s.find("</tool_call>").map(|p| (p, p + "</tool_call>".len()));
                let en = s.find("<tool_call>")  // next call starts → current one ends here
                    .and_then(|p| if p > 0 { Some((p, p)) } else { None });
                // pick earliest
                let best = [ef, et, en].into_iter().flatten()
                    .min_by_key(|(pos, _)| *pos);
                best.map(|(_, after)| (&s[..after - (after - after)], &s[after..]))
                    // simpler: just return (block_content_before_end, rest_after_end)
            }

            let mut clean_content = String::new();
            let mut rest = assistant_content.as_str();
            while let Some(start) = rest.find("<tool_call>") {
                clean_content.push_str(&rest[..start]);
                let after_open = &rest[start + "<tool_call>".len()..];

                // Find block end: </function>, </tool_call>, or next <tool_call>
                let ef = after_open.find("</function>").map(|p| (p, p + "</function>".len()));
                let et = after_open.find("</tool_call>").map(|p| (p, p + "</tool_call>".len()));
                // next <tool_call> with offset > 0 means current block has no explicit close
                let en = after_open.find("<tool_call>").and_then(|p| if p > 0 { Some((p, p)) } else { None });
                let best = [ef, et, en].into_iter().flatten().min_by_key(|(pos, _)| *pos);

                let (block, remainder) = if let Some((end_pos, after_end)) = best {
                    (&after_open[..end_pos], &after_open[after_end..])
                } else {
                    // No closing marker — treat whole remainder as one block
                    (after_open, "")
                };

                // Parse <function=NAME>...<parameter=K>V</parameter>...
                if let Some(fname_start) = block.find("<function=") {
                    let fname_rest = &block[fname_start + "<function=".len()..];
                    let fname_end = fname_rest.find('>').or_else(|| fname_rest.find('\n')).unwrap_or(fname_rest.len());
                    let fname = fname_rest[..fname_end].trim().to_string();
                    if !fname.is_empty() {
                        let mut args = serde_json::Map::new();
                        let mut param_rest = fname_rest;
                        while let Some(ps) = param_rest.find("<parameter=") {
                            let after_ps = &param_rest[ps + "<parameter=".len()..];
                            let key_end = after_ps.find('>').unwrap_or(after_ps.len());
                            let key = after_ps[..key_end].to_string();
                            let val_start = key_end + 1;
                            let val_end = after_ps[val_start..]
                                .find("</parameter>")
                                .map(|e| val_start + e)
                                .unwrap_or(after_ps.len());
                            let val = after_ps[val_start..val_end].trim().to_string();
                            args.insert(key, Value::String(val));
                            param_rest = &after_ps[val_end..];
                        }
                        let call_id = format!("inline_{}", accumulated_tools.len());
                        accumulated_tools.push(ToolCallAccumulator {
                            id: call_id,
                            name: fname,
                            arguments: serde_json::to_string(&args)
                                .unwrap_or_else(|_| "{}".to_string()),
                        });
                    }
                }
                rest = remainder;
                if rest.is_empty() { break; }
            }
            clean_content.push_str(rest);
            assistant_content = clean_content.trim().to_string();
        }
        // ── Format 2: [Calling tool 'NAME' with arguments 'JSON'] ─────────────
        // Some OpenRouter models emit tool calls in this bracket notation.
        // Only run if <tool_call> parsing found nothing.
        if accumulated_tools.is_empty() && assistant_content.contains("[Calling tool ") {
            let marker = "[Calling tool ";
            let mut clean2 = String::new();
            let mut rest2 = assistant_content.as_str();
            while let Some(start) = rest2.find(marker) {
                clean2.push_str(&rest2[..start]);
                let after = &rest2[start + marker.len()..];
                // Extract tool name between first ' and next '
                if let Some(name_end) = after.find('\'') {
                    let raw_name = &after[..name_end];
                    // skip past " with arguments '"
                    let args_marker = " with arguments '";
                    if let Some(args_start_rel) = after[name_end..].find(args_marker) {
                        let args_start = name_end + args_start_rel + args_marker.len();
                        // args end at the closing ']
                        let args_section = &after[args_start..];
                        let args_end = args_section.find("']").unwrap_or(args_section.len());
                        let args_str = &args_section[..args_end];
                        let arguments: Value = serde_json::from_str(args_str).unwrap_or(json!({}));
                        let call_id = format!("bracket_{}", accumulated_tools.len());
                        accumulated_tools.push(ToolCallAccumulator {
                            id: call_id,
                            name: raw_name.to_string(),
                            arguments: serde_json::to_string(&arguments)
                                .unwrap_or_else(|_| "{}".to_string()),
                        });
                        // Advance past the full match: marker + after[..args_start + args_end + "']".len()]
                        let consumed_in_after = args_start + args_end + "']".len();
                        rest2 = &rest2[start + marker.len() + consumed_in_after.min(after.len())..];
                    } else {
                        // No args section found — keep remainder as-is
                        clean2.push_str(&rest2[start..]);
                        rest2 = &rest2[rest2.len()..];
                        break;
                    }
                } else {
                    clean2.push_str(&rest2[start..]);
                    rest2 = &rest2[rest2.len()..];
                    break;
                }
            }
            clean2.push_str(rest2);
            if !accumulated_tools.is_empty() {
                assistant_content = clean2.trim().to_string();
            }
        }
        let final_tool_calls_value = if !accumulated_tools.is_empty() {
            let tc_json: Vec<Value> = accumulated_tools.iter().map(|t| {
                json!({
                    "id": t.id,
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "arguments": t.arguments
                    }
                })
            }).collect();
            Some(Value::Array(tc_json))
        } else {
            None
        };

        // Prepends reasoning content wrapped in <think> tags if any was received, matching standard format
        let final_saved_content = if !reasoning_content.is_empty() {
            format!("<think>\n{}\n</think>\n{}", reasoning_content, assistant_content)
        } else {
            assistant_content.clone()
        };

        message_history.push(ChatMessage {
            role: "assistant".to_string(),
            content: final_saved_content,
            name: None,
            tool_call_id: None,
            tool_calls: final_tool_calls_value.clone(),
        });

        if let Some(account) = used_account {
            let tokens_out = estimate_tokens(&assistant_content) + estimate_tokens(&reasoning_content);
            usage_tracker.record_request(
                account.n,
                &account.label,
                &account.tier,
                tokens_in,
                tokens_out,
            );
            send_usage_update(usage_tracker, accounts, or_accounts, ui_tx);
            ui_tx.send(AgentToUiMessage::StatusUpdate(format!(
                "Using account: {} ({} req today)",
                account.label,
                usage_tracker
                    .build_views(accounts, or_accounts)
                    .iter()
                    .find(|v| v.n == account.n && v.label == account.label)
                    .map(|v| v.requests)
                    .unwrap_or(0)
            ))).ok();
        }

        if let Some(account) = used_or_account {
            let tokens_out = estimate_tokens(&assistant_content) + estimate_tokens(&reasoning_content);
            usage_tracker.record_or_request(
                account.n,
                &account.label,
                &account.tier,
                tokens_in,
                tokens_out,
            );
            send_usage_update(usage_tracker, accounts, or_accounts, ui_tx);
            ui_tx.send(AgentToUiMessage::StatusUpdate(format!(
                "Using OpenRouter: {} ({} req today)",
                account.label,
                usage_tracker
                    .build_views(accounts, or_accounts)
                    .iter()
                    .find(|v| v.label == account.label)
                    .map(|v| v.requests)
                    .unwrap_or(0)
            ))).ok();
        }

        // Handle tool calls if any
        if let Some(ref tcs) = final_tool_calls_value {
            let tool_calls_arr = tcs.as_array().unwrap();
            
            let mut pending_ids = std::collections::HashSet::new();
            let mut tool_specs = Vec::new();
            
            for tc in tool_calls_arr {
                let call_id = tc["id"].as_str().unwrap_or("").to_string();
                let tool_name = tc["function"]["name"].as_str().unwrap_or("").to_string();
                let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                let arguments: Value = serde_json::from_str(args_str).unwrap_or(json!({}));
                
                ui_tx.send(AgentToUiMessage::StatusUpdate(format!("Requesting approval for tool: {}", tool_name))).ok();
                ui_tx.send(AgentToUiMessage::RequestToolApproval {
                    id: call_id.clone(),
                    tool_name: tool_name.clone(),
                    arguments: arguments.clone(),
                }).ok();
                
                pending_ids.insert(call_id.clone());
                tool_specs.push((call_id, tool_name, arguments));
            }
            
            // Wait for all approvals
            let mut resolved_approvals = std::collections::HashMap::new();
            
            while !pending_ids.is_empty() {
                if let Ok(ui_msg) = ui_rx.recv() {
                    match ui_msg {
                        UiToAgentMessage::ApproveTool { id, tool_name: _, arguments } => {
                            if pending_ids.remove(&id) {
                                resolved_approvals.insert(id, Some(arguments));
                            }
                        }
                        UiToAgentMessage::RejectTool { id, tool_name: _ } => {
                            if pending_ids.remove(&id) {
                                resolved_approvals.insert(id, None);
                            }
                        }
                        _ => {}
                    }
                } else {
                    break;
                }
            }
            
            // Execute approved tools in parallel
            let mut handles = Vec::new();
            
            for (call_id, tool_name, _original_arguments) in tool_specs {
                let approval = resolved_approvals.get(&call_id).cloned().flatten();
                let workspace_root_clone = workspace_root.clone();
                let ui_tx_clone = ui_tx.clone();
                
                let handle = std::thread::spawn(move || {
                    let mut tool_result = String::new();
                    let mut file_buffer_update = None;
                    let mut changelog_entry = None;
                    
                    if let Some(approved_args) = approval {
                        ui_tx_clone.send(AgentToUiMessage::ToolExecutionStarted { tool_name: tool_name.clone() }).ok();
                        
                        match registry::call_tool_in_workspace(&workspace_root_clone, &tool_name, &approved_args) {
                            Ok(res) => {
                                tool_result = res;
                                if tool_name == "write_file" {
                                    if let Some(rel_path) = approved_args["relativeFilePath"].as_str() {
                                        let full_path = workspace_root_clone.join(rel_path);
                                        if let Some(content) = approved_args["content"].as_str() {
                                            file_buffer_update = Some((full_path, content.to_string()));
                                        }
                                        changelog_entry = Some((rel_path.to_string(), "write_file"));
                                    }
                                }
                            }
                            Err(e) => {
                                tool_result = format!("Error executing tool: {:?}", e);
                            }
                        }
                        
                        ui_tx_clone.send(AgentToUiMessage::ToolExecutionFinished {
                            tool_name: tool_name.clone(),
                            result: tool_result.clone(),
                        }).ok();
                    } else {
                        tool_result = "Error: Tool execution rejected by the user.".to_string();
                    }
                    
                    (call_id, tool_name, tool_result, file_buffer_update, changelog_entry)
                });
                
                handles.push(handle);
            }
            
            let mut thread_results = Vec::new();
            for h in handles {
                if let Ok(res) = h.join() {
                    thread_results.push(res);
                }
            }
            
            let mut any_success = false;
            let mut any_rejected = false;
            let mut any_error = false;
            
            for (call_id, tool_name, tool_result, file_buffer_update, changelog_entry) in thread_results {
                if tool_result.contains("Error executing tool") {
                    any_error = true;
                } else if tool_result.contains("rejected by the user") {
                    any_rejected = true;
                } else {
                    any_success = true;
                }
                
                if let Some((path, content)) = file_buffer_update {
                    ui_tx.send(AgentToUiMessage::UpdateFileBuffer { path, content }).ok();
                }
                if let Some((rel_path, action)) = changelog_entry {
                    append_changelog_nda(workspace_root, &rel_path, action);
                    sitemap_needed = true;
                }
                
                // Add tool response to history
                message_history.push(ChatMessage {
                    role: "tool".to_string(),
                    content: tool_result,
                    name: Some(tool_name.clone()),
                    tool_call_id: Some(call_id),
                    tool_calls: None,
                });
            }
            
            // Handover update
            if any_error {
                write_handover_nda(workspace_root, "tool_error", loop_count, "tool error in batch", false);
            } else if any_rejected {
                write_handover_nda(workspace_root, "tool_rejected", loop_count, "user reject in batch", false);
            } else if any_success {
                write_handover_nda(workspace_root, "executing", loop_count, "batch ok", false);
            }
            
            // Save progress chatlogs
            save_chatlogs_nda(workspace_root, message_history);
        } else {
            // No tool calls, the model claims it is done. Let's validate compilation.
            ui_tx.send(AgentToUiMessage::StatusUpdate("Running automatic compilation validation...".to_string())).ok();
            match run_compilation_check(workspace_root) {
                Ok(()) => {
                    write_handover_nda(workspace_root, "idle", loop_count, "compiler validated", false);
                    ui_tx.send(AgentToUiMessage::StatusUpdate("Compiler validation succeeded!".to_string())).ok();
                    break;
                }
                Err(errors) => {
                    if loop_count < max_loops {
                        write_handover_nda(workspace_root, "self_correcting", loop_count, "compile_failed", false);
                        ui_tx.send(AgentToUiMessage::StatusUpdate("Compilation failed! Self-correcting...".to_string())).ok();
                        
                        let error_prompt = format!(
                            "[SYSTEM NOTIFICATION: compiler validation failed. Please fix the following build errors immediately]\n{}",
                            errors
                        );
                        message_history.push(ChatMessage {
                            role: "user".to_string(),
                            content: error_prompt,
                            name: None,
                            tool_call_id: None,
                            tool_calls: None,
                        });
                        
                        // Continue flat loop iteration to self-correct
                        continue;
                    } else {
                        write_handover_nda(workspace_root, "idle", loop_count, "compile_failed", false);
                        ui_tx.send(AgentToUiMessage::StatusUpdate("Compilation validation failed (Max limits reached)".to_string())).ok();
                        break;
                    }
                }
            }
        }
    }
    
    save_chatlogs_nda(workspace_root, message_history);
    
    if sitemap_needed {
        write_sitemap_nda(workspace_root);
    }
    convert_jsonl_to_nda(workspace_root);
    ui_tx.send(AgentToUiMessage::StatusUpdate("Agent workflow finished. Idling.".to_string())).ok();
    ui_tx.send(AgentToUiMessage::AgentFinished).ok();
}

fn hash_str(s: &str) -> u64 {
    use sha2::{Sha256, Digest};
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    let d = h.finalize();
    u64::from_le_bytes(d[..8].try_into().unwrap())
}

fn sanitize_chat_token(s: &str) -> String {
    let mut out = s.to_string();
    let tags = [
        "</tool_call>", "<tool_call>",
        "</function>", "<function>",
        "</parameter>", "<parameter>",
    ];
    for tag in &tags {
        out = out.replace(&format!("{}\r\n", tag), "");
        out = out.replace(&format!("{}\n", tag), "");
        out = out.replace(tag, "");
    }
    let mut result = String::with_capacity(out.len());
    let mut in_tag = false;
    let chars: Vec<char> = out.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            let mut j = i + 1;
            let mut is_tag_structure = false;
            while j < chars.len() && chars[j] != '>' {
                let c = chars[j];
                if c.is_alphabetic() || c == '/' || c == '=' || c == '_' || c == '-' || c.is_ascii_digit() || c == '\"' || c == '\'' || c == '.' {
                    is_tag_structure = true;
                } else {
                    is_tag_structure = false;
                    break;
                }
                j += 1;
            }
            if is_tag_structure && j < chars.len() && chars[j] == '>' {
                i = j + 1;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message() -> ChatMessage {
        ChatMessage {
            role: "user".into(),
            content: "hello".into(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }
    }

    #[test]
    fn openai_chat_profile_omits_tools_and_thinking() {
        let profile = ModelInfo {
            id: "@cf/example/chat".into(),
            label: "chat".into(),
            api_style: ApiStyle::OpenAiChat,
            supports_tools: false,
            supports_thinking: false,
        };
        let request = build_request(&profile, &profile.id, &[message()], &[json!({"type": "function"})], true, AiProvider::CloudflareWorkersAi);
        assert!(request.get("messages").is_some());
        assert!(request.get("tools").is_none());
        assert!(request.get("thinking").is_none());
    }

    #[test]
    fn serializes_last_request_as_nda() {
        let profile = ModelInfo {
            id: "@cf/example/chat".into(),
            label: "chat".into(),
            api_style: ApiStyle::OpenAiTools,
            supports_tools: true,
            supports_thinking: true,
        };
        let messages = vec![
            ChatMessage {
                role: "system".into(),
                content: "system prompt".into(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
            ChatMessage {
                role: "assistant".into(),
                content: "calling tool".into(),
                name: None,
                tool_call_id: None,
                tool_calls: Some(json!([{"id":"call_1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"src/main.rs\"}"}}])),
            },
        ];
        let tools = vec![json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read a file",
                "parameters": {"type": "object"}
            }
        })];
        let request = build_request(&profile, &profile.id, &messages, &tools, true, AiProvider::CloudflareWorkersAi);

        let nda = serialize_last_request_nda(
            &profile,
            &profile.id,
            AiProvider::CloudflareWorkersAi,
            true,
            &messages,
            &tools,
            &request,
        );

        assert!(nda.starts_with("last-request version 1\n"));
        assert!(nda.contains("provider cloudflare-workers-ai"));
        assert!(nda.contains("api_style openai-tools"));
        assert!(nda.contains("message_count 2"));
        assert!(nda.contains("tool_count 1"));
        assert!(nda.contains("message\t1\tassistant\t-\t-\tcalling tool\t["));
        assert!(nda.contains("read_file"));
        assert!(nda.contains("tool\t0\tread_file\tRead a file"));
    }

    #[test]
    fn writes_plaintext_transcript_nda() {
        let tmp = tempfile::tempdir().unwrap();
        let content = b"{\"role\":\"user\"}\n{\"role\":\"assistant\"}\n";

        write_workspace_transcript_nda(tmp.path(), content);
        let transcript = std::fs::read_to_string(tmp.path().join(".velocity").join("transcript.nda")).unwrap();
        assert!(transcript.starts_with("transcript version 1\n"));
        assert!(transcript.contains("source jsonl\n"));
        assert!(transcript.contains("line\t0\t{\"role\":\"user\"}"));
        assert!(transcript.contains("line\t1\t{\"role\":\"assistant\"}"));
        assert!(transcript.contains("trailing_newline true\n"));
    }

    #[test]
    fn writes_plaintext_sitemap_nda() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src").join("nested")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".velocity")).unwrap();
        std::fs::write(tmp.path().join("src").join("main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(tmp.path().join("src").join("nested").join("lib.rs"), "pub fn x() {}\n").unwrap();
        std::fs::write(tmp.path().join(".velocity").join("ignored.txt"), "ignore me").unwrap();

        write_sitemap_nda(tmp.path());
        let sitemap = std::fs::read_to_string(tmp.path().join(".velocity").join("sitemap.nda")).unwrap();
        assert!(sitemap.starts_with("sitemap version 1\n"));
        assert!(sitemap.contains("entry\tdir\tsrc"));
        assert!(sitemap.contains("entry\tdir\tsrc\\\\nested"));
        assert!(sitemap.contains("entry\tfile\tsrc\\\\main.rs\t"));
        assert!(sitemap.contains("entry\tfile\tsrc\\\\nested\\\\lib.rs\t"));
        assert!(!sitemap.contains("V.E.L.O.C.I.T.Y. Codebase Sitemap Registry"));
        assert!(!sitemap.contains("ignored.txt"));
    }

    #[test]
    fn writes_plaintext_chatlogs_nda() {
        let tmp = tempfile::tempdir().unwrap();
        let messages = vec![
            ChatMessage {
                role: "assistant".into(),
                content: "hello\nworld".into(),
                name: None,
                tool_call_id: None,
                tool_calls: Some(json!([{"id":"call_1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"src/main.rs\"}"}}])),
            },
            ChatMessage {
                role: "tool".into(),
                content: "done".into(),
                name: Some("read_file".into()),
                tool_call_id: Some("call_1".into()),
                tool_calls: None,
            },
        ];

        save_chatlogs_nda(tmp.path(), &messages);
        let chatlogs = std::fs::read_to_string(tmp.path().join(".velocity").join("chatlogs.nda")).unwrap();
        assert!(chatlogs.starts_with("chatlogs version 2\n"));
        assert!(chatlogs.contains("message_count 2"));
        assert!(chatlogs.contains("field\t0\trole\tassistant"));
        assert!(chatlogs.contains("field\t0\tcontent\thello\\nworld"));
        assert!(chatlogs.contains("field\t1\tname\tread_file"));
        assert!(chatlogs.contains("field\t1\ttool_call_id\tcall_1"));

        let loaded = load_chatlogs_nda(tmp.path()).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].content, "hello\nworld");
        assert!(loaded[0].tool_calls.is_some());
        assert_eq!(loaded[1].name.as_deref(), Some("read_file"));
        assert_eq!(loaded[1].tool_call_id.as_deref(), Some("call_1"));
    }

    #[test]
    fn loads_legacy_v1_chatlogs_nda() {
        let tmp = tempfile::tempdir().unwrap();
        let velocity_dir = tmp.path().join(".velocity");
        std::fs::create_dir_all(&velocity_dir).unwrap();
        std::fs::write(
            velocity_dir.join("chatlogs.nda"),
            "chatlogs version 1\nmessage\t0\tassistant\t-\t-\thello\\nworld\t-\nmessage\t1\ttool\tread_file\tcall_1\tdone\t-\n",
        )
        .unwrap();

        let loaded = load_chatlogs_nda(tmp.path()).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].role, "assistant");
        assert_eq!(loaded[0].content, "hello\nworld");
        assert_eq!(loaded[1].role, "tool");
        assert_eq!(loaded[1].name.as_deref(), Some("read_file"));
        assert_eq!(loaded[1].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(loaded[1].content, "done");
    }

    #[test]
    fn loads_legacy_ndav_chatlogs_nda() {
        let tmp = tempfile::tempdir().unwrap();
        let velocity_dir = tmp.path().join(".velocity");
        std::fs::create_dir_all(&velocity_dir).unwrap();
        let legacy = "user\nhello\n---\ntool\nread_file\tcall_1\ndone";
        std::fs::write(
            velocity_dir.join("chatlogs.nda"),
            pack_ndav("chatlogs.txt", legacy.as_bytes()),
        )
        .unwrap();

        let loaded = load_chatlogs_nda(tmp.path()).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].role, "user");
        assert_eq!(loaded[0].content, "hello");
        assert_eq!(loaded[1].role, "tool");
        assert_eq!(loaded[1].name.as_deref(), Some("read_file"));
        assert_eq!(loaded[1].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(loaded[1].content, "done");
    }

    #[test]
    fn writes_plaintext_handover_nda() {
        let tmp = tempfile::tempdir().unwrap();
        write_handover_nda(tmp.path(), "self_correcting", 7, "compile failed", true);
        let handover = std::fs::read_to_string(tmp.path().join(".velocity").join("handover.nda")).unwrap();
        assert!(handover.starts_with("handover version 1\n"));
        assert!(handover.contains("state self_correcting"));
        assert!(handover.contains("turn 7"));
        assert!(handover.contains("build compile failed"));
        assert!(handover.contains("interrupted true"));
    }

    #[test]
    fn appends_plaintext_changelog_nda() {
        let tmp = tempfile::tempdir().unwrap();
        append_changelog_nda(tmp.path(), "src/main.rs", "edited");
        append_changelog_nda(tmp.path(), "src/lib.rs", "created");
        let changelog = std::fs::read_to_string(tmp.path().join(".velocity").join("changelog.nda")).unwrap();
        assert!(changelog.starts_with("changelog version 1\n"));
        assert!(changelog.contains("src/main.rs\tedited"));
        assert!(changelog.contains("src/lib.rs\tcreated"));
    }

    #[test]
    fn appends_to_legacy_ndav_changelog() {
        let tmp = tempfile::tempdir().unwrap();
        let velocity_dir = tmp.path().join(".velocity");
        std::fs::create_dir_all(&velocity_dir).unwrap();
        std::fs::write(
            velocity_dir.join("changelog.nda"),
            pack_ndav("changelog.txt", b"123\tsrc/old.rs\tupdated\n"),
        )
        .unwrap();

        append_changelog_nda(tmp.path(), "src/new.rs", "created");
        let changelog = std::fs::read_to_string(velocity_dir.join("changelog.nda")).unwrap();
        assert!(changelog.starts_with("changelog version 1\n"));
        assert!(changelog.contains("src/old.rs\tupdated"));
        assert!(changelog.contains("src/new.rs\tcreated"));
    }

    #[test]
    fn test_sanitize_chat_token_removes_tags() {
        let input = "Hello there </tool_call>\n</tool_call>\n<parameter=path>src/main.rs</parameter> checking.";
        let expected = "Hello there src/main.rs checking.";
        assert_eq!(sanitize_chat_token(input).trim(), expected);
    }

    #[test]
    fn prompt_profile_uses_prompt_and_no_tools() {
        let profile = ModelInfo {
            id: "@cf/example/base".into(),
            label: "base".into(),
            api_style: ApiStyle::PromptCompletion,
            supports_tools: false,
            supports_thinking: false,
        };
        let request = build_request(&profile, &profile.id, &[message()], &[json!({"type": "function"})], false, AiProvider::CloudflareWorkersAi);
        assert_eq!(request["prompt"], "user: hello");
        assert!(request.get("messages").is_none());
        assert!(request.get("tools").is_none());
    }

    #[test]
    fn compress_history_flattens_tools_when_unsupported() {
        let original_messages = vec![
            ChatMessage {
                role: "assistant".to_string(),
                content: "".to_string(),
                name: None,
                tool_call_id: None,
                tool_calls: Some(json!([
                    {
                        "id": "call_abc",
                        "type": "function",
                        "function": {
                            "name": "write_file",
                            "arguments": "{\"path\":\"hello.txt\"}"
                        }
                    }
                ])),
            },
            ChatMessage {
                role: "tool".to_string(),
                content: "Success".to_string(),
                name: Some("write_file".to_string()),
                tool_call_id: Some("call_abc".to_string()),
                tool_calls: None,
            }
        ];

        // When supports_tools = false
        let compressed = compress_history(&original_messages, false);
        assert_eq!(compressed.len(), 2);
        
        // Assistant message should be flattened
        assert_eq!(compressed[0].role, "assistant");
        assert_eq!(compressed[0].content, "[Calling tool 'write_file' with arguments '{\"path\":\"hello.txt\"}']");
        assert!(compressed[0].tool_calls.is_none());

        // Tool message should become user message
        assert_eq!(compressed[1].role, "user");
        assert_eq!(compressed[1].content, "[Tool result for 'write_file']: Success");
        assert!(compressed[1].name.is_none());
        assert!(compressed[1].tool_call_id.is_none());
    }

    #[test]
    fn writes_last_request_artifacts() {
        let tmp = tempfile::tempdir().unwrap();
        let profile = ModelInfo {
            id: "@cf/example/chat".into(),
            label: "chat".into(),
            api_style: ApiStyle::OpenAiChat,
            supports_tools: false,
            supports_thinking: false,
        };
        let messages = vec![message()];
        let tools = vec![json!({"type": "function", "function": {"name": "search", "description": "Search"}})];
        let request = build_request(&profile, &profile.id, &messages, &tools, false, AiProvider::CloudflareWorkersAi);

        write_last_request_artifacts(
            tmp.path(),
            &profile,
            &profile.id,
            AiProvider::CloudflareWorkersAi,
            false,
            &messages,
            &tools,
            &request,
        );

        let nda = std::fs::read_to_string(tmp.path().join(".velocity").join("last_request.nda")).unwrap();
        let json = std::fs::read_to_string(tmp.path().join(".velocity").join("last_request.json")).unwrap();
        assert!(nda.contains("last-request version 1"));
        assert!(json.contains("\"model\""));
    }

    #[test]
    fn test_eager_merkle_compaction() {
        let mut long_content = String::new();
        long_content.push_str("fn test_function() {\n    println!(\"Hello\");\n}\nclass TestClass {}");
        for i in 0..100 {
            long_content.push_str(&format!("\n// Dummy line padding number {} to ensure we are well above the one thousand character compaction threshold.", i));
        }

        let original_messages = vec![
            ChatMessage {
                role: "tool".to_string(),
                content: long_content,
                name: Some("read_file".to_string()),
                tool_call_id: Some("call_xyz".to_string()),
                tool_calls: None,
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "I have read the file.".to_string(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            }
        ];

        let compressed = compress_history(&original_messages, true);
        assert_eq!(compressed.len(), 2);
        assert_eq!(compressed[0].role, "tool");
        
        let content = &compressed[0].content;
        assert!(content.contains("compressed to optimize context"));
        assert!(content.contains("Merkle Hash:"));
        assert!(content.contains("fn test_function"));
        assert!(content.contains("class TestClass"));
    }
}
