//! A worker represents one sub-agent assigned to a single task.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use velocity_ide::site_map::SiteMap;

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
    pub provider_label: String,
    pub model_label: String,
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
            "provider: {}\nmodel: {}\n\n{}",
            assignment.provider_label, assignment.model_label, assignment.instructions
        ),
    )
    .map_err(|err| format!("write instructions: {err}"))?;

    let snapshot_root = run_dir.join("scope_snapshot");
    fs::create_dir_all(&snapshot_root).map_err(|err| format!("create snapshot dir: {err}"))?;

    let mut outputs = Vec::new();
    for entry in scope {
        let rel_path = PathBuf::from(entry);
        let abs_path = if rel_path.is_absolute() {
            rel_path.clone()
        } else {
            assignment.workspace_root.join(&rel_path)
        };
        if abs_path.is_file() {
            let dest = snapshot_root.join(&rel_path);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|err| format!("create snapshot parent: {err}"))?;
            }
            fs::copy(&abs_path, &dest).map_err(|err| format!("snapshot file {}: {err}", abs_path.display()))?;
            outputs.push(rel_path.display().to_string());
        }
    }

    let summary = if outputs.is_empty() {
        "No existing scoped files were available to snapshot.".to_string()
    } else {
        format!("Snapshotted {} scoped file(s).", outputs.len())
    };
    fs::write(run_dir.join("summary.txt"), &summary).map_err(|err| format!("write summary: {err}"))?;

    if outputs.is_empty() {
        Err(summary)
    } else {
        Ok(outputs)
    }
}
