use super::super::models::*;
use super::super::nda::*;
use super::super::provider::*;
use super::thread::{apply_headless_control_messages, run_compilation_check};
use super::utils::{
    build_request, compress_history, estimate_tokens, sanitize_chat_token, send_usage_update,
};
use crate::registry;
use crate::usage::{
    AzureOpenAiAccount, CloudflareAccount, LocalOllamaAccount, OpenRouterAccount, UsageTracker,
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
) {
    let mut sitemap_needed = false;
    let mut loop_count: usize = 0;
    let max_loops: usize = 15;
    let mut current_provider = provider;
    let mut current_model = model.to_string();
    let mut current_profile = profile.clone();
    let mut current_thinking = thinking && current_profile.supports_thinking;
    let mut provider_switched = false;

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
                current_provider.label(),
                loop_count
            )))
            .ok();

        let compressed_history = compress_history(message_history, current_profile.supports_tools);

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
            _ => {
                super::dispatch::execute_openrouter_request(
                    or_accounts,
                    accounts,
                    usage_tracker,
                    &request_body,
                    ui_tx,
                ).0
            }
        };

        let response = match ureq_response {
            Some(res) => res,
            None => {
                let fallback = fallback_provider(current_provider);
                let fallback_available = match fallback {
                    AiProvider::OpenRouter => !or_accounts.is_empty() || !openrouter_api_key().trim().is_empty(),
                    AiProvider::CloudflareWorkersAi => !accounts.is_empty(),
                    AiProvider::AzureOpenAi => !azure_accounts.is_empty(),
                    AiProvider::LocalOllama => !ollama_accounts.is_empty(),
                    _ => true,
                };

                if !provider_switched && fallback_available {
                    provider_switched = true;
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

                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "Provider {} unavailable/exhausted. Seamlessly falling back to {} (Model: {})...",
                            prev_provider.label(),
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

                let err_msg = match current_provider {
                    AiProvider::OpenRouter => "OpenRouter request failed across all accounts.",
                    AiProvider::CloudflareWorkersAi => {
                        "All Cloudflare Workers AI accounts exhausted or failed."
                    }
                    AiProvider::AzureOpenAi => "Azure OpenAI request failed or no accounts configured.",
                    AiProvider::LocalOllama => "Local Ollama server unreachable (http://localhost:11434).",
                    _ => "Selected AI Provider request failed or key missing.",
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
            let mut interrupted = false;
            while let Ok(ui_msg) = ui_rx.try_recv() {
                match ui_msg {
                    UiToAgentMessage::CancelTask => {
                        ui_tx.send(AgentToUiMessage::StatusUpdate("Task interrupted by operator.".to_string())).ok();
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
                            ui_tx.send(AgentToUiMessage::StatusUpdate("Task interrupted by operator.".to_string())).ok();
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

            let mut handles = Vec::new();

            for (call_id, tool_name, _original_arguments) in tool_specs {
                let approval = resolved_approvals.get(&call_id).cloned().flatten();
                let workspace_root_clone = workspace_root.clone();
                let ui_tx_clone = ui_tx.clone();

                let handle = std::thread::spawn(move || {
                    let (tool_result, file_buffer_update, changelog_entry) =
                        if let Some(approved_args) = approval {
                            ui_tx_clone
                                .send(AgentToUiMessage::ToolExecutionStarted {
                                    tool_name: tool_name.clone(),
                                })
                                .ok();

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
            if sitemap_needed {
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
                        ui_tx.send(AgentToUiMessage::StatusUpdate(format!("Build status: {e}"))).ok();
                        break;
                    }
                }
            } else {
                write_handover_nda(workspace_root, "idle", loop_count, "completed", false);
                break;
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
