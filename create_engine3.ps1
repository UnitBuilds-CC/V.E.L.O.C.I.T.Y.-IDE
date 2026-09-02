$base = "c:\Users\visse\OneDrive\Documents\Velocity-IDE\Velocity-IDE\shared\velocity-workflow-engine\src"

# executor.rs
@"
//! Step executor — runs individual steps locally or dispatches to remote workers.
use async_trait::async_trait;
use velocity_workflow_core::*;
use std::collections::HashMap;

/// Trait for step execution — can be implemented for local or remote execution.
#[async_trait]
pub trait StepExecutor: Send + Sync {
    /// Execute a step and return its outcome plus any state mutations.
    async fn execute_step(
        &self,
        step: &Step,
        context: &ExecutionContext,
    ) -> WorkflowResult<StepOutcome>;

    /// The name of this executor (for logging/metrics).
    fn name(&self) -> &str;
}

/// Context available to a step during execution.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub run_id: RunId,
    pub workflow_id: WorkflowId,
    pub step_id: StepId,
    pub object_state: HashMap<VirtualObjectId, HashMap<String, serde_json::Value>>,
    pub outputs: HashMap<String, serde_json::Value>,
}

impl ExecutionContext {
    pub fn new(run_id: RunId, workflow_id: WorkflowId, step_id: StepId) -> Self {
        Self {
            run_id,
            workflow_id,
            step_id,
            object_state: HashMap::new(),
            outputs: HashMap::new(),
        }
    }
}

/// Local step executor — runs steps in the current process.
pub struct LocalExecutor;

#[async_trait]
impl StepExecutor for LocalExecutor {
    async fn execute_step(
        &self,
        step: &Step,
        context: &ExecutionContext,
    ) -> WorkflowResult<StepOutcome> {
        match &step.kind {
            StepKind::Execute { command, args } => {
                let output = tokio::process::Command::new(command)
                    .args(args)
                    .output()
                    .await
                    .map_err(|e| WorkflowError::StepFailed {
                        run_id: context.run_id.clone(),
                        step_id: step.id.clone(),
                        error: format!("execute command: {e}"),
                    })?;

                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    Ok(StepOutcome::Ok {
                        output: serde_json::json!({ "stdout": stdout, "exit_code": 0 }),
                        mutations: vec![],
                    })
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    Ok(StepOutcome::Failed {
                        error: format!("exit {}: {}", output.status, stderr),
                        retryable: true,
                    })
                }
            }
            StepKind::Call { service, method } => {
                Ok(StepOutcome::Ok {
                    output: serde_json::json!({
                        "service": service,
                        "method": method,
                        "status": "dispatched",
                    }),
                    mutations: vec![],
                })
            }
            StepKind::Transform { expression } => {
                Ok(StepOutcome::Ok {
                    output: serde_json::json!({ "transform": expression, "result": "applied" }),
                    mutations: vec![],
                })
            }
            StepKind::AwaitEvent { event_name } => {
                Ok(StepOutcome::Pending {
                    await_token: format!("event:{}", event_name),
                })
            }
            StepKind::Branch { condition } => {
                Ok(StepOutcome::Ok {
                    output: serde_json::json!({ "branch": condition, "taken": true }),
                    mutations: vec![],
                })
            }
            StepKind::Barrier => {
                Ok(StepOutcome::Ok {
                    output: serde_json::json!(null),
                    mutations: vec![],
                })
            }
        }
    }

    fn name(&self) -> &str { "local" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_executor_barrier() {
        let exec = LocalExecutor;
        let step = Step::new("noop", StepKind::Barrier);
        let ctx = ExecutionContext::new(RunId::new(), WorkflowId::new(), step.id.clone());
        let outcome = exec.execute_step(&step, &ctx).await.unwrap();
        assert!(matches!(outcome, StepOutcome::Ok { .. }));
    }

    #[tokio::test]
    async fn local_executor_transform() {
        let exec = LocalExecutor;
        let step = Step::new("transform", StepKind::Transform { expression: "x + 1".into() });
        let ctx = ExecutionContext::new(RunId::new(), WorkflowId::new(), step.id.clone());
        let outcome = exec.execute_step(&step, &ctx).await.unwrap();
        assert!(matches!(outcome, StepOutcome::Ok { .. }));
    }
}
"@ | Set-Content -Path "$base\executor.rs" -Encoding UTF8
Write-Host "Created executor.rs"

# worker_pool.rs
@"
//! Worker pool — manages remote step execution workers.
//!
//! Workers register with the engine, advertise capabilities, and receive
//! step assignments via a task queue. The pool handles heartbeats, health
//! checks, and failover.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use velocity_workflow_core::*;
use serde::{Serialize, Deserialize};

/// A registered worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worker {
    pub id: String,
    pub address: String,
    pub capabilities: Vec<String>,
    pub registered_at: chrono::DateTime<chrono::Utc>,
    pub last_heartbeat: chrono::DateTime<chrono::Utc>,
    pub active_tasks: usize,
    pub max_tasks: usize,
    pub status: WorkerStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerStatus {
    Idle,
    Busy,
    Draining,
    Offline,
}

