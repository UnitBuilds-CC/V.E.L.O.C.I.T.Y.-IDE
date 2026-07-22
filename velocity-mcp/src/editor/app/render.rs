use eframe::egui;
use egui_dock::TabViewer;
use super::types::*;
use super::wa::*;
use crate::editor::agent_ui_render::{render_agent_metrics, render_pending_approvals, render_thinking_panel, RenderSnapshot};
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
                self.app
                    .orchestrator
                    .ui(ui, &self.app.workspace_root, &self.app.mediator, palette);
            }
            TabKind::MissionControl => {
                self.mission_control_panel(ui);
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
                    ui.heading("Workspace settings");
                    ui.label(
                        egui::RichText::new(
                            "Tune layout, agent defaults, and per-workspace provider credentials without leaving the app.",
                        )
                        .color(palette.text_muted),
                    );
                    ui.add_space(8.0);

                    ui.group(|ui| {
                        ui.label(egui::RichText::new("Appearance & workspace").strong());
                        ui.label(
                            egui::RichText::new(
                                "Shape the shell for coding, automation, supervision, or accessibility-first work.",
                            )
                            .small()
                            .color(palette.text_muted),
                        );
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
                                if ui.button("Reset to profile defaults").clicked() {
                                    let profile = self.app.appearance.profile;
                                    self.app.apply_workspace_profile(profile);
                                    self.app.apply_appearance(ui.ctx());
                                    self.app.save_workspace_preferences();
                                }
                                if ui.button("Reset workspace layout").clicked() {
                                    self.app.reset_workspace_layout();
                                    self.app.apply_appearance(ui.ctx());
                                }
                            });
                        });
                    });

                    ui.add_space(8.0);
                    ui.group(|ui| {
                        ui.label(egui::RichText::new("Agent defaults").strong());
                        ui.label(
                            egui::RichText::new(
                                "These controls mirror the chat toolbar, but live here so workspace behavior is configurable in one place.",
                            )
                            .small()
                            .color(palette.text_muted),
                        );
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

                        ui.label(
                            egui::RichText::new(
                                "Provider, model, and thinking changes apply immediately. Auto-approve and thought visibility persist per workspace.",
                            )
                            .small()
                            .color(palette.text_muted),
                        );

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
                        ui.label(
                            egui::RichText::new(
                                "Credentials are stored per workspace in .velocity\\provider-settings.json. Leave fields blank to keep using environment or .env values.",
                            )
                            .small()
                            .color(palette.text_muted),
                        );
                        ui.add_space(8.0);

                        egui::CollapsingHeader::new("Cloudflare Workers AI")
                            .default_open(true)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    provider_badge(ui, self.app.provider_settings.cloudflare.is_configured());
                                    ui.label(
                                        egui::RichText::new("Account ID + token are required for workspace override.")
                                            .small()
                                            .color(palette.text_muted),
                                    );
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
                                    ui.label(
                                        egui::RichText::new("Workspace key overrides env-based OpenRouter access.")
                                            .small()
                                            .color(palette.text_muted),
                                    );
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
                                    ui.label(
                                        egui::RichText::new("Endpoint and key are required; deployment and API version can be tuned here too.")
                                            .small()
                                            .color(palette.text_muted),
                                    );
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
                                    ui.label(
                                        egui::RichText::new("Host is enough to activate a workspace-local Ollama target.")
                                            .small()
                                            .color(palette.text_muted),
                                    );
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
                            if ui.button("Reload provider settings").clicked() {
                                self.app.reload_workspace_provider_settings();
                                self.app.status_message = "Reloaded workspace provider settings from disk".into();
                            }
                            if ui.button("Open chat controls").clicked() {
                                self.app.focus_panel(TabKind::Chat);
                            }
                        });
                    });

                    ui.add_space(8.0);
                    ui.group(|ui| {
                        let profile = self.app.appearance.profile;
                        let retryable_blocked = self.app.orchestrator.retryable_blocked_task_count();
                        let approval_count = self.app.pending_approvals.len();
                        let dirty_count = self.app.dirty_buffer_count();
                        ui.label(egui::RichText::new("Workspace guidance").strong());
                        ui.label(
                            egui::RichText::new(profile.focus_label())
                                .small()
                                .color(palette.accent),
                        );
                        ui.label(
                            egui::RichText::new(profile.quick_tip())
                                .small()
                                .color(palette.text_muted),
                        );
                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            for summary in [
                                format!("Pending approvals: {approval_count}"),
                                format!("Dirty editors: {dirty_count}"),
                                format!("Blocked tasks: {retryable_blocked}"),
                            ] {
                                egui::Frame::new()
                                    .fill(palette.bg_tertiary)
                                    .stroke(egui::Stroke::new(1.0, palette.border))
                                    .corner_radius(egui::CornerRadius::same(6))
                                    .inner_margin(egui::Margin::symmetric(10, 6))
                                    .show(ui, |ui| {
                                        ui.label(summary);
                                    });
                            }
                        });
                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            match profile {
                                WorkspaceProfile::Coder => {
                                    if ui.button("Focus chat").clicked() {
                                        self.app.focus_panel(TabKind::Chat);
                                    }
                                    if ui.button("Open search").clicked() {
                                        self.app.focus_panel(TabKind::Search);
                                    }
                                    if ui.button("Show output").clicked() {
                                        self.app.focus_panel(TabKind::Output);
                                    }
                                }
                                WorkspaceProfile::AutomationOperator => {
                                    if ui.button("Open orchestrator").clicked() {
                                        self.app.focus_panel(TabKind::Orchestrator);
                                    }
                                    if ui.button("Show output").clicked() {
                                        self.app.focus_panel(TabKind::Output);
                                    }
                                    if ui.button("Focus mission control").clicked() {
                                        self.app.focus_panel(TabKind::MissionControl);
                                    }
                                }
                                WorkspaceProfile::MissionControl => {
                                    if ui.button("Open mission control").clicked() {
                                        self.app.focus_panel(TabKind::MissionControl);
                                    }
                                    if ui.button("Review approvals").clicked() {
                                        self.app.focus_panel(TabKind::Chat);
                                    }
                                    if ui.button("Open orchestrator").clicked() {
                                        self.app.focus_panel(TabKind::Orchestrator);
                                    }
                                }
                                WorkspaceProfile::Accessibility => {
                                    if ui.button("Open settings").clicked() {
                                        self.app.focus_panel(TabKind::Settings);
                                    }
                                    if ui.button("Focus mission control").clicked() {
                                        self.app.focus_panel(TabKind::MissionControl);
                                    }
                                    if ui.button("Focus chat").clicked() {
                                        self.app.focus_panel(TabKind::Chat);
                                    }
                                }
                            }
                        });
                    });

                    ui.add_space(8.0);
                    ui.group(|ui| {
                        ui.label(egui::RichText::new("Preview").strong());
                        ui.horizontal_wrapped(|ui| {
                            for (label, color) in [
                                ("Accent", palette.accent),
                                ("Success", palette.success),
                                ("Warning", palette.warning),
                                ("Error", palette.error),
                            ] {
                                egui::Frame::new()
                                    .fill(palette.bg_tertiary)
                                    .stroke(egui::Stroke::new(1.0, palette.border))
                                    .corner_radius(egui::CornerRadius::same(6))
                                    .inner_margin(egui::Margin::symmetric(10, 6))
                                    .show(ui, |ui| {
                                        ui.colored_label(color, "●");
                                        ui.label(label);
                                    });
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
                        ui.group(|ui| {
                            ui.label(egui::RichText::new("Output is quiet").strong());
                            ui.label(
                                egui::RichText::new(
                                    "Use this panel to capture shell output, automation evidence, and quick diagnostics without leaving the workspace.",
                                )
                                .small()
                                .color(palette.text_muted),
                            );
                            ui.add_space(4.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    egui::RichText::new("Good first moves:")
                                        .small()
                                        .color(palette.text_muted),
                                );
                                if ui.small_button("pwd").clicked() {
                                    self.app.terminal_input = "pwd".into();
                                }
                                if ui.small_button("git status").clicked() {
                                    self.app.terminal_input = "git status".into();
                                }
                                if ui.small_button("Focus mission control").clicked() {
                                    self.app.focus_panel(TabKind::MissionControl);
                                }
                            });
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
        let palette = self.app.palette();
        let snapshot = self.app.orchestrator.dashboard_snapshot();
        let valid_task_ids: Vec<u64> = snapshot.tasks.iter().map(|task| task.id).collect();
        self.app.mission_control.sync_selected_task(&valid_task_ids);
        self.app.mirror_worker_events_into_timeline(&snapshot);
        let is_empty_state = !snapshot.has_routed_plan
            && snapshot.tasks.is_empty()
            && self.app.mission_control.interventions.is_empty();
        egui::Frame::new().inner_margin(egui::Margin::same(10)).show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("mission_control_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
            ui.heading("🎛 Mission Control");
            ui.label(
                egui::RichText::new("One brief → routed plan → live swarm → operator interventions")
                    .small()
                    .color(palette.text_muted),
            );
            ui.add_space(8.0);

            if is_empty_state {
                ui.group(|ui| {
                    ui.label(egui::RichText::new("Start a supervised run").strong());
                    ui.label(
                        egui::RichText::new(
                            "Mission Control is quiet right now. Start with a brief, generate a routed plan, then monitor live execution and intervene only when the system surfaces something important.",
                        )
                        .small()
                        .color(palette.text_muted),
                    );
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            egui::RichText::new("Good first moves:")
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
                        if ui.small_button("Focus chat").clicked() {
                            self.app.focus_panel(TabKind::Chat);
                        }
                    });
                });
                ui.add_space(8.0);
            }

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
                        if is_empty_state {
                            ui.separator();
                            ui.label(
                                egui::RichText::new("No routed mission yet. Add a brief above, plan it, then use this panel to watch health, blockers, and live worker detail.")
                                    .small()
                                    .color(palette.text_muted),
                            );
                        } else if let Some(task) = selected_task {
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
                                    .color(palette.text_muted),
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
                                                .color(palette.accent),
                                        );
                                        if let Some(evidence_state) = desktop_evidence_state {
                                            ui.label(
                                                egui::RichText::new("Desktop automation")
                                                    .small()
                                                    .color(palette.warning),
                                            );
                                            let evidence_color = match evidence_state {
                                                DesktopAutomationEvidenceState::LiveEvidence => {
                                                    palette.accent
                                                }
                                                DesktopAutomationEvidenceState::ArtifactBacked => {
                                                    palette.success
                                                }
                                                DesktopAutomationEvidenceState::AwaitingEvidence => {
                                                    palette.warning
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
                                                .color(palette.accent),
                                        );
                                    }
                                    ui.label(egui::RichText::new(&task.description).small().weak());
                                    if !task.scope.is_empty() {
                                        ui.label(
                                            egui::RichText::new(format!("Scope: {}", task.scope.join(", ")))
                                                .small()
                                                .color(palette.text_muted),
                                        );
                                    }
                                    if !task.rationale.is_empty() {
                                        ui.label(
                                            egui::RichText::new(format!("Why: {}", task.rationale))
                                                .small()
                                                .color(palette.text),
                                        );
                                    }
                                    if let Some(evidence_state) = desktop_evidence_state {
                                        ui.label(
                                            egui::RichText::new(evidence_state.detail())
                                                .small()
                                                .color(palette.text_muted),
                                        );
                                    }
                                    if !task.outputs.is_empty() {
                                        ui.label(
                                            egui::RichText::new(format!("Outputs: {}", task.outputs.join(", ")))
                                                .small()
                                                .color(palette.success),
                                        );
                                    }
                                    if !task.message.is_empty() {
                                        ui.label(
                                            egui::RichText::new(format!("Status: {}", task.message))
                                                .small()
                                                .color(palette.warning),
                                        );
                                    }
                                    if let Some(path) = &task.wa_run_path {
                                        ui.label(
                                            egui::RichText::new(format!("WA run: {}", path))
                                                .small()
                                                .color(palette.text_muted),
                                        );
                                    }
                                    if let Some(path) = &task.run_summary_path {
                                        ui.label(
                                            egui::RichText::new(format!("Run summary: {}", path))
                                                .small()
                                                .color(palette.text_muted),
                                        );
                                    }
                                    if let Some(path) = &task.run_facts_path {
                                        ui.label(
                                            egui::RichText::new(format!("Run facts: {}", path))
                                                .small()
                                                .color(palette.text_muted),
                                        );
                                    }
                                    if let Some(run_id) = &task.wa_run_id {
                                        ui.label(
                                            egui::RichText::new(format!("WA run id: {}", run_id))
                                                .small()
                                                .color(palette.text_muted),
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
        });
    }
}

pub fn desktop_automation_smoke_test_brief() -> &'static str {
    "Run a Windows automation smoke test for the IDE desktop flow: capture a live window snapshot, resolve deterministic selectors, execute a narrow scripted interaction, and report any failing desktop-testing step with truthful WA evidence."
}

pub fn desktop_automation_runtime_validation_brief() -> &'static str {
    "Validate the WA desktop automation runtime end-to-end for a Windows app: capture the target window, verify selectors against the live UIA tree, run the saved script with post-action verification, and summarize any desktop automation mismatch or blocked step."
}
