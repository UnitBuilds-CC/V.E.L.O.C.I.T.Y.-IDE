use super::artifacts::*;
use super::scope::*;
use super::types::*;
use crate::agent::{run_headless_subagent, HeadlessSubAgentProgress, HeadlessSubAgentRequest};
use crate::automation::instruction_registry::AgentTaskKind;
use crate::automation::mediator::MediatorArena;
use crate::automation::task_router::RoutedModelRoute;
use crate::editor::continuation_ledger::ContinuationLedger;
use crate::registry::event_store::{EventOutcome, EventStore};
use crossbeam_channel::unbounded;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::Instant;
use velocity_ide::site_map::SiteMap;

pub fn spawn_live_worker(
    assignment: WorkerAssignment,
    mediator: Arc<MediatorArena>,
    weight_root: u64,
) -> Box<dyn WorkerHandle> {
    let (tx, rx) = mpsc::channel();
    let (control_tx, control_rx) = unbounded();
    let progress = Arc::new(std::sync::Mutex::new(HeadlessSubAgentProgress::default()));
    let progress_for_thread = progress.clone();
    let task_id = assignment.task.id;
    std::thread::spawn(move || {
        let result = run_assignment(
            assignment,
            mediator,
            weight_root,
            control_rx,
            progress_for_thread,
        );
        let _ = tx.send(result);
    });
    Box::new(LiveWorkerHandle {
        rx,
        control_tx,
        cancel_sent: false,
        progress,
        task_id,
    })
}

