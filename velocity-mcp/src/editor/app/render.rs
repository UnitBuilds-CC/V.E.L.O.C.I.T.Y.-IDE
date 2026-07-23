use eframe::egui;
use egui_dock::TabViewer;
use super::types::*;
use crate::editor::chat_panel::render_chat_panel;
use crate::editor::code_editor::CodeEditor;
use crate::editor::task_timeline::{render_mission_activity_feed, TaskTimelineSnapshot};
use crate::editor::theme::{Density, ThemeVariant, WorkspaceProfile};
use crate::editor::usage_panel::render_usage_panel;
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
                                self.app.appearance,
                            );
                            if self.app.pending_cursor_line.is_some() {
                                self.app.pending_cursor_line = None;
                            }
                        },
                    );
                }
            }
            TabKind::Chat => {
                let palette = self.app.palette();
                if render_chat_panel(ui, &mut self.app.chat, &self.app.agent_tx, palette) {
                    self.app.auto_approve = self.app.chat.auto_approve;
                    self.app.selected_model = self.app.chat.selected_model.clone();
                    self.app.thinking_enabled = self.app.chat.thinking_enabled;
                    self.app.provider = self.app.chat.provider;
                    self.app.save_workspace_preferences();
                }
            }
            TabKind::Output => self.output_panel(ui),
            TabKind::Orchestrator => {
                let palette = self.app.palette();
                self.app.orchestrator.ui(
                    ui,
                    &self.app.workspace_root,
                    &self.app.mediator,
                    &mut self.app.expert_teams,
                    &mut self.app.active_team_index,
                    palette,
                );
            }
            TabKind::MissionControl => {
                self.mission_control_panel(ui);
            }
            TabKind::TeamStudio => {
                self.app.render_team_studio(ui);
            }
            TabKind::Usage => {
                render_usage_panel(
                    ui,
                    &self.app.account_usage,
                    &self.app.usage_date,
                    self.app.palette(),
                    || {
                        let _ = self.app.agent_tx.send(crate::agent::UiToAgentMessage::RefreshUsage);
                    },
                );
            }
            TabKind::Search => {
                self.app.search_panel(ui);
            }
            TabKind::Graph => {
                self.app
                    .graph_view
                    .ui(ui, &self.app.workspace_root, &self.app.mediator);
            }
            TabKind::Settings => {
                self.settings_panel(ui);
            }
        }
    }
}

impl<'a> TabViewerImpl<'a> {
    pub fn settings_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.app.palette();
        let truncate_model = |model: &str| {
            if model.chars().count() > 36 {
                format!("{}…", model.chars().take(35).collect::<String>())
            } else {
                model.to_string()
            }
        };
        let provider_badge = |ui: &mut egui::Ui, configured: bool| {
            let (label, color) = if configured {
                ("Workspace configured", palette.success)
            } else {
                ("Env fallback", palette.warning)
            };
            ui.label(egui::RichText::new(label).small().color(color));
        };
        let text_row = |ui: &mut egui::Ui,
                        label: &str,
                        value: &mut String,
                        hint: &str,
                        secret: bool| {
            ui.horizontal(|ui| {
                ui.label(label);
                ui.add(
                    egui::TextEdit::singleline(value)
                        .desired_width(260.0)
                        .hint_text(hint)
                        .password(secret),
                );
            });
        };

        egui::Frame::new()
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("Settings");
                    ui.add_space(8.0);

