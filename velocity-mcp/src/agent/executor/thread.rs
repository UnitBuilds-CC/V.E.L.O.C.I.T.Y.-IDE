use super::super::coordination::CoordinationBus;
use super::super::models::*;
use super::super::nda::*;
use super::super::provider::*;
use super::dispatch::resolve_api_key;
use super::dispatch::{alibaba_base_url, ALIBABA_DASHSCOPE_INTL_BASE_URL};
use super::loop_runner::run_agent_reasoning_loop;
use super::team_routing::try_route_team_prompt;
use super::utils::{build_inline_tool_docs, send_usage_update, SYSTEM_PROMPT_BASE};
use crate::editor::expert_team::{load_expert_teams, ExpertTeam};
use crate::safety::SafeMutex;
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

/// Load Velocity Router settings from the dedicated provider-settings file.
fn load_router_settings(workspace_root: &PathBuf) -> WorkspaceRouterSettings {
    load_workspace_provider_settings(workspace_root).velocity_router
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

fn initial_model_for_provider(
    provider: AiProvider,
    ollama_accounts: &[LocalOllamaAccount],
) -> String {
    match provider {
        AiProvider::OpenRouter => {
            std::env::var("OPENROUTER_MODEL").unwrap_or_else(|_| default_provider_model(provider))
        }
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
    workspace_root: &PathBuf,
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
            let key = resolve_api_key(workspace_root, "openai", "OPENAI_API_KEY");
            fetch_openai_models(&key)
        }
        AiProvider::Groq => {
            let key = resolve_api_key(workspace_root, "groq", "GROQ_API_KEY");
            fetch_groq_models(&key)
        }
        AiProvider::Mistral => {
            let key = resolve_api_key(workspace_root, "mistral", "MISTRAL_API_KEY");
            fetch_mistral_models(&key)
        }
        AiProvider::Deepseek => {
            let key = resolve_api_key(workspace_root, "deepseek", "DEEPSEEK_API_KEY");
            fetch_deepseek_models(&key)
        }
        AiProvider::AlibabaQwen => {
            let key = resolve_api_key(workspace_root, "alibaba", "DASHSCOPE_API_KEY");
            // Route the catalog fetch to whichever base URL matches the caller's
            // active plan (Token Plan vs. Coding Plan / DashScope international).
            let base = alibaba_base_url(workspace_root);
            if base == ALIBABA_DASHSCOPE_INTL_BASE_URL {
                fetch_alibaba_models(&key)
            } else {
                fetch_alibaba_models_at(&base, &key)
            }
        }
        AiProvider::GoogleVertex => {
            let key = resolve_api_key(workspace_root, "google", "GOOGLE_API_KEY");
            fetch_google_models(&key)
        }
        AiProvider::TogetherAi => {
            let key = resolve_api_key(workspace_root, "together", "TOGETHER_API_KEY");
            fetch_together_models(&key)
        }
        AiProvider::FireworksAi => {
            let key = resolve_api_key(workspace_root, "fireworks", "FIREWORKS_API_KEY");
            fetch_fireworks_models(&key)
        }
        AiProvider::Perplexity => {
            let key = resolve_api_key(workspace_root, "perplexity", "PERPLEXITY_API_KEY");
            fetch_perplexity_models(&key)
        }
        AiProvider::Cerebras => {
            let key = resolve_api_key(workspace_root, "cerebras", "CEREBRAS_API_KEY");
            fetch_cerebras_models(&key)
        }
        AiProvider::AwsBedrock => fetch_bedrock_models(),
        AiProvider::Anthropic => {
            let key = resolve_api_key(workspace_root, "anthropic", "ANTHROPIC_API_KEY");
            fetch_anthropic_models(&key)
        }
    }
}

/// The model to land on when the requested one is not in the catalog.
///
/// `model_catalog.first()` is whatever order the endpoint listed, and some
/// plans are multi-vendor: the Alibaba token plan serves DeepSeek, Kimi and GLM
/// beside Qwen, so the alphabetical first entry was a DeepSeek model on a
/// workspace that had just been moved onto Alibaba Qwen. Prefer the family the
/// provider is named after; everyone else keeps the endpoint's own order.
fn preferred_default_model(provider: AiProvider, catalog: &[ModelInfo]) -> Option<&ModelInfo> {
    let family = provider_family_keyword(provider)?;
    catalog
        .iter()
        .find(|model| model.id.to_ascii_lowercase().contains(family))
}

