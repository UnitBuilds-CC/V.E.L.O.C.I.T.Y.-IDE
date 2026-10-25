use super::super::coordination::CoordinationBus;
use super::super::models::*;
use super::super::provider::*;
use super::loop_runner::run_agent_reasoning_loop;
use super::utils::{build_inline_tool_docs, SYSTEM_PROMPT_BASE};
use crate::editor::speculative_precomp::precompute_files;
use crate::safety::SafeMutex;
use crate::usage::{
    load_accounts, load_azure_accounts, load_local_ollama_accounts, load_openrouter_accounts,
    UsageTracker,
};
use std::sync::{Arc, Mutex};

/// Last-resort wall-clock backstop for a headless run nobody is watching.
///
/// This is deliberately NOT the primary limiter any more. A local model does
/// not run at cloud throughput — measured, `qwen3.6:35b-a3b` resident on one
/// 8 GB GPU shared by four parallel specialists produced ~6 tok/s, so a
/// legitimate first port generation ran for many minutes and a 900 s cap
/// killed work that was alive, merely slow, never stalled. Normal dead ends are
/// now caught much sooner and more honestly by [`stall_window`] (no output for
/// a window) and by the loop's repeat/no-action detectors (a structural loop,
/// not a clock). This value only has to be large enough to stop a pathological
/// run that keeps trickling bytes yet never finishes logically.
///
/// Shared by every unattended caller (`team_dispatch`, workflow agent steps,
/// orchestrator workers) so one knob bounds them all. An interactive session
/// passes `None` instead — a human at the keyboard is the deadline there.
pub fn unattended_budget() -> std::time::Duration {
    // A backstop, not a target: 3600 s is far above any single attempt that is
    // actually making progress, so it should never be the thing that ends a
    // healthy run — a stall or a loop fires first.
    const DEFAULT_SECS: u64 = 3600;
    std::env::var("VELOCITY_DISPATCH_BUDGET_SECS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        // Zero or garbage falls back to the default: a zero budget would abort
        // a run before it completed a single turn.
        .filter(|secs| *secs > 0)
        .map(std::time::Duration::from_secs)
        .unwrap_or_else(|| std::time::Duration::from_secs(DEFAULT_SECS))
}

/// How long a run may go with no streamed output before the generation is
/// declared stalled rather than merely slow.
///
/// The loop's per-token read waits at most this long between lines, so the
/// clock measures *time since the last token* — a model still producing
/// tokens is never cut off, however long the whole message takes, while a
/// wedged GPU/stream is abandoned within one window instead of at the backstop.
/// Sized generously (300 s) because a reasoning model under GPU contention can
/// spend minutes composing a single large `write_file` payload before any of it
/// reaches the socket. Zero or garbage disables the stall check (the run then
/// relies only on the loop's repeat detectors and the backstop).
pub fn stall_window() -> Option<std::time::Duration> {
    const DEFAULT_SECS: u64 = 300;
    match std::env::var("VELOCITY_STALL_WINDOW_SECS") {
        Ok(raw) => match raw.trim().parse::<u64>() {
            // An explicitly-zero window means "no stall check", which is a
            // legitimate operator choice, unlike the budget's zero-means-default.
            Ok(0) => None,
            Ok(secs) => Some(std::time::Duration::from_secs(secs)),
            Err(_) => Some(std::time::Duration::from_secs(DEFAULT_SECS)),
        },
        Err(_) => Some(std::time::Duration::from_secs(DEFAULT_SECS)),
    }
}

/// How many times an orchestrator worker re-issues a route after the loop ends
/// it on a *restartable* dead end (a stall or a structural loop, reported as
/// `Blocked`), resuming from the continuation ledger rather than the clock.
///
/// Bounded so a task the model genuinely cannot do cannot restart forever; the
/// [`unattended_budget`] backstop still caps the total across every restart.
pub fn max_restarts() -> u32 {
    const DEFAULT: u32 = 2;
    std::env::var("VELOCITY_MAX_RESTARTS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .unwrap_or(DEFAULT)
}

/// What is left of `total` once `elapsed` has been spent.
///
/// Pure so the arithmetic is testable without a clock. `None` means "nothing
/// left — do not start another run", which is why callers must never forward
/// `None` to [`HeadlessSubAgentRequest::budget`]: there it means the exact
/// opposite ("no deadline"), and silently turning "out of time" into "run
/// forever" is the bug this exists to prevent.
pub fn budget_left(
    total: std::time::Duration,
    elapsed: std::time::Duration,
) -> Option<std::time::Duration> {
    total.checked_sub(elapsed).filter(|left| !left.is_zero())
}

/// How many turns one unattended attempt may take, from the seconds left to it.
///
/// Shared by every unattended dispatcher (`team_dispatch`, orchestrator
/// workers) for the same reason [`unattended_budget`] is: a fixed count is a
/// wall-clock cap in disguise, and which one it is depends on the model's speed.
/// The 16 `team_dispatch` used was measured against `qwen3.6:35b-a3b` at
/// 130-235 s a turn; a GPU-resident 7B answers in 1-6 s, and `lr7b_v6` hit 16
/// having spent 46 of its 1200 seconds — stopped with a compile repair still
/// unspent, one edit from a clean build. The budget is what should end an
/// attempt, not the model's speed.
///
/// The floor keeps a slow model's behavior identical (the deadline stops it
/// first), and the ceiling stops a run that only ever reads from running for the
/// whole budget. The ~20 s per turn is the assumption behind both bounds: below
/// it the count cannot bind, above it the deadline does.
pub fn turn_ceiling_for(attempt_budget: std::time::Duration) -> usize {
    (attempt_budget.as_secs() / 20).clamp(16, 48) as usize
}

pub fn run_headless_subagent(request: HeadlessSubAgentRequest) -> HeadlessSubAgentResult {
    // Resolved once, here, so the deadline covers the whole run including
    // pre-indexing — not just the turns. `checked_add` because an absurd budget
    // must degrade to "no deadline" rather than panic on overflow.
    let deadline = request
        .budget
        .and_then(|budget| std::time::Instant::now().checked_add(budget));
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
        format!(
            "\n\n## Pre-indexed Workspace Context\n{}",
            precomp_result.context_summary()
        )
    } else {
        String::new()
    };

    let use_inline_tools =
        request.provider == AiProvider::OpenRouter || !selected_profile.supports_tools;
    let mut message_history = vec![ChatMessage {
        role: "system".to_string(),
        content: format!(
            "{}{}{}",
            SYSTEM_PROMPT_BASE,
            if use_inline_tools {
                build_inline_tool_docs()
            } else {
                String::new()
            },
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
                    status_updates_collector.lock_safe().push(status.clone());
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
                    status_updates_collector.lock_safe().push(status.clone());
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
                    let _ = auto_approve_tx.send(UiToAgentMessage::ApproveTool { id, arguments });
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

    let outcome = run_agent_reasoning_loop(
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
        None, // Headless sub-agents use direct dispatch, not MoA router
        request.max_turns,
        deadline,
        stall_window(),
        request.requires_artifact,
    );

    drop(agent_event_tx);
    let _ = collector.join();

    let status_updates = status_updates.lock_safe().clone();
    let transcript = transcript.lock_safe().clone();
    HeadlessSubAgentResult {
        status_updates,
        transcript,
        outcome,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn an_exhausted_budget_reads_as_none_so_callers_stop_instead_of_running_unbounded() {
        let total = Duration::from_secs(10);
        assert_eq!(
            budget_left(total, Duration::from_secs(4)),
            Some(Duration::from_secs(6))
        );
        // Exactly spent is `None` too: a zero-length budget handed to the loop
        // would abort the run before its first turn, which looks like a model
        // failure rather than a timeout.
        assert_eq!(budget_left(total, total), None);
        assert_eq!(budget_left(total, Duration::from_secs(11)), None);
    }

    #[test]
    fn the_turn_ceiling_scales_with_the_seconds_left_to_the_attempt() {
        // 16 was the fixed count `team_dispatch` used, calibrated on
        // qwen3.6:35b-a3b at 130-235 s a turn; the worker's 20 had the same
        // problem. lr7b_v6 ran a real budget on a GPU-resident 7B at 2-6 s a turn,
        // stopped at the count with 1154 of its 1200 seconds unspent and a compile
        // repair still owed — the budget, not the count, is what should end an
        // attempt.
        assert_eq!(turn_ceiling_for(Duration::from_secs(1200)), 48);
        // The floor keeps a slow model's behavior identical: at 15 turns' worth of
        // seconds the deadline stops that attempt before the count ever could.
        assert_eq!(turn_ceiling_for(Duration::from_secs(300)), 16);
        assert_eq!(turn_ceiling_for(Duration::from_secs(20)), 16);
        // And the ceiling bounds the other direction — an hour of budget must not
        // license a run that only ever reads files.
        assert_eq!(turn_ceiling_for(Duration::from_secs(3600)), 48);
    }

    #[test]
    fn the_unattended_budget_is_never_zero() {
        // Whatever the environment says, the value returned must be usable as a
        // deadline: zero would make every unattended run time out immediately.
        assert!(!unattended_budget().is_zero());
    }
}
