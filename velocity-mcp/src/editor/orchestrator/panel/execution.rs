use super::super::types::*;
use super::struct_def::{OrchestratorPanel, WorkerWatchdog};
use crate::automation::resolve_weight_root;
use crate::orchestrator::registry::{OrchestratorRegistry, TaskStatus};
use crate::orchestrator::validator;
use crate::orchestrator::worker::{spawn_live_worker, WorkerAssignment, WorkerResult};
use crate::orchestrator::TaskId;
use std::path::Path;
use std::time::Duration;

/// How long a worker may produce zero progress events before the watchdog
/// asks it to stop. Generous on purpose: a slow reasoning model can spend
/// minutes inside one turn, but it emits at least a status event per turn.
const WORKER_STALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Grace between the watchdog's cancel request and failing the task outright
/// (the thread may never observe the cancel; the UI must not wait on it).
const WORKER_CANCEL_GRACE: Duration = Duration::from_secs(60);

/// Turn `OrchestratorPanel::max_worker_wall_secs` into a [`Duration`] the
/// watchdog can compare against. `0` means "no hard ceiling" — the same
/// convention
/// [`MissionConfig::max_wall_secs`](crate::automation::mission_runner::MissionConfig::max_wall_secs)
/// uses, and the shape the live-VC pipeline relies on for whole-repo ports
/// that legitimately run for hours.
pub(crate) fn hard_timeout_for(max_secs: u64) -> Duration {
    if max_secs == 0 {
        Duration::MAX
    } else {
        Duration::from_secs(max_secs)
    }
}

