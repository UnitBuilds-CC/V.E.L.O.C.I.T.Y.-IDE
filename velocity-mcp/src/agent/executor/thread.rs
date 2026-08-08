use super::super::models::*;
use super::super::nda::*;
use super::super::provider::*;
use super::super::coordination::CoordinationBus;
use super::loop_runner::run_agent_reasoning_loop;
use super::team_routing::try_route_team_prompt;
use super::utils::{build_inline_tool_docs, send_usage_update};
use crate::editor::expert_team::{load_expert_teams, ExpertTeam};
use crate::usage::*;
use crossbeam_channel::{Receiver, Sender};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn load_runtime_accounts(
    workspace_root: &PathBuf,
) -> (
    Vec<CloudflareAccount>,
    Vec<OpenRouterAccount>,
    Vec<AzureOpenAiAccount>,
    Vec<LocalOllamaAccount>,
) {
    (
        load_accounts(workspace_root),
        load_openrouter_accounts(workspace_root),
        load_azure_accounts(workspace_root),
        load_local_ollama_accounts(workspace_root),
    )
}

fn initial_provider_from_env() -> AiProvider {
    match std::env::var("LLM_PROVIDER")
        .unwrap_or_default()
        .to_lowercase()
        .as_str()
    {
        "openrouter" | "or" => AiProvider::OpenRouter,
        "azure" | "azure_openai" => AiProvider::AzureOpenAi,
        "ollama" | "local" => AiProvider::LocalOllama,
        _ => AiProvider::CloudflareWorkersAi,
    }
}

fn initial_model_for_provider(provider: AiProvider, ollama_accounts: &[LocalOllamaAccount]) -> String {
    match provider {
        AiProvider::OpenRouter => std::env::var("OPENROUTER_MODEL")
            .unwrap_or_else(|_| default_provider_model(provider)),
        AiProvider::CloudflareWorkersAi => {
            std::env::var("CF_MODEL").unwrap_or_else(|_| default_provider_model(provider))
        }
        AiProvider::AzureOpenAi => std::env::var("AZURE_OPENAI_DEPLOYMENT")
            .unwrap_or_else(|_| default_provider_model(provider)),
        AiProvider::LocalOllama => ollama_accounts
            .first()
            .map(|account| account.default_model.clone())
            .or_else(|| std::env::var("OLLAMA_MODEL").ok())
            .unwrap_or_else(|| default_provider_model(provider)),
        _ => default_provider_model(provider),
    }
}

fn initial_selected_profile(provider: AiProvider, model: &str) -> ModelInfo {
    match provider {
        AiProvider::OpenRouter => ModelInfo {
            id: model.to_string(),
            label: model.rsplit('/').next().unwrap_or(model).to_string(),
            api_style: ApiStyle::OpenAiTools,
            supports_tools: true,
            supports_thinking: false,
        },
        _ => default_model_info(model),
    }
}

