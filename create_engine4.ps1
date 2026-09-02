$base = "c:\Users\visse\OneDrive\Documents\Velocity-IDE\Velocity-IDE\shared\velocity-workflow-engine\src"

# engine.rs
@"
//! The Workflow Engine — ties together WAL, batching, concurrency, and execution.
//!
//! This is the central coordinator. It:
//! 1. Accepts workflow submissions
//! 2. Resolves step dependencies for parallel execution
//! 3. Executes ready steps concurrently (bounded by semaphore)
//! 4. Collects mutations into a batch buffer
//! 5. Commits batches to the WAL when buffer reaches `sync_steps`
//! 6. Applies committed mutations to virtual objects
//! 7. Handles crash recovery via WAL replay

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Semaphore, Mutex, Notify};
use velocity_workflow_core::*;
use crate::wal::{WriteAheadLog, WalEntry};
use crate::executor::{StepExecutor, LocalExecutor, ExecutionContext};
use crate::worker_pool::WorkerPool;

/// The main workflow engine.
pub struct WorkflowEngine {
    config: EngineConfig,
    wal: WriteAheadLog,
    executor: Arc<dyn StepExecutor>,
    object_store: Arc<Mutex<VirtualObjectStore>>,
    active_runs: Arc<Mutex<HashMap<RunId, WorkflowRunState>>>,
    worker_pool: Option<Arc<WorkerPool>>,
    run_semaphore: Arc<Semaphore>,
    step_semaphore: Arc<Semaphore>,
    batch_buffer: Arc<Mutex<Vec<WalEntry>>>,
    shutdown: Arc<Notify>,
}

impl WorkflowEngine {
    /// Create a new engine with the given configuration.
    pub async fn new(config: EngineConfig) -> WorkflowResult<Self> {
        config.validate().map_err(|e| WorkflowError::Internal(e))?;

        let wal = WriteAheadLog::open(&config.journal_dir.join("workflow.wal"))?;
        let run_semaphore = Arc::new(Semaphore::new(config.max_concurrent_runs));
        let step_semaphore = Arc::new(Semaphore::new(config.max_step_parallelism));

        Ok(Self {
            config,
            wal,
            executor: Arc::new(LocalExecutor),
            object_store: Arc::new(Mutex::new(VirtualObjectStore::new())),
            active_runs: Arc::new(Mutex::new(HashMap::new())),
            worker_pool: None,
            run_semaphore,
            step_semaphore,
            batch_buffer: Arc::new(Mutex::new(Vec::new())),
            shutdown: Arc::new(Notify::new()),
        })
    }

    /// Create an engine with a custom executor (for testing or remote execution).
    pub async fn with_executor(config: EngineConfig, executor: Arc<dyn StepExecutor>) -> WorkflowResult<Self> {
        let mut engine = Self::new(config).await?;
        engine.executor = executor;
        Ok(engine)
    }

    /// Set the worker pool for remote step execution.
    pub fn set_worker_pool(&mut self, pool: Arc<WorkerPool>) {
        self.worker_pool = Some(pool);
    }

    /// Submit a workflow for execution. Returns the run ID immediately.
    pub async fn submit(&self, workflow: &Workflow) -> WorkflowResult<RunId> {
        let run_id = RunId::new();
        let state = WorkflowRunState::new(
            run_id.clone(),
            workflow.id.clone(),
            workflow.steps.len(),
        );

        self.wal.save_run_state(&state)?;
        {
            let mut runs = self.active_runs.lock().await;
            runs.insert(run_id.clone(), state);
        }

        Ok(run_id)
    }

