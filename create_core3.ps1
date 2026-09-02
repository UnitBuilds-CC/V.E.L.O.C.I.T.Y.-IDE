$base = "c:\Users\visse\OneDrive\Documents\Velocity-IDE\Velocity-IDE\shared\velocity-workflow-core\src"

# workflow.rs
@"
//! Workflow definition — a named, ordered sequence of steps.
use serde::{Deserialize, Serialize};
use crate::{WorkflowId, Step};

/// A workflow definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub id: WorkflowId,
    pub name: String,
    pub steps: Vec<Step>,
    pub max_concurrency: usize,
    pub description: Option<String>,
}

impl Workflow {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: WorkflowId::new(),
            name: name.into(),
            steps: Vec::new(),
            max_concurrency: 1,
            description: None,
        }
    }

    pub fn add_step(&mut self, step: Step) {
        self.steps.push(step);
    }

    pub fn with_concurrency(mut self, n: usize) -> Self {
        self.max_concurrency = n;
        self
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Compute the dependency graph: returns adjacency list of step index -> dependent step indices.
    pub fn dependency_graph(&self) -> Vec<Vec<usize>> {
        let mut graph = vec![Vec::new(); self.steps.len()];
        for (i, step) in self.steps.iter().enumerate() {
            for dep_id in &step.input.depends_on {
                if let Some(dep_idx) = self.steps.iter().position(|s| &s.id == dep_id) {
                    graph[dep_idx].push(i);
                }
            }
        }
        graph
    }

    /// Topological sort of steps respecting dependencies.
    /// Returns None if there is a cycle.
    pub fn topological_order(&self) -> Option<Vec<usize>> {
        let n = self.steps.len();
        let mut in_degree = vec![0usize; n];
        let graph = self.dependency_graph();

        for deps in &graph {
            for &dep in deps {
                in_degree[dep] += 1;
            }
        }

        let mut queue: Vec<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
        let mut order = Vec::with_capacity(n);

        while let Some(node) = queue.pop() {
            order.push(node);
            for &dep in &graph[node] {
                in_degree[dep] -= 1;
                if in_degree[dep] == 0 {
                    queue.push(dep);
                }
            }
        }

        if order.len() == n { Some(order) } else { None }
    }

    /// Get steps that have no unresolved dependencies given a set of completed step IDs.
    pub fn ready_steps(&self, completed: &[crate::StepId]) -> Vec<(usize, &Step)> {
        self.steps.iter().enumerate().filter(|(_, step)| {
            step.input.depends_on.iter().all(|dep| completed.contains(dep))
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{StepKind, StepId};

    #[test]
    fn workflow_add_steps() {
        let mut wf = Workflow::new("test");
        wf.add_step(Step::new("step1", StepKind::Barrier));
        wf.add_step(Step::new("step2", StepKind::Barrier));
        assert_eq!(wf.steps.len(), 2);
    }

    #[test]
    fn topological_order_no_deps() {
        let mut wf = Workflow::new("test");
        wf.add_step(Step::new("a", StepKind::Barrier));
        wf.add_step(Step::new("b", StepKind::Barrier));
        let order = wf.topological_order().unwrap();
        assert_eq!(order.len(), 2);
    }

    #[test]
    fn topological_order_with_deps() {
        let mut wf = Workflow::new("test");
        let s1 = Step::new("first", StepKind::Barrier);
        let s1_id = s1.id.clone();
        wf.add_step(s1);
        wf.add_step(Step::new("second", StepKind::Barrier).depends_on(s1_id));
        let order = wf.topological_order().unwrap();
        assert_eq!(order[0], 0);
        assert_eq!(order[1], 1);
    }

    #[test]
    fn ready_steps_respects_deps() {
        let mut wf = Workflow::new("test");
        let s1 = Step::new("first", StepKind::Barrier);
        let s1_id = s1.id.clone();
        wf.add_step(s1);
        wf.add_step(Step::new("second", StepKind::Barrier).depends_on(s1_id.clone()));
        let ready = wf.ready_steps(&[]);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].0, 0);
        let ready = wf.ready_steps(&[s1_id]);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].0, 1);
    }
}
"@ | Set-Content -Path "$base\workflow.rs" -Encoding UTF8
Write-Host "Created workflow.rs"

# state.rs
@"
//! Workflow execution state machine.
use serde::{Deserialize, Serialize};
use crate::{RunId, WorkflowId, StepId, StepRecord, StepOutcome};

/// The execution state of a workflow run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl RunState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

