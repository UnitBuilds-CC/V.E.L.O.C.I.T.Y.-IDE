use eframe::egui;
use egui_dock::TabViewer;
use super::types::*;
use super::wa::*;
use crate::editor::theme::IdePalette;
use crate::editor::agent_ui_render::{render_agent_metrics, render_pending_approvals, render_thinking_panel, RenderSnapshot};
use crate::editor::chat_panel::render_chat_panel;
use crate::editor::code_editor::CodeEditor;
use crate::editor::usage_panel::render_usage_panel;
use crate::editor::task_timeline::{render_mission_activity_feed, TaskTimelineSnapshot};
use crate::automation::AgentTaskKind;

pub struct TabViewerImpl<'a> {
    pub app: &'a mut super::VelocityApp,
}

impl<'a> TabViewer for TabViewerImpl<'a> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        tab.title().into()
    }

    fn on_close(&mut self, tab: &mut Self::Tab) -> egui_dock::tab_viewer::OnCloseResponse {
        self.app.close_tab(&tab.id);
        egui_dock::tab_viewer::OnCloseResponse::Close
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match &mut tab.kind {
            TabKind::Editor { path, buffer_id } => {
                if let Some(buf) = self.app.buffers.get_mut(buffer_id) {
                    egui::Frame::new().inner_margin(egui::Margin::same(4)).show(
                        ui,
                        |ui: &mut egui::Ui| {
                            let mut editor = CodeEditor::new("code_editor");
                            let locks = path
                                .as_deref()
                                .map(|p| self.app.mediator.get_locks_for_file(p))
                                .unwrap_or_default();
                            editor.show(
                                ui,
                                buf.content_mut(),
                                path.as_deref(),
                                self.app.pending_cursor_line,
                                &locks,
                            );
                            if self.app.pending_cursor_line.is_some() {
                                self.app.pending_cursor_line = None;
                            }
                        },
                    );
                }
            }
            TabKind::Chat => {
                render_chat_panel(ui, &mut self.app.chat, &self.app.agent_tx);
            }
            TabKind::Output => self.output_panel(ui),
            TabKind::Orchestrator => {
                self.app
                    .orchestrator
                    .ui(ui, &self.app.workspace_root, &self.app.mediator);
            }
            TabKind::MissionControl => {
                self.mission_control_panel(ui);
            }
            TabKind::Usage => {
                render_usage_panel(ui, &self.app.account_usage, &self.app.usage_date, || {
                    let _ = self.app.agent_tx.send(crate::agent::UiToAgentMessage::RefreshUsage);
                });
            }
            TabKind::Search => {
                self.app.search_panel(ui);
            }
            TabKind::Graph => {
                self.app
                    .graph_view
                    .ui(ui, &self.app.workspace_root, &self.app.mediator);
            }
        }
    }
}

