//! Optional IDE panel showing the orchestrator task graph.

use crate::automation::{
    resolve_weight_root, AgentTaskKind, DecompositionStyle, InstructionRegistry, RoutedSubAgentTask,
};
use crate::orchestrator::blueprint::{Task, TaskGraph};
use crate::orchestrator::registry::{OrchestratorRegistry, TaskStatus};
use crate::orchestrator::scheduler;
use crate::orchestrator::validator;
use crate::orchestrator::worker::{spawn_live_worker, WorkerAssignment, WorkerHandle, WorkerResult};
use crate::orchestrator::TaskId;
use eframe::egui;
use egui::{ScrollArea, Ui};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone)]
struct RoutedPlanState {
    goal: String,
    kind: AgentTaskKind,
    scope_count: usize,
    tasks: Vec<RoutedSubAgentTask>,
}

#[derive(Debug, Clone)]
struct PolicyEditorState {
    kind: AgentTaskKind,
    selected_policy_id: String,
    loaded_policy_id: String,
    draft_label: String,
    draft_template_id: String,
    draft_style: DecompositionStyle,
    draft_expectations: String,
    status: String,
}

impl Default for PolicyEditorState {
    fn default() -> Self {
        Self {
            kind: AgentTaskKind::Refactor,
            selected_policy_id: String::new(),
            loaded_policy_id: String::new(),
            draft_label: String::new(),
            draft_template_id: String::new(),
            draft_style: DecompositionStyle::CoupledComponents,
            draft_expectations: String::new(),
            status: "Select a policy to tune routed planning.".to_string(),
        }
    }
}

pub struct OrchestratorPanel {
    pub graph: TaskGraph,
    pub registry: Option<OrchestratorRegistry>,
    pub expanded: bool,
    routed_plan: Option<RoutedPlanState>,
    policy_editor: PolicyEditorState,
    planning_status: String,
    runtime_status: String,
    execution_running: bool,
    running_workers: HashMap<TaskId, Box<dyn WorkerHandle>>,
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

    pub fn set_selected_policy_kind(&mut self, kind: AgentTaskKind) {
        if self.policy_editor.kind != kind {
            self.policy_editor.kind = kind;
            self.policy_editor.loaded_policy_id.clear();
        }
    }