pub fn run_assignment(
    assignment: WorkerAssignment,
    mediator: Arc<MediatorArena>,
    weight_root: u64,
    cancel_rx: crossbeam_channel::Receiver<crate::agent::UiToAgentMessage>,
    progress: Arc<std::sync::Mutex<HeadlessSubAgentProgress>>,
) -> WorkerResult {
    let start = Instant::now();
    let task = assignment.task.clone();
    log::info!(
        "worker: starting assignment for task '{}'",
        assignment.task.title
    );
    let mut result = WorkerResult::new(&task);
    result.is_read_only = assignment.task_kind.is_read_only();
    let run_dir = assignment
        .workspace_root
        .join(".velocity")
        .join("agentic")
        .join("runs")
        .join(format!("task-{}", task.id.0));
    let site_map_path = assignment.workspace_root.join(".velocity").join("site_map");
    let site_map = match SiteMap::open(&site_map_path, weight_root) {
        Ok(site_map) => site_map,
        Err(err) => {
            result.success = false;
            result.duration = start.elapsed();
            result.message = format!("failed to open site map: {err}");
            return result;
        }
    };

    if site_map.root() != assignment.planned_site_map_root {
        result.success = false;
        result.duration = start.elapsed();
        result.message = format!(
            "stale routed plan: planned SiteMap root {:016x} but current root is {:016x}",
            assignment.planned_site_map_root,
            site_map.root()
        );
        result.status_updates.push(result.message.clone());
        // Surface the rejection in durable history so a future agent learns
        // *why* this intended change never landed, rather than repeating it.
        let store = EventStore::open(&assignment.workspace_root);
        let _ = store.record_stale_plan(
            &format!("worker '{}' assignment discarded", task.title),
            &format!("{:016x}", assignment.planned_site_map_root),
            &format!("{:016x}", site_map.root()),
            task.scope.clone(),
        );
        return result;
    }

    // System C §6.1: the orchestrator worker is the headless twin of the chat
    // executor's write gate, so it must consult the *same* RBAC decision before
    // taking any scope lock — otherwise a governed workspace's headless mission
    // would bypass `require_write` and edit files an interactive run could not
    // (spec §11 R-1). Governance is opt-in: an ungoverned workspace (every tempdir
    // test) yields `governed() == false` and the gate is never consulted, while a
    // governed one resolves the OS-login user's role — the same node identity §6.4
    // uses — and a denial is already appended to the durable trail (INV-6).
    {
        let governor = crate::automation::Governor::open(&assignment.workspace_root);
        if governor.governed() {
            let os_user = std::env::var("USERNAME")
                .or_else(|_| std::env::var("USER"))
                .unwrap_or_else(|_| "operator".to_string());
            let principal =
                governor.principal_for_human(&os_user, crate::automation::NodeId::local());
            if let Err(reason) = governor.require_write(&principal) {
                result.success = false;
                result.duration = start.elapsed();
                result.message = reason.clone();
                result.status_updates.push(reason);
                return result;
            }
        }
    }

    let locked_scopes = match acquire_scope_locks(
        &assignment.workspace_root,
        &task.scope,
        &mediator,
        &site_map,
        task.id,
    ) {
        Ok(locked_scopes) => locked_scopes,
        Err(conflict) => {
            result.success = false;
            result.duration = start.elapsed();
            result.message = conflict.contract.clone();
            // Record the shared-write conflict into the decision trail: this is
            // the artefact that makes conflict resolution auditable/learnable.
            let store = EventStore::open(&assignment.workspace_root);
            let _ = store.record_shared_write_conflict(
                "worker_assignment",
                &format!("worker '{}' blocked by shared-write conflict", task.title),
                conflict.kind,
                &conflict.existing_agent,
                &conflict.requested_agent,
                conflict.files,
                &conflict.contract,
            );
            return result;
        }
    };

    let outcome = execute_live_task(&assignment, &run_dir, &task.scope, &cancel_rx, &progress);

    for scope in &locked_scopes {
        mediator.release_lock(scope, &format!("task-{}", task.id.0));
    }

    match outcome {
        Ok(execution) => {
            let wa_run_path =
                detect_wa_run_artifact_path(&execution.changed_files, &execution.created_files);
            let wa_run_id = wa_run_path.as_deref().and_then(wa_run_id_from_path);
            result.outputs = execution.changed_files;
            result.created_files = execution.created_files;
            result.deleted_files = execution.deleted_files;
            result.out_of_scope_created_files = execution.out_of_scope_created_files;
            result.provider_label = execution.provider_label;
            result.model_label = execution.model_label;
            result.transcript = execution.transcript;
            result.status_updates = execution.status_updates;
            result.attempts = execution.attempts;
            result.run_summary_path = Some(run_dir.join("summary.txt"));
            result.run_facts_path = Some(run_dir.join("facts.nda"));
            result.wa_run_path = wa_run_path;
            result.wa_run_id = wa_run_id;
            result.duration = start.elapsed();
            result.message = execution.message;
        }
        Err(execution) => {
            let wa_run_path =
                detect_wa_run_artifact_path(&execution.changed_files, &execution.created_files);
            let wa_run_id = wa_run_path.as_deref().and_then(wa_run_id_from_path);
            result.success = false;
            result.outputs = execution.changed_files;
            result.created_files = execution.created_files;
            result.deleted_files = execution.deleted_files;
            result.out_of_scope_created_files = execution.out_of_scope_created_files;
            result.provider_label = execution.provider_label;
            result.model_label = execution.model_label;
            result.transcript = execution.transcript;
            result.status_updates = execution.status_updates;
            result.attempts = execution.attempts;
            result.run_summary_path = Some(run_dir.join("summary.txt"));
            result.run_facts_path = Some(run_dir.join("facts.nda"));
            result.wa_run_path = wa_run_path;
            result.wa_run_id = wa_run_id;
            result.duration = start.elapsed();
            result.message = execution.message;
        }
    }

    // Close the loop: record the terminal outcome of the worker task so the
    // decision trail links site-map transitions to the agent that produced
    // them and read-only history noise no longer buries the result.
    let store = EventStore::open(&assignment.workspace_root);
    let _ = store.record_task_outcome(
        task.id.0,
        &format!("worker '{}' finished", task.title),
        if result.success {
            EventOutcome::Success
        } else {
            EventOutcome::Failure
        },
        &result.message,
        result.outputs.clone(),
    );

    result
}

