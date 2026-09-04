use super::super::types::*;
use super::struct_def::OrchestratorPanel;
use crate::automation::resolve_weight_root;
use crate::orchestrator::registry::{OrchestratorRegistry, TaskStatus};
use crate::orchestrator::validator;
use crate::orchestrator::worker::{spawn_live_worker, WorkerAssignment, WorkerResult};
use crate::orchestrator::TaskId;
use std::path::Path;

impl OrchestratorPanel {
    pub fn execute_routed_tasks(
        &mut self,
        workspace_root: &Path,
        mediator: &std::sync::Arc<crate::automation::mediator::MediatorArena>,
    ) {
        self.start_execution(workspace_root, mediator);
    }

    pub fn retry_blocked_tasks_action(
        &mut self,
        workspace_root: &Path,
        mediator: &std::sync::Arc<crate::automation::mediator::MediatorArena>,
    ) {
        self.retry_blocked_tasks(workspace_root, mediator);
    }

    pub fn reset_runtime_action(&mut self) {
        self.reset_runtime();
    }

    pub fn retry_task_action(
        &mut self,
        task_id: TaskId,
        workspace_root: &Path,
        mediator: &std::sync::Arc<crate::automation::mediator::MediatorArena>,
    ) -> bool {
        self.retry_task(task_id, workspace_root, mediator)
    }

    pub fn reset_task_action(&mut self, task_id: TaskId) -> bool {
        self.reset_task(task_id)
    }

    pub fn stop_task_action(&mut self, task_id: TaskId) -> bool {
        self.stop_task(task_id)
    }

    pub fn send_task_note_action(&mut self, task_id: TaskId, note: String) -> bool {
        self.send_task_note(task_id, note)
    }

    pub fn start_execution(
        &mut self,
        workspace_root: &Path,
        mediator: &std::sync::Arc<crate::automation::mediator::MediatorArena>,
    ) {
        if self.registry.is_none() {
            self.registry = Some(OrchestratorRegistry::new(&self.graph));
        }
        self.execution_running = true;
        self.runtime_status = "Dispatching routed tasks".to_string();
        self.poll_live_workers(workspace_root, mediator);
    }

    pub fn reset_runtime(&mut self) {
        self.execution_running = false;
        self.running_workers.clear();
        if let Some(reg) = &mut self.registry {
            for status in reg.statuses.values_mut() {
                *status = TaskStatus::Pending;
            }
            reg.outputs.clear();
        }
        self.runtime_status = "Idle".to_string();
    }

    pub fn retryable_blocked_task_count(&self) -> usize {
        self.registry
            .as_ref()
            .map(|reg| {
                reg.statuses
                    .values()
                    .filter(|status| matches!(status, TaskStatus::Blocked(result) if is_retryable_blocked_result(result)))
                    .count()
            })
            .unwrap_or(0)
    }

    pub fn stale_plan_blocked_task_count(&self) -> usize {
        self.registry
            .as_ref()
            .map(|reg| {
                reg.statuses
                    .values()
                    .filter(|status| matches!(status, TaskStatus::Blocked(result) if is_stale_plan_blocked_result(result)))
                    .count()
            })
            .unwrap_or(0)
    }

    pub fn blocked_task_count(&self) -> usize {
        self.registry
            .as_ref()
            .map(|reg| {
                reg.statuses
                    .values()
                    .filter(|status| matches!(status, TaskStatus::Blocked(_)))
                    .count()
            })
            .unwrap_or(0)
    }

    pub fn refresh_runtime_status(&mut self) {
        let blocked = self.blocked_task_count();
        let retryable = self.retryable_blocked_task_count();
        let stale_plan = self.stale_plan_blocked_task_count();
        self.runtime_status = if self.execution_running {
            format!("Running {} worker(s)", self.running_workers.len())
        } else if blocked > 0 && retryable == blocked {
            format!("Waiting for retry on {retryable} blocked task(s)")
        } else if blocked > 0 && stale_plan == blocked {
            format!("Waiting for routed plan refresh on {stale_plan} blocked task(s)")
        } else if blocked > 0 && retryable > 0 {
            format!("Waiting on {blocked} blocked task(s) ({retryable} retryable)")
        } else if blocked > 0 {
            format!("Waiting for follow-up on {blocked} blocked task(s)")
        } else if self.registry.as_ref().is_some_and(|reg| reg.is_complete()) {
            "Execution complete".to_string()
        } else {
            "Waiting for executable routed tasks".to_string()
        };
    }