/// A task in the queue awaiting a worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAssignment {
    pub id: String,
    pub run_id: RunId,
    pub step_id: StepId,
    pub step_name: String,
    pub input: serde_json::Value,
    pub enqueued_at: chrono::DateTime<chrono::Utc>,
    pub priority: u32,
}

/// The worker pool manages remote execution workers.
pub struct WorkerPool {
    workers: Arc<Mutex<HashMap<String, Worker>>>,
    task_queue: Arc<Mutex<Vec<TaskAssignment>>>,
    heartbeat_timeout: Duration,
}

impl WorkerPool {
    pub fn new(heartbeat_timeout_secs: u64) -> Self {
        Self {
            workers: Arc::new(Mutex::new(HashMap::new())),
            task_queue: Arc::new(Mutex::new(Vec::new())),
            heartbeat_timeout: Duration::from_secs(heartbeat_timeout_secs),
        }
    }

    /// Register a new worker.
    pub fn register_worker(&self, worker: Worker) {
        let mut workers = self.workers.lock().unwrap();
        workers.insert(worker.id.clone(), worker);
    }

    /// Update a worker's heartbeat.
    pub fn heartbeat(&self, worker_id: &str) -> bool {
        let mut workers = self.workers.lock().unwrap();
        if let Some(w) = workers.get_mut(worker_id) {
            w.last_heartbeat = chrono::Utc::now();
            true
        } else {
            false
        }
    }

    /// Enqueue a task for remote execution.
    pub fn enqueue_task(&self, task: TaskAssignment) {
        let mut queue = self.task_queue.lock().unwrap();
        queue.push(task);
        queue.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    /// Dequeue the next task for a worker with matching capabilities.
    pub fn dequeue_task(&self, worker_id: &str) -> Option<TaskAssignment> {
        let workers = self.workers.lock().unwrap();
        let worker = workers.get(worker_id)?;
        if worker.active_tasks >= worker.max_tasks {
            return None;
        }

        let mut queue = self.task_queue.lock().unwrap();
        let idx = queue.iter().position(|t| {
            worker.capabilities.iter().any(|c| c == "*") ||
            worker.capabilities.contains(&t.step_name)
        })?;
        Some(queue.remove(idx))
    }

    /// Find idle workers.
    pub fn idle_workers(&self) -> Vec<Worker> {
        let workers = self.workers.lock().unwrap();
        workers.values()
            .filter(|w| w.status == WorkerStatus::Idle && w.active_tasks < w.max_tasks)
            .cloned()
            .collect()
    }

    /// Mark stale workers as offline.
    pub fn evict_stale_workers(&self) -> Vec<String> {
        let now = chrono::Utc::now();
        let mut workers = self.workers.lock().unwrap();
        let mut evicted = Vec::new();

        for (id, w) in workers.iter_mut() {
            let elapsed = now.signed_duration_since(w.last_heartbeat);
            if elapsed.to_std().unwrap_or(Duration::MAX) > self.heartbeat_timeout {
                w.status = WorkerStatus::Offline;
                evicted.push(id.clone());
            }
        }
        evicted
    }

    /// Number of pending tasks.
    pub fn pending_tasks(&self) -> usize {
        self.task_queue.lock().unwrap().len()
    }

    /// Number of registered workers.
    pub fn worker_count(&self) -> usize {
        self.workers.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_worker(id: &str) -> Worker {
        Worker {
            id: id.to_string(),
            address: format!("http://{id}:8080"),
            capabilities: vec!["*".to_string()],
            registered_at: chrono::Utc::now(),
            last_heartbeat: chrono::Utc::now(),
            active_tasks: 0,
            max_tasks: 4,
            status: WorkerStatus::Idle,
        }
    }

    #[test]
    fn register_and_heartbeat() {
        let pool = WorkerPool::new(30);
        pool.register_worker(test_worker("w1"));
        assert_eq!(pool.worker_count(), 1);
        assert!(pool.heartbeat("w1"));
        assert!(!pool.heartbeat("nonexistent"));
    }

    #[test]
    fn enqueue_and_dequeue() {
        let pool = WorkerPool::new(30);
        pool.register_worker(test_worker("w1"));

        let task = TaskAssignment {
            id: "t1".into(),
            run_id: RunId::new(),
            step_id: StepId::new(),
            step_name: "transform".into(),
            input: serde_json::json!({}),
            enqueued_at: chrono::Utc::now(),
            priority: 1,
        };
        pool.enqueue_task(task);
        assert_eq!(pool.pending_tasks(), 1);

        let dequeued = pool.dequeue_task("w1").unwrap();
        assert_eq!(dequeued.id, "t1");
        assert_eq!(pool.pending_tasks(), 0);
    }

    #[test]
    fn idle_workers_filter() {
        let pool = WorkerPool::new(30);
        let mut w1 = test_worker("w1");
        w1.active_tasks = 4;
        w1.status = WorkerStatus::Busy;
        pool.register_worker(w1);
        pool.register_worker(test_worker("w2"));

        let idle = pool.idle_workers();
        assert_eq!(idle.len(), 1);
        assert_eq!(idle[0].id, "w2");
    }
}
"@ | Set-Content -Path "$base\worker_pool.rs" -Encoding UTF8
Write-Host "Created worker_pool.rs"
