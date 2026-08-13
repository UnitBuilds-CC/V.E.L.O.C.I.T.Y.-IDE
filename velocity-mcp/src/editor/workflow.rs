//! Workflow composer: sequential + branching multi-step automation.
//!
//! A [`Workflow`] is an ordered list of [`WorkflowStep`]s. [`Workflow::execute`]
//! runs them in order, reusing the MCP tool dispatch
//! ([`crate::registry::dispatch::call_tool_in_workspace`]) for `Tool` steps and
//! the headless agent runtime for `AgentTask` steps. A `Condition` step inspects
//! the previous step's [`StepOutcome`] and short-circuits the remaining steps
//! when its requirement is not met. Each step is captured in a [`StepRecord`],
//! and the whole run produces a [`WorkflowRun`] for the run log and governance
//! audit.
//!
//! Workflows persist as individual JSON files under `.velocity/workflows/`, one
//! file per workflow id, loaded/saved by [`WorkflowRegistry`].

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const WORKFLOWS_DIR: &str = "workflows";

/// A single step in a workflow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WorkflowStep {
    /// Dispatch a free-form prompt to a headless agent (optionally a named team).
    AgentTask {
        prompt: String,
        team: Option<String>,
    },
    /// Invoke a registered MCP tool with JSON arguments.
    Tool {
        name: String,
        args: serde_json::Value,
    },
    /// Invoke a configured connector by id (wired in Pillar 2).
    Connector { id: String, req: serde_json::Value },
    /// Continue only if the previous step's outcome matches `require`;
    /// otherwise short-circuit the remaining steps.
    Condition { require: StepOutcome },
}

impl WorkflowStep {
    /// Short human label describing the step kind.
    pub fn kind_label(&self) -> String {
        match self {
            WorkflowStep::AgentTask { .. } => "agent".to_string(),
            WorkflowStep::Tool { name, .. } => format!("tool:{name}"),
            WorkflowStep::Connector { id, .. } => format!("connector:{id}"),
            WorkflowStep::Condition { require } => format!("condition=={}", require.label()),
        }
    }
}

/// Outcome of executing a single step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepOutcome {
    Ok,
    Failed,
    Skipped,
}

impl StepOutcome {
    pub fn label(self) -> &'static str {
        match self {
            StepOutcome::Ok => "ok",
            StepOutcome::Failed => "failed",
            StepOutcome::Skipped => "skipped",
        }
    }
}

/// The recorded result of one executed step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepRecord {
    pub index: usize,
    pub kind: String,
    pub outcome: StepOutcome,
    pub output: String,
}

/// Overall status of a completed run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunStatus {
    Success,
    Partial,
    Failed,
}

impl RunStatus {
    pub fn label(self) -> &'static str {
        match self {
            RunStatus::Success => "success",
            RunStatus::Partial => "partial",
            RunStatus::Failed => "failed",
        }
    }
}

/// The record of a single workflow execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub workflow_id: String,
    pub status: RunStatus,
    pub steps: Vec<StepRecord>,
    pub started_at: u64,
    pub finished_at: u64,
}

impl WorkflowRun {
    /// Number of steps that completed successfully.
    pub fn ok_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| s.outcome == StepOutcome::Ok)
            .count()
    }
}

/// A named, ordered sequence of steps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub steps: Vec<WorkflowStep>,
}

impl Workflow {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            steps: Vec::new(),
        }
    }

    /// Execute the workflow sequentially against `workspace_root`, producing a
    /// [`WorkflowRun`]. A failed `Condition` step short-circuits the remainder
    /// (recorded as `Skipped`). Non-condition failures are recorded but do not
    /// halt execution — use a `Condition` to gate downstream steps.
    pub fn execute(&self, workspace_root: &Path) -> WorkflowRun {
        let started_at = now_secs();
        let mut records = Vec::with_capacity(self.steps.len());
        let mut short_circuit = false;
        let mut prior: Option<StepOutcome> = None;

        for (index, step) in self.steps.iter().enumerate() {
            if short_circuit {
                records.push(StepRecord {
                    index,
                    kind: step.kind_label(),
                    outcome: StepOutcome::Skipped,
                    output: "skipped (prior condition not met)".to_string(),
                });
                continue;
            }

            let (outcome, output) = run_step(workspace_root, step, prior);
            // A failed condition halts the rest of the workflow.
            if matches!(step, WorkflowStep::Condition { .. }) && outcome == StepOutcome::Failed {
                short_circuit = true;
            }
            prior = Some(outcome);
            records.push(StepRecord {
                index,
                kind: step.kind_label(),
                outcome,
                output,
            });
        }

        let has_failed = records.iter().any(|r| r.outcome == StepOutcome::Failed);
        let has_skipped = records.iter().any(|r| r.outcome == StepOutcome::Skipped);
        let status = if has_failed {
            RunStatus::Failed
        } else if has_skipped {
            RunStatus::Partial
        } else {
            RunStatus::Success
        };

        WorkflowRun {
            workflow_id: self.id.clone(),
            status,
            steps: records,
            started_at,
            finished_at: now_secs(),
        }
    }
}

