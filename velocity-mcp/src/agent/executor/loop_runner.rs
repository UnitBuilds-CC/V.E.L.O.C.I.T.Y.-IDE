use super::super::checkpoint::CheckpointManager;
use super::super::coordination::CoordinationBus;
use super::super::memory_store::PersistentMemory;
use super::super::models::*;
use super::super::nda::*;
use super::super::provider::*;
use super::super::self_improve::ImprovementEngine;
use super::context_budget::compress_history_with_budget;
use super::router_client::{
    self, AssignmentRequest, AssignmentStatus, ExecutionMode, ExecutionTier,
};
use super::thread::{apply_headless_control_messages, run_compilation_check};
use super::utils::{build_request, estimate_tokens, sanitize_chat_token, send_usage_update};
use crate::registry;
use crate::safety::SafeMutex;
use crate::usage::{
    AzureOpenAiAccount, CloudflareAccount, LocalOllamaAccount, OpenRouterAccount, UsageTracker,
    WorkspaceRouterSettings,
};
use crossbeam_channel::{Receiver, Sender};
use serde_json::{json, Value};
use std::io::BufRead;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub fn run_agent_reasoning_loop(
    workspace_root: &PathBuf,
    accounts: &[CloudflareAccount],
    or_accounts: &[OpenRouterAccount],
    azure_accounts: &[AzureOpenAiAccount],
    ollama_accounts: &[LocalOllamaAccount],
    model: &str,
    profile: &ModelInfo,
    provider: AiProvider,
    thinking: bool,
    message_history: &mut Vec<ChatMessage>,
    usage_tracker: &mut UsageTracker,
    ui_rx: &Receiver<UiToAgentMessage>,
    cancel_rx: Option<&Receiver<UiToAgentMessage>>,
    progress: Option<&Arc<Mutex<HeadlessSubAgentProgress>>>,
    ui_tx: &Sender<AgentToUiMessage>,
    deferred_messages: &mut Vec<UiToAgentMessage>,
    coordination_bus: &CoordinationBus,
    router_settings: Option<&WorkspaceRouterSettings>,
    max_loops_override: Option<usize>,
) {
    let mut sitemap_needed = false;
    let mut loop_count: usize = 0;
    let max_loops: usize = max_loops_override.unwrap_or(15);
    let mut current_provider = provider;
    let mut current_model = model.to_string();
    let mut current_profile = profile.clone();
    let mut current_thinking = thinking && current_profile.supports_thinking;
    let mut fallback_attempts: usize = 0;
    // Budget for same-provider retries on hard transport failures; bounded so
    // a permanently dead provider still terminates the run (as "blocked").
    let mut hard_fail_retries: u32 = 0;
    // Budget for auto-continuations when the model announces an action but
    // returns no tool call ("Let me fix X:" → turn ends). Bounded so a model
    // that genuinely loops on announcements still terminates.
    let mut auto_continuations: u32 = 0;

    // Phase 2: Workspace checkpointing for safe, reversible tool operations
    let mut checkpoint_mgr = CheckpointManager::new(workspace_root);
    // Checkpoint created before the most recent file-modifying batch, so a
    // fully-failed batch can be rolled back.
    let mut last_checkpoint_id: Option<usize> = None;

    // Phase 3: Persistent memory for cross-session learning
    let mut memory = PersistentMemory::open(workspace_root);

    // Phase 4: Self-improvement engine — analyzes failures and refines prompts
    let mut improve_engine = ImprovementEngine::new(&memory);

    // Inject previously-learned directives into system prompt at session start
    let learned_directives = ImprovementEngine::recall_directives(&memory, 5);
    if !learned_directives.is_empty() {
        if let Some(sys_msg) = message_history.iter_mut().find(|m| m.role == "system") {
            sys_msg
                .content
                .push_str("\n\n## Previously Learned Patterns\n");
            for d in &learned_directives {
                sys_msg.content.push_str(&format!("- {}\n", d));
            }
        }
    }

    // T2b: LSP-gated writes — track files written this session to prevent
    // repeated overwrites when build diagnostics report errors.
    let lsp_written_files: Arc<Mutex<std::collections::HashSet<PathBuf>>> =
        Arc::new(Mutex::new(std::collections::HashSet::new()));

    let registered_tools = registry::get_tools();
    let cf_tools: Vec<Value> = registered_tools
        .iter()
        .map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema
                }
            })
        })
        .collect();

    while loop_count < max_loops {
        if apply_headless_control_messages(cancel_rx, message_history, ui_tx, progress) {
            break;
        }
        loop_count += 1;
        ui_tx
            .send(AgentToUiMessage::StatusUpdate(format!(
                "Querying {} (Turn {})\u{2026}",
                current_provider.label(),
                loop_count
            )))
            .ok();

        // T1a: Inject recalled memories into system prompt for context-aware planning
        if !memory.is_empty() {
            let last_user_msg = message_history
                .iter()
                .rev()
                .find(|m| m.role == "user")
                .map(|m| m.content.clone())
                .unwrap_or_default();
            if !last_user_msg.is_empty() {
                let hits = memory.recall(&last_user_msg, 3);
                if !hits.is_empty() {
                    let recall_block: String = hits
                        .iter()
                        .map(|h| format!("- [{}] {}", h.entry.key, h.entry.content))
                        .collect::<Vec<_>>()
                        .join("\n");
                    if let Some(sys_msg) = message_history.iter_mut().find(|m| m.role == "system") {
                        // Remove any previous recall block before appending new one
                        if let Some(idx) = sys_msg.content.find("\n\n## Recalled Context") {
                            sys_msg.content.truncate(idx);
                        }
                        sys_msg.content.push_str(&format!(
                            "\n\n## Recalled Context (from past sessions)\n{}",
                            recall_block
                        ));
                    }
                }
            }
        }

        let compressed_history = compress_history_with_budget(
            message_history,
            &current_model,
            current_profile.supports_tools,
            &cf_tools,
        );

        let request_body = build_request(
            &current_profile,
            &current_model,
            &compressed_history,
            &cf_tools,
            current_thinking,
            current_provider,
        );
        write_last_request_artifacts(
            workspace_root,
            &current_profile,
            &current_model,
            current_provider,
            current_thinking,
            &compressed_history,
            &cf_tools,
            &request_body,
        );

        // ─── Velocity Router MoA Dispatch ─────────────────────────────────────
        // If the router is enabled and available, try routing through the MoA
        // orchestrator first. If it succeeds, we handle the response and skip
        // direct provider dispatch. If it fails, we fall through to direct.
        let router_handled = if let Some(router_cfg) = router_settings {
            if router_cfg.enabled && !router_cfg.api_key.trim().is_empty() {
                // Check router health (cached after first check)
                let router_available = router_client::is_router_available()
                    || router_client::refresh_router_availability(&router_cfg.url);

                if router_available {
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "Routing through Velocity MoA (Turn {})\u{2026}",
                            loop_count
                        )))
                        .ok();

                    // Extract the user's prompt from message history
                    let user_prompt = message_history
                        .iter()
                        .rev()
                        .find(|m| m.role == "user")
                        .map(|m| m.content.clone())
                        .unwrap_or_default();

                    if !user_prompt.is_empty() {
                        // Gather workspace context: extract file paths from the prompt
                        // and read their contents for the MoA specialists.
                        let (file_paths, context) =
                            gather_workspace_context(workspace_root, &user_prompt);

                        let router_request = AssignmentRequest {
                            task: user_prompt,
                            tier: ExecutionTier::Ultimate,
                            context: if context.is_empty() {
                                None
                            } else {
                                Some(context)
                            },
                            file_paths,
                            mode: ExecutionMode::Sync,
                            max_cost_usd: None,
                        };

                        match router_client::submit_assignment(
                            &router_cfg.url,
                            &router_cfg.api_key,
                            &router_request,
                        ) {
                            Ok(response) => {
                                if response.status == AssignmentStatus::Completed {
                                    if let Some(assembled) = response.assembled_output {
                                        // Stream the assembled output to the UI
                                        ui_tx
                                            .send(AgentToUiMessage::OutputToken(
                                                sanitize_chat_token(&assembled),
                                            ))
                                            .ok();

                                        // Add to message history
                                        message_history.push(ChatMessage {
                                            role: "assistant".to_string(),
                                            content: assembled.clone(),
                                            name: None,
                                            tool_call_id: None,
                                            tool_calls: None,
                                        });

                                        // Log routing info if available
                                        if let Some(plan) = &response.plan {
                                            let subtask_count = plan.subtasks.len();
                                            let models_used: Vec<_> = plan
                                                .subtasks
                                                .iter()
                                                .map(|s| format!("{} ({})", s.model, s.domain))
                                                .collect();
                                            ui_tx
                                                .send(AgentToUiMessage::StatusUpdate(format!(
                                                    "MoA: {} sub-tasks → {}",
                                                    subtask_count,
                                                    models_used.join(", ")
                                                )))
                                                .ok();
                                        }

                                        if let Some(cost) = &response.cost {
                                            ui_tx
                                                .send(AgentToUiMessage::StatusUpdate(format!(
                                                    "MoA cost: ${:.6} ({} tokens)",
                                                    cost.total_cost_usd, cost.total_tokens
                                                )))
                                                .ok();
                                        }

                                        // Router handled it — skip direct dispatch
                                        true
                                    } else {
                                        ui_tx
                                            .send(AgentToUiMessage::StatusUpdate(
                                                "MoA completed but produced no output. Falling back to direct dispatch.".to_string(),
                                            ))
                                            .ok();
                                        false
                                    }
                                } else if response.status == AssignmentStatus::Failed {
                                    let err =
                                        response.error.unwrap_or_else(|| "Unknown error".into());
                                    ui_tx
                                        .send(AgentToUiMessage::StatusUpdate(format!(
                                            "MoA failed: {}. Falling back to direct dispatch.",
                                            err
                                        )))
                                        .ok();
                                    false
                                } else {
                                    ui_tx
                                        .send(AgentToUiMessage::StatusUpdate(format!(
                                            "MoA status: {:?}. Falling back to direct dispatch.",
                                            response.status
                                        )))
                                        .ok();
                                    false
                                }
                            }
                            Err(e) => {
                                ui_tx
                                    .send(AgentToUiMessage::StatusUpdate(format!(
                                        "MoA unavailable ({}). Falling back to direct dispatch.",
                                        e
                                    )))
                                    .ok();
                                router_client::mark_router_unavailable();
                                false
                            }
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        // If the router handled the request, continue to the next iteration
        // (tool call processing, etc.)
        if router_handled {
            // Skip the rest of the dispatch loop — the response is already
            // in message_history and streamed to the UI.
            // The loop will continue to check for tool calls, etc.
            continue;
        }

        // ─── Direct Provider Dispatch (fallback) ──────────────────────────────
        let mut used_account: Option<&CloudflareAccount> = None;
        let mut used_or_account: Option<&OpenRouterAccount> = None;
        let ureq_response = match current_provider {
            AiProvider::OpenRouter => {
                let (res, acct) = super::dispatch::execute_openrouter_request(
                    or_accounts,
                    accounts,
                    usage_tracker,
                    &request_body,
                    ui_tx,
                );
                used_or_account = acct;
                res
            }
            AiProvider::CloudflareWorkersAi => {
                let (res, acct) = super::dispatch::execute_cloudflare_request(
                    accounts,
                    or_accounts,
                    usage_tracker,
                    &request_body,
                    ui_tx,
                );
                used_account = acct;
                res
            }
            AiProvider::AzureOpenAi => {
                super::dispatch::execute_azure_request(azure_accounts, &request_body, ui_tx)
            }
            AiProvider::LocalOllama => {
                super::dispatch::execute_ollama_request(ollama_accounts, &request_body, ui_tx)
            }
            AiProvider::Deepseek => {
                super::dispatch::execute_deepseek_request(&request_body, ui_tx, workspace_root)
            }
            AiProvider::AlibabaQwen => {
                super::dispatch::execute_alibaba_qwen_request(&request_body, ui_tx, workspace_root)
            }
            AiProvider::AwsBedrock => {
                super::dispatch::execute_bedrock_request(&request_body, ui_tx, workspace_root)
            }
            AiProvider::Groq => {
                super::dispatch::execute_groq_request(&request_body, ui_tx, workspace_root)
            }
            AiProvider::Mistral => {
                super::dispatch::execute_mistral_request(&request_body, ui_tx, workspace_root)
            }
            AiProvider::OpenAI => {
                super::dispatch::execute_openai_request(&request_body, ui_tx, workspace_root)
            }
            AiProvider::GoogleVertex => {
                super::dispatch::execute_google_request(&request_body, ui_tx, workspace_root)
            }
            AiProvider::TogetherAi => {
                super::dispatch::execute_together_request(&request_body, ui_tx, workspace_root)
            }
            AiProvider::FireworksAi => {
                super::dispatch::execute_fireworks_request(&request_body, ui_tx, workspace_root)
            }
            AiProvider::Perplexity => {
                super::dispatch::execute_perplexity_request(&request_body, ui_tx, workspace_root)
            }
            AiProvider::Cerebras => {
                super::dispatch::execute_cerebras_request(&request_body, ui_tx, workspace_root)
            }
            AiProvider::Anthropic => {
                super::dispatch::execute_anthropic_request(&request_body, ui_tx, workspace_root)
            }
        };

        let response = match ureq_response {
            Some(res) => res,
            None => {
                let fallback = fallback_provider(current_provider);
                let fallback_available = match fallback {
                    AiProvider::OpenRouter => {
                        !or_accounts.is_empty() || !openrouter_api_key().trim().is_empty()
                    }
                    AiProvider::CloudflareWorkersAi => !accounts.is_empty(),
                    AiProvider::AzureOpenAi => !azure_accounts.is_empty(),
                    AiProvider::LocalOllama => !ollama_accounts.is_empty(),
                    AiProvider::Deepseek => !std::env::var("DEEPSEEK_API_KEY")
                        .unwrap_or_default()
                        .trim()
                        .is_empty(),
                    AiProvider::AlibabaQwen => !std::env::var("DASHSCOPE_API_KEY")
                        .unwrap_or_default()
                        .trim()
                        .is_empty(),
                    AiProvider::AwsBedrock => std::env::var("BEDROCK_PROXY_URL").is_ok(),
                    AiProvider::Groq => !std::env::var("GROQ_API_KEY")
                        .unwrap_or_default()
                        .trim()
                        .is_empty(),
                    AiProvider::Mistral => !std::env::var("MISTRAL_API_KEY")
                        .unwrap_or_default()
                        .trim()
                        .is_empty(),
                    AiProvider::OpenAI => !std::env::var("OPENAI_API_KEY")
                        .unwrap_or_default()
                        .trim()
                        .is_empty(),
                    AiProvider::GoogleVertex => !std::env::var("GOOGLE_API_KEY")
                        .unwrap_or_default()
                        .trim()
                        .is_empty(),
                    AiProvider::TogetherAi => !std::env::var("TOGETHER_API_KEY")
                        .unwrap_or_default()
                        .trim()
                        .is_empty(),
                    AiProvider::FireworksAi => !std::env::var("FIREWORKS_API_KEY")
                        .unwrap_or_default()
                        .trim()
                        .is_empty(),
                    AiProvider::Perplexity => !std::env::var("PERPLEXITY_API_KEY")
                        .unwrap_or_default()
                        .trim()
                        .is_empty(),
                    AiProvider::Cerebras => !std::env::var("CEREBRAS_API_KEY")
                        .unwrap_or_default()
                        .trim()
                        .is_empty(),
                    AiProvider::Anthropic => !std::env::var("ANTHROPIC_API_KEY")
                        .unwrap_or_default()
                        .trim()
                        .is_empty(),
                };

                if fallback_attempts < 7 && fallback_available {
                    fallback_attempts += 1;
                    let prev_provider = current_provider;
                    current_provider = fallback;
                    current_model = default_provider_model(current_provider);
                    current_profile = match current_provider {
                        AiProvider::OpenRouter => default_model_info(&current_model),
                        AiProvider::CloudflareWorkersAi => {
                            enrich_model_profile(accounts, &default_model_info(&current_model))
                        }
                        _ => default_model_info(&current_model),
                    };
                    current_thinking = thinking && current_profile.supports_thinking;
                    hard_fail_retries = 0; // fresh provider earns its own retries

                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "Provider {} unavailable/exhausted (Attempt {}/7). Seamlessly falling back to default provider {} (Model: {})...",
                            prev_provider.label(),
                            fallback_attempts,
                            current_provider.label(),
                            current_model
                        )))
                        .ok();
                    ui_tx
                        .send(AgentToUiMessage::ProviderChanged(current_provider))
                        .ok();

                    loop_count = loop_count.saturating_sub(1);
                    continue;
                }

                // No other provider to move to. Transport failures are often
                // transient (connection reset, TLS hiccup, upstream blip), so
                // retry the same request with backoff before giving up.
                if hard_fail_retries < 2 {
                    hard_fail_retries += 1;
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "Provider {} failed and no fallback provider is configured. Retrying same request in 5s (attempt {}/2)...",
                            current_provider.label(),
                            hard_fail_retries
                        )))
                        .ok();
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    loop_count = loop_count.saturating_sub(1);
                    continue;
                }

                let err_msg = match current_provider {
                    AiProvider::OpenRouter => "OpenRouter request failed across all accounts.",
                    AiProvider::CloudflareWorkersAi => {
                        "All Cloudflare Workers AI accounts exhausted or failed."
                    }
                    AiProvider::AzureOpenAi => {
                        "Azure OpenAI request failed or no accounts configured."
                    }
                    AiProvider::LocalOllama => {
                        "Local Ollama server unreachable (http://localhost:11434)."
                    }
                    AiProvider::Deepseek => "Deepseek request failed or DEEPSEEK_API_KEY missing.",
                    AiProvider::AlibabaQwen => {
                        "Alibaba Qwen request failed or DASHSCOPE_API_KEY missing."
                    }
                    AiProvider::AwsBedrock => {
                        "AWS Bedrock request failed or BEDROCK_PROXY_URL not configured."
                    }
                    AiProvider::Groq => "Groq request failed or GROQ_API_KEY missing.",
                    AiProvider::Mistral => "Mistral AI request failed or MISTRAL_API_KEY missing.",
                    AiProvider::OpenAI => "OpenAI request failed or OPENAI_API_KEY missing.",
                    AiProvider::GoogleVertex => {
                        "Google Vertex request failed or GOOGLE_API_KEY missing."
                    }
                    AiProvider::TogetherAi => {
                        "Together AI request failed or TOGETHER_API_KEY missing."
                    }
                    AiProvider::FireworksAi => {
                        "Fireworks AI request failed or FIREWORKS_API_KEY missing."
                    }
                    AiProvider::Perplexity => {
                        "Perplexity request failed or PERPLEXITY_API_KEY missing."
                    }
                    AiProvider::Cerebras => "Cerebras request failed or CEREBRAS_API_KEY missing.",
                    AiProvider::Anthropic => {
                        "Anthropic request failed or ANTHROPIC_API_KEY missing."
                    }
                };
                // Record the failure honestly in the handover so the next
                // session (and the UI) know the mission ended mid-flight
                // rather than silently "finishing".
                write_handover_nda(
                    workspace_root,
                    "blocked",
                    loop_count,
                    "provider unavailable",
                    false,
                );
                ui_tx
                    .send(AgentToUiMessage::StatusUpdate(format!(
                        "Agent BLOCKED: {} ({}) could not fulfill the request after retries.",
                        current_provider.label(),
                        current_model
                    )))
                    .ok();
                ui_tx
                    .send(AgentToUiMessage::OutputToken(format!(
                        "\n\nError: {err_msg}"
                    )))
                    .ok();
                break;
            }
        };

        let tokens_in = compressed_history
            .iter()
            .map(|m| estimate_tokens(&m.content))
            .sum::<u64>();

        let mut assistant_content = String::new();
        let mut reasoning_content = String::new();
        let mut accumulated_tools: Vec<ToolCallAccumulator> = Vec::new();
        let mut streamed_len: usize = 0;
        let mut suppressing = false;

        // Anthropic returns a non-streaming JSON response in a different
        // format than the OpenAI SSE events the stream parser expects.
        // Handle it here, then skip the stream-reading loop.
        let inner: Box<dyn std::io::Read> = if current_provider == AiProvider::Anthropic {
            let body_str = response.into_string().unwrap_or_default();
            if let Ok(parsed) = serde_json::from_str::<Value>(&body_str) {
                if let Some(content_blocks) = parsed["content"].as_array() {
                    for block in content_blocks {
                        match block["type"].as_str() {
                            Some("text") => {
                                if let Some(text) = block["text"].as_str() {
                                    assistant_content.push_str(text);
                                    ui_tx
                                        .send(AgentToUiMessage::OutputToken(sanitize_chat_token(
                                            text,
                                        )))
                                        .ok();
                                }
                            }
                            Some("thinking") => {
                                if let Some(text) = block["thinking"].as_str() {
                                    reasoning_content.push_str(text);
                                    ui_tx
                                        .send(AgentToUiMessage::ThoughtToken(text.to_string()))
                                        .ok();
                                }
                            }
                            Some("tool_use") => {
                                let name = block["name"].as_str().unwrap_or("").to_string();
                                let id = block["id"].as_str().unwrap_or("").to_string();
                                let input = block["input"].clone();
                                let arguments = serde_json::to_string(&input)
                                    .unwrap_or_else(|_| "{}".to_string());
                                accumulated_tools.push(ToolCallAccumulator {
                                    id,
                                    name,
                                    arguments,
                                });
                            }
                            _ => {}
                        }
                    }
                }
                if let Some(err) = parsed["error"]["message"].as_str() {
                    ui_tx
                        .send(AgentToUiMessage::OutputToken(format!(
                            "\n\nAnthropic error: {err}"
                        )))
                        .ok();
                }
            }
            streamed_len = assistant_content.len();
            Box::new(std::io::empty())
        } else {
            Box::new(response.into_reader())
        };

        let mut reader = std::io::BufReader::new(inner);
        let mut line_buf = String::new();

        loop {
            if apply_headless_control_messages(cancel_rx, message_history, ui_tx, progress) {
                break;
            }
            let mut interrupted = false;
            while let Ok(ui_msg) = ui_rx.try_recv() {
                match ui_msg {
                    UiToAgentMessage::CancelTask => {
                        ui_tx
                            .send(AgentToUiMessage::StatusUpdate(
                                "Task interrupted by operator.".to_string(),
                            ))
                            .ok();
                        interrupted = true;
                        break;
                    }
                    other => {
                        deferred_messages.push(other);
                    }
                }
            }
            if interrupted {
                break;
            }
            line_buf.clear();
            match reader.read_line(&mut line_buf) {
                Ok(0) => break,
                Ok(_) => {
                    let cleaned = line_buf.trim();
                    if cleaned.is_empty() {
                        continue;
                    }
                    if cleaned == "data: [DONE]" {
                        break;
                    }

                    if let Some(json_part) = cleaned.strip_prefix("data: ") {
                        if let Ok(parsed) = serde_json::from_str::<Value>(json_part) {
                            if let Some(choices) = parsed["choices"].as_array() {
                                if let Some(first_choice) = choices.first() {
                                    let delta = &first_choice["delta"];

                                    if let Some(r) = delta["reasoning_content"]
                                        .as_str()
                                        .or_else(|| delta["reasoning"].as_str())
                                    {
                                        reasoning_content.push_str(r);
                                        ui_tx
                                            .send(AgentToUiMessage::ThoughtToken(r.to_string()))
                                            .ok();
                                    }

                                    if let Some(tok) = delta["content"].as_str() {
                                        assistant_content.push_str(tok);

                                        let ac = &assistant_content;
                                        loop {
                                            if suppressing {
                                                let search = &ac[streamed_len..];
                                                let end = search
                                                    .find("</function>")
                                                    .map(|p| (p, "</function>".len()))
                                                    .or_else(|| {
                                                        search
                                                            .find("</tool_call>")
                                                            .map(|p| (p, "</tool_call>".len()))
                                                    })
                                                    .or_else(|| search.find(']').map(|p| (p, 1)));
                                                if let Some((p, mlen)) = end {
                                                    streamed_len += p + mlen;
                                                    suppressing = false;
                                                } else {
                                                    break;
                                                }
                                            } else {
                                                let search = &ac[streamed_len..];
                                                let tc1 =
                                                    search.find("<tool_call>").map(|p| (p, false));
                                                let tc2 =
                                                    search.find("[Calling tool").map(|p| (p, true));
                                                let detected = match (tc1, tc2) {
                                                    (Some(a), Some(b)) => {
                                                        Some(if a.0 <= b.0 { a } else { b })
                                                    }
                                                    (Some(a), None) => Some(a),
                                                    (None, Some(b)) => Some(b),
                                                    (None, None) => None,
                                                };
                                                if let Some((p, _is_bracket)) = detected {
                                                    let safe = &search[..p];
                                                    if !safe.is_empty() {
                                                        ui_tx
                                                            .send(AgentToUiMessage::OutputToken(
                                                                sanitize_chat_token(safe),
                                                            ))
                                                            .ok();
                                                    }
                                                    streamed_len += p;
                                                    suppressing = true;
                                                } else {
                                                    let total = ac.len();
                                                    let mut safe_end = total.saturating_sub(14);
                                                    while safe_end > streamed_len
                                                        && !ac.is_char_boundary(safe_end)
                                                    {
                                                        safe_end -= 1;
                                                    }
                                                    if safe_end > streamed_len {
                                                        let chunk = &ac[streamed_len..safe_end];
                                                        ui_tx
                                                            .send(AgentToUiMessage::OutputToken(
                                                                sanitize_chat_token(chunk),
                                                            ))
                                                            .ok();
                                                        streamed_len = safe_end;
                                                    }
                                                    break;
                                                }
                                            }
                                        }
                                    }

                                    if let Some(tool_calls) = delta["tool_calls"].as_array() {
                                        for tc in tool_calls {
                                            let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                                            while accumulated_tools.len() <= idx {
                                                accumulated_tools.push(ToolCallAccumulator {
                                                    id: String::new(),
                                                    name: String::new(),
                                                    arguments: String::new(),
                                                });
                                            }
                                            if let Some(id) = tc["id"].as_str() {
                                                accumulated_tools[idx].id.push_str(id);
                                            }
                                            if let Some(func) = tc["function"].as_object() {
                                                if let Some(n) =
                                                    func.get("name").and_then(|v| v.as_str())
                                                {
                                                    accumulated_tools[idx].name.push_str(n);
                                                }
                                                if let Some(a) =
                                                    func.get("arguments").and_then(|v| v.as_str())
                                                {
                                                    accumulated_tools[idx].arguments.push_str(a);
                                                }
                                            }
                                        }
                                    }
                                }
                            } else if let Some(content) = parsed["response"]
                                .as_str()
                                .or_else(|| parsed["output"].as_str())
                                .or_else(|| parsed["text"].as_str())
                            {
                                assistant_content.push_str(content);
                                streamed_len += content.len();
                                ui_tx
                                    .send(AgentToUiMessage::OutputToken(sanitize_chat_token(
                                        content,
                                    )))
                                    .ok();
                            }
                        }
                    }
                }
                Err(e) => {
                    ui_tx
                        .send(AgentToUiMessage::OutputToken(format!(
                            "\nError reading stream: {:?}",
                            e
                        )))
                        .ok();
                    break;
                }
            }
        }

        if !suppressing && streamed_len <= assistant_content.len() {
            let mut flush_start = streamed_len;
            while flush_start > 0 && !assistant_content.is_char_boundary(flush_start) {
                flush_start -= 1;
            }
            let tail = &assistant_content[flush_start..];
            if !tail.is_empty() {
                ui_tx
                    .send(AgentToUiMessage::OutputToken(sanitize_chat_token(tail)))
                    .ok();
            }
        }

        if accumulated_tools.is_empty() && assistant_content.contains("<tool_call>") {
            let mut clean_content = String::new();
            let mut rest = assistant_content.as_str();
            while let Some(start) = rest.find("<tool_call>") {
                clean_content.push_str(&rest[..start]);
                let after_open = &rest[start + "<tool_call>".len()..];

                let ef = after_open
                    .find("</function>")
                    .map(|p| (p, p + "</function>".len()));
                let et = after_open
                    .find("</tool_call>")
                    .map(|p| (p, p + "</tool_call>".len()));
                let en = after_open.find("<tool_call>").and_then(|p| {
                    if p > 0 {
                        Some((p, p))
                    } else {
                        None
                    }
                });
                let best = [ef, et, en]
                    .into_iter()
                    .flatten()
                    .min_by_key(|(pos, _)| *pos);

                let (block, remainder) = if let Some((end_pos, after_end)) = best {
                    (&after_open[..end_pos], &after_open[after_end..])
                } else {
                    (after_open, "")
                };

                if let Some(fname_start) = block.find("<function=") {
                    let fname_rest = &block[fname_start + "<function=".len()..];
                    let fname_end = fname_rest
                        .find('>')
                        .or_else(|| fname_rest.find('\n'))
                        .unwrap_or(fname_rest.len());
                    let fname = fname_rest[..fname_end].trim().to_string();
                    if !fname.is_empty() {
                        let mut args = serde_json::Map::new();
                        let mut param_rest = fname_rest;
                        while let Some(ps) = param_rest.find("<parameter=") {
                            let after_ps = &param_rest[ps + "<parameter=".len()..];
                            let key_end = match after_ps.find('>') {
                                Some(pos) => pos,
                                None => break,
                            };
                            let key = after_ps[..key_end].to_string();
                            let val_start = (key_end + 1).min(after_ps.len());
                            let val_end = after_ps[val_start..]
                                .find("</parameter>")
                                .map(|e| val_start + e)
                                .unwrap_or(after_ps.len());
                            let val = after_ps[val_start..val_end].trim().to_string();
                            args.insert(key, Value::String(val));
                            let next_slice_start =
                                (val_end + "</parameter>".len()).min(after_ps.len());
                            param_rest = &after_ps[next_slice_start..];
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
                if rest.is_empty() {
                    break;
                }
            }
            clean_content.push_str(rest);
            assistant_content = clean_content.trim().to_string();
        }

        if accumulated_tools.is_empty() && assistant_content.contains("[Calling tool ") {
            let marker = "[Calling tool ";
            let mut clean2 = String::new();
            let mut rest2 = assistant_content.as_str();
            while let Some(start) = rest2.find(marker) {
                clean2.push_str(&rest2[..start]);
                let after = &rest2[start + marker.len()..];
                if let Some(name_end) = after.find('\'') {
                    let raw_name = &after[..name_end];
                    let args_marker = " with arguments '";
                    if let Some(args_start_rel) = after[name_end..].find(args_marker) {
                        let args_start = name_end + args_start_rel + args_marker.len();
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
                        let consumed_in_after = args_start + args_end + "']".len();
                        rest2 = &rest2[start + marker.len() + consumed_in_after.min(after.len())..];
                    } else {
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
            let tc_json: Vec<Value> = accumulated_tools
                .iter()
                .map(|t| {
                    json!({
                        "id": t.id,
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "arguments": t.arguments
                        }
                    })
                })
                .collect();
            Some(Value::Array(tc_json))
        } else {
            None
        };

        let final_saved_content = if !reasoning_content.is_empty() {
            format!(
                "<think>\n{}\n</think>\n{}",
                reasoning_content, assistant_content
            )
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
            let tokens_out =
                estimate_tokens(&assistant_content) + estimate_tokens(&reasoning_content);
            usage_tracker.record_request(
                account.n,
                &account.label,
                &account.tier,
                tokens_in,
                tokens_out,
            );
            send_usage_update(usage_tracker, accounts, or_accounts, ui_tx);
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(format!(
                    "Using account: {} ({} req today)",
                    account.label,
                    usage_tracker
                        .build_views(accounts, or_accounts)
                        .iter()
                        .find(|v| v.n == account.n && v.label == account.label)
                        .map(|v| v.requests)
                        .unwrap_or(0)
                )))
                .ok();
        }

        if let Some(account) = used_or_account {
            let tokens_out =
                estimate_tokens(&assistant_content) + estimate_tokens(&reasoning_content);
            usage_tracker.record_or_request(
                account.n,
                &account.label,
                &account.tier,
                tokens_in,
                tokens_out,
            );
            send_usage_update(usage_tracker, accounts, or_accounts, ui_tx);
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(format!(
                    "Using OpenRouter: {} ({} req today)",
                    account.label,
                    usage_tracker
                        .build_views(accounts, or_accounts)
                        .iter()
                        .find(|v| v.label == account.label)
                        .map(|v| v.requests)
                        .unwrap_or(0)
                )))
                .ok();
        }

        if let Some(ref tcs) = final_tool_calls_value {
            let Some(tool_calls_arr) = tcs.as_array() else {
                ui_tx
                    .send(AgentToUiMessage::StatusUpdate(
                        "Unexpected tool_calls format from provider, skipping.".to_string(),
                    ))
                    .ok();
                // Fall through to break
                write_handover_nda(workspace_root, "idle", loop_count, "completed", false);
                break;
            };

            let mut pending_ids = std::collections::HashSet::new();
            let mut tool_specs = Vec::new();
            let mut truncated_calls = 0usize;

            for tc in tool_calls_arr {
                let call_id = tc["id"].as_str().unwrap_or("").to_string();
                let tool_name = tc["function"]["name"].as_str().unwrap_or("").to_string();
                let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                let arguments: Value = match serde_json::from_str(args_str) {
                    Ok(v) => v,
                    Err(_) => {
                        // The stream died mid-arguments (socket timeout,
                        // provider drop). Executing a truncated payload as `{}`
                        // produced misleading tool errors and polluted history
                        // — reject the call and tell the model exactly why.
                        truncated_calls += 1;
                        ui_tx
                            .send(AgentToUiMessage::StatusUpdate(format!(
                                "Tool call '{}' had incomplete JSON arguments (stream truncated) \u{2014} not executed.",
                                tool_name
                            )))
                            .ok();
                        message_history.push(ChatMessage {
                            role: "tool".to_string(),
                            content: format!(
                                "Error: The arguments JSON for this '{}' call was incomplete or malformed (output likely cut off mid-stream) and the call was NOT executed. Re-issue the tool call with complete arguments; if the payload is very large, split it across multiple smaller write_file/apply_diff operations.",
                                tool_name
                            ),
                            name: Some(tool_name.clone()),
                            tool_call_id: Some(call_id),
                            tool_calls: None,
                        });
                        continue;
                    }
                };

                ui_tx
                    .send(AgentToUiMessage::StatusUpdate(format!(
                        "Requesting approval for tool: {}",
                        tool_name
                    )))
                    .ok();
                ui_tx
                    .send(AgentToUiMessage::RequestToolApproval {
                        id: call_id.clone(),
                        tool_name: tool_name.clone(),
                        arguments: arguments.clone(),
                    })
                    .ok();

                pending_ids.insert(call_id.clone());
                tool_specs.push((call_id, tool_name, arguments));
            }

            if tool_specs.is_empty() && truncated_calls > 0 {
                // Every call in the batch was rejected for truncated
                // arguments. Don't end the run — record the gap honestly and
                // give the model another turn to re-issue them.
                write_handover_nda(
                    workspace_root,
                    "tool_error",
                    loop_count,
                    "truncated tool-call arguments",
                    false,
                );
                save_chatlogs_nda(workspace_root, message_history);
                continue;
            }

            let mut resolved_approvals = std::collections::HashMap::new();

            while !pending_ids.is_empty() {
                if apply_headless_control_messages(cancel_rx, message_history, ui_tx, progress) {
                    pending_ids.clear();
                    break;
                }
                if let Ok(ui_msg) = ui_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                    match ui_msg {
                        UiToAgentMessage::ApproveTool { id, arguments } => {
                            if pending_ids.remove(&id) {
                                resolved_approvals.insert(id, Some(arguments));
                            }
                        }
                        UiToAgentMessage::RejectTool { id } => {
                            if pending_ids.remove(&id) {
                                resolved_approvals.insert(id, None);
                            }
                        }
                        UiToAgentMessage::CancelTask => {
                            pending_ids.clear();
                            ui_tx
                                .send(AgentToUiMessage::StatusUpdate(
                                    "Task interrupted by operator.".to_string(),
                                ))
                                .ok();
                            break;
                        }
                        other => {
                            deferred_messages.push(other);
                        }
                    }
                } else if apply_headless_control_messages(
                    cancel_rx,
                    message_history,
                    ui_tx,
                    progress,
                ) {
                    pending_ids.clear();
                    break;
                }
            }

            // Phase 2: Checkpoint before file-modifying tool batch
            let file_modifying = ["write_file", "delete_file", "run_command", "apply_diff"];
            let has_file_mod = tool_specs
                .iter()
                .any(|(_, name, _)| file_modifying.contains(&name.as_str()));
            if has_file_mod && checkpoint_mgr.enabled {
                let label = format!("before tool batch (loop {})", loop_count);
                if let Some(cp_id) = checkpoint_mgr.checkpoint(&label) {
                    last_checkpoint_id = Some(cp_id);
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "Checkpoint #{} created: {}",
                            cp_id, label
                        )))
                        .ok();
                }
            }

            let mut handles = Vec::new();
            let bus_clone = coordination_bus.clone();
            let lsp_written_clone = lsp_written_files.clone();

            for (call_id, tool_name, _original_arguments) in tool_specs {
                let approval = resolved_approvals.get(&call_id).cloned().flatten();
                let workspace_root_clone = workspace_root.clone();
                let ui_tx_clone = ui_tx.clone();
                let bus = bus_clone.clone();
                let lsp_written = lsp_written_clone.clone();

                let handle = std::thread::spawn(move || {
                    let (tool_result, file_buffer_update, changelog_entry) = if let Some(
                        approved_args,
                    ) = approval
                    {
                        ui_tx_clone
                            .send(AgentToUiMessage::ToolExecutionStarted {
                                tool_name: tool_name.clone(),
                            })
                            .ok();

                        // T1b: Claim file via coordination bus before writing
                        let file_to_lock: Option<PathBuf> = match tool_name.as_str() {
                            "write_file" | "apply_diff" | "delete_file" => approved_args
                                ["relativeFilePath"]
                                .as_str()
                                .or_else(|| approved_args["path"].as_str())
                                .map(|p| workspace_root_clone.join(p)),
                            _ => None,
                        };
                        if let Some(ref lock_path) = file_to_lock {
                            if !bus.claim_file("primary", lock_path) {
                                return (
                                    call_id,
                                    tool_name,
                                    format!(
                                        "Error: File '{}' is locked by another agent.",
                                        lock_path.display()
                                    ),
                                    None,
                                    None,
                                );
                            }
                        }

                        // T2b: LSP gate — if this file was already written this session
                        // and build diagnostics still report errors referencing it,
                        // block the write to force the agent to fix errors first.
                        if let Some(ref lock_path) = file_to_lock {
                            if lsp_written.lock_safe().contains(lock_path) {
                                let diag = crate::automation::read_latest_diagnostics(
                                    &workspace_root_clone,
                                );
                                if !diag.success {
                                    let file_str = lock_path.display().to_string();
                                    let rel_str = lock_path
                                        .strip_prefix(&workspace_root_clone)
                                        .map(|p| p.display().to_string())
                                        .unwrap_or_default();
                                    let has_file_errors = diag
                                        .errors
                                        .iter()
                                        .any(|e| e.contains(&file_str) || e.contains(&rel_str));
                                    if has_file_errors {
                                        let relevant: Vec<&String> = diag
                                            .errors
                                            .iter()
                                            .filter(|e| {
                                                e.contains(&file_str) || e.contains(&rel_str)
                                            })
                                            .take(5)
                                            .collect();
                                        return (
                                                call_id,
                                                tool_name,
                                                format!(
                                                    "BLOCKED: '{}' has unresolved compilation errors from your previous write. \
                                                    Fix these errors before writing again:\n{}",
                                                    rel_str,
                                                    relevant.iter().map(|e| format!("  {}", e)).collect::<Vec<_>>().join("\n")
                                                ),
                                                None,
                                                None,
                                            );
                                    }
                                }
                            }
                        }

                        let mut file_buffer_update = None;
                        let mut changelog_entry = None;
                        let tool_result = match registry::call_tool_in_workspace(
                            &workspace_root_clone,
                            &tool_name,
                            &approved_args,
                        ) {
                            Ok(res) => {
                                if tool_name == "write_file" {
                                    if let Some(rel_path) =
                                        approved_args["relativeFilePath"].as_str()
                                    {
                                        let full_path = workspace_root_clone.join(rel_path);
                                        if let Some(content) = approved_args["content"].as_str() {
                                            file_buffer_update =
                                                Some((full_path, content.to_string()));
                                        }
                                        changelog_entry =
                                            Some((rel_path.to_string(), "write_file"));
                                    }
                                }
                                res
                            }
                            Err(e) => format!("Error executing tool: {:?}", e),
                        };

                        // T1b: Release file lock after execution
                        if let Some(ref lock_path) = file_to_lock {
                            bus.release_file("primary", lock_path);
                            // T2b: Track written files for LSP gating
                            if matches!(tool_name.as_str(), "write_file" | "apply_diff") {
                                lsp_written.lock_safe().insert(lock_path.clone());
                            }
                        }

                        ui_tx_clone
                            .send(AgentToUiMessage::ToolExecutionFinished {
                                tool_name: tool_name.clone(),
                                result: tool_result.clone(),
                            })
                            .ok();

                        (tool_result, file_buffer_update, changelog_entry)
                    } else {
                        (
                            "Error: Tool execution rejected by the user.".to_string(),
                            None,
                            None,
                        )
                    };

                    (
                        call_id,
                        tool_name,
                        tool_result,
                        file_buffer_update,
                        changelog_entry,
                    )
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

            for (call_id, tool_name, tool_result, file_buffer_update, changelog_entry) in
                thread_results
            {
                if tool_result.contains("Error executing tool")
                    || tool_result.starts_with("BLOCKED:")
                {
                    any_error = true;
                    // Phase 3: Penalize failed strategy
                    let mem_key = format!("tool:{}:error", tool_name);
                    memory.reinforce(&mem_key, -0.1);
                    // Phase 4: Record failure for self-improvement analysis
                    improve_engine.record_failure(&tool_name, &tool_result, loop_count);
                } else if tool_result.contains("rejected by the user") {
                    any_rejected = true;
                    improve_engine.record_failure(&tool_name, "rejected by the user", loop_count);
                } else {
                    any_success = true;
                    // Phase 3: Remember successful tool usage
                    let mem_key = format!("tool:{}:success", tool_name);
                    let summary = if tool_result.len() > 200 {
                        &tool_result[..200]
                    } else {
                        &tool_result
                    };
                    memory.remember(
                        &mem_key,
                        &format!("{} -> {}", tool_name, summary),
                        &["tool", "success"],
                        0.8,
                    );
                    // Phase 4: Record success for ratio tracking
                    improve_engine.record_success();
                }

                if let Some((path, content)) = file_buffer_update {
                    ui_tx
                        .send(AgentToUiMessage::UpdateFileBuffer { path, content })
                        .ok();
                }
                if let Some((rel_path, action)) = changelog_entry {
                    append_changelog_nda(workspace_root, &rel_path, action);
                    sitemap_needed = true;
                }

                message_history.push(ChatMessage {
                    role: "tool".to_string(),
                    content: tool_result,
                    name: Some(tool_name.clone()),
                    tool_call_id: Some(call_id),
                    tool_calls: None,
                });
            }

            if any_error {
                // Phase 2: a batch that produced errors and no successes left the
                // workspace in a bad state (e.g. a failed run_command with partial
                // side effects). Roll back to the pre-batch checkpoint so broken
                // changes don't accumulate across loops.
                if !any_success {
                    if let Some(cp_id) = last_checkpoint_id.take() {
                        match checkpoint_mgr.restore(cp_id) {
                            Ok(()) => {
                                ui_tx
                                    .send(AgentToUiMessage::StatusUpdate(format!(
                                        "Batch failed \u{2014} rolled back to checkpoint #{}.",
                                        cp_id
                                    )))
                                    .ok();
                            }
                            Err(e) => {
                                ui_tx
                                    .send(AgentToUiMessage::StatusUpdate(format!(
                                        "Rollback to checkpoint #{} failed: {}",
                                        cp_id, e
                                    )))
                                    .ok();
                            }
                        }
                    }
                }
                write_handover_nda(
                    workspace_root,
                    "tool_error",
                    loop_count,
                    "tool error in batch",
                    false,
                );
            } else if any_rejected {
                write_handover_nda(
                    workspace_root,
                    "tool_rejected",
                    loop_count,
                    "user reject in batch",
                    false,
                );
            } else if any_success {
                write_handover_nda(workspace_root, "executing", loop_count, "batch ok", false);
            }

            save_chatlogs_nda(workspace_root, message_history);
        } else if sitemap_needed {
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(
                    "Running automatic compilation validation...".to_string(),
                ))
                .ok();
            match run_compilation_check(workspace_root) {
                Ok(()) => {
                    write_handover_nda(
                        workspace_root,
                        "idle",
                        loop_count,
                        "compiler validated",
                        false,
                    );
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(
                            "Compiler validation succeeded!".to_string(),
                        ))
                        .ok();
                    break;
                }
                Err(e) => {
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!("Build status: {e}")))
                        .ok();
                    break;
                }
            }
        } else if unfinished_announced_turn(&assistant_content) && auto_continuations < 2 {
            // The model ended its turn with an announcement ("Let me fix the
            // test:") or an empty response instead of a tool call or a final
            // summary. Historically this surfaced as "Agent finished" with the
            // mission half-done. Nudge it to act, without user intervention.
            auto_continuations += 1;
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(format!(
                    "Turn announced an action but made no tool call — auto-continuing ({}/2)...",
                    auto_continuations
                )))
                .ok();
            message_history.push(ChatMessage {
                role: "user".to_string(),
                content: "[System notice] Your previous response announced an action but contained no tool call, so nothing was executed and the turn ended. If work remains, make the tool calls now; if the mission is truly complete, reply with your final summary instead of another announcement.".to_string(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            });
            loop_count = loop_count.saturating_sub(1);
            continue;
        } else {
            write_handover_nda(workspace_root, "idle", loop_count, "completed", false);
            break;
        }
    }

    save_chatlogs_nda(workspace_root, message_history);

    // Phase 4: Run self-improvement analysis and persist learnings
    if improve_engine.has_data() {
        improve_engine.persist_to_memory(&mut memory);
        let failure_count = improve_engine.failure_count();
        if let Some(addendum) = improve_engine.generate_prompt_addendum(0.3) {
            // Store the generated addendum for next session's system prompt
            memory.remember(
                "self_improve:prompt_addendum",
                &addendum,
                &["self_improve", "prompt"],
                0.85,
            );
        }
        ui_tx
            .send(AgentToUiMessage::StatusUpdate(format!(
                "Self-improvement: analyzed {} failure(s), updated strategy memory.",
                failure_count
            )))
            .ok();
    }

    // Phase 3: Persist memory to disk
    let _ = memory.save();

    // Phase 2: Report checkpoint status, then tear down the session's snapshots.
    let cp_count = checkpoint_mgr.count();
    if cp_count > 0 {
        let last_id = checkpoint_mgr.list().last().map(|c| c.id);
        let diff_summary = last_id
            .and_then(|id| checkpoint_mgr.diff_since(id).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let msg = match diff_summary {
            Some(diff) => format!(
                "Session complete. {} checkpoint(s) taken; changes since the last one:\n{}",
                cp_count, diff
            ),
            None => format!(
                "Session complete. {} checkpoint(s) taken; no net changes since the last one.",
                cp_count
            ),
        };
        ui_tx.send(AgentToUiMessage::StatusUpdate(msg)).ok();
        // Snapshots are dangling git commits reclaimed by gc; drop our tracking.
        checkpoint_mgr.cleanup();
    }

    if sitemap_needed {
        write_sitemap_nda(workspace_root);
    }
    convert_jsonl_to_nda(workspace_root);
    ui_tx
        .send(AgentToUiMessage::StatusUpdate(
            "Agent workflow finished. Idling.".to_string(),
        ))
        .ok();
    ui_tx.send(AgentToUiMessage::AgentFinished).ok();
}

