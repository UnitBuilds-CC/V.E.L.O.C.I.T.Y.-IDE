use super::types::*;
use crate::automation::{
    resolve_weight_root, AgentTaskKind, DecompositionStyle, InstructionRegistry, RoutedSubAgentTask,
};
use crate::editor::theme::IdePalette;
use crate::orchestrator::blueprint::{Task, TaskGraph};
use crate::orchestrator::registry::{OrchestratorRegistry, TaskStatus};
use crate::orchestrator::scheduler;
use crate::orchestrator::validator;
use crate::orchestrator::worker::{
    spawn_live_worker, WorkerAssignment, WorkerHandle, WorkerResult,
};
use crate::orchestrator::TaskId;
use eframe::egui;
use egui::{ScrollArea, Ui};
use std::collections::HashMap;
use std::path::Path;

pub struct OrchestratorPanel {
    pub graph: TaskGraph,
    pub registry: Option<OrchestratorRegistry>,
    pub expanded: bool,
    pub routed_plan: Option<RoutedPlanState>,
    pub policy_editor: PolicyEditorState,
    pub planning_status: String,
    pub runtime_status: String,
    pub execution_running: bool,
    pub running_workers: HashMap<TaskId, Box<dyn WorkerHandle>>,
    // Builder form state
    pub builder_title: String,
    pub builder_desc: String,
    pub builder_deps: String,
    pub builder_scope: String,
    pub next_task_id: u64,
}

impl Default for OrchestratorPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl OrchestratorPanel {
    pub fn new() -> Self {
        let graph = TaskGraph::example_game();
        let registry = OrchestratorRegistry::new(&graph);
        Self {
            graph,
            registry: Some(registry),
            expanded: true,
            routed_plan: None,
            policy_editor: PolicyEditorState::default(),
            planning_status: "No routed sub-agent plan yet.".to_string(),
            runtime_status: "Idle".to_string(),
            execution_running: false,
            running_workers: HashMap::new(),
            builder_title: String::new(),
            builder_desc: String::new(),
            builder_deps: String::new(),
            builder_scope: String::new(),
            next_task_id: 10,
        }
    }

    pub fn set_routed_tasks(
        &mut self,
        goal: String,
        kind: AgentTaskKind,
        scope_count: usize,
        tasks: Vec<RoutedSubAgentTask>,
    ) {
        self.planning_status = if tasks.is_empty() {
            "No routed tasks were produced for the requested goal.".to_string()
        } else {
            format!(
                "Planned {} routed task(s) from {} scoped file(s).",
                tasks.len(),
                scope_count,
            )
        };
        self.routed_plan = Some(RoutedPlanState {
            goal: goal.clone(),
            kind,
            scope_count,
            tasks: tasks.clone(),
        });
        self.policy_editor.kind = kind;
        self.policy_editor.loaded_policy_id.clear();
        self.graph = build_routed_graph(&goal, &tasks);
        self.registry = Some(OrchestratorRegistry::new(&self.graph));
        self.runtime_status = "Plan ready".to_string();
        self.execution_running = false;
        self.running_workers.clear();
    }

    pub fn selected_policy_kind(&self) -> AgentTaskKind {
        self.policy_editor.kind
    }