    /// Execute a workflow to completion.
    pub async fn execute(&self, workflow: &Workflow) -> WorkflowResult<WorkflowRunState> {
        let _permit = self.run_semaphore.acquire().await
            .map_err(|_| WorkflowError::Internal("semaphore closed".into()))?;

        let run_id = RunId::new();
        let mut state = WorkflowRunState::new(
            run_id.clone(),
            workflow.id.clone(),
            workflow.steps.len(),
        );
        state.transition_to(RunState::Running);

        let mut completed_ids: Vec<StepId> = Vec::new();
        let mut batch = Vec::new();

        // Execute steps in dependency order with parallelism.
        loop {
            let ready = workflow.ready_steps(&completed_ids);
            if ready.is_empty() {
                if completed_ids.len() == workflow.steps.len() {
                    break;
                } else {
                    return Err(WorkflowError::Internal(
                        "deadlock: no ready steps but not all complete".into()
                    ));
                }
            }

            // Execute ready steps in parallel (bounded by semaphore).
            let mut handles = Vec::new();
            for (idx, step) in &ready {
                let step = (*step).clone();
                let idx = *idx;
                let executor = self.executor.clone();
                let ctx = ExecutionContext::new(
                    run_id.clone(),
                    workflow.id.clone(),
                    step.id.clone(),
                );
                let sem = self.step_semaphore.clone();

                let handle = tokio::spawn(async move {
                    let _permit = sem.acquire().await.unwrap();
                    let start = std::time::Instant::now();
                    let outcome = executor.execute_step(&step, &ctx).await;
                    (idx, step.id.clone(), outcome, start.elapsed())
                });
                handles.push(handle);
            }

            // Collect results.
            for handle in handles {
                let (idx, step_id, outcome, duration) = handle.await
                    .map_err(|e| WorkflowError::Internal(format!("task join: {e}")))?;

                let record = StepRecord {
                    step_id: step_id.clone(),
                    outcome: outcome.clone(),
                    started_at: chrono::Utc::now(),
                    finished_at: chrono::Utc::now(),
                    attempt: 1,
                };

                // Extract mutations from successful steps.
                let mutations = match &outcome {
                    StepOutcome::Ok { mutations, .. } => mutations.clone(),
                    _ => vec![],
                };

                // Add to batch buffer.
                batch.push(WalEntry {
                    sequence: 0,
                    run_id: run_id.clone(),
                    step_id: step_id.clone(),
                    outcome: outcome.clone(),
                    mutations: mutations.clone(),
                    timestamp: chrono::Utc::now(),
                });

                state.record_step(record);

                // Add mutations to pending.
                for m in &mutations {
                    state.add_pending_mutation(m.clone());
                }

                if matches!(outcome, StepOutcome::Ok { .. }) {
                    completed_ids.push(step_id);
                }

                // Check if we should flush the batch.
                if batch.len() >= self.config.sync_steps {
                    self.flush_batch(&mut batch).await?;
                    // Apply mutations to virtual objects.
                    let pending = state.take_pending_mutations();
                    let mut store = self.object_store.lock().await;
                    store.apply_mutations(&pending);
                    store.mark_all_clean();
                }
            }
        }

        // Final flush — commit any remaining buffered entries.
        if !batch.is_empty() {
            self.flush_batch(&mut batch).await?;
            let pending = state.take_pending_mutations();
            let mut store = self.object_store.lock().await;
            store.apply_mutations(&pending);
            store.mark_all_clean();
        }

        state.transition_to(RunState::Completed);
        self.wal.save_run_state(&state)?;

        {
            let mut runs = self.active_runs.lock().await;
            runs.insert(run_id.clone(), state.clone());
        }

        Ok(state)
    }

    /// Flush the batch buffer to the WAL in a single transaction.
    async fn flush_batch(&self, batch: &mut Vec<WalEntry>) -> WorkflowResult<()> {
        if batch.is_empty() { return Ok(()); }

        let seq = self.wal.append_batch(batch)?;
        tracing::debug!(
            sequence = seq,
            entries = batch.len(),
            "flushed batch to WAL"
        );
        batch.clear();
        Ok(())
    }

    /// Recover a workflow run from the WAL after a crash.
    pub async fn recover_run(&self, run_id: &RunId) -> WorkflowResult<WorkflowRunState> {
        let entries = self.wal.replay_run(run_id)?;
        let saved = self.wal.load_run_state(run_id)?;

        let mut state = match saved {
            Some(s) => s,
            None => return Err(WorkflowError::RunNotFound(run_id.clone())),
        };

        // Replay mutations from WAL entries.
        let mut store = self.object_store.lock().await;
        for entry in &entries {
            if let StepOutcome::Ok { mutations, .. } = &entry.outcome {
                store.apply_mutations(mutations);
            }
        }
        store.mark_all_clean();

        // Update state from replayed entries.
        state.steps_completed = entries.iter()
            .filter(|e| matches!(e.outcome, StepOutcome::Ok { .. }))
            .count();

        Ok(state)
    }

    /// Register a virtual object with the engine.
    pub async fn register_object(&self, obj: VirtualObject) {
        let mut store = self.object_store.lock().await;
        store.register(obj);
    }