// ─── Workspace Context Gathering for MoA Router ─────────────────────────────

/// Extract file paths mentioned in the user's prompt and read their contents
/// to provide context to the MoA specialists.
///
/// Returns (file_paths, context_string) where context_string includes the
/// actual file contents so the models can analyze real code.
fn gather_workspace_context(workspace_root: &PathBuf, prompt: &str) -> (Vec<String>, String) {
    let mut file_paths = Vec::new();
    let mut context_parts = Vec::new();

    // Common source file extensions to look for
    let source_extensions = [
        ".rs", ".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".java", ".c", ".cpp", ".h", ".hpp",
        ".css", ".scss", ".html", ".vue", ".svelte", ".toml", ".json", ".yaml", ".yml", ".md",
    ];

    // Extract file paths from the prompt using simple pattern matching
    for word in prompt.split_whitespace() {
        let cleaned = word.trim_matches(|c: char| {
            !c.is_alphanumeric() && c != '.' && c != '/' && c != '\\' && c != '-' && c != '_'
        });

        // Check if it looks like a file path
        if cleaned.contains('.') || cleaned.contains('/') || cleaned.contains('\\') {
            let has_extension = source_extensions.iter().any(|ext| cleaned.ends_with(ext));

            if has_extension {
                // Try to resolve the path relative to workspace root
                let path = if std::path::Path::new(cleaned).is_absolute() {
                    std::path::PathBuf::from(cleaned)
                } else {
                    workspace_root.join(cleaned)
                };

                // Read the file if it exists and is reasonably sized (< 100KB)
                if path.exists() && path.is_file() {
                    if let Ok(metadata) = path.metadata() {
                        if metadata.len() < 100_000 {
                            if let Ok(contents) = std::fs::read_to_string(&path) {
                                let relative_path = path
                                    .strip_prefix(workspace_root)
                                    .unwrap_or(&path)
                                    .to_string_lossy()
                                    .to_string();

                                file_paths.push(relative_path.clone());
                                context_parts
                                    .push(format!("=== {} ===\n{}\n", relative_path, contents));
                            }
                        }
                    }
                }
            }
        }
    }

    // Also gather a high-level overview of the workspace structure
    let workspace_overview = gather_workspace_overview(workspace_root);
    if !workspace_overview.is_empty() {
        context_parts.insert(
            0,
            format!("=== Workspace Structure ===\n{}\n", workspace_overview),
        );
    }

    let context = context_parts.join("\n");
    (file_paths, context)
}

