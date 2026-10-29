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

/// A wall-clock backstop of last resort, when the operator set none explicitly.
///
/// A backstop, not a target: 3600 s is far above any single attempt that is
/// actually making progress, so it should never be the thing that ends a healthy
/// run — a stall or a loop fires first.
const DEFAULT_BACKSTOP_SECS: u64 = 3600;

/// How an unattended run's wall-clock backstop is configured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackstopSetting {
    /// The variable is absent or unreadable, so the default applies.
    Unset,
    /// The operator deliberately removed the clock.
    Unbounded,
    /// The operator named a number of seconds.
    Seconds(u64),
}

/// Read `VELOCITY_DISPATCH_BUDGET_SECS` without pretending a typo is a choice.
///
/// Split out from [`unattended_budget`] so tests can exercise every spelling
/// without mutating the environment the rest of the process reads — the same
/// reason the loop's other budget helpers are pure.
fn parse_backstop_setting(raw: &str) -> BackstopSetting {
    use super::utils::{parse_limit_setting, LimitSetting};
    // An explicit `0`/`off`/`unlimited` means no clock, which is the operator's
    // decision — unlike a typo, which falls back to the default rather than
    // silently launching unbounded runs. Same reading as every other knob the
    // loop exposes, so the accepted spellings live in one parser.
    match parse_limit_setting(Some(raw)) {
        LimitSetting::Default => BackstopSetting::Unset,
        LimitSetting::Unbounded => BackstopSetting::Unbounded,
        LimitSetting::Value(secs) => BackstopSetting::Seconds(secs),
    }
}

/// What an unattended caller sets as its wall-clock backstop, if anything.
///
/// `None` is a real, operator-chosen value — it means "this run has no clock and
/// ends only when the loop ends it" — so it must never be conflated with `Some`
/// and never be produced by arithmetic failure. Two measured cases drove this:
/// a LiteLLM port whose single legitimate `write_file` turn needed 1999 s ran to
/// completion at 1800 s and 3600 s ceilings, and an operator who asked to be
/// able to let one task run as long as it has to. The loop's own limiters are
/// what should end a run — [`stall_window`] (no streamed output for a window),
/// the repeat/no-action detectors, and the artifact gates — not elapsed seconds.
///
/// Shared by every unattended caller (`team_dispatch`, workflow agent steps,
/// orchestrator workers) so one knob bounds them all. An interactive session
/// passes `None` instead — a human at the keyboard is the deadline there.
pub fn unattended_budget() -> Option<std::time::Duration> {
    let setting = std::env::var("VELOCITY_DISPATCH_BUDGET_SECS")
        .ok()
        .map(|raw| parse_backstop_setting(&raw))
        .unwrap_or(BackstopSetting::Unset);
    match setting {
        BackstopSetting::Unbounded => None,
        BackstopSetting::Seconds(secs) => Some(std::time::Duration::from_secs(secs)),
        BackstopSetting::Unset => Some(std::time::Duration::from_secs(DEFAULT_BACKSTOP_SECS)),
    }
}

/// What one fresh unattended attempt is allowed to run for, given what earlier
/// attempts of the same dispatch already spent.
///
/// Two variants, deliberately: the shape this replaces was `Option<Duration>`
/// where `None` meant "the clock is gone", while the very same `None` handed to
/// [`HeadlessSubAgentRequest::budget`] means "there is no deadline". A caller
/// that forwarded the first as the second turned a timeout into an infinite run,
/// which is why "spent" is not representable here — see [`attempt_budget`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttemptBudget {
    /// No backstop is configured; the attempt ends on the loop's own signals.
    Unbounded,
    /// `left` remains of the backstop, for *this* attempt alone. Handing the
    /// original total to every attempt lets a failover chain multiply the
    /// ceiling by its own length.
    Remaining(std::time::Duration),
}

impl AttemptBudget {
    /// The value for `HeadlessSubAgentRequest::budget`.
    ///
    /// `Unbounded` becomes `None` — genuinely "no deadline" — which is safe to
    /// express here and nowhere else, because a spent clock never reaches this
    /// type.
    pub fn deadline(self) -> Option<std::time::Duration> {
        match self {
            AttemptBudget::Unbounded => None,
            AttemptBudget::Remaining(left) => Some(left),
        }
    }
}

