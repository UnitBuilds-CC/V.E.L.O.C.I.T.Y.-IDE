use super::artifacts::*;
use super::scope::*;
use super::types::*;
use crate::agent::{run_headless_subagent, HeadlessSubAgentProgress, HeadlessSubAgentRequest};
use crate::automation::instruction_registry::AgentTaskKind;
use crate::automation::mediator::MediatorArena;
use crate::automation::task_router::RoutedModelRoute;
use crate::editor::continuation_ledger::ContinuationLedger;
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
        return result;
    }

    let locked_scopes = match acquire_scope_locks(
        &assignment.workspace_root,
        &task.scope,
        &mediator,
        &site_map,
        task.id,
    ) {
        Ok(locked_scopes) => locked_scopes,
        Err(message) => {
            result.success = false;
            result.duration = start.elapsed();
            result.message = message;
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
    let mut final_provider_label = assignment.provider_label.clone();
    let mut final_model_label = assignment.model_label.clone();

    for route in routes {
        let route_start = Instant::now();
        let subagent = run_headless_subagent(HeadlessSubAgentRequest {
            workspace_root: assignment.workspace_root.clone(),
            provider: route.provider,
            model: route.model_id.clone(),
            thinking: route.thinking,
            prompt: assignment.instructions.clone(),
            cancel_rx: Some(cancel_rx.clone()),
            progress: Some(progress.clone()),
            scoped_files: assignment.scoped_files.clone(),
        });
        last_status_updates = subagent.status_updates.clone();
        last_transcript = subagent.transcript.clone();
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
        let success =
            !changed_files.is_empty() || !created_files.is_empty() || !deleted_files.is_empty();
        let message = if success {
            if assignment.task_kind == AgentTaskKind::DesktopAutomation {
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
        let ledger = ContinuationLedger::capture(
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
        // Persist ledger for diagnostics and potential manual inspection.
        let ledger_path = run_dir.join(format!(
            "continuation_ledger_attempt_{}.txt",
            attempts.len()
        ));
        let _ = fs::write(&ledger_path, ledger.continuation_prompt());
    }

    let cancelled = last_status_updates
        .iter()
        .any(|update| update.contains("cancelled by operator"));
    let message = if cancelled {
        if assignment.task_kind == AgentTaskKind::DesktopAutomation {
            "desktop automation run cancelled before WA evidence was captured".to_string()
        } else {
            "cancelled by operator before scoped changes were produced".to_string()
        }
    } else if assignment.task_kind == AgentTaskKind::DesktopAutomation {
        "Desktop automation run finished without scoped file changes or captured WA evidence."
            .to_string()
    } else {
        "No scoped file changes were produced by any provider-backed sub-agent route.".to_string()
    };
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
    }
}