impl OrchestratorPanel {
    /// Keep the balancer's seat pool aligned with `max_concurrent_workers`.
    /// Seats are registered with capacity 1, so the pool doubles as an
    /// admission counter: `assign_task` hands out a seat only while one is
    /// free, and a full pool refuses new spawns until a running task releases.
    /// Shrinking never unregisters a seat that still holds an in-flight task,
    /// so those releases land on a seat that exists.
    fn sync_seat_pool(&mut self) {
        let desired = self.max_concurrent_workers.max(1) as u64;
        // Make sure a seat exists for every slot up to the cap.
        for id in 0..desired {
            if self.load_balancer.get_worker_load(id).is_none() {
                self.load_balancer.register_worker(id, 1, Vec::new());
            }
        }
        // Retire seats past the cap only once they go idle, so lowering
        // `max_concurrent_workers` mid-run never deletes the seat an
        // in-flight task will report its completion against.
        let stale: Vec<u64> = self
            .load_balancer
            .worker_ids()
            .into_iter()
            .filter(|id| {
                *id >= desired
                    && self
                        .load_balancer
                        .get_worker_load(*id)
                        .is_some_and(|w| w.active_tasks == 0)
            })
            .collect();
        for id in stale {
            self.load_balancer.unregister_worker(id);
        }
    }
}

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

    /// Record a task's status without holding a mutable borrow of the whole
    /// panel. The dispatch loop needs to read bindings and running workers
    /// between status writes, and `registry` is one field of `self`.
    pub(super) fn set_task_status(&mut self, task_id: TaskId, status: TaskStatus) {
        if let Some(reg) = self.registry.as_mut() {
            reg.statuses.insert(task_id, status);
        }
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
        for handle in self.running_workers.values_mut() {
            let _ = handle.cancel();
        }
        self.running_workers.clear();
        self.worker_watchdog.clear();
        // Seats force-released here must not carry their in-flight count into
        // the next run; a reset pool starts admission from zero.
        self.worker_seats.clear();
        self.load_balancer.reset();
        if let Some(reg) = &mut self.registry {
            for status in reg.statuses.values_mut() {
                *status = TaskStatus::Pending;
            }
            reg.outputs.clear();
        }
        // Bindings carry the SiteMap root they were planned against. Dropping
        // the hand-authored ones makes the next run re-bind against the current
        // root instead of reporting a stale plan for work that never ran.
        let authored: Vec<TaskId> = self.authored_kinds.keys().copied().collect();
        for id in authored {
            self.bindings.remove(&id);
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
            format!(
                "Running {} of {} worker slot(s)",
                self.running_workers.len(),
                self.max_concurrent_workers
            )
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
        self.worker_watchdog.clear();
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
        self.worker_watchdog.remove(&task_id);
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
        self.worker_watchdog.remove(&task_id);
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
            // Release the seat via direct field access: `reg` (a mutable
            // borrow of `self.registry`) is live through this loop, so a
            // `&mut self` method call would collide with it. `worker_seats`
            // and `load_balancer` are disjoint fields, so this is allowed.
            if let Some(seat) = self.worker_seats.remove(&id) {
                self.load_balancer.record_task_complete(
                    seat,
                    result.duration.as_secs_f64() * 1000.0,
                    result.success,
                );
            }
            let outputs = task_result_outputs(&result);
            let report = validator::validate_with_workspace(&result, workspace_root);
            let reconciliation_error =
                reconciliation_error(&self.graph, &reg.outputs, id, &outputs);
            let needs_follow_up = reconciliation_error.is_some() || requires_follow_up(&result);
            if result.success && report.ok && reconciliation_error.is_none() && !needs_follow_up {
                let task_output_text = if result.message.trim().is_empty() {
                    outputs.join(", ")
                } else {
                    result.message.clone()
                };
                reg.outputs.insert(id, outputs);
                reg.statuses.insert(id, TaskStatus::Done(result));
                if let Some(task) = self.graph.tasks.get_mut(&id) {
                    task.output = Some(task_output_text);
                }
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
        complete_reconcile_root(&mut self.graph, reg);

        // ─── Worker watchdog ──────────────────────────────────────────────
        // A worker thread can stay alive and simply stop doing anything (a
        // provider stream that trickles forever, a tool that never returns).
        // The Sept 2026 agent_eval run left two tasks stuck in `Running` with
        // no progress and no path out. Cancel a hung worker, then fail the
        // task after a grace period so one stall cannot hold the plan open.
        let running_ids: Vec<TaskId> = self.running_workers.keys().copied().collect();
        self.worker_watchdog
            .retain(|id, _| running_ids.contains(id));
        let now = std::time::Instant::now();
        let hard = hard_timeout_for(self.max_worker_wall_secs);
        let mut timed_out: Vec<(TaskId, String)> = Vec::new();
        for id in &running_ids {
            let Some(handle) = self.running_workers.get_mut(id) else {
                continue;
            };
            let event_count = handle.snapshot().events.len();
            let watchdog = self
                .worker_watchdog
                .entry(*id)
                .or_insert_with(|| WorkerWatchdog::new(event_count));
            if event_count > watchdog.last_event_count {
                watchdog.last_event_count = event_count;
                watchdog.last_activity = now;
            }
            if watchdog.reason.is_none() {
                let silent = now.duration_since(watchdog.last_activity);
                let total = now.duration_since(watchdog.started);
                if silent > WORKER_STALL_TIMEOUT {
                    watchdog.reason = Some(format!("no progress for {}s", silent.as_secs()));
                } else if total > hard {
                    watchdog.reason = Some(format!("exceeded {}s hard limit", total.as_secs()));
                }
            }
            if let Some(reason) = watchdog.reason.clone() {
                match watchdog.cancel_sent_at {
                    None => {
                        handle.cancel();
                        watchdog.cancel_sent_at = Some(now);
                        log::warn!("watchdog: cancelling task {} ({})", id.0, reason);
                    }
                    Some(at) if now.duration_since(at) > WORKER_CANCEL_GRACE => {
                        timed_out.push((*id, reason));
                    }
                    Some(_) => {}
                }
            }
        }
        for (id, reason) in timed_out {
            self.running_workers.remove(&id);
            self.worker_watchdog.remove(&id);
            // The thread may still be alive but its outcome is decided here,
            // so the seat is released as a failure and the hang is recorded
            // in the seat's statistics like any other completed task. Direct
            // field access again: `reg` stays live through this loop.
            if let Some(seat) = self.worker_seats.remove(&id) {
                self.load_balancer.record_task_complete(seat, 0.0, false);
            }
            let Some(task) = self.graph.tasks.get(&id).cloned() else {
                continue;
            };
            let mut result = WorkerResult::new(&task);
            result.success = false;
            result.message = format!("worker hung: {reason}; cancelled by watchdog");
            result.status_updates.push(result.message.clone());
            // `reg` is live here; write the field directly rather than through
            // `set_task_status`, which would reborrow the whole panel.
            reg.statuses.insert(id, TaskStatus::Failed(result));
        }

        // Conflict-aware dispatch wave: among the ready tasks, keep only a
        // set that cannot collide — no textual scope overlap and no shared
        // historical conflict group from the durable event store. Deferred
        // tasks reappear on a later tick once the contested scope frees up,
        // instead of racing into a mediator rejection and a recorded
        // shared-write-conflict failure.
        let ready_ids = {
            let all_ready = reg.ready_ids(&self.graph);
            if all_ready.len() > 1 {
                let groups = crate::registry::event_store::conflict_file_groups(workspace_root);
                reg.ready_ids_conflict_aware(&self.graph, &groups)
            } else {
                all_ready
            }
        };
        // From here on the panel is borrowed whole, so every registry write goes
        // through `set_task_status` rather than `reg`.
        // Hand-authored tasks have no router-produced binding; give every task
        // that is about to be considered for dispatch one, against the SiteMap
        // root as of now, before the freshness check below compares to it.
        let _ = self.bind_unbound_tasks(workspace_root);
        let weight_root = resolve_weight_root(workspace_root);
        self.sync_seat_pool();
        for id in ready_ids {
            if self.running_workers.contains_key(&id) {
                continue;
            }
            if Some(id) == self.reconcile_root {
                // Bookkeeping node: completed by `complete_reconcile_root` once
                // its dependencies are, never by a worker of its own.
                continue;
            }
            let Some(task) = self.graph.tasks.get(&id).cloned() else {
                continue;
            };
            let site_map_path = workspace_root.join(".velocity").join("site_map");
            let current_site_map_root =
                velocity_ide::site_map::SiteMap::open(&site_map_path, weight_root)
                    .map(|site_map| site_map.root());
            match current_site_map_root {
                Ok(current_root) => {
                    let Some(routed_task) = self.binding_for(id).cloned() else {
                        // Unreachable in practice -- binding above covers every
                        // dispatchable id -- but a task that cannot say what it
                        // runs with must not be launched on a guess.
                        continue;
                    };
                    if current_root != routed_task.planned_site_map_root {
                        let mut result = WorkerResult::new(&task);
                        result.success = false;
                        result.message = format!(
                            "stale routed plan: planned SiteMap root {:016x} but current root is {:016x}",
                            routed_task.planned_site_map_root, current_root
                        );
                        result.status_updates.push(result.message.clone());
                        self.set_task_status(id, TaskStatus::Blocked(result));
                        continue;
                    }
                    // Admission gate: take a seat before spawning. Every seat
                    // busy means the plan is already running at capacity; the
                    // task stays Pending and is re-offered on the next tick.
                    // LeastLoaded spreads starts across seats so each one's
                    // duration/success statistics stay comparable.
                    let Some(seat) = self
                        .load_balancer
                        .assign_task(routed_task.task_kind.as_str(), task.estimated_weight)
                    else {
                        continue;
                    };
                    self.load_balancer.record_task_start(seat);
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
                    self.set_task_status(id, TaskStatus::Running);
                    self.worker_seats.insert(id, seat);
                    self.running_workers.insert(id, handle);
                }
                Err(err) => {
                    let mut result = WorkerResult::new(&task);
                    result.success = false;
                    result.message = format!("failed to open site map for freshness check: {err}");
                    result.status_updates.push(result.message.clone());
                    self.set_task_status(id, TaskStatus::Blocked(result));
                }
            }
        }

        self.execution_running = !self.running_workers.is_empty();
        self.refresh_runtime_status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::orchestrator::panel::struct_def::OrchestratorPanel;

    // Non-vacuity of the parameterized wall clock: with `hard_timeout_for`
    // reverted to always return the old 90-minute const, the first assertion
    // fails because `0` becomes 5400s, not MAX. If it instead always returned
    // MAX, the second assertion fails because 5400s does not come back as
    // MAX. The pair proves both the plumbing and the semantics.
    #[test]
    fn panel_wall_secs_zero_means_unbounded() {
        assert_eq!(hard_timeout_for(0), Duration::MAX);
        assert_eq!(hard_timeout_for(5400), Duration::from_secs(5400));
        assert_eq!(hard_timeout_for(1), Duration::from_secs(1));
    }

    #[test]
    fn panel_default_wall_secs_is_90_minutes_and_operator_can_lift_it() {
        let mut panel = OrchestratorPanel::new();
        assert_eq!(
            panel.max_worker_wall_secs,
            OrchestratorPanel::DEFAULT_MAX_WORKER_WALL_SECS
        );
        assert_eq!(panel.max_worker_wall_secs, 90 * 60);
        panel.max_worker_wall_secs = 0;
        assert_eq!(hard_timeout_for(panel.max_worker_wall_secs), Duration::MAX);
    }
}
