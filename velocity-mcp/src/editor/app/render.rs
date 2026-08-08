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
        let mut title = tab.title();
        // Append an unsaved-changes marker for editor tabs with pending edits.
        if let TabKind::Editor { buffer_id, .. } = &tab.kind {
            if self
                .app
                .buffers
                .get(buffer_id)
                .map(|b| b.is_dirty())
                .unwrap_or(false)
            {
                title.push_str("  ●");
            }
        }
        title.into()
    }

    fn on_close(&mut self, tab: &mut Self::Tab) -> egui_dock::tab_viewer::OnCloseResponse {
        // Guard unsaved editor tabs: defer to the confirm dialog instead of closing.
        if self.app.tab_is_dirty(&tab.id) {
            self.app.pending_close_tab = Some(tab.id.clone());
            return egui_dock::tab_viewer::OnCloseResponse::Ignore;
        }
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
                            // Breadcrumbs are rendered in the dedicated top
                            // panel (see ui_render.rs breadcrumb strip) so we
                            // no longer duplicate them inside each editor tab.

                            // Find/Replace overlay
                            if buf.find_replace.visible {
                                let palette = self.app.appearance.palette();
                                crate::editor::find_replace::render_find_replace(
                                    ui,
                                    &mut buf.find_replace,
                                    &mut buf.content,
                                    palette,
                                );
                            }

                            // Main editor area with optional minimap
                            ui.horizontal_top(|ui| {
                                // Editor
                                let editor_width = if self.app.show_minimap {
                                    ui.available_width() - self.app.minimap_config.width - 8.0
                                } else {
                                    ui.available_width()
                                };
                                ui.allocate_ui(egui::Vec2::new(editor_width, ui.available_height()), |ui| {
                                    let mut editor = CodeEditor::new("code_editor");
                                    let locks = path
                                        .as_deref()
                                        .map(|p| self.app.mediator.get_locks_for_file(p))
                                        .unwrap_or_default();
                                    buf.refresh_diff_marks();
                                    let diff_marks = buf.diff_marks.clone();
                                    let options = crate::editor::code_editor::EditorOptions {
                                        cursor_offset: 0,
                                        diagnostic_lines: self.app.diagnostics.lines_for_file(
                                            path.as_deref().unwrap_or(std::path::Path::new(""))
                                        ),
                                        breakpoints: buf.breakpoints.clone(),
                                        collapsed_lines: buf.fold_state.collapsed_lines(),
                                        word_wrap: self.app.word_wrap,
                                    };
                                    editor.show_enhanced(
                                        ui,
                                        buf.content_mut(),
                                        path.as_deref(),
                                        self.app.pending_cursor_line,
                                        &locks,
                                        self.app.appearance,
                                        &diff_marks,
                                        &options,
                                    );
                                    if self.app.pending_cursor_line.is_some() {
                                        self.app.pending_cursor_line = None;
                                    }

                                    // Show inline diagnostic popup if cursor is on a diagnostic line.
                                    if let Some(file_path) = path.as_deref() {
                                        let cursor_line = self.app.current_cursor_line;
                                        let palette = self.app.appearance.palette();
                                        // Use the editor's cursor position for popup placement.
                                        let cursor_pos = ui.cursor().min;
                                        self.app.diagnostics.render_inline_popup_at_line(
                                            ui,
                                            file_path,
                                            cursor_line,
                                            cursor_pos,
                                            &palette,
                                        );
                                    }
                                });

                                // Minimap
                                if self.app.show_minimap {
                                    let palette = self.app.appearance.palette();
                                    let highlights: Vec<crate::editor::minimap::MinimapHighlight> = buf.breakpoints.iter()
                                        .map(|&line| crate::editor::minimap::MinimapHighlight {
                                            line,
                                            color: palette.error,
                                        })
                                        .collect();
                                    crate::editor::minimap::render_minimap(
                                        ui,
                                        &buf.content,
                                        self.app.minimap_config,
                                        0, // viewport_start_line
                                        30, // viewport_end_line
                                        &highlights,
                                        &palette,
                                    );
                                }
                            });

                            // Completion popup
                            if self.app.completion_state.active {
                                let palette = self.app.appearance.palette();
                                crate::editor::completion::render_completion_popup(
                                    ui,
                                    &self.app.completion_state,
                                    palette,
                                );
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
                let action = self
                    .app
                    .graph_view
                    .ui(ui, &self.app.workspace_root, self.app.palette());
                if let Some(crate::editor::graph_view::GraphAction::NavigateToSymbol(name)) = action
                {
                    self.app.push_nav_location();
                    self.app.jump_to_symbol_name(&name);
                }
            }
            TabKind::Wiki => {
                let palette = self.app.palette();
                let action = self.app.wiki_view.ui(
                    ui,
                    &self.app.workspace_root,
                    &mut self.app.toasts,
                    palette,
                );
                if let Some(crate::editor::wiki_view::WikiAction::GenerateDetail(prompt)) = action {
                    let _ = self
                        .app
                        .agent_tx
                        .send(crate::agent::UiToAgentMessage::UserPrompt(prompt));
                    self.app.toggle_panel(TabKind::Chat);
                    self.app
                        .toasts
                        .push(crate::editor::toast::Toast::info(
                            "Detail request sent to agent — see Chat",
                        ));
                }
            }
            TabKind::Settings => {
                self.settings_panel(ui);
            }
            TabKind::Extensions => {
                self.app.render_extensions_panel(ui);
            }
            TabKind::Activity => {
                self.app.render_activity_panel(ui);
            }
            TabKind::Coverage => {
                self.app.render_coverage_panel(ui);
            }
            TabKind::Pipeline => {
                self.app.render_pipeline_panel(ui);
            }
            TabKind::Voice => {
                self.app.render_voice_panel(ui);
            }
            TabKind::Knowledge => {
                self.app.render_knowledge_panel(ui);
            }
            TabKind::Triggers => {
                self.app.render_triggers_panel(ui);
            }
            TabKind::Workflows => {
                self.app.render_workflows_panel(ui);
            }
            TabKind::Governance => {
                self.app.render_governance_panel(ui);
            }
            TabKind::NdaDoc { .. } => {
                let palette = self.app.palette();
                let tab_id = tab.id.clone();
                let workspace_root = self.app.workspace_root.clone();
                let view = self
                    .app
                    .nda_docs
                    .entry(tab_id)
                    .or_default();
                let open_path = view.ui(ui, &workspace_root, &mut self.app.toasts, palette);
                if let Some(p) = open_path {
                    crate::editor::nda_document::open_in_browser(&p);
                }
            }
            // Mode-specific panel tabs - real content from orchestrator/timeline data
            _ => {
                let palette = self.app.palette();
                let kind_label = tab.title();
                match &tab.kind {
                    TabKind::Flows => {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new("⧉ Automation Flows").size(14.0).strong().color(palette.accent));
                            ui.add_space(8.0);
                            let tasks = &self.app.orchestrator.graph.tasks;
                            if tasks.is_empty() {
                                ui.label(egui::RichText::new("No flows defined. Create a flow via the Orchestrator panel.").size(11.0).color(palette.text_muted));
                            } else {
                                for task in tasks.values() {
                                    egui::Frame::new().fill(palette.bg_tertiary).corner_radius(4.0).inner_margin(8.0).show(ui, |ui| {
                                        ui.label(egui::RichText::new(&task.title).size(11.0).strong().color(palette.text));
                                        ui.label(egui::RichText::new(&task.description).size(9.0).color(palette.text_muted));
                                        ui.label(egui::RichText::new(format!("Scope: {} file(s)", task.scope.len())).size(9.0).color(palette.text_muted));
                                    });
                                    ui.add_space(4.0);
                                }
                            }
                        });
                    }
                    TabKind::Terminal => {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new("$ Terminal").monospace().size(11.0).color(palette.accent));
                            ui.add_space(4.0);
                            if self.app.command_output.is_empty() {
                                ui.label(egui::RichText::new("Ready.").monospace().size(10.0).color(palette.text_muted));
                            } else {
                                ui.label(egui::RichText::new(&self.app.command_output).monospace().size(10.0).color(palette.text));
                            }
                        });
                    }
                    TabKind::Logs => {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new("≣ Execution Logs").size(14.0).strong().color(palette.accent));
                            ui.add_space(8.0);
                            let snapshot = crate::editor::task_timeline::TaskTimelineSnapshot::new(&self.app.task_timeline);
                            crate::editor::task_timeline::render_task_timeline(ui, &snapshot, palette);
                        });
                    }
                    TabKind::Agents => {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new("⊙ Agent Roster").size(14.0).strong().color(palette.accent));
                            ui.add_space(8.0);
                            let snapshot = self.app.orchestrator.dashboard_snapshot();
                            if snapshot.tasks.is_empty() {
                                ui.label(egui::RichText::new("No agents running. Deploy via the toolbar.").size(11.0).color(palette.text_muted));
                            } else {
                                for t in &snapshot.tasks {
                                    let color = match t.status_label.as_str() {
                                        "Running" => palette.success,
                                        "Done" => palette.text_muted,
                                        "Failed" => palette.error,
                                        _ => palette.warning,
                                    };
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("●").color(color));
                                        ui.label(egui::RichText::new(&t.title).size(11.0).color(palette.text));
                                        ui.label(egui::RichText::new(&t.status_label).size(9.0).color(color));
                                    });
                                }
                            }
                        });
                    }
                    TabKind::Queue => {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new("⊞ Task Queue").size(14.0).strong().color(palette.accent));
                            ui.add_space(8.0);
                            let snapshot = self.app.orchestrator.dashboard_snapshot();
                            ui.label(egui::RichText::new(format!("{} pending · {} running · {} done · {} failed",
                                snapshot.pending_tasks, snapshot.running_tasks, snapshot.done_tasks, snapshot.failed_tasks
                            )).size(10.0).color(palette.text_muted));
                            ui.add_space(8.0);
                            for t in &snapshot.tasks {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(format!("#{}", t.id)).size(9.0).color(palette.text_muted));
                                    ui.label(egui::RichText::new(&t.title).size(10.0).color(palette.text));
                                    ui.label(egui::RichText::new(&t.status_label).size(9.0).color(palette.accent));
                                });
                            }
                        });
                    }
                    TabKind::Timeline => {
                        let snapshot = crate::editor::task_timeline::TaskTimelineSnapshot::new(&self.app.task_timeline);
                        crate::editor::task_timeline::render_task_timeline(ui, &snapshot, palette);
                    }
                    TabKind::Metrics => {
                        let snapshot = self.app.orchestrator.dashboard_snapshot();
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new("⊿ Mission Metrics").size(14.0).strong().color(palette.accent));
                            ui.add_space(8.0);
                            egui::Grid::new("panel_metrics_grid").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                                ui.label(egui::RichText::new("Pending:").size(11.0).color(palette.text_muted));
                                ui.label(egui::RichText::new(format!("{}", snapshot.pending_tasks)).size(11.0).color(palette.warning));
                                ui.end_row();
                                ui.label(egui::RichText::new("Running:").size(11.0).color(palette.text_muted));
                                ui.label(egui::RichText::new(format!("{}", snapshot.running_tasks)).size(11.0).color(palette.success));
                                ui.end_row();
                                ui.label(egui::RichText::new("Done:").size(11.0).color(palette.text_muted));
                                ui.label(egui::RichText::new(format!("{}", snapshot.done_tasks)).size(11.0).color(palette.text));
                                ui.end_row();
                                ui.label(egui::RichText::new("Failed:").size(11.0).color(palette.text_muted));
                                ui.label(egui::RichText::new(format!("{}", snapshot.failed_tasks)).size(11.0).color(palette.error));
                                ui.end_row();
                                ui.label(egui::RichText::new("Active workers:").size(11.0).color(palette.text_muted));
                                ui.label(egui::RichText::new(format!("{}", snapshot.active_workers)).size(11.0).color(palette.accent));
                                ui.end_row();
                            });
                        });
                    }
                    TabKind::Changes => {
                        // Recent changes timeline with git log and uncommitted changes.
                        let mut git_state = crate::editor::git_ui::GitState::from_workspace(&self.app.workspace_root);
                        git_state.refresh_log(&self.app.workspace_root);
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            ui.add_space(8.0);
                            crate::editor::git_ui::render_recent_changes_timeline(ui, &git_state, palette);
                        });
                    }
                    _ => {
                        ui.vertical_centered(|ui| {
                            ui.add_space(32.0);
                            ui.label(egui::RichText::new(&kind_label).size(16.0).strong().color(palette.accent));
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new("Panel active in the current mode.").size(11.0).color(palette.text_muted));
                        });
                    }
                }
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
        // At-a-glance configured dot shown right on each provider header, so you
        // don't have to expand a section to see whether it's set up. The whole
        // header is tinted by status (green = configured, muted = not).
        let provider_header = |name: &str, configured: bool| {
            egui::RichText::new(format!("●  {}", name))
                .strong()
                .color(if configured {
                    palette.success
                } else {
                    palette.text_muted
                })
        };
        // Consistent section headers: accent-tinted with breathing room.
        let section_header = |ui: &mut egui::Ui, title: &str| {
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new(title.to_uppercase())
                    .small()
                    .strong()
                    .color(palette.accent),
            );
            ui.add_space(6.0);
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
        let api_key_provider_row = |ui: &mut egui::Ui,
                                    name: &str,
                                    settings: &mut crate::usage::WorkspaceApiKeySettings,
                                    hint: &str| {
            egui::CollapsingHeader::new(
                provider_header(name, settings.is_configured()),
            )
                .default_open(false)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        provider_badge(ui, settings.is_configured());
                    });
                    text_row(ui, "API key", &mut settings.api_key, hint, true);
                    text_row(ui, "Label", &mut settings.label, &format!("{}-Default", name), false);
                });
        };

        egui::Frame::new()
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("Settings");
                    ui.add_space(8.0);

                    ui.group(|ui| {
                        section_header(ui, "Appearance");

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
                        // Route through set_work_mode so switching here shares the
                        // toolbar's per-mode layout memory (snapshot + restore).
                        if selected_profile != self.app.appearance.profile {
                            self.app.set_work_mode(selected_profile);
                            self.app.apply_appearance(ui.ctx());
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

                        ui.add_space(8.0);
                        ui.group(|ui| {
                            section_header(ui, "Editor");
                            ui.checkbox(&mut self.app.show_breadcrumbs, "Show breadcrumbs above editor");
                            ui.checkbox(&mut self.app.word_wrap, "Word wrap in editor");
                        });
                    });

                    ui.add_space(8.0);
                    ui.group(|ui| {
                        section_header(ui, "System");
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("GPU").small().color(palette.text_muted));
                            ui.label(
                                egui::RichText::new(&self.app.gpu_name)
                                    .monospace()
                                    .small()
                                    .color(palette.text),
                            );
                        });
                    });

                    ui.add_space(8.0);
                    ui.group(|ui| {
                        section_header(ui, "Agent defaults");

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
                        section_header(ui, "Providers & credentials");

                        egui::CollapsingHeader::new(
                            provider_header("Cloudflare Workers AI", self.app.provider_settings.cloudflare.is_configured()),
                        )
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

                        egui::CollapsingHeader::new(
                            provider_header("OpenRouter", self.app.provider_settings.openrouter.is_configured()),
                        )
                            .default_open(true)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    provider_badge(ui, self.app.provider_settings.openrouter.is_configured());
                                });
                                text_row(ui, "API key", &mut self.app.provider_settings.openrouter.api_key, "OpenRouter API key", true);
                                text_row(ui, "Tier", &mut self.app.provider_settings.openrouter.tier, "free or paid", false);
                                text_row(ui, "Label", &mut self.app.provider_settings.openrouter.label, "OR-Default", false);
                            });

                        egui::CollapsingHeader::new(
                            provider_header("Azure OpenAI", self.app.provider_settings.azure_openai.is_configured()),
                        )
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

                        egui::CollapsingHeader::new(
                            provider_header("Local Ollama", self.app.provider_settings.ollama.is_configured()),
                        )
                            .default_open(false)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    provider_badge(ui, self.app.provider_settings.ollama.is_configured());
                                });
                                text_row(ui, "Host", &mut self.app.provider_settings.ollama.host, "http://localhost:11434", false);
                                text_row(ui, "Default model", &mut self.app.provider_settings.ollama.default_model, "llama3.2", false);
                                text_row(ui, "Label", &mut self.app.provider_settings.ollama.label, "Local-Ollama", false);
                            });

                        // ── API-key providers ──
                        api_key_provider_row(ui, "OpenAI", &mut self.app.provider_settings.openai, "OpenAI API key");
                        api_key_provider_row(ui, "Google Vertex / Gemini", &mut self.app.provider_settings.google, "Google API key");
                        api_key_provider_row(ui, "Deepseek", &mut self.app.provider_settings.deepseek, "Deepseek API key");
                        api_key_provider_row(ui, "Groq", &mut self.app.provider_settings.groq, "Groq API key");
                        api_key_provider_row(ui, "Mistral AI", &mut self.app.provider_settings.mistral, "Mistral API key");
                        api_key_provider_row(ui, "Alibaba Qwen", &mut self.app.provider_settings.alibaba, "DashScope API key");
                        api_key_provider_row(ui, "Together AI", &mut self.app.provider_settings.together, "Together API key");
                        api_key_provider_row(ui, "Fireworks AI", &mut self.app.provider_settings.fireworks, "Fireworks API key");
                        api_key_provider_row(ui, "Perplexity", &mut self.app.provider_settings.perplexity, "Perplexity API key");
                        api_key_provider_row(ui, "Cerebras", &mut self.app.provider_settings.cerebras, "Cerebras API key");
                        api_key_provider_row(ui, "AWS Bedrock", &mut self.app.provider_settings.bedrock, "Bedrock proxy API key (set BEDROCK_PROXY_URL env var)");
                        api_key_provider_row(ui, "Anthropic", &mut self.app.provider_settings.anthropic, "Anthropic API key");

                        ui.add_space(8.0);
                        ui.horizontal_wrapped(|ui| {
                            let save = ui.add(
                                egui::Button::new(
                                    egui::RichText::new("Save provider settings")
                                        .color(palette.success)
                                        .strong(),
                                )
                            );
                            if save.on_hover_text("Writes credentials to the workspace provider settings file").clicked() {
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

        // Human-readable byte size for the output counter (e.g. "12.3 KB").
        fn human_bytes(n: usize) -> String {
            const KB: f64 = 1024.0;
            const MB: f64 = KB * 1024.0;
            let n = n as f64;
            if n >= MB {
                format!("{:.1} MB", n / MB)
            } else if n >= KB {
                format!("{:.1} KB", n / KB)
            } else {
                format!("{} B", n as usize)
            }
        }

        egui::Frame::new()
            .inner_margin(egui::Margin::same(10))
            .fill(palette.bg_primary)
            .show(ui, |ui: &mut egui::Ui| {
                ui.vertical(|ui: &mut egui::Ui| {
                    if is_empty {
                        ui.add_space(16.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("›_")
                                    .monospace()
                                    .size(26.0)
                                    .color(palette.accent.gamma_multiply(0.7)),
                            );
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new("Run a command below — output appears here")
                                    .color(palette.text_muted),
                            );
                        });
                        ui.add_space(12.0);
                    }

                    let scroll_height = ui.available_height() - 75.0;
                    egui::ScrollArea::vertical()
                        .max_height(scroll_height)
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui: &mut egui::Ui| {
                            // Color each line by meaning so output is scannable:
                            // commands in accent, errors red, warnings amber.
                            ui.vertical(|ui| {
                                for line in self.app.command_output.lines() {
                                    let trimmed = line.trim_start();
                                    let color = if trimmed.starts_with('>') {
                                        palette.accent
                                    } else if trimmed.starts_with("error")
                                        || trimmed.contains("error:")
                                        || trimmed.starts_with("Error")
                                    {
                                        palette.error
                                    } else if trimmed.starts_with("warning")
                                        || trimmed.contains("warning:")
                                    {
                                        palette.warning
                                    } else {
                                        palette.text
                                    };
                                    ui.label(
                                        egui::RichText::new(if line.is_empty() { " " } else { line })
                                            .monospace()
                                            .size(13.0)
                                            .color(color),
                                    );
                                }
                            });
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
                                    egui::RichText::new(human_bytes(self.app.command_output.len()))
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
                                    c.args(["/C", &cmd_str]);
                                    c
                                } else {
                                    let mut c = std::process::Command::new("sh");
                                    c.args(["-c", &cmd_str]);
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
                ui.heading(egui::RichText::new("Mission Control").color(palette.accent));
                ui.add_space(12.0);
                if ui.selectable_label(self.app.mission_control.active_sub_tab == 0, "Brief").clicked() {
                    self.app.mission_control.active_sub_tab = 0;
                }
                if ui.selectable_label(self.app.mission_control.active_sub_tab == 1, "Workers").clicked() {
                    self.app.mission_control.active_sub_tab = 1;
                }
                if ui.selectable_label(self.app.mission_control.active_sub_tab == 2, "Live").clicked() {
                    self.app.mission_control.active_sub_tab = 2;
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
                                ui.label(egui::RichText::new("·").small().color(palette.text_muted.gamma_multiply(0.6)));
                                ui.label(egui::RichText::new(format!("Runtime: {}", snapshot.runtime_status)).small());
                            });
                            if let Some(goal) = &snapshot.goal {
                                ui.label(egui::RichText::new(format!("Goal: {}", goal)).small().color(palette.text_muted));
                            }
                            ui.horizontal_wrapped(|ui| {
                                if let Some(kind) = &snapshot.task_kind {
                                    ui.label(egui::RichText::new(format!("Kind: {}", kind)).small().color(palette.text_muted));
                                    ui.label(egui::RichText::new("·").small().color(palette.text_muted.gamma_multiply(0.6)));
                                }
                                ui.label(egui::RichText::new(format!("Scope: {}", snapshot.scope_count)).small().color(palette.text_muted));
                                if snapshot.has_routed_plan {
                                    ui.label(egui::RichText::new("· Routed plan ready").small().color(palette.success));
                                }
                                if snapshot.has_dependency_cycle {
                                    ui.label(egui::RichText::new("· ⚠ dependency cycle").small().color(palette.warning));
                                }
                            });
                            ui.horizontal_wrapped(|ui| {
                                ui.label(egui::RichText::new(format!("Pending: {}", snapshot.pending_tasks)).small());
                                ui.label(egui::RichText::new(format!("Running: {}", snapshot.running_tasks)).small().color(palette.accent));
                                ui.label(egui::RichText::new(format!("Active workers: {}", snapshot.active_workers)).small().color(palette.accent));
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
                            render_mission_activity_feed(ui, &timeline_snapshot, None, 15, palette);
                        });

                    } else if self.app.mission_control.active_sub_tab == 1 {
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
                                if !task.provider_label.is_empty() || !task.model_label.is_empty() {
                                    let model = if task.model_label.is_empty() { "default" } else { task.model_label.as_str() };
                                    ui.label(egui::RichText::new(format!("Model: {} / {}", task.provider_label, model)).small().color(palette.text_muted));
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
                            ui.add_space(16.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    egui::RichText::new("◇")
                                        .size(28.0)
                                        .color(palette.accent.gamma_multiply(0.7)),
                                );
                                ui.add_space(6.0);
                                ui.label(
                                    egui::RichText::new("No active workers — launch a mission to see them here")
                                        .color(palette.text_muted),
                                );
                            });
                            ui.add_space(12.0);
                        }
                        for task in &snapshot.tasks {
                            ui.push_id(task.id, |ui| {
                                let is_selected = self.app.mission_control.selected_task_id == Some(task.id);
                                let (status_color, glyph) = match task.status_label.as_str() {
                                    "Done" => (palette.success, "✔"),
                                    "Running" => (palette.accent, "▷"),
                                    "Blocked" => (palette.warning, "◆"),
                                    "Failed" => (palette.error, "✖"),
                                    _ => (palette.text_muted, "○"),
                                };

                                egui::Frame::new()
                                    .fill(if is_selected { palette.bg_tertiary } else { palette.bg_secondary })
                                    .stroke(egui::Stroke::new(1.0, if is_selected { palette.accent } else { palette.border }))
                                    .corner_radius(egui::CornerRadius::same(6))
                                    .inner_margin(egui::Margin::same(8))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(egui::RichText::new(glyph).color(status_color));
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
                                            if ui.small_button("Reset").clicked() {
                                                self.app.orchestrator.reset_task_action(crate::orchestrator::TaskId(task.id));
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
                            if !self.app.mission_control.interventions.is_empty() {
                                ui.add_space(6.0);
                                ui.separator();
                                for intervention in self.app.mission_control.interventions.iter().rev() {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(egui::RichText::new(format!("#{}", intervention.id)).monospace().small().color(palette.text_muted));
                                        ui.label(egui::RichText::new(&intervention.note).small());
                                        ui.label(egui::RichText::new(format!("— {}", intervention.status)).small().color(palette.text_muted));
                                    });
                                }
                            }
                        });
                    } else {
                        // T2c: Live multi-agent monitoring tab
                        let running_tasks: Vec<&crate::editor::orchestrator_panel::OrchestratorTaskSnapshot> =
                            snapshot.tasks.iter().filter(|t| t.status_label == "Running").collect();

                        if running_tasks.is_empty() {
                            ui.add_space(16.0);
                            ui.vertical_centered(|ui| {
                                ui.label(egui::RichText::new("⚡").size(28.0).color(palette.accent.gamma_multiply(0.7)));
                                ui.add_space(6.0);
                                ui.label(egui::RichText::new("No agents running — launch a mission to see live telemetry").color(palette.text_muted));
                            });
                            ui.add_space(12.0);
                        } else {
                            // Live agent cards with real-time telemetry
                            ui.label(egui::RichText::new(format!("Active Agents: {}", running_tasks.len())).strong().color(palette.accent));
                            ui.add_space(6.0);

                            for task in &running_tasks {
                                ui.push_id(format!("live_{}", task.id), |ui| {
                                    egui::Frame::new()
                                        .fill(palette.bg_secondary)
                                        .stroke(egui::Stroke::new(1.0, palette.accent.gamma_multiply(0.5)))
                                        .corner_radius(egui::CornerRadius::same(6))
                                        .inner_margin(egui::Margin::same(8))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.label(egui::RichText::new("▷").color(palette.accent));
                                                ui.label(egui::RichText::new(format!("#{} {}", task.id, task.title)).small().strong());
                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                    if !task.model_label.is_empty() {
                                                        ui.label(egui::RichText::new(&task.model_label).small().color(palette.text_muted));
                                                    }
                                                });
                                            });

                                            if let Some(thread) = &task.live_thread {
                                                // Current tool (last ToolStarted without matching ToolFinished)
                                                let mut current_tool: Option<&str> = None;
                                                let mut last_status: Option<&str> = None;
                                                for ev in thread.events.iter().rev() {
                                                    match ev.kind {
                                                        crate::orchestrator::worker::WorkerThreadEventKind::ToolStarted => {
                                                            if current_tool.is_none() {
                                                                current_tool = Some(ev.message.as_str());
                                                            }
                                                        }
                                                        crate::orchestrator::worker::WorkerThreadEventKind::ToolFinished => {
                                                            // tool finished, keep looking for an active one
                                                        }
                                                        crate::orchestrator::worker::WorkerThreadEventKind::Status => {
                                                            if last_status.is_none() {
                                                                last_status = Some(ev.message.as_str());
                                                            }
                                                        }
                                                        _ => {}
                                                    }
                                                    if current_tool.is_some() && last_status.is_some() {
                                                        break;
                                                    }
                                                }

                                                ui.horizontal(|ui| {
                                                    if let Some(tool) = current_tool {
                                                        ui.label(egui::RichText::new(format!("⚙ {}", tool)).small().color(palette.warning));
                                                    } else {
                                                        ui.label(egui::RichText::new("⚙ thinking…").small().color(palette.text_muted));
                                                    }
                                                });

                                                if let Some(status) = last_status {
                                                    let truncated = if status.len() > 120 { &status[..120] } else { status };
                                                    ui.label(egui::RichText::new(truncated).small().color(palette.text_muted));
                                                }

                                                // Files changed
                                                if !thread.changed_files.is_empty() {
                                                    ui.horizontal_wrapped(|ui| {
                                                        ui.label(egui::RichText::new("Files:").small().color(palette.text_muted));
                                                        for f in thread.changed_files.iter().take(5) {
                                                            ui.label(egui::RichText::new(f).small().monospace().color(palette.success));
                                                        }
                                                        if thread.changed_files.len() > 5 {
                                                            ui.label(egui::RichText::new(format!("+{}", thread.changed_files.len() - 5)).small().color(palette.text_muted));
                                                        }
                                                    });
                                                }

                                                // Event count
                                                ui.label(egui::RichText::new(format!("{} events · {} notes", thread.events.len(), thread.operator_notes.len())).small().color(palette.text_muted.gamma_multiply(0.7)));
                                            } else {
                                                ui.label(egui::RichText::new("Spawning…").small().color(palette.text_muted));
                                            }
                                        });
                                    ui.add_space(4.0);
                                });
                            }
                        }

                        // Global event feed (latest events across all workers)
                        ui.add_space(8.0);
                        ui.group(|ui| {
                            ui.label(egui::RichText::new("Live Event Feed").small().strong());
                            ui.separator();
                            let mut all_events: Vec<(u64, &crate::orchestrator::worker::WorkerThreadEvent)> = Vec::new();
                            for task in &snapshot.tasks {
                                if let Some(thread) = &task.live_thread {
                                    for ev in thread.events.iter().rev().take(5) {
                                        all_events.push((task.id, ev));
                                    }
                                }
                            }
                            // Show most recent events (already in reverse order per worker)
                            for (task_id, ev) in all_events.iter().take(20) {
                                let (icon, color) = match ev.kind {
                                    crate::orchestrator::worker::WorkerThreadEventKind::ToolStarted => ("⚙", palette.warning),
                                    crate::orchestrator::worker::WorkerThreadEventKind::ToolFinished => ("✔", palette.success),
                                    crate::orchestrator::worker::WorkerThreadEventKind::Status => ("○", palette.text_muted),
                                    crate::orchestrator::worker::WorkerThreadEventKind::FileChange => ("△", palette.accent),
                                    crate::orchestrator::worker::WorkerThreadEventKind::OperatorNote => ("✉", palette.accent),
                                    _ => ("·", palette.text_muted),
                                };
                                let msg = if ev.message.len() > 100 { &ev.message[..100] } else { &ev.message };
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(icon).small().color(color));
                                    ui.label(egui::RichText::new(format!("[#{}]", task_id)).small().monospace().color(palette.text_muted));
                                    ui.label(egui::RichText::new(msg).small());
                                });
                            }
                            if all_events.is_empty() {
                                ui.label(egui::RichText::new("No events yet").small().color(palette.text_muted));
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