/// [`AttemptBudget`] for a new attempt, or `None` when the backstop is spent and
/// no further attempt may start.
///
/// Pure so the arithmetic is testable without a clock. `total: None` (no
/// backstop) always yields [`AttemptBudget::Unbounded`] — an unbounded dispatch
/// never runs out of time, by construction.
pub fn attempt_budget(
    total: Option<std::time::Duration>,
    elapsed: std::time::Duration,
) -> Option<AttemptBudget> {
    match total {
        None => Some(AttemptBudget::Unbounded),
        Some(total) => total
            .checked_sub(elapsed)
            // Exactly spent is "no time left" too: a zero-length budget handed
            // to the loop would abort the run before its first turn, which reads
            // as a model failure instead of a timeout.
            .filter(|left| !left.is_zero())
            .map(AttemptBudget::Remaining),
    }
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
            // An explicitly-zero window means "no stall check" — a legitimate
            // operator choice, and the only limiter left for a run that also
            // removed the wall-clock backstop.
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
/// Bounded so a task the model genuinely cannot do cannot restart forever; with
/// a backstop configured it also caps the total across every restart, and with
/// none the count alone is what bounds the re-issue loop.
pub fn max_restarts() -> u32 {
    const DEFAULT: u32 = 2;
    std::env::var("VELOCITY_MAX_RESTARTS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .unwrap_or(DEFAULT)
}

/// The highest turn count a time-boxed attempt is ever handed.
const MAX_BOUNDED_TURN_CEILING: usize = 48;

/// The turn count for a dispatch that sets no wall-clock backstop.
pub const UNBOUNDED_TURN_CEILING: usize = 512;

// Checked where the number is defined rather than in a test that has to be run:
// an unbounded run's turn count is a sanity bound, and if it ever sat at or below
// what a time-boxed attempt gets, removing the clock would install a *tighter*
// ceiling than keeping it — the opposite of what the operator asked for.
const _: () = assert!(
    UNBOUNDED_TURN_CEILING > MAX_BOUNDED_TURN_CEILING,
    "the unbounded ceiling must not become the new binding limit"
);

/// How many turns one unattended attempt may take, from its budget.
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
pub fn turn_ceiling_for(attempt_budget: AttemptBudget) -> usize {
    match attempt_budget {
        AttemptBudget::Remaining(left) => {
            (left.as_secs() / 20).clamp(16, MAX_BOUNDED_TURN_CEILING as u64) as usize
        }
        // No clock, so the count has to be a sanity bound rather than a limit:
        // high enough that no real task reaches it (a 3600 s dispatch, the
        // default backstop, is 48 turns) and finite because with no deadline the
        // loop's other long-run guard is offline — `wasted_time_ends_the_run`
        // only fires when `budget_secs > 0`, so an unbounded run that trickles
        // fresh bytes while achieving nothing is stopped only here and by the
        // stall window.
        AttemptBudget::Unbounded => UNBOUNDED_TURN_CEILING,
    }
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

    // The dispatch's ramble tally as this hop received it. The loop adds what it
    // pays for, and the hop hands the total back, so a fallback chain bounds the
    // prose allowance once for the run instead of once per candidate.
    let mut ramble = request.ramble;
    let use_inline_tools =
        request.provider == AiProvider::OpenRouter || !selected_profile.supports_tools;
    // Owned here, so a group the model unlocks mid-run changes the prompt this
    // run rebuilds on every later turn but never leaks back to the caller's
    // template.
    let mut tool_surface = request.tool_surface.clone();
    let mut message_history = vec![ChatMessage {
        role: "system".to_string(),
        content: format!(
            "{}{}{}",
            SYSTEM_PROMPT_BASE,
            if use_inline_tools {
                build_inline_tool_docs(&tool_surface)
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
        &mut tool_surface,
        &mut ramble,
    );

    drop(agent_event_tx);
    let _ = collector.join();

    let status_updates = status_updates.lock_safe().clone();
    let transcript = transcript.lock_safe().clone();
    HeadlessSubAgentResult {
        status_updates,
        transcript,
        outcome,
        ramble,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn an_exhausted_budget_reads_as_none_so_callers_stop_instead_of_running_unbounded() {
        let total = Some(Duration::from_secs(10));
        assert_eq!(
            attempt_budget(total, Duration::from_secs(4)),
            Some(AttemptBudget::Remaining(Duration::from_secs(6)))
        );
        // Exactly spent is `None` too: a zero-length budget handed to the loop
        // would abort the run before its first turn, which looks like a model
        // failure rather than a timeout.
        assert_eq!(attempt_budget(total, Duration::from_secs(10)), None);
        assert_eq!(attempt_budget(total, Duration::from_secs(11)), None);
    }

    /// The whole point of the type: "no clock" and "clock spent" were both
    /// `None` in the shape this replaces, and the request field reads `None` as
    /// "no deadline" — so a caller that mapped spent onto it launched an
    /// unbounded run out of a timeout. Reverted, this assertion fails to compile
    /// the old way because spent is not a value of `AttemptBudget` at all.
    #[test]
    fn a_spent_clock_cannot_be_mistaken_for_no_clock() {
        assert_eq!(
            attempt_budget(Some(Duration::from_secs(10)), Duration::from_secs(10))
                .map(|budget| budget.deadline()),
            None,
            "spent must stop the caller before a deadline is ever built"
        );
        assert_eq!(
            attempt_budget(None, Duration::from_secs(10_000)).map(|b| b.deadline()),
            Some(None),
            "no backstop is 'no deadline', however much time has gone by"
        );
    }

    #[test]
    fn the_turn_ceiling_scales_with_the_seconds_left_to_the_attempt() {
        // 16 was the fixed count `team_dispatch` used, calibrated on
        // qwen3.6:35b-a3b at 130-235 s a turn; the worker's 20 had the same
        // problem. lr7b_v6 ran a real budget on a GPU-resident 7B at 2-6 s a turn,
        // stopped at the count with 1154 of its 1200 seconds unspent and a compile
        // repair still owed — the budget, not the count, is what should end an
        // attempt.
        assert_eq!(
            turn_ceiling_for(AttemptBudget::Remaining(Duration::from_secs(1200))),
            48
        );
        // The floor keeps a slow model's behavior identical: at 15 turns' worth of
        // seconds the deadline stops that attempt before the count ever could.
        assert_eq!(
            turn_ceiling_for(AttemptBudget::Remaining(Duration::from_secs(300))),
            16
        );
        assert_eq!(
            turn_ceiling_for(AttemptBudget::Remaining(Duration::from_secs(20))),
            16
        );
        // And the ceiling bounds the other direction — an hour of budget must not
        // license a run that only ever reads files.
        assert_eq!(
            turn_ceiling_for(AttemptBudget::Remaining(Duration::from_secs(3600))),
            MAX_BOUNDED_TURN_CEILING
        );
        // Removing the clock must not remove the turn bound as well: with no
        // deadline `wasted_time_ends_the_run` is offline, so the count is the
        // only thing between a read-only run and running forever. The const block
        // above proves it sits above the bounded ceiling; this proves the unbounded
        // attempt actually receives it.
        assert_eq!(
            turn_ceiling_for(AttemptBudget::Unbounded),
            UNBOUNDED_TURN_CEILING
        );
    }

    #[test]
    fn a_named_backstop_is_read_as_seconds_and_garbage_is_not_reinterpreted() {
        assert_eq!(
            parse_backstop_setting("1800"),
            BackstopSetting::Seconds(1800)
        );
        assert_eq!(
            parse_backstop_setting(" 1800 "),
            BackstopSetting::Seconds(1800)
        );
        // Garbage is not a request to run forever.
        assert_eq!(parse_backstop_setting("soon"), BackstopSetting::Unset);
        assert_eq!(parse_backstop_setting(""), BackstopSetting::Unset);
    }

    #[test]
    fn the_backstop_can_be_removed_by_saying_so() {
        for word in [
            "0",
            "OFF",
            "off",
            "none",
            "unbounded",
            "unlimited",
            "INFINITE",
        ] {
            assert_eq!(
                parse_backstop_setting(word),
                BackstopSetting::Unbounded,
                "{word} must mean no wall-clock ceiling"
            );
        }
    }

    #[test]
    fn an_unattended_budget_is_either_a_usable_deadline_or_explicitly_absent() {
        // Whatever the environment says, a `Some` must be usable as a deadline:
        // one second of budget would make every unattended run time out before
        // its first turn completes, and zero would be a silent unbounded run.
        if let Some(budget) = unattended_budget() {
            assert!(budget.as_secs() >= 1, "{budget:?} is not a usable deadline");
        }
    }
}