    /// Get the current state of a virtual object.
    pub async fn get_object(&self, id: &VirtualObjectId) -> Option<VirtualObject> {
        let store = self.object_store.lock().await;
        store.get(id).cloned()
    }

    /// Get the state of an active run.
    pub async fn get_run_state(&self, run_id: &RunId) -> Option<WorkflowRunState> {
        let runs = self.active_runs.lock().await;
        runs.get(run_id).cloned()
    }

    /// Get engine statistics.
    pub async fn stats(&self) -> EngineStats {
        let store = self.object_store.lock().await;
        let runs = self.active_runs.lock().await;
        let batch = self.batch_buffer.lock().await;

        EngineStats {
            active_runs: runs.len(),
            total_objects: store.dirty_objects().len(),
            dirty_objects: store.dirty_count(),
            pending_batch: batch.len(),
            sync_steps: self.config.sync_steps,
        }
    }

    /// Signal the engine to shut down gracefully.
    pub fn shutdown(&self) {
        self.shutdown.notify_waiters();
    }
}

/// Engine statistics.
#[derive(Debug, Clone)]
pub struct EngineStats {
    pub active_runs: usize,
    pub total_objects: usize,
    pub dirty_objects: usize,
    pub pending_batch: usize,
    pub sync_steps: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_engine() -> WorkflowEngine {
        let config = EngineConfig {
            sync_steps: 5,
            journal_dir: std::path::PathBuf::from(".velocity/test-journal"),
            ..Default::default()
        };
        // Use in-memory WAL for tests.
        let mut engine = WorkflowEngine {
            config: config.clone(),
            wal: WriteAheadLog::open_memory().unwrap(),
            executor: Arc::new(LocalExecutor),
            object_store: Arc::new(Mutex::new(VirtualObjectStore::new())),
            active_runs: Arc::new(Mutex::new(HashMap::new())),
            worker_pool: None,
            run_semaphore: Arc::new(Semaphore::new(config.max_concurrent_runs)),
            step_semaphore: Arc::new(Semaphore::new(config.max_step_parallelism)),
            batch_buffer: Arc::new(Mutex::new(Vec::new())),
            shutdown: Arc::new(Notify::new()),
        };
        engine
    }

    #[tokio::test]
    async fn engine_submit_and_execute() {
        let engine = test_engine().await;
        let mut wf = Workflow::new("test-wf");
        wf.add_step(Step::new("step1", StepKind::Barrier));
        wf.add_step(Step::new("step2", StepKind::Barrier));

        let state = engine.execute(&wf).await.unwrap();
        assert_eq!(state.state, RunState::Completed);
        assert_eq!(state.steps_completed, 2);
    }

    #[tokio::test]
    async fn engine_parallel_steps() {
        let engine = test_engine().await;
        let mut wf = Workflow::new("parallel-wf");
        // Two independent steps (no deps) should run in parallel.
        wf.add_step(Step::new("a", StepKind::Barrier));
        wf.add_step(Step::new("b", StepKind::Barrier));
        wf.add_step(Step::new("c", StepKind::Barrier));

        let state = engine.execute(&wf).await.unwrap();
        assert_eq!(state.steps_completed, 3);
        assert_eq!(state.state, RunState::Completed);
    }

    #[tokio::test]
    async fn engine_batching_flush() {
        let engine = test_engine().await;
        let mut wf = Workflow::new("batch-wf");
        // Add more steps than sync_steps to trigger a flush.
        for i in 0..12 {
            wf.add_step(Step::new(format!("step-{i}"), StepKind::Barrier));
        }

        let state = engine.execute(&wf).await.unwrap();
        assert_eq!(state.steps_completed, 12);
    }

    #[tokio::test]
    async fn engine_with_dependencies() {
        let engine = test_engine().await;
        let mut wf = Workflow::new("dep-wf");
        let s1 = Step::new("first", StepKind::Barrier);
        let s1_id = s1.id.clone();
        wf.add_step(s1);
        wf.add_step(Step::new("second", StepKind::Barrier).depends_on(s1_id));

        let state = engine.execute(&wf).await.unwrap();
        assert_eq!(state.steps_completed, 2);
    }

    #[tokio::test]
    async fn engine_stats() {
        let engine = test_engine().await;
        let stats = engine.stats().await;
        assert_eq!(stats.active_runs, 0);
        assert_eq!(stats.sync_steps, 5);
    }
}
"@ | Set-Content -Path "$base\engine.rs" -Encoding UTF8
Write-Host "Created engine.rs"