    pub fn retry_blocked_tasks(
        &mut self,
        workspace_root: &Path,
        mediator: &std::sync::Arc<crate::automation::mediator::MediatorArena>,
    ) {
        if self.registry.is_none() {
            self.registry = Some(OrchestratorRegistry::new(&self.graph));
        }
        let reg = self.registry.as_mut().unwrap();
        let mut retried = 0usize;
        for status in reg.statuses.values_mut() {
            if matches!(status, TaskStatus::Blocked(result) if is_retryable_blocked_result(result))
            {
                *status = TaskStatus::Pending;
                retried += 1;
            }
        }
        self.running_workers.clear();
        self.execution_running = retried > 0;
        self.runtime_status = if retried > 0 {
            format!("Retrying {retried} blocked task(s)")
        } else {
            "No retryable blocked tasks".to_string()
        };
        if retried > 0 {
            self.poll_live_workers(workspace_root, mediator);
        }
    }

    pub fn retry_task(
        &mut self,
        task_id: TaskId,
        workspace_root: &Path,
        mediator: &std::sync::Arc<crate::automation::mediator::MediatorArena>,
    ) -> bool {
        if self.registry.is_none() {
            self.registry = Some(OrchestratorRegistry::new(&self.graph));
        }
        let Some(reg) = self.registry.as_mut() else {
            return false;
        };
        let can_retry = matches!(reg.statuses.get(&task_id), Some(TaskStatus::Blocked(result)) if is_retryable_blocked_result(result));
        if !can_retry {
            return false;
        }
        reg.statuses.insert(task_id, TaskStatus::Pending);
        self.running_workers.remove(&task_id);
        self.execution_running = true;
        self.runtime_status = format!("Retrying task {}", task_id.0);
        self.poll_live_workers(workspace_root, mediator);
        true
    }

    pub fn reset_task(&mut self, task_id: TaskId) -> bool {
        if self.registry.is_none() {
            self.registry = Some(OrchestratorRegistry::new(&self.graph));
        }
        let Some(reg) = self.registry.as_mut() else {
            return false;
        };
        if !self.graph.tasks.contains_key(&task_id) {
            return false;
        }
        reg.statuses.insert(task_id, TaskStatus::Pending);
        reg.outputs.remove(&task_id);
        self.running_workers.remove(&task_id);
        self.execution_running = !self.running_workers.is_empty();
        self.runtime_status = format!("Reset task {} to pending", task_id.0);
        true
    }

    pub fn stop_task(&mut self, task_id: TaskId) -> bool {
        let Some(handle) = self.running_workers.get_mut(&task_id) else {
            return false;
        };
        let cancelled = handle.cancel();
        if cancelled {
            self.runtime_status = format!("Stopping task {}", task_id.0);
        }
        cancelled
    }

    pub fn send_task_note(&mut self, task_id: TaskId, note: String) -> bool {
        let Some(handle) = self.running_workers.get_mut(&task_id) else {
            return false;
        };
        let sent = handle.send_note(note);
        if sent {
            self.runtime_status = format!("Sent operator note to task {}", task_id.0);
        }
        sent
    }

