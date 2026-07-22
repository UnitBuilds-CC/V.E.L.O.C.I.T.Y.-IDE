use super::struct_def::OrchestratorPanel;
use crate::editor::theme::IdePalette;
use crate::orchestrator::blueprint::{Task, TaskGraph};
use crate::orchestrator::registry::{OrchestratorRegistry, TaskStatus};
use crate::orchestrator::scheduler;
use crate::orchestrator::TaskId;
use eframe::egui;
use egui::{ScrollArea, Ui};
use std::collections::HashMap;
use std::path::Path;

impl OrchestratorPanel {
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
        if !canvas_size.x.is_finite() || canvas_size.x < 300.0 {
            canvas_size.x = 450.0;
        }
        if !canvas_size.y.is_finite() || canvas_size.y < 300.0 {
            canvas_size.y = 480.0;
        }
        canvas_size.y = canvas_size.y.max(450.0);

        let (rect, _response) = ui.allocate_exact_size(canvas_size, egui::Sense::hover());
        let painter = ui.painter_at(rect);

        painter.rect_filled(rect, 4.0, palette.bg_primary);

        let mut node_positions = HashMap::new();

        if !has_cycle && !plan.phases.is_empty() {
            let _max_tasks_in_phase = plan.phases.iter().map(|p| p.len()).max().unwrap_or(1);
            let num_phases = plan.phases.len();

            let usable_w = (rect.width() - 140.0).max(100.0);
            let x_spacing = if num_phases > 1 {
                usable_w / (num_phases - 1) as f32
            } else {
                0.0
            };

            let usable_h = (rect.height() - 80.0).max(100.0);
            let start_x = rect.min.x + 70.0;

            for (phase_idx, phase) in plan.phases.iter().enumerate() {
                let x = if num_phases == 1 {
                    rect.center().x
                } else {
                    start_x + phase_idx as f32 * x_spacing
                };

                let n_tasks = phase.len();
                let y_spacing = if n_tasks > 1 {
                    usable_h / (n_tasks - 1) as f32
                } else {
                    0.0
                };
                let start_y = if n_tasks == 1 {
                    rect.center().y
                } else {
                    rect.min.y + 40.0
                };

                for (task_idx, &id) in phase.iter().enumerate() {
                    let y = if n_tasks == 1 {
                        start_y
                    } else {
                        start_y + task_idx as f32 * y_spacing
                    };
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
