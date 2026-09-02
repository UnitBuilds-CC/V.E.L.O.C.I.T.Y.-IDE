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
