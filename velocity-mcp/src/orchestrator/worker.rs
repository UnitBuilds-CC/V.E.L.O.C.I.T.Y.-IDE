//! A worker represents one sub-agent assigned to a single task.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use velocity_ide::site_map::SiteMap;

use crate::agent::{AiProvider, HeadlessSubAgentRequest, run_headless_subagent};
use crate::automation::mediator::MediatorArena;

use super::blueprint::Task;
use super::TaskId;

/// Structured result produced by a worker after attempting a task.
#[derive(Debug, Clone)]
pub struct WorkerResult {
    pub success: bool,
    pub task_id: TaskId,
    pub outputs: Vec<String>,
    pub duration: Duration,
    pub message: String,
}

impl WorkerResult {
    pub fn new(task: &Task) -> Self {
        Self {
            success: true,
            task_id: task.id,
            outputs: Vec::new(),
            duration: Duration::ZERO,
            message: "ok".to_string(),
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
        Ok(outputs) => {
            result.outputs = outputs;
            result.duration = start.elapsed();
            result.message = format!(
                "captured {} scoped artifact(s) via {} / {}",
                result.outputs.len(),
                assignment.provider_label,
                assignment.model_label,
            );
        }
        Err(err) => {
            result.success = false;
            result.duration = start.elapsed();
            result.message = err;
        }
    }

    result
}

fn execute_live_task(
    assignment: &WorkerAssignment,
    run_dir: &Path,
    scope: &[String],
) -> Result<Vec<String>, String> {
    fs::create_dir_all(run_dir).map_err(|err| format!("create run dir: {err}"))?;
    fs::write(
        run_dir.join("instructions.txt"),
        format!(
            "provider: {}\nmodel: {}\nmodel_id: {}\n\n{}",
            assignment.provider_label, assignment.model_label, assignment.model_id, assignment.instructions
        ),
    )
    .map_err(|err| format!("write instructions: {err}"))?;

    let snapshot_root = run_dir.join("scope_snapshot");
    fs::create_dir_all(&snapshot_root).map_err(|err| format!("create snapshot dir: {err}"))?;

    let scoped_paths = collect_scoped_paths(&assignment.workspace_root, scope);
    let before_contents = snapshot_scope(&scoped_paths, &assignment.workspace_root, &snapshot_root)?;

    let subagent = run_headless_subagent(HeadlessSubAgentRequest {
        workspace_root: assignment.workspace_root.clone(),
        provider: assignment.provider,
        model: assignment.model_id.clone(),
        thinking: assignment.thinking,
        prompt: assignment.instructions.clone(),
    });

    let changed = detect_scoped_changes(&scoped_paths, &before_contents)?;
    let mut outputs: Vec<String> = changed
        .into_iter()
        .map(|path| path.strip_prefix(&assignment.workspace_root).unwrap_or(&path).display().to_string())
        .collect();
    outputs.sort();
    outputs.dedup();

    let mut summary = String::new();
    if !subagent.status_updates.is_empty() {
        summary.push_str("Status updates:\n");
        for line in &subagent.status_updates {
            summary.push_str("- ");
            summary.push_str(line);
            summary.push('\n');
        }
        summary.push('\n');
    }
    if !subagent.transcript.trim().is_empty() {
        summary.push_str("Transcript:\n");
        summary.push_str(subagent.transcript.trim());
        summary.push_str("\n\n");
    }
    if outputs.is_empty() {
        summary.push_str("No scoped file changes were produced by the provider-backed sub-agent.");
    } else {
        summary.push_str(&format!("Changed {} scoped file(s).", outputs.len()));
    }
    fs::write(run_dir.join("summary.txt"), &summary).map_err(|err| format!("write summary: {err}"))?;

    if outputs.is_empty() {
        Err(summary)
    } else {
        Ok(outputs)
    }
}

fn collect_scoped_paths(workspace_root: &Path, scope: &[String]) -> Vec<PathBuf> {
    scope
        .iter()
        .map(PathBuf::from)
        .map(|rel_path| if rel_path.is_absolute() { rel_path } else { workspace_root.join(rel_path) })
        .filter(|path| path.is_file())
        .collect()
}

fn snapshot_scope(
    scoped_paths: &[PathBuf],
    workspace_root: &Path,
    snapshot_root: &Path,
) -> Result<Vec<(PathBuf, Option<Vec<u8>>)>, String> {
    let mut before_contents = Vec::new();
    for abs_path in scoped_paths {
        let rel_path = abs_path.strip_prefix(workspace_root).unwrap_or(abs_path);
        let dest = snapshot_root.join(rel_path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("create snapshot parent: {err}"))?;
        }
        let bytes = fs::read(abs_path).map_err(|err| format!("snapshot file {}: {err}", abs_path.display()))?;
        fs::write(&dest, &bytes).map_err(|err| format!("write snapshot {}: {err}", dest.display()))?;
        before_contents.push((abs_path.clone(), Some(bytes)));
    }
    Ok(before_contents)
}

fn detect_scoped_changes(
    scoped_paths: &[PathBuf],
    before_contents: &[(PathBuf, Option<Vec<u8>>)],
) -> Result<Vec<PathBuf>, String> {
    let mut changed = Vec::new();
    for abs_path in scoped_paths {
        let before = before_contents
            .iter()
            .find(|(path, _)| path == abs_path)
            .and_then(|(_, bytes)| bytes.clone());
        let after = match fs::read(abs_path) {
            Ok(bytes) => Some(bytes),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => return Err(format!("read post-run file {}: {err}", abs_path.display())),
        };
        if after != before {
            changed.push(abs_path.clone());
        }
    }
    Ok(changed)
}