/// Gather a high-level overview of the workspace structure (top-level directories
/// and key files) to give the MoA specialists context about the project.
fn gather_workspace_overview(workspace_root: &PathBuf) -> String {
    let mut overview = String::new();

    // List top-level directories
    if let Ok(entries) = std::fs::read_dir(workspace_root) {
        let mut dirs = Vec::new();
        let mut files = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden directories and common non-source directories
            if name.starts_with('.') || name == "target" || name == "node_modules" || name == "dist"
            {
                continue;
            }

            if path.is_dir() {
                dirs.push(name);
            } else if path.is_file() {
                // Only include key config files
                if [
                    "Cargo.toml",
                    "package.json",
                    "README.md",
                    "Cargo.lock",
                    "tsconfig.json",
                ]
                .contains(&name.as_str())
                {
                    files.push(name);
                }
            }
        }

        dirs.sort();
        files.sort();

        if !dirs.is_empty() {
            overview.push_str(&format!("Directories: {}\n", dirs.join(", ")));
        }
        if !files.is_empty() {
            overview.push_str(&format!("Key files: {}\n", files.join(", ")));
        }
    }

    // Read Cargo.toml or package.json for project metadata
    let cargo_toml = workspace_root.join("Cargo.toml");
    if cargo_toml.exists() {
        if let Ok(contents) = std::fs::read_to_string(&cargo_toml) {
            // Extract just the [package] section
            if let Some(start) = contents.find("[package]") {
                if let Some(end) = contents[start..].find("\n[").map(|p| start + p) {
                    overview.push_str(&format!(
                        "\n=== Cargo.toml [package] ===\n{}\n",
                        &contents[start..end]
                    ));
                }
            }
        }
    }

    overview
}

