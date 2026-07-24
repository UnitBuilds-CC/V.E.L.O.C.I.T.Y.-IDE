use super::super::models::*;
use super::super::provider::{default_model_info, enrich_model_profile, openrouter_api_key};
use super::loop_runner::run_agent_reasoning_loop;
use crate::editor::expert_team::ExpertTeam;
use crate::editor::skill_file::{is_builtin_skill, load_skill_file};
use crate::editor::team_router::{parse_team_directive, resolve_team, route_member};
use crate::usage::*;
use crossbeam_channel::{Receiver, Sender};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

/// Build the execution profile for a routed member, mirroring the way the
/// interactive session builds its own profile (tools enabled where possible).
fn build_member_profile(
    provider: AiProvider,
    model: &str,
    accounts: &[CloudflareAccount],
) -> ModelInfo {
    match provider {
        AiProvider::OpenRouter => ModelInfo {
            id: model.to_string(),
            label: model.rsplit('/').next().unwrap_or(model).to_string(),
            api_style: ApiStyle::OpenAiTools,
            supports_tools: true,
            supports_thinking: false,
        },
        AiProvider::CloudflareWorkersAi => {
            let profile = default_model_info(model);
            enrich_model_profile(accounts, &profile)
        }
        _ => default_model_info(model),
    }
}

/// One-shot, non-streaming completion used only by the LLM router fallback.
/// Kept intentionally minimal (no usage tracking) so it can run inside an
/// immutable `Fn` closure.
#[allow(clippy::too_many_arguments)]
fn router_completion(
    provider: AiProvider,
    model: &str,
    accounts: &[CloudflareAccount],
    or_accounts: &[OpenRouterAccount],
    azure_accounts: &[AzureOpenAiAccount],
    ollama_accounts: &[LocalOllamaAccount],
    system: &str,
    user: &str,
) -> Option<String> {
    let body = json!({
        "model": model,
        "stream": false,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ]
    });

    let response = match provider {
        AiProvider::OpenRouter => {
            let key = or_accounts
                .first()
                .map(|a| a.token.clone())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(openrouter_api_key);
            ureq::post("https://openrouter.ai/api/v1/chat/completions")
                .timeout(Duration::from_secs(30))
                .set("Authorization", &format!("Bearer {}", key))
                .set("Content-Type", "application/json")
                .send_json(&body)
                .ok()
        }
        AiProvider::CloudflareWorkersAi => {
            let account = accounts.first()?;
            let url = format!(
                "https://api.cloudflare.com/client/v4/accounts/{}/ai/v1/chat/completions",
                account.id
            );
            ureq::post(&url)
                .timeout(Duration::from_secs(30))
                .set("Authorization", &format!("Bearer {}", account.token))
                .set("Content-Type", "application/json")
                .send_json(&body)
                .ok()
        }
        AiProvider::AzureOpenAi => {
            let account = azure_accounts.first()?;
            let endpoint = account.endpoint.trim_end_matches('/');
            let url = format!(
                "{}/openai/deployments/{}/chat/completions?api-version={}",
                endpoint, account.deployment, account.api_version
            );
            ureq::post(&url)
                .timeout(Duration::from_secs(30))
                .set("api-key", &account.api_key)
                .set("Content-Type", "application/json")
                .send_json(&body)
                .ok()
        }
        AiProvider::LocalOllama => {
            let host = ollama_accounts
                .first()
                .map(|a| a.host.as_str())
                .unwrap_or("http://localhost:11434");
            let url = format!("{}/v1/chat/completions", host.trim_end_matches('/'));
            ureq::post(&url)
                .timeout(Duration::from_secs(30))
                .set("Content-Type", "application/json")
                .send_json(&body)
                .ok()
        }
        _ => None,
    }?;

    let text = response.into_string().ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Compose the per-member persona system message: role + workflow instructions
/// + attached skill-file bodies + scope + optional tool allow-list note.
fn compose_persona(team: &ExpertTeam, member_idx: usize, workspace_root: &PathBuf) -> String {
    let member = &team.members[member_idx];
    let mut persona = format!(
        "You are {}, the {} on the \"{}\" team. Focus strictly on your specialty and complete the routed task end-to-end.",
        member.name, member.role, team.name
    );

    if !member.workflow_instructions.trim().is_empty() {
        persona.push_str("\n\nOperating instructions:\n");
        persona.push_str(member.workflow_instructions.trim());
    }

    for skill_id in &member.skills {
        if is_builtin_skill(skill_id) {
            continue;
        }
        if let Some(skill) = load_skill_file(workspace_root, skill_id) {
            if !skill.body.trim().is_empty() {
                persona.push_str(&format!("\n\n## Skill: {}\n{}", skill.name, skill.body.trim()));
            }
        }
    }

    if !member.scope_patterns.is_empty() {
        persona.push_str(&format!(
            "\n\nYour scope of responsibility (paths/areas): {}",
            member.scope_patterns.join(", ")
        ));
    }

    if !member.tools.is_empty() {
        persona.push_str(&format!(
            "\n\nRestrict tool usage to only these tools: {}. Do not call any other tool.",
            member.tools.join(", ")
        ));
    }

    persona
}

