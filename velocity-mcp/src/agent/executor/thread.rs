use super::super::models::*;
use super::super::nda::*;
use super::super::provider::*;
use super::loop_runner::run_agent_reasoning_loop;
use super::utils::{build_inline_tool_docs, send_usage_update};
use crate::usage::*;
use crossbeam_channel::{Receiver, Sender};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub fn run_agent_thread(
    mut workspace_root: PathBuf,
    ui_rx: Receiver<UiToAgentMessage>,
    ui_tx: Sender<AgentToUiMessage>,
) {
    let accounts = load_accounts_from_env();
    let or_accounts = load_openrouter_accounts_from_env();
    let azure_accounts = load_azure_accounts_from_env();
    let _ollama_accounts = load_local_ollama_accounts_from_env();
    let mut usage_tracker = UsageTracker::new(&workspace_root);
    send_usage_update(&mut usage_tracker, &accounts, &or_accounts, &ui_tx);
    let mut provider = match std::env::var("LLM_PROVIDER")
        .unwrap_or_default()
        .to_lowercase()
        .as_str()
    {
        "openrouter" | "or" => AiProvider::OpenRouter,
        "azure" | "azure_openai" => AiProvider::AzureOpenAi,
        "ollama" | "local" => AiProvider::LocalOllama,
        _ => AiProvider::CloudflareWorkersAi,
    };
    let mut model = match provider {
        AiProvider::OpenRouter => std::env::var("OPENROUTER_MODEL")
            .unwrap_or_else(|_| "tencent/hy3:free".to_string()),
        AiProvider::CloudflareWorkersAi => std::env::var("CF_MODEL")
            .unwrap_or_else(|_| "@cf/moonshotai/kimi-k2.7-code".to_string()),
        AiProvider::AzureOpenAi => std::env::var("AZURE_OPENAI_DEPLOYMENT")
            .unwrap_or_else(|_| "gpt-4o".to_string()),
        AiProvider::LocalOllama => std::env::var("OLLAMA_MODEL")
            .unwrap_or_else(|_| "llama3.2".to_string()),
    };
    let mut thinking = std::env::var("CF_THINKING")
        .map(|v| v != "0")
        .unwrap_or(true);

    let mut selected_profile = match provider {
        AiProvider::OpenRouter => ModelInfo {
            id: model.clone(),
            label: model.rsplit('/').next().unwrap_or(&model).to_string(),
            api_style: ApiStyle::OpenAiTools,
            supports_tools: true,
            supports_thinking: false,
        },
        AiProvider::CloudflareWorkersAi => default_model_info(&model),
        AiProvider::AzureOpenAi => default_model_info(&model),
        AiProvider::LocalOllama => default_model_info(&model),
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
            let sys = format!(
                "You are Antigravity, a high-performance agent running directly in V.E.L.O.C.I.T.Y.-IDE workspace. \
                You have direct local workspace access via tools. NEVER ask the user to paste code snippets, upload files, or provide repository links. \
                Immediately call `list_dir`, `read_file`, or `grep_search` to inspect and review the workspace.\n\n{}",
                build_inline_tool_docs()
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

    let mut deferred_messages: Vec<UiToAgentMessage> = Vec::new();

    while let Ok(msg) = ui_rx.recv() {
        process_ui_message(
            msg,
            &mut workspace_root,
            &accounts,
            &or_accounts,
            &azure_accounts,
            &mut provider,
            &mut model,
            &mut thinking,
            &mut selected_profile,
            &mut model_catalog,
            &mut message_history,
            &mut usage_tracker,
            &ui_rx,
            &ui_tx,
            &mut deferred_messages,
        );

        while !deferred_messages.is_empty() {
            let deferred = deferred_messages.remove(0);
            process_ui_message(
                deferred,
                &mut workspace_root,
                &accounts,
                &or_accounts,
                &azure_accounts,
                &mut provider,
                &mut model,
                &mut thinking,
                &mut selected_profile,
                &mut model_catalog,
                &mut message_history,
                &mut usage_tracker,
                &ui_rx,
                &ui_tx,
                &mut deferred_messages,
            );
        }
    }
}

fn process_ui_message(
    msg: UiToAgentMessage,
    workspace_root: &mut PathBuf,
    accounts: &[CloudflareAccount],
    or_accounts: &[OpenRouterAccount],
    azure_accounts: &[AzureOpenAiAccount],
    provider: &mut AiProvider,
    model: &mut String,
    thinking: &mut bool,
    selected_profile: &mut ModelInfo,
    model_catalog: &mut Vec<ModelInfo>,
    message_history: &mut Vec<ChatMessage>,
    usage_tracker: &mut UsageTracker,
    ui_rx: &Receiver<UiToAgentMessage>,
    ui_tx: &Sender<AgentToUiMessage>,
    deferred_messages: &mut Vec<UiToAgentMessage>,
) {
    match msg {
        UiToAgentMessage::RefreshModels => {
            let fetch_result = match *provider {
                AiProvider::CloudflareWorkersAi => fetch_model_catalog(accounts),
                AiProvider::OpenRouter => fetch_openrouter_models(or_accounts, usage_tracker),
                AiProvider::AzureOpenAi => fetch_azure_models(azure_accounts),
                AiProvider::LocalOllama => fetch_local_ollama_models(&load_local_ollama_accounts_from_env()),
            };
            match fetch_result {
                Ok(models) => {
                    *model_catalog = models;
                    if !model_catalog.iter().any(|candidate| candidate.id == *model) {
                        *model = model_catalog
                            .first()
                            .map(|candidate| candidate.id.clone())
                            .unwrap_or_else(|| model.clone());
                    }
                    *selected_profile = model_catalog
                        .iter()
                        .find(|candidate| candidate.id == *model)
                        .cloned()
                        .unwrap_or_else(|| default_model_info(model));
                    if *provider == AiProvider::CloudflareWorkersAi {
                        *selected_profile = enrich_model_profile(accounts, selected_profile);
                    }
                    if !selected_profile.supports_thinking {
                        *thinking = false;
                    }
                    ui_tx
                        .send(AgentToUiMessage::ModelCatalog {
                            models: model_catalog.clone(),
                            selected: model.clone(),
                            thinking: *thinking,
                        })
                        .ok();
                }
                Err(error) => {
                    ui_tx.send(AgentToUiMessage::StatusUpdate(error)).ok();
                }
            };
        }
        UiToAgentMessage::RefreshUsage => {
            send_usage_update(usage_tracker, accounts, or_accounts, ui_tx);
        }
        UiToAgentMessage::SetModel(selected) => {
            if !selected.trim().is_empty() {
                *model = selected;
                *selected_profile = model_catalog
                    .iter()
                    .find(|candidate| candidate.id == *model)
                    .cloned()
                    .unwrap_or_else(|| default_model_info(model));
                *selected_profile = enrich_model_profile(accounts, selected_profile);
                if let Some(entry) = model_catalog
                    .iter_mut()
                    .find(|candidate| candidate.id == *model)
                {
                    *entry = selected_profile.clone();
                }
                if !selected_profile.supports_thinking {
                    *thinking = false;
                }
                ui_tx
                    .send(AgentToUiMessage::ModelCatalog {
                        models: model_catalog.clone(),
                        selected: model.clone(),
                        thinking: *thinking,
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
            *thinking = enabled && selected_profile.supports_thinking;
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(
                    if *thinking {
                        "Thinking enabled"
                    } else {
                        "Thinking disabled"
                    }
                    .to_string(),
                ))
                .ok();
        }
        UiToAgentMessage::SetProvider(new_provider) => {
            *provider = new_provider;
            ui_tx.send(AgentToUiMessage::ProviderChanged(*provider)).ok();

            let fetch_result = match *provider {
                AiProvider::CloudflareWorkersAi => fetch_model_catalog(accounts),
                AiProvider::OpenRouter => fetch_openrouter_models(or_accounts, usage_tracker),
                AiProvider::AzureOpenAi => fetch_azure_models(azure_accounts),
                AiProvider::LocalOllama => fetch_local_ollama_models(&load_local_ollama_accounts_from_env()),
            };
            match fetch_result {
                Ok(models) => {
                    *model_catalog = models;
                    if !model_catalog.iter().any(|candidate| candidate.id == *model) {
                        *model = model_catalog
                            .first()
                            .map(|candidate| candidate.id.clone())
                            .unwrap_or_else(|| model.clone());
                    }
                    *selected_profile = model_catalog
                        .iter()
                        .find(|candidate| candidate.id == *model)
                        .cloned()
                        .unwrap_or_else(|| default_model_info(model));
                    if *provider == AiProvider::CloudflareWorkersAi {
                        *selected_profile = enrich_model_profile(accounts, selected_profile);
                    }
                    if !selected_profile.supports_thinking {
                        *thinking = false;
                    }
                    ui_tx
                        .send(AgentToUiMessage::ModelCatalog {
                            models: model_catalog.clone(),
                            selected: model.clone(),
                            thinking: *thinking,
                        })
                        .ok();
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "Loaded {} models for {}",
                            model_catalog.len(),
                            provider.label()
                        )))
                        .ok();
                }
                Err(error) => {
                    ui_tx.send(AgentToUiMessage::StatusUpdate(error)).ok();
                }
            }
        }
        UiToAgentMessage::SetWorkspace(new_root) => {
            if new_root.is_dir() {
                write_sitemap_nda(&new_root);
                *message_history = load_chatlogs_nda(&new_root).unwrap_or_else(|| {
                    let use_inline = *provider == AiProvider::OpenRouter
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
                *workspace_root = new_root.clone();
                *usage_tracker = UsageTracker::new(&new_root);
                send_usage_update(usage_tracker, accounts, or_accounts, ui_tx);
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
        UiToAgentMessage::ClearHistory => {
            let sys = format!(
                "You are Antigravity, a high-performance agent running directly in V.E.L.O.C.I.T.Y.-IDE workspace. \
                You have direct local workspace access via tools. NEVER ask the user to paste code snippets, upload files, or provide repository links. \
                Immediately call `list_dir`, `read_file`, or `grep_search` to inspect and review the workspace.\n\n{}",
                build_inline_tool_docs()
            );
            *message_history = vec![ChatMessage {
                role: "system".to_string(),
                content: sys,
                name: None,
                tool_call_id: None,
                tool_calls: None,
            }];
            save_chatlogs_nda(workspace_root, message_history);
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(
                    "Chat history cleared.".to_string(),
                ))
                .ok();
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
                workspace_root,
                accounts,
                or_accounts,
                azure_accounts,
                model,
                selected_profile,
                *provider,
                *thinking,
                message_history,
                usage_tracker,
                ui_rx,
                None,
                None,
                ui_tx,
                deferred_messages,
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
                .current_dir(workspace_root as &PathBuf)
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

            let _ = run_compilation_check(workspace_root);
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
                .current_dir(workspace_root as &PathBuf)
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
