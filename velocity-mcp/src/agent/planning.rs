//! Multi-step task planner with validation and confidence scoring.
//!
//! Provides structured planning before execution:
//! - Decompose complex tasks into atomic, validated steps
//! - Track dependencies between steps
//! - Score confidence in each step's feasibility
//! - Support plan revision and rollback

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A single step in a plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    /// Unique step identifier.
    pub id: String,
    /// Human-readable description of what this step does.
    pub description: String,
    /// The action to perform (free-form for the agent).
    pub action: String,
    /// IDs of steps that must complete before this one.
    pub depends_on: Vec<String>,
    /// Confidence in this step's feasibility (0.0 to 1.0).
    pub confidence: f32,
    /// Current status of this step.
    pub status: StepStatus,
    /// Output/result of this step (populated after execution).
    pub output: Option<String>,
    /// Estimated complexity (1 = trivial, 5 = very complex).
    pub complexity: u8,
    /// Whether this step has been validated.
    pub validated: bool,
}

/// Status of a plan step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepStatus {
    /// Not yet started.
    Pending,
    /// Currently being executed.
    InProgress,
    /// Completed successfully.
    Completed,
    /// Failed during execution.
    Failed,
    /// Skipped (dependency failed or no longer needed).
    Skipped,
    /// Blocked by an unresolved dependency.
    Blocked,
}

impl StepStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Blocked => "blocked",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Skipped)
    }
}

/// A complete plan for accomplishing a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    /// The original task/goal description.
    pub goal: String,
    /// All steps in the plan.
    pub steps: Vec<PlanStep>,
    /// Overall plan status.
    pub status: PlanStatus,
    /// Counter for step IDs.
    next_id: u64,
    /// Revision history (snapshots of previous plan states).
    pub revisions: Vec<PlanRevision>,
}

/// Overall status of a plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanStatus {
    /// Being drafted/revised.
    Drafting,
    /// Validated and ready for execution.
    Ready,
    /// Currently executing.
    Executing,
    /// Completed successfully.
    Completed,
    /// Failed (one or more critical steps failed).
    Failed,
    /// Abandoned by user or agent.
    Abandoned,
}

impl PlanStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Drafting => "drafting",
            Self::Ready => "ready",
            Self::Executing => "executing",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Abandoned => "abandoned",
        }
    }
}

/// A snapshot of a plan at a point in time (for rollback).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanRevision {
    pub revision: u32,
    pub timestamp: u64,
    pub note: String,
    pub step_count: usize,
    pub completed_count: usize,
}

impl Plan {
    /// Create a new plan for a goal.
    pub fn new(goal: &str) -> Self {
        Self {
            goal: goal.to_string(),
            steps: Vec::new(),
            status: PlanStatus::Drafting,
            next_id: 1,
            revisions: Vec::new(),
        }
    }

    /// Generate a unique step ID.
    fn gen_id(&mut self) -> String {
        let id = format!("s{}", self.next_id);
        self.next_id += 1;
        id
    }

    /// Add a step to the plan.
    pub fn add_step(
        &mut self,
        description: &str,
        action: &str,
        depends_on: Vec<String>,
        complexity: u8,
    ) -> String {
        let id = self.gen_id();
        self.steps.push(PlanStep {
            id: id.clone(),
            description: description.to_string(),
            action: action.to_string(),
            depends_on,
            confidence: 0.5,
            status: StepStatus::Pending,
            output: None,
            complexity: complexity.clamp(1, 5),
            validated: false,
        });
        id
    }

    /// Set confidence for a step.
    pub fn set_confidence(&mut self, step_id: &str, confidence: f32) {
        if let Some(step) = self.steps.iter_mut().find(|s| s.id == step_id) {
            step.confidence = confidence.clamp(0.0, 1.0);
        }
    }

    /// Validate a step (confirm it's feasible).
    pub fn validate_step(&mut self, step_id: &str) {
        if let Some(step) = self.steps.iter_mut().find(|s| s.id == step_id) {
            step.validated = true;
        }
    }

