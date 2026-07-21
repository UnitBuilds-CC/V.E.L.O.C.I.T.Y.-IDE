use super::models::*;
use super::nda::*;
use super::provider::*;
use crate::registry;
use crate::usage::{
    load_accounts_from_env, load_openrouter_accounts_from_env, CloudflareAccount,
    OpenRouterAccount, UsageTracker,
};
use crossbeam_channel::{Receiver, Sender};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::io::BufRead;

pub fn send_usage_update(
    tracker: &mut UsageTracker,
    accounts: &[CloudflareAccount],
    or_accounts: &[OpenRouterAccount],
    ui_tx: &Sender<AgentToUiMessage>,
) {
    let views = tracker.build_views(accounts, or_accounts);
    let date = tracker.current_date();
    ui_tx
        .send(AgentToUiMessage::AccountUsage {
            accounts: views,
            date,
        })
        .ok();
}

pub fn is_quota_exhausted_error(body: &str) -> bool {
    body.contains("4006") || body.to_lowercase().contains("quota")
}

pub fn estimate_tokens(text: &str) -> u64 {
    (text.len() as u64).max(1) / 4
}

pub fn render_prompt(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .map(|message| format!("{}: {}", message.role, message.content))
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn build_inline_tool_docs() -> String {
    use crate::registry::get_tools;
    let tools = get_tools();
    let mut doc = String::from(
        "\n\n## Available Tools\n\
        Call tools using this exact syntax (one block per call):\n\
        <tool_call>\n\
        <function=TOOL_NAME>\n\
        <parameter=PARAM_NAME>VALUE</parameter>\n\
        </function>\n\n",
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
                let req = if required.contains(&param.as_str()) {
                    " (required)"
                } else {
                    " (optional)"
                };
                doc.push_str(&format!("  - `{}`{}: {}\n", param, req, desc));
            }
        }
        doc.push('\n');
    }
    doc.push_str(
        "Always call exactly one tool per <tool_call> block. \
        Wait for the tool result before continuing.\n",
    );
    doc
}

