//! Step definition and execution types.
use serde::{Deserialize, Serialize};
use crate::{StepId, VirtualObjectId};
use std::collections::HashMap;

/// A unit of work within a workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub id: StepId,
    pub name: String,
    pub kind: StepKind,
    pub input: StepInput,
    pub timeout_ms: Option<u64>,
    pub retry_policy: RetryPolicy,
    pub target_object: Option<VirtualObjectId>,
}

/// The kind of work a step performs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StepKind {
    /// Execute a tool/command locally.
    Execute { command: String, args: Vec<String> },
    /// Call a remote service/function.
    Call { service: String, method: String },
    /// Transform data (pure function).
    Transform { expression: String },
    /// Wait for an external event.
    AwaitEvent { event_name: String },
    /// Branch based on condition.
    Branch { condition: String },
    /// No-op placeholder for dependency ordering.
    Barrier,
}

/// Input data for a step.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StepInput {
    pub params: HashMap<String, serde_json::Value>,
    pub depends_on: Vec<StepId>,
}

/// Retry policy for step execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub backoff_ms: u64,
    pub max_backoff_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self { max_retries: 3, backoff_ms: 100, max_backoff_ms: 5000 }
    }
}

/// Outcome of executing a step.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum StepOutcome {
    Ok { output: serde_json::Value, mutations: Vec<StateMutation> },
    Failed { error: String, retryable: bool },
    Pending { await_token: String },
}

/// A state mutation to be applied to a virtual object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateMutation {
    pub object_id: VirtualObjectId,
    pub operation: MutationOp,
}

/// The type of state mutation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MutationOp {
    Set { key: String, value: serde_json::Value },
    Delete { key: String },
    Increment { key: String, delta: i64 },
    Append { key: String, value: serde_json::Value },
}

/// Record of a completed step execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepRecord {
    pub step_id: StepId,
    pub outcome: StepOutcome,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: chrono::DateTime<chrono::Utc>,
    pub attempt: u32,
}

impl Step {
    pub fn new(name: impl Into<String>, kind: StepKind) -> Self {
        Self {
            id: StepId::new(),
            name: name.into(),
            kind,
            input: StepInput::default(),
            timeout_ms: None,
            retry_policy: RetryPolicy::default(),
            target_object: None,
        }
    }

    pub fn with_timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = Some(ms);
        self
    }

    pub fn with_retry(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    pub fn with_target_object(mut self, id: VirtualObjectId) -> Self {
        self.target_object = Some(id);
        self
    }

    pub fn depends_on(mut self, step_id: StepId) -> Self {
        self.input.depends_on.push(step_id);
        self
    }
}
