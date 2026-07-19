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
    pub run_summary_path: Option<PathBuf>,
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
            run_summary_path: None,
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
            result.provider_label = execution.provider_label;
            result.model_label = execution.model_label;
            result.transcript = execution.transcript;
            result.status_updates = execution.status_updates;
            result.attempts = execution.attempts;
            result.run_summary_path = Some(run_dir.join("summary.txt"));
            result.duration = start.elapsed();
            result.message = execution.message;
        }
        Err(execution) => {
            result.success = false;
            result.outputs = execution.changed_files;
            result.created_files = execution.created_files;
            result.deleted_files = execution.deleted_files;
            result.provider_label = execution.provider_label;
            result.model_label = execution.model_label;
            result.transcript = execution.transcript;
            result.status_updates = execution.status_updates;
            result.attempts = execution.attempts;
            result.run_summary_path = Some(run_dir.join("summary.txt"));
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
    fs::write(
        run_dir.join("instructions.txt"),
        format!(
            "provider: {}\nmodel: {}\nmodel_id: {}\n\n{}",
            assignment.provider_label, assignment.model_label, assignment.model_id, assignment.instructions
        ),
    )
    .map_err(|err| failed_execution(assignment, format!("write instructions: {err}")))?;

    let snapshot_root = run_dir.join("scope_snapshot");
    fs::create_dir_all(&snapshot_root).map_err(|err| failed_execution(assignment, format!("create snapshot dir: {err}")))?;

    let scoped_paths = collect_scoped_paths(&assignment.workspace_root, scope);
    let before_contents = snapshot_scope(&scoped_paths, &assignment.workspace_root, &snapshot_root)
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
            let outcome = ExecutionOutcome {
                success: true,
                provider_label: final_provider_label,
                model_label: final_model_label,
                changed_files,
                created_files,
                deleted_files,
                transcript: last_transcript,
                status_updates: last_status_updates,
                attempts,
                message,
            };
            write_execution_summary(run_dir, &outcome).map_err(|err| failed_execution(assignment, err))?;
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
        transcript: last_transcript,
        status_updates: last_status_updates,
        attempts,
        message: "No scoped file changes were produced by any provider-backed sub-agent route.".to_string(),
    };
    write_execution_summary(run_dir, &outcome).map_err(|err| failed_execution(assignment, err))?;
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
        transcript: String::new(),
        status_updates: Vec::new(),
        attempts: Vec::new(),
        message,
    }
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
    if !outcome.transcript.trim().is_empty() {
        summary.push_str("\nTranscript:\n");
        summary.push_str(outcome.transcript.trim());
        summary.push('\n');
    }
    fs::write(run_dir.join("summary.txt"), &summary).map_err(|err| format!("write summary: {err}"))
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

#[cfg(test)]
mod tests {
    use super::{collect_scoped_paths, detect_scoped_changes, snapshot_scope};
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
}