    /// Validate all steps in the plan.
    pub fn validate_all(&mut self) {
        for step in &mut self.steps {
            step.validated = true;
        }
    }

    /// Mark the plan as ready for execution.
    pub fn mark_ready(&mut self) {
        self.status = PlanStatus::Ready;
    }

    /// Get the next executable step (all dependencies met, not terminal).
    pub fn next_step(&self) -> Option<&PlanStep> {
        if self.status != PlanStatus::Ready && self.status != PlanStatus::Executing {
            return None;
        }
        self.steps.iter().find(|step| {
            if step.status != StepStatus::Pending {
                return false;
            }
            // Check all dependencies are completed.
            step.depends_on.iter().all(|dep_id| {
                self.steps
                    .iter()
                    .find(|s| s.id == *dep_id)
                    .map(|s| s.status == StepStatus::Completed)
                    .unwrap_or(false)
            })
        })
    }

    /// Get steps that are blocked (have unmet dependencies).
    pub fn blocked_steps(&self) -> Vec<&PlanStep> {
        self.steps
            .iter()
            .filter(|step| {
                if step.status != StepStatus::Pending {
                    return false;
                }
                // Has dependencies but not all are completed.
                !step.depends_on.is_empty()
                    && !step.depends_on.iter().all(|dep_id| {
                        self.steps
                            .iter()
                            .find(|s| s.id == *dep_id)
                            .map(|s| s.status == StepStatus::Completed)
                            .unwrap_or(false)
                    })
            })
            .collect()
    }

    /// Record the output of a completed step.
    pub fn complete_step(&mut self, step_id: &str, output: &str) {
        if let Some(step) = self.steps.iter_mut().find(|s| s.id == step_id) {
            step.status = StepStatus::Completed;
            step.output = Some(output.to_string());
        }
        // Check if all steps are done.
        if self.steps.iter().all(|s| s.status.is_terminal()) {
            let all_ok = self.steps.iter().all(|s| s.status == StepStatus::Completed);
            self.status = if all_ok {
                PlanStatus::Completed
            } else {
                PlanStatus::Failed
            };
        }
    }

    /// Mark a step as failed.
    pub fn fail_step(&mut self, step_id: &str) {
        if let Some(step) = self.steps.iter_mut().find(|s| s.id == step_id) {
            step.status = StepStatus::Failed;
        }
    }

    /// Skip a step (e.g., because a dependency failed).
    pub fn skip_step(&mut self, step_id: &str) {
        if let Some(step) = self.steps.iter_mut().find(|s| s.id == step_id) {
            step.status = StepStatus::Skipped;
        }
    }

    /// Record a revision snapshot.
    pub fn snapshot(&mut self, note: &str) {
        let revision = self.revisions.len() as u32 + 1;
        let completed = self
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Completed)
            .count();
        self.revisions.push(PlanRevision {
            revision,
            timestamp: now_secs(),
            note: note.to_string(),
            step_count: self.steps.len(),
            completed_count: completed,
        });
    }

    /// Overall plan confidence (average of step confidences).
    pub fn overall_confidence(&self) -> f32 {
        if self.steps.is_empty() {
            return 0.0;
        }
        let sum: f32 = self.steps.iter().map(|s| s.confidence).sum();
        sum / self.steps.len() as f32
    }

    /// Progress as a fraction (0.0 to 1.0).
    pub fn progress(&self) -> f32 {
        if self.steps.is_empty() {
            return 0.0;
        }
        let done = self
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Completed)
            .count();
        done as f32 / self.steps.len() as f32
    }

    /// Summary for display.
    pub fn summary(&self) -> PlanSummary {
        let total = self.steps.len();
        let completed = self
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Completed)
            .count();
        let failed = self
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Failed)
            .count();
        let pending = self
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Pending)
            .count();
        let validated = self.steps.iter().filter(|s| s.validated).count();
        let total_complexity: u8 = self.steps.iter().map(|s| s.complexity).sum();

        PlanSummary {
            goal: self.goal.clone(),
            status: self.status,
            total_steps: total,
            completed,
            failed,
            pending,
            validated,
            total_complexity,
            confidence: self.overall_confidence(),
            progress: self.progress(),
        }
    }
}