    pub fn dashboard_snapshot(&self) -> OrchestratorDashboardSnapshot {
        let has_routed_plan = self.routed_plan.is_some();
        let has_dependency_cycle = scheduler::detect_cycle(&self.graph);
        let retryable_blocked_tasks = self.retryable_blocked_task_count();
        let has_runtime_activity = has_routed_plan
            || self.execution_running
            || !self.running_workers.is_empty()
            || self.runtime_status != "Idle"
            || self.registry.as_ref().is_some_and(|reg| {
                !reg.outputs.is_empty()
                    || reg
                        .statuses
                        .values()
                        .any(|status| !matches!(status, TaskStatus::Pending))
            });
        let mut snapshot = OrchestratorDashboardSnapshot {
            goal: self.routed_plan.as_ref().map(|plan| plan.goal.clone()),
            task_kind: self
                .routed_plan
                .as_ref()
                .map(|plan| plan.kind.as_str().to_string()),
            scope_count: self
                .routed_plan
                .as_ref()
                .map(|plan| plan.scope_count)
                .unwrap_or(0),
            planning_status: self.planning_status.clone(),
            runtime_status: self.runtime_status.clone(),
            execution_running: self.execution_running,
            has_routed_plan,
            has_dependency_cycle,
            can_launch_routed_tasks: has_routed_plan
                && !has_dependency_cycle
                && !self.execution_running,
            can_reset_runtime: has_runtime_activity,
            active_workers: self.running_workers.len(),
            retryable_blocked_tasks,
            ..OrchestratorDashboardSnapshot::default()
        };

        for task in self.graph.tasks.values() {
            let status = self
                .registry
                .as_ref()
                .and_then(|registry| registry.statuses.get(&task.id))
                .cloned()
                .unwrap_or(TaskStatus::Pending);
            let routed = routed_task_for_id(&self.routed_plan, task.id);
            let (
                status_label,
                outputs,
                message,
                provider_label,
                model_label,
                run_summary_path,
                run_facts_path,
                wa_run_path,
                wa_run_id,
            ) = match status {
                TaskStatus::Pending => {
                    snapshot.pending_tasks += 1;
                    (
                        "Pending".to_string(),
                        Vec::new(),
                        String::new(),
                        routed
                            .map(|task| task.provider.label().to_string())
                            .unwrap_or_default(),
                        routed
                            .map(|task| task.model_label.clone())
                            .unwrap_or_default(),
                        None,
                        None,
                        None,
                        None,
                    )
                }
                TaskStatus::Running => {
                    snapshot.running_tasks += 1;
                    (
                        "Running".to_string(),
                        Vec::new(),
                        String::new(),
                        routed
                            .map(|task| task.provider.label().to_string())
                            .unwrap_or_default(),
                        routed
                            .map(|task| task.model_label.clone())
                            .unwrap_or_default(),
                        None,
                        None,
                        None,
                        None,
                    )
                }
                TaskStatus::Done(result) => {
                    snapshot.done_tasks += 1;
                    (
                        "Done".to_string(),
                        task_result_outputs(&result),
                        result.message.clone(),
                        result.provider_label,
                        result.model_label,
                        result
                            .run_summary_path
                            .as_ref()
                            .map(|path| path.display().to_string()),
                        result
                            .run_facts_path
                            .as_ref()
                            .map(|path| path.display().to_string()),
                        result.wa_run_path.clone(),
                        result.wa_run_id.clone(),
                    )
                }
                TaskStatus::Failed(result) => {
                    snapshot.failed_tasks += 1;
                    (
                        "Failed".to_string(),
                        task_result_outputs(&result),
                        result.message.clone(),
                        result.provider_label,
                        result.model_label,
                        result
                            .run_summary_path
                            .as_ref()
                            .map(|path| path.display().to_string()),
                        result
                            .run_facts_path
                            .as_ref()
                            .map(|path| path.display().to_string()),
                        result.wa_run_path.clone(),
                        result.wa_run_id.clone(),
                    )
                }
                TaskStatus::Blocked(result) => {
                    snapshot.blocked_tasks += 1;
                    (
                        "Follow-up".to_string(),
                        task_result_outputs(&result),
                        result.message.clone(),
                        result.provider_label,
                        result.model_label,
                        result
                            .run_summary_path
                            .as_ref()
                            .map(|path| path.display().to_string()),
                        result
                            .run_facts_path
                            .as_ref()
                            .map(|path| path.display().to_string()),
                        result.wa_run_path.clone(),
                        result.wa_run_id.clone(),
                    )
                }
            };

            snapshot.tasks.push(OrchestratorTaskSnapshot {
                id: task.id.0,
                title: task.title.clone(),
                description: task.description.clone(),
                status_label,
                provider_label,
                model_label,
                scope: task.scope.clone(),
                rationale: routed
                    .map(|task| task.rationale.clone())
                    .unwrap_or_default(),
                outputs,
                message,
                run_summary_path,
                run_facts_path,
                wa_run_path,
                wa_run_id,
                live_thread: self
                    .running_workers
                    .get(&task.id)
                    .map(|handle| handle.snapshot()),
            });
        }

        snapshot.tasks.sort_by_key(|task| task.id);
        snapshot
    }

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