                    ui.group(|ui| {
                        ui.label(egui::RichText::new("Appearance").strong());
                        ui.add_space(8.0);

                        ui.label(egui::RichText::new("Workspace profile").strong());
                        let mut selected_profile = self.app.appearance.profile;
                        egui::ComboBox::from_id_salt("appearance_profile")
                            .selected_text(selected_profile.label())
                            .show_ui(ui, |ui| {
                                for profile in WorkspaceProfile::ALL {
                                    ui.selectable_value(&mut selected_profile, profile, profile.label());
                                }
                            });
                        ui.label(
                            egui::RichText::new(selected_profile.description())
                                .small()
                                .color(palette.text_muted),
                        );
                        if selected_profile != self.app.appearance.profile {
                            self.app.apply_workspace_profile(selected_profile);
                            self.app.apply_appearance(ui.ctx());
                            self.app.save_workspace_preferences();
                        }

                        ui.add_space(8.0);
                        ui.columns(2, |columns| {
                            columns[0].group(|ui| {
                                ui.label(egui::RichText::new("Theme").strong());
                                let mut theme = self.app.appearance.theme;
                                egui::ComboBox::from_id_salt("appearance_theme")
                                    .selected_text(theme.label())
                                    .show_ui(ui, |ui| {
                                        for variant in ThemeVariant::ALL {
                                            ui.selectable_value(&mut theme, variant, variant.label());
                                        }
                                    });
                                if theme != self.app.appearance.theme {
                                    self.app.appearance.theme = theme;
                                    self.app.apply_appearance(ui.ctx());
                                    self.app.save_workspace_preferences();
                                }

                                ui.add_space(6.0);
                                ui.label(egui::RichText::new("Density").strong());
                                let mut density = self.app.appearance.density;
                                egui::ComboBox::from_id_salt("appearance_density")
                                    .selected_text(density.label())
                                    .show_ui(ui, |ui| {
                                        for option in Density::ALL {
                                            ui.selectable_value(&mut density, option, option.label());
                                        }
                                    });
                                if density != self.app.appearance.density {
                                    self.app.appearance.density = density;
                                    self.app.apply_appearance(ui.ctx());
                                    self.app.save_workspace_preferences();
                                }
                            });

                            columns[1].group(|ui| {
                                ui.label(egui::RichText::new("Scale").strong());
                                let mut changed = false;
                                changed |= ui
                                    .add(egui::Slider::new(&mut self.app.appearance.ui_scale, 0.85..=1.35).text("UI scale"))
                                    .changed();
                                changed |= ui
                                    .add(egui::Slider::new(&mut self.app.appearance.code_scale, 0.85..=1.35).text("Code scale"))
                                    .changed();
                                if changed {
                                    self.app.apply_appearance(ui.ctx());
                                    self.app.save_workspace_preferences();
                                }

                                ui.add_space(6.0);
                                if ui.button("Reset defaults").clicked() {
                                    let profile = self.app.appearance.profile;
                                    self.app.apply_workspace_profile(profile);
                                    self.app.apply_appearance(ui.ctx());
                                    self.app.save_workspace_preferences();
                                }
                                if ui.button("Reset layout").clicked() {
                                    self.app.reset_workspace_layout();
                                    self.app.apply_appearance(ui.ctx());
                                }
                            });
                        });
                    });

                    ui.add_space(8.0);
                    ui.group(|ui| {
                        ui.label(egui::RichText::new("Agent defaults").strong());
                        ui.add_space(6.0);

                        let mut provider = self.app.provider;
                        let mut selected_model = self.app.selected_model.clone();
                        let mut thinking_enabled = self.app.thinking_enabled;
                        let mut auto_approve = self.app.auto_approve;
                        let mut show_thoughts = self.app.chat.show_thoughts;
                        let mut provider_changed = false;
                        let mut model_changed = false;
                        let mut refresh_models = false;

                        ui.horizontal_wrapped(|ui| {
                            ui.label(egui::RichText::new("Provider").small().color(palette.text_muted));
                            egui::ComboBox::from_id_salt("settings_agent_provider")
                                .selected_text(provider.label())
                                .width(180.0)
                                .show_ui(ui, |ui| {
                                    for prov in [
                                        crate::agent::AiProvider::CloudflareWorkersAi,
                                        crate::agent::AiProvider::OpenRouter,
                                        crate::agent::AiProvider::AzureOpenAi,
                                        crate::agent::AiProvider::LocalOllama,
                                    ] {
                                        provider_changed |= ui
                                            .selectable_value(&mut provider, prov, prov.label())
                                            .changed();
                                    }
                                });

                            ui.add_space(8.0);
                            ui.label(egui::RichText::new("Model").small().color(palette.text_muted));
                            if self.app.available_models.is_empty() {
                                ui.label(
                                    egui::RichText::new(truncate_model(&selected_model))
                                        .small()
                                        .color(palette.text_muted),
                                );
                            } else {
                                egui::ComboBox::from_id_salt("settings_agent_model")
                                    .selected_text(truncate_model(&selected_model))
                                    .width(280.0)
                                    .show_ui(ui, |ui| {
                                        for model in self.app.available_models.clone() {
                                            model_changed |= ui
                                                .selectable_value(&mut selected_model, model.id.clone(), model.label)
                                                .changed();
                                        }
                                    });
                            }

                            if ui
                                .button(if self.app.models_loading {
                                    "Loading…"
                                } else {
                                    "↻ Models"
                                })
                                .clicked()
                                && !self.app.models_loading
                            {
                                refresh_models = true;
                            }
                        });

                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.add_enabled(
                                self.app.thinking_supported,
                                egui::Checkbox::new(&mut thinking_enabled, "Thinking"),
                            );
                            ui.checkbox(&mut auto_approve, "Auto-approve tools");
                            ui.checkbox(&mut show_thoughts, "Show thoughts");
                        });