pub fn execute_live_task(
    assignment: &WorkerAssignment,
    run_dir: &Path,
    scope: &[String],
    cancel_rx: &crossbeam_channel::Receiver<crate::agent::UiToAgentMessage>,
    progress: &Arc<std::sync::Mutex<HeadlessSubAgentProgress>>,
) -> Result<ExecutionOutcome, ExecutionOutcome> {
    fs::create_dir_all(run_dir)
        .map_err(|err| failed_execution(assignment, format!("create run dir: {err}")))?;
    write_execution_contract_artifacts(run_dir, assignment)
        .map_err(|err| failed_execution(assignment, format!("write instructions: {err}")))?;

    let snapshot_root = run_dir.join("scope_snapshot");
    fs::create_dir_all(&snapshot_root)
        .map_err(|err| failed_execution(assignment, format!("create snapshot dir: {err}")))?;

    let scoped_paths = collect_scoped_paths(&assignment.workspace_root, scope);
    let before_contents = snapshot_scope(&scoped_paths, &assignment.workspace_root, &snapshot_root)
        .map_err(|err| failed_execution(assignment, err))?;
    let before_workspace_files = collect_workspace_files(&assignment.workspace_root)
        .map_err(|err| failed_execution(assignment, err))?;

    let routes = if assignment.fallback_chain.is_empty() {
        vec![RoutedModelRoute {
            provider: assignment.provider,
            model_id: assignment.model_id.clone(),
            model_label: assignment.model_label.clone(),
            thinking: assignment.thinking,
            score: 0,
        }]
    } else {
        assignment.fallback_chain.clone()
    };

    let mut attempts = Vec::new();
    let mut last_status_updates = Vec::new();
    let mut last_transcript = String::new();
    // How the most recent route ended, as the loop reported it. Kept so the
    // failure message below can distinguish "an operator stopped this" from
    // "the model could not do it" without reading status-line prose.
    let mut last_outcome = crate::agent::models::AgentLoopOutcome::default();
    let mut final_provider_label = assignment.provider_label.clone();
    let mut final_model_label = assignment.model_label.clone();
    // Continuation ledger from the immediately-failed route, fed into the next
    // attempt's prompt. Without this, every fallback retry re-disclosed the
    // workspace from zero and burned its whole turn budget re-auditing — the
    // exact failure the agent_eval run exhibited (3 attempts, "No scoped
    // changes" ×3).
    let mut carry_forward: Option<String> = None;

    // Memory loop: read the durable decision history back into the first
    // attempt. Past failures, shared-write conflicts and reverts on this
    // task's scope files are prepended to the instructions, so the worker
    // inherits what already happened instead of blindly re-running attempts
    // that failed in an earlier session. Empty brief -> untouched instructions.
    let memory_brief = crate::registry::event_store::history_brief(
        &assignment.workspace_root,
        &assignment.task.scope,
        12,
    );
    // Skills loop: workspace markdown skills (.velocity/skills/*.md) activate
    // against this task by trigger keywords in the task text or globs over
    // the scope files; matched bodies are inlined, the rest demote to a
    // use_skill-able index. No skills -> no change to instructions.
    let skills_block = crate::registry::skills::skills_brief(
        &assignment.workspace_root,
        &format!("{} {}", assignment.task.title, assignment.instructions),
        &assignment.task.scope,
        6000,
    );
    let mut base_instructions = assignment.instructions.clone();
    if !memory_brief.is_empty() {
        log::info!(
            "worker: injecting prior decision history ({} scope entries) into '{}'",
            assignment.task.scope.len(),
            assignment.task.title
        );
        base_instructions = format!("{base_instructions}\n\n{memory_brief}");
    }
    if !skills_block.is_empty() {
        log::info!(
            "worker: injecting project skills into '{}'",
            assignment.task.title
        );
        base_instructions = format!("{base_instructions}\n\n{skills_block}");
    }

    // One budget for the whole route chain rather than one per route: a worker
    // is unattended, so no single run may be unbounded and a failover chain may
    // not multiply the ceiling by its own length.
    let worker_start = Instant::now();
    let worker_budget = crate::agent::unattended_budget();
    let mut budget_spent = false;

    for route in routes {
        let route_start = Instant::now();
        // `None` means the budget is gone, and forwarding `None` to the request
        // would mean the opposite ("no deadline") — so this is the stop
        // condition, not a default.
        let Some(attempt_budget) = crate::agent::budget_left(worker_budget, worker_start.elapsed())
        else {
            budget_spent = true;
            break;
        };
        let prompt = attempt_prompt(&base_instructions, carry_forward.as_deref());
        // Modification kinds get more turns than the loop's default 15: with the
        // ledger in hand, the extra turns go to edits and build fixes rather than
        // exploration. The count comes from the seconds this attempt actually has
        // left, not from a constant — a fixed 20 was another wall-clock cap in
        // disguise, binding early on a fast model and never on a slow one.
        // Read-only kinds keep the loop's default.
        let attempt_max_turns = if assignment.task_kind.is_read_only() {
            None
        } else {
            Some(crate::agent::turn_ceiling_for(attempt_budget))
        };
        let subagent = run_headless_subagent(HeadlessSubAgentRequest {
            workspace_root: assignment.workspace_root.clone(),
            provider: route.provider,
            model: route.model_id.clone(),
            thinking: route.thinking,
            prompt,
            cancel_rx: Some(cancel_rx.clone()),
            progress: Some(progress.clone()),
            scoped_files: assignment.scoped_files.clone(),
            max_turns: attempt_max_turns,
            budget: Some(attempt_budget),
            // The worker already gates a file-modification task by diffing the
            // filesystem (`detect_scoped_changes`), so it does not need the
            // loop's in-run artifact gate too; keep the prior behaviour.
            requires_artifact: false,
        });
        last_status_updates = subagent.status_updates.clone();
        last_transcript = subagent.transcript.clone();
        last_outcome = subagent.outcome.clone();
        final_provider_label = route.provider.label().to_string();
        final_model_label = route.model_label.clone();

        let (changed_files, created_files, deleted_files) =
            detect_scoped_changes(&scoped_paths, &before_contents, &assignment.workspace_root)
                .map_err(|err| failed_execution(assignment, err))?;
        let out_of_scope_created_files = detect_out_of_scope_created_files(
            &scoped_paths,
            &before_workspace_files,
            &assignment.workspace_root,
        )
        .map_err(|err| failed_execution(assignment, err))?;
        // Read-only task kinds (Analysis, Planning) succeed when the loop
        // actually finished and said something; file-modification kinds require
        // changes on disk.
        let success = if assignment.task_kind.is_read_only() {
            // The loop's own structured outcome, not a substring hunt through
            // its status lines. Scanning for "error" failed a run whenever the
            // analysis merely *quoted* one — a build log, a compiler diagnostic
            // it was asked to explain — and passed a run that blew its whole
            // turn budget without ever producing an answer.
            subagent.outcome.is_completed() && !subagent.transcript.trim().is_empty()
        } else {
            !changed_files.is_empty() || !created_files.is_empty() || !deleted_files.is_empty()
        };
        let message = if success {
            if assignment.task_kind.is_read_only() {
                // For read-only tasks, surface the model's response (truncated).
                let snippet: String = subagent.transcript.chars().take(500).collect();
                format!(
                    "Analysis via {} / {}: {}",
                    final_provider_label, final_model_label, snippet,
                )
            } else if assignment.task_kind == AgentTaskKind::DesktopAutomation {
                format!(
                    "Desktop automation evidence captured: changed {}, created {}, deleted {} via {} / {}",
                    changed_files.len(),
                    created_files.len(),
                    deleted_files.len(),
                    final_provider_label,
                    final_model_label,
                )
            } else {
                format!(
                    "Changed {}, created {}, deleted {} via {} / {}",
                    changed_files.len(),
                    created_files.len(),
                    deleted_files.len(),
                    final_provider_label,
                    final_model_label,
                )
            }
        } else if assignment.task_kind == AgentTaskKind::DesktopAutomation {
            format!(
                "Desktop automation run produced no scoped file changes via {} / {}",
                final_provider_label, final_model_label
            )
        } else {
            format!(
                "No scoped changes via {} / {}",
                final_provider_label, final_model_label
            )
        };
        attempts.push(WorkerAttempt {
            provider_label: final_provider_label.clone(),
            model_label: final_model_label.clone(),
            model_id: route.model_id.clone(),
            success,
            message: message.clone(),
        });

        if success {
            let mut status_updates = last_status_updates;
            if !out_of_scope_created_files.is_empty() {
                status_updates.push(format!(
                    "Out-of-scope created files detected: {}",
                    out_of_scope_created_files.join(", ")
                ));
            }
            let outcome = ExecutionOutcome {
                success: true,
                task_kind: assignment.task_kind,
                provider_label: final_provider_label,
                model_label: final_model_label,
                changed_files,
                created_files,
                deleted_files,
                out_of_scope_created_files,
                transcript: last_transcript,
                status_updates,
                attempts,
                message,
                is_read_only: assignment.task_kind.is_read_only(),
            };
            write_execution_artifacts(run_dir, &outcome)
                .map_err(|err| failed_execution(assignment, err))?;
            return Ok(outcome);
        }

        // ─── Continuation Ledger: capture state for cross-model handoff ───
        // After each failed attempt, build a ledger so the next route in the
        // fallback chain receives structured context about what was tried,
        // what partially changed, and what still needs doing.
        let scope_paths: Vec<PathBuf> = scoped_paths.explicit_files.clone();
        let mut ledger = ContinuationLedger::capture(
            &format!("task-{}", assignment.task.id.0),
            &assignment.instructions,
            &format!("{:?}", assignment.task_kind),
            &scope_paths,
            &assignment.workspace_root,
            assignment.planned_site_map_root,
            &last_transcript,
            &changed_files,
            &last_status_updates,
            &final_provider_label,
            &final_model_label,
            &route.model_id,
            route_start.elapsed(),
            false,
        );

        // Enrich the brief with the workspace SiteMap so the next route in the
        // fallback chain inherits the call graph around the scoped files, not
        // just their contents. Best-effort: a missing or unreadable map leaves
        // the brief exactly as captured.
        if let Ok(site_map) = crate::automation::open_workspace_site_map(&assignment.workspace_root)
        {
            let site_map_root = assignment.workspace_root.join(".velocity").join("site_map");
            crate::editor::continuation_ledger::enrich_from_site_map(
                &mut ledger.environment,
                &site_map_root,
                &scope_paths,
                &|hash| site_map.resolve_string(hash),
                &|hash| site_map.get_callers(hash),
                &|hash| site_map.get_dependencies(hash),
            );
        }
        // Persist ledger for diagnostics and potential manual inspection.
        let ledger_path = run_dir.join(format!(
            "continuation_ledger_attempt_{}.txt",
            attempts.len()
        ));
        let ledger_prompt = ledger.continuation_prompt();
        let _ = fs::write(&ledger_path, &ledger_prompt);
        // Hand the next route the map of what was already learned and changed
        // instead of making it rediscover everything.
        carry_forward = Some(ledger_prompt);
    }

    // Structured: the loop reports a cancellation as its own outcome variant,
    // so this no longer depends on the exact wording of a status line.
    let cancelled = last_outcome.is_cancelled();
    let mut message = if cancelled {
        if assignment.task_kind == AgentTaskKind::DesktopAutomation {
            "desktop automation run cancelled before WA evidence was captured".to_string()
        } else {
            "cancelled by operator before scoped changes were produced".to_string()
        }
    } else if assignment.task_kind == AgentTaskKind::DesktopAutomation {
        "Desktop automation run finished without scoped file changes or captured WA evidence."
            .to_string()
    } else if assignment.task_kind.is_read_only() {
        "Model produced no textual response for read-only task.".to_string()
    } else {
        "No scoped file changes were produced by any provider-backed sub-agent route.".to_string()
    };
    if budget_spent {
        // Say so explicitly: "no route produced changes" reads like a model
        // problem, and the fix for a spent budget is a bigger budget or a
        // smaller task, not a different provider.
        message = format!(
            "{message} (wall-clock budget of {}s spent)",
            worker_budget.as_secs()
        );
    }
    let outcome = ExecutionOutcome {
        success: false,
        task_kind: assignment.task_kind,
        provider_label: final_provider_label,
        model_label: final_model_label,
        changed_files: Vec::new(),
        created_files: Vec::new(),
        deleted_files: Vec::new(),
        out_of_scope_created_files: Vec::new(),
        transcript: last_transcript,
        status_updates: last_status_updates,
        attempts,
        message,
        is_read_only: assignment.task_kind.is_read_only(),
    };
    write_execution_artifacts(run_dir, &outcome)
        .map_err(|err| failed_execution(assignment, err))?;
    Err(outcome)
}