/// Summary statistics of a plan.
#[derive(Debug, Clone)]
pub struct PlanSummary {
    pub goal: String,
    pub status: PlanStatus,
    pub total_steps: usize,
    pub completed: usize,
    pub failed: usize,
    pub pending: usize,
    pub validated: usize,
    pub total_complexity: u8,
    pub confidence: f32,
    pub progress: f32,
}

impl PlanSummary {
    pub fn display(&self) -> String {
        format!(
            "Goal: {}\n\
             Status: {} | Progress: {:.0}%\n\
             Steps: {} total, {} completed, {} failed, {} pending\n\
             Validated: {}/{} | Complexity: {} | Confidence: {:.0}%",
            self.goal,
            self.status.label(),
            self.progress * 100.0,
            self.total_steps,
            self.completed,
            self.failed,
            self.pending,
            self.validated,
            self.total_steps,
            self.total_complexity,
            self.confidence * 100.0,
        )
    }
}

/// Decompose a task into a plan using pattern matching.
/// This is a local heuristic — for complex tasks, the agent loop
/// should use an LLM to generate the plan.
pub fn decompose_task(goal: &str) -> Plan {
    let lower = goal.to_lowercase();
    let mut plan = Plan::new(goal);

    if contains_any(&lower, &["implement", "build", "create", "add feature"]) {
        let s1 = plan.add_step(
            "Analyze requirements",
            "Read relevant code and understand current state",
            vec![],
            2,
        );
        let s2 = plan.add_step(
            "Design solution",
            "Plan the implementation approach",
            vec![s1.clone()],
            3,
        );
        let s3 = plan.add_step(
            "Implement changes",
            "Write the code following the design",
            vec![s2.clone()],
            4,
        );
        let s4 = plan.add_step(
            "Write tests",
            "Add tests for the new functionality",
            vec![s3.clone()],
            3,
        );
        let s5 = plan.add_step(
            "Validate",
            "Run tests and check for regressions",
            vec![s4.clone()],
            2,
        );
        let s6 = plan.add_step(
            "Document",
            "Update documentation and comments",
            vec![s5.clone()],
            1,
        );
    } else if contains_any(&lower, &["fix", "bug", "error", "debug"]) {
        let s1 = plan.add_step(
            "Reproduce the issue",
            "Understand the error and how to trigger it",
            vec![],
            2,
        );
        let s2 = plan.add_step(
            "Identify root cause",
            "Trace the execution path to find the bug",
            vec![s1.clone()],
            3,
        );
        let s3 = plan.add_step(
            "Implement fix",
            "Apply the minimal fix for the root cause",
            vec![s2.clone()],
            3,
        );
        let s4 = plan.add_step(
            "Verify fix",
            "Run tests to confirm the fix works",
            vec![s3.clone()],
            2,
        );
        let s5 = plan.add_step(
            "Add regression test",
            "Prevent this bug from recurring",
            vec![s4.clone()],
            2,
        );
    } else if contains_any(&lower, &["refactor", "clean", "improve"]) {
        let s1 = plan.add_step(
            "Audit current code",
            "Identify pain points and improvement areas",
            vec![],
            2,
        );
        let s2 = plan.add_step(
            "Plan refactoring",
            "Design the new structure",
            vec![s1.clone()],
            3,
        );
        let s3 = plan.add_step(
            "Apply changes",
            "Refactor the code incrementally",
            vec![s2.clone()],
            4,
        );
        let s4 = plan.add_step(
            "Run tests",
            "Ensure no behavior changes",
            vec![s3.clone()],
            2,
        );
    } else if contains_any(&lower, &["test", "verify", "validate"]) {
        let s1 = plan.add_step(
            "Identify test targets",
            "Determine what needs testing",
            vec![],
            1,
        );
        let s2 = plan.add_step(
            "Write test cases",
            "Create comprehensive test scenarios",
            vec![s1.clone()],
            3,
        );
        let s3 = plan.add_step("Run tests", "Execute the test suite", vec![s2.clone()], 2);
        let s4 = plan.add_step(
            "Analyze results",
            "Review failures and coverage gaps",
            vec![s3.clone()],
            2,
        );
    } else {
        // Generic plan
        let s1 = plan.add_step(
            "Understand the task",
            "Analyze what needs to be done",
            vec![],
            2,
        );
        let s2 = plan.add_step(
            "Plan approach",
            "Determine the best strategy",
            vec![s1.clone()],
            2,
        );
        let s3 = plan.add_step("Execute", "Carry out the plan", vec![s2.clone()], 3);
        let s4 = plan.add_step("Verify", "Check the results", vec![s3.clone()], 2);
    }

    plan
}