/// Attempt to interpret `prompt` as a team directive and, if a team resolves,
/// route the task to the best member and execute it with that member's
/// provider/model/persona. Returns `true` when the prompt was handled here.
#[allow(clippy::too_many_arguments)]
pub fn try_route_team_prompt(
    prompt: &str,
    teams: &[ExpertTeam],
    workspace_root: &PathBuf,
    accounts: &[CloudflareAccount],
    or_accounts: &[OpenRouterAccount],
    azure_accounts: &[AzureOpenAiAccount],
    ollama_accounts: &[LocalOllamaAccount],
    session_provider: AiProvider,
    session_model: &str,
    thinking: bool,
    message_history: &mut Vec<ChatMessage>,
    usage_tracker: &mut UsageTracker,
    ui_rx: &Receiver<UiToAgentMessage>,
    ui_tx: &Sender<AgentToUiMessage>,
    deferred_messages: &mut Vec<UiToAgentMessage>,
) -> bool {
    let Some(directive) = parse_team_directive(prompt) else {
        return false;
    };
    let Some(team_idx) = resolve_team(teams, &directive.team_query) else {
        ui_tx
            .send(AgentToUiMessage::StatusUpdate(format!(
                "No team matched \"{}\"; handling as a normal prompt.",
                directive.team_query
            )))
            .ok();
        return false;
    };
    let team = &teams[team_idx];
    if team.members.is_empty() {
        ui_tx
            .send(AgentToUiMessage::StatusUpdate(format!(
                "Team \"{}\" has no members; handling as a normal prompt.",
                team.name
            )))
            .ok();
        return false;
    }

    let task = if directive.task.trim().is_empty() {
        prompt.trim().to_string()
    } else {
        directive.task.clone()
    };

    // LLM router fallback closure (only consulted when keyword/scope is weak).
    let roster: Vec<Value> = team
        .members
        .iter()
        .map(|m| {
            json!({
                "id": m.id,
                "name": m.name,
                "role": m.role,
                "scope": m.scope_patterns,
            })
        })
        .collect();
    let roster_text = serde_json::to_string_pretty(&roster).unwrap_or_default();
    let router = |task: &str| -> Option<String> {
        let system = format!(
            "You are a routing assistant for the \"{}\" team. Choose the single best member for the task. \
             Respond with ONLY that member's id from the list, nothing else.\n\nMembers:\n{}",
            team.name, roster_text
        );
        router_completion(
            session_provider,
            session_model,
            accounts,
            or_accounts,
            azure_accounts,
            ollama_accounts,
            &system,
            task,
        )
    };

    let Some(routed) = route_member(team, &task, &[], Some(&router)) else {
        return false;
    };
    let Some(member_idx) = team.members.iter().position(|m| m.id == routed.member_id) else {
        return false;
    };
    let member = &team.members[member_idx];

    let (member_provider, member_model) =
        member.resolve_effective_provider_and_model(session_provider, session_model);
    let member_profile = build_member_profile(member_provider, &member_model, accounts);
    let member_thinking = thinking && member_profile.supports_thinking;

    ui_tx
        .send(AgentToUiMessage::StatusUpdate(format!(
            "Team {} -> {} ({}/{}): {}",
            team.name,
            member.name,
            member_provider.slug(),
            member_model,
            routed.reason
        )))
        .ok();

    let persona = compose_persona(team, member_idx, workspace_root);
    message_history.push(ChatMessage {
        role: "system".to_string(),
        content: persona,
        name: None,
        tool_call_id: None,
        tool_calls: None,
    });
    message_history.push(ChatMessage {
        role: "user".to_string(),
        content: task,
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
        &member_model,
        &member_profile,
        member_provider,
        member_thinking,
        message_history,
        usage_tracker,
        ui_rx,
        None,
        None,
        ui_tx,
        deferred_messages,
    );

    true
}