fn fetch_models_for_provider(
    provider: AiProvider,
    accounts: &[CloudflareAccount],
    or_accounts: &[OpenRouterAccount],
    azure_accounts: &[AzureOpenAiAccount],
    ollama_accounts: &[LocalOllamaAccount],
    usage_tracker: &UsageTracker,
) -> Result<Vec<ModelInfo>, String> {
    match provider {
        AiProvider::CloudflareWorkersAi => fetch_model_catalog(accounts),
        AiProvider::OpenRouter => fetch_openrouter_models(or_accounts, usage_tracker),
        AiProvider::AzureOpenAi => fetch_azure_models(azure_accounts),
        AiProvider::LocalOllama => fetch_local_ollama_models(ollama_accounts),
        AiProvider::OpenAI => {
            let key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
            fetch_openai_models(&key)
        }
        AiProvider::Groq => {
            let key = std::env::var("GROQ_API_KEY").unwrap_or_default();
            fetch_groq_models(&key)
        }
        AiProvider::Mistral => {
            let key = std::env::var("MISTRAL_API_KEY").unwrap_or_default();
            fetch_mistral_models(&key)
        }
        AiProvider::Deepseek => {
            let key = std::env::var("DEEPSEEK_API_KEY").unwrap_or_default();
            fetch_deepseek_models(&key)
        }
        AiProvider::AlibabaQwen => {
            let key = std::env::var("DASHSCOPE_API_KEY").unwrap_or_default();
            fetch_alibaba_models(&key)
        }
        AiProvider::GoogleVertex => {
            let key = std::env::var("GOOGLE_API_KEY").unwrap_or_default();
            fetch_google_models(&key)
        }
        AiProvider::TogetherAi => {
            let key = std::env::var("TOGETHER_API_KEY").unwrap_or_default();
            fetch_together_models(&key)
        }
        AiProvider::FireworksAi => {
            let key = std::env::var("FIREWORKS_API_KEY").unwrap_or_default();
            fetch_fireworks_models(&key)
        }
        AiProvider::Perplexity => {
            let key = std::env::var("PERPLEXITY_API_KEY").unwrap_or_default();
            fetch_perplexity_models(&key)
        }
        AiProvider::Cerebras => {
            let key = std::env::var("CEREBRAS_API_KEY").unwrap_or_default();
            fetch_cerebras_models(&key)
        }
        _ => Ok(vec![default_model_info(&default_provider_model(provider))]),
    }
}

fn sync_model_state(
    provider: AiProvider,
    accounts: &[CloudflareAccount],
    model_catalog: &mut Vec<ModelInfo>,
    model: &mut String,
    selected_profile: &mut ModelInfo,
    thinking: &mut bool,
    requested_model: Option<String>,
    requested_thinking: Option<bool>,
    ui_tx: &Sender<AgentToUiMessage>,
) {
    let fallback_model = requested_model.unwrap_or_else(|| model.clone());
    if model_catalog.iter().any(|candidate| candidate.id == fallback_model) {
        *model = fallback_model;
    } else {
        *model = model_catalog
            .first()
            .map(|candidate| candidate.id.clone())
            .unwrap_or(fallback_model);
    }

    *selected_profile = model_catalog
        .iter()
        .find(|candidate| candidate.id == *model)
        .cloned()
        .unwrap_or_else(|| default_model_info(model));

    if provider == AiProvider::CloudflareWorkersAi {
        *selected_profile = enrich_model_profile(accounts, selected_profile);
        if let Some(entry) = model_catalog.iter_mut().find(|candidate| candidate.id == *model) {
            *entry = selected_profile.clone();
        }
    }

    let desired_thinking = requested_thinking.unwrap_or(*thinking);
    *thinking = desired_thinking && selected_profile.supports_thinking;

    ui_tx
        .send(AgentToUiMessage::ModelCatalog {
            models: model_catalog.clone(),
            selected: model.clone(),
            thinking: *thinking,
        })
        .ok();
}