pub fn build_request(
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

pub fn strip_think_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        if let Some(end) = rest[start..].find("</think>") {
            rest = &rest[start + end + "</think>".len()..];
        } else {
            break;
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

pub fn compress_history(messages: &[ChatMessage], supports_tools: bool) -> Vec<ChatMessage> {
    const VALID_ROLES: &[&str] = &[
        "system",
        "user",
        "assistant",
        "tool",
        "function",
        "developer",
    ];
    let messages: Vec<ChatMessage> = messages.iter().filter_map(|m| {
        let trimmed_content = m.content.trim();
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
            if !m.content.trim().is_empty() {
                let mut fixed = m.clone();
                fixed.role = "assistant".to_string();
                Some(fixed)
            } else {
                None
            }
        } else if !VALID_ROLES.contains(&m.role.as_str()) {
            let mut fixed = m.clone();
            fixed.role = "user".to_string();
            Some(fixed)
        } else {
            Some(m.clone())
        }
    }).collect();

    let mut messages = messages;
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

    let mut compressed: Vec<ChatMessage> = Vec::new();
    for (idx, m) in messages.iter().enumerate() {
        let mut m_copy = m.clone();
        if m_copy.role == "assistant" {
            m_copy.content = strip_think_tags(&m_copy.content);
        }

        if m.role == "tool" {
            let has_subsequent_assistant_msg = messages[idx + 1..]
                .iter()
                .any(|msg| msg.role == "assistant");
            if has_subsequent_assistant_msg && m_copy.content.len() > 1000 {
                let tool_name = m_copy
                    .name
                    .clone()
                    .unwrap_or_else(|| "unknown_tool".to_string());
                let content_len = m_copy.content.len();
                let content_hash = hash_str(&m_copy.content);

                let mut decls = Vec::new();
                if tool_name == "read_file"
                    || m_copy.content.contains("fn ")
                    || m_copy.content.contains("class ")
                {
                    for line in m_copy.content.lines() {
                        let line = line.trim();
                        if line.contains("fn ")
                            || line.contains("void ")
                            || line.contains("def ")
                            || line.contains("class ")
                        {
                            let parts: Vec<&str> = line.split_whitespace().collect();
                            for (i, &word) in parts.iter().enumerate() {
                                if word == "fn"
                                    || word == "def"
                                    || word == "class"
                                    || word == "void"
                                {
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
                let tool_name = m_copy
                    .name
                    .clone()
                    .unwrap_or_else(|| "unknown_tool".to_string());
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
                            desc.push_str(&format!(
                                "[Calling tool '{}' with arguments '{}']",
                                name, args
                            ));
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

        if m_copy.role == "assistant"
            && m_copy.content.trim().is_empty()
            && m_copy.tool_calls.is_none()
        {
            continue;
        }

        compressed.push(m_copy);
    }

    const BUDGET: usize = 60_000;
    let system: Vec<ChatMessage> = compressed
        .iter()
        .filter(|m| m.role == "system")
        .cloned()
        .collect();
    let non_system: Vec<ChatMessage> = compressed
        .into_iter()
        .filter(|m| m.role != "system")
        .collect();

    let system_chars: usize = system.iter().map(|m| m.content.len()).sum();
    let remaining_budget = BUDGET.saturating_sub(system_chars);

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
    let mut thinking = std::env::var("CF_THINKING")
        .map(|v| v != "0")
        .unwrap_or(false);
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
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(
                    "Loaded previous chat session context.".to_string(),
                ))
                .ok();
            let restored: Vec<(String, String)> = history
                .iter()
                .filter(|m| m.role == "user" || m.role == "assistant")
                .map(|m| (m.role.clone(), m.content.clone()))
                .collect();
            if !restored.is_empty() {
                ui_tx
                    .send(AgentToUiMessage::ChatHistoryRestored(restored))
                    .ok();
            }
            history
        }
        None => {
            let use_inline_tools =
                provider == AiProvider::OpenRouter || !selected_profile.supports_tools;
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

    write_sitemap_nda(&workspace_root);

    ui_tx
        .send(AgentToUiMessage::StatusUpdate(
            "Agent thread initialized and idling.".to_string(),
        ))
        .ok();
    ui_tx.send(AgentToUiMessage::ProviderChanged(provider)).ok();
    ui_tx
        .send(AgentToUiMessage::ModelCatalog {
            models: model_catalog.clone(),
            selected: model.clone(),
            thinking,
        })
        .ok();

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
                            model = model_catalog
                                .first()
                                .map(|candidate| candidate.id.clone())
                                .unwrap_or(model);
                        }
                        selected_profile = model_catalog
                            .iter()
                            .find(|candidate| candidate.id == model)
                            .cloned()
                            .unwrap_or_else(|| default_model_info(&model));
                        if provider == AiProvider::CloudflareWorkersAi {
                            selected_profile = enrich_model_profile(&accounts, &selected_profile);
                        }
                        if !selected_profile.supports_thinking {
                            thinking = false;
                        }
                        ui_tx
                            .send(AgentToUiMessage::ModelCatalog {
                                models: model_catalog.clone(),
                                selected: model.clone(),
                                thinking,
                            })
                            .ok();
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
                    selected_profile = model_catalog
                        .iter()
                        .find(|candidate| candidate.id == model)
                        .cloned()
                        .unwrap_or_else(|| default_model_info(&model));
                    selected_profile = enrich_model_profile(&accounts, &selected_profile);
                    if let Some(entry) = model_catalog
                        .iter_mut()
                        .find(|candidate| candidate.id == model)
                    {
                        *entry = selected_profile.clone();
                    }
                    if !selected_profile.supports_thinking {
                        thinking = false;
                    }
                    ui_tx
                        .send(AgentToUiMessage::ModelCatalog {
                            models: model_catalog.clone(),
                            selected: model.clone(),
                            thinking,
                        })
                        .ok();
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "Model set to {model}"
                        )))
                        .ok();
                }
            }
            UiToAgentMessage::SetThinking(enabled) => {
                thinking = enabled && selected_profile.supports_thinking;
                ui_tx
                    .send(AgentToUiMessage::StatusUpdate(
                        if thinking {
                            "Thinking enabled"
                        } else {
                            "Thinking disabled"
                        }
                        .to_string(),
                    ))
                    .ok();
            }
            UiToAgentMessage::SetProvider(new_provider) => {
                provider = new_provider;
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
                selected_profile = model_catalog
                    .first()
                    .cloned()
                    .unwrap_or_else(|| default_model_info(&model));
                thinking = thinking && selected_profile.supports_thinking;
                ui_tx.send(AgentToUiMessage::ProviderChanged(provider)).ok();
                ui_tx
                    .send(AgentToUiMessage::ModelCatalog {
                        models: model_catalog.clone(),
                        selected: model.clone(),
                        thinking,
                    })
                    .ok();
                ui_tx
                    .send(AgentToUiMessage::StatusUpdate(format!(
                        "Provider switched to {}",
                        provider.label()
                    )))
                    .ok();
                let _ = ui_tx.send(AgentToUiMessage::StatusUpdate(format!(
                    "Fetching {} model catalog…",
                    provider.label()
                )));
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
                        ui_tx
                            .send(AgentToUiMessage::ChatHistoryRestored(restored))
                            .ok();
                    }
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(
                            "Agent workspace switched.".to_string(),
                        ))
                        .ok();
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
                    None,
                    None,
                    &ui_tx,
                );
            }
            UiToAgentMessage::RunLocalBuild => {
                ui_tx
                    .send(AgentToUiMessage::StatusUpdate(
                        "Running local cargo check...".to_string(),
                    ))
                    .ok();
                ui_tx
                    .send(AgentToUiMessage::OutputToken(
                        "\n$ cargo check (Local)\n".to_string(),
                    ))
                    .ok();

                let output = std::process::Command::new("cargo")
                    .arg("check")
                    .current_dir(&workspace_root)
                    .output();

                match output {
                    Ok(out) => {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        ui_tx
                            .send(AgentToUiMessage::OutputToken(stdout.into_owned()))
                            .ok();
                        ui_tx
                            .send(AgentToUiMessage::OutputToken(stderr.into_owned()))
                            .ok();

                        if out.status.success() {
                            ui_tx
                                .send(AgentToUiMessage::StatusUpdate(
                                    "Local build succeeded!".to_string(),
                                ))
                                .ok();
                        } else {
                            ui_tx
                                .send(AgentToUiMessage::StatusUpdate(
                                    "Local build failed!".to_string(),
                                ))
                                .ok();
                        }
                    }
                    Err(e) => {
                        ui_tx
                            .send(AgentToUiMessage::OutputToken(format!(
                                "Failed to run build: {:?}",
                                e
                            )))
                            .ok();
                        ui_tx
                            .send(AgentToUiMessage::StatusUpdate(
                                "Local build failed to launch".to_string(),
                            ))
                            .ok();
                    }
                }

                let _ = run_compilation_check(&workspace_root);
                ui_tx.send(AgentToUiMessage::AgentFinished).ok();
            }
            UiToAgentMessage::RunLocalRun => {
                ui_tx
                    .send(AgentToUiMessage::StatusUpdate(
                        "Running local cargo run...".to_string(),
                    ))
                    .ok();
                ui_tx
                    .send(AgentToUiMessage::OutputToken(
                        "\n$ cargo run (Local)\n".to_string(),
                    ))
                    .ok();

                let output = std::process::Command::new("cargo")
                    .arg("run")
                    .current_dir(&workspace_root)
                    .output();

                match output {
                    Ok(out) => {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        ui_tx
                            .send(AgentToUiMessage::OutputToken(stdout.into_owned()))
                            .ok();
                        ui_tx
                            .send(AgentToUiMessage::OutputToken(stderr.into_owned()))
                            .ok();

                        if out.status.success() {
                            ui_tx
                                .send(AgentToUiMessage::StatusUpdate(
                                    "Local run finished successfully!".to_string(),
                                ))
                                .ok();
                        } else {
                            ui_tx
                                .send(AgentToUiMessage::StatusUpdate(
                                    "Local run exited with error!".to_string(),
                                ))
                                .ok();
                        }
                    }
                    Err(e) => {
                        ui_tx
                            .send(AgentToUiMessage::OutputToken(format!(
                                "Failed to run executable: {:?}",
                                e
                            )))
                            .ok();
                        ui_tx
                            .send(AgentToUiMessage::StatusUpdate(
                                "Local run failed to launch".to_string(),
                            ))
                            .ok();
                    }
                }

                ui_tx.send(AgentToUiMessage::AgentFinished).ok();
            }
            _ => {}
        }
    }
}

