//! A worker represents one sub-agent assigned to a single task.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use velocity_ide::site_map::SiteMap;

use crate::agent::{AiProvider, HeadlessSubAgentRequest, run_headless_subagent};
use crate::automation::mediator::MediatorArena;
use crate::automation::task_router::RoutedModelRoute;

use super::blueprint::Task;
use super::TaskId;

/// Structured result produced by a worker after attempting a task.
#[derive(Debug, Clone)]
pub struct WorkerAttempt {
    pub provider_label: String,
    pub model_label: String,
    pub model_id: String,
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct WorkerResult {
    pub success: bool,
    pub task_id: TaskId,
    pub outputs: Vec<String>,
    pub duration: Duration,
    pub message: String,
    pub provider_label: String,
    pub model_label: String,
    pub transcript: String,
    pub status_updates: Vec<String>,
    pub attempts: Vec<WorkerAttempt>,
    pub created_files: Vec<String>,
    pub deleted_files: Vec<String>,
    pub out_of_scope_created_files: Vec<String>,
    pub run_summary_path: Option<PathBuf>,
    pub run_facts_path: Option<PathBuf>,
}

impl WorkerResult {
    pub fn new(task: &Task) -> Self {
        Self {
            success: true,
            task_id: task.id,
            outputs: Vec::new(),
            duration: Duration::ZERO,
            message: "ok".to_string(),
            provider_label: String::new(),
            model_label: String::new(),
            transcript: String::new(),
            status_updates: Vec::new(),
            attempts: Vec::new(),
            created_files: Vec::new(),
            deleted_files: Vec::new(),
            out_of_scope_created_files: Vec::new(),
            run_summary_path: None,
            run_facts_path: None,
        }
    }
}

/// Abstract handle for launching and polling a worker task.
pub trait WorkerHandle {
    fn poll(&mut self) -> Option<WorkerResult>;
}

pub struct LiveWorkerHandle {
    rx: mpsc::Receiver<WorkerResult>,
}

impl WorkerHandle for LiveWorkerHandle {
    fn poll(&mut self) -> Option<WorkerResult> {
        self.rx.try_recv().ok()
    }
}

#[derive(Debug, Clone)]
pub struct WorkerAssignment {
    pub task: Task,
    pub workspace_root: PathBuf,
    pub instructions: String,
    pub planned_site_map_root: u64,
    pub provider: AiProvider,
    pub provider_label: String,
    pub model_id: String,
    pub model_label: String,
    pub thinking: bool,
    pub fallback_chain: Vec<RoutedModelRoute>,
}

pub fn spawn_live_worker(
    assignment: WorkerAssignment,
    mediator: Arc<MediatorArena>,
    weight_root: u64,
) -> Box<dyn WorkerHandle> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = run_assignment(assignment, mediator, weight_root);
        let _ = tx.send(result);
    });
    Box::new(LiveWorkerHandle { rx })
}

fn run_assignment(
    assignment: WorkerAssignment,
    mediator: Arc<MediatorArena>,
    weight_root: u64,
) -> WorkerResult {
    let start = Instant::now();
    let task = assignment.task.clone();
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

    let mut locked_files = Vec::new();
    for scope in &task.scope {
        let rel = PathBuf::from(scope);
        let abs = if rel.is_absolute() {
            rel
        } else {
            assignment.workspace_root.join(&rel)
        };
        if abs.is_file() {
            if let Err(conflict) = mediator.acquire_lock(abs.clone(), (1, usize::MAX / 4), format!("task-{}", task.id.0), &site_map) {
                result.success = false;
                result.duration = start.elapsed();
                result.message = mediator.resolve_conflict(&conflict);
                return result;
            }
            locked_files.push(abs);
        }
    }

    let outcome = execute_live_task(&assignment, &run_dir, &task.scope);

    for file in &locked_files {
        mediator.release_lock(file, &format!("task-{}", task.id.0));
    }

    match outcome {
        Ok(execution) => {
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
            result.duration = start.elapsed();
            result.message = execution.message;
        }
        Err(execution) => {
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
            result.duration = start.elapsed();
            result.message = execution.message;
        }
    }

    result
}