pub fn run_agent_thread(
    mut workspace_root: PathBuf,
    ui_rx: Receiver<UiToAgentMessage>,
    ui_tx: Sender<AgentToUiMessage>,
) {
    let (mut accounts, mut or_accounts, mut azure_accounts, mut ollama_accounts) =
        load_runtime_accounts(&workspace_root);
    let mut usage_tracker = UsageTracker::new(&workspace_root);
    send_usage_update(&mut usage_tracker, &accounts, &or_accounts, &ui_tx);
    let mut provider = initial_provider_from_env();
    let mut model = initial_model_for_provider(provider, &ollama_accounts);
    let mut thinking = std::env::var("CF_THINKING")
        .map(|v| v != "0")
        .unwrap_or(true);

    let mut selected_profile = initial_selected_profile(provider, &model);
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
    let mut expert_teams: Vec<ExpertTeam> = load_expert_teams(&workspace_root);

    // Phase 5: Multi-agent coordination bus
    let coordination_bus = CoordinationBus::new();
    coordination_bus.report_progress("primary", 0.0, "initialized");

    while let Ok(msg) = ui_rx.recv() {
        process_ui_message(
            msg,
            &mut workspace_root,
            &mut accounts,
            &mut or_accounts,
            &mut azure_accounts,
            &mut ollama_accounts,
            &mut provider,
            &mut model,
            &mut thinking,
            &mut selected_profile,
            &mut model_catalog,
            &mut message_history,
            &mut usage_tracker,
            &mut expert_teams,
            &ui_rx,
            &ui_tx,
            &mut deferred_messages,
            &coordination_bus,
        );

        while !deferred_messages.is_empty() {
            let deferred = deferred_messages.remove(0);
            process_ui_message(
                deferred,
                &mut workspace_root,
                &mut accounts,
                &mut or_accounts,
                &mut azure_accounts,
                &mut ollama_accounts,
                &mut provider,
                &mut model,
                &mut thinking,
                &mut selected_profile,
                &mut model_catalog,
                &mut message_history,
                &mut usage_tracker,
                &mut expert_teams,
                &ui_rx,
                &ui_tx,
                &mut deferred_messages,
                &coordination_bus,
            );
        }
    }
}