pub fn run_compilation_check(workspace_root: &std::path::Path) -> Result<(), String> {
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
                    if trimmed.contains("error[E")
                        || trimmed.contains("error:")
                        || trimmed.starts_with("--> src/")
                    {
                        errors.push(trimmed.to_string());
                    }
                }
                if errors.is_empty() {
                    let lines: Vec<&str> = stderr.lines().collect();
                    let start = lines.len().saturating_sub(10);
                    return Err(lines[start..]
                        .iter()
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>()
                        .join("\n"));
                }
                return Err(errors.join("\n"));
            }
            Ok(())
        }
        Err(e) => Err(format!("Failed to execute cargo check: {:?}", e)),
    }
}

pub fn apply_headless_control_messages(
    control_rx: Option<&Receiver<UiToAgentMessage>>,
    message_history: &mut Vec<ChatMessage>,
    ui_tx: &Sender<AgentToUiMessage>,
    progress: Option<&Arc<Mutex<HeadlessSubAgentProgress>>>,
) -> bool {
    let Some(control_rx) = control_rx else {
        return false;
    };
    loop {
        match control_rx.try_recv() {
            Ok(UiToAgentMessage::CancelTask) => {
                ui_tx
                    .send(AgentToUiMessage::StatusUpdate(
                        "Headless sub-agent cancelled by operator.".to_string(),
                    ))
                    .ok();
                return true;
            }
            Ok(UiToAgentMessage::UserPrompt(note)) => {
                let note = note.trim();
                if note.is_empty() {
                    continue;
                }
                let prompt = format!(
                    "Operator intervention for this routed task. Treat it as the highest-priority steering update and continue the existing assignment unless it explicitly changes scope.\n\n{}",
                    note
                );
                message_history.push(ChatMessage {
                    role: "user".to_string(),
                    content: prompt,
                    name: None,
                    tool_call_id: None,
                    tool_calls: None,
                });
                if let Some(progress) = progress {
                    let mut guard = progress.lock().unwrap();
                    guard.operator_notes.push(note.to_string());
                    guard
                        .status_updates
                        .push("Operator note routed to this worker thread.".to_string());
                    guard.events.push(HeadlessSubAgentEvent {
                        kind: HeadlessSubAgentEventKind::OperatorNote,
                        message: note.to_string(),
                    });
                    guard.events.push(HeadlessSubAgentEvent {
                        kind: HeadlessSubAgentEventKind::Status,
                        message: "Operator note routed to this worker thread.".to_string(),
                    });
                }
                ui_tx
                    .send(AgentToUiMessage::StatusUpdate(
                        "Operator note routed to this worker thread.".to_string(),
                    ))
                    .ok();
            }
            Ok(_) => {}
            Err(crossbeam_channel::TryRecvError::Empty) => return false,
            Err(crossbeam_channel::TryRecvError::Disconnected) => return false,
        }
    }
}

