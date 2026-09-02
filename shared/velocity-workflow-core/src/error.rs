//! Error types for the workflow engine.
use thiserror::Error;
use crate::{RunId, StepId, WorkflowId};

#[derive(Debug, Error)]
pub enum WorkflowError {
    #[error("workflow not found: {0}")]
    WorkflowNotFound(WorkflowId),
    #[error("run not found: {0}")]
    RunNotFound(RunId),
    #[error("step failed: run={run_id} step={step_id} error={error}")]
    StepFailed { run_id: RunId, step_id: StepId, error: String },
    #[error("persistence error: {0}")]
    Persistence(String),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("timeout: run={run_id} step={step_id}")]
    Timeout { run_id: RunId, step_id: StepId },
    #[error("cancelled: run={0}")]
    Cancelled(RunId),
    #[error("worker unavailable: {0}")]
    WorkerUnavailable(String),
    #[error("internal error: {0}")]
    Internal(String),
}

pub type WorkflowResult<T> = Result<T, WorkflowError>;
