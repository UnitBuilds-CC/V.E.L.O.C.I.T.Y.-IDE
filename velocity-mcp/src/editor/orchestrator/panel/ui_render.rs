use super::authoring::TaskDraft;
use super::struct_def::OrchestratorPanel;
use crate::automation::AgentTaskKind;
use crate::editor::expert_team::ExpertTeam;
use crate::editor::theme::IdePalette;
use crate::orchestrator::blueprint::Task;
use crate::orchestrator::registry::TaskStatus;
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
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }

        // Tasks the user deletes this pass are applied once drawing finishes, so
        // no card is ever rendered from a graph that changed mid-iteration.
        let mut removals: Vec<TaskId> = Vec::new();

        ScrollArea::vertical()
            .id_salt("orchestrator_panel_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(6.0);

                // Header with status and primary actions
                ui.horizontal(|ui| {
                    ui.heading(RichText::new("Orchestrator").color(palette.accent));
                    ui.label(
                        RichText::new(&self.runtime_status)
                            .small()
                            .color(palette.text_muted),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Policy").clicked() {
                            self.show_policy_editor = !self.show_policy_editor;
                        }
                        ui.toggle_value(&mut self.expanded, "Show task graph");
                    });
                });

                ui.add_space(6.0);

                // Compact stats row
                let has_cycle = scheduler::detect_cycle(&self.graph);
                let plan = if has_cycle {
                    scheduler::Plan::default()
                } else {
                    scheduler::plan(&self.graph)
                };
                let bfs_order = if has_cycle {
                    Vec::new()
                } else {
                    scheduler::bfs(&self.graph)
                };
                let completed_count = self
                    .registry
                    .as_ref()
                    .map(|r| {
                        r.statuses
                            .values()
                            .filter(|s| matches!(s, TaskStatus::Done(_)))
                            .count()
                    })
                    .unwrap_or(0);
                let retryable_blocked = self.retryable_blocked_task_count();

                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("Tasks: {}", self.graph.tasks.len())).small());
                    ui.label(
                        RichText::new("\u{00b7}")
                            .small()
                            .color(palette.text_muted.gamma_multiply(0.6)),
                    );
                    ui.label(RichText::new(format!("Phases: {}", plan.phases.len())).small());
                    ui.label(
                        RichText::new("\u{00b7}")
                            .small()
                            .color(palette.text_muted.gamma_multiply(0.6)),
                    );
                    ui.label(RichText::new(format!("BFS order: {}", bfs_order.len())).small());
                    ui.label(
                        RichText::new("\u{00b7}")
                            .small()
                            .color(palette.text_muted.gamma_multiply(0.6)),
                    );
                    ui.label(
                        RichText::new(format!("Done: {}", completed_count))
                            .small()
                            .color(palette.success),
                    );
                    if retryable_blocked > 0 {
                        ui.label(
                            RichText::new("\u{00b7}")
                                .small()
                                .color(palette.text_muted.gamma_multiply(0.6)),
                        );
                        ui.label(
                            RichText::new(format!("Blocked: {}", retryable_blocked))
                                .small()
                                .color(palette.warning),
                        );
                    }
                    if !self.running_workers.is_empty() {
                        ui.label(
                            RichText::new("\u{00b7}")
                                .small()
                                .color(palette.text_muted.gamma_multiply(0.6)),
                        );
                        ui.label(
                            RichText::new(format!("Workers: {}", self.running_workers.len()))
                                .small()
                                .color(palette.accent),
                        );
                    }
                });

                ui.add_space(6.0);

                // Action buttons
                let blocker = self.execution_blocker();
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button(RichText::new("+ Add task").strong())
                        .on_hover_text("Write a task by hand and put it in the plan.")
                        .clicked()
                    {
                        self.draft.open = !self.draft.open;
                        self.draft.error.clear();
                    }
                    if ui
                        .button("Route goal")
                        .on_hover_text("Ask the planner to decompose a goal into sub-agent tasks.")
                        .clicked()
                    {
                        self.goal_input_open = !self.goal_input_open;
                    }

                    if has_cycle {
                        if ui
                            .button(RichText::new("Fix Cycle").color(palette.warning))
                            .on_hover_text(
                                "Drop only the dependencies that loop; every task is kept.",
                            )
                            .clicked()
                        {
                            self.repair_cycles_action();
                        }
                    } else if self.execution_running {
                        ui.add_enabled_ui(false, |ui| {
                            let _ = ui.button("Executing...");
                        });
                    } else {
                        ui.add_enabled_ui(blocker.is_none(), |ui| {
                            if ui
                                .button(RichText::new("Execute").color(palette.success))
                                .clicked()
                            {
                                self.execute_routed_tasks(workspace_root, mediator);
                            }
                        });
                    }

                    if ui
                        .add_enabled(
                            !self.execution_running && retryable_blocked > 0,
                            egui::Button::new(format!("Retry Blocked ({})", retryable_blocked)),
                        )
                        .clicked()
                    {
                        self.retry_blocked_tasks_action(workspace_root, mediator);
                    }

                    if ui.button("Reset").clicked() {
                        self.reset_runtime_action();
                    }

                    if !self.plan_is_empty() {
                        ui.add_enabled_ui(!self.execution_running, |ui| {
                            if ui
                                .button("Clear plan")
                                .on_hover_text("Empty the plan. Running work is cancelled.")
                                .clicked()
                            {
                                self.clear_plan();
                            }
                        });
                    }
                });

                // Stated in the open: a greyed-out Execute with no reason reads
                // exactly like a button that is broken.
                if let Some(reason) = blocker {
                    if !self.execution_running && !self.draft.open && !has_cycle {
                        ui.label(RichText::new(reason).small().color(palette.warning));
                    }
                }

                if self.goal_input_open {
                    ui.add_space(6.0);
                    self.render_route_goal(ui, palette);
                }

                if self.draft.open {
                    ui.add_space(6.0);
                    self.render_task_draft(ui, palette);
                }

                // Policy editor (collapsible)
                if self.show_policy_editor {
                    ui.add_space(6.0);
                    self.render_policy_controls(ui, workspace_root, palette);
                }

                // Cycle warning
                if has_cycle {
                    ui.add_space(6.0);
                    ui.group(|ui| {
                        ui.label(
                            RichText::new("Dependency cycle detected").color(palette.error),
                        );
                        ui.label(
                            RichText::new(
                                "The plan cannot be scheduled until the loop is cut.",
                            )
                            .small()
                            .color(palette.text_muted),
                        );
                        if ui.small_button("Cut back-edges").clicked() {
                            self.repair_cycles_action();
                        }
                    });
                } else if !self.repair_report.is_empty() {
                    ui.add_space(6.0);
                    ui.group(|ui| {
                        ui.label(
                            RichText::new(format!("Repaired: {}", self.repair_report))
                                .small()
                                .color(palette.warning),
                        );
                    });
                }

                ui.add_space(8.0);

                // Routed plan info
                if let Some(route_plan) = &self.routed_plan {
                    ui.group(|ui| {
                        ui.label(
                            RichText::new(format!("Goal: {}", route_plan.goal))
                                .small()
                                .strong(),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{} tasks, {} scoped files",
                                route_plan.tasks.len(),
                                route_plan.scope_count
                            ))
                            .small()
                            .color(palette.text_muted),
                        );
                    });
                    ui.add_space(6.0);
                }

                // Keep the operational list and topology visible together. The task
                // list owns its own scroll surface, so a long plan never pushes the graph
                // below the fold.
                ui.add_space(8.0);
                ui.horizontal_top(|ui| {
                    let graph_visible = self.expanded;
                    let list_width = if graph_visible {
                        // Bias the split toward topology: task cards remain readable at
                        // this width while the graph can normally be read without panning.
                        (ui.available_width() * 0.46).clamp(320.0, 520.0)
                    } else {
                        ui.available_width()
                    };
                    ui.allocate_ui_with_layout(
                        Vec2::new(list_width, ui.available_height()),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ScrollArea::vertical()
                                .id_salt("orchestrator_task_list_scroll")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
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
                                                ui.label(
                                                    RichText::new(format!(
                                                        "Phase {}",
                                                        phase_idx + 1
                                                    ))
                                                    .small()
                                                    .strong()
                                                    .color(palette.accent),
                                                );
                                                ui.add_space(2.0);
                                                for id in phase {
                                                    let remove =
                                                        if let Some(task) = self.graph.tasks.get(id)
                                                        {
                                                            self.render_task_card(
                                                                ui,
                                                                task,
                                                                active_team,
                                                                palette,
                                                            )
                                                        } else {
                                                            false
                                                        };
                                                    if remove {
                                                        removals.push(*id);
                                                    }
                                                }
                                            });
                                            ui.add_space(4.0);
                                        }
                                    } else if self.graph.tasks.is_empty() {
                                        ui.add_space(16.0);
                                        ui.vertical_centered(|ui| {
                                            ui.label(
                                                RichText::new("\u{25cb}")
                                                    .size(28.0)
                                                    .color(palette.accent.gamma_multiply(0.7)),
                                            );
                                            ui.add_space(6.0);
                                            ui.label(
                                                RichText::new(
                                                    "No tasks yet \u{2014} pick a model, then \"+ Add task\"\nto write the plan by hand, or \"Route goal\" to\nhave the planner decompose one for you.",
                                                )
                                                .color(palette.text_muted),
                                            );
                                        });
                                    } else {
                                        ui.group(|ui| {
                                            ui.label(RichText::new("Tasks").small().strong());
                                            ui.add_space(2.0);
                                            let ids: Vec<TaskId> =
                                                self.graph.tasks.keys().copied().collect();
                                            for id in ids {
                                                let remove =
                                                    if let Some(task) = self.graph.tasks.get(&id) {
                                                        self.render_task_card(
                                                            ui,
                                                            task,
                                                            active_team,
                                                            palette,
                                                        )
                                                    } else {
                                                        false
                                                    };
                                                if remove {
                                                    removals.push(id);
                                                }
                                            }
                                        });
                                    }
                                });
                        },
                    );

                    if graph_visible {
                        ui.add_space(10.0);
                        egui::Frame::new()
                            .fill(palette.bg_primary)
                            .stroke(Stroke::new(1.0, palette.border))
                            .corner_radius(egui::CornerRadius::same(8))
                            .inner_margin(egui::Margin::same(10))
                            .show(ui, |ui| {
                                ui.label(RichText::new("Task graph").small().strong());
                                ui.label(
                                    RichText::new("Dependencies are arranged left to right by execution phase.")
                                        .small()
                                        .color(palette.text_muted),
                                );
                                ui.add_space(6.0);
                                self.draw_task_graph(ui, &plan, has_cycle, palette);
                            });
                    }
                });
            });

        for id in removals {
            self.remove_task(id);
        }
    }

    /// Inline "route a goal" box. Hands the goal to the app, which owns the
    /// coordinator, the SiteMap and Mission Control.
    fn render_route_goal(&mut self, ui: &mut Ui, palette: IdePalette) {
        let mut submit = false;
        let mut cancel = false;
        ui.group(|ui| {
            ui.label(RichText::new("Route a goal").strong().color(palette.accent));
            ui.label(
                RichText::new(
                    "The planner decomposes it into scoped sub-agents. This replaces the current plan.",
                )
                .small()
                .color(palette.text_muted),
            );
            ui.add(
                egui::TextEdit::multiline(&mut self.goal_draft)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY)
                    .hint_text("Outcome, constraints, and acceptance criteria..."),
            );
            ui.horizontal_wrapped(|ui| {
                if ui.button(RichText::new("Route it").strong()).clicked() {
                    submit = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });

        if cancel {
            self.goal_input_open = false;
            self.goal_draft.clear();
        } else if submit {
            let goal = self.goal_draft.clone();
            if goal.trim().is_empty() {
                self.runtime_status = "Type the goal you want routed.".to_string();
            } else {
                self.request_route_goal(&goal);
            }
        }
    }

    /// The inline add-task form. Fields live on the panel (`self.draft`) rather
    /// than in locals so a half-typed task survives the frame it loses focus in.
    fn render_task_draft(&mut self, ui: &mut Ui, palette: IdePalette) {
        let mut submit = false;
        let mut cancel = false;

        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("New task").strong().color(palette.accent));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!(
                            "Runs on {} / {}",
                            self.defaults.provider.label(),
                            self.defaults.model_label
                        ))
                        .small()
                        .color(palette.text_muted),
                    );
                });
            });

            let title_response = ui.add(
                egui::TextEdit::singleline(&mut self.draft.title)
                    .hint_text("Title (required)")
                    .desired_width(f32::INFINITY),
            );
            // Enter in the title field is how everyone tries to submit a one-word
            // task first; making it work removes a click from the common path.
            if title_response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))
            {
                submit = true;
            }
            ui.add(
                egui::TextEdit::multiline(&mut self.draft.description)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY)
                    .hint_text("What the worker should actually do"),
            );
            ui.add(
                egui::TextEdit::multiline(&mut self.draft.scope)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY)
                    .hint_text("Files it may touch, comma or newline separated"),
            );
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Depends on").small());
                ui.add(
                    egui::TextEdit::singleline(&mut self.draft.dependencies)
                        .hint_text("1, 2 (blank starts immediately)")
                        .desired_width(170.0),
                );
                ui.label(RichText::new("Kind").small());
                let current_kind = self.draft.kind;
                egui::ComboBox::from_id_salt("orchestrator-draft-kind")
                    .selected_text(current_kind.as_str())
                    .show_ui(ui, |ui| {
                        for candidate in AgentTaskKind::ALL {
                            if ui
                                .selectable_label(current_kind == candidate, candidate.as_str())
                                .clicked()
                            {
                                self.draft.kind = candidate;
                            }
                        }
                    });
            });

            ui.horizontal_wrapped(|ui| {
                if ui.button(RichText::new("Add to plan").strong()).clicked() {
                    submit = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });

            if !self.draft.error.is_empty() {
                ui.label(
                    RichText::new(&self.draft.error)
                        .small()
                        .color(palette.error),
                );
            }
        });

        if cancel {
            self.draft = TaskDraft::default();
        } else if submit {
            if let Err(err) = self.add_draft_task() {
                self.draft.error = err;
            } else {
                // `add_draft_task` cleared the fields; keep the form open so a
                // plan can be typed task by task without re-opening it each time.
                self.draft.open = true;
            }
        }
    }

    /// Draw one task. Returns true when the user asked to delete it, so the
    /// caller can mutate the graph outside the borrow this card holds of it.
    fn render_task_card(
        &self,
        ui: &mut Ui,
        task: &Task,
        active_team: Option<&ExpertTeam>,
        palette: IdePalette,
    ) -> bool {
        let mut remove = false;
        let status = self
            .registry
            .as_ref()
            .and_then(|r| r.statuses.get(&task.id))
            .cloned()
            .unwrap_or(TaskStatus::Pending);

        let (status_text, status_color, glyph) = match &status {
            TaskStatus::Pending => ("Pending", palette.text_muted, "\u{25cb}"),
            TaskStatus::Running => ("Running", palette.accent, "\u{25b7}"),
            TaskStatus::Done(_) => ("Done", palette.success, "\u{2714}"),
            TaskStatus::Failed(_) => ("Failed", palette.error, "\u{2716}"),
            TaskStatus::Blocked(_) => ("Blocked", palette.warning, "\u{25c6}"),
        };

        let assigned_expert =
            active_team.and_then(|team| team.find_expert_for_task(&task.title, &task.scope));

        // Card with a subtle fill and a status-tinted border so state reads at a
        // glance; the leading glyph reinforces it for color-blind users.
        egui::Frame::new()
            .fill(palette.bg_secondary.gamma_multiply(0.6))
            .stroke(Stroke::new(1.0, status_color.gamma_multiply(0.5)))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(glyph).color(status_color));
                    ui.label(
                        RichText::new(format!("#{}", task.id.0))
                            .monospace()
                            .small()
                            .color(palette.text_muted),
                    );
                    ui.label(RichText::new(&task.title).small().strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.is_removable(task.id) && ui.small_button("Remove").clicked() {
                            remove = true;
                        }
                        ui.label(RichText::new(status_text).small().color(status_color));
                    });
                });

                // Keep operational metadata on one compact line. It improves
                // scan speed and stops long plans from turning into a wall of cards.
                ui.horizontal_wrapped(|ui| {
                    if let Some(expert) = assigned_expert {
                        ui.label(
                            RichText::new(format!("Agent: {}", expert.name))
                                .small()
                                .color(palette.text_muted),
                        );
                    }
                    if !task.scope.is_empty() {
                        ui.label(
                            RichText::new(format!("Scope: {}", task.scope.join(", ")))
                                .small()
                                .color(palette.text_muted),
                        );
                    }
                });
            });
        ui.add_space(4.0);
        remove
    }

    pub fn draw_task_graph(
        &self,
        ui: &mut Ui,
        plan: &scheduler::Plan,
        has_cycle: bool,
        palette: IdePalette,
    ) {
        let node_w = 156.0;
        let node_h = 48.0;
        let col_spacing = 190.0;
        let row_spacing = 68.0;

        let mut node_positions = HashMap::new();

        let num_phases = if !has_cycle && !plan.phases.is_empty() {
            plan.phases.len()
        } else {
            1
        };
        let max_nodes_in_phase = if !has_cycle && !plan.phases.is_empty() {
            plan.phases.iter().map(|p| p.len()).max().unwrap_or(1)
        } else {
            self.graph.tasks.len().max(1)
        };

        let required_w = (num_phases as f32 * col_spacing + 80.0).max(480.0);
        let required_h = (max_nodes_in_phase as f32 * row_spacing + 80.0).max(340.0);
        let viewport_height = ui.available_height().clamp(360.0, 640.0);

        ScrollArea::both()
            .id_salt("topology_canvas_scroll")
            .max_height(viewport_height)
            .show(ui, |ui| {
                let (rect, _response) =
                    ui.allocate_exact_size(Vec2::new(required_w, required_h), egui::Sense::hover());
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
                                painter
                                    .line_segment([p_from, p_to], Stroke::new(1.5, palette.border));
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

                        let color = match &status {
                            TaskStatus::Pending => palette.text_muted,
                            TaskStatus::Running => palette.accent,
                            TaskStatus::Done(_) => palette.success,
                            TaskStatus::Failed(_) => palette.error,
                            TaskStatus::Blocked(_) => palette.warning,
                        };

                        let node_rect =
                            egui::Rect::from_center_size(pos, Vec2::new(node_w, node_h));
                        painter.rect_filled(node_rect, 4.0, palette.bg_primary);
                        painter.rect_stroke(
                            node_rect,
                            4.0,
                            Stroke::new(1.0, color),
                            egui::StrokeKind::Inside,
                        );

                        let title: String = task.title.chars().take(20).collect();
                        let line_width = 18;
                        let split_at = title
                            .char_indices()
                            .nth(line_width)
                            .map(|(idx, _)| idx)
                            .unwrap_or(title.len());
                        let (first_line, second_line) = title.split_at(split_at);
                        let label = if second_line.is_empty() {
                            format!("#{} {}", id.0, first_line)
                        } else {
                            format!("#{} {}\n{}", id.0, first_line, second_line.trim())
                        };
                        painter.text(
                            pos,
                            egui::Align2::CENTER_CENTER,
                            label,
                            egui::FontId::proportional(11.5),
                            palette.text,
                        );
                    }
                }
            });
    }
}