pub fn run_headless_subagent(request: HeadlessSubAgentRequest) -> HeadlessSubAgentResult {
    let accounts = load_accounts_from_env();
    let or_accounts = load_openrouter_accounts_from_env();
    let mut usage_tracker = UsageTracker::new(&request.workspace_root);
    let selected_profile = match request.provider {
        AiProvider::OpenRouter => ModelInfo {
            id: request.model.clone(),
            label: request
                .model
                .rsplit('/')
                .next()
                .unwrap_or(&request.model)
                .to_string(),
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

    let use_inline_tools =
        request.provider == AiProvider::OpenRouter || !selected_profile.supports_tools;
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
    let cancel_rx = request.cancel_rx;
    let progress = request.progress;
    let status_updates = Arc::new(Mutex::new(Vec::new()));
    let transcript = Arc::new(Mutex::new(String::new()));
    let changed_files = Arc::new(Mutex::new(Vec::new()));

    let status_updates_collector = status_updates.clone();
    let transcript_collector = transcript.clone();
    let changed_files_collector = changed_files.clone();
    let progress_collector = progress.clone();
    let auto_approve_tx = agent_ui_tx.clone();

    let collector = std::thread::spawn(move || {
        while let Ok(msg) = agent_event_rx.recv() {
            match msg {
                AgentToUiMessage::StatusUpdate(status) => {
                    status_updates_collector
                        .lock()
                        .unwrap()
                        .push(status.clone());
                    if let Some(progress) = &progress_collector {
                        let mut progress = progress.lock().unwrap();
                        progress.status_updates.push(status.clone());
                        progress.events.push(HeadlessSubAgentEvent {
                            kind: HeadlessSubAgentEventKind::Status,
                            message: status,
                        });
                    }
                }
                AgentToUiMessage::OutputToken(token) | AgentToUiMessage::ThoughtToken(token) => {
                    transcript_collector.lock().unwrap().push_str(&token);
                    if let Some(progress) = &progress_collector {
                        let mut progress = progress.lock().unwrap();
                        progress.transcript.push_str(&token);
                        progress.events.push(HeadlessSubAgentEvent {
                            kind: HeadlessSubAgentEventKind::Transcript,
                            message: token,
                        });
                    }
                }
                AgentToUiMessage::UpdateFileBuffer { path, .. } => {
                    let mut guard = changed_files_collector.lock().unwrap();
                    if !guard.contains(&path) {
                        guard.push(path.clone());
                    }
                    if let Some(progress) = &progress_collector {
                        let changed_path = path.display().to_string();
                        let mut progress = progress.lock().unwrap();
                        if !progress.changed_files.contains(&changed_path) {
                            progress.changed_files.push(changed_path.clone());
                        }
                        progress.events.push(HeadlessSubAgentEvent {
                            kind: HeadlessSubAgentEventKind::FileChange,
                            message: changed_path,
                        });
                    }
                }
                AgentToUiMessage::RequestToolApproval {
                    id,
                    tool_name,
                    arguments,
                } => {
                    let status = format!("Auto-approving tool: {tool_name}");
                    status_updates_collector
                        .lock()
                        .unwrap()
                        .push(status.clone());
                    if let Some(progress) = &progress_collector {
                        let mut progress = progress.lock().unwrap();
                        progress.status_updates.push(status.clone());
                        progress.events.push(HeadlessSubAgentEvent {
                            kind: HeadlessSubAgentEventKind::ToolApproval,
                            message: tool_name.clone(),
                        });
                        progress.events.push(HeadlessSubAgentEvent {
                            kind: HeadlessSubAgentEventKind::Status,
                            message: status,
                        });
                    }
                    let _ = auto_approve_tx.send(UiToAgentMessage::ApproveTool {
                        id,
                        tool_name,
                        arguments,
                    });
                }
                AgentToUiMessage::ToolExecutionStarted { tool_name } => {
                    if let Some(progress) = &progress_collector {
                        progress.lock().unwrap().events.push(HeadlessSubAgentEvent {
                            kind: HeadlessSubAgentEventKind::ToolStarted,
                            message: tool_name,
                        });
                    }
                }
                AgentToUiMessage::ToolExecutionFinished { tool_name, result } => {
                    if let Some(progress) = &progress_collector {
                        progress.lock().unwrap().events.push(HeadlessSubAgentEvent {
                            kind: HeadlessSubAgentEventKind::ToolFinished,
                            message: format!("{} => {}", tool_name, result),
                        });
                    }
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
        cancel_rx.as_ref(),
        progress.as_ref(),
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

pub fn run_agent_reasoning_loop(
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
    cancel_rx: Option<&Receiver<UiToAgentMessage>>,
    progress: Option<&Arc<Mutex<HeadlessSubAgentProgress>>>,
    ui_tx: &Sender<AgentToUiMessage>,
) {
    let mut sitemap_needed = false;
    let mut loop_count = 0;
    let max_loops = 15;

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
                "Querying {} (Turn {})…",
                provider.label(),
                loop_count
            )))
            .ok();

        let compressed_history = compress_history(message_history, profile.supports_tools);

        let request_body = build_request(
            profile,
            model,
            &compressed_history,
            &cf_tools,
            thinking,
            provider,
        );
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

        let mut used_account: Option<&CloudflareAccount> = None;
        let mut used_or_account: Option<&OpenRouterAccount> = None;
        let ureq_response = match provider {
            AiProvider::OpenRouter => {
                let start_idx = usage_tracker
                    .pick_or_account(or_accounts)
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
                                if body_lower.contains("free-models-per-day")
                                    || body_lower.contains("quota")
                                    || body_lower.contains("credit")
                                    || body_lower.contains("limit exceeded")
                                {
                                    if let Some(acct) = active_acct {
                                        usage_tracker.mark_or_exhausted(
                                            acct.n,
                                            &acct.label,
                                            &acct.tier,
                                        );
                                        send_usage_update(
                                            usage_tracker,
                                            accounts,
                                            or_accounts,
                                            ui_tx,
                                        );
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
                                        std::thread::sleep(std::time::Duration::from_secs(
                                            wait_secs,
                                        ));
                                    } else {
                                        ui_tx
                                            .send(AgentToUiMessage::OutputToken(format!(
                                                "\n\nOpenRouter rate limit error (429): {}",
                                                body
                                            )))
                                            .ok();
                                    }
                                }
                            }
                            Err(ureq::Error::Status(code, resp)) => {
                                let body = resp.into_string().unwrap_or_default();
                                ui_tx
                                    .send(AgentToUiMessage::OutputToken(format!(
                                        "\n\nOpenRouter error ({}): {}",
                                        code, body
                                    )))
                                    .ok();
                                break;
                            }
                            Err(e) => {
                                ui_tx
                                    .send(AgentToUiMessage::OutputToken(format!(
                                        "\n\nOpenRouter connection error: {:?}",
                                        e
                                    )))
                                    .ok();
                                break;
                            }
                        }
                    }

                    if final_res.is_some() {
                        break;
                    }
                    if !account_exhausted {
                        ui_tx
                            .send(AgentToUiMessage::StatusUpdate(
                                "OpenRouter request failed, trying next account key…".to_string(),
                            ))
                            .ok();
                    }
                }
                final_res
            }
            AiProvider::CloudflareWorkersAi => {
                if accounts.is_empty() {
                    ui_tx
                        .send(AgentToUiMessage::OutputToken(
                            "\n\nError: No Cloudflare accounts configured.".to_string(),
                        ))
                        .ok();
                    break;
                }
                let start_idx = usage_tracker
                    .pick_account(accounts)
                    .and_then(|picked| accounts.iter().position(|a| a.n == picked.n))
                    .unwrap_or(0);
                let mut cf_response = None;
                for i in 0..accounts.len() {
                    let account = &accounts[(start_idx + i) % accounts.len()];
                    if usage_tracker.is_exhausted(account.n) {
                        continue;
                    }
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
                                usage_tracker.mark_exhausted(
                                    account.n,
                                    &account.label,
                                    &account.tier,
                                );
                                send_usage_update(usage_tracker, accounts, or_accounts, ui_tx);
                                ui_tx
                                    .send(AgentToUiMessage::StatusUpdate(format!(
                                        "Account '{}' quota exhausted — trying next…",
                                        account.label
                                    )))
                                    .ok();
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
                    AiProvider::CloudflareWorkersAi => {
                        "All Cloudflare Workers AI accounts exhausted or failed."
                    }
                };
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

        let mut reader = std::io::BufReader::new(response.into_reader());
        let mut line_buf = String::new();

        let mut assistant_content = String::new();
        let mut reasoning_content = String::new();
        let mut accumulated_tools: Vec<ToolCallAccumulator> = Vec::new();

        let mut streamed_len: usize = 0;
        let mut suppressing = false;

        loop {
            if apply_headless_control_messages(cancel_rx, message_history, ui_tx, progress) {
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

                    if cleaned.starts_with("data: ") {
                        let json_part = &cleaned[6..];
                        if let Ok(parsed) = serde_json::from_str::<Value>(json_part) {
                            if let Some(choices) = parsed["choices"].as_array() {
                                if let Some(first_choice) = choices.get(0) {
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
            let tool_calls_arr = tcs.as_array().unwrap();

            let mut pending_ids = std::collections::HashSet::new();
            let mut tool_specs = Vec::new();

            for tc in tool_calls_arr {
                let call_id = tc["id"].as_str().unwrap_or("").to_string();
                let tool_name = tc["function"]["name"].as_str().unwrap_or("").to_string();
                let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                let arguments: Value = serde_json::from_str(args_str).unwrap_or(json!({}));

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

            let mut resolved_approvals = std::collections::HashMap::new();

            while !pending_ids.is_empty() {
                if apply_headless_control_messages(cancel_rx, message_history, ui_tx, progress) {
                    pending_ids.clear();
                    break;
                }
                if let Ok(ui_msg) = ui_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                    match ui_msg {
                        UiToAgentMessage::ApproveTool {
                            id,
                            tool_name: _,
                            arguments,
                        } => {
                            if pending_ids.remove(&id) {
                                resolved_approvals.insert(id, Some(arguments));
                            }
                        }
                        UiToAgentMessage::RejectTool { id, tool_name: _ } => {
                            if pending_ids.remove(&id) {
                                resolved_approvals.insert(id, None);
                            }
                        }
                        UiToAgentMessage::CancelTask => {
                            pending_ids.clear();
                            break;
                        }
                        _ => {}
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
                        ui_tx_clone
                            .send(AgentToUiMessage::ToolExecutionStarted {
                                tool_name: tool_name.clone(),
                            })
                            .ok();

                        match registry::call_tool_in_workspace(
                            &workspace_root_clone,
                            &tool_name,
                            &approved_args,
                        ) {
                            Ok(res) => {
                                tool_result = res;
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
                            }
                            Err(e) => {
                                tool_result = format!("Error executing tool: {:?}", e);
                            }
                        }

                        ui_tx_clone
                            .send(AgentToUiMessage::ToolExecutionFinished {
                                tool_name: tool_name.clone(),
                                result: tool_result.clone(),
                            })
                            .ok();
                    } else {
                        tool_result = "Error: Tool execution rejected by the user.".to_string();
                    }

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
                if tool_result.contains("Error executing tool") {
                    any_error = true;
                } else if tool_result.contains("rejected by the user") {
                    any_rejected = true;
                } else {
                    any_success = true;
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
        } else {
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
                Err(errors) => {
                    if loop_count < max_loops {
                        write_handover_nda(
                            workspace_root,
                            "self_correcting",
                            loop_count,
                            "compile_failed",
                            false,
                        );
                        ui_tx
                            .send(AgentToUiMessage::StatusUpdate(
                                "Compilation failed! Self-correcting...".to_string(),
                            ))
                            .ok();

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

                        continue;
                    } else {
                        write_handover_nda(
                            workspace_root,
                            "idle",
                            loop_count,
                            "compile_failed",
                            false,
                        );
                        ui_tx
                            .send(AgentToUiMessage::StatusUpdate(
                                "Compilation validation failed (Max limits reached)".to_string(),
                            ))
                            .ok();
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
    ui_tx
        .send(AgentToUiMessage::StatusUpdate(
            "Agent workflow finished. Idling.".to_string(),
        ))
        .ok();
    ui_tx.send(AgentToUiMessage::AgentFinished).ok();
}

pub fn sanitize_chat_token(s: &str) -> String {
    let mut out = s.to_string();
    let tags = [
        "</tool_call>",
        "<tool_call>",
        "</function>",
        "<function>",
        "</parameter>",
        "<parameter>",
    ];
    for tag in &tags {
        out = out.replace(&format!("{}\r\n", tag), "");
        out = out.replace(&format!("{}\n", tag), "");
        out = out.replace(tag, "");
    }
    let mut result = String::with_capacity(out.len());
    let chars: Vec<char> = out.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            let mut j = i + 1;
            let mut is_tag_structure = false;
            while j < chars.len() && chars[j] != '>' {
                let c = chars[j];
                if c.is_alphabetic()
                    || c == '/'
                    || c == '='
                    || c == '_'
                    || c == '-'
                    || c.is_ascii_digit()
                    || c == '\"'
                    || c == '\''
                    || c == '.'
                {
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