#[derive(Debug, Clone)]
struct ExecutionOutcome {
    success: bool,
    provider_label: String,
    model_label: String,
    changed_files: Vec<String>,
    created_files: Vec<String>,
    deleted_files: Vec<String>,
    out_of_scope_created_files: Vec<String>,
    transcript: String,
    status_updates: Vec<String>,
    attempts: Vec<WorkerAttempt>,
    message: String,
}

fn execute_live_task(
    assignment: &WorkerAssignment,
    run_dir: &Path,
    scope: &[String],
) -> Result<ExecutionOutcome, ExecutionOutcome> {
    fs::create_dir_all(run_dir).map_err(|err| failed_execution(assignment, format!("create run dir: {err}")))?;
    write_execution_contract_artifacts(run_dir, assignment)
        .map_err(|err| failed_execution(assignment, format!("write instructions: {err}")))?;

    let snapshot_root = run_dir.join("scope_snapshot");
    fs::create_dir_all(&snapshot_root).map_err(|err| failed_execution(assignment, format!("create snapshot dir: {err}")))?;

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
        let subagent = run_headless_subagent(HeadlessSubAgentRequest {
            workspace_root: assignment.workspace_root.clone(),
            provider: route.provider,
            model: route.model_id.clone(),
            thinking: route.thinking,
            prompt: assignment.instructions.clone(),
        });
        last_status_updates = subagent.status_updates.clone();
        last_transcript = subagent.transcript.clone();
        final_provider_label = route.provider.label().to_string();
        final_model_label = route.model_label.clone();

        let (changed_files, created_files, deleted_files) = detect_scoped_changes(&scoped_paths, &before_contents, &assignment.workspace_root)
            .map_err(|err| failed_execution(assignment, err))?;
        let out_of_scope_created_files = detect_out_of_scope_created_files(
            &scoped_paths,
            &before_workspace_files,
            &assignment.workspace_root,
        )
        .map_err(|err| failed_execution(assignment, err))?;
        let success = !changed_files.is_empty() || !created_files.is_empty() || !deleted_files.is_empty();
        let message = if success {
            format!(
                "Changed {}, created {}, deleted {} via {} / {}",
                changed_files.len(),
                created_files.len(),
                deleted_files.len(),
                final_provider_label,
                final_model_label,
            )
        } else {
            format!("No scoped changes via {} / {}", final_provider_label, final_model_label)
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
            write_execution_artifacts(run_dir, &outcome).map_err(|err| failed_execution(assignment, err))?;
            return Ok(outcome);
        }
    }

    let outcome = ExecutionOutcome {
        success: false,
        provider_label: final_provider_label,
        model_label: final_model_label,
        changed_files: Vec::new(),
        created_files: Vec::new(),
        deleted_files: Vec::new(),
        out_of_scope_created_files: Vec::new(),
        transcript: last_transcript,
        status_updates: last_status_updates,
        attempts,
        message: "No scoped file changes were produced by any provider-backed sub-agent route.".to_string(),
    };
    write_execution_artifacts(run_dir, &outcome).map_err(|err| failed_execution(assignment, err))?;
    Err(outcome)
}