    pub fn poll_live_workers(
        &mut self,
        workspace_root: &Path,
        mediator: &std::sync::Arc<crate::automation::mediator::MediatorArena>,
    ) {
        if self.registry.is_none() {
            self.registry = Some(OrchestratorRegistry::new(&self.graph));
        }
        let reg = self.registry.as_mut().unwrap();

        let finished_ids: Vec<_> = self
            .running_workers
            .iter_mut()
            .filter_map(|(id, handle)| handle.poll().map(|result| (*id, result)))
            .collect();

        for (id, mut result) in finished_ids {
            self.running_workers.remove(&id);
            let outputs = task_result_outputs(&result);
            let report = validator::validate_with_workspace(&result, workspace_root);
            let reconciliation_error =
                reconciliation_error(&self.graph, &reg.outputs, id, &outputs);
            let needs_follow_up = reconciliation_error.is_some() || requires_follow_up(&result);
            if result.success && report.ok && reconciliation_error.is_none() && !needs_follow_up {
                reg.outputs.insert(id, outputs);
                reg.statuses.insert(id, TaskStatus::Done(result));
            } else {
                if let Some(error) = reconciliation_error {
                    result.success = false;
                    result.status_updates.push(error.clone());
                    result.message = if result.message.trim().is_empty() {
                        error
                    } else {
                        format!("{} | {}", result.message, error)
                    };
                } else if !report.ok && !report.messages.is_empty() {
                    let details = report.messages.join(" | ");
                    result.status_updates.push(details.clone());
                    result.message = if result.message.trim().is_empty() {
                        details
                    } else {
                        format!("{} | {}", result.message, details)
                    };
                }
                if needs_follow_up {
                    reg.statuses.insert(id, TaskStatus::Blocked(result));
                } else {
                    reg.statuses.insert(id, TaskStatus::Failed(result));
                }
            }
        }

        propagate_blocked_dependents(&self.graph, reg);
        complete_reconcile_root(&self.graph, reg);

        let ready_ids = reg.ready_ids(&self.graph);
        let weight_root = resolve_weight_root(workspace_root);
        for id in ready_ids {
            if self.running_workers.contains_key(&id) {
                continue;
            }
            let Some(task) = self.graph.tasks.get(&id).cloned() else {
                continue;
            };
            let Some(routed_task) = routed_task_for_id(&self.routed_plan, id) else {
                continue;
            };
            let site_map_path = workspace_root.join(".velocity").join("site_map");
            let current_site_map_root =
                velocity_ide::site_map::SiteMap::open(&site_map_path, weight_root)
                    .map(|site_map| site_map.root());
            match current_site_map_root {
                Ok(current_root) if current_root == routed_task.planned_site_map_root => {
                    let handle = spawn_live_worker(
                        WorkerAssignment {
                            task,
                            task_kind: routed_task.task_kind,
                            workspace_root: workspace_root.to_path_buf(),
                            instructions: routed_task.execution_contract.clone(),
                            planned_site_map_root: routed_task.planned_site_map_root,
                            provider: routed_task.provider,
                            provider_label: routed_task.provider.label().to_string(),
                            model_id: routed_task.model_id.clone(),
                            model_label: routed_task.model_label.clone(),
                            thinking: routed_task.thinking,
                            fallback_chain: routed_task.fallback_chain.clone(),
                            scoped_files: None,
                        },
                        mediator.clone(),
                        weight_root,
                    );
                    reg.statuses.insert(id, TaskStatus::Running);
                    self.running_workers.insert(id, handle);
                }
                Ok(current_root) => {
                    let mut result = WorkerResult::new(&task);
                    result.success = false;
                    result.message = format!(
                        "stale routed plan: planned SiteMap root {:016x} but current root is {:016x}",
                        routed_task.planned_site_map_root,
                        current_root
                    );
                    result.status_updates.push(result.message.clone());
                    reg.statuses.insert(id, TaskStatus::Blocked(result));
                }
                Err(err) => {
                    let mut result = WorkerResult::new(&task);
                    result.success = false;
                    result.message = format!("failed to open site map for freshness check: {err}");
                    result.status_updates.push(result.message.clone());
                    reg.statuses.insert(id, TaskStatus::Blocked(result));
                }
            }
        }

        self.execution_running = !self.running_workers.is_empty();
        self.refresh_runtime_status();
    }
}