/// Returns true when a no-tool-call assistant turn looks unfinished rather
/// than like a final summary: an empty response (stream returned nothing), a
/// trailing colon/ellipsis ("Let me fix the test:"), or a last line that is
/// itself an action announcement. The WASIX POC run ended twice this way —
/// "Agent finished" while the mission was half-done.
fn unfinished_announced_turn(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed.ends_with(':')
        || trimmed.ends_with('：')
        || trimmed.ends_with("...")
        || trimmed.ends_with('…')
    {
        return true;
    }
    if let Some(last_line) = trimmed.lines().rev().find(|l| !l.trim().is_empty()) {
        let lower = last_line.trim_start().to_lowercase();
        // "Let me know …" closings are normal final-summary prose, not intent.
        if lower.starts_with("let me know") || lower.starts_with("let us know") {
            return false;
        }
        const ANNOUNCEMENTS: [&str; 8] = [
            "let me ",
            "let's ",
            "i'll ",
            "i will ",
            "i am going to ",
            "i'm going to ",
            "now i ",
            "next, i ",
        ];
        if ANNOUNCEMENTS.iter().any(|p| lower.starts_with(p)) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::unfinished_announced_turn;

    #[test]
    fn empty_response_is_unfinished() {
        assert!(unfinished_announced_turn(""));
        assert!(unfinished_announced_turn("   \n "));
    }

    #[test]
    fn trailing_colon_or_ellipsis_is_unfinished() {
        assert!(unfinished_announced_turn("I see the issue. Let me fix the test to use unique data:"));
        assert!(unfinished_announced_turn("Next steps…"));
        assert!(unfinished_announced_turn("Running the tests now..."));
        assert!(unfinished_announced_turn("修改说明："));
    }

    #[test]
    fn announcement_last_line_is_unfinished() {
        assert!(unfinished_announced_turn(
            "Some analysis here.\n\nLet me write the benchmarks now."
        ));
        assert!(unfinished_announced_turn("I'll add the tests next."));
        assert!(unfinished_announced_turn("Now I run cargo test."));
    }

    #[test]
    fn genuine_final_summaries_are_finished() {
        assert!(!unfinished_announced_turn(
            "All 14 tests pass and the benchmarks show 3.2 µs per read. The implementation is complete."
        ));
        assert!(!unfinished_announced_turn(
            "Summary: fork shares the offset, close is safe, reads serialize.\n\nLet me know if you want a follow-up."
        ));
        assert!(!unfinished_announced_turn("Done. The crate builds and all tests pass."));
    }
}