/// The full runtime state of a workflow execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunState {
    pub run_id: RunId,
    pub workflow_id: WorkflowId,
    pub state: RunState,
    pub step_records: Vec<StepRecord>,
    pub pending_mutations: Vec<crate::StateMutation>,
    pub steps_completed: usize,
    pub steps_total: usize,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl WorkflowRunState {
    pub fn new(run_id: RunId, workflow_id: WorkflowId, steps_total: usize) -> Self {
        let now = chrono::Utc::now();
        Self {
            run_id,
            workflow_id,
            state: RunState::Pending,
            step_records: Vec::new(),
            pending_mutations: Vec::new(),
            steps_completed: 0,
            steps_total,
            created_at: now,
            updated_at: now,
            completed_at: None,
        }
    }

    pub fn transition_to(&mut self, new_state: RunState) {
        self.state = new_state;
        self.updated_at = chrono::Utc::now();
        if new_state.is_terminal() {
            self.completed_at = Some(self.updated_at);
        }
    }

    pub fn record_step(&mut self, record: StepRecord) {
        if matches!(record.outcome, StepOutcome::Ok { .. }) {
            self.steps_completed += 1;
        }
        self.step_records.push(record);
        self.updated_at = chrono::Utc::now();
    }

    pub fn add_pending_mutation(&mut self, mutation: crate::StateMutation) {
        self.pending_mutations.push(mutation);
    }

    pub fn take_pending_mutations(&mut self) -> Vec<crate::StateMutation> {
        std::mem::take(&mut self.pending_mutations)
    }

    pub fn pending_mutation_count(&self) -> usize {
        self.pending_mutations.len()
    }

    pub fn progress_pct(&self) -> f64 {
        if self.steps_total == 0 { return 100.0; }
        (self.steps_completed as f64 / self.steps_total as f64) * 100.0
    }

    pub fn completed_step_ids(&self) -> Vec<StepId> {
        self.step_records.iter()
            .filter(|r| matches!(r.outcome, StepOutcome::Ok { .. }))
            .map(|r| r.step_id.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RunId, WorkflowId};

    #[test]
    fn new_run_is_pending() {
        let state = WorkflowRunState::new(RunId::new(), WorkflowId::new(), 5);
        assert_eq!(state.state, RunState::Pending);
        assert_eq!(state.steps_completed, 0);
    }

    #[test]
    fn transition_to_completed_sets_timestamp() {
        let mut state = WorkflowRunState::new(RunId::new(), WorkflowId::new(), 1);
        state.transition_to(RunState::Running);
        assert_eq!(state.state, RunState::Running);
        assert!(state.completed_at.is_none());
        state.transition_to(RunState::Completed);
        assert!(state.completed_at.is_some());
    }

    #[test]
    fn progress_calculation() {
        let mut state = WorkflowRunState::new(RunId::new(), WorkflowId::new(), 4);
        assert_eq!(state.progress_pct(), 0.0);
        state.steps_completed = 2;
        assert_eq!(state.progress_pct(), 50.0);
        state.steps_completed = 4;
        assert_eq!(state.progress_pct(), 100.0);
    }
}
"@ | Set-Content -Path "$base\state.rs" -Encoding UTF8
Write-Host "Created state.rs"

# config.rs
@"
//! Engine configuration.
use serde::{Deserialize, Serialize};

/// Configuration for the workflow engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineConfig {
    /// Number of steps to batch before forcing a commit (fsync).
    /// Set to 1 for immediate persistence per step (safe but slow).
    /// Set to 0 for unlimited batching (fast but risky).
    /// Recommended: 10-100 for most workloads.
    pub sync_steps: usize,

    /// Maximum number of concurrent workflow runs.
    pub max_concurrent_runs: usize,

    /// Maximum number of steps executed in parallel within a single run.
    pub max_step_parallelism: usize,

    /// Default step timeout in milliseconds.
    pub default_step_timeout_ms: u64,

    /// Path to the WAL/journal directory.
    pub journal_dir: std::path::PathBuf,

    /// Whether to fsync the journal on each batch commit.
    pub fsync_on_commit: bool,

    /// Worker pool size for remote step execution (0 = local only).
    pub worker_pool_size: usize,

    /// Heartbeat interval for worker health checks.
    pub worker_heartbeat_ms: u64,

    /// Enable multi-region replication.
    pub replication_enabled: bool,

    /// Replication factor (number of replicas).
    pub replication_factor: usize,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            sync_steps: 10,
            max_concurrent_runs: 64,
            max_step_parallelism: 4,
            default_step_timeout_ms: 30_000,
            journal_dir: std::path::PathBuf::from(".velocity/workflow-journal"),
            fsync_on_commit: true,
            worker_pool_size: 0,
            worker_heartbeat_ms: 5_000,
            replication_enabled: false,
            replication_factor: 1,
        }
    }
}

impl EngineConfig {
    /// Create a config optimized for safety (sync every step).
    pub fn safe() -> Self {
        Self { sync_steps: 1, ..Default::default() }
    }

    /// Create a config optimized for throughput (batch 100 steps).
    pub fn throughput() -> Self {
        Self { sync_steps: 100, ..Default::default() }
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), String> {
        if self.max_concurrent_runs == 0 {
            return Err("max_concurrent_runs must be > 0".into());
        }
        if self.replication_factor < 1 {
            return Err("replication_factor must be >= 1".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        let config = EngineConfig::default();
        assert!(config.validate().is_ok());
        assert_eq!(config.sync_steps, 10);
    }

    #[test]
    fn safe_config_syncs_every_step() {
        let config = EngineConfig::safe();
        assert_eq!(config.sync_steps, 1);
    }

    #[test]
    fn throughput_config_batches() {
        let config = EngineConfig::throughput();
        assert_eq!(config.sync_steps, 100);
    }
}
"@ | Set-Content -Path "$base\config.rs" -Encoding UTF8
Write-Host "Created config.rs"