fn contains_any(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|kw| text.contains(kw))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_plan_and_add_steps() {
        let mut plan = Plan::new("Build feature X");
        let s1 = plan.add_step("Analyze", "Read code", vec![], 2);
        let s2 = plan.add_step("Implement", "Write code", vec![s1.clone()], 3);
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[1].depends_on, vec![s1]);
    }

    #[test]
    fn next_step_respects_dependencies() {
        let mut plan = Plan::new("Test");
        plan.status = PlanStatus::Executing;
        let s1 = plan.add_step("First", "Do first", vec![], 1);
        let s2 = plan.add_step("Second", "Do second", vec![s1.clone()], 1);

        // First step should be next.
        let next = plan.next_step().unwrap();
        assert_eq!(next.id, s1);

        // Complete first, second should be next.
        plan.complete_step(&s1, "done");
        let next = plan.next_step().unwrap();
        assert_eq!(next.id, s2);
    }

    #[test]
    fn plan_completion_tracking() {
        let mut plan = Plan::new("Test");
        plan.status = PlanStatus::Executing;
        let s1 = plan.add_step("Step 1", "Do it", vec![], 1);
        let s2 = plan.add_step("Step 2", "Do it", vec![], 1);

        plan.complete_step(&s1, "ok");
        assert_eq!(plan.status, PlanStatus::Executing);
        assert!((plan.progress() - 0.5).abs() < 0.01);

        plan.complete_step(&s2, "ok");
        assert_eq!(plan.status, PlanStatus::Completed);
        assert!((plan.progress() - 1.0).abs() < 0.01);
    }

    #[test]
    fn decompose_task_patterns() {
        let plan = decompose_task("Implement a new login feature");
        assert!(plan.steps.len() >= 4);

        let plan = decompose_task("Fix the null pointer bug");
        assert!(plan.steps.len() >= 4);

        let plan = decompose_task("Refactor the auth module");
        assert!(plan.steps.len() >= 3);

        let plan = decompose_task("something completely different");
        assert!(plan.steps.len() >= 3);
    }

    #[test]
    fn plan_snapshot_and_revision() {
        let mut plan = Plan::new("Test");
        plan.add_step("Step 1", "Do it", vec![], 1);
        plan.snapshot("Initial plan");
        assert_eq!(plan.revisions.len(), 1);
        assert_eq!(plan.revisions[0].step_count, 1);
    }

    #[test]
    fn validate_and_confidence() {
        let mut plan = Plan::new("Test");
        let s1 = plan.add_step("Step 1", "Do it", vec![], 1);
        plan.set_confidence(&s1, 0.9);
        plan.validate_step(&s1);
        assert!(plan.steps[0].validated);
        assert!((plan.overall_confidence() - 0.9).abs() < 0.01);
    }

    #[test]
    fn summary_display() {
        let plan = decompose_task("Build a new API endpoint");
        let summary = plan.summary();
        let display = summary.display();
        assert!(display.contains("Goal:"));
        assert!(display.contains("Steps:"));
    }
}
