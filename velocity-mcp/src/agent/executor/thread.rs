use super::super::coordination::CoordinationBus;
use super::super::models::*;
use super::super::nda::*;
use super::super::provider::*;
use super::dispatch::resolve_api_key;
use super::dispatch::{alibaba_base_url, ALIBABA_DASHSCOPE_INTL_BASE_URL};
use super::loop_runner::run_agent_reasoning_loop;
use super::team_routing::try_route_team_prompt;
use super::utils::{build_inline_tool_docs, send_usage_update, SYSTEM_PROMPT_BASE};
use crate::automation::MediatorArena;
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
            // A multi-host account list (e.g. from OLLAMA_HOSTS) carries an
            // empty default_model per host; skip it so we fall through to the
            // env / provider default instead of starting the loop with "".
            .filter(|m| !m.trim().is_empty())
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
    mediator: Arc<MediatorArena>,
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

    // Phase 5: Multi-agent coordination bus. Built on the app's shared write gate
    // so this executor's file claims consult the *same* table as the orchestrator
    // (spec D5 runtime unify) instead of a private one.
    let coordination_bus = CoordinationBus::with_gate(mediator.gate_arc());
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
                    let use_inline =
                        *provider == AiProvider::OpenRouter || !selected_profile.supports_tools;
                    vec![ChatMessage {
                        role: "system".to_string(),
                        content: format!(
                            "{}{}",
                            SYSTEM_PROMPT_BASE,
                            if use_inline {
                                build_inline_tool_docs()
                            } else {
                                String::new()
                            }
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

            // Intent brain. No explicit `@team`/`/team` directive matched, so ask
            // the (pluggable) decision layer whether this is a sprawling task that
            // wants a coordinated build-out. When it clearly does, auto-provision
            // the team the request implies and hand it to the proven routing path.
            // Gated conservatively: an ordinary prompt keeps the single-agent path
            // it always had, and a failed/uncertain decision never blocks progress.
            const AUTO_TEAM_CONFIDENCE: f32 = 0.70;
            let decision = crate::agent::intent::decide_intent(&prompt);
            if decision.scope == crate::agent::intent::TaskScope::Team
                && decision.confidence >= AUTO_TEAM_CONFIDENCE
                && decision.roles.len() >= 2
            {
                match crate::agent::intent::provision_team(
                    workspace_root,
                    &decision,
                    "Intent Auto-Team",
                ) {
                    Ok(slug) => {
                        ui_tx
                            .send(AgentToUiMessage::StatusUpdate(format!(
                                "Intent: assembling a {}-specialist team ({}).",
                                decision.roles.len(),
                                decision.rationale
                            )))
                            .ok();
                        *expert_teams = load_expert_teams(workspace_root);
                        let directive = format!("@{slug} {prompt}");
                        let routed = try_route_team_prompt(
                            &directive,
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
                    }
                    Err(e) => {
                        ui_tx
                            .send(AgentToUiMessage::StatusUpdate(format!(
                                "Team-shaped task, but no team was provisioned ({e}); using a single agent."
                            )))
                            .ok();
                    }
                }
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
                None,  // default max_loops
                None,  // interactive session: the user cancels, no wall-clock budget
                false, // interactive chat: a text answer is a valid completion
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

/// A `NNN |` rustc code-echo line, which starts at column 0 once the line
/// number reaches the gutter width — so indentation alone cannot identify it.
fn is_error_block_body(line: &str) -> bool {
    if line == "..." {
        return true; // rustc's elision marker inside a span
    }
    let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    digits > 0 && line[digits..].trim_start().starts_with('|')
}

/// The `cargo check` stderr lines the model needs in order to fix the code.
///
/// A repair turn advertises no `read_file`, so the diagnostic has to carry both
/// halves of the job: *where* (`E0753: expected outer doc comment` says nothing
/// about location) and *what rustc concluded*. The first version kept the
/// heading and its `-->` line only, which threw away the whole span body — the
/// `mutable borrow occurs here` / `mutable borrow later used here` lines, and
/// with them the second site's line number. Half of a borrow conflict is fixed
/// at that other site, so the model was handed the symptom and left to guess.
///
/// One early version also required `--> src/`, but Windows rustc prints
/// `--> src\file.rs`, which matched nothing — silently discarding every
/// location on the platform most agents run on.
///
/// Whole `error` blocks are therefore kept and everything outside them dropped:
/// warnings use rustc's exact line shape, and a warning-heavy crate would bury
/// the errors. Output is size-capped because one `cargo check` of a 30 KB port
/// can print fifty blocks.
fn select_error_diagnostics(stderr: &str) -> Vec<String> {
    const MAX_CHARS: usize = 6_000;
    let mut kept: Vec<String> = Vec::new();
    let mut spent = 0usize;
    let mut under_error = false;
    let mut impl_list = false;
    let mut over_budget = false;
    let mut omitted = 0usize;

    for line in stderr.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            under_error = false;
            impl_list = false;
            continue;
        }
        let is_head = trimmed.contains("error[E") || trimmed.starts_with("error:");
        if is_head {
            under_error = true;
        } else if !(under_error
            && (line.starts_with(' ') || line.starts_with('\t') || is_error_block_body(trimmed)))
        {
            // Anything else ends the block: rustc's `For more information`
            // trailer, cargo's `warning: generated N warnings` summary.
            under_error = false;
            impl_list = false;
            continue;
        }
        // rustc's `= help: the following other types implement trait` lead-in
        // is followed by up to sixty indented `` `&f64` implements `Mul<f64>` ``
        // lines per error. None of them can be the fix — for `86400.0 * 2` the
        // caret line two above already names the types that mismatch — and at
        // two or three errors they consume the whole budget, so real errors get
        // the "not shown" treatment. Keep the lead-in, say the list was dropped,
        // skip the items.
        if trimmed.starts_with("= help: the following other types implement trait") {
            impl_list = true;
            let trait_name = trimmed
                .split(" trait ")
                .nth(1)
                .map(|t| t.trim_end_matches(':'))
                .unwrap_or("this trait");
            let summary = format!("= help: the other implementors of {trait_name} omitted");
            spent += summary.len() + 1;
            kept.push(summary);
            continue;
        }
        if impl_list {
            let item = trimmed.starts_with('`')
                || (trimmed.starts_with("and ") && trimmed.ends_with("others"));
            if item {
                continue;
            }
            impl_list = false;
        }
        if over_budget {
            if is_head {
                omitted += 1;
            }
            continue;
        }
        spent += trimmed.len() + 1;
        kept.push(trimmed.to_string());
        if spent > MAX_CHARS {
            over_budget = true;
        }
    }
    if over_budget {
        kept.push(format!(
            "... further diagnostics omitted ({} error block(s) not shown)",
            omitted
        ));
    }
    kept
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
                let errors = select_error_diagnostics(&stderr);
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

/// The gate the loop runs after a write: the library must build, and so must
/// the test code in the files the agent just wrote.
pub fn run_post_write_check(
    workspace_root: &std::path::Path,
    written: &[&str],
) -> Result<(), String> {
    run_compilation_check(workspace_root)?;
    run_test_code_check(workspace_root, written)
}

/// Does the `#[cfg(test)]` code in the files the agent wrote compile?
///
/// `cargo check` deliberately skips test targets, so a port whose own tests do
/// not build sails through the loop's gate while the deliverable is graded by
/// `cargo test` — and the repair directive the loop sends already tells the
/// model to keep the `#[cfg(test)]` tests real.
///
/// The gate cannot simply become `cargo check --all-targets`: a crate's
/// pre-existing sibling modules would then fail every dispatch, and the agent
/// would spend its repair turns on files it was never asked to touch. So test
/// targets are checked separately and only error blocks rustc located inside a
/// file the agent wrote are kept. A failure we cannot attribute is not reported
/// at all — the agent is never blamed for a line it did not edit, and the loop is
/// never blocked by an error it cannot fix.
pub fn run_test_code_check(
    workspace_root: &std::path::Path,
    written: &[&str],
) -> Result<(), String> {
    let rust_files: Vec<&str> = written
        .iter()
        .copied()
        .filter(|w| w.replace('\\', "/").ends_with(".rs"))
        .collect();
    if rust_files.is_empty() {
        return Ok(());
    }
    let cargo_dir = crate::automation::build_runner::cargo_manifest_dir(workspace_root)?;
    let output = std::process::Command::new("cargo")
        .arg("check")
        .arg("--all-targets")
        .current_dir(&cargo_dir)
        .output();
    let Ok(out) = output else {
        // A gate that cannot launch is not evidence about the agent's code.
        log::debug!("test-target check failed to launch: {:?}", output.err());
        return Ok(());
    };
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let mine = retain_own_error_blocks(&stderr, workspace_root, &cargo_dir, &rust_files);
    if mine.trim().is_empty() {
        log::debug!("test-target failures exist, none inside {rust_files:?}");
        return Ok(());
    }
    let errors = select_error_diagnostics(&mine);
    if errors.is_empty() {
        return Ok(());
    }
    Err(format!(
        "These failures are in `#[cfg(test)]` code, which a plain `cargo check` does not \
         build; the library itself compiles. Full command: `cargo check --all-targets`.\n{}",
        errors.join("\n")
    ))
}

/// One file the agent wrote, in every spelling a toolchain might use for it.
struct WrittenFile {
    workspace_rel: String,
    crate_rel: String,
    abs: std::path::PathBuf,
}

/// The written paths, resolved against the crate the toolchain runs in.
///
/// A batch can write several modules, and a toolchain names a location either
/// relative to the directory it ran from, or absolutely. Both forms have to be
/// recognised: `rust_port/src/budget_manager.rs` is what the changelog records
/// while `src/budget_manager.rs` is what cargo prints — and on Windows the
/// second arrives with backslashes.
fn written_files(
    workspace_root: &std::path::Path,
    cargo_dir: &std::path::Path,
    written: &[&str],
) -> Vec<WrittenFile> {
    let crate_prefix = cargo_dir
        .strip_prefix(workspace_root)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .filter(|p| !p.is_empty())
        .map(|p| if p.ends_with('/') { p } else { format!("{p}/") });

    written
        .iter()
        .map(|w| {
            let workspace_rel = w.replace('\\', "/").trim_start_matches("./").to_string();
            let crate_rel = match crate_prefix.as_deref() {
                Some(prefix) => workspace_rel
                    .strip_prefix(prefix)
                    .unwrap_or(&workspace_rel)
                    .to_string(),
                None => workspace_rel.clone(),
            };
            let abs = workspace_root.join(w.replace('/', std::path::MAIN_SEPARATOR_STR));
            WrittenFile {
                workspace_rel,
                crate_rel,
                abs,
            }
        })
        .collect()
}

/// Does this toolchain location name one of the files the agent wrote?
fn location_is_written(files: &[WrittenFile], loc: &str) -> bool {
    let loc_norm = loc.replace('\\', "/");
    let absolute = std::path::Path::new(loc).is_absolute();
    files.iter().any(|f| {
        if loc_norm == f.crate_rel || loc_norm == f.workspace_rel {
            return true;
        }
        absolute
            && match (
                std::path::Path::new(loc).canonicalize(),
                f.abs.canonicalize(),
            ) {
                (Ok(a), Ok(b)) => a == b,
                _ => false,
            }
    })
}

/// The error blocks of a cargo run whose reported location sits in one of the
/// files the agent wrote.
fn retain_own_error_blocks(
    stderr: &str,
    workspace_root: &std::path::Path,
    cargo_dir: &std::path::Path,
    written: &[&str],
) -> String {
    // Blocks are found by their blank-line separation, so the child's newline
    // spelling has to be settled first: a CRLF stream would leave the whole
    // stderr as one block and the first location in it would decide the
    // attribution for every error the run printed.
    let stderr = stderr.replace("\r\n", "\n");
    let owned = written_files(workspace_root, cargo_dir, written);
    let mut kept = String::new();
    for block in stderr.split("\n\n") {
        let mut is_error = false;
        let mut is_mine = false;
        let mut located = false;
        for line in block.lines() {
            let body = line.trim();
            if body.starts_with("error[E") || body.starts_with("error:") {
                is_error = true;
            }
            // rustc prints the span it ADDRESSED the error at first, then any
            // related spans as `note:`/`help:`. Attribute on that first one only:
            // an error whose primary span sits in a sibling module is not this
            // agent's, however much it touches the written file on the way.
            if located {
                continue;
            }
            if let Some(rest) = body.strip_prefix("-->") {
                located = true;
                if let Some(loc) = rest.rsplitn(3, ':').last() {
                    is_mine = location_is_written(&owned, loc.trim());
                }
            }
        }
        if is_error && is_mine {
            kept.push_str(block);
            kept.push('\n');
        }
    }
    kept
}

/// One test the harness reported as failing, with where it panicked.
struct TestFailure {
    name: String,
    /// The path exactly as the harness printed it, backslashes included.
    location: String,
    line: usize,
    /// Everything the panic said after the location, usually the assertion.
    message: String,
}

/// The failing tests of a `cargo test` run.
///
/// libtest prints one section per failure:
///
/// ```text
/// failures:
///
/// ---- budget_manager::tests::test_projected_cost stdout ----
///
/// thread '...' panicked at src\budget_manager.rs:535:9:
/// assertion failed: (projected - 0.009).abs() < f64::EPSILON
/// ```
///
/// The location is peeled from the right because a panic inside an absolutely
/// pathed source file carries a Windows drive letter, which a left-to-right
/// split reads as the whole path.
fn parse_test_failures(output: &str) -> Vec<TestFailure> {
    let output = output.replace('\r', "");
    let mut out: Vec<TestFailure> = Vec::new();
    // The section being read, if the harness printed one.
    let mut current: Option<TestFailure> = None;
    // Whether the panic line has been seen yet: only what follows it is the
    // message, and the line before it is the harness naming the thread.
    let mut past_panic = false;
    let flush = |failure: Option<TestFailure>, out: &mut Vec<TestFailure>| {
        if let Some(done) = failure {
            out.push(done);
        }
    };
    for raw in output.lines() {
        let body = raw.trim();
        if let Some(rest) = body.strip_prefix("---- ").and_then(|r| {
            r.strip_suffix(" stdout ----")
                .or_else(|| r.strip_suffix(" stderr ----"))
        }) {
            flush(current.take(), &mut out);
            past_panic = false;
            current = Some(TestFailure {
                name: rest.trim().to_string(),
                location: String::new(),
                line: 0,
                message: String::new(),
            });
            continue;
        }
        // The summary list repeats the names without a panic, and `test result:`
        // closes the section — neither is a failure to report.
        if body == "failures:" || body.starts_with("test result:") {
            flush(current.take(), &mut out);
            past_panic = false;
            continue;
        }
        let Some(f) = current.as_mut() else { continue };
        // `thread '<name>' panicked at src\budget_manager.rs:535:9:` — the
        // location is the tail of the line, not the whole of it.
        if let Some(rest) = body.split_once("panicked at ").map(|(_, after)| after) {
            let rest = rest.strip_suffix(':').unwrap_or(rest);
            let mut bits = rest.rsplitn(3, ':');
            let col = bits.next().unwrap_or("");
            let line = bits.next().unwrap_or("");
            past_panic = true;
            match (line.trim().parse::<usize>(), col.parse::<usize>()) {
                (Ok(at), Ok(_)) => {
                    f.location = bits.next().unwrap_or(rest).trim().to_string();
                    f.line = at;
                }
                _ => f.location = rest.trim().to_string(),
            }
            continue;
        }
        if !past_panic || body.is_empty() || body.starts_with("note:") {
            continue;
        }
        if !f.message.is_empty() {
            f.message.push(' ');
        }
        f.message.push_str(body);
    }
    flush(current, &mut out);
    out.retain(|f| !f.location.is_empty());
    out
}

/// The failing tests this agent can be held to: those whose panic sits in a
/// file it wrote.
///
/// A sibling module's red test is not this dispatch's fault, and reporting it
/// would spend a repair turn on a file the agent was never given.
fn own_test_failures(
    output: &str,
    workspace_root: &std::path::Path,
    cargo_dir: &std::path::Path,
    written: &[&str],
) -> Vec<TestFailure> {
    let owned = written_files(workspace_root, cargo_dir, written);
    parse_test_failures(output)
        .into_iter()
        .filter(|f| location_is_written(&owned, &f.location))
        .collect()
}

/// Wall-clock ceiling for the post-run test gate.
///
/// A test binary links real codegen, so it costs strictly more than the two
/// `cargo check` passes that precede it — and a network-bound test can hang
/// forever, which is why the run is bounded rather than trusted. A gate that
/// overruns is killed and reports no verdict: a slow crate loses the test
/// verdict, never the dispatch.
const TEST_GATE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(240);

/// How many failing tests one repair turn is shown.
const MAX_TEST_FAILURES: usize = 6;

/// The preamble [`run_written_tests`] puts on its report, which is how the repair
/// directive tells "these tests fail" apart from "this does not compile".
pub const TEST_FAILURE_PREAMBLE: &str = "The crate BUILDS, but its own tests do NOT pass.";

pub fn is_test_failure_report(gate_message: &str) -> bool {
    gate_message.starts_with(TEST_FAILURE_PREAMBLE)
}

/// What the post-run test gate concluded.
#[derive(Debug, PartialEq, Eq)]
pub enum TestVerdict {
    /// The tests ran and were green. Carries cargo's own summary so the loop can
    /// report a number it actually observed.
    Passed(String),
    /// The gate abstained. `loop` says so in its success line instead of
    /// claiming a test run it never saw.
    NotJudged(String),
}

/// Do the tests of the written crate actually pass?
///
/// The loop's completion gate used to stop at `cargo check --all-targets`, which
/// proves the `#[cfg(test)]` code compiles and says nothing about what it
/// asserts. A probe run printed "Compiler validation succeeded!" over a port whose
/// `cargo test` returned 68 passed / 2 failed: the deliverable was graded by a
/// gate that had never run the thing being graded.
///
/// Runs once, at the completion gate, not after every write batch: linking and
/// executing the test binary is the expensive half of validation, and a model that
/// is mid-edit gains nothing from paying for it per turn. The attribution rule is
/// the compile gates' rule — a failure the loop cannot place inside a file this
/// agent wrote is not reported, because the agent cannot repair a module it was
/// never given, and a gate that overreaches blocks every dispatch in the repo.
///
/// `Err` is a report of failing tests; `Ok(NotJudged)` keeps the loop honest about
/// the cases where cargo could not be consulted.
pub fn run_written_tests(
    workspace_root: &std::path::Path,
    written: &[&str],
) -> Result<TestVerdict, String> {
    let rust_files: Vec<&str> = written
        .iter()
        .copied()
        .filter(|w| w.replace('\\', "/").ends_with(".rs"))
        .collect();
    if rust_files.is_empty() {
        return Ok(TestVerdict::NotJudged(
            "no Rust file was written, so there are no tests to run".to_string(),
        ));
    }
    let cargo_dir = crate::automation::build_runner::cargo_manifest_dir(workspace_root)?;
    let Ok(run) = crate::automation::mission_runner::run_bounded(
        &cargo_dir,
        "cargo",
        &["test"],
        TEST_GATE_TIMEOUT,
    ) else {
        return Ok(TestVerdict::NotJudged(
            "`cargo test` could not be launched here".to_string(),
        ));
    };
    if run.timed_out {
        log::warn!(
            "test gate killed at {}s in {}",
            TEST_GATE_TIMEOUT.as_secs(),
            cargo_dir.display()
        );
        return Ok(TestVerdict::NotJudged(format!(
            "`cargo test` overran its {}s deadline and was killed",
            TEST_GATE_TIMEOUT.as_secs()
        )));
    }
    let output = run.combined();
    if run.succeeded() {
        return Ok(TestVerdict::Passed(test_result_summary(&output)));
    }
    let failures: Vec<TestFailure> =
        own_test_failures(&output, workspace_root, &cargo_dir, &rust_files);
    if failures.is_empty() {
        log::debug!("`cargo test` failed, no panic located inside {rust_files:?}");
        return Ok(TestVerdict::NotJudged(
            "`cargo test` failed outside the files this agent wrote".to_string(),
        ));
    }

    // rustc-shaped blocks, so the repair turn's excerpt reader finds the same
    // `--> file:line` it parses out of a compile error and pastes the verbatim
    // bytes the assertion sits on.
    let mut report = format!(
        "{TEST_FAILURE_PREAMBLE} Real `cargo test` output ({}), one block per failing test:\n",
        test_result_summary(&output)
    );
    for f in failures.iter().take(MAX_TEST_FAILURES) {
        let path = f.location.replace('\\', "/");
        let source = std::fs::read_to_string(cargo_dir.join(&f.location))
            .ok()
            .and_then(|text| {
                text.lines()
                    .nth(f.line.saturating_sub(1))
                    .map(str::to_string)
            });
        report.push_str(&format!(
            "error: test `{}` failed\n  --> {}:{}:1\n",
            f.name, path, f.line
        ));
        if let Some(source) = source {
            report.push_str(&format!(
                "{} | {}\n      | {}\n",
                f.line,
                source.trim_end(),
                if f.message.is_empty() {
                    "panicked".to_string()
                } else {
                    f.message.clone()
                }
            ));
        }
    }
    if failures.len() > MAX_TEST_FAILURES {
        report.push_str(&format!(
            "... {} failing test(s) not shown.\n",
            failures.len() - MAX_TEST_FAILURES
        ));
    }
    Err(report)
}

/// cargo's own tally, e.g. `test result: ok. 70 passed; 0 failed`.
///
/// A crate with several test targets prints one line per target; joining them
/// keeps a `src/lib.rs` unit run and an integration run from being reported as
/// whichever finished first.
fn test_result_summary(output: &str) -> String {
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("test result:"))
        .take(3)
        .collect();
    if lines.is_empty() {
        "`cargo test` reported no result line".to_string()
    } else {
        lines.join(" / ")
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

    /// A repair turn has no `read_file`, so a location line is the only thing
    /// that tells the model *where* to patch — and `E0753` says nothing without
    /// it. The filter matched `--> src/` only, while Windows rustc prints
    /// `--> src\file.rs`, so every location on that platform was discarded and
    /// the model was sent a message with no address.
    #[test]
    fn error_diagnostics_keep_locations_written_with_either_separator() {
        let stderr = "warning: unused variable: `x`\n  --> src\\other.rs:1:5\n\
             \n\
             error[E0753]: expected outer doc comment\n  --> src\\budget_manager.rs:20:1\n\
             \n\
             error[E0382]: borrow of moved value: `val`\n   --> C:\\ws\\src\\lib.rs:44:9\n";
        let kept = select_error_diagnostics(stderr);
        let joined = kept.join("\n");
        assert!(
            joined.contains("--> src\\budget_manager.rs:20:1"),
            "backslash location must survive: {joined}"
        );
        assert!(
            joined.contains("--> C:\\ws\\src\\lib.rs:44:9"),
            "absolute location must survive: {joined}"
        );
        assert!(
            !joined.contains("other.rs"),
            "a warning's location is noise, not a fix target: {joined}"
        );
        assert!(
            !joined.contains("unused variable"),
            "warnings are not diagnostics: {joined}"
        );
    }

    /// Blank-line block separation is what bounds the state machine; without
    /// it one error's location would be attributed to the next five messages.
    #[test]
    fn error_diagnostics_stop_attributing_locations_after_the_block() {
        let stderr = "error[E0308]: mismatched types\n  --> src/a.rs:1:1\n\
             \n\
             help: consider here\n";
        let kept = select_error_diagnostics(stderr);
        assert_eq!(
            kept,
            vec!["error[E0308]: mismatched types", "--> src/a.rs:1:1"],
            "kept lines are trimmed of rustc's gutter indent"
        );
    }

    /// The span body IS the diagnosis. `E0502` addresses one line, but the fix
    /// is usually at the other end of the conflict, and a repair turn has no
    /// `read_file` to find it with: keeping the heading and `-->` alone sent
    /// the model a message naming a symptom and half a location.
    #[test]
    fn error_diagnostics_keep_the_span_body_that_names_the_other_site() {
        let stderr = "warning: unused doc comment\n   --> src\\budget_manager.rs:178:9\n\
             \n\
             error[E0502]: cannot borrow `*self` as immutable because it is also borrowed as mutable\n\
             \x20\x20\x20--> src\\budget_manager.rs:275:9\n\
             \x20\x20\x20|\n\
             259 |           let entry = self\n\
             \x20\x20\x20|  _____________________-\n\
             260 | |             .user_dict\n\
             \x20\x20\x20| |______________________- mutable borrow occurs here\n\
             ...\n\
             275 |           self._save_data();\n\
             \x20\x20\x20|           ^^^^ immutable borrow occurs here\n\
             ...\n\
             278 |           result.insert(\"user\".to_string(), entry.clone());\n\
             \x20\x20\x20|                                             ----- mutable borrow later used here\n\
             \n\
             For more information about this error, try `rustc --explain E0502`.\n\
             warning: `litellm-port` (lib) generated 1 warning\n\
             error: could not compile `litellm-port` (lib) due to 1 previous error\n";
        let joined = select_error_diagnostics(stderr).join("\n");
        assert!(
            joined.contains("mutable borrow later used here"),
            "rustc's conclusion about the other site must survive: {joined}"
        );
        assert!(
            joined.contains("259 |           let entry = self"),
            "the borrow's origin line is the other half of the fix: {joined}"
        );
        assert!(
            !joined.contains("For more information"),
            "cargo's trailer is not part of the block: {joined}"
        );
        assert!(
            !joined.contains("unused doc comment"),
            "a warning block is noise beside the error: {joined}"
        );
        assert!(
            joined.contains("could not compile"),
            "the summary line tells the run really failed: {joined}"
        );
    }

    /// A 30 KB port can fail with fifty blocks. The directive is pasted into a
    /// context shared with a 20 K-token window, so the tail is cut — with the
    /// count named, because the loop tells the model to fix *every* error.
    #[test]
    fn error_diagnostics_cap_their_context_footprint_and_say_so() {
        let mut stderr = String::new();
        for i in 0..60 {
            stderr.push_str(&format!(
                "error[E0308]: mismatched types #{i}\n  --> src/big.rs:{}:1\n   |   padding to widen this block and burn the budget quickly\n\n",
                i * 7 + 3
            ));
        }
        let kept = select_error_diagnostics(&stderr);
        let joined = kept.join("\n");
        assert!(
            joined.len() < 7_000,
            "cap must bound the prompt: {} bytes",
            joined.len()
        );
        let marker = kept.last().expect("a truncation marker");
        assert!(marker.contains("omitted"), "got {marker}");
        assert!(
            joined.contains("#0") && joined.contains("error[E0308]"),
            "the first blocks must still be intact: {marker}"
        );
    }

    /// rustc answers a `Mul` mismatch with a catalogue: up to sixty indented
    /// `` `&f64` implements `Mul<f64>` `` lines per error. The fix is in the
    /// caret line two above (`{float} * {integer}` → write `2.0`), the catalogue
    /// is what filled the 6 K budget and pushed the *other* errors into "not
    /// shown" — gateprobe2 spent two of three repairs on that noise.
    #[test]
    fn a_trait_implementor_catalogue_is_dropped_so_the_next_error_stays_visible() {
        let mut stderr = String::from(
            "error[E0277]: cannot multiply `{float}` by `{integer}`\n  \
             --> src\\budget_manager.rs:578:32\n   \
             |\n 578 |             Some(now - 86400.0 * 2),\n    \
             |                                ^ no implementation for `{float} * {integer}`\n   \
             |\n   = help: the trait `Mul<{integer}>` is not implemented for `{float}`\n   \
             = help: the following other types implement trait `Mul<Rhs>`:\n",
        );
        for _ in 0..60 {
            stderr.push_str("            `&f64` implements `Mul<f64>`\n");
        }
        stderr.push_str(
            "and 57 others\n\nerror[E0308]: mismatched types\n  \
             --> src\\budget_manager.rs:640:9\n   |\n 640 |     let s: String = 1u8;\n   \
             |                 ^^^ expected `String`, found `u8`\n",
        );

        let kept = select_error_diagnostics(&stderr);
        let joined = kept.join("\n");
        assert!(
            !joined.contains("implements `Mul<f64>`"),
            "the catalogue itself must not be sent:\n{joined}"
        );
        assert!(
            joined.contains("the other implementors of `Mul<Rhs>` omitted"),
            "and say what happened to it: {joined}"
        );
        assert!(
            joined.contains("is not implemented for `{float}`"),
            "the help line that names the real mismatch stays: {joined}"
        );
        assert!(
            joined.contains("expected `String`, found `u8`"),
            "which is what buys the next error a place in the prompt: {joined}"
        );
        assert!(
            !joined.contains("not shown"),
            "no block may be cut by this: {joined}"
        );
    }

    /// The crate's other modules are not the agent's assignment. A test target
    /// that fails inside `scheduler.rs` must not be handed to a model that only
    /// touched `budget_manager.rs` — it cannot repair what it was not told to
    /// own, and the directive would then push it off its own file.
    #[test]
    fn only_error_blocks_inside_the_written_file_are_kept() {
        let temp = tempfile::TempDir::new().unwrap();
        let crate_dir = temp.path().join("rust_port");
        let mine = "rust_port/src/budget_manager.rs";
        let stderr = "error[E0308]: mismatched types\n  --> src\\budget_manager.rs:20:19\n\
             \x20\x20\x20|\n\
             20 |     let s: String = 1u8;\n\
             \x20\x20\x20|                 ^^^ expected `String`, found `u8`\n\
             \n\
             error[E0425]: cannot find value `x` in this scope\n  --> src\\scheduler.rs:99:5\n\
             \x20\x20\x20|\n\
             99 |     x\n\
             \x20\x20\x20|     ^ not found in this scope\n\
             \n\
             For more information about this error, try `rustc --explain E0308`.\n";
        let kept = retain_own_error_blocks(stderr, temp.path(), &crate_dir, &[mine]);
        assert!(
            kept.contains("budget_manager.rs") && kept.contains("expected `String`, found `u8`"),
            "the agent's own test error must reach it:\n{kept}"
        );
        assert!(
            !kept.contains("scheduler.rs"),
            "a sibling module is not this dispatch's fault:\n{kept}"
        );
        assert!(
            !kept.contains("For more information"),
            "cargo's trailer is not part of the block:\n{kept}"
        );
    }

    /// A batch that writes two modules owns the test code of both. Attributing
    /// only the last write lets the gate print "Compiler validation succeeded!"
    /// over a crate whose `#[cfg(test)]` broke in the file written one call
    /// earlier — the same false green as run 8, reached by a different route.
    #[test]
    fn every_rust_file_the_agent_wrote_is_own_its_own_errors() {
        let temp = tempfile::TempDir::new().unwrap();
        let crate_dir = temp.path().join("rust_port");
        let stderr = "error[E0308]: mismatched types\n  --> src\\budget_manager.rs:20:19\n\
             \x20\x20\x20|\n\
             20 |     let s: String = 1u8;\n\
             \x20\x20\x20|                 ^^^ expected `String`, found `u8`\n\
             \n\
             error[E0425]: cannot find value `x` in this scope\n  --> src\\scheduler.rs:99:5\n\
             \x20\x20\x20|\n\
             99 |     x\n\
             \x20\x20\x20|     ^ not found in this scope\n\n";
        let both = retain_own_error_blocks(
            stderr,
            temp.path(),
            &crate_dir,
            &[
                "rust_port/src/budget_manager.rs",
                "rust_port/src/scheduler.rs",
            ],
        );
        assert!(
            both.contains("budget_manager.rs") && both.contains("scheduler.rs"),
            "both writes are the agent's:\n{both}"
        );
        // And the rule that came before still holds: one of them alone does not
        // license reporting the other.
        let one = retain_own_error_blocks(
            stderr,
            temp.path(),
            &crate_dir,
            &["rust_port/src/scheduler.rs"],
        );
        assert!(
            one.contains("scheduler.rs") && !one.contains("budget_manager.rs"),
            "a file nobody wrote stays out of the report:\n{one}"
        );
    }

    /// A trait the agent wrote in its own file can be implemented badly by a
    /// sibling module, and rustc then prints the sibling's span first and ours
    /// as the `note:`. The repair turn belongs to whoever has to change code, so
    /// only the block's first location decides.
    #[test]
    fn a_block_is_attributed_to_the_file_rustc_addressed_it_at_not_to_its_notes() {
        let temp = tempfile::TempDir::new().unwrap();
        let crate_dir = temp.path().join("rust_port");
        let stderr = "error[E0053]: method `save_data` has an incompatible type for trait\n  --> src\\scheduler.rs:412:13\n   |\n412 |             &mut self,\n   |             ^^^^^^^^^ types differ in mutability\n   |\nnote: type in trait\n  --> src\\budget_manager.rs:125:9\n   |\n125 |         &self,\n   |         ^^^^^\n\n";
        let kept = retain_own_error_blocks(
            stderr,
            temp.path(),
            &crate_dir,
            &["rust_port/src/budget_manager.rs"],
        );
        assert!(
            kept.trim().is_empty(),
            "the fix is in a file this dispatch never wrote:\n{kept}"
        );
    }

    /// An absolute location is the same file, and rustc switches to it for path
    /// dependencies. Comparing strings alone would drop every such error.
    #[test]
    fn an_absolute_location_is_matched_by_the_file_it_resolves_to() {
        let temp = tempfile::TempDir::new().unwrap();
        let crate_dir = temp.path().join("rust_port");
        std::fs::create_dir_all(crate_dir.join("src")).unwrap();
        let file = crate_dir.join("src").join("budget_manager.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        let abs = file.to_string_lossy().replace('/', "\\");
        let stderr = format!(
            "error[E0308]: mismatched types\n  --> {abs}:3:9\n   |\n3 |  bad\n   |   ^^^ expected found\n\n"
        );
        let kept = retain_own_error_blocks(
            &stderr,
            temp.path(),
            &crate_dir,
            &["rust_port/src/budget_manager.rs"],
        );
        assert!(
            kept.contains("mismatched types"),
            "the same file under an absolute path is still the agent's:\n{kept}"
        );
    }

    /// `cargo check --all-targets` prints warnings against the written file
    /// too. They are not compile failures and must not arm a repair turn, so
    /// only blocks that carry an `error` heading are retained.
    #[test]
    fn a_warning_against_the_written_file_is_not_an_error() {
        let temp = tempfile::TempDir::new().unwrap();
        let crate_dir = temp.path().join("rust_port");
        let stderr = "warning: unused variable: `entry`\n  --> src\\budget_manager.rs:262:13\n\
             \x20\x20\x20|\n\
             262 |         let entry = self;\n\
             \x20\x20\x20|             ^^^^^ help: if this is intentional, prefix it with an underscore\n\n";
        let kept = retain_own_error_blocks(
            stderr,
            temp.path(),
            &crate_dir,
            &["rust_port/src/budget_manager.rs"],
        );
        assert!(
            kept.trim().is_empty(),
            "warnings must not fail the gate: {kept}"
        );
    }

    /// Blocks are found by the blank line between them, so a CRLF stream has to
    /// be normalised first — otherwise the whole stderr reads as one block and
    /// its very first location decides the attribution of every error printed.
    #[test]
    fn a_crlf_stderr_is_still_split_into_blocks() {
        let temp = tempfile::TempDir::new().unwrap();
        let crate_dir = temp.path().join("rust_port");
        let stderr = "error[E0308]: mismatched types\r\n  --> src\\scheduler.rs:9:1\r\n   |\r\n9 | x\r\n   | ^\r\n\r\n\
             error[E0277]: cannot multiply\r\n  --> src\\budget_manager.rs:20:19\r\n   |\r\n20 | y\r\n   | ^\r\n\r\n";
        let kept = retain_own_error_blocks(
            stderr,
            temp.path(),
            &crate_dir,
            &["rust_port/src/budget_manager.rs"],
        );
        assert!(
            kept.contains("E0277") && !kept.contains("E0308"),
            "the second block is the agent's, the first is not:\n{kept}"
        );
    }

    /// A gate that cannot attribute its failure stays silent, and a non-Rust
    /// deliverable never pays for a second cargo run at all.
    #[test]
    fn the_test_gate_stays_out_of_the_way_when_it_cannot_judge() {
        let temp = tempfile::TempDir::new().unwrap();
        // Neither call reaches cargo: a markdown deliverable has no test
        // targets, and without a written file there is nothing to attribute a
        // failure to. Both must report Ok rather than invent a build error.
        assert!(
            run_test_code_check(temp.path(), &["notes.md"]).is_ok(),
            "a markdown deliverable has no test targets"
        );
        assert!(
            run_test_code_check(temp.path(), &[]).is_ok(),
            "no write means nothing to attribute"
        );
    }

    /// End to end, against a real toolchain: `cargo check` calls this crate
    /// clean while `cargo check --all-targets` rejects the `#[cfg(test)]` code
    /// the agent itself wrote. Without this the loop prints "Compiler
    /// validation succeeded!" over a deliverable that cannot be tested.
    #[test]
    #[ignore = "spawns cargo; run with --ignored whenever the compile gate changes"]
    fn a_broken_test_module_in_the_written_file_fails_the_gate() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(
            temp.path().join("Cargo.toml"),
            "[package]\nname = \"gate\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            temp.path().join("src").join("lib.rs"),
            "pub fn ok() -> u8 {\n    1\n}\n\n#[cfg(test)]\nmod tests {\n    use super::ok;\n\n    #[test]\n    fn broken() {\n        let s: String = ok();\n        assert!(!s.is_empty());\n    }\n}\n",
        )
        .unwrap();

        assert!(
            run_compilation_check(temp.path()).is_ok(),
            "cargo check must pass, or this proves nothing"
        );
        let err = run_test_code_check(temp.path(), &["src/lib.rs"])
            .expect_err("the crate's own test code does not compile");
        assert!(
            err.contains("E0308"),
            "the gate must hand back the rustc error: {err}"
        );
        assert!(
            err.contains("src/lib.rs") || err.contains("src\\lib.rs"),
            "and the file it belongs to: {err}"
        );
    }

    /// libtest's failure report, as cargo printed it for the run-6 probe: two
    /// assertions in the file the agent wrote, then the harness's own summary
    /// repeating the names.
    const TWO_FAILURES: &str = "running 70 tests\ntest budget_manager::tests::test_reset ... ok\n\
         test scheduler::tests::test_peek_empty_queue_errors ... ok\n\n\
         failures:\n\n\
         ---- budget_manager::tests::test_projected_cost stdout ----\n\n\
         thread 'budget_manager::tests::test_projected_cost' (6412) panicked at src\\budget_manager.rs:535:9:\n\
         assertion failed: (projected - 0.009).abs() < f64::EPSILON\n\
         note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n\n\
         ---- budget_manager::tests::test_model_cost_breakdown stdout ----\n\n\
         thread 'budget_manager::tests::test_model_cost_breakdown' (20100) panicked at src\\budget_manager.rs:662:9:\n\
         assertion failed: (mc.get(\"gpt-4\").copied().unwrap() - 0.004).abs() < f64::EPSILON\n\n\
         failures:\n    budget_manager::tests::test_model_cost_breakdown\n    budget_manager::tests::test_projected_cost\n\n\
         test result: FAILED. 68 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out;\n";

    /// A failing test is only actionable as a NAME plus the ASSERTION that did
    /// not hold. The summary list repeats names without either, and the
    /// backtrace hint is not the message — both must be told apart.
    #[test]
    fn the_test_parser_reads_names_lines_and_the_assertion_that_failed() {
        let failures = parse_test_failures(TWO_FAILURES);
        assert_eq!(failures.len(), 2, "{:?} ", failures.len());
        assert_eq!(
            failures[0].name,
            "budget_manager::tests::test_projected_cost"
        );
        assert_eq!(failures[0].line, 535);
        assert_eq!(failures[0].location, "src\\budget_manager.rs");
        assert!(
            failures[0]
                .message
                .starts_with("assertion failed: (projected - 0.009)"),
            "{}",
            failures[0].message
        );
        assert!(
            !failures[0].message.contains("RUST_BACKTRACE"),
            "the harness's hint is not the assertion: {}",
            failures[0].message
        );
        assert_eq!(failures[1].line, 662);
    }

    /// A panic location has to be recognised whether the toolchain printed it
    /// crate-relative with Windows separators or as an absolute path.
    #[test]
    fn a_panic_is_attributed_to_the_written_file_in_either_path_spelling() {
        let temp = tempfile::TempDir::new().unwrap();
        let crate_dir = temp.path().join("rust_port");
        std::fs::create_dir_all(crate_dir.join("src")).unwrap();
        let file = crate_dir.join("src").join("budget_manager.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        let written = ["rust_port/src/budget_manager.rs"];

        let abs = file.to_string_lossy().replace('/', "\\");
        let both = format!(
            "failures:\n\n---- a::relative stdout ----\n\nthread 'a' panicked at src\\budget_manager.rs:5:1:\n\
             x\n\n---- a::absolute stdout ----\n\nthread 'a' panicked at {abs}:9:1:\ny\n\nfailures:\n"
        );
        let owned = own_test_failures(&both, temp.path(), &crate_dir, &written);
        assert_eq!(owned.len(), 2, "both spellings name the same file");
        assert!(owned.iter().all(|f| f.name.starts_with("a::")));

        let sibling = "---- scheduler::tests::wait stdout ----\n\nthread 'x' panicked at src\\scheduler.rs:44:9:\n\
             boom\n\nfailures:\n";
        assert!(
            own_test_failures(sibling, temp.path(), &crate_dir, &written).is_empty(),
            "a module the agent never wrote is not its failure to repair"
        );
    }

    /// The gate must say what it saw, not guess.
    #[test]
    fn the_test_gate_reports_its_own_verdict() {
        assert!(is_test_failure_report(&format!(
            "{TEST_FAILURE_PREAMBLE} rest of the report"
        )));
        assert!(!is_test_failure_report(
            "error[E0308]: mismatched types\n  --> src/lib.rs:3:9"
        ));
        let summary = test_result_summary(
            "test result: ok. 70 passed; 0 failed; 0 ignored\nother noise\ntest result: ok. 3 passed; 0 failed\n",
        );
        assert_eq!(
            summary,
            "test result: ok. 70 passed; 0 failed; 0 ignored / test result: ok. 3 passed; 0 failed"
        );
        assert!(test_result_summary("nothing").contains("no result line"));
    }

    /// A non-Rust deliverable and an empty write list both abstain, without ever
    /// spawning cargo.
    #[test]
    fn the_test_gate_abstains_when_no_rust_was_written() {
        let temp = tempfile::TempDir::new().unwrap();
        for writes in [&["notes.md"][..], &[][..]] {
            let verdict = run_written_tests(temp.path(), writes).unwrap();
            assert!(
                matches!(verdict, TestVerdict::NotJudged(_)),
                "no Rust file, no test run: {verdict:?}"
            );
        }
    }

    /// End to end on a real toolchain: the build is clean, the test code builds,
    /// and the assertion still fails — the exact state probe run 6 reported as
    /// "Compiler validation succeeded!". Fixing the library has to flip the
    /// verdict, and the report has to carry the line the model must edit.
    #[test]
    #[ignore = "spawns cargo; run with --ignored whenever the test gate changes"]
    fn a_passing_build_with_a_failing_test_fails_the_gate() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(
            temp.path().join("Cargo.toml"),
            "[package]\nname = \"tgate\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        let lib = temp.path().join("src").join("lib.rs");
        std::fs::write(
            &lib,
            "pub fn projected(current: f64, prompt: f64) -> f64 {\n    current + prompt\n}\n\n\
             #[cfg(test)]\nmod tests {\n    use super::projected;\n\n    #[test]\n    fn adds_the_two_costs() {\n        \
             assert!((projected(0.002, 0.002) - 0.009).abs() < f64::EPSILON);\n    }\n}\n",
        )
        .unwrap();

        let err = run_written_tests(temp.path(), &["src/lib.rs"])
            .expect_err("the crate's own assertion does not hold");
        assert!(is_test_failure_report(&err), "{err}");
        assert!(err.contains("adds_the_two_costs"), "{err}");
        assert!(err.contains("--> src/lib.rs:11"), "{err}");
        assert!(err.contains("assertion failed"), "{err}");
        assert!(err.contains("test result: FAILED"), "{err}");

        std::fs::write(
            &lib,
            "pub fn projected(current: f64, prompt: f64) -> f64 {\n    current + prompt\n}\n\n\
             #[cfg(test)]\nmod tests {\n    use super::projected;\n\n    #[test]\n    fn adds_the_two_costs() {\n        \
             assert!((projected(0.002, 0.002) - 0.004).abs() < 1e-12);\n    }\n}\n",
        )
        .unwrap();
        let verdict = run_written_tests(temp.path(), &["src/lib.rs"]).unwrap();
        match verdict {
            TestVerdict::Passed(summary) => assert!(summary.contains("1 passed"), "{summary}"),
            other => panic!("the tests ran and were green, got {other:?}"),
        }
    }
}