    fn ensure_policy_editor_loaded(&mut self, workspace_root: &Path) {
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
            self.policy_editor.status = format!("Editing policy '{}' for {}.", policy.label, self.policy_editor.kind.as_str());
        }
    }

    fn render_policy_controls(&mut self, ui: &mut Ui, workspace_root: &Path) {
        let registry = InstructionRegistry::open(workspace_root);
        let kind = self.policy_editor.kind;
        let policies = registry.policies_for_kind(kind);
        let templates = registry.templates_for_kind(kind);

        ui.group(|ui| {
            ui.label(egui::RichText::new("⚙ Routing policy controls").strong());
            ui.label(
                egui::RichText::new(&self.policy_editor.status)
                    .small()
                    .color(egui::Color32::from_rgb(125, 131, 166)),
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

    pub fn ui(&mut self, ui: &mut Ui, workspace_root: &Path, mediator: &std::sync::Arc<crate::automation::mediator::MediatorArena>) {
        self.ensure_policy_editor_loaded(workspace_root);
        if self.execution_running {
            self.poll_live_workers(workspace_root, mediator);
            ui.ctx().request_repaint();
        }

        ui.horizontal(|ui| {
            ui.heading("🧠 Live Orchestrator");
            let label = if self.expanded { "− Less Details" } else { "+ More Details" };
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
                        .color(egui::Color32::from_rgb(125, 131, 166)),
                );
                ui.label(format!("Goal: {}", plan.goal));
                ui.label(format!("Task kind: {}", plan.kind.as_str()));
                ui.label(format!("Scoped files: {} | Planned agents: {}", plan.scope_count, plan.tasks.len()));
            });
            ui.add_space(6.0);
        }

        // Cycle Warning
        let has_cycle = scheduler::detect_cycle(&self.graph);
        if has_cycle {
            ui.group(|ui| {
                ui.colored_label(egui::Color32::from_rgb(239, 68, 68), "❌ Dependency Loop Blocked Scheduling!");
                ui.label("Topological sort is disabled until the loop is fixed.");
            });
        }

        ui.group(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(format!("Runtime: {}", self.runtime_status))
                        .small()
                        .color(egui::Color32::from_rgb(125, 131, 166)),
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
                    self.start_execution(workspace_root, mediator);
                }

                if ui.button("↻ Reset Runtime").clicked() {
                    self.reset_runtime();
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
        ui.label(format!("Tasks: {} | Phases: {}", self.graph.tasks.len(), plan.phases.len()));

        ui.columns(2, |columns: &mut [Ui]| {
            // Left column: Scroll area with lists and form
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
                                            .color(egui::Color32::from_rgb(34, 211, 238)),
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
                                        .color(egui::Color32::from_rgb(125, 131, 166)),
                                    );
                                    ui.label(
                                        egui::RichText::new(&task.rationale)
                                            .small()
                                            .color(egui::Color32::from_rgb(226, 227, 243)),
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
                                                .color(egui::Color32::from_rgb(168, 85, 247)),
                                        );
                                    }
                                    if !task.fallback_chain.is_empty() {
                                        let fallback = task
                                            .fallback_chain
                                            .iter()
                                            .map(|route| format!("{} / {} [{}]", route.provider.label(), route.model_label, route.score))
                                            .collect::<Vec<_>>()
                                            .join(" -> ");
                                        ui.label(
                                            egui::RichText::new(format!("Fallbacks: {fallback}"))
                                                .small()
                                                .color(egui::Color32::from_rgb(244, 114, 182)),
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
                                ui.label(egui::RichText::new(format!("Phase {}", phase_idx + 1)).strong());
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

                    // Reconciler checks
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
                            ui.label(egui::RichText::new("⚠️ Reconciler Warnings").strong().color(egui::Color32::from_rgb(234, 179, 8)));
                            for c in &collisions {
                                ui.colored_label(
                                    egui::Color32::from_rgb(250, 204, 21),
                                    format!("Conflict: tasks {} and {} both touch file '{}'", c.task_a, c.task_b, c.path),
                                );
                            }
                            for v in &violations {
                                ui.colored_label(
                                    egui::Color32::from_rgb(248, 113, 113),
                                    format!("Scope Violation: task {} wrote unauthorized path '{}'", v.0, v.1),
                                );
                            }
                        });
                    }

                    // Task Builder Form
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

                                let scope: Vec<String> = self.builder_scope
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

            // Right column: Canvas drawing the task graph pipeline
            columns[1].vertical(|ui: &mut egui::Ui| {
                ui.label(egui::RichText::new("📊 TASK FLOW PIPELINE").strong().color(egui::Color32::from_rgb(34, 211, 238)));
                self.draw_task_graph(ui, &plan, has_cycle);
            });
        });
    }

    fn draw_task_graph(&self, ui: &mut Ui, plan: &scheduler::Plan, has_cycle: bool) {
        use std::collections::HashMap;

        let mut canvas_size = ui.available_size();
        if !canvas_size.x.is_finite() { canvas_size.x = 400.0; }
        if !canvas_size.y.is_finite() { canvas_size.y = 300.0; }
        canvas_size.y = canvas_size.y.min(350.0);

        let (rect, _response) = ui.allocate_exact_size(canvas_size, egui::Sense::hover());
        let painter = ui.painter_at(rect);

        // Fill background
        painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(8, 9, 14));

        let mut node_positions = HashMap::new();

        if !has_cycle {
            // Compute positions based on Phase layout
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
            // Draw in a circle if there is a cycle
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

        // 1. Draw connecting dependency lines
        for (&id, task) in &self.graph.tasks {
            if let Some(&p_to) = node_positions.get(&id) {
                for dep_id in &task.dependencies {
                    if let Some(&p_from) = node_positions.get(dep_id) {
                        painter.line_segment([p_from, p_to], egui::Stroke::new(1.5, egui::Color32::from_rgb(100, 116, 139)));
                    }
                }
            }
        }

        // 2. Draw Task Node boxes
        for (&id, task) in &self.graph.tasks {
            if let Some(&pos) = node_positions.get(&id) {
                let status = self.registry.as_ref()
                    .and_then(|r| r.statuses.get(&id))
                    .cloned()
                    .unwrap_or(TaskStatus::Pending);

                let color = match status {
                    TaskStatus::Pending => egui::Color32::from_rgb(55, 65, 81),    // Gray
                    TaskStatus::Running => egui::Color32::from_rgb(59, 130, 246),  // Blue
                    TaskStatus::Done(_) => egui::Color32::from_rgb(34, 197, 94),   // Green
                    TaskStatus::Failed(_) => egui::Color32::from_rgb(239, 68, 68), // Red
                    TaskStatus::Blocked(_) => egui::Color32::from_rgb(245, 158, 11), // Amber
                };

                let size = egui::vec2(130.0, 45.0);
                let node_rect = egui::Rect::from_center_size(pos, size);
                painter.rect(
                    node_rect,
                    6.0,
                    color,
                    egui::Stroke::new(1.0, egui::Color32::from_rgb(226, 227, 243)),
                    egui::StrokeKind::Inside,
                );

                let truncated_title: String = task.title.chars().take(16).collect();
                painter.text(
                    pos,
                    egui::Align2::CENTER_CENTER,
                    format!("ID: {}\n{}", id.0, truncated_title),
                    egui::FontId::monospace(10.0),
                    egui::Color32::from_rgb(226, 227, 243),
                );
            }
        }
    }



    fn start_execution(&mut self, workspace_root: &Path, mediator: &std::sync::Arc<crate::automation::mediator::MediatorArena>) {
        if self.registry.is_none() {
            self.registry = Some(OrchestratorRegistry::new(&self.graph));
        }
        self.execution_running = true;
        self.runtime_status = "Dispatching routed tasks".to_string();
        self.poll_live_workers(workspace_root, mediator);
    }

    fn reset_runtime(&mut self) {
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

    fn poll_live_workers(&mut self, workspace_root: &Path, mediator: &std::sync::Arc<crate::automation::mediator::MediatorArena>) {
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
            let report = validator::validate(&result);
            let reconciliation_error = reconciliation_error(&self.graph, &reg.outputs, id, &outputs);
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
            let current_site_map_root = velocity_ide::site_map::SiteMap::open(&site_map_path, weight_root)
                .map(|site_map| site_map.root());
            match current_site_map_root {
                Ok(current_root) if current_root == routed_task.planned_site_map_root => {
                    let handle = spawn_live_worker(
                        WorkerAssignment {
                            task,
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
        self.runtime_status = if self.execution_running {
            format!("Running {} worker(s)", self.running_workers.len())
        } else if reg.has_blocked() {
            "Follow-up required".to_string()
        } else if reg.is_complete() {
            "Execution complete".to_string()
        } else {
            "Waiting for executable routed tasks".to_string()
        };
    }

    fn task_row(&self, ui: &mut Ui, task: &Task) {
        let status = self.registry.as_ref()
            .and_then(|r| r.statuses.get(&task.id))
            .cloned()
            .unwrap_or(TaskStatus::Pending);

        let (status_label, bg_color) = match &status {
            TaskStatus::Pending => ("⏳ Pending", egui::Color32::from_rgb(156, 163, 175)),
            TaskStatus::Running => ("🔵 Running", egui::Color32::from_rgb(96, 165, 250)),
            TaskStatus::Done(_) => ("✅ Done", egui::Color32::from_rgb(74, 222, 128)),
            TaskStatus::Failed(_) => ("❌ Failed", egui::Color32::from_rgb(248, 113, 113)),
            TaskStatus::Blocked(_) => ("⚠️ Follow-up", egui::Color32::from_rgb(251, 191, 36)),
        };

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(status_label).color(bg_color).strong());
            ui.label(egui::RichText::new(format!("(ID: {})", task.id.0)).small().weak());
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
                    ui.label(egui::RichText::new(format!("Scope: {scope_str}")).small().color(egui::Color32::from_rgb(168, 85, 247)));
                });
            }
            match &status {
                TaskStatus::Done(result) | TaskStatus::Failed(result) | TaskStatus::Blocked(result) => {
                    ui.horizontal_wrapped(|ui| {
                        ui.add_space(16.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "Route: {} / {} | Duration: {:.2?}",
                                result.provider_label,
                                result.model_label,
                                result.duration,
                            ))
                            .small()
                            .color(egui::Color32::from_rgb(34, 211, 238)),
                        );
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.add_space(16.0);
                        ui.label(egui::RichText::new(&result.message).small().weak());
                    });
                    if !result.outputs.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.add_space(16.0);
                            ui.label(egui::RichText::new(format!("Changed: {}", result.outputs.join(", "))).small().color(egui::Color32::from_rgb(74, 222, 128)));
                        });
                    }
                    if !result.created_files.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.add_space(16.0);
                            ui.label(egui::RichText::new(format!("Created: {}", result.created_files.join(", "))).small().color(egui::Color32::from_rgb(250, 204, 21)));
                        });
                    }
                    if !result.deleted_files.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.add_space(16.0);
                            ui.label(egui::RichText::new(format!("Deleted: {}", result.deleted_files.join(", "))).small().color(egui::Color32::from_rgb(248, 113, 113)));
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
                                .color(egui::Color32::from_rgb(251, 191, 36)),
                            );
                        });
                    }
                    if !result.attempts.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.add_space(16.0);
                            let attempts = result
                                .attempts
                                .iter()
                                .map(|attempt| format!("{} / {} ({})", attempt.provider_label, attempt.model_label, if attempt.success { "ok" } else { "miss" }))
                                .collect::<Vec<_>>()
                                .join(" -> ");
                            ui.label(egui::RichText::new(format!("Attempts: {attempts}")).small().color(egui::Color32::from_rgb(244, 114, 182)));
                        });
                    }
                    if !result.status_updates.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.add_space(16.0);
                            ui.label(egui::RichText::new(format!("Status: {}", result.status_updates.join(" | "))).small().weak());
                        });
                    }
                    if !result.transcript.trim().is_empty() {
                        let preview: String = result.transcript.chars().take(240).collect();
                        ui.horizontal_wrapped(|ui| {
                            ui.add_space(16.0);
                            ui.label(egui::RichText::new(format!("Transcript: {}{}", preview, if result.transcript.len() > preview.len() { "…" } else { "" })).small().color(egui::Color32::from_rgb(226, 227, 243)));
                        });
                    }
                }
                _ => {}
            }
        }
    }
}

