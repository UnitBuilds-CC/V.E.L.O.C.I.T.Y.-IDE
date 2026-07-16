//! A worker represents one sub-agent assigned to a single task.
//!
//! Currently this is a strongly-typed skeleton.  In a multi-agent deployment it
//! would be backed by a spawned child process or LLM call.  Keeping it
//! dependency-light lets us test the scheduler and reconcile logic without
//! needing a live sub-agent.

use std::path::PathBuf;
use std::time::{Duration, Instant};

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

/// A concrete mock worker that immediately succeeds.
pub struct MockWorker {
    result: WorkerResult,
}

impl MockWorker {
    pub fn new(task: &Task) -> Self {
        Self { result: WorkerResult::new(task) }
    }
}

impl WorkerHandle for MockWorker {
    fn poll(&mut self) -> Option<WorkerResult> {
        let mut result = self.result.clone();
        result.duration = Duration::from_millis(10);
        Some(result)
    }
}

/// The typed task card handed to a worker launcher.
pub struct WorkerAssignment<'a> {
    pub task_id: TaskId,
    pub spec: &'a str,
    pub worktree: PathBuf,
}