fn failed_execution(assignment: &WorkerAssignment, message: String) -> ExecutionOutcome {
    ExecutionOutcome {
        success: false,
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

fn write_execution_artifacts(run_dir: &Path, outcome: &ExecutionOutcome) -> Result<(), String> {
    write_execution_summary(run_dir, outcome)?;
    write_execution_facts(run_dir, outcome)
}

fn serialize_execution_contract_nda(assignment: &WorkerAssignment) -> String {
    let mut lines = vec![
        "worker-execution-contract version 2".to_string(),
        format!("field\tprovider\t{}", encode_nda_text(&assignment.provider_label)),
        format!("field\tmodel\t{}", encode_nda_text(&assignment.model_label)),
        format!("field\tmodel_id\t{}", encode_nda_text(&assignment.model_id)),
        format!("field\tthinking\t{}", assignment.thinking),
        format!("field\ttask_id\t{}", assignment.task.id.0),
        format!("field\ttask_title\t{}", encode_nda_text(&assignment.task.title)),
        format!("field\ttask_description\t{}", encode_nda_text(&assignment.task.description)),
        format!("field\tplanned_site_map_root\t{:016x}", assignment.planned_site_map_root),
        format!("scope_count {}", assignment.task.scope.len()),
        format!("fallback_route_count {}", assignment.fallback_chain.len()),
    ];

    let instruction_lines: Vec<&str> = assignment.instructions.split('\n').collect();
    lines.push(format!("instruction_line_count {}", instruction_lines.len()));

    for (index, scope) in assignment.task.scope.iter().enumerate() {
        lines.push(format!("scope\t{}\t{}", index, encode_nda_text(scope)));
    }

    for (index, route) in assignment.fallback_chain.iter().enumerate() {
        lines.push(format!("fallback_route\t{}", index));
        lines.push(format!("fallback_route_field\t{}\tprovider\t{}", index, encode_nda_text(route.provider.label())));
        lines.push(format!("fallback_route_field\t{}\tmodel\t{}", index, encode_nda_text(&route.model_label)));
        lines.push(format!("fallback_route_field\t{}\tmodel_id\t{}", index, encode_nda_text(&route.model_id)));
        lines.push(format!("fallback_route_field\t{}\tthinking\t{}", index, route.thinking));
        lines.push(format!("fallback_route_field\t{}\tscore\t{}", index, route.score));
    }

    for (index, instruction_line) in instruction_lines.iter().enumerate() {
        lines.push(format!("instruction_line\t{}\t{}", index, encode_nda_text(instruction_line)));
    }

    lines.join("\n") + "\n"
}

fn write_execution_contract_artifacts(run_dir: &Path, assignment: &WorkerAssignment) -> Result<(), String> {
    fs::write(run_dir.join("instructions.nda"), serialize_execution_contract_nda(assignment))
        .map_err(|err| format!("write nda instructions: {err}"))?;
    fs::write(
        run_dir.join("instructions.txt"),
        format!(
            "provider: {}\nmodel: {}\nmodel_id: {}\n\nthinking: {}\n\n{}",
            assignment.provider_label,
            assignment.model_label,
            assignment.model_id,
            assignment.thinking,
            assignment.instructions
        ),
    )
    .map_err(|err| format!("write text instructions: {err}"))
}

fn encode_nda_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn write_execution_summary(run_dir: &Path, outcome: &ExecutionOutcome) -> Result<(), String> {
    let mut summary = String::new();
    summary.push_str(if outcome.success { "Result: success\n" } else { "Result: failed\n" });
    summary.push_str("Active route: ");
    summary.push_str(&outcome.provider_label);
    summary.push_str(" / ");
    summary.push_str(&outcome.model_label);
    summary.push_str("\n\nAttempts:\n");
    for attempt in &outcome.attempts {
        summary.push_str("- ");
        summary.push_str(&attempt.provider_label);
        summary.push_str(" / ");
        summary.push_str(&attempt.model_label);
        summary.push_str(": ");
        summary.push_str(&attempt.message);
        summary.push('\n');
    }
    if !outcome.status_updates.is_empty() {
        summary.push_str("\nStatus updates:\n");
        for line in &outcome.status_updates {
            summary.push_str("- ");
            summary.push_str(line);
            summary.push('\n');
        }
    }
    if !outcome.changed_files.is_empty() {
        summary.push_str("\nChanged files:\n");
        for path in &outcome.changed_files {
            summary.push_str("- ");
            summary.push_str(path);
            summary.push('\n');
        }
    }
    if !outcome.created_files.is_empty() {
        summary.push_str("\nCreated files:\n");
        for path in &outcome.created_files {
            summary.push_str("- ");
            summary.push_str(path);
            summary.push('\n');
        }
    }
    if !outcome.deleted_files.is_empty() {
        summary.push_str("\nDeleted files:\n");
        for path in &outcome.deleted_files {
            summary.push_str("- ");
            summary.push_str(path);
            summary.push('\n');
        }
    }
    if !outcome.out_of_scope_created_files.is_empty() {
        summary.push_str("\nOut-of-scope created files:\n");
        for path in &outcome.out_of_scope_created_files {
            summary.push_str("- ");
            summary.push_str(path);
            summary.push('\n');
        }
    }
    if !outcome.transcript.trim().is_empty() {
        summary.push_str("\nTranscript:\n");
        summary.push_str(outcome.transcript.trim());
        summary.push('\n');
    }
    fs::write(run_dir.join("summary.txt"), &summary).map_err(|err| format!("write summary: {err}"))
}

fn write_execution_facts(run_dir: &Path, outcome: &ExecutionOutcome) -> Result<(), String> {
    let mut facts = Vec::new();
    facts.push("artifact:worker-run kind orchestrator-run".to_string());
    facts.push(format!(
        "artifact:worker-run result {}",
        if outcome.success { "success" } else { "failed" }
    ));
    facts.push(format!("artifact:worker-run provider provider:{}", nda_atom(&outcome.provider_label)));
    facts.push(format!("artifact:worker-run model model:{}", nda_atom(&outcome.model_label)));
    facts.push(format!("artifact:worker-run message text:{}", nda_atom(&outcome.message)));

    for (idx, attempt) in outcome.attempts.iter().enumerate() {
        let attempt_id = format!("attempt:{}", idx + 1);
        facts.push(format!("artifact:worker-run attempted {}", attempt_id));
        facts.push(format!("{} provider provider:{}", attempt_id, nda_atom(&attempt.provider_label)));
        facts.push(format!("{} model model:{}", attempt_id, nda_atom(&attempt.model_label)));
        facts.push(format!("{} model_id model-id:{}", attempt_id, nda_atom(&attempt.model_id)));
        facts.push(format!("{} result {}", attempt_id, if attempt.success { "success" } else { "failed" }));
        facts.push(format!("{} message text:{}", attempt_id, nda_atom(&attempt.message)));
    }

    for path in &outcome.changed_files {
        facts.push(format!("artifact:worker-run changed file:{}", nda_atom(path)));
    }
    for path in &outcome.created_files {
        facts.push(format!("artifact:worker-run created file:{}", nda_atom(path)));
    }
    for path in &outcome.deleted_files {
        facts.push(format!("artifact:worker-run deleted file:{}", nda_atom(path)));
    }
    for path in &outcome.out_of_scope_created_files {
        facts.push(format!("artifact:worker-run out_of_scope_created file:{}", nda_atom(path)));
    }
    for status in &outcome.status_updates {
        facts.push(format!("artifact:worker-run status text:{}", nda_atom(status)));
    }
    if !outcome.transcript.trim().is_empty() {
        facts.push(format!("artifact:worker-run transcript text:{}", nda_atom(outcome.transcript.trim())));
    }

    fs::write(run_dir.join("facts.nda"), facts.join("\n")).map_err(|err| format!("write facts: {err}"))
}

fn nda_atom(value: &str) -> String {
    let mut atom = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            atom.push(ch.to_ascii_lowercase());
        } else {
            atom.push('-');
        }
    }
    let atom = atom.trim_matches('-');
    if atom.is_empty() {
        "empty".to_string()
    } else {
        atom.to_string()
    }
}