fn provider_family_keyword(provider: AiProvider) -> Option<&'static str> {
    match provider {
        AiProvider::AlibabaQwen => Some("qwen"),
        _ => None,
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
    if model_catalog
        .iter()
        .any(|candidate| candidate.id == fallback_model)
    {
        *model = fallback_model;
    } else {
        *model = preferred_default_model(provider, model_catalog)
            .or_else(|| model_catalog.first())
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
        if let Some(entry) = model_catalog
            .iter_mut()
            .find(|candidate| candidate.id == *model)
        {
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
            let mut sys = String::from(SYSTEM_PROMPT_BASE);
            // Inline tool docs only for models that receive no native tool
            // schemas; otherwise the same catalog ships twice per request.
            if provider == AiProvider::OpenRouter || !selected_profile.supports_tools {
                sys.push_str(&build_inline_tool_docs());
            }
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

    // Phase 6: Velocity Router MoA settings
    let router_settings = load_router_settings(&workspace_root);
    if router_settings.enabled {
        ui_tx
            .send(AgentToUiMessage::StatusUpdate(format!(
                "Velocity MoA routing enabled (router: {}).",
                router_settings.url
            )))
            .ok();
    }

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
            &router_settings,
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
                &router_settings,
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
    router_settings: &WorkspaceRouterSettings,
) {
    match msg {
        UiToAgentMessage::RefreshModels => {
            match fetch_models_for_provider(
                *provider,
                workspace_root,
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
                    .send(AgentToUiMessage::StatusUpdate(format!(
                        "Model set to {model}"
                    )))
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
            ui_tx
                .send(AgentToUiMessage::ProviderChanged(*provider))
                .ok();

            match fetch_models_for_provider(
                *provider,
                workspace_root,
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
            ui_tx
                .send(AgentToUiMessage::ProviderChanged(*provider))
                .ok();

            match fetch_models_for_provider(
                *provider,
                workspace_root,
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
                            "{}{}",
                            SYSTEM_PROMPT_BASE,
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
            let mut sys = String::from(SYSTEM_PROMPT_BASE);
            if *provider == AiProvider::OpenRouter || !selected_profile.supports_tools {
                sys.push_str(&build_inline_tool_docs());
            }
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
                Some(router_settings),
                None, // default max_loops
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
        UiToAgentMessage::FetchPanelData { panel } => {
            // Delegate to the shared panel-data implementation so MCP tools and
            // the agent channel stay in sync.
            let data = crate::registry::system_tools::fetch_panel_data_value(
                workspace_root,
                panel.as_str(),
                None,
            );
            match data {
                Ok(value) => {
                    let _ = ui_tx.send(AgentToUiMessage::PanelData {
                        panel: panel.clone(),
                        data: value,
                    });
                }
                Err(err) => {
                    let _ = ui_tx.send(AgentToUiMessage::PanelData {
                        panel: panel.clone(),
                        data: serde_json::json!({"error": err.to_string()}),
                    });
                }
            }
        }
        UiToAgentMessage::RunLocalBuild => {
            // Pinned to a manifest that belongs to this workspace. With a bare
            // `current_dir`, cargo walks *up* to whatever repository contains the
            // folder, so pressing Build in a scratch directory inside a Rust repo
            // silently compiled the whole repo -- minutes on every core, during
            // which the control bridge reads as a dead IDE rather than a busy one.
            let cargo_dir =
                match crate::automation::build_runner::cargo_manifest_dir(workspace_root) {
                    Ok(dir) => dir,
                    Err(reason) => {
                        refuse_local_cargo(ui_tx, "cargo check", &reason);
                        return;
                    }
                };
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
                .current_dir(&cargo_dir)
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

            // No second `run_compilation_check` here, as this used to end with:
            // the press already ran the check above, and the repeat run discarded
            // its result, so it bought nothing and cost another full compile.
            ui_tx.send(AgentToUiMessage::AgentFinished).ok();
        }
        UiToAgentMessage::RunLocalRun => {
            // Same pinning as the build, and the hazard is sharper here: cargo
            // would have walked up and *run* whatever binary the enclosing
            // workspace defaults to, from inside the IDE.
            let cargo_dir =
                match crate::automation::build_runner::cargo_manifest_dir(workspace_root) {
                    Ok(dir) => dir,
                    Err(reason) => {
                        refuse_local_cargo(ui_tx, "cargo run", &reason);
                        return;
                    }
                };
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
                .current_dir(&cargo_dir)
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

/// Tell the UI that a local cargo command was refused, and clear the "working"
/// state exactly the way a finished one does: without `AgentFinished` the editor
/// spinner never stops, so a refusal that returned early would leave the app
/// looking permanently busy instead of permanently refusing.
fn refuse_local_cargo(ui_tx: &Sender<AgentToUiMessage>, command: &str, reason: &str) {
    ui_tx
        .send(AgentToUiMessage::OutputToken(format!(
            "\n$ {} refused: {}\n",
            command, reason
        )))
        .ok();
    ui_tx
        .send(AgentToUiMessage::StatusUpdate(format!(
            "Local {} refused: this workspace has no Cargo.toml",
            command
        )))
        .ok();
    ui_tx.send(AgentToUiMessage::AgentFinished).ok();
}

pub fn run_compilation_check(workspace_root: &std::path::Path) -> Result<(), String> {
    // Called after agent edits to confirm the workspace still compiles, so a
    // refusal has to read as a failure rather than as a check that passed --
    // least of all a check that quietly passed on somebody's *other* project.
    let cargo_dir = crate::automation::build_runner::cargo_manifest_dir(workspace_root)?;
    let output = std::process::Command::new("cargo")
        .arg("check")
        .current_dir(&cargo_dir)
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
                    let mut guard = progress.lock_safe();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            label: id.into(),
            api_style: ApiStyle::OpenAiChat,
            supports_tools: false,
            supports_thinking: false,
        }
    }

    /// The token-plan listing is alphabetical, so its first entry is a DeepSeek
    /// model. A workspace that has just been reconciled onto Alibaba Qwen must
    /// land on Qwen, not on somebody else's model that happens to sort first.
    #[test]
    fn alibaba_defaults_to_a_qwen_model_not_the_first_in_the_list() {
        let catalog = [
            model("deepseek-v4-flash-0731"),
            model("kimi-k2.7-code"),
            model("qwen3.8-flash"),
        ];
        assert_eq!(
            preferred_default_model(AiProvider::AlibabaQwen, &catalog).map(|m| m.id.as_str()),
            Some("qwen3.8-flash")
        );
    }

    #[test]
    fn other_providers_keep_the_endpoints_own_order() {
        let catalog = [model("gpt-x"), model("gpt-y")];
        assert!(preferred_default_model(AiProvider::OpenAI, &catalog).is_none());
        assert!(preferred_default_model(AiProvider::CloudflareWorkersAi, &catalog).is_none());
    }

    /// A token-plan listing with no Qwen in it makes the family preference
    /// useless; the caller must still fall through to the first entry rather
    /// than end up with no model at all.
    #[test]
    fn family_preference_is_skipped_when_the_vendor_is_absent() {
        let catalog = [model("deepseek-v4-flash-0731"), model("glm-4.6")];
        assert!(preferred_default_model(AiProvider::AlibabaQwen, &catalog).is_none());
        let picked = preferred_default_model(AiProvider::AlibabaQwen, &catalog)
            .or_else(|| catalog.first())
            .map(|m| m.id.clone())
            .unwrap_or_default();
        assert_eq!(picked, "deepseek-v4-flash-0731");
    }

    #[test]
    fn family_match_is_case_insensitive() {
        let catalog = [model("DeepSeek-R1"), model("QWEN3-MAX")];
        assert_eq!(
            preferred_default_model(AiProvider::AlibabaQwen, &catalog).map(|m| m.id.as_str()),
            Some("QWEN3-MAX")
        );
    }

    #[test]
    fn empty_catalog_has_no_preferred_model() {
        let catalog: [ModelInfo; 0] = [];
        assert!(preferred_default_model(AiProvider::AlibabaQwen, &catalog).is_none());
    }
}