fn process_ui_message(
    msg: UiToAgentMessage,
    workspace_root: &mut PathBuf,
    accounts: &mut Vec<CloudflareAccount>,
    or_accounts: &mut Vec<OpenRouterAccount>,
    azure_accounts: &mut Vec<AzureOpenAiAccount>,
    ollama_accounts: &mut Vec<LocalOllamaAccount>,
    provider: &mut AiProvider,
    model: &mut String,
    thinking: &mut bool,
    selected_profile: &mut ModelInfo,
    model_catalog: &mut Vec<ModelInfo>,
    message_history: &mut Vec<ChatMessage>,
    usage_tracker: &mut UsageTracker,
    expert_teams: &mut Vec<ExpertTeam>,
    ui_rx: &Receiver<UiToAgentMessage>,
    ui_tx: &Sender<AgentToUiMessage>,
    deferred_messages: &mut Vec<UiToAgentMessage>,
    coordination_bus: &CoordinationBus,
) {
    match msg {
        UiToAgentMessage::RefreshModels => {
            match fetch_models_for_provider(
                *provider,
                accounts,
                or_accounts,
                azure_accounts,
                ollama_accounts,
                usage_tracker,
            ) {
                Ok(models) => {
                    *model_catalog = models;
                    sync_model_state(
                        *provider,
                        accounts,
                        model_catalog,
                        model,
                        selected_profile,
                        thinking,
                        None,
                        None,
                        ui_tx,
                    );
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
                sync_model_state(
                    *provider,
                    accounts,
                    model_catalog,
                    model,
                    selected_profile,
                    thinking,
                    Some(selected),
                    None,
                    ui_tx,
                );
                ui_tx
                    .send(AgentToUiMessage::StatusUpdate(format!("Model set to {model}")))
                    .ok();
            }
        }
        UiToAgentMessage::SetThinking(enabled) => {
            *thinking = enabled && selected_profile.supports_thinking;
            ui_tx
                .send(AgentToUiMessage::ModelCatalog {
                    models: model_catalog.clone(),
                    selected: model.clone(),
                    thinking: *thinking,
                })
                .ok();
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
        UiToAgentMessage::ReloadProviderConfig => {
            let (new_accounts, new_or_accounts, new_azure_accounts, new_ollama_accounts) =
                load_runtime_accounts(workspace_root);
            *accounts = new_accounts;
            *or_accounts = new_or_accounts;
            *azure_accounts = new_azure_accounts;
            *ollama_accounts = new_ollama_accounts;
            send_usage_update(usage_tracker, accounts, or_accounts, ui_tx);
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(
                    "Reloaded workspace provider settings.".to_string(),
                ))
                .ok();
        }
        UiToAgentMessage::ApplySessionState {
            provider: requested_provider,
            model: requested_model,
            thinking: requested_thinking,
        } => {
            *provider = requested_provider;
            ui_tx.send(AgentToUiMessage::ProviderChanged(*provider)).ok();

            match fetch_models_for_provider(
                *provider,
                accounts,
                or_accounts,
                azure_accounts,
                ollama_accounts,
                usage_tracker,
            ) {
                Ok(models) => {
                    *model_catalog = models;
                    sync_model_state(
                        *provider,
                        accounts,
                        model_catalog,
                        model,
                        selected_profile,
                        thinking,
                        Some(requested_model),
                        Some(requested_thinking),
                        ui_tx,
                    );
                }
                Err(error) => {
                    *model = requested_model;
                    *selected_profile = initial_selected_profile(*provider, model);
                    *thinking = requested_thinking && selected_profile.supports_thinking;
                    *model_catalog = vec![selected_profile.clone()];
                    ui_tx
                        .send(AgentToUiMessage::ModelCatalog {
                            models: model_catalog.clone(),
                            selected: model.clone(),
                            thinking: *thinking,
                        })
                        .ok();
                    ui_tx.send(AgentToUiMessage::StatusUpdate(error)).ok();
                }
            }
        }
        UiToAgentMessage::SetProvider(new_provider) => {
            *provider = new_provider;
            ui_tx.send(AgentToUiMessage::ProviderChanged(*provider)).ok();

            match fetch_models_for_provider(
                *provider,
                accounts,
                or_accounts,
                azure_accounts,
                ollama_accounts,
                usage_tracker,
            ) {
                Ok(models) => {
                    *model_catalog = models;
                    sync_model_state(
                        *provider,
                        accounts,
                        model_catalog,
                        model,
                        selected_profile,
                        thinking,
                        None,
                        None,
                        ui_tx,
                    );
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
                *workspace_root = new_root.clone();
                let (new_accounts, new_or_accounts, new_azure_accounts, new_ollama_accounts) =
                    load_runtime_accounts(workspace_root);
                *accounts = new_accounts;
                *or_accounts = new_or_accounts;
                *azure_accounts = new_azure_accounts;
                *ollama_accounts = new_ollama_accounts;
                *usage_tracker = UsageTracker::new(workspace_root);
                send_usage_update(usage_tracker, accounts, or_accounts, ui_tx);
                write_sitemap_nda(workspace_root);
                *expert_teams = load_expert_teams(workspace_root);
                *message_history = load_chatlogs_nda(workspace_root).unwrap_or_else(|| {
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
                let restored: Vec<(String, String)> = message_history
                    .iter()
                    .filter(|m| m.role == "user" || m.role == "assistant")
                    .map(|m| (m.role.clone(), m.content.clone()))
                    .collect();
                ui_tx
                    .send(AgentToUiMessage::ChatHistoryRestored(restored))
                    .ok();
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
            // Pick up any teams/skills authored via tools in a previous turn so
            // they become routable immediately.
            *expert_teams = load_expert_teams(workspace_root);
            let routed = try_route_team_prompt(
                &prompt,
                expert_teams,
                workspace_root,
                accounts,
                or_accounts,
                azure_accounts,
                ollama_accounts,
                *provider,
                model,
                *thinking,
                message_history,
                usage_tracker,
                ui_rx,
                ui_tx,
                deferred_messages,
            );
            if routed {
                return;
            }

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
                ollama_accounts,
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
                coordination_bus,
            );
        }
        UiToAgentMessage::ReloadTeams => {
            *expert_teams = load_expert_teams(workspace_root);
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(format!(
                    "Reloaded {} expert team(s) from disk.",
                    expert_teams.len()
                )))
                .ok();
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