#[derive(Debug, Clone)]
struct ScopedPaths {
    explicit_files: Vec<PathBuf>,
    scope_roots: Vec<PathBuf>,
}

fn collect_scoped_paths(workspace_root: &Path, scope: &[String]) -> ScopedPaths {
    let scope_roots = scope
        .iter()
        .map(PathBuf::from)
        .map(|rel_path| if rel_path.is_absolute() { rel_path } else { workspace_root.join(rel_path) })
        .collect::<Vec<_>>();
    let explicit_files = scope_roots
        .iter()
        .filter(|path| !path.is_dir())
        .cloned()
        .collect::<Vec<_>>();
    ScopedPaths {
        explicit_files,
        scope_roots,
    }
}

fn snapshot_scope(
    scoped_paths: &ScopedPaths,
    workspace_root: &Path,
    snapshot_root: &Path,
) -> Result<HashMap<PathBuf, Option<Vec<u8>>>, String> {
    let mut before_contents = HashMap::new();
    for abs_path in collect_candidate_files(scoped_paths)? {
        let rel_path = abs_path.strip_prefix(workspace_root).unwrap_or(&abs_path);
        let dest = snapshot_root.join(rel_path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("create snapshot parent: {err}"))?;
        }
        let bytes = read_scoped_file(&abs_path)
            .map_err(|err| format!("snapshot file {}: {err}", abs_path.display()))?;
        if let Some(bytes) = &bytes {
            fs::write(&dest, bytes).map_err(|err| format!("write snapshot {}: {err}", dest.display()))?;
        }
        before_contents.insert(abs_path, bytes);
    }
    Ok(before_contents)
}

