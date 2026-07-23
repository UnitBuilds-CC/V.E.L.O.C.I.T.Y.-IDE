use super::struct_def::OrchestratorPanel;
use crate::editor::expert_team::ExpertTeam;
use crate::editor::theme::IdePalette;
use crate::orchestrator::blueprint::{Task, TaskGraph};
use crate::orchestrator::registry::{OrchestratorRegistry, TaskStatus};
use crate::orchestrator::scheduler;
use crate::orchestrator::TaskId;
use eframe::egui;
use egui::{RichText, ScrollArea, Stroke, Ui, Vec2};
use std::collections::HashMap;
use std::path::Path;

impl OrchestratorPanel {
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        workspace_root: &Path,
        mediator: &std::sync::Arc<crate::automation::mediator::MediatorArena>,
        expert_teams: &mut [ExpertTeam],
        active_team_index: &mut usize,
        palette: IdePalette,
    ) {
        self.ensure_policy_editor_loaded(workspace_root);
        if self.execution_running {
            self.poll_live_workers(workspace_root, mediator);
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }

        ScrollArea::vertical()
            .id_salt("orchestrator_panel_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(6.0);

                // Header with status and primary actions
                ui.horizontal(|ui| {
                    ui.heading(RichText::new("Orchestrator").color(palette.accent));
                    ui.label(RichText::new(&self.runtime_status).small().color(palette.text_muted));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Policy").clicked() {
                            self.show_policy_editor = !self.show_policy_editor;
                        }
                        ui.toggle_value(&mut self.expanded, "Graph");
                    });
                });

                ui.add_space(6.0);

                // Compact stats row
                let has_cycle = scheduler::detect_cycle(&self.graph);
                let plan = if has_cycle { scheduler::Plan::default() } else { scheduler::plan(&self.graph) };
                let completed_count = self.registry.as_ref().map(|r| r.statuses.values().filter(|s| matches!(s, TaskStatus::Done(_))).count()).unwrap_or(0);
                let retryable_blocked = self.retryable_blocked_task_count();

                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("Tasks: {}", self.graph.tasks.len())).small());
                    ui.separator();
                    ui.label(RichText::new(format!("Phases: {}", plan.phases.len())).small());
                    ui.separator();
                    ui.label(RichText::new(format!("Done: {}", completed_count)).small().color(palette.success));
                    if retryable_blocked > 0 {
                        ui.separator();
                        ui.label(RichText::new(format!("Blocked: {}", retryable_blocked)).small().color(palette.warning));
                    }
                    if !self.running_workers.is_empty() {
                        ui.separator();
                        ui.label(RichText::new(format!("Workers: {}", self.running_workers.len())).small().color(palette.accent));
                    }
                });

                ui.add_space(6.0);

                // Action buttons
                ui.horizontal_wrapped(|ui| {
                    if has_cycle {
                        if ui.button(RichText::new("Fix Cycle").color(palette.warning)).clicked() {
                            self.graph = TaskGraph::example_game();
                            self.registry = Some(OrchestratorRegistry::new(&self.graph));
                            self.execution_running = false;
                            self.running_workers.clear();
                            self.runtime_status = "Graph repaired".to_string();
                        }
                    } else if self.execution_running {
                        ui.add_enabled_ui(false, |ui| { let _ = ui.button("Executing..."); });
                    } else if ui.button(RichText::new("Execute").color(palette.success)).clicked() {
                        self.execute_routed_tasks(workspace_root, mediator);
                    }

                    if ui.add_enabled(!self.execution_running && retryable_blocked > 0, egui::Button::new(format!("Retry Blocked ({})", retryable_blocked))).clicked() {
                        self.retry_blocked_tasks_action(workspace_root, mediator);
                    }

                    if ui.button("Reset").clicked() {
                        self.reset_runtime_action();
                    }
                });

                // Policy editor (collapsible)
                if self.show_policy_editor {
                    ui.add_space(6.0);
                    self.render_policy_controls(ui, workspace_root, palette);
                }

                // Cycle warning
                if has_cycle {
                    ui.add_space(6.0);
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Dependency cycle detected").color(palette.error));
                            if ui.small_button("Auto-repair").clicked() {
                                self.graph = TaskGraph::example_game();
                                self.registry = Some(OrchestratorRegistry::new(&self.graph));
                                self.runtime_status = "Graph repaired".to_string();
                            }
                        });
                    });
                }

                ui.add_space(8.0);

                // Routed plan info
                if let Some(route_plan) = &self.routed_plan {
                    ui.group(|ui| {
                        ui.label(RichText::new(format!("Goal: {}", route_plan.goal)).small().strong());
                        ui.label(RichText::new(format!("{} tasks, {} scoped files", route_plan.tasks.len(), route_plan.scope_count)).small().color(palette.text_muted));
                    });
                    ui.add_space(6.0);
                }

                // Task list
                let active_team = if !expert_teams.is_empty() {
                    let idx = (*active_team_index).min(expert_teams.len() - 1);
                    Some(&expert_teams[idx])
                } else {
                    None
                };

                if !has_cycle && !plan.phases.is_empty() {
                    for (phase_idx, phase) in plan.phases.iter().enumerate() {
                        ui.group(|ui| {
                            ui.label(RichText::new(format!("Phase {}", phase_idx + 1)).small().strong().color(palette.accent));
                            for id in phase {
                                if let Some(task) = self.graph.tasks.get(id) {
                                    self.render_task_card(ui, task, active_team, palette);
                                }
                            }
                        });
                        ui.add_space(4.0);
                    }
                } else {
                    ui.group(|ui| {
                        ui.label(RichText::new("Tasks").small().strong());
                        for task in self.graph.tasks.values() {
                            self.render_task_card(ui, task, active_team, palette);
                        }
                    });
                }

                // Graph visualization (expandable)
                if self.expanded {
                    ui.add_space(8.0);
                    ui.separator();
                    ui.label(RichText::new("Task Graph").small().strong());
                    self.draw_task_graph(ui, &plan, has_cycle, palette);
                }
            });
    }

    fn render_task_card(&self, ui: &mut Ui, task: &Task, active_team: Option<&ExpertTeam>, palette: IdePalette) {
        let status = self.registry.as_ref()
            .and_then(|r| r.statuses.get(&task.id))
            .cloned()
            .unwrap_or(TaskStatus::Pending);

        let (status_text, status_color) = match &status {
            TaskStatus::Pending => ("Pending", palette.text_muted),
            TaskStatus::Running => ("Running", palette.accent),
            TaskStatus::Done(_) => ("Done", palette.success),
            TaskStatus::Failed(_) => ("Failed", palette.error),
            TaskStatus::Blocked(_) => ("Blocked", palette.warning),
        };

        let assigned_expert = active_team.and_then(|team| team.find_expert_for_task(&task.title, &task.scope));

        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("#{}", task.id.0)).monospace().small().color(palette.text_muted));
            ui.label(RichText::new(&task.title).small().strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(status_text).small().color(status_color));
            });
        });

        if let Some(expert) = assigned_expert {
            ui.label(RichText::new(format!("  Agent: {}", expert.name)).small().color(palette.text_muted));
        }

        if !task.scope.is_empty() {
            ui.label(RichText::new(format!("  Scope: {}", task.scope.join(", "))).small().color(palette.text_muted));
        }
    }

    pub fn draw_task_graph(
        &self,
        ui: &mut Ui,
        plan: &scheduler::Plan,
        has_cycle: bool,
        palette: IdePalette,
    ) {
        let node_w = 140.0;
        let node_h = 44.0;
        let col_spacing = 175.0;
        let row_spacing = 60.0;

        let mut node_positions = HashMap::new();

        let num_phases = if !has_cycle && !plan.phases.is_empty() { plan.phases.len() } else { 1 };
        let max_nodes_in_phase = if !has_cycle && !plan.phases.is_empty() {
            plan.phases.iter().map(|p| p.len()).max().unwrap_or(1)
        } else {
            self.graph.tasks.len().max(1)
        };

        let required_w = (num_phases as f32 * col_spacing + 60.0).max(400.0);
        let required_h = (max_nodes_in_phase as f32 * row_spacing + 60.0).max(300.0);

        ScrollArea::both()
            .id_salt("topology_canvas_scroll")
            .max_height(360.0)
            .show(ui, |ui| {
                let (rect, _response) = ui.allocate_exact_size(Vec2::new(required_w, required_h), egui::Sense::hover());
                let painter = ui.painter_at(rect);

                painter.rect_filled(rect, 4.0, palette.bg_secondary);

                if !has_cycle && !plan.phases.is_empty() {
                    let start_x = rect.min.x + 40.0;
                    for (phase_idx, phase) in plan.phases.iter().enumerate() {
                        let x = start_x + phase_idx as f32 * col_spacing + node_w / 2.0;
                        let n_tasks = phase.len();
                        let phase_height = n_tasks as f32 * row_spacing;
                        let start_y = rect.center().y - (phase_height / 2.0) + (row_spacing / 2.0);
                        for (task_idx, &id) in phase.iter().enumerate() {
                            let y = start_y + task_idx as f32 * row_spacing;
                            node_positions.insert(id, egui::pos2(x, y));
                        }
                    }
                } else {
                    let center = rect.center();
                    let radius = (rect.width().min(rect.height()) * 0.35).max(60.0);
                    let tasks_vec: Vec<TaskId> = self.graph.tasks.keys().cloned().collect();
                    let count = tasks_vec.len();
                    for (idx, &id) in tasks_vec.iter().enumerate() {
                        let angle = (idx as f32 / count as f32) * 2.0 * std::f32::consts::PI;
                        let x = center.x + radius * angle.cos();
                        let y = center.y + radius * angle.sin();
                        node_positions.insert(id, egui::pos2(x, y));
                    }
                }

                // Draw connections
                for (&id, task) in &self.graph.tasks {
                    if let Some(&p_to) = node_positions.get(&id) {
                        for dep_id in &task.dependencies {
                            if let Some(&p_from) = node_positions.get(dep_id) {
                                painter.line_segment([p_from, p_to], Stroke::new(1.5, palette.border));
                            }
                        }
                    }
                }

                // Draw nodes
                for (&id, task) in &self.graph.tasks {
                    if let Some(&pos) = node_positions.get(&id) {
                        let status = self.registry.as_ref()
                            .and_then(|r| r.statuses.get(&id))
                            .cloned()
                            .unwrap_or(TaskStatus::Pending);

                        let color = match &status {
                            TaskStatus::Pending => palette.text_muted,
                            TaskStatus::Running => palette.accent,
                            TaskStatus::Done(_) => palette.success,
                            TaskStatus::Failed(_) => palette.error,
                            TaskStatus::Blocked(_) => palette.warning,
                        };

                        let node_rect = egui::Rect::from_center_size(pos, Vec2::new(node_w, node_h));
                        painter.rect_filled(node_rect, 4.0, palette.bg_primary);
                        painter.rect_stroke(node_rect, 4.0, Stroke::new(1.0, color), egui::StrokeKind::Inside);

                        let truncated_title: String = task.title.chars().take(14).collect();
                        painter.text(
                            pos,
                            egui::Align2::CENTER_CENTER,
                            format!("#{} {}", id.0, truncated_title),
                            egui::FontId::proportional(11.0),
                            palette.text,
                        );
                    }
                }
            });
    }
}