impl<'a> TabViewerImpl<'a> {
    pub fn output_panel(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .inner_margin(egui::Margin::same(10))
            .fill(IdePalette::dark().bg_primary)
            .show(ui, |ui: &mut egui::Ui| {
                ui.vertical(|ui: &mut egui::Ui| {
                    let scroll_height = ui.available_height() - 75.0;
                    egui::ScrollArea::vertical()
                        .max_height(scroll_height)
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui: &mut egui::Ui| {
                            let mut text = self.app.command_output.clone();
                            ui.add(
                                egui::TextEdit::multiline(&mut text)
                                    .code_editor()
                                    .font(egui::FontId::monospace(13.0))
                                    .desired_width(f32::INFINITY)
                                    .text_color(IdePalette::dark().accent),
                            );
                        });

                    ui.separator();

                    let mut run_command = false;
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("> ")
                                .monospace()
                                .color(IdePalette::dark().accent),
                        );
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut self.app.terminal_input)
                                .font(egui::FontId::monospace(13.0))
                                .desired_width(ui.available_width() - 120.0)
                                .text_color(IdePalette::dark().text),
                        );
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            run_command = true;
                        }
                    });

                    ui.add_space(4.0);

                    ui.horizontal(|ui: &mut egui::Ui| {
                        if ui.button("🗑 Clear Console").clicked() {
                            self.app.command_output.clear();
                        }
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui: &mut egui::Ui| {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Buffer: {} bytes",
                                        self.app.command_output.len()
                                    ))
                                    .small()
                                    .weak(),
                                );
                            },
                        );
                    });

                    if run_command {
                        let cmd_str = self.app.terminal_input.trim().to_string();
                        if !cmd_str.is_empty() {
                            self.app
                                .command_output
                                .push_str(&format!("> {}\n", cmd_str));
                            self.app.terminal_input.clear();

                            let (tx, rx) = std::sync::mpsc::channel();
                            self.app.terminal_rx = Some(rx);

                            let workspace_root = self.app.workspace_root.clone();
                            std::thread::spawn(move || {
                                let mut cmd = if cfg!(target_os = "windows") {
                                    let mut c = std::process::Command::new("cmd");
                                    c.args(&["/C", &cmd_str]);
                                    c
                                } else {
                                    let mut c = std::process::Command::new("sh");
                                    c.args(&["-c", &cmd_str]);
                                    c
                                };
                                cmd.current_dir(&workspace_root);
                                if let Ok(output) = cmd.output() {
                                    let stdout = String::from_utf8_lossy(&output.stdout);
                                    let stderr = String::from_utf8_lossy(&output.stderr);
                                    let _ = tx.send(format!("{}{}", stdout, stderr));
                                } else {
                                    let _ =
                                        tx.send("Error: Command execution failed\n".to_string());
                                }
                            });
                        }
                    }
                });
            });
    }

    pub fn mission_control_panel(&mut self, ui: &mut egui::Ui) {
        let palette = IdePalette::dark();
        let snapshot = self.app.orchestrator.dashboard_snapshot();
        let valid_task_ids: Vec<u64> = snapshot.tasks.iter().map(|task| task.id).collect();
        self.app.mission_control.sync_selected_task(&valid_task_ids);
        self.app.mirror_worker_events_into_timeline(&snapshot);
        egui::Frame::new().inner_margin(egui::Margin::same(10)).show(ui, |ui| {
            ui.heading("🎛 Mission Control");
            ui.label(
                egui::RichText::new("One brief → routed plan → live swarm → operator interventions")
                    .small()
                    .color(palette.text_muted),
            );
            ui.add_space(8.0);

            if let Some(wa_summary) =
                desktop_automation_mission_summary(&snapshot.tasks, snapshot.task_kind.as_deref())
            {
                ui.group(|ui| {
                    ui.label(egui::RichText::new("Desktop testing summary").strong());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("WA tasks {}", wa_summary.task_count));
                        ui.separator();
                        ui.label(format!("Live {}", wa_summary.live_count));
                        ui.separator();
                        ui.label(format!("Artifact-backed {}", wa_summary.artifact_count));
                        ui.separator();
                        ui.label(format!("Awaiting evidence {}", wa_summary.awaiting_count));
                    });
                    if !wa_summary.state_labels.is_empty() {
                        ui.label(
                            egui::RichText::new(wa_summary.state_labels.join(" • "))
                                .small()
                                .color(palette.text_muted),
                        );
                    }
                });
                ui.add_space(8.0);
            }

            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label("Mission brief:");
                    ui.checkbox(&mut self.app.mission_control.auto_execute, "Auto-launch after planning");
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new("Quick presets:")
                            .small()
                            .color(palette.text_muted),
                    );
                    if ui.small_button("Desktop smoke test").clicked() {
                        self.app.apply_mission_brief_preset(
                            desktop_automation_smoke_test_brief(),
                            AgentTaskKind::DesktopAutomation,
                        );
                    }
                    if ui.small_button("WA runtime validation").clicked() {
                        self.app.apply_mission_brief_preset(
                            desktop_automation_runtime_validation_brief(),
                            AgentTaskKind::DesktopAutomation,
                        );
                    }
                });
                ui.add(
                    egui::TextEdit::multiline(&mut self.app.mission_control.brief)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .hint_text("Build me a full app..."),
                );
                ui.horizontal(|ui| {
                    if ui.button("Plan mission").clicked() {
                        self.app.chat.input = self.app.mission_control.brief.clone();
                        self.app.plan_routed_subagents();
                    }
                    if ui
                        .add_enabled(snapshot.can_launch_routed_tasks, egui::Button::new("Launch routed tasks"))
                        .clicked()
                    {
                        self.app
                            .orchestrator
                            .execute_routed_tasks(&self.app.workspace_root, &self.app.mediator);
                    }
                    if ui
                        .add_enabled(
                            !snapshot.execution_running && snapshot.retryable_blocked_tasks > 0,
                            egui::Button::new(format!("Retry blocked ({})", snapshot.retryable_blocked_tasks)),
                        )
                        .clicked()
                    {
                        self.app
                            .orchestrator
                            .retry_blocked_tasks_action(&self.app.workspace_root, &self.app.mediator);
                    }
                    if ui
                        .add_enabled(snapshot.can_reset_runtime, egui::Button::new("Reset runtime"))
                        .clicked()
                    {
                        self.app.orchestrator.reset_runtime_action();
                    }
                });
                let runtime_hint = if !snapshot.has_routed_plan {
                    Some("Plan mission first to create runnable routed tasks.")
                } else if snapshot.has_dependency_cycle {
                    Some("Resolve the dependency cycle before launching routed tasks.")
                } else if snapshot.execution_running {
                    Some("Routed tasks are already running; use task controls below for live intervention.")
                } else {
                    None
                };
                if let Some(runtime_hint) = runtime_hint {
                    ui.label(
                        egui::RichText::new(runtime_hint)
                            .small()
                            .color(palette.text_muted),
                    );
                }
            });

            ui.add_space(8.0);
            ui.columns(2, |columns| {
                columns[0].vertical(|ui| {
                    let selected_task = self
                        .app
                        .mission_control
                        .selected_task_id
                        .and_then(|selected_id| snapshot.tasks.iter().find(|task| task.id == selected_id));
                    ui.group(|ui| {
                        ui.label(egui::RichText::new("Mission status").strong());
                        ui.label(format!("Plan: {}", snapshot.planning_status));
                        ui.label(format!("Runtime: {}", snapshot.runtime_status));
                        if let Some(goal) = &snapshot.goal {
                            ui.label(format!("Goal: {}", goal));
                        }
                        if let Some(kind) = &snapshot.task_kind {
                            ui.label(format!("Kind: {}", kind));
                        }
                        ui.label(format!("Scoped files: {}", snapshot.scope_count));
                        if let Some(task) = selected_task {
                            ui.separator();
                            ui.label(egui::RichText::new(format!("Selected task: #{} {}", task.id, task.title)).strong());
                            ui.label(
                                egui::RichText::new(format!("Targeted scope: {}", if task.scope.is_empty() { "(inherits routed scope)".to_string() } else { task.scope.join(", ") }))
                                    .small()
                                    .color(palette.text_muted),
                            );
                        }
                    });

                    ui.add_space(6.0);
                    ui.group(|ui| {
                        ui.label(egui::RichText::new("Swarm scoreboard").strong());
                        ui.horizontal_wrapped(|ui| {
                            ui.label(format!("Pending {}", snapshot.pending_tasks));
                            ui.separator();
                            ui.label(format!("Running {}", snapshot.running_tasks));
                            ui.separator();
                            ui.label(format!("Done {}", snapshot.done_tasks));
                            ui.separator();
                            ui.label(format!("Failed {}", snapshot.failed_tasks));
                            ui.separator();
                            ui.label(format!("Blocked {}", snapshot.blocked_tasks));
                            ui.separator();
                            ui.label(format!("Workers {}", snapshot.active_workers));
                        });
                    });

                    ui.add_space(6.0);
                    ui.group(|ui| {
                        ui.label(egui::RichText::new("Selected task thread").strong());
                        if let Some(task) = selected_task {
                            let is_selected_deskt_auto = task_matches_desktop_automation_lane(
                                task,
                                snapshot.task_kind.as_deref(),
                            );
                            ui.label(egui::RichText::new(format!("#{} {}", task.id, task.title)).strong());
                            if task.status_label == "Running" {
                                ui.label(
                                    egui::RichText::new("Live worker thread is active. Notes sent here go directly to the routed worker. Stop is supported; pause/resume is intentionally unavailable until the runtime can suspend honestly.")
                                        .small()
                                        .color(palette.text_muted),
                                );
                            } else {
                                ui.label(
                                    egui::RichText::new("Task is not currently running. Direct worker notes are only available during live execution.")
                                        .small()
                                        .color(palette.text_muted),
                                );
                            }
                            if is_selected_deskt_auto {
                                let wa_status = desktop_automation_selected_task_status(task);
                                let wa_cues = desktop_automation_selected_task_cues(task);
                                ui.separator();
                                ui.label(egui::RichText::new("Desktop automation status").small().strong());
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(format!("State {}", wa_status.state_label));
                                    ui.separator();
                                    ui.label(format!("Artifacts {}", wa_status.artifact_count));
                                    ui.separator();
                                    ui.label(format!("Outputs {}", wa_status.output_count));
                                    ui.separator();
                                    ui.label(format!("Evidence updates {}", wa_status.evidence_update_count));
                                });
                                ui.label(
                                    egui::RichText::new(wa_status.state_detail)
                                        .small()
                                        .color(palette.text_muted),
                                );
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(if wa_status.has_transcript {
                                        "Transcript captured"
                                    } else {
                                        "Transcript pending"
                                    });
                                    ui.separator();
                                    ui.label(if wa_status.has_operator_notes {
                                        "Operator notes present"
                                    } else {
                                        "No operator notes"
                                    });
                                });
                                ui.label(egui::RichText::new("Desktop automation artifacts").small().strong());
                                if wa_cues.artifact_lines.is_empty() {
                                    ui.label(
                                        egui::RichText::new("No WA artifacts captured yet.")
                                            .small()
                                            .color(palette.text_muted),
                                    );
                                } else {
                                    for line in &wa_cues.artifact_lines {
                                        ui.label(
                                            egui::RichText::new(line)
                                                .small()
                                                .color(palette.text_muted),
                                        );
                                    }
                                }
                                ui.label(
                                    egui::RichText::new(format!("Next operator step: {}", wa_cues.next_action))
                                        .small()
                                        .color(palette.text_muted),
                                );
                                ui.label(egui::RichText::new("Desktop automation evidence").small().strong());
                                for line in desktop_automation_evidence_lines(task) {
                                    ui.label(
                                        egui::RichText::new(line)
                                            .small()
                                            .color(palette.text_muted),
                                    );
                                }
                            }
                            ui.add(
                                egui::TextEdit::multiline(&mut self.app.mission_control.selected_task_note_input)
                                    .desired_rows(3)
                                    .desired_width(f32::INFINITY)
                                    .hint_text("Send a note to the selected routed worker..."),
                            );
                            let can_send_task_note = task.status_label == "Running";
                            if ui
                                .add_enabled(can_send_task_note, egui::Button::new("Send to selected task"))
                                .clicked()
                            {
                                let note = self.app.mission_control.selected_task_note_input.trim().to_string();
                                if !note.is_empty()
                                    && self
                                        .app
                                        .orchestrator
                                        .send_task_note_action(crate::orchestrator::TaskId(task.id), note)
                                {
                                    self.app.status_message = format!("Sent note to task #{}", task.id);
                                    self.app.toasts.push(crate::editor::toast::Toast::info(format!("Sent note to task #{}", task.id)));
                                    self.app.mission_control.selected_task_note_input.clear();
                                }
                            }
                            if let Some(thread) = &task.live_thread {
                                egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, false]).show(ui, |ui| {
                                    if !thread.events.is_empty() {
                                        ui.separator();
                                        ui.label(egui::RichText::new("Worker event stream").small().strong());
                                        egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                                            for event in thread.events.iter().rev().take(12) {
                                                let color = match event.kind {
                                                    crate::orchestrator::worker::WorkerThreadEventKind::Status => palette.accent,
                                                    crate::orchestrator::worker::WorkerThreadEventKind::Transcript => palette.text,
                                                    crate::orchestrator::worker::WorkerThreadEventKind::FileChange => palette.success,
                                                    crate::orchestrator::worker::WorkerThreadEventKind::OperatorNote => palette.warning,
                                                    crate::orchestrator::worker::WorkerThreadEventKind::ToolApproval => palette.accent,
                                                    crate::orchestrator::worker::WorkerThreadEventKind::ToolStarted => palette.accent.gamma_multiply(0.8),
                                                    crate::orchestrator::worker::WorkerThreadEventKind::ToolFinished => palette.accent.gamma_multiply(0.6),
                                                };
                                                ui.label(egui::RichText::new(&event.message).small().color(color));
                                            }
                                        });
                                    }
                                    if !thread.operator_notes.is_empty() {
                                        ui.separator();
                                        ui.label(egui::RichText::new("Operator notes").small().strong());
                                        for note in thread.operator_notes.iter().rev().take(4) {
                                            ui.label(egui::RichText::new(note).small().color(palette.warning));
                                        }
                                    }
                                    if !thread.changed_files.is_empty() {
                                        ui.separator();
                                        ui.label(
                                            egui::RichText::new(format!("Observed file activity: {}", thread.changed_files.join(", ")))
                                                .small()
                                                .color(palette.success),
                                        );
                                    }
                                    if !thread.transcript.trim().is_empty() {
                                        ui.separator();
                                        ui.label(egui::RichText::new("Live transcript").small().strong());
                                        let mut transcript = thread.transcript.clone();
                                        ui.add(
                                            egui::TextEdit::multiline(&mut transcript)
                                                .desired_rows(8)
                                                .desired_width(f32::INFINITY)
                                                .interactive(false),
                                        );
                                    }
                                });
                            } else if !task.message.is_empty() {
                                ui.separator();
                                ui.label(egui::RichText::new(task.message.clone()).small().color(palette.warning));
                            }
                        } else {
                            ui.label(
                                egui::RichText::new("Select a routed task to inspect its dedicated worker thread.")
                                    .small()
                                    .color(palette.text_muted),
                            );
                        }
                    });

                    ui.add_space(6.0);
                    ui.group(|ui| {
                        let timeline_snapshot = TaskTimelineSnapshot::new(&self.app.task_timeline);
                        render_mission_activity_feed(
                            ui,
                            &timeline_snapshot,
                            self.app.mission_control.selected_task_id,
                            14,
                        );
                    });

                    ui.add_space(6.0);
                    ui.group(|ui| {
                        ui.label(egui::RichText::new("Operator intervention inbox").strong());
                        ui.add(
                            egui::TextEdit::multiline(&mut self.app.mission_control.intervention_input)
                                .desired_rows(3)
                                .desired_width(f32::INFINITY)
                                .hint_text("Mid-flight change, tweak, expansion, or correction..."),
                        );
                        if ui.button("Queue intervention").clicked() {
                            let note = self.app.mission_control.intervention_input.trim().to_string();
                            if !note.is_empty() {
                                let id = self.app.next_intervention_id;
                                self.app.next_intervention_id += 1;
                                self.app.mission_control.queue_intervention(id, note.clone());
                                self.app.mission_control.intervention_input.clear();
                                self.app.status_message = format!("Queued intervention #{id}");
                            }
                        }

                        let mut queued_action: Option<(u64, crate::editor::mission_control::InterventionDisposition, String)> = None;
                        for item in &self.app.mission_control.interventions {
                            ui.separator();
                            ui.label(egui::RichText::new(format!("#{}", item.id)).strong());
                            ui.label(&item.note);
                            ui.label(
                                egui::RichText::new(&item.status)
                                    .small()
                                    .color(IdePalette::dark().text_muted),
                            );
                            ui.horizontal(|ui| {
                                let action_label = if selected_task.map(|task| task.status_label == "Running").unwrap_or(false) {
                                    "Send to selected task"
                                } else {
                                    "Apply to running agent"
                                };
                                if ui.small_button(action_label).clicked() {
                                    queued_action = Some((item.id, crate::editor::mission_control::InterventionDisposition::ApplyToRunningAgent, item.note.clone()));
                                }
                                if ui.small_button("Spawn routed task").clicked() {
                                    queued_action = Some((item.id, crate::editor::mission_control::InterventionDisposition::SpawnRoutedFollowUp, item.note.clone()));
                                }
                                if ui.small_button("Dismiss").clicked() {
                                    queued_action = Some((item.id, crate::editor::mission_control::InterventionDisposition::Dismissed, item.note.clone()));
                                }
                            });
                        }

                        if let Some((id, disposition, note)) = queued_action {
                            let targeted_context = self
                                .app
                                .mission_control
                                .selected_task_id
                                .and_then(|selected_id| snapshot.tasks.iter().find(|task| task.id == selected_id))
                                .map(|task| {
                                    let scope = if task.scope.is_empty() {
                                        "(inherits routed scope)".to_string()
                                    } else {
                                        task.scope.join(", ")
                                    };
                                    format!("Task #{} {}\nScope: {}", task.id, task.title, scope)
                                });
                            if let Some(item) = self
                                .app
                                .mission_control
                                .interventions
                                .iter_mut()
                                .find(|entry| entry.id == id)
                            {
                                item.disposition = Some(disposition.clone());
                                item.status = match disposition {
                                    crate::editor::mission_control::InterventionDisposition::ApplyToRunningAgent => {
                                        if selected_task
                                            .and_then(|task| (task.status_label == "Running").then_some(task.id))
                                            .is_some()
                                        {
                                            "Sent to selected worker thread".to_string()
                                        } else {
                                            "Sent to agent chat for live steering".to_string()
                                        }
                                    }
                                    crate::editor::mission_control::InterventionDisposition::SpawnRoutedFollowUp => "Prepared as a new routed mission brief".to_string(),
                                    crate::editor::mission_control::InterventionDisposition::Dismissed => "Dismissed by operator".to_string(),
                                };
                            }

                            match disposition {
                                crate::editor::mission_control::InterventionDisposition::ApplyToRunningAgent => {
                                    let sent_to_task = selected_task
                                        .and_then(|task| (task.status_label == "Running").then_some(task.id))
                                        .map(|task_id| {
                                            self.app.orchestrator.send_task_note_action(
                                                crate::orchestrator::TaskId(task_id),
                                                note.clone(),
                                            )
                                        })
                                        .unwrap_or(false);
                                    if sent_to_task {
                                        self.app.status_message = "Sent intervention to selected worker thread".to_string();
                                        self.app.toasts.push(crate::editor::toast::Toast::info("Sent intervention to selected worker thread".to_string()));
                                    } else {
                                        let prompt = if let Some(context) = &targeted_context {
                                            format!("Apply this operator intervention with priority to the targeted routed task context below.\n\n{context}\n\nOperator intervention:\n{note}")
                                        } else {
                                            note.clone()
                                        };
                                        self.app.chat.push_user(prompt.clone());
                                        self.app.chat_history.push_str("\nYou: ");
                                        self.app.chat_history.push_str(&prompt);
                                        self.app.agent_active = true;
                                        self.app.chat.agent_active = true;
                                        let _ = self.app.agent_tx.send(crate::agent::UiToAgentMessage::UserPrompt(prompt));
                                    }
                                }
                                crate::editor::mission_control::InterventionDisposition::SpawnRoutedFollowUp => {
                                    let brief = if let Some(context) = &targeted_context {
                                        format!("{note}\n\nTarget this routed follow-up at:\n{context}")
                                    } else {
                                        note.clone()
                                    };
                                    self.app.mission_control.brief = brief.clone();
                                    self.app.chat.input = brief;
                                    self.app.plan_routed_subagents();
                                }
                                crate::editor::mission_control::InterventionDisposition::Dismissed => {}
                            }
                        }
                    });
                });

                columns[1].vertical(|ui| {
                    ui.group(|ui| {
                        let agent_snapshot = RenderSnapshot::new(&self.app.agent_ui_state);
                        ui.label(egui::RichText::new("Approvals, metrics, and reasoning").strong());
                        render_agent_metrics(ui, &agent_snapshot);
                        ui.separator();
                        render_pending_approvals(ui, &agent_snapshot);
                        ui.separator();
                        render_thinking_panel(ui, &agent_snapshot, (226, 227, 243));
                    });

                    ui.add_space(6.0);
                    ui.group(|ui: &mut egui::Ui| {
                        ui.label(egui::RichText::new("Live agent cards").strong());
                        egui::ScrollArea::vertical()
                            .id_salt("live_agent_cards_scroll")
                            .max_height(420.0)
                            .show(ui, |ui: &mut egui::Ui| {
                            for task in &snapshot.tasks {
                                ui.push_id(task.id, |ui: &mut egui::Ui| {
                                let is_selected = self.app.mission_control.selected_task_id == Some(task.id);
                                let is_desktop_automation = task_matches_desktop_automation_lane(
                                    task,
                                    snapshot.task_kind.as_deref(),
                                );
                                let desktop_evidence_state =
                                    is_desktop_automation.then(|| desktop_automation_evidence_state(task));
                                ui.group(|ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        if ui.selectable_label(is_selected, format!("#{} {}", task.id, task.title)).clicked() {
                                            self.app.mission_control.set_selected_task(Some(task.id));
                                        }
                                        ui.label(
                                            egui::RichText::new(&task.status_label)
                                                .small()
                                                .color(IdePalette::dark().accent),
                                        );
                                        if let Some(evidence_state) = desktop_evidence_state {
                                            ui.label(
                                                egui::RichText::new("Desktop automation")
                                                    .small()
                                                    .color(IdePalette::dark().warning),
                                            );
                                            let evidence_color = match evidence_state {
                                                DesktopAutomationEvidenceState::LiveEvidence => {
                                                    IdePalette::dark().accent
                                                }
                                                DesktopAutomationEvidenceState::ArtifactBacked => {
                                                    IdePalette::dark().success
                                                }
                                                DesktopAutomationEvidenceState::AwaitingEvidence => {
                                                    IdePalette::dark().warning
                                                }
                                            };
                                            ui.label(
                                                egui::RichText::new(evidence_state.label())
                                                    .small()
                                                    .color(evidence_color),
                                            );
                                        }
                                    });
                                    if !task.provider_label.is_empty() && !task.model_label.is_empty() {
                                        ui.label(
                                            egui::RichText::new(format!("{} / {}", task.provider_label, task.model_label))
                                                .small()
                                                .color(IdePalette::dark().accent),
                                        );
                                    }
                                    ui.label(egui::RichText::new(&task.description).small().weak());
                                    if !task.scope.is_empty() {
                                        ui.label(
                                            egui::RichText::new(format!("Scope: {}", task.scope.join(", ")))
                                                .small()
                                                .color(IdePalette::dark().text_muted),
                                        );
                                    }
                                    if !task.rationale.is_empty() {
                                        ui.label(
                                            egui::RichText::new(format!("Why: {}", task.rationale))
                                                .small()
                                                .color(IdePalette::dark().text),
                                        );
                                    }
                                    if let Some(evidence_state) = desktop_evidence_state {
                                        ui.label(
                                            egui::RichText::new(evidence_state.detail())
                                                .small()
                                                .color(IdePalette::dark().text_muted),
                                        );
                                    }
                                    if !task.outputs.is_empty() {
                                        ui.label(
                                            egui::RichText::new(format!("Outputs: {}", task.outputs.join(", ")))
                                                .small()
                                                .color(IdePalette::dark().success),
                                        );
                                    }
                                    if !task.message.is_empty() {
                                        ui.label(
                                            egui::RichText::new(format!("Status: {}", task.message))
                                                .small()
                                                .color(IdePalette::dark().warning),
                                        );
                                    }
                                    if let Some(path) = &task.wa_run_path {
                                        ui.label(
                                            egui::RichText::new(format!("WA run: {}", path))
                                                .small()
                                                .color(IdePalette::dark().text_muted),
                                        );
                                    }
                                    if let Some(path) = &task.run_summary_path {
                                        ui.label(
                                            egui::RichText::new(format!("Run summary: {}", path))
                                                .small()
                                                .color(IdePalette::dark().text_muted),
                                        );
                                    }
                                    if let Some(path) = &task.run_facts_path {
                                        ui.label(
                                            egui::RichText::new(format!("Run facts: {}", path))
                                                .small()
                                                .color(IdePalette::dark().text_muted),
                                        );
                                    }
                                    if let Some(run_id) = &task.wa_run_id {
                                        ui.label(
                                            egui::RichText::new(format!("WA run id: {}", run_id))
                                                .small()
                                                .color(IdePalette::dark().text_muted),
                                        );
                                    }
                                    ui.horizontal_wrapped(|ui| {
                                        if ui.small_button(if is_selected { "Selected" } else { "Select" }).clicked() {
                                            self.app.mission_control.set_selected_task(Some(task.id));
                                        }
                                        let can_retry_task = task.status_label == "Follow-up";
                                        let can_stop_task = task.status_label == "Running";
                                        if ui
                                            .add_enabled(can_stop_task, egui::Button::new("Stop task"))
                                            .clicked()
                                        {
                                            if self.app.orchestrator.stop_task_action(crate::orchestrator::TaskId(task.id)) {
                                                self.app.status_message = format!("Stopping task #{}", task.id);
                                                self.app.toasts.push(crate::editor::toast::Toast::warn(format!("Stopping task #{}", task.id)));
                                            }
                                        }
                                        if ui
                                            .add_enabled(can_retry_task, egui::Button::new("Retry task"))
                                            .clicked()
                                        {
                                            if self
                                                .app
                                                .orchestrator
                                                .retry_task_action(crate::orchestrator::TaskId(task.id), &self.app.workspace_root, &self.app.mediator)
                                            {
                                                self.app.status_message = format!("Retrying task #{}", task.id);
                                                self.app.toasts.push(crate::editor::toast::Toast::info(format!("Retrying task #{}", task.id)));
                                            }
                                        }
                                        if ui.small_button("Reset task").clicked() {
                                            if self.app.orchestrator.reset_task_action(crate::orchestrator::TaskId(task.id)) {
                                                self.app.status_message = format!("Reset task #{}", task.id);
                                                self.app.toasts.push(crate::editor::toast::Toast::warn(format!("Reset task #{} to pending", task.id)));
                                            }
                                        }
                                        if ui.small_button("Route follow-up").clicked() {
                                            self.app.mission_control.set_selected_task(Some(task.id));
                                            let scope = if task.scope.is_empty() {
                                                "(inherits routed scope)".to_string()
                                            } else {
                                                task.scope.join(", ")
                                            };
                                            self.app.mission_control.brief = format!(
                                                "Follow up on routed task #{} {}.\n\nFocus scope: {}\n\nGoal:\n",
                                                task.id,
                                                task.title,
                                                scope
                                            );
                                        }
                                    });
                                });
                                ui.add_space(4.0);
                                });
                            }
                        });
                    });
                });
            });
        });
    }
}

pub fn desktop_automation_smoke_test_brief() -> &'static str {
    "Run a Windows automation smoke test for the IDE desktop flow: capture a live window snapshot, resolve deterministic selectors, execute a narrow scripted interaction, and report any failing desktop-testing step with truthful WA evidence."
}

pub fn desktop_automation_runtime_validation_brief() -> &'static str {
    "Validate the WA desktop automation runtime end-to-end for a Windows app: capture the target window, verify selectors against the live UIA tree, run the saved script with post-action verification, and summarize any desktop automation mismatch or blocked step."
}