fn detect_scoped_changes(
    scoped_paths: &ScopedPaths,
    before_contents: &HashMap<PathBuf, Option<Vec<u8>>>,
    workspace_root: &Path,
) -> Result<(Vec<String>, Vec<String>, Vec<String>), String> {
    let mut changed = Vec::new();
    let mut created = Vec::new();
    let mut deleted = Vec::new();
    let mut candidate_paths = before_contents.keys().cloned().collect::<BTreeSet<_>>();
    for abs_path in collect_candidate_files(scoped_paths)? {
        candidate_paths.insert(abs_path);
    }
    for abs_path in candidate_paths {
        let before = before_contents.get(&abs_path).cloned().flatten();
        let after = read_scoped_file(&abs_path)
            .map_err(|err| format!("read post-run file {}: {err}", abs_path.display()))?;
        let rel = abs_path.strip_prefix(workspace_root).unwrap_or(&abs_path).display().to_string();
        match (before, after) {
            (Some(before_bytes), Some(after_bytes)) if before_bytes != after_bytes => changed.push(rel),
            (None, Some(_)) => created.push(rel),
            (Some(_), None) => deleted.push(rel),
            _ => {}
        }
    }
    Ok((changed, created, deleted))
}

fn collect_candidate_files(scoped_paths: &ScopedPaths) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for path in &scoped_paths.explicit_files {
        push_unique_path(&mut files, path.clone());
    }
    for root in &scoped_paths.scope_roots {
        collect_existing_files(root, &mut files)?;
    }
    Ok(files)
}

fn collect_existing_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if path.is_file() {
        push_unique_path(files, path.to_path_buf());
        return Ok(());
    }
    if !path.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(path).map_err(|err| format!("read scope dir {}: {err}", path.display()))? {
        let entry = entry.map_err(|err| format!("read scope dir entry {}: {err}", path.display()))?;
        let entry_path = entry.path();
        if entry_path.is_dir() {
            collect_existing_files(&entry_path, files)?;
        } else if entry_path.is_file() {
            push_unique_path(files, entry_path);
        }
    }
    Ok(())
}

fn push_unique_path(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !paths.iter().any(|path| path == &candidate) {
        paths.push(candidate);
    }
}

