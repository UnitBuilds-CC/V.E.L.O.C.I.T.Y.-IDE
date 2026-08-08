use super::super::coordination::CoordinationBus;
use super::super::models::*;
use super::super::provider::*;
use super::loop_runner::run_agent_reasoning_loop;
use super::utils::build_inline_tool_docs;
use crate::editor::speculative_precomp::precompute_files;
use crate::safety::SafeMutex;
use crate::usage::{
    load_accounts, load_azure_accounts, load_local_ollama_accounts, load_openrouter_accounts,
    UsageTracker,
};
use std::sync::{Arc, Mutex};

pub fn run_headless_subagent(request: HeadlessSubAgentRequest) -> HeadlessSubAgentResult {
    let accounts = load_accounts(&request.workspace_root);
    let or_accounts = load_openrouter_accounts(&request.workspace_root);
    let azure_accounts = load_azure_accounts(&request.workspace_root);
    let ollama_accounts = load_local_ollama_accounts(&request.workspace_root);
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
        _ => default_model_info(&request.model),
    };
    let thinking = request.thinking && selected_profile.supports_thinking;

    // T2a: Speculative pre-computation - pre-index scoped files before agent runs
    let scoped_files = request.scoped_files.clone().unwrap_or_default();
    let precomp_context = if !scoped_files.is_empty() {
        let precomp_result = precompute_files(&request.workspace_root, &scoped_files);
        format!("\n\n## Pre-indexed Workspace Context\n{}", precomp_result.context_summary())
    } else {
        String::new()
    };

    let use_inline_tools =
        request.provider == AiProvider::OpenRouter || !selected_profile.supports_tools;
    let mut message_history = vec![ChatMessage {
        role: "system".to_string(),
        content: format!(
            "You are Antigravity, a high-performance agent running directly in V.E.L.O.C.I.T.Y.-IDE. \
            You have access to local workspace files and execution sandboxes via tools. \
            Help the user program the workspace. Always output concise, correct, and high-quality responses.{}{}",
            if use_inline_tools { build_inline_tool_docs() } else { String::new() },
            precomp_context
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

    let status_updates_collector = status_updates.clone();
    let transcript_collector = transcript.clone();
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
                        let mut progress = progress.lock_safe();
                        progress.status_updates.push(status.clone());
                        progress.events.push(HeadlessSubAgentEvent {
                            kind: HeadlessSubAgentEventKind::Status,
                            message: status,
                        });
                    }
                }
                AgentToUiMessage::OutputToken(token) | AgentToUiMessage::ThoughtToken(token) => {
                    transcript_collector.lock_safe().push_str(&token);
                    if let Some(progress) = &progress_collector {
                        let mut progress = progress.lock_safe();
                        progress.transcript.push_str(&token);
                        progress.events.push(HeadlessSubAgentEvent {
                            kind: HeadlessSubAgentEventKind::Transcript,
                            message: token,
                        });
                    }
                }
                AgentToUiMessage::UpdateFileBuffer { path, .. } => {
                    if let Some(progress) = &progress_collector {
                        let changed_path = path.display().to_string();
                        let mut progress = progress.lock_safe();
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
                        .lock_safe()
                        .push(status.clone());
                    if let Some(progress) = &progress_collector {
                        let mut progress = progress.lock_safe();
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
                        arguments,
                    });
                }
                AgentToUiMessage::ToolExecutionStarted { tool_name } => {
                    if let Some(progress) = &progress_collector {
                        progress.lock_safe().events.push(HeadlessSubAgentEvent {
                            kind: HeadlessSubAgentEventKind::ToolStarted,
                            message: tool_name,
                        });
                    }
                }
                AgentToUiMessage::ToolExecutionFinished { tool_name, result } => {
                    if let Some(progress) = &progress_collector {
                        progress.lock_safe().events.push(HeadlessSubAgentEvent {
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
        &azure_accounts,
        &ollama_accounts,
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
        &mut Vec::new(),
        &CoordinationBus::new(),
    );

    drop(agent_event_tx);
    let _ = collector.join();

    let status_updates = status_updates.lock_safe().clone();
    let transcript = transcript.lock_safe().clone();
    HeadlessSubAgentResult {
        status_updates,
        transcript,
    }
}