                        let mut prefs_dirty = false;
                        if provider_changed {
                            self.app.provider = provider;
                            self.app.chat.provider = provider;
                            self.app.models_loading = true;
                            let _ = self.app.agent_tx.send(crate::agent::UiToAgentMessage::SetProvider(provider));
                            prefs_dirty = true;
                        }
                        if model_changed {
                            self.app.selected_model = selected_model.clone();
                            self.app.chat.selected_model = selected_model.clone();
                            let _ = self.app.agent_tx.send(crate::agent::UiToAgentMessage::SetModel(selected_model));
                            prefs_dirty = true;
                        }
                        if thinking_enabled != self.app.thinking_enabled {
                            self.app.thinking_enabled = thinking_enabled;
                            self.app.chat.thinking_enabled = thinking_enabled;
                            let _ = self.app.agent_tx.send(crate::agent::UiToAgentMessage::SetThinking(thinking_enabled));
                            prefs_dirty = true;
                        }
                        if auto_approve != self.app.auto_approve {
                            self.app.auto_approve = auto_approve;
                            self.app.chat.auto_approve = auto_approve;
                            prefs_dirty = true;
                        }
                        if show_thoughts != self.app.chat.show_thoughts {
                            self.app.chat.show_thoughts = show_thoughts;
                            prefs_dirty = true;
                        }
                        if refresh_models {
                            self.app.models_loading = true;
                            let _ = self.app.agent_tx.send(crate::agent::UiToAgentMessage::RefreshModels);
                        }
                        if prefs_dirty {
                            self.app.save_workspace_preferences();
                        }
                    });

                    ui.add_space(8.0);
                    ui.group(|ui| {
                        ui.label(egui::RichText::new("Providers & credentials").strong());
                        ui.add_space(8.0);

                        egui::CollapsingHeader::new("Cloudflare Workers AI")
                            .default_open(true)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    provider_badge(ui, self.app.provider_settings.cloudflare.is_configured());
                                });
                                text_row(ui, "Account ID", &mut self.app.provider_settings.cloudflare.account_id, "Cloudflare account ID", false);
                                text_row(ui, "API token", &mut self.app.provider_settings.cloudflare.api_token, "Cloudflare API token", true);
                                text_row(ui, "Tier", &mut self.app.provider_settings.cloudflare.tier, "free or paid", false);
                                text_row(ui, "Label", &mut self.app.provider_settings.cloudflare.label, "default", false);
                            });

                        egui::CollapsingHeader::new("OpenRouter")
                            .default_open(true)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    provider_badge(ui, self.app.provider_settings.openrouter.is_configured());
                                });
                                text_row(ui, "API key", &mut self.app.provider_settings.openrouter.api_key, "OpenRouter API key", true);
                                text_row(ui, "Tier", &mut self.app.provider_settings.openrouter.tier, "free or paid", false);
                                text_row(ui, "Label", &mut self.app.provider_settings.openrouter.label, "OR-Default", false);
                            });

                        egui::CollapsingHeader::new("Azure OpenAI")
                            .default_open(false)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    provider_badge(ui, self.app.provider_settings.azure_openai.is_configured());
                                });
                                text_row(ui, "Endpoint", &mut self.app.provider_settings.azure_openai.endpoint, "https://your-resource.openai.azure.com", false);
                                text_row(ui, "API key", &mut self.app.provider_settings.azure_openai.api_key, "Azure OpenAI key", true);
                                text_row(ui, "Deployment", &mut self.app.provider_settings.azure_openai.deployment, "gpt-4o", false);
                                text_row(ui, "API version", &mut self.app.provider_settings.azure_openai.api_version, "2024-06-01", false);
                                text_row(ui, "Tier", &mut self.app.provider_settings.azure_openai.tier, "paid", false);
                                text_row(ui, "Label", &mut self.app.provider_settings.azure_openai.label, "Azure-Default", false);
                            });

                        egui::CollapsingHeader::new("Local Ollama")
                            .default_open(false)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    provider_badge(ui, self.app.provider_settings.ollama.is_configured());
                                });
                                text_row(ui, "Host", &mut self.app.provider_settings.ollama.host, "http://localhost:11434", false);
                                text_row(ui, "Default model", &mut self.app.provider_settings.ollama.default_model, "llama3.2", false);
                                text_row(ui, "Label", &mut self.app.provider_settings.ollama.label, "Local-Ollama", false);
                            });

                        ui.add_space(8.0);
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Save provider settings").clicked() {
                                self.app.save_provider_settings();
                            }
                            if ui.button("Reload").clicked() {
                                self.app.reload_workspace_provider_settings();
                                self.app.status_message = "Reloaded provider settings".into();
                            }
                        });
                    });
                });
            });
    }

    pub fn output_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.app.palette();
        let code_font = self.app.appearance.code_font_id();
        let is_empty = self.app.command_output.trim().is_empty();
        egui::Frame::new()
            .inner_margin(egui::Margin::same(10))
            .fill(palette.bg_primary)
            .show(ui, |ui: &mut egui::Ui| {
                ui.vertical(|ui: &mut egui::Ui| {
                    if is_empty {
                        ui.add_space(12.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("Terminal output will appear here")
                                    .color(palette.text_muted),
                            );
                        });
                        ui.add_space(8.0);
                    }

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
                                    .font(code_font.clone())
                                    .desired_width(f32::INFINITY)
                                    .text_color(palette.accent),
                            );
                        });

                    ui.separator();

                    let mut run_command = false;
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("> ")
                                .monospace()
                                .color(palette.accent),
                        );
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut self.app.terminal_input)
                                .font(code_font)
                                .desired_width(ui.available_width() - 120.0)
                                .text_color(palette.text),
                        );
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            run_command = true;
                        }
                    });

                    ui.add_space(4.0);

                    ui.horizontal(|ui: &mut egui::Ui| {
                        if ui.small_button("Clear").clicked() {
                            self.app.command_output.clear();
                        }
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui: &mut egui::Ui| {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} B",
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
        let palette = self.app.palette();
        let snapshot = self.app.orchestrator.dashboard_snapshot();
        let valid_task_ids: Vec<u64> = snapshot.tasks.iter().map(|task| task.id).collect();
        self.app.mission_control.sync_selected_task(&valid_task_ids);
        self.app.mirror_worker_events_into_timeline(&snapshot);

        egui::Frame::new().inner_margin(egui::Margin::same(10)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Mission Control");
                ui.add_space(12.0);
                if ui.selectable_label(self.app.mission_control.active_sub_tab == 0, "Brief").clicked() {
                    self.app.mission_control.active_sub_tab = 0;
                }
                if ui.selectable_label(self.app.mission_control.active_sub_tab == 1, "Workers").clicked() {
                    self.app.mission_control.active_sub_tab = 1;
                }
            });
            ui.separator();
            ui.add_space(6.0);

            egui::ScrollArea::vertical()
                .id_salt("mission_control_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if self.app.mission_control.active_sub_tab == 0 {
                        // Brief tab
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Mission Brief").strong().color(palette.accent));
                                ui.checkbox(&mut self.app.mission_control.auto_execute, "Auto-launch");
                            });
                            ui.horizontal_wrapped(|ui| {
                                if ui.small_button("Desktop smoke test").clicked() {
                                    self.app.apply_mission_brief_preset(desktop_automation_smoke_test_brief(), AgentTaskKind::DesktopAutomation);
                                }
                                if ui.small_button("WA validation").clicked() {
                                    self.app.apply_mission_brief_preset(desktop_automation_runtime_validation_brief(), AgentTaskKind::DesktopAutomation);
                                }
                            });
                            ui.add(
                                egui::TextEdit::multiline(&mut self.app.mission_control.brief)
                                    .desired_rows(3)
                                    .desired_width(f32::INFINITY)
                                    .hint_text("Describe your mission..."),
                            );
                            ui.horizontal(|ui| {
                                if ui.button("Plan").clicked() {
                                    self.app.chat.input = self.app.mission_control.brief.clone();
                                    self.app.plan_routed_subagents();
                                }
                                if ui.add_enabled(snapshot.can_launch_routed_tasks, egui::Button::new("Launch")).clicked() {
                                    self.app.orchestrator.execute_routed_tasks(&self.app.workspace_root, &self.app.mediator);
                                }
                                if ui.add_enabled(!snapshot.execution_running && snapshot.retryable_blocked_tasks > 0, egui::Button::new(format!("Retry ({})", snapshot.retryable_blocked_tasks))).clicked() {
                                    self.app.orchestrator.retry_blocked_tasks_action(&self.app.workspace_root, &self.app.mediator);
                                }
                                if ui.add_enabled(snapshot.can_reset_runtime, egui::Button::new("Reset")).clicked() {
                                    self.app.orchestrator.reset_runtime_action();
                                }
                            });
                        });

                        ui.add_space(8.0);

                        // Status summary
                        ui.group(|ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(egui::RichText::new(format!("Plan: {}", snapshot.planning_status)).small());
                                ui.separator();
                                ui.label(egui::RichText::new(format!("Runtime: {}", snapshot.runtime_status)).small());
                            });
                            if let Some(goal) = &snapshot.goal {
                                ui.label(egui::RichText::new(format!("Goal: {}", goal)).small().color(palette.text_muted));
                            }
                            ui.horizontal_wrapped(|ui| {
                                ui.label(egui::RichText::new(format!("Pending: {}", snapshot.pending_tasks)).small());
                                ui.label(egui::RichText::new(format!("Running: {}", snapshot.running_tasks)).small().color(palette.accent));
                                ui.label(egui::RichText::new(format!("Done: {}", snapshot.done_tasks)).small().color(palette.success));
                                if snapshot.failed_tasks > 0 {
                                    ui.label(egui::RichText::new(format!("Failed: {}", snapshot.failed_tasks)).small().color(palette.error));
                                }
                                if snapshot.blocked_tasks > 0 {
                                    ui.label(egui::RichText::new(format!("Blocked: {}", snapshot.blocked_tasks)).small().color(palette.warning));
                                }
                            });
                        });

                        // Activity feed
                        ui.add_space(8.0);
                        ui.group(|ui| {
                            let timeline_snapshot = TaskTimelineSnapshot::new(&self.app.task_timeline);
                            render_mission_activity_feed(ui, &timeline_snapshot, None, 15);
                        });

                    } else {
                        // Workers tab
                        let selected_task = self.app.mission_control.selected_task_id
                            .and_then(|id| snapshot.tasks.iter().find(|t| t.id == id));

                        // Selected task detail
                        if let Some(task) = selected_task {
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(format!("#{} {}", task.id, task.title)).strong());
                                    let status_color = match task.status_label.as_str() {
                                        "Done" => palette.success,
                                        "Running" => palette.accent,
                                        "Blocked" => palette.warning,
                                        "Failed" => palette.error,
                                        _ => palette.text_muted,
                                    };
                                    ui.label(egui::RichText::new(&task.status_label).small().color(status_color));
                                });
                                if !task.scope.is_empty() {
                                    ui.label(egui::RichText::new(format!("Scope: {}", task.scope.join(", "))).small().color(palette.text_muted));
                                }
                                if task.status_label == "Running" {
                                    ui.add(
                                        egui::TextEdit::multiline(&mut self.app.mission_control.selected_task_note_input)
                                            .desired_rows(2)
                                            .desired_width(f32::INFINITY)
                                            .hint_text("Send note to worker..."),
                                    );
                                    if ui.button("Send").clicked() {
                                        let note = self.app.mission_control.selected_task_note_input.trim().to_string();
                                        if !note.is_empty() && self.app.orchestrator.send_task_note_action(crate::orchestrator::TaskId(task.id), note) {
                                            self.app.status_message = format!("Sent note to task #{}", task.id);
                                            self.app.mission_control.selected_task_note_input.clear();
                                        }
                                    }
                                }
                            });
                            ui.add_space(8.0);
                        }

                        // Worker cards
                        if snapshot.tasks.is_empty() {
                            ui.label(egui::RichText::new("No active workers").small().color(palette.text_muted));
                        }
                        for task in &snapshot.tasks {
                            ui.push_id(task.id, |ui| {
                                let is_selected = self.app.mission_control.selected_task_id == Some(task.id);
                                let status_color = match task.status_label.as_str() {
                                    "Done" => palette.success,
                                    "Running" => palette.accent,
                                    "Blocked" => palette.warning,
                                    "Failed" => palette.error,
                                    _ => palette.text_muted,
                                };

                                egui::Frame::new()
                                    .fill(if is_selected { palette.bg_tertiary } else { palette.bg_secondary })
                                    .stroke(egui::Stroke::new(1.0, if is_selected { palette.accent } else { palette.border }))
                                    .corner_radius(egui::CornerRadius::same(6))
                                    .inner_margin(egui::Margin::same(8))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(egui::RichText::new(format!("#{}", task.id)).monospace().small().color(palette.text_muted));
                                            ui.label(egui::RichText::new(&task.title).small().strong());
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                ui.label(egui::RichText::new(&task.status_label).small().color(status_color));
                                            });
                                        });
                                        ui.horizontal(|ui| {
                                            if ui.selectable_label(is_selected, "Inspect").clicked() {
                                                self.app.mission_control.set_selected_task(Some(task.id));
                                            }
                                            if ui.small_button("Stop").clicked() {
                                                self.app.orchestrator.stop_task_action(crate::orchestrator::TaskId(task.id));
                                            }
                                            if ui.small_button("Retry").clicked() {
                                                self.app.orchestrator.retry_task_action(crate::orchestrator::TaskId(task.id), &self.app.workspace_root, &self.app.mediator);
                                            }
                                        });
                                    });
                                ui.add_space(4.0);
                            });
                        }

                        // Intervention input
                        ui.add_space(8.0);
                        ui.group(|ui| {
                            ui.label(egui::RichText::new("Intervention").small().strong());
                            ui.add(
                                egui::TextEdit::multiline(&mut self.app.mission_control.intervention_input)
                                    .desired_rows(2)
                                    .desired_width(f32::INFINITY)
                                    .hint_text("Mid-flight correction..."),
                            );
                            if ui.button("Queue").clicked() {
                                let note = self.app.mission_control.intervention_input.trim().to_string();
                                if !note.is_empty() {
                                    let id = self.app.next_intervention_id;
                                    self.app.next_intervention_id += 1;
                                    self.app.mission_control.queue_intervention(id, note);
                                    self.app.mission_control.intervention_input.clear();
                                }
                            }
                        });
                    }
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