    pub fn set_selected_policy_kind(&mut self, kind: AgentTaskKind) {
        if self.policy_editor.kind != kind {
            self.policy_editor.kind = kind;
            self.policy_editor.loaded_policy_id.clear();
        }
    }

    pub fn ensure_policy_editor_loaded(&mut self, workspace_root: &Path) {
        let registry = InstructionRegistry::open(workspace_root);
        let policies = registry.policies_for_kind(self.policy_editor.kind);
        let desired_policy_id = registry
            .policy_for_kind(self.policy_editor.kind)
            .or_else(|| policies.first().copied())
            .map(|policy| policy.id.clone())
            .unwrap_or_default();

        if self.policy_editor.selected_policy_id.is_empty() {
            self.policy_editor.selected_policy_id = desired_policy_id.clone();
        }
        let load_policy_id = if registry
            .get_policy(&self.policy_editor.selected_policy_id)
            .filter(|policy| policy.task_kind == self.policy_editor.kind)
            .is_some()
        {
            self.policy_editor.selected_policy_id.clone()
        } else {
            desired_policy_id
        };

        if self.policy_editor.loaded_policy_id == load_policy_id {
            return;
        }

        if let Some(policy) = registry.get_policy(&load_policy_id) {
            self.policy_editor.selected_policy_id = policy.id.clone();
            self.policy_editor.loaded_policy_id = policy.id.clone();
            self.policy_editor.draft_label = policy.label.clone();
            self.policy_editor.draft_template_id = policy.instruction_template_id.clone();
            self.policy_editor.draft_style = policy.decomposition_style;
            self.policy_editor.draft_expectations = policy.shared_expectations.join("\n");
            self.policy_editor.status = format!(
                "Editing policy '{}' for {}.",
                policy.label,
                self.policy_editor.kind.as_str()
            );
        }
    }

