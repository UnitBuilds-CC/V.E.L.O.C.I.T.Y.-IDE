use super::struct_def::OrchestratorPanel;
use crate::editor::theme::IdePalette;
use crate::orchestrator::blueprint::{Task, TaskGraph};
use crate::orchestrator::registry::{OrchestratorRegistry, TaskStatus};
use crate::orchestrator::scheduler;
use crate::orchestrator::TaskId;
use eframe::egui;
use egui::{Color32, RichText, ScrollArea, Stroke, Ui, Vec2};
use std::collections::HashMap;
use std::path::Path;

impl OrchestratorPanel {
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        workspace_root: &Path,
        mediator: &std::sync::Arc<crate::automation::mediator::MediatorArena>,
        palette: IdePalette,
    ) {
        self.ensure_policy_editor_loaded(workspace_root);
        if self.execution_running {
            self.poll_live_workers(workspace_root, mediator);
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }

        ScrollArea::vertical()
            .id_salt("orchestrator_panel_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(6.0);

                // --- 1. SLEEK TOP CONTROL HUB ---
                egui::Frame::group(ui.style())
                    .fill(Color32::from_rgb(22, 25, 33))
                    .inner_margin(12.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.heading(RichText::new("🚀 Swarm Task Flow Pipeline").strong().color(Color32::from_rgb(130, 180, 255)));
                            ui.label(RichText::new("|").color(Color32::DARK_GRAY));
                            ui.label(RichText::new(&self.runtime_status).small().color(Color32::LIGHT_BLUE));

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let label = if self.expanded { "− Minimal View" } else { "+ Expanded Inspector" };
                                ui.toggle_value(&mut self.expanded, label);

                                if ui.button(RichText::new("⚙️ Policy & Router").small()).clicked() {
                                    self.show_policy_editor = !self.show_policy_editor;
                                }
                            });
                        });

                        ui.add_space(8.0);

                        // Stats & Primary Action Toolbar
                        ui.horizontal_wrapped(|ui| {
                            let has_cycle = scheduler::detect_cycle(&self.graph);
                            let plan = if has_cycle { scheduler::Plan::default() } else { scheduler::plan(&self.graph) };
                            let completed_count = self.registry.as_ref().map(|r| r.statuses.values().filter(|s| matches!(s, TaskStatus::Done(_))).count()).unwrap_or(0);
                            let retryable_blocked = self.retryable_blocked_task_count();

                            // Stat Pills
                            for (label, val, color) in [
                                ("Tasks", format!("{}", self.graph.tasks.len()), Color32::WHITE),
                                ("Phases", format!("{}", plan.phases.len()), Color32::from_rgb(100, 200, 255)),
                                ("Workers", format!("{}", self.running_workers.len()), Color32::from_rgb(180, 150, 255)),
                                ("Done", format!("{completed_count}"), Color32::GREEN),
                                ("Blocked", format!("{retryable_blocked}"), if retryable_blocked > 0 { Color32::LIGHT_RED } else { Color32::GRAY }),
                            ] {
                                egui::Frame::group(ui.style())
                                    .fill(Color32::from_rgb(32, 36, 48))
                                    .inner_margin(egui::Margin::symmetric(10, 4))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new(format!("{label}: {val}")).small().strong().color(color));
                                    });
                            }

                            ui.add_space(10.0);
                            ui.separator();
                            ui.add_space(10.0);

                            // Action Buttons
                            if has_cycle {
                                if ui.button(RichText::new("⚡ Auto-Fix Dependency Cycle").strong().color(Color32::GOLD)).clicked() {
                                    self.graph = TaskGraph::example_game();
                                    self.registry = Some(OrchestratorRegistry::new(&self.graph));
                                    self.execution_running = false;
                                    self.running_workers.clear();
                                    self.runtime_status = "Dependency graph auto-repaired and acyclic.".to_string();
                                }
                            } else if self.execution_running {
                                ui.add_enabled_ui(false, |ui| {
                                    let _ = ui.button(RichText::new("⏳ Executing Swarm...").strong());
                                });
                            } else if ui.button(RichText::new("▶️ Execute Swarm Plan").strong().color(Color32::from_rgb(100, 220, 150))).clicked() {
                                self.execute_routed_tasks(workspace_root, mediator);
                            }

                            if ui.add_enabled(!self.execution_running && retryable_blocked > 0, egui::Button::new(format!("↻ Retry Blocked ({retryable_blocked})"))).clicked() {
                                self.retry_blocked_tasks_action(workspace_root, mediator);
                            }

                            if ui.button("🧹 Reset Runtime").clicked() {
                                self.reset_runtime_action();
                            }

                            if ui.button("🔄 Re-Plan Graph").clicked() {
                                self.graph = TaskGraph::example_game();
                                self.registry = Some(OrchestratorRegistry::new(&self.graph));
                                self.execution_running = false;
                                self.running_workers.clear();
                                self.runtime_status = "Graph re-planned".to_string();
                            }
                        });
                    });

                // Render Policy Controls when toggled
                if self.show_policy_editor {
                    ui.add_space(6.0);
                    self.render_policy_controls(ui, workspace_root, palette);
                }

                // Cycle Warning banner if present
                let has_cycle = scheduler::detect_cycle(&self.graph);
                if has_cycle {
                    ui.add_space(6.0);
                    egui::Frame::group(ui.style())
                        .fill(Color32::from_rgb(50, 20, 25))
                        .stroke(Stroke::new(1.0, Color32::RED))
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("⚠️ Dependency Loop Detected!").strong().color(Color32::RED));
                                ui.label("Topological phase sorting paused until the cycle is resolved.");
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button(RichText::new("⚡ Auto-Repair Graph").strong()).clicked() {
                                        self.graph = TaskGraph::example_game();
                                        self.registry = Some(OrchestratorRegistry::new(&self.graph));
                                        self.runtime_status = "Graph auto-repaired.".to_string();
                                    }
                                });
                            });
                        });
                }

                ui.add_space(8.0);

                let plan = if has_cycle { scheduler::Plan::default() } else { scheduler::plan(&self.graph) };

                // --- 2. MAIN SPLIT: PHASE SWIMLANES (LEFT) vs VISUAL NODE GRAPH (RIGHT) ---
                ui.columns(2, |cols| {
                    // COLUMN 1: Topological Execution Phases & Task Cards
                    cols[0].vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.heading(RichText::new("📋 Execution Phases & Task Cards").small().strong());
                            ui.label(RichText::new(format!("({} Phases)", plan.phases.len())).small().color(Color32::GRAY));
                        });
                        ui.separator();

                        if let Some(route_plan) = &self.routed_plan {
                            egui::Frame::group(ui.style())
                                .fill(Color32::from_rgb(25, 30, 42))
                                .inner_margin(8.0)
                                .show(ui, |ui| {
                                    ui.label(RichText::new("🧭 Active Routed Mission Plan").strong().color(Color32::LIGHT_BLUE));
                                    ui.label(RichText::new(format!("Goal: {}", route_plan.goal)).small());
                                    ui.label(RichText::new(format!("Kind: {} | Scoped Files: {} | Agents: {}", route_plan.kind.as_str(), route_plan.scope_count, route_plan.tasks.len())).small().color(Color32::GRAY));
                                });
                            ui.add_space(6.0);
                        }

                        ScrollArea::vertical().id_salt("phases_task_cards_scroll").max_height(520.0).show(ui, |ui| {
                            if !has_cycle && !plan.phases.is_empty() {
                                for (phase_idx, phase) in plan.phases.iter().enumerate() {
                                    egui::Frame::group(ui.style())
                                        .fill(Color32::from_rgb(20, 24, 32))
                                        .stroke(Stroke::new(1.0, Color32::from_rgb(45, 55, 75)))
                                        .inner_margin(8.0)
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.label(RichText::new(format!("📍 Phase {} ({})", phase_idx + 1, match phase_idx {
                                                    0 => "Foundation & Architecture",
                                                    1 => "Core Subsystems",
                                                    2 => "Integration & Features",
                                                    _ => "Verification & Testing",
                                                })).strong().color(Color32::from_rgb(140, 190, 255)));
                                                ui.label(RichText::new(format!("{} Task(s)", phase.len())).small().color(Color32::GRAY));
                                            });

                                            ui.add_space(4.0);

                                            for id in phase {
                                                if let Some(task) = self.graph.tasks.get(id) {
                                                    self.render_task_card(ui, task, palette);
                                                    ui.add_space(4.0);
                                                }
                                            }
                                        });
                                    ui.add_space(6.0);
                                }
                            } else {
                                ui.group(|ui| {
                                    ui.label(RichText::new("Raw Tasks List (Unscheduled)").strong());
                                    for task in self.graph.tasks.values() {
                                        self.render_task_card(ui, task, palette);
                                        ui.add_space(4.0);
                                    }
                                });
                            }
                        });
                    });

                    // COLUMN 2: Visual Interactive Node Canvas
                    cols[1].vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.heading(RichText::new("🕸️ Interactive Task Flow Topology").small().strong());
                        });
                        ui.separator();

                        self.draw_task_graph(ui, &plan, has_cycle, palette);
                    });
                });
            });
    }

    /// Renders a modern glassmorphic Task Card with status badges, model tags, and execution controls.
    fn render_task_card(&self, ui: &mut Ui, task: &Task, palette: IdePalette) {
        let status = self
            .registry
            .as_ref()
            .and_then(|r| r.statuses.get(&task.id))
            .cloned()
            .unwrap_or(TaskStatus::Pending);

        let (status_text, status_color) = match &status {
            TaskStatus::Pending => ("🟡 Pending", Color32::from_rgb(220, 180, 80)),
            TaskStatus::Running => ("🔵 Executing", Color32::from_rgb(100, 180, 255)),
            TaskStatus::Done(_) => ("🟢 Completed", Color32::from_rgb(100, 220, 120)),
            TaskStatus::Failed(_) => ("🔴 Failed", Color32::from_rgb(240, 90, 90)),
            TaskStatus::Blocked(_) => ("⚠️ Blocked", Color32::from_rgb(240, 160, 60)),
        };

        egui::Frame::group(ui.style())
            .fill(Color32::from_rgb(28, 32, 42))
            .stroke(Stroke::new(1.0, Color32::from_rgb(45, 52, 68)))
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("#{}", task.id.0)).monospace().strong().color(Color32::GRAY));
                    ui.label(RichText::new(&task.title).strong().color(Color32::WHITE));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(status_text).small().strong().color(status_color));
                    });
                });

                ui.label(RichText::new(&task.description).small().color(Color32::LIGHT_GRAY));

                if !task.scope.is_empty() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Scope:").small().strong().color(Color32::GRAY));
                        for path in &task.scope {
                            ui.label(RichText::new(format!("[{path}]")).small().color(Color32::from_rgb(120, 200, 180)));
                        }
                    });
                }

                if !task.dependencies.is_empty() {
                    let deps_str = task.dependencies.iter().map(|d| format!("#{}", d.0)).collect::<Vec<_>>().join(", ");
                    ui.label(RichText::new(format!("Prerequisites: {deps_str}")).small().italics().color(Color32::GRAY));
                }
            });
    }

    pub fn draw_task_graph(
        &self,
        ui: &mut Ui,
        plan: &scheduler::Plan,
        has_cycle: bool,
        palette: IdePalette,
    ) {
        let mut canvas_size = ui.available_size();
        if !canvas_size.x.is_finite() || canvas_size.x < 300.0 {
            canvas_size.x = 450.0;
        }
        if !canvas_size.y.is_finite() || canvas_size.y < 300.0 {
            canvas_size.y = 480.0;
        }
        canvas_size.y = canvas_size.y.max(480.0);

        let (rect, _response) = ui.allocate_exact_size(canvas_size, egui::Sense::hover());
        let painter = ui.painter_at(rect);

        // Dark Canvas background
        painter.rect_filled(rect, 6.0, Color32::from_rgb(16, 18, 24));

        let mut node_positions = HashMap::new();

        if !has_cycle && !plan.phases.is_empty() {
            let num_phases = plan.phases.len();
            let usable_w = (rect.width() - 140.0).max(100.0);
            let x_spacing = if num_phases > 1 { usable_w / (num_phases - 1) as f32 } else { 0.0 };
            let usable_h = (rect.height() - 80.0).max(100.0);
            let start_x = rect.min.x + 70.0;

            for (phase_idx, phase) in plan.phases.iter().enumerate() {
                let x = if num_phases == 1 { rect.center().x } else { start_x + phase_idx as f32 * x_spacing };
                let n_tasks = phase.len();
                let y_spacing = if n_tasks > 1 { usable_h / (n_tasks - 1) as f32 } else { 0.0 };
                let start_y = if n_tasks == 1 { rect.center().y } else { rect.min.y + 40.0 };

                for (task_idx, &id) in phase.iter().enumerate() {
                    let y = if n_tasks == 1 { start_y } else { start_y + task_idx as f32 * y_spacing };
                    node_positions.insert(id, egui::pos2(x, y));
                }
            }
        } else {
            let center = rect.center();
            let radius = (rect.width().min(rect.height()) * 0.35).max(70.0);
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
                        painter.line_segment([p_from, p_to], Stroke::new(2.0, Color32::from_rgb(60, 90, 140)));
                    }
                }
            }
        }

        // Draw nodes
        for (&id, task) in &self.graph.tasks {
            if let Some(&pos) = node_positions.get(&id) {
                let status = self
                    .registry
                    .as_ref()
                    .and_then(|r| r.statuses.get(&id))
                    .cloned()
                    .unwrap_or(TaskStatus::Pending);

                let (color, status_str) = match &status {
                    TaskStatus::Pending => (Color32::from_rgb(200, 160, 60), "Pending"),
                    TaskStatus::Running => (Color32::from_rgb(80, 160, 240), "Running"),
                    TaskStatus::Done(_) => (Color32::from_rgb(80, 200, 100), "Done"),
                    TaskStatus::Failed(_) => (Color32::from_rgb(220, 70, 70), "Failed"),
                    TaskStatus::Blocked(_) => (Color32::from_rgb(220, 140, 50), "Blocked"),
                };

                let size = Vec2::new(135.0, 48.0);
                let node_rect = egui::Rect::from_center_size(pos, size);
                painter.rect_filled(node_rect, 6.0, Color32::from_rgb(24, 28, 38));

                let truncated_title: String = task.title.chars().take(16).collect();
                painter.text(
                    pos,
                    egui::Align2::CENTER_CENTER,
                    format!("#{} {}\n{}", id.0, truncated_title, status_str),
                    egui::FontId::proportional(11.0),
                    Color32::WHITE,
                );
            }
        }
    }
}