fn routed_task_for_id(plan: &Option<RoutedPlanState>, task_id: TaskId) -> Option<&RoutedSubAgentTask> {
    let routed_idx = task_id.0.checked_sub(2)? as usize;
    plan.as_ref()?.tasks.get(routed_idx)
}

fn task_result_outputs(result: &WorkerResult) -> Vec<String> {
    let mut outputs = result.outputs.clone();
    outputs.extend(result.created_files.clone());
    outputs.extend(result.deleted_files.clone());
    outputs.sort();
    outputs.dedup();
    outputs
}

fn reconciliation_error(
    graph: &TaskGraph,
    existing_outputs: &HashMap<TaskId, Vec<String>>,
    task_id: TaskId,
    outputs: &[String],
) -> Option<String> {
    let mut candidate_outputs = existing_outputs.clone();
    candidate_outputs.insert(task_id, outputs.to_vec());

    let scope_violations = crate::orchestrator::reconcile::scope_violations(graph, &candidate_outputs)
        .into_iter()
        .filter(|(violating_task_id, _)| *violating_task_id == task_id)
        .map(|(_, path)| path)
        .collect::<Vec<_>>();
    if !scope_violations.is_empty() {
        return Some(format!(
            "Reconciliation blocked: task touched files outside its declared scope: {}",
            scope_violations.join(", ")
        ));
    }

    let collisions = crate::orchestrator::reconcile::detect_collisions(graph, &candidate_outputs)
        .into_iter()
        .filter(|collision| collision.task_a == task_id || collision.task_b == task_id)
        .map(|collision| {
            let other_task_id = if collision.task_a == task_id {
                collision.task_b.0
            } else {
                collision.task_a.0
            };
            format!("{} with task {}", collision.path, other_task_id)
        })
        .collect::<Vec<_>>();
    if !collisions.is_empty() {
        return Some(format!(
            "Reconciliation blocked: overlapping outputs detected for {}",
            collisions.join(", ")
        ));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::registry::OrchestratorRegistry;

    fn sample_result(task_id: TaskId, message: &str) -> WorkerResult {
        WorkerResult {
            success: false,
            task_id,
            outputs: Vec::new(),
            duration: std::time::Duration::ZERO,
            message: message.to_string(),
            provider_label: String::new(),
            model_label: String::new(),
            transcript: String::new(),
            status_updates: Vec::new(),
            attempts: Vec::new(),
            created_files: Vec::new(),
            deleted_files: Vec::new(),
            out_of_scope_created_files: Vec::new(),
            run_summary_path: None,
            run_facts_path: None,
        }
    }

    #[test]
    fn follow_up_detection_matches_mediation_and_reconciliation() {
        assert!(requires_follow_up(&sample_result(TaskId(2), "MEDIATION CONTRACT:\nConflict Type: DIRECT LINE COLLISION")));
        assert!(requires_follow_up(&sample_result(TaskId(2), "Reconciliation blocked: overlapping outputs detected")));
        let mut out_of_scope = sample_result(TaskId(2), "provider call succeeded");
        out_of_scope.out_of_scope_created_files.push("docs/rogue.md".to_string());
        assert!(requires_follow_up(&out_of_scope));
        assert!(!requires_follow_up(&sample_result(TaskId(2), "provider call failed")));
    }

    #[test]
    fn blocked_tasks_propagate_to_dependents() {
        let mut graph = TaskGraph::default();
        graph.root = TaskId(1);
        graph.add(TaskId(1), "root", "root", vec![], vec![TaskId(2)], None);
        graph.add(TaskId(2), "child", "child", vec![], vec![], None);
        let mut registry = OrchestratorRegistry::new(&graph);
        registry.statuses.insert(TaskId(2), TaskStatus::Blocked(sample_result(TaskId(2), "MEDIATION CONTRACT:")));

        propagate_blocked_dependents(&graph, &mut registry);

        assert!(matches!(registry.statuses.get(&TaskId(1)), Some(TaskStatus::Blocked(_))));
    }

    #[test]
    fn dependency_blocked_tasks_return_to_pending_when_dependencies_clear() {
        let mut graph = TaskGraph::default();
        graph.root = TaskId(1);
        graph.add(TaskId(1), "root", "root", vec![], vec![TaskId(2)], None);
        graph.add(TaskId(2), "child", "child", vec![], vec![], None);
        let mut registry = OrchestratorRegistry::new(&graph);
        registry.statuses.insert(TaskId(2), TaskStatus::Blocked(sample_result(TaskId(2), "MEDIATION CONTRACT:")));

        propagate_blocked_dependents(&graph, &mut registry);
        assert!(matches!(registry.statuses.get(&TaskId(1)), Some(TaskStatus::Blocked(_))));

        registry.statuses.insert(TaskId(2), TaskStatus::Done(WorkerResult::new(graph.tasks.get(&TaskId(2)).unwrap())));
        propagate_blocked_dependents(&graph, &mut registry);

        assert!(matches!(registry.statuses.get(&TaskId(1)), Some(TaskStatus::Pending)));
    }

    #[test]
    fn stale_routed_plan_blocks_dispatch() {
        let temp = tempfile::tempdir().unwrap();
        let workspace_root = temp.path();
        let site_map_dir = workspace_root.join(".velocity").join("site_map");
        std::fs::create_dir_all(&site_map_dir).unwrap();
        let mut site_map = velocity_ide::site_map::SiteMap::open(&site_map_dir, 0).unwrap();
        site_map
            .put_node(&velocity_ide::site_map::NdaNode::Triple {
                subject_hash: 1,
                predicate_id: 2,
                object_hash: 2,
            })
            .unwrap();
        let current_root = site_map.root();

        let mut panel = OrchestratorPanel::new();
        panel.set_routed_tasks(
            "goal".to_string(),
            AgentTaskKind::Refactor,
            1,
            vec![RoutedSubAgentTask {
                task_id: "task-1".to_string(),
                task_kind: AgentTaskKind::Refactor,
                planned_site_map_root: current_root.wrapping_add(1),
                files: vec![std::path::PathBuf::from("src/main.rs")],
                provider: crate::agent::AiProvider::CloudflareWorkersAi,
                model_id: "model".to_string(),
                model_label: "model".to_string(),
                thinking: false,
                fallback_chain: Vec::new(),
                instruction_template_id: "template".to_string(),
                decomposition_policy_id: "policy".to_string(),
                decomposition_style: crate::automation::DecompositionStyle::CoupledComponents,
                execution_contract: String::new(),
                summary: String::new(),
                rationale: String::new(),
            }],
        );

        let mediator = std::sync::Arc::new(crate::automation::mediator::MediatorArena::new());
        panel.poll_live_workers(workspace_root, &mediator);

        let registry = panel.registry.as_ref().unwrap();
        assert!(matches!(registry.statuses.get(&TaskId(2)), Some(TaskStatus::Blocked(_))));
        assert!(panel.running_workers.is_empty());
    }

    #[test]
    fn reconcile_root_completes_after_successful_children() {
        let graph = build_routed_graph("goal", &[]);
        let mut registry = OrchestratorRegistry::new(&graph);
        registry.statuses.insert(TaskId(1), TaskStatus::Pending);
        complete_reconcile_root(&graph, &mut registry);
        assert!(matches!(registry.statuses.get(&TaskId(1)), Some(TaskStatus::Pending)));

        let graph = build_routed_graph(
            "goal",
            &[RoutedSubAgentTask {
                task_id: "task-1".to_string(),
                task_kind: AgentTaskKind::Refactor,
                planned_site_map_root: 0,
                files: Vec::new(),
                provider: crate::agent::AiProvider::CloudflareWorkersAi,
                model_id: "model".to_string(),
                model_label: "model".to_string(),
                thinking: false,
                fallback_chain: Vec::new(),
                instruction_template_id: "template".to_string(),
                decomposition_policy_id: "policy".to_string(),
                decomposition_style: crate::automation::DecompositionStyle::CoupledComponents,
                execution_contract: String::new(),
                summary: String::new(),
                rationale: String::new(),
            }],
        );
        let mut registry = OrchestratorRegistry::new(&graph);
        registry.statuses.insert(TaskId(2), TaskStatus::Done(WorkerResult::new(graph.tasks.get(&TaskId(2)).unwrap())));

        complete_reconcile_root(&graph, &mut registry);

        assert!(matches!(registry.statuses.get(&TaskId(1)), Some(TaskStatus::Done(_))));
    }
}

fn requires_follow_up(result: &WorkerResult) -> bool {
    !result.out_of_scope_created_files.is_empty()
        || result.message.contains("MEDIATION CONTRACT:")
        || result.message.contains("Reconciliation blocked:")
        || result
            .status_updates
            .iter()
            .any(|status| status.contains("MEDIATION CONTRACT:") || status.contains("Reconciliation blocked:"))
}

fn is_dependency_blocked_message(message: &str) -> bool {
    message.starts_with("Follow-up required before this task can run because dependency task(s)")
}

fn propagate_blocked_dependents(graph: &TaskGraph, registry: &mut OrchestratorRegistry) {
    loop {
        let mut changed = false;
        for task in graph.tasks.values() {
            let current_status = registry.statuses.get(&task.id).cloned().unwrap_or_default();
            if matches!(current_status, TaskStatus::Done(_) | TaskStatus::Running) {
                continue;
            }

            let dependency_blocked = matches!(
                &current_status,
                TaskStatus::Blocked(result) if is_dependency_blocked_message(&result.message)
            );
            if !matches!(current_status, TaskStatus::Pending | TaskStatus::Blocked(_)) || (!dependency_blocked && matches!(current_status, TaskStatus::Blocked(_))) {
                continue;
            }

            let blocking_dependencies = task
                .dependencies
                .iter()
                .filter(|dependency| {
                    matches!(
                        registry.statuses.get(dependency),
                        Some(TaskStatus::Failed(_)) | Some(TaskStatus::Blocked(_))
                    )
                })
                .copied()
                .collect::<Vec<_>>();

            if blocking_dependencies.is_empty() {
                if dependency_blocked {
                    registry.statuses.insert(task.id, TaskStatus::Pending);
                    changed = true;
                }
                continue;
            }

            let message = format!(
                "Follow-up required before this task can run because dependency task(s) {} did not complete cleanly.",
                blocking_dependencies
                    .iter()
                    .map(|dependency| dependency.0.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );

            let needs_update = match &current_status {
                TaskStatus::Pending => true,
                TaskStatus::Blocked(result) if is_dependency_blocked_message(&result.message) => result.message != message,
                _ => false,
            };

            if !needs_update {
                continue;
            }

            let mut result = WorkerResult::new(task);
            result.success = false;
            result.message = message;
            result.status_updates.push(result.message.clone());
            registry.statuses.insert(task.id, TaskStatus::Blocked(result));
            changed = true;
        }
        if !changed {
            break;
        }
    }
}

fn complete_reconcile_root(graph: &TaskGraph, registry: &mut OrchestratorRegistry) {
    let Some(root_task) = graph.tasks.get(&graph.root) else {
        return;
    };
    if !matches!(registry.statuses.get(&graph.root), Some(TaskStatus::Pending) | None) {
        return;
    }
    if root_task.dependencies.is_empty() {
        return;
    }
    if !root_task
        .dependencies
        .iter()
        .all(|dependency| matches!(registry.statuses.get(dependency), Some(TaskStatus::Done(_))))
    {
        return;
    }

    let mut result = WorkerResult::new(root_task);
    result.message = format!(
        "Reconciliation complete across {} routed task(s).",
        root_task.dependencies.len()
    );
    result.status_updates.push(result.message.clone());
    registry.statuses.insert(graph.root, TaskStatus::Done(result));
}

fn build_routed_graph(goal: &str, tasks: &[RoutedSubAgentTask]) -> TaskGraph {
    let mut graph = TaskGraph::default();
    graph.root = TaskId(1);
    graph.add(
        TaskId(1),
        "Reconcile routed plan",
        format!("Reconcile sub-agent outputs for goal: {goal}"),
        vec![".velocity/agentic".to_string()],
        vec![],
        None,
    );

    for (idx, task) in tasks.iter().enumerate() {
        let scope = task
            .files
            .iter()
            .map(|file| file.display().to_string())
            .collect::<Vec<_>>();
        graph.add(
            TaskId(idx as u64 + 2),
            format!("{} {}", task.task_kind.as_str(), idx + 1),
            format!("{}\n{}", task.summary, task.rationale),
            scope,
            vec![],
            Some(TaskId(1)),
        );
    }

    graph
}