    pub fn render_policy_controls(&mut self, ui: &mut Ui, workspace_root: &Path) {
        let palette = IdePalette::dark();
        let registry = InstructionRegistry::open(workspace_root);
        let kind = self.policy_editor.kind;
        let policies = registry.policies_for_kind(kind);
        let templates = registry.templates_for_kind(kind);

        ui.group(|ui| {
            ui.label(egui::RichText::new("⚙ Routing policy controls").strong());
            ui.label(
                egui::RichText::new(&self.policy_editor.status)
                    .small()
                    .color(palette.text_muted),
            );

            egui::ComboBox::from_label("Task kind")
                .selected_text(kind.as_str())
                .show_ui(ui, |ui| {
                    for candidate in AgentTaskKind::ALL {
                        let selected = self.policy_editor.kind == candidate;
                        if ui.selectable_label(selected, candidate.as_str()).clicked() {
                            self.policy_editor.kind = candidate;
                            self.policy_editor.loaded_policy_id.clear();
                        }
                    }
                });

            let selected_policy_text = if self.policy_editor.selected_policy_id.is_empty() {
                "No policy".to_string()
            } else {
                self.policy_editor.selected_policy_id.clone()
            };
            egui::ComboBox::from_label("Preferred policy")
                .selected_text(selected_policy_text)
                .show_ui(ui, |ui| {
                    for policy in &policies {
                        let selected = self.policy_editor.selected_policy_id == policy.id;
                        if ui.selectable_label(selected, format!("{} ({})", policy.label, policy.id)).clicked() {
                            self.policy_editor.selected_policy_id = policy.id.clone();
                            self.policy_editor.loaded_policy_id.clear();
                        }
                    }
                });

            ui.horizontal(|ui| {
                if ui.button("Save preferred policy").clicked() {
                    let mut writable = InstructionRegistry::open(workspace_root);
                    writable.set_preferred_policy(self.policy_editor.kind, self.policy_editor.selected_policy_id.clone());
                    match writable.persist() {
                        Ok(()) => {
                            self.policy_editor.status = format!(
                                "Preferred policy for {} saved as '{}'.",
                                self.policy_editor.kind.as_str(),
                                self.policy_editor.selected_policy_id
                            );
                        }
                        Err(err) => {
                            self.policy_editor.status = format!("Failed to save preferred policy: {err}");
                        }
                    }
                }
                if ui.button("Reload policy").clicked() {
                    self.policy_editor.loaded_policy_id.clear();
                    self.ensure_policy_editor_loaded(workspace_root);
                }
            });

            ui.separator();
            ui.label(egui::RichText::new("Policy details").small().strong());
            ui.horizontal(|ui| {
                ui.label("Label:");
                ui.text_edit_singleline(&mut self.policy_editor.draft_label);
            });
            ui.horizontal(|ui| {
                ui.label("Template:");
                let selected_template = if self.policy_editor.draft_template_id.is_empty() {
                    "No template".to_string()
                } else {
                    self.policy_editor.draft_template_id.clone()
                };
                egui::ComboBox::from_id_salt("policy-template-select")
                    .selected_text(selected_template)
                    .show_ui(ui, |ui: &mut egui::Ui| {
                        for template in &templates {
                            let selected = self.policy_editor.draft_template_id == template.id;
                            if ui.selectable_label(selected, format!("{} ({})", template.label, template.id)).clicked() {
                                self.policy_editor.draft_template_id = template.id.clone();
                            }
                        }
                    });
            });
            ui.horizontal(|ui| {
                ui.label("Style:");
                egui::ComboBox::from_id_salt("policy-style-select")
                    .selected_text(self.policy_editor.draft_style.as_str())
                    .show_ui(ui, |ui: &mut egui::Ui| {
                        for style in DecompositionStyle::ALL {
                            let selected = self.policy_editor.draft_style == style;
                            if ui.selectable_label(selected, style.as_str()).clicked() {
                                self.policy_editor.draft_style = style;
                            }
                        }
                    });
            });
            ui.label("Shared expectations (one per line):");
            ui.add(
                egui::TextEdit::multiline(&mut self.policy_editor.draft_expectations)
                    .desired_rows(4)
                    .desired_width(f32::INFINITY),
            );

            if ui.button("Persist policy edits").clicked() {
                let mut writable = InstructionRegistry::open(workspace_root);
                let mut policy = match writable.get_policy(&self.policy_editor.selected_policy_id).cloned() {
                    Some(policy) => policy,
                    None => {
                        self.policy_editor.status = "Select a valid policy before persisting edits.".to_string();
                        return;
                    }
                };
                policy.label = self.policy_editor.draft_label.trim().to_string();
                policy.instruction_template_id = self.policy_editor.draft_template_id.trim().to_string();
                policy.decomposition_style = self.policy_editor.draft_style;
                policy.shared_expectations = self
                    .policy_editor
                    .draft_expectations
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(ToOwned::to_owned)
                    .collect();
                writable.upsert_policy(policy);
                writable.set_preferred_policy(self.policy_editor.kind, self.policy_editor.selected_policy_id.clone());
                match writable.persist() {
                    Ok(()) => {
                        self.policy_editor.loaded_policy_id.clear();
                        self.ensure_policy_editor_loaded(workspace_root);
                        self.policy_editor.status = format!(
                            "Persisted policy '{}' for {}. Re-run routed planning to apply changes.",
                            self.policy_editor.selected_policy_id,
                            self.policy_editor.kind.as_str()
                        );
                    }
                    Err(err) => {
                        self.policy_editor.status = format!("Failed to persist policy edits: {err}");
                    }
                }
            }
        });
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        workspace_root: &Path,
        mediator: &std::sync::Arc<crate::automation::mediator::MediatorArena>,
    ) {
        let palette = IdePalette::dark();
        self.ensure_policy_editor_loaded(workspace_root);
        if self.execution_running {
            self.poll_live_workers(workspace_root, mediator);
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }

        ui.horizontal(|ui| {
            ui.heading("🧠 Live Orchestrator");
            let label = if self.expanded {
                "− Less Details"
            } else {
                "+ More Details"
            };
            ui.toggle_value(&mut self.expanded, label);
        });
        ui.separator();

        self.render_policy_controls(ui, workspace_root);
        ui.add_space(6.0);

        if let Some(plan) = &self.routed_plan {
            ui.group(|ui| {
                ui.label(egui::RichText::new("🧭 Routed sub-agent plan").strong());
                ui.label(
                    egui::RichText::new(&self.planning_status)
                        .small()
                        .color(palette.text_muted),
                );
                ui.label(format!("Goal: {}", plan.goal));
                ui.label(format!("Task kind: {}", plan.kind.as_str()));
                ui.label(format!(
                    "Scoped files: {} | Planned agents: {}",
                    plan.scope_count,
                    plan.tasks.len()
                ));
            });
            ui.add_space(6.0);
        }

        // Cycle Warning
        let has_cycle = scheduler::detect_cycle(&self.graph);
        if has_cycle {
            ui.group(|ui| {
                ui.colored_label(palette.error, "❌ Dependency Loop Blocked Scheduling!");
                ui.label("Topological sort is disabled until the loop is fixed.");
            });
        }

        ui.group(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(format!("Runtime: {}", self.runtime_status))
                        .small()
                        .color(palette.text_muted),
                );
                ui.separator();
                ui.label(format!("Active workers: {}", self.running_workers.len()));
            });

            ui.horizontal(|ui| {
                if has_cycle {
                    ui.add_enabled_ui(false, |ui| {
                        let _ = ui.button("▶ Execute Routed Tasks");
                    });
                } else if self.execution_running {
                    ui.add_enabled_ui(false, |ui| {
                        let _ = ui.button("▶ Executing…");
                    });
                } else if ui.button("▶ Execute Routed Tasks").clicked() {
                    self.execute_routed_tasks(workspace_root, mediator);
                }

                let retryable_blocked = self.retryable_blocked_task_count();
                if ui
                    .add_enabled(
                        !self.execution_running && retryable_blocked > 0,
                        egui::Button::new(format!("↻ Retry Blocked Tasks ({retryable_blocked})")),
                    )
                    .clicked()
                {
                    self.retry_blocked_tasks_action(workspace_root, mediator);
                }

                if ui.button("↻ Reset Runtime").clicked() {
                    self.reset_runtime_action();
                }

                ui.separator();

                if ui.button("⚠️ Inject Cycle").clicked() {
                    if let Some(t3) = self.graph.tasks.get_mut(&TaskId(3)) {
                        if !t3.dependencies.contains(&TaskId(4)) {
                            t3.dependencies.push(TaskId(4));
                        }
                    }
                    if let Some(t4) = self.graph.tasks.get_mut(&TaskId(4)) {
                        if !t4.dependencies.contains(&TaskId(3)) {
                            t4.dependencies.push(TaskId(3));
                        }
                    }
                    self.execution_running = false;
                    self.running_workers.clear();
                    self.runtime_status = "Cycle injected".to_string();
                }

                if ui.button("Fix & Reset Graph").clicked() {
                    self.graph = TaskGraph::example_game();
                    self.registry = Some(OrchestratorRegistry::new(&self.graph));
                    self.execution_running = false;
                    self.running_workers.clear();
                    self.runtime_status = "Graph reset".to_string();
                }
            });
        });

        ui.add_space(4.0);
        let plan = if has_cycle {
            scheduler::Plan::default()
        } else {
            scheduler::plan(&self.graph)
        };
        ui.label(format!(
            "Tasks: {} | Phases: {}",
            self.graph.tasks.len(),
            plan.phases.len()
        ));

        ui.columns(2, |columns: &mut [Ui]| {
            columns[0].vertical(|ui: &mut egui::Ui| {
                ScrollArea::vertical().show(ui, |ui: &mut egui::Ui| {
                    if let Some(route_plan) = &self.routed_plan {
                        ui.group(|ui| {
                            ui.label(egui::RichText::new("Planned routed assignments").strong());
                            for task in &route_plan.tasks {
                                ui.add_space(4.0);
                                ui.group(|ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(egui::RichText::new(&task.task_id).strong());
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "{} / {} / {}",
                                                task.task_kind.as_str(),
                                                task.provider.label(),
                                                task.model_label,
                                            ))
                                            .small()
                                            .color(palette.accent),
                                        );
                                    });
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "Policy: {} ({}) | Template: {}",
                                            task.decomposition_policy_id,
                                            task.decomposition_style.as_str(),
                                            task.instruction_template_id,
                                        ))
                                        .small()
                                        .color(palette.text_muted),
                                    );
                                    ui.label(
                                        egui::RichText::new(&task.rationale)
                                            .small()
                                            .color(palette.text),
                                    );
                                    if !task.files.is_empty() {
                                        let scope = task
                                            .files
                                            .iter()
                                            .map(|file| file.display().to_string())
                                            .collect::<Vec<_>>()
                                            .join(", ");
                                        ui.label(
                                            egui::RichText::new(format!("Files: {scope}"))
                                                .small()
                                                .color(palette.accent),
                                        );
                                    }
                                    if !task.fallback_chain.is_empty() {
                                        let fallback = task
                                            .fallback_chain
                                            .iter()
                                            .map(|route| {
                                                format!(
                                                    "{} / {} [{}]",
                                                    route.provider.label(),
                                                    route.model_label,
                                                    route.score
                                                )
                                            })
                                            .collect::<Vec<_>>()
                                            .join(" -> ");
                                        ui.label(
                                            egui::RichText::new(format!("Fallbacks: {fallback}"))
                                                .small()
                                                .color(palette.warning),
                                        );
                                    }
                                });
                            }
                        });
                        ui.add_space(8.0);
                    }
                    if !has_cycle {
                        for (phase_idx, phase) in plan.phases.iter().enumerate() {
                            ui.group(|ui: &mut egui::Ui| {
                                ui.label(
                                    egui::RichText::new(format!("Phase {}", phase_idx + 1))
                                        .strong(),
                                );
                                for id in phase {
                                    if let Some(task) = self.graph.tasks.get(id) {
                                        self.task_row(ui, task);
                                    }
                                }
                            });
                            ui.add_space(4.0);
                        }
                    } else {
                        ui.group(|ui: &mut egui::Ui| {
                            ui.label("Raw Tasks List (Unscheduled):");
                            for task in self.graph.tasks.values() {
                                self.task_row(ui, task);
                            }
                        });
                    }

                    let outputs = self.registry.as_ref().map(|r| &r.outputs);
                    let collisions = if let Some(outs) = outputs {
                        crate::orchestrator::reconcile::detect_collisions(&self.graph, outs)
                    } else {
                        Vec::new()
                    };
                    let violations = if let Some(outs) = outputs {
                        crate::orchestrator::reconcile::scope_violations(&self.graph, outs)
                    } else {
                        Vec::new()
                    };

                    if !collisions.is_empty() || !violations.is_empty() {
                        ui.add_space(8.0);
                        ui.group(|ui: &mut egui::Ui| {
                            ui.label(
                                egui::RichText::new("⚠️ Reconciler Warnings")
                                    .strong()
                                    .color(palette.warning),
                            );
                            for c in &collisions {
                                ui.colored_label(
                                    palette.warning,
                                    format!(
                                        "Conflict: tasks {} and {} both touch file '{}'",
                                        c.task_a, c.task_b, c.path
                                    ),
                                );
                            }
                            for v in &violations {
                                ui.colored_label(
                                    palette.error,
                                    format!(
                                        "Scope Violation: task {} wrote unauthorized path '{}'",
                                        v.0, v.1
                                    ),
                                );
                            }
                        });
                    }

                    ui.add_space(8.0);
                    ui.collapsing("⚙️ Add Custom Task", |ui: &mut egui::Ui| {
                        ui.horizontal(|ui: &mut egui::Ui| {
                            ui.label("Title:");
                            ui.text_edit_singleline(&mut self.builder_title);
                        });
                        ui.horizontal(|ui: &mut egui::Ui| {
                            ui.label("Description:");
                            ui.text_edit_singleline(&mut self.builder_desc);
                        });
                        ui.horizontal(|ui: &mut egui::Ui| {
                            ui.label("Dependencies (comma-separated IDs, e.g. 1,2):");
                            ui.text_edit_singleline(&mut self.builder_deps);
                        });
                        ui.horizontal(|ui: &mut egui::Ui| {
                            ui.label("Scope Paths (comma-separated, e.g. crates/renderer):");
                            ui.text_edit_singleline(&mut self.builder_scope);
                        });
                        if ui.button("➕ Add Task").clicked() {
                            if !self.builder_title.is_empty() {
                                let id = TaskId(self.next_task_id);
                                self.next_task_id += 1;

                                let mut deps = Vec::new();
                                for d_str in self.builder_deps.split(',') {
                                    if let Ok(d_id) = d_str.trim().parse::<u64>() {
                                        deps.push(TaskId(d_id));
                                    }
                                }

                                let scope: Vec<String> = self
                                    .builder_scope
                                    .split(',')
                                    .map(|s| s.trim().to_string())
                                    .filter(|s| !s.is_empty())
                                    .collect();

                                self.graph.add(
                                    id,
                                    &self.builder_title,
                                    &self.builder_desc,
                                    scope,
                                    deps,
                                    None,
                                );

                                self.registry = Some(OrchestratorRegistry::new(&self.graph));
                                self.builder_title.clear();
                                self.builder_desc.clear();
                                self.builder_deps.clear();
                                self.builder_scope.clear();
                            }
                        }
                    });
                });
            });

            columns[1].vertical(|ui: &mut egui::Ui| {
                ui.label(
                    egui::RichText::new("📊 TASK FLOW PIPELINE")
                        .strong()
                        .color(palette.accent),
                );
                self.draw_task_graph(ui, &plan, has_cycle);
            });
        });
    }

    pub fn draw_task_graph(&self, ui: &mut Ui, plan: &scheduler::Plan, has_cycle: bool) {
        let palette = IdePalette::dark();

        let mut canvas_size = ui.available_size();
        if !canvas_size.x.is_finite() {
            canvas_size.x = 400.0;
        }
        if !canvas_size.y.is_finite() {
            canvas_size.y = 300.0;
        }
        canvas_size.y = canvas_size.y.min(350.0);

        let (rect, _response) = ui.allocate_exact_size(canvas_size, egui::Sense::hover());
        let painter = ui.painter_at(rect);

        painter.rect_filled(rect, 4.0, palette.bg_primary);

        let mut node_positions = HashMap::new();

        if !has_cycle {
            let x_spacing = 160.0;
            let y_spacing = 75.0;
            let start_pos = rect.min + egui::vec2(60.0, 40.0);

            for (phase_idx, phase) in plan.phases.iter().enumerate() {
                let x = start_pos.x + phase_idx as f32 * x_spacing;
                for (task_idx, &id) in phase.iter().enumerate() {
                    let y = start_pos.y + task_idx as f32 * y_spacing;
                    node_positions.insert(id, egui::pos2(x, y));
                }
            }
        } else {
            let center = rect.center();
            let radius = (rect.width().min(rect.height()) * 0.3).max(80.0);
            let tasks_vec: Vec<TaskId> = self.graph.tasks.keys().cloned().collect();
            let count = tasks_vec.len();
            for (idx, &id) in tasks_vec.iter().enumerate() {
                let angle = (idx as f32 / count as f32) * 2.0 * std::f32::consts::PI;
                let x = center.x + radius * angle.cos();
                let y = center.y + radius * angle.sin();
                node_positions.insert(id, egui::pos2(x, y));
            }
        }

        for (&id, task) in &self.graph.tasks {
            if let Some(&p_to) = node_positions.get(&id) {
                for dep_id in &task.dependencies {
                    if let Some(&p_from) = node_positions.get(dep_id) {
                        painter
                            .line_segment([p_from, p_to], egui::Stroke::new(1.5, palette.border));
                    }
                }
            }
        }

        for (&id, task) in &self.graph.tasks {
            if let Some(&pos) = node_positions.get(&id) {
                let status = self
                    .registry
                    .as_ref()
                    .and_then(|r| r.statuses.get(&id))
                    .cloned()
                    .unwrap_or(TaskStatus::Pending);

                let color = match status {
                    TaskStatus::Pending => palette.text_muted.gamma_multiply(0.6),
                    TaskStatus::Running => palette.accent,
                    TaskStatus::Done(_) => palette.success,
                    TaskStatus::Failed(_) => palette.error,
                    TaskStatus::Blocked(_) => palette.warning,
                };

                let size = egui::vec2(130.0, 45.0);
                let node_rect = egui::Rect::from_center_size(pos, size);
                painter.rect(
                    node_rect,
                    6.0,
                    color,
                    egui::Stroke::new(1.0, palette.text),
                    egui::StrokeKind::Inside,
                );

                let truncated_title: String = task.title.chars().take(16).collect();
                painter.text(
                    pos,
                    egui::Align2::CENTER_CENTER,
                    format!("ID: {}\n{}", id.0, truncated_title),
                    egui::FontId::monospace(10.0),
                    palette.text,
                );
            }
        }
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

    pub fn task_row(&self, ui: &mut Ui, task: &Task) {
        let status = self
            .registry
            .as_ref()
            .and_then(|r| r.statuses.get(&task.id))
            .cloned()
            .unwrap_or(TaskStatus::Pending);

        let palette = IdePalette::dark();
        let (status_label, bg_color) = match &status {
            TaskStatus::Pending => ("⏳ Pending", palette.text_muted),
            TaskStatus::Running => ("🔵 Running", palette.accent),
            TaskStatus::Done(_) => ("✅ Done", palette.success),
            TaskStatus::Failed(_) => ("❌ Failed", palette.error),
            TaskStatus::Blocked(_) => ("⚠️ Follow-up", palette.warning),
        };

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(status_label).color(bg_color).strong());
            ui.label(
                egui::RichText::new(format!("(ID: {})", task.id.0))
                    .small()
                    .weak(),
            );
            ui.label(&task.title);
        });

        if self.expanded {
            ui.horizontal_wrapped(|ui| {
                ui.add_space(16.0);
                ui.label(egui::RichText::new(&task.description).small().weak());
            });
            if !task.scope.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(16.0);
                    let scope_str = task.scope.join(", ");
                    ui.label(
                        egui::RichText::new(format!("Scope: {scope_str}"))
                            .small()
                            .color(palette.accent),
                    );
                });
            }
            match &status {
                TaskStatus::Done(result)
                | TaskStatus::Failed(result)
                | TaskStatus::Blocked(result) => {
                    ui.horizontal_wrapped(|ui| {
                        ui.add_space(16.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "Route: {} / {} | Duration: {:.2?}",
                                result.provider_label, result.model_label, result.duration,
                            ))
                            .small()
                            .color(palette.accent),
                        );
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.add_space(16.0);
                        ui.label(egui::RichText::new(&result.message).small().weak());
                    });
                    if !result.outputs.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.add_space(16.0);
                            ui.label(
                                egui::RichText::new(format!(
                                    "Changed: {}",
                                    result.outputs.join(", ")
                                ))
                                .small()
                                .color(palette.success),
                            );
                        });
                    }
                    if !result.created_files.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.add_space(16.0);
                            ui.label(
                                egui::RichText::new(format!(
                                    "Created: {}",
                                    result.created_files.join(", ")
                                ))
                                .small()
                                .color(palette.warning),
                            );
                        });
                    }
                    if !result.deleted_files.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.add_space(16.0);
                            ui.label(
                                egui::RichText::new(format!(
                                    "Deleted: {}",
                                    result.deleted_files.join(", ")
                                ))
                                .small()
                                .color(palette.error),
                            );
                        });
                    }
                    if !result.out_of_scope_created_files.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.add_space(16.0);
                            ui.label(
                                egui::RichText::new(format!(
                                    "Out-of-scope created: {}",
                                    result.out_of_scope_created_files.join(", ")
                                ))
                                .small()
                                .color(palette.warning),
                            );
                        });
                    }
                    if !result.attempts.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.add_space(16.0);
                            let attempts = result
                                .attempts
                                .iter()
                                .map(|attempt| {
                                    format!(
                                        "{} / {} ({})",
                                        attempt.provider_label,
                                        attempt.model_label,
                                        if attempt.success { "ok" } else { "miss" }
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join(" -> ");
                            ui.label(
                                egui::RichText::new(format!("Attempts: {attempts}"))
                                    .small()
                                    .color(palette.warning),
                            );
                        });
                    }
                    if !result.status_updates.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.add_space(16.0);
                            ui.label(
                                egui::RichText::new(format!(
                                    "Status: {}",
                                    result.status_updates.join(" | ")
                                ))
                                .small()
                                .weak(),
                            );
                        });
                    }
                    if !result.transcript.trim().is_empty() {
                        let preview: String = result.transcript.chars().take(240).collect();
                        ui.horizontal_wrapped(|ui| {
                            ui.add_space(16.0);
                            ui.label(
                                egui::RichText::new(format!(
                                    "Transcript: {}{}",
                                    preview,
                                    if result.transcript.len() > preview.len() {
                                        "…"
                                    } else {
                                        ""
                                    }
                                ))
                                .small()
                                .color(palette.text),
                            );
                        });
                    }
                }
                _ => {}
            }
        }
    }
}