fn read_scoped_file(path: &Path) -> Result<Option<Vec<u8>>, std::io::Error> {
    if path.is_dir() {
        return Ok(None);
    }
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

fn detect_out_of_scope_created_files(
    scoped_paths: &ScopedPaths,
    before_workspace_files: &BTreeSet<PathBuf>,
    workspace_root: &Path,
) -> Result<Vec<String>, String> {
    let after_workspace_files = collect_workspace_files(workspace_root)?;
    let mut created = after_workspace_files
        .difference(before_workspace_files)
        .filter(|path| !is_path_within_scope(path, scoped_paths, workspace_root))
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    created.sort();
    Ok(created)
}

fn collect_workspace_files(workspace_root: &Path) -> Result<BTreeSet<PathBuf>, String> {
    let mut files = BTreeSet::new();
    collect_workspace_files_recursive(workspace_root, workspace_root, &mut files)?;
    Ok(files)
}

fn collect_workspace_files_recursive(
    workspace_root: &Path,
    current: &Path,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    if !current.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(current).map_err(|err| format!("read workspace dir {}: {err}", current.display()))? {
        let entry = entry.map_err(|err| format!("read workspace dir entry {}: {err}", current.display()))?;
        let entry_path = entry.path();
        let rel_path = entry_path
            .strip_prefix(workspace_root)
            .unwrap_or(&entry_path)
            .to_path_buf();
        if entry_path.is_dir() {
            if should_skip_workspace_dir(&rel_path) {
                continue;
            }
            collect_workspace_files_recursive(workspace_root, &entry_path, files)?;
        } else if entry_path.is_file() && !should_skip_workspace_file(&rel_path) {
            files.insert(rel_path);
        }
    }
    Ok(())
}

fn should_skip_workspace_dir(rel_path: &Path) -> bool {
    rel_path
        .components()
        .any(|component| matches!(component.as_os_str().to_str(), Some(".git" | ".velocity" | "target" | "node_modules" | "archive")))
}

fn should_skip_workspace_file(rel_path: &Path) -> bool {
    rel_path
        .components()
        .any(|component| matches!(component.as_os_str().to_str(), Some(".git" | ".velocity" | "target" | "node_modules" | "archive")))
}

fn is_path_within_scope(rel_path: &Path, scoped_paths: &ScopedPaths, workspace_root: &Path) -> bool {
    scoped_paths.scope_roots.iter().any(|root| {
        let root_rel = root.strip_prefix(workspace_root).unwrap_or(root);
        rel_path == root_rel || rel_path.starts_with(root_rel)
    })
}

#[cfg(test)]
mod tests {
    use super::{
        collect_scoped_paths, collect_workspace_files, detect_out_of_scope_created_files,
        detect_scoped_changes, snapshot_scope, write_execution_contract_artifacts,
        write_execution_facts, ExecutionOutcome, WorkerAssignment, WorkerAttempt,
    };
    use crate::agent::AiProvider;
    use crate::automation::RoutedModelRoute;
    use crate::orchestrator::blueprint::Task;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn detects_new_files_inside_directory_scope() {
        let workspace = tempdir().unwrap();
        let workspace_root = workspace.path();
        fs::create_dir_all(workspace_root.join("src")).unwrap();
        let snapshot_root = workspace_root.join("snapshot");
        fs::create_dir_all(&snapshot_root).unwrap();

        let scoped_paths = collect_scoped_paths(workspace_root, &["src".to_string()]);
        let before = snapshot_scope(&scoped_paths, workspace_root, &snapshot_root).unwrap();

        fs::write(workspace_root.join("src").join("new_file.rs"), "fn main() {}\n").unwrap();

        let (changed, created, deleted) = detect_scoped_changes(&scoped_paths, &before, workspace_root).unwrap();
        assert!(changed.is_empty());
        assert!(deleted.is_empty());
        assert_eq!(
            created,
            vec![PathBuf::from("src").join("new_file.rs").display().to_string()]
        );
    }

    #[test]
    fn ignores_new_files_outside_exact_file_scope() {
        let workspace = tempdir().unwrap();
        let workspace_root = workspace.path();
        fs::create_dir_all(workspace_root.join("src")).unwrap();
        let snapshot_root = workspace_root.join("snapshot");
        fs::create_dir_all(&snapshot_root).unwrap();

        let scoped_paths = collect_scoped_paths(workspace_root, &["src/lib.rs".to_string()]);
        let before = snapshot_scope(&scoped_paths, workspace_root, &snapshot_root).unwrap();

        fs::write(workspace_root.join("src").join("new_file.rs"), "pub fn helper() {}\n").unwrap();

        let (changed, created, deleted) = detect_scoped_changes(&scoped_paths, &before, workspace_root).unwrap();
        assert!(changed.is_empty());
        assert!(created.is_empty());
        assert!(deleted.is_empty());
    }

    #[test]
    fn detects_out_of_scope_created_files() {
        let workspace = tempdir().unwrap();
        let workspace_root = workspace.path();
        fs::create_dir_all(workspace_root.join("src")).unwrap();
        fs::create_dir_all(workspace_root.join("docs")).unwrap();
        let scoped_paths = collect_scoped_paths(workspace_root, &["src".to_string()]);
        let before_workspace = collect_workspace_files(workspace_root).unwrap();

        fs::write(workspace_root.join("src").join("in_scope.rs"), "fn scoped() {}\n").unwrap();
        fs::write(workspace_root.join("docs").join("rogue.md"), "rogue\n").unwrap();
        fs::create_dir_all(workspace_root.join(".velocity").join("agentic")).unwrap();
        fs::write(workspace_root.join(".velocity").join("agentic").join("ignored.txt"), "ignore\n").unwrap();

        let out_of_scope = detect_out_of_scope_created_files(&scoped_paths, &before_workspace, workspace_root).unwrap();

        assert_eq!(out_of_scope, vec![PathBuf::from("docs").join("rogue.md").display().to_string()]);
    }

    #[test]
    fn writes_execution_contract_as_nda() {
        let workspace = tempdir().unwrap();
        let assignment = WorkerAssignment {
            task: Task {
                id: super::TaskId(1),
                title: "demo".to_string(),
                description: "demo task".to_string(),
                scope: vec!["src/main.rs".to_string()],
                dependencies: Vec::new(),
                output: None,
            },
            workspace_root: workspace.path().to_path_buf(),
            instructions: "step one\nstep two".to_string(),
            planned_site_map_root: 42,
            provider: AiProvider::CloudflareWorkersAi,
            provider_label: "Workers AI".to_string(),
            model_id: "@cf/meta/llama-3.1-8b-instruct".to_string(),
            model_label: "Llama 3.1 8B".to_string(),
            thinking: true,
            fallback_chain: vec![RoutedModelRoute {
                provider: AiProvider::OpenRouter,
                model_id: "openrouter/sonnet".to_string(),
                model_label: "Sonnet".to_string(),
                thinking: false,
                score: 7,
            }],
        };

        write_execution_contract_artifacts(workspace.path(), &assignment).unwrap();
        let nda = fs::read_to_string(workspace.path().join("instructions.nda")).unwrap();
        let txt = fs::read_to_string(workspace.path().join("instructions.txt")).unwrap();

        assert!(nda.starts_with("worker-execution-contract version 2\n"));
        assert!(nda.contains("field\tprovider\tWorkers AI"));
        assert!(nda.contains("field\tthinking\ttrue"));
        assert!(nda.contains("field\ttask_id\t1"));
        assert!(nda.contains("field\tplanned_site_map_root\t000000000000002a"));
        assert!(nda.contains("scope\t0\tsrc/main.rs"));
        assert!(nda.contains("fallback_route\t0"));
        assert!(nda.contains("fallback_route_field\t0\tprovider\tOpenRouter"));
        assert!(nda.contains("fallback_route_field\t0\tmodel\tSonnet"));
        assert!(nda.contains("instruction_line_count 2"));
        assert!(nda.contains("instruction_line\t0\tstep one"));
        assert!(nda.contains("instruction_line\t1\tstep two"));
        assert!(txt.contains("provider: Workers AI"));
        assert!(txt.contains("thinking: true"));
    }

    #[test]
    fn writes_execution_facts_as_nda() {
        let workspace = tempdir().unwrap();
        let outcome = ExecutionOutcome {
            success: true,
            provider_label: "Workers AI".to_string(),
            model_label: "Llama 3.1 8B".to_string(),
            changed_files: vec!["src/main.rs".to_string()],
            created_files: vec!["src/new.rs".to_string()],
            deleted_files: vec!["src/old.rs".to_string()],
            out_of_scope_created_files: vec!["docs/rogue.md".to_string()],
            transcript: "done".to_string(),
            status_updates: vec!["updated scope".to_string()],
            attempts: vec![WorkerAttempt {
                provider_label: "Workers AI".to_string(),
                model_label: "Llama 3.1 8B".to_string(),
                model_id: "@cf/meta/llama-3.1-8b-instruct".to_string(),
                success: true,
                message: "Changed 1 file".to_string(),
            }],
            message: "Changed 1, created 1, deleted 1 via Workers AI / Llama 3.1 8B".to_string(),
        };

        write_execution_facts(workspace.path(), &outcome).unwrap();
        let facts = fs::read_to_string(workspace.path().join("facts.nda")).unwrap();

        assert!(facts.contains("artifact:worker-run result success"));
        assert!(facts.contains("artifact:worker-run changed file:src-main-rs"));
        assert!(facts.contains("artifact:worker-run out_of_scope_created file:docs-rogue-md"));
        assert!(facts.contains("attempt:1 result success"));
    }
}