pub fn failed_execution(assignment: &WorkerAssignment, message: String) -> ExecutionOutcome {
    ExecutionOutcome {
        success: false,
        task_kind: assignment.task_kind,
        provider_label: assignment.provider_label.clone(),
        model_label: assignment.model_label.clone(),
        changed_files: Vec::new(),
        created_files: Vec::new(),
        deleted_files: Vec::new(),
        out_of_scope_created_files: Vec::new(),
        transcript: String::new(),
        status_updates: Vec::new(),
        attempts: Vec::new(),
        message,
        is_read_only: assignment.task_kind.is_read_only(),
    }
}

/// Prompt for one fallback route: plain instructions on the first attempt,
/// then the previous attempt's continuation ledger appended so the new model
/// resumes from what was already learned and edited instead of re-auditing
/// the workspace from zero.
fn attempt_prompt(instructions: &str, ledger_prompt: Option<&str>) -> String {
    let Some(ledger_prompt) = ledger_prompt else {
        return instructions.to_string();
    };
    format!(
        "{instructions}\n\n# Continuation from the previous attempt\n{ledger_prompt}\n\n\
         The previous attempt ended without completing this mission. Do NOT re-read files it already \
         examined or repeat its exploration \u{2014} trust the ledger above and check the current file \
         state only where the ledger is unclear. Prioritize writing the files the mission \
         needs (write_file replaces a whole file) over further auditing, and complete the \
         mission starting from the workspace's current state."
    )
}

#[cfg(test)]
mod tests {
    use super::attempt_prompt;

    #[test]
    fn first_attempt_prompt_is_plain_instructions() {
        let prompt = attempt_prompt("Refactor the parser.", None);
        assert_eq!(prompt, "Refactor the parser.");
    }

    #[test]
    fn retry_prompt_appends_ledger_and_write_first_directive() {
        let ledger = "## Mission\nGoal: Refactor the parser.\n\n## Completed Edits\n- src/parser.rs \u{2014} split lexer";
        let prompt = attempt_prompt("Refactor the parser.", Some(ledger));
        assert!(prompt.starts_with("Refactor the parser."));
        assert!(prompt.contains("# Continuation from the previous attempt"));
        assert!(prompt.contains("## Completed Edits"));
        assert!(prompt.contains("Do NOT re-read files"));
        assert!(prompt.contains("Prioritize writing the files"));
    }
}
