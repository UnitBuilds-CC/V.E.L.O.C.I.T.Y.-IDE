$base = "c:\Users\visse\OneDrive\Documents\Velocity-IDE\Velocity-IDE\shared\velocity-workflow-core\src"

# error.rs
@"
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
"@ | Set-Content -Path "$base\error.rs" -Encoding UTF8
Write-Host "Created error.rs"

# step.rs
@"
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
"@ | Set-Content -Path "$base\step.rs" -Encoding UTF8
Write-Host "Created step.rs"

# virtual_object.rs
@"
//! Virtual objects — Restate-style batchable state containers.
//!
//! Virtual objects encapsulate state that can be mutated by workflow steps.
//! Instead of persisting each mutation immediately (fsync per step), mutations
//! are collected and committed in batches, dramatically reducing I/O overhead.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::{VirtualObjectId, StateMutation, MutationOp};

/// A virtual object — a named, keyed state container.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualObject {
    pub id: VirtualObjectId,
    pub name: String,
    state: HashMap<String, serde_json::Value>,
    version: u64,
    dirty: bool,
}

impl VirtualObject {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: VirtualObjectId::new(),
            name: name.into(),
            state: HashMap::new(),
            version: 0,
            dirty: false,
        }
    }

    pub fn with_id(mut self, id: VirtualObjectId) -> Self {
        self.id = id;
        self
    }

    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.state.get(key)
    }

    pub fn set(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.state.insert(key.into(), value);
        self.dirty = true;
    }

    pub fn delete(&mut self, key: &str) -> Option<serde_json::Value> {
        let removed = self.state.remove(key);
        if removed.is_some() { self.dirty = true; }
        removed
    }

    pub fn increment(&mut self, key: impl Into<String>, delta: i64) -> i64 {
        let key = key.into();
        let current = self.state.get(&key)
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let new_val = current + delta;
        self.state.insert(key, serde_json::Value::from(new_val));
        self.dirty = true;
        new_val
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.state.keys()
    }

    pub fn state(&self) -> &HashMap<String, serde_json::Value> {
        &self.state
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Apply a mutation to this object.
    pub fn apply_mutation(&mut self, mutation: &StateMutation) {
        match &mutation.operation {
            MutationOp::Set { key, value } => { self.set(key.clone(), value.clone()); }
            MutationOp::Delete { key } => { self.delete(key); }
            MutationOp::Increment { key, delta } => { self.increment(key.clone(), *delta); }
            MutationOp::Append { key, value } => {
                let entry = self.state.entry(key.clone()).or_insert_with(|| serde_json::json!([]));
                if let Some(arr) = entry.as_array_mut() {
                    arr.push(value.clone());
                    self.dirty = true;
                }
            }
        }
    }

    /// Mark as persisted (called after batch commit).
    pub fn mark_clean(&mut self) {
        self.dirty = false;
        self.version += 1;
    }
}

/// Registry of virtual objects for a workflow run.
#[derive(Debug, Default)]
pub struct VirtualObjectStore {
    objects: HashMap<VirtualObjectId, VirtualObject>,
}

impl VirtualObjectStore {
    pub fn new() -> Self { Self { objects: HashMap::new() } }

    pub fn register(&mut self, obj: VirtualObject) {
        self.objects.insert(obj.id.clone(), obj);
    }

    pub fn get(&self, id: &VirtualObjectId) -> Option<&VirtualObject> {
        self.objects.get(id)
    }

    pub fn get_mut(&mut self, id: &VirtualObjectId) -> Option<&mut VirtualObject> {
        self.objects.get_mut(id)
    }

    /// Apply a batch of mutations, returning the IDs of affected objects.
    pub fn apply_mutations(&mut self, mutations: &[StateMutation]) -> Vec<VirtualObjectId> {
        let mut affected = Vec::new();
        for m in mutations {
            if let Some(obj) = self.objects.get_mut(&m.object_id) {
                obj.apply_mutation(m);
                if !affected.contains(&m.object_id) {
                    affected.push(m.object_id.clone());
                }
            }
        }
        affected
    }

    /// Get all dirty objects (pending commit).
    pub fn dirty_objects(&self) -> Vec<&VirtualObject> {
        self.objects.values().filter(|o| o.is_dirty()).collect()
    }

    /// Mark all objects as clean (after batch commit).
    pub fn mark_all_clean(&mut self) {
        for obj in self.objects.values_mut() {
            obj.mark_clean();
        }
    }

    /// Number of dirty objects pending commit.
    pub fn dirty_count(&self) -> usize {
        self.objects.values().filter(|o| o.is_dirty()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_object_set_and_get() {
        let mut obj = VirtualObject::new("counter");
        obj.set("count", serde_json::json!(42));
        assert_eq!(obj.get("count").unwrap(), &serde_json::json!(42));
        assert!(obj.is_dirty());
    }

    #[test]
    fn virtual_object_increment() {
        let mut obj = VirtualObject::new("counter");
        assert_eq!(obj.increment("count", 5), 5);
        assert_eq!(obj.increment("count", 3), 8);
    }

    #[test]
    fn virtual_object_mark_clean() {
        let mut obj = VirtualObject::new("test");
        obj.set("key", serde_json::json!("value"));
        assert!(obj.is_dirty());
        obj.mark_clean();
        assert!(!obj.is_dirty());
        assert_eq!(obj.version(), 1);
    }

    #[test]
    fn store_apply_mutations() {
        let mut store = VirtualObjectStore::new();
        let obj_id = VirtualObjectId::from_str("obj-1");
        store.register(VirtualObject::new("test").with_id(obj_id.clone()));
        let mutations = vec![
            StateMutation {
                object_id: obj_id.clone(),
                operation: MutationOp::Set { key: "x".into(), value: serde_json::json!(10) },
            },
        ];
        let affected = store.apply_mutations(&mutations);
        assert_eq!(affected.len(), 1);
        assert_eq!(store.get(&obj_id).unwrap().get("x").unwrap(), &serde_json::json!(10));
        assert_eq!(store.dirty_count(), 1);
    }
}
"@ | Set-Content -Path "$base\virtual_object.rs" -Encoding UTF8
Write-Host "Created virtual_object.rs"
