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