/// Run a single step and return its outcome plus a captured output string.
fn run_step(
    workspace_root: &Path,
    step: &WorkflowStep,
    prior: Option<StepOutcome>,
) -> (StepOutcome, String) {
    match step {
        WorkflowStep::Tool { name, args } => {
            match crate::registry::dispatch::call_tool_in_workspace(workspace_root, name, args) {
                Ok(out) => (StepOutcome::Ok, out),
                Err(e) => (StepOutcome::Failed, e.to_string()),
            }
        }
        WorkflowStep::AgentTask { prompt, team: _ } => {
            let request = crate::agent::HeadlessSubAgentRequest {
                workspace_root: workspace_root.to_path_buf(),
                provider: crate::agent::AiProvider::CloudflareWorkersAi,
                model: crate::agent::provider::default_provider_model(
                    crate::agent::AiProvider::CloudflareWorkersAi,
                ),
                thinking: false,
                prompt: prompt.clone(),
                cancel_rx: None,
                progress: None,
                scoped_files: None,
            };
            let result = crate::agent::run_headless_subagent(request);
            (
                StepOutcome::Ok,
                format!("{} status update(s)", result.status_updates.len()),
            )
        }
        WorkflowStep::Connector { id, req: _ } => (
            StepOutcome::Skipped,
            format!("connector '{id}' execution wired in Pillar 2"),
        ),
        WorkflowStep::Condition { require } => match prior {
            Some(o) if o == *require => (
                StepOutcome::Ok,
                format!("condition met: prior == {}", require.label()),
            ),
            Some(o) => (
                StepOutcome::Failed,
                format!(
                    "condition failed: prior {} != {}",
                    o.label(),
                    require.label()
                ),
            ),
            None => (
                StepOutcome::Failed,
                "condition failed: no prior step".to_string(),
            ),
        },
    }
}

/// In-memory set of workflows, backed by `.velocity/workflows/<id>.json`.
#[derive(Debug, Clone, Default)]
pub struct WorkflowRegistry {
    pub workflows: Vec<Workflow>,
}

impl WorkflowRegistry {
    /// Load every `*.json` workflow file from `.velocity/workflows/`. Missing
    /// directory or unreadable/corrupt files are skipped.
    pub fn load(workspace_root: &Path) -> Self {
        let dir = workspace_root.join(".velocity").join(WORKFLOWS_DIR);
        let mut workflows = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                if let Ok(bytes) = std::fs::read(&path) {
                    if let Ok(wf) = serde_json::from_slice::<Workflow>(&bytes) {
                        workflows.push(wf);
                    }
                }
            }
        }
        workflows.sort_by(|a, b| a.name.cmp(&b.name));
        Self { workflows }
    }

    /// Persist all current workflows to `.velocity/workflows/`, pruning any
    /// orphaned `*.json` files whose id is no longer present.
    pub fn save(&self, workspace_root: &Path) -> Result<(), String> {
        let dir = workspace_root.join(".velocity").join(WORKFLOWS_DIR);
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create workflows dir: {e}"))?;

        for wf in &self.workflows {
            let json = serde_json::to_vec_pretty(wf)
                .map_err(|e| format!("workflow serialize failed: {e}"))?;
            std::fs::write(dir.join(format!("{}.json", wf.id)), json)
                .map_err(|e| format!("cannot write workflow: {e}"))?;
        }

        // Prune files for removed workflows.
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_string();
                if !self.workflows.iter().any(|w| w.id == stem) {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        Ok(())
    }

    pub fn add(&mut self, workflow: Workflow) {
        self.workflows.push(workflow);
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.workflows.len();
        self.workflows.retain(|w| w.id != id);
        self.workflows.len() != before
    }

    pub fn get(&self, id: &str) -> Option<&Workflow> {
        self.workflows.iter().find(|w| w.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Workflow> {
        self.workflows.iter_mut().find(|w| w.id == id)
    }

    pub fn is_empty(&self) -> bool {
        self.workflows.is_empty()
    }

    pub fn len(&self) -> usize {
        self.workflows.len()
    }
}

/// Current wall-clock time as Unix epoch seconds.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write_step(rel: &str) -> WorkflowStep {
        WorkflowStep::Tool {
            name: "write_file".to_string(),
            args: json!({ "relativeFilePath": rel, "content": "hello" }),
        }
    }

    #[test]
    fn two_step_workflow_executes_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        let mut wf = Workflow::new("wf1", "two steps");
        wf.steps.push(write_step("a.txt"));
        wf.steps.push(write_step("b.txt"));

        let run = wf.execute(tmp.path());
        assert_eq!(run.status, RunStatus::Success);
        assert_eq!(run.steps.len(), 2);
        assert_eq!(run.steps[0].index, 0);
        assert_eq!(run.steps[1].index, 1);
        assert_eq!(run.steps[0].outcome, StepOutcome::Ok);
        assert_eq!(run.ok_count(), 2);
        assert!(tmp.path().join("a.txt").exists());
        assert!(tmp.path().join("b.txt").exists());
    }

    #[test]
    fn condition_short_circuits_remaining_steps() {
        let tmp = tempfile::tempdir().unwrap();
        let mut wf = Workflow::new("wf2", "gated");
        wf.steps.push(write_step("first.txt")); // Ok
        wf.steps.push(WorkflowStep::Condition {
            require: StepOutcome::Failed,
        }); // prior Ok != Failed -> fails, short-circuits
        wf.steps.push(write_step("never.txt")); // Skipped

        let run = wf.execute(tmp.path());
        assert_eq!(run.steps[0].outcome, StepOutcome::Ok);
        assert_eq!(run.steps[1].outcome, StepOutcome::Failed);
        assert_eq!(run.steps[2].outcome, StepOutcome::Skipped);
        assert_eq!(run.status, RunStatus::Failed);
        assert!(!tmp.path().join("never.txt").exists());
    }

    #[test]
    fn condition_met_allows_continuation() {
        let tmp = tempfile::tempdir().unwrap();
        let mut wf = Workflow::new("wf3", "passes gate");
        wf.steps.push(write_step("one.txt")); // Ok
        wf.steps.push(WorkflowStep::Condition {
            require: StepOutcome::Ok,
        }); // prior Ok == Ok -> passes
        wf.steps.push(write_step("two.txt")); // runs

        let run = wf.execute(tmp.path());
        assert_eq!(run.steps[1].outcome, StepOutcome::Ok);
        assert_eq!(run.steps[2].outcome, StepOutcome::Ok);
        assert_eq!(run.status, RunStatus::Success);
        assert!(tmp.path().join("two.txt").exists());
    }

    #[test]
    fn failed_tool_step_is_recorded() {
        let tmp = tempfile::tempdir().unwrap();
        let mut wf = Workflow::new("wf4", "bad tool");
        wf.steps.push(WorkflowStep::Tool {
            name: "does_not_exist".to_string(),
            args: json!({}),
        });
        let run = wf.execute(tmp.path());
        assert_eq!(run.steps.len(), 1);
        assert_eq!(run.steps[0].outcome, StepOutcome::Failed);
        assert_eq!(run.status, RunStatus::Failed);
        assert_eq!(run.ok_count(), 0);
    }

    #[test]
    fn connector_step_is_pending() {
        let tmp = tempfile::tempdir().unwrap();
        let mut wf = Workflow::new("wf5", "connector");
        wf.steps.push(WorkflowStep::Connector {
            id: "gh".to_string(),
            req: json!({}),
        });
        let run = wf.execute(tmp.path());
        assert_eq!(run.steps[0].outcome, StepOutcome::Skipped);
        assert_eq!(run.status, RunStatus::Partial);
    }

    #[test]
    fn registry_round_trip_and_prune() {
        let tmp = tempfile::tempdir().unwrap();
        let mut reg = WorkflowRegistry::default();
        let mut wf = Workflow::new("keep", "Keep Me");
        wf.steps.push(write_step("x.txt"));
        reg.add(wf);
        reg.add(Workflow::new("drop", "Drop Me"));
        reg.save(tmp.path()).expect("save");

        let loaded = WorkflowRegistry::load(tmp.path());
        assert_eq!(loaded.len(), 2);
        // Sorted by name: "Drop Me" < "Keep Me".
        assert_eq!(loaded.workflows[0].id, "drop");
        assert_eq!(loaded.get("keep").unwrap().steps.len(), 1);

        // Remove one and re-save: its file should be pruned.
        let mut reg2 = loaded;
        assert!(reg2.remove("drop"));
        reg2.save(tmp.path()).expect("save2");
        let reloaded = WorkflowRegistry::load(tmp.path());
        assert_eq!(reloaded.len(), 1);
        assert!(reloaded.get("drop").is_none());
        assert!(reloaded.get("keep").is_some());
    }
}
