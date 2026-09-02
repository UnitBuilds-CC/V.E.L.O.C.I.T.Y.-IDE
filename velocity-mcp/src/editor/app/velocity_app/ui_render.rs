use eframe::egui;
use std::path::PathBuf;

use crate::editor::agent_ui_render::{render_agent_metrics, RenderSnapshot};
use crate::editor::task_timeline::render_task_timeline;

use super::super::helpers::*;
use super::super::render::TabViewerImpl;
use super::super::types::*;
use super::struct_def::VelocityApp;
use crate::editor::theme::{IdePalette, FONT_CAPTION};

impl VelocityApp {
    pub fn search_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        ui.set_max_width(ui.available_width());
        let suggested_queries: &[&str] = match self.appearance.profile {
            crate::editor::theme::WorkspaceProfile::Coder => &["TODO", "fn ", "struct "],
            crate::editor::theme::WorkspaceProfile::AutomationOperator => {
                &["desktop", "browser", "automation"]
            }
            crate::editor::theme::WorkspaceProfile::MissionControl => {
                &["worker", "task", "approval"]
            }
            crate::editor::theme::WorkspaceProfile::Accessibility => {
                &["theme", "contrast", "scale"]
            }
        };

        egui::Frame::new()
            .inner_margin(egui::Margin::same(10))
            .fill(palette.bg_primary)
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.heading("Search & Replace");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let semantic_label = if self.semantic_search_active {
                                "\u{2295} Semantic"
                            } else {
                                "\u{2295} Literal"
                            };
                            if ui
                                .small_button(
                                    egui::RichText::new(semantic_label)
                                        .size(9.0)
                                        .color(palette.accent),
                                )
                                .clicked()
                            {
                                self.semantic_search_active = !self.semantic_search_active;
                                // Build index on first activation
                                if self.semantic_search_active && self.semantic_index.is_none() {
                                    self.semantic_index =
                                        Some(crate::editor::semantic_search::SemanticIndex::build(
                                            &self.workspace_root,
                                        ));
                                    self.toasts.push(crate::editor::toast::Toast::info(
                                        "Semantic index built",
                                    ));
                                }
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        let hint = if self.semantic_search_active {
                            "Semantic search\u{2026}"
                        } else {
                            "Search\u{2026}"
                        };
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.search_query)
                                .hint_text(hint)
                                .desired_width(ui.available_width() - 10.0),
                        );
                        if response.changed() {
                            // Debounce: defer the walk until typing pauses.
                            self.search_pending_since = Some(std::time::Instant::now());
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            self.search_pending_since = None;
                            if self.semantic_search_active {
                                self.run_semantic_search();
                            } else {
                                self.search_hits = crate::editor::search::project_search(
                                    &self.workspace_root,
                                    &self.search_query,
                                    100,
                                );
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.replace_query)
                                .hint_text("Replace with\u{2026}")
                                .desired_width(ui.available_width() - 90.0),
                        );
                        let can_replace = !self.search_query.is_empty();
                        if ui
                            .add_enabled(can_replace, egui::Button::new("Replace All"))
                            .on_hover_text(
                                "Replace every case-sensitive match across the workspace",
                            )
                            .clicked()
                        {
                            let summary = crate::editor::search::project_replace(
                                &self.workspace_root,
                                &self.search_query,
                                &self.replace_query,
                            );
                            if summary.replacements > 0 {
                                self.toasts
                                    .push(crate::editor::toast::Toast::success(format!(
                                        "Replaced {} occurrence(s) in {} file(s)",
                                        summary.replacements, summary.files_changed
                                    )));
                            } else {
                                self.toasts.push(crate::editor::toast::Toast::info(
                                    "No matching occurrences to replace",
                                ));
                            }
                            // Refresh results against the updated files.
                            self.search_hits = crate::editor::search::project_search(
                                &self.workspace_root,
                                &self.search_query,
                                100,
                            );
                        }
                    });
                    // Run the debounced search once typing has settled (~250ms).
                    if let Some(since) = self.search_pending_since {
                        if since.elapsed() >= std::time::Duration::from_millis(250) {
                            self.search_pending_since = None;
                            if self.semantic_search_active {
                                self.run_semantic_search();
                            } else {
                                self.search_hits = crate::editor::search::project_search(
                                    &self.workspace_root,
                                    &self.search_query,
                                    100,
                                );
                            }
                        } else {
                            ui.ctx()
                                .request_repaint_after(std::time::Duration::from_millis(120));
                        }
                    }
                    ui.separator();

                    let hits = self.search_hits.clone();
                    egui::ScrollArea::vertical()
                        .max_width(ui.available_width())
                        .show(ui, |ui| {
                            ui.set_max_width(ui.available_width());
                            if hits.is_empty() {
                                if self.search_query.is_empty() {
                                    ui.add_space(8.0);
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(
                                            egui::RichText::new("Try:")
                                                .small()
                                                .color(palette.text_muted),
                                        );
                                        for query in suggested_queries {
                                            if ui.small_button(*query).clicked() {
                                                self.search_query = (*query).to_string();
                                                self.search_hits =
                                                    crate::editor::search::project_search(
                                                        &self.workspace_root,
                                                        &self.search_query,
                                                        100,
                                                    );
                                            }
                                        }
                                    });
                                } else {
                                    ui.vertical_centered(|ui| {
                                        ui.add_space(20.0);
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "No results for \"{}\"",
                                                self.search_query
                                            ))
                                            .color(palette.text_muted),
                                        );
                                    });
                                }
                            } else {
                                ui.label(
                                    egui::RichText::new(format!("{} results", hits.len()))
                                        .small()
                                        .color(palette.text_muted),
                                );
                                for hit in &hits {
                                    let icon = crate::editor::search::icon_for_path(&hit.path);
                                    let title = format!(
                                        "{} {} : line {}",
                                        icon,
                                        hit.path.display(),
                                        hit.line
                                    );
                                    ui.group(|ui| {
                                        ui.horizontal(|ui| {
                                            if ui.link(title).clicked() {
                                                let abs_path = self.workspace_root.join(&hit.path);
                                                self.push_nav_location();
                                                self.open_editor(Some(abs_path));
                                                self.pending_cursor_line = Some(hit.line);
                                            }
                                        });
                                        let truncated = if hit.text.len() > 80 {
                                            format!("{}\u{2026}", &hit.text[..80])
                                        } else {
                                            hit.text.clone()
                                        };
                                        ui.label(
                                            egui::RichText::new(truncated).monospace().size(11.0),
                                        );
                                    });
                                }
                            }
                        });
                });
            });
    }

    pub fn browse_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        ui.set_max_width(ui.available_width());

        // Poll for progress updates
        self.browse_state.poll();

        egui::Frame::new()
            .inner_margin(egui::Margin::same(10))
            .fill(palette.bg_primary)
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.heading(
                        egui::RichText::new("\u{1F310} Browse")
                            .size(14.0)
                            .color(palette.accent),
                    );
                    ui.add_space(4.0);

                    // URL input (optional)
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("URL")
                                .size(9.0)
                                .color(palette.text_muted),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.browse_state.url_input)
                                .hint_text("https://... (optional)")
                                .desired_width(ui.available_width() - 4.0),
                        );
                    });

                    // Query input + send
                    ui.horizontal(|ui| {
                        let input_resp = ui.add(
                            egui::TextEdit::singleline(&mut self.browse_state.input)
                                .hint_text("Ask a question...")
                                .desired_width(ui.available_width() - 50.0),
                        );
                        let enter = input_resp.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let send = ui
                            .add_enabled(
                                !self.browse_state.waiting
                                    && !self.browse_state.input.trim().is_empty(),
                                egui::Button::new(egui::RichText::new("Go").size(10.0)),
                            )
                            .clicked();

                        if (enter || send)
                            && !self.browse_state.waiting
                            && !self.browse_state.input.trim().is_empty()
                        {
                            let ws = self.workspace_root.clone();
                            let provider = self.provider;
                            let model = self.selected_model.clone();
                            self.browse_state.send(&ws, provider, &model);
                        }
                    });

                    if self.browse_state.waiting {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(
                                egui::RichText::new("Browsing...")
                                    .size(9.0)
                                    .color(palette.warning),
                            );
                        });
                    }

                    ui.separator();

                    // Messages area
                    egui::ScrollArea::vertical()
                        .id_salt("browse_panel_scroll")
                        .stick_to_bottom(true)
                        .max_width(ui.available_width())
                        .show(ui, |ui| {
                            ui.set_max_width(ui.available_width());
                            for msg in &self.browse_state.messages {
                                match msg.role.as_str() {
                                    "user" => {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(
                                                egui::RichText::new("\u{25B6}")
                                                    .size(9.0)
                                                    .color(palette.accent),
                                            );
                                            ui.label(
                                                egui::RichText::new(&msg.content)
                                                    .size(10.0)
                                                    .strong()
                                                    .color(palette.text),
                                            );
                                        });
                                    }
                                    "assistant" => {
                                        egui::Frame::new()
                                            .fill(palette.bg_secondary)
                                            .corner_radius(6.0)
                                            .inner_margin(6.0)
                                            .show(ui, |ui| {
                                                ui.set_max_width(ui.available_width());
                                                ui.label(
                                                    egui::RichText::new(&msg.content)
                                                        .size(10.0)
                                                        .color(palette.text),
                                                );
                                            });
                                    }
                                    "streaming" => {
                                        egui::Frame::new()
                                            .fill(palette.bg_tertiary)
                                            .corner_radius(6.0)
                                            .inner_margin(6.0)
                                            .stroke(egui::Stroke::new(0.5, palette.accent))
                                            .show(ui, |ui| {
                                                ui.set_max_width(ui.available_width());
                                                ui.label(
                                                    egui::RichText::new(&msg.content)
                                                        .size(10.0)
                                                        .color(palette.text),
                                                );
                                                ui.label(
                                                    egui::RichText::new("\u{2588}")
                                                        .size(10.0)
                                                        .color(palette.accent),
                                                );
                                            });
                                    }
                                    "status" => {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "  \u{2022} {}",
                                                msg.content
                                            ))
                                            .size(9.0)
                                            .italics()
                                            .color(palette.text_muted),
                                        );
                                    }
                                    _ => {}
                                }
                                ui.add_space(3.0);
                            }
                        });
                });
            });
    }

    /// Render checkpoint list in the bottom panel Checkpoints tab.
    pub fn render_checkpoints(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();

        if !self.checkpoint_manager.enabled {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("\u{1f4be}")
                        .size(22.0)
                        .color(palette.text_muted.gamma_multiply(0.6)),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Checkpointing unavailable")
                        .size(11.0)
                        .strong()
                        .color(palette.text),
                );
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new("Initialize a git repository to enable automatic checkpoints")
                        .size(9.0)
                        .color(palette.text_muted),
                );
            });
            return;
        }

        if self.checkpoint_manager.checkpoints.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("\u{1f4be}")
                        .size(22.0)
                        .color(palette.accent.gamma_multiply(0.6)),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("No checkpoints yet")
                        .size(11.0)
                        .strong()
                        .color(palette.text),
                );
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new("Checkpoints are created automatically before agent operations")
                        .size(9.0)
                        .color(palette.text_muted),
                );
            });
            return;
        }

        // Header when checkpoints exist
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("\u{1f4be} Checkpoints")
                    .size(10.0)
                    .strong()
                    .color(palette.accent),
            );
            ui.label(
                egui::RichText::new(format!("({})", self.checkpoint_manager.checkpoints.len()))
                    .size(9.0)
                    .color(palette.text_muted),
            );
        });
        ui.add_space(4.0);

        let mut action: Option<crate::editor::bottom_panel::CheckpointAction> = None;
        for (idx, cp) in self.checkpoint_manager.checkpoints.iter().enumerate() {
            egui::Frame::new()
                .fill(palette.bg_secondary)
                .corner_radius(4.0)
                .inner_margin(6.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(&cp.label)
                                .size(10.0)
                                .strong()
                                .color(palette.text),
                        );
                        ui.label(
                            egui::RichText::new(format!("{} file(s)", cp.files_changed))
                                .size(9.0)
                                .color(palette.text_muted),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .small_button(
                                    egui::RichText::new("\u{2716} Discard")
                                        .size(9.0)
                                        .color(palette.error),
                                )
                                .clicked()
                            {
                                action = Some(
                                    crate::editor::bottom_panel::CheckpointAction::Discard(idx),
                                );
                            }
                            if ui
                                .small_button(
                                    egui::RichText::new("\u{21A9} Restore")
                                        .size(9.0)
                                        .color(palette.success),
                                )
                                .clicked()
                            {
                                action = Some(
                                    crate::editor::bottom_panel::CheckpointAction::Restore(idx),
                                );
                            }
                        });
                    });
                });
            ui.add_space(2.0);
        }

        // Process the action
        if let Some(act) = action {
            match act {
                crate::editor::bottom_panel::CheckpointAction::Restore(idx) => {
                    match self.checkpoint_manager.restore_checkpoint(idx) {
                        Ok(label) => {
                            self.toasts
                                .push(crate::editor::toast::Toast::success(format!(
                                    "Restored: {}",
                                    label
                                )));
                            self.status_message = format!("Checkpoint restored: {}", label);
                            // Refresh git state and reload buffers
                            self.git_state.refresh(&self.workspace_root);
                        }
                        Err(e) => {
                            self.toasts.push(crate::editor::toast::Toast::error(format!(
                                "Restore failed: {}",
                                e
                            )));
                        }
                    }
                }
                crate::editor::bottom_panel::CheckpointAction::Discard(idx) => {
                    match self.checkpoint_manager.discard_checkpoint(idx) {
                        Ok(label) => {
                            self.toasts.push(crate::editor::toast::Toast::info(format!(
                                "Discarded: {}",
                                label
                            )));
                        }
                        Err(e) => {
                            self.toasts.push(crate::editor::toast::Toast::error(format!(
                                "Discard failed: {}",
                                e
                            )));
                        }
                    }
                }
            }
        }
    }

    pub fn handle_global_shortcuts(&mut self, ctx: &egui::Context) {
        if self.command_palette.open
            || self.quick_open.open
            || self.goto_line_open
            || self.goto_symbol_open
            || self.mru.open
        {
            return;
        }
        // Whether the user pressed Tab while an inline suggestion is showing.
        // Detected inside the input closure (which borrows `ctx`) and acted on
        // afterwards so we can pass `ctx` to the accept routine.
        let mut accept_inline = false;
        ctx.input(|i| {
            let cmd = i.modifiers.command;
            let shift = i.modifiers.shift;
            let inline_active = self.inline_suggestions.state
                == crate::editor::inline_suggestions::SuggestionState::Showing;
            if inline_active && i.key_pressed(egui::Key::Tab) {
                accept_inline = true;
            } else if inline_active && i.key_pressed(egui::Key::Escape) {
                self.inline_suggestions.dismiss();
            } else if i.key_pressed(egui::Key::F1) {
                self.show_shortcuts = !self.show_shortcuts;
            } else if cmd && shift && i.key_pressed(egui::Key::P) {
                self.open_command_palette();
            } else if cmd && i.key_pressed(egui::Key::P) {
                self.open_quick_open();
            } else if cmd && shift && i.key_pressed(egui::Key::T) {
                self.reopen_closed_tab();
            } else if cmd && i.key_pressed(egui::Key::G) {
                self.open_goto_line();
            } else if cmd && shift && i.key_pressed(egui::Key::O) {
                self.open_goto_symbol();
            } else if cmd && shift && i.key_pressed(egui::Key::W) {
                // Open workspace switcher (Ctrl+Shift+W).
                self.workspace_switcher_open = !self.workspace_switcher_open;
                self.workspace_switcher_selected = 0;
                self.workspace_switcher_just_opened = true;
            } else if i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft) {
                self.nav_back();
            } else if i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight) {
                self.nav_forward();
            } else if cmd && i.key_pressed(egui::Key::N) {
                self.open_editor(None);
            } else if cmd && i.key_pressed(egui::Key::O) {
                self.open_file_dialog();
            } else if cmd && shift && i.key_pressed(egui::Key::S) {
                self.save_all();
            } else if cmd && i.key_pressed(egui::Key::S) {
                self.save_active();
            } else if cmd && i.key_pressed(egui::Key::B) {
                self.build_active();
            } else if cmd && i.key_pressed(egui::Key::R) {
                self.run_active();
            } else if cmd && i.key_pressed(egui::Key::W) {
                self.close_active_tab();
            } else if cmd && i.key_pressed(egui::Key::Backslash) {
                // Split editor view (Ctrl+\).
                self.split_editor();
            } else if cmd && i.key_pressed(egui::Key::J) {
                self.toggle_panel(TabKind::Chat);
            } else if cmd && i.key_pressed(egui::Key::Backtick) {
                // Toggle bottom panel (Terminal)
                self.bottom_panel_state.collapsed = !self.bottom_panel_state.collapsed;
                if !self.bottom_panel_state.collapsed {
                    self.bottom_panel_state.active_tab = crate::editor::bottom_panel::TAB_TERMINAL;
                }
            } else if cmd && i.key_pressed(egui::Key::E) {
                self.toggle_left_sidebar();
            } else if cmd && shift && i.key_pressed(egui::Key::E) {
                self.toggle_right_sidebar();
            } else if cmd && i.key_pressed(egui::Key::PageDown) {
                self.cycle_tabs(1);
            } else if cmd && i.key_pressed(egui::Key::PageUp) {
                self.cycle_tabs(-1);
            } else if cmd && i.key_pressed(egui::Key::Num1) {
                self.set_work_mode(crate::editor::theme::WorkspaceProfile::Coder);
            } else if cmd && i.key_pressed(egui::Key::Num2) {
                self.set_work_mode(crate::editor::theme::WorkspaceProfile::AutomationOperator);
            } else if cmd && i.key_pressed(egui::Key::Num3) {
                self.set_work_mode(crate::editor::theme::WorkspaceProfile::MissionControl);
            } else if cmd && i.key_pressed(egui::Key::Num4) {
                self.set_work_mode(crate::editor::theme::WorkspaceProfile::Accessibility);
            }
            // --- Panel toggle shortcuts ---
            else if cmd && shift && i.key_pressed(egui::Key::Y) {
                self.toggle_orchestrator();
            } else if cmd && shift && i.key_pressed(egui::Key::F) {
                self.toggle_search();
            } else if cmd && i.key_pressed(egui::Key::Comma) {
                self.toggle_settings();
            } else if cmd && shift && i.key_pressed(egui::Key::I) {
                self.request_inline_suggestion();
            } else if cmd && shift && i.key_pressed(egui::Key::X) {
                self.toggle_extensions();
            } else if cmd && shift && i.key_pressed(egui::Key::A) {
                self.toggle_activity();
            } else if cmd && shift && i.key_pressed(egui::Key::V) {
                self.toggle_voice();
            } else if cmd && i.modifiers.alt && i.key_pressed(egui::Key::R) {
                self.rollback_deploy();
            }
            // --- IDE Editor Shortcuts ---
            else if cmd && i.key_pressed(egui::Key::F) {
                // Find in current buffer
                if let Some(id) = &self.active_tab {
                    if let Some(buf) = self.buffers.get_mut(id) {
                        buf.find_replace.open_find();
                    }
                }
            } else if cmd && i.key_pressed(egui::Key::H) {
                // Find & Replace
                if let Some(id) = &self.active_tab {
                    if let Some(buf) = self.buffers.get_mut(id) {
                        buf.find_replace.open_find_replace();
                    }
                }
            } else if cmd && i.key_pressed(egui::Key::Z) && shift {
                // Redo
                if let Some(id) = &self.active_tab {
                    if let Some(buf) = self.buffers.get_mut(id) {
                        buf.redo();
                    }
                }
            } else if cmd && i.key_pressed(egui::Key::Z) {
                // Undo
                if let Some(id) = &self.active_tab {
                    if let Some(buf) = self.buffers.get_mut(id) {
                        buf.undo();
                    }
                }
            } else if i.key_pressed(egui::Key::Escape) {
                // Close find/replace if open
                if let Some(id) = &self.active_tab {
                    if let Some(buf) = self.buffers.get_mut(id) {
                        if buf.find_replace.visible {
                            buf.find_replace.close();
                        }
                    }
                }
            } else if i.key_pressed(egui::Key::F5) {
                // Start/Continue debugging
                if let Some(dap) = &mut self.dap_client {
                    let _ = dap.continue_execution();
                } else {
                    // Launch a new debug session
                    self.launch_debug_session();
                }
            } else if i.key_pressed(egui::Key::F9) {
                // Toggle breakpoint at current line
                self.toggle_breakpoint_current_line();
            } else if i.key_pressed(egui::Key::F10) {
                // Step over
                if let Some(dap) = &mut self.dap_client {
                    let _ = dap.step_over();
                }
            } else if i.key_pressed(egui::Key::F11) {
                // Step into
                if let Some(dap) = &mut self.dap_client {
                    let _ = dap.step_into();
                }
            } else if shift && i.key_pressed(egui::Key::F12) {
                // Find all references (LSP)
                self.find_references_at_cursor();
            } else if i.key_pressed(egui::Key::F12) {
                // Go to definition (LSP)
                self.goto_definition_at_cursor();
            } else if cmd && i.key_pressed(egui::Key::Space) {
                // Trigger completion
                self.trigger_completion();
            } else if cmd && shift && i.key_pressed(egui::Key::M) {
                // Toggle minimap
                self.show_minimap = !self.show_minimap;
            } else if i.modifiers.alt && i.key_pressed(egui::Key::Z) {
                // Toggle word wrap
                self.word_wrap = !self.word_wrap;
            }
        });
        if accept_inline {
            self.accept_inline_suggestion(ctx);
        }
    }

    // ── Activity Bar Category Panels ──

    fn render_category_header(&self, ui: &mut egui::Ui, palette: IdePalette, title: &str) {
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(title)
                .strong()
                .size(14.0)
                .color(palette.text),
        );
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);
    }

    fn render_sub_tabs(
        &mut self,
        ui: &mut egui::Ui,
        palette: IdePalette,
        category: usize,
        tabs: &[&str],
    ) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (i, tab) in tabs.iter().enumerate() {
                let is_selected = self.activity_sub_panel[category] == i;
                let text_color = if is_selected {
                    palette.text
                } else {
                    palette.text_muted
                };
                let btn = egui::Button::new(
                    egui::RichText::new(*tab).size(11.0).color(text_color),
                )
                .fill(egui::Color32::TRANSPARENT)
                .stroke(egui::Stroke::NONE)
                .min_size(egui::Vec2::new(0.0, 28.0));
                let resp = ui.add(btn);
                // Accent underline for selected tab
                if is_selected {
                    let rect = resp.rect;
                    let underline = egui::Rect::from_min_size(
                        egui::pos2(rect.min.x + 2.0, rect.max.y - 1.5),
                        egui::vec2(rect.width() - 4.0, 2.0),
                    );
                    ui.painter().rect_filled(
                        underline,
                        egui::CornerRadius::same(1),
                        palette.accent,
                    );
                }
                if resp.clicked() {
                    self.activity_sub_panel[category] = i;
                }
            }
        });
        ui.add_space(2.0);
        ui.separator();
        ui.add_space(4.0);
    }

    fn render_files_category(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let tabs = ["Files", "Bookmarks", "Favorites"];
        self.render_sub_tabs(ui, palette, 0, &tabs);
        self.render_category_header(ui, palette, tabs[self.activity_sub_panel[0]]);

        match self.activity_sub_panel[0] {
            0 => self.render_file_tree_subpanel(ui, palette),
            1 => self.render_bookmarks_subpanel(ui, palette),
            2 => self.render_favorites_subpanel(ui, palette),
            _ => {}
        }
    }

    fn render_search_category(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let tabs = ["Search", "Semantic", "Code Graph"];
        self.render_sub_tabs(ui, palette, 1, &tabs);
        self.render_category_header(ui, palette, tabs[self.activity_sub_panel[1]]);

        match self.activity_sub_panel[1] {
            0 => self.search_panel(ui),
            1 => self.render_semantic_search_panel(ui),
            2 => self.render_code_graph_subpanel(ui, palette),
            _ => {}
        }
    }

    fn render_git_category(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let tabs = ["Changes", "Branches", "Commits"];
        self.render_sub_tabs(ui, palette, 2, &tabs);
        self.render_category_header(ui, palette, tabs[self.activity_sub_panel[2]]);

        match self.activity_sub_panel[2] {
            0 => self.render_git_changes_subpanel(ui, palette),
            1 => self.render_branches_subpanel(ui, palette),
            2 => self.render_commits_subpanel(ui, palette),
            _ => {}
        }
    }

    fn render_chat_category(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let tabs = ["Chat", "Voice", "Multimodal"];
        self.render_sub_tabs(ui, palette, 3, &tabs);
        self.render_category_header(ui, palette, tabs[self.activity_sub_panel[3]]);

        match self.activity_sub_panel[3] {
            0 => self.render_chat_subpanel(ui, palette),
            1 => self.render_voice_subpanel(ui, palette),
            2 => self.render_multimodal_subpanel(ui, palette),
            _ => {}
        }
    }

    fn render_build_category(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let tabs = ["Build", "Test", "Deploy", "Debug", "LSP"];
        self.render_sub_tabs(ui, palette, 4, &tabs);
        self.render_category_header(ui, palette, tabs[self.activity_sub_panel[4]]);

        match self.activity_sub_panel[4] {
            0 => self.render_build_subpanel(ui, palette),
            1 => self.render_test_generator_panel(ui),
            2 => self.render_pipeline_panel(ui),
            3 => self.render_debugger_panel(ui),
            4 => self.render_lsp_panel(ui),
            _ => {}
        }
    }

    fn render_agents_category(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let tabs = ["Activity", "Roster", "Orchestration", "Memory", "Timeline", "Metrics"];
        self.render_sub_tabs(ui, palette, 5, &tabs);
        self.render_category_header(ui, palette, tabs[self.activity_sub_panel[5]]);

        match self.activity_sub_panel[5] {
            0 => self.render_activity_panel(ui),
            1 => self.render_agent_roster_subpanel(ui, palette),
            2 => self.render_live_orchestration_panel(ui),
            3 => self.render_agent_memory_panel(ui),
            4 => self.render_timeline_subpanel(ui, palette),
            5 => self.render_mission_metrics_subpanel(ui, palette),
            _ => {}
        }
    }

    fn render_knowledge_category(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let tabs = ["Wiki", "Knowledge Base", "Snippets", "NDA"];
        self.render_sub_tabs(ui, palette, 6, &tabs);
        self.render_category_header(ui, palette, tabs[self.activity_sub_panel[6]]);

        match self.activity_sub_panel[6] {
            0 => self.render_wiki_subpanel(ui, palette),
            1 => self.render_knowledge_panel(ui),
            2 => self.render_snippets_panel(ui),
            3 => self.render_nda_subpanel(ui, palette),
            _ => {}
        }
    }

    fn render_workspace_category(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let tabs = ["Extensions", "Plugins", "Skills", "Team Studio", "Usage", "Governance"];
        self.render_sub_tabs(ui, palette, 7, &tabs);
        self.render_category_header(ui, palette, tabs[self.activity_sub_panel[7]]);

        match self.activity_sub_panel[7] {
            0 => self.render_extensions_panel(ui),
            1 => self.render_plugin_registry_subpanel(ui, palette),
            2 => self.render_skills_subpanel(ui, palette),
            3 => self.render_team_studio(ui),
            4 => self.render_usage_subpanel(ui, palette),
            5 => self.render_governance_panel(ui),
            _ => {}
        }
    }
}

impl eframe::App for VelocityApp {
    fn on_exit(&mut self) {
        self.save_workspace_preferences();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.apply_appearance(&ctx);
        let palette = self.palette();
        self.handle_agent_messages();
        self.handle_global_shortcuts(&ctx);
        self.mru_overlay_ui(&ctx);
        // Pick up any inline suggestion produced by the background model call.
        self.inline_suggestions.poll();
        // Drain incoming LSP diagnostics from language server stdout readers.
        if let Some(lsp) = self.lsp_manager.as_mut() {
            lsp.poll_notifications();
        }
        self.update_diagnostics();
        // Sync diagnostics counts to bottom panel
        self.bottom_panel_state.error_count = self.diagnostics.error_count();
        self.bottom_panel_state.warning_count = self.diagnostics.warning_count();
        // Sync terminal output
        self.bottom_panel_state.terminal_output = self.command_output.clone();

        // Poll open buffers for external on-disk changes (throttled ~5s).
        let external_due = self
            .last_external_check
            .map(|at| at.elapsed() >= std::time::Duration::from_secs(5))
            .unwrap_or(true);
        if external_due {
            self.last_external_check = Some(std::time::Instant::now());
            self.check_external_file_changes();
        }

        // Poll background file-tree build results (non-blocking).
        while let Ok((tree, mtime)) = self.file_tree_rx.try_recv() {
            self.file_tree = Some(tree);
            self.last_tree_mtime = mtime;
            self.last_tree_update = std::time::Instant::now();
            self.tree_build_in_flight = false;
        }

        // Poll background file I/O results (non-blocking).
        self.poll_file_io_results();

        // Poll OS-level file watcher for instant external change detection.
        let watcher_events = if let Some(watcher) = &mut self.file_watcher {
            let evts = watcher.poll();
            watcher.cleanup_stale();
            evts
        } else {
            Vec::new()
        };
        let mut force_refresh = false;
        if !watcher_events.is_empty() {
            force_refresh = true;
            // Reload any open buffers that were externally modified.
            for ev in &watcher_events {
                self.reload_buffer_if_open(&ev.path);
            }
        }

        let now = std::time::Instant::now();
        // Only re-walk the workspace when its top-level mtime changes (a file/dir was
        // added/removed) or, as a safety net for nested changes, every 30 seconds.
        // The actual walk runs on a background thread to keep the UI responsive.
        let needs_rebuild = force_refresh
            || self.file_tree.is_none()
            || now.duration_since(self.last_tree_update) > std::time::Duration::from_secs(30);
        if needs_rebuild && !self.tree_build_in_flight {
            self.tree_build_in_flight = true;
            let root = self.workspace_root.clone();
            let tx = self.file_tree_tx.clone();
            std::thread::spawn(move || {
                let mtime = std::fs::metadata(&root).and_then(|m| m.modified()).ok();
                let tree = build_file_tree(&root);
                let _ = tx.send((tree, mtime));
            });
        }

        let mut cursor_pos = None;
        if let Some(active_id) = &self.active_tab {
            if let Some(buf) = self.buffers.get(active_id) {
                let editor_id = egui::Id::new("code_editor");
                if let Some(state) = egui::widgets::text_edit::TextEditState::load(&ctx, editor_id)
                {
                    if let Some(cursor_range) = state.cursor.char_range() {
                        let char_idx = cursor_range.primary.index.into();
                        let pos = get_cursor_pos(buf.content(), char_idx);
                        // Update current cursor line and column (0-based, for LSP).
                        self.current_cursor_line = pos.0;
                        self.current_cursor_col = pos.1;
                        cursor_pos = Some(pos);
                    }
                }
            }
        }

        // Sync active buffer to LSP server when it has unsaved edits (throttled ~1s).
        let lsp_sync_due = self
            .last_lsp_sync
            .map(|at| at.elapsed() >= std::time::Duration::from_secs(1))
            .unwrap_or(true);
        if lsp_sync_due {
            if let Some(active_id) = &self.active_tab {
                if self.tab_is_dirty(active_id) {
                    if let Some(path) = self.tab_path(active_id).cloned() {
                        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                            if let Some(content) =
                                self.buffers.get(active_id).map(|b| b.content().to_string())
                            {
                                if let Some(lsp) = self.lsp_manager.as_mut() {
                                    lsp.sync_document(ext, &path, &content);
                                }
                                self.last_lsp_sync = Some(std::time::Instant::now());
                            }
                        }
                    }
                }
            }
        }

        let active_change_preview = self.active_change_preview();

        // Keep the application chrome visually distinct from the dock. A fixed
        // height and opaque frame prevent it from disappearing into a dark workspace.
        egui::Panel::top("toolbar")
            .frame(
                egui::Frame::new()
                    .fill(palette.bg_secondary)
                    .stroke(egui::Stroke::new(1.0, palette.border))
                    .inner_margin(egui::Margin::symmetric(10, 4)),
            )
            .show(ui, |ui: &mut egui::Ui| {
                ui.set_min_height(30.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;

                    if self.use_unified_header {
                        ui.label(
                            egui::RichText::new("VELOCITY")
                                .strong()
                                .color(palette.accent),
                        );
                        ui.separator();
                        // A fixed set of compact menus avoids controls wrapping or moving when
                        // profiles change, while every primary surface stays within two clicks.
                        ui.menu_button(egui::RichText::new("Velocity").strong(), |ui| {
                            if ui.button("Command Palette  Ctrl+Shift+P").clicked() {
                                self.open_command_palette();
                                ui.close();
                            }
                            if ui.button("Keyboard Shortcuts  F1").clicked() {
                                self.show_shortcuts = true;
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("Settings  Ctrl+,").clicked() {
                                self.focus_panel(TabKind::Settings);
                                ui.close();
                            }
                        });
                        ui.menu_button("File", |ui| {
                            if ui.button("New File  Ctrl+N").clicked() {
                                self.open_editor(None);
                                ui.close();
                            }
                            if ui.button("Open File\u{2026}  Ctrl+O").clicked() {
                                self.open_file_dialog();
                                ui.close();
                            }
                            if ui.button("Quick Open  Ctrl+P").clicked() {
                                self.open_quick_open();
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("Save  Ctrl+S").clicked() {
                                self.save_active();
                                ui.close();
                            }
                            if ui.button("Save All  Ctrl+Shift+S").clicked() {
                                self.save_all();
                                ui.close();
                            }
                        });
                        ui.menu_button("Navigate", |ui| {
                            if ui.button("Chat  Ctrl+J").clicked() {
                                self.focus_panel(TabKind::Chat);
                                ui.close();
                            }
                            if ui.button("Search  Ctrl+Shift+F").clicked() {
                                self.focus_panel(TabKind::Search);
                                ui.close();
                            }
                            if ui
                                .button("Research browser")
                                .on_hover_text("Open the native browser and research workspace.")
                                .clicked()
                            {
                                self.open_browse_workspace();
                                ui.close();
                            }
                            if ui
                                .button("Review changes")
                                .on_hover_text("Inspect uncommitted work and recent Git history.")
                                .clicked()
                            {
                                self.focus_panel(TabKind::Changes);
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("Output  Ctrl+`").clicked() {
                                self.focus_panel(TabKind::Output);
                                ui.close();
                            }
                            if ui.button("Terminal").clicked() {
                                self.focus_panel(TabKind::Terminal);
                                ui.close();
                            }
                        });
                        ui.menu_button("Build", |ui| {
                            if ui.button("Build  Ctrl+B").clicked() {
                                self.build_active();
                                ui.close();
                            }
                            if ui.button("Run  Ctrl+R").clicked() {
                                self.run_active();
                                ui.close();
                            }
                            ui.separator();
                            for (label, panel) in [
                                ("Test generator", TabKind::TestGenerator),
                                ("Test coverage", TabKind::Coverage),
                                ("Deploy pipeline", TabKind::Pipeline),
                                ("Debugger", TabKind::Debugger),
                                ("Language servers", TabKind::LanguageServers),
                                ("Snippets", TabKind::Snippets),
                                ("Inline suggestions", TabKind::InlineSuggestions),
                                ("Precompiled cache", TabKind::PrecompCache),
                            ] {
                                if ui.button(label).clicked() {
                                    self.focus_panel(panel);
                                    ui.close();
                                }
                            }
                        });
                        ui.menu_button("Agents", |ui| {
                            for (label, panel) in [
                                ("Live activity", TabKind::Activity),
                                ("Agent roster", TabKind::Agents),
                                ("Background agents", TabKind::BackgroundAgents),
                                ("Live orchestration", TabKind::LiveOrchestration),
                                ("Task queue", TabKind::Queue),
                                ("Timeline", TabKind::Timeline),
                                ("Mission metrics", TabKind::Metrics),
                                ("Conflict resolver", TabKind::ConflictResolver),
                                ("Improvement engine", TabKind::ImprovementEngine),
                                ("Continuity ledger", TabKind::ContinuationLedger),
                            ] {
                                if ui.button(label).clicked() {
                                    self.focus_panel(panel);
                                    ui.close();
                                }
                            }
                        });
                        ui.menu_button("Knowledge", |ui| {
                            for (label, panel) in [
                                ("Knowledge base", TabKind::Knowledge),
                                ("Wiki", TabKind::Wiki),
                                ("Code graph", TabKind::Graph),
                                ("Semantic search", TabKind::SemanticSearch),
                                ("Bookmarks", TabKind::Bookmarks),
                                ("Favorites", TabKind::Favorites),
                                ("Agent memory", TabKind::AgentMemory),
                                ("Shared memory", TabKind::SharedMemory),
                                ("Persistent memory", TabKind::PersistentMemory),
                                ("Recent changes", TabKind::Changes),
                            ] {
                                if ui.button(label).clicked() {
                                    self.focus_panel(panel);
                                    ui.close();
                                }
                            }
                        });
                        ui.menu_button("Automate", |ui| {
                            for (label, panel) in [
                                ("Workflows", TabKind::Workflows),
                                ("Triggers", TabKind::Triggers),
                                ("Automation flows", TabKind::Flows),
                                ("Automation targets", TabKind::Targets),
                                ("Execution logs", TabKind::Logs),
                                ("Recordings", TabKind::Recordings),
                                ("Voice commands", TabKind::Voice),
                                ("Multimodal attachments", TabKind::Multimodal),
                                ("Accessibility audit", TabKind::AccessibilityAudit),
                                ("Governance", TabKind::Governance),
                            ] {
                                if ui.button(label).clicked() {
                                    self.focus_panel(panel);
                                    ui.close();
                                }
                            }
                        });
                        ui.menu_button("Workspace", |ui| {
                            ui.label(
                                egui::RichText::new("Workspace tools")
                                    .small()
                                    .color(palette.text_muted),
                            );
                            ui.separator();
                            for (label, panel) in [
                                ("Extensions", TabKind::Extensions),
                                ("Plugin registry", TabKind::PluginRegistry),
                                ("Skills", TabKind::SkillFiles),
                                ("Collaboration", TabKind::Collaboration),
                                ("Peers", TabKind::Peers),
                                ("Usage", TabKind::Usage),
                            ] {
                                if ui.button(label).clicked() {
                                    self.focus_panel(panel);
                                    ui.close();
                                }
                            }
                            ui.separator();
                            if ui.button("New NDA document").clicked() {
                                self.new_nda_document();
                                ui.close();
                            }
                            if ui.button("Import active file to NDA").clicked() {
                                self.import_file_to_nda();
                                ui.close();
                            }
                            if ui.button("Open NDA browser viewer").clicked() {
                                self.open_nda_viewer();
                                ui.close();
                            }
                        });
                        let active_mode = self.appearance.profile;
                        ui.menu_button(
                            egui::RichText::new(format!(
                                "Layouts: {} {} \u{25be}",
                                active_mode.glyph(),
                                active_mode.short_label()
                            ))
                            .color(palette.accent),
                            |ui| {
                                ui.label(
                                    egui::RichText::new("Workspaces")
                                        .small()
                                        .color(palette.text_muted),
                                );
                                // Build and Mission are the product-level workspaces.
                                // Specialized profiles remain available without competing
                                // with the default mental model.
                                for mode in [
                                    crate::editor::theme::WorkspaceProfile::Coder,
                                    crate::editor::theme::WorkspaceProfile::MissionControl,
                                ] {
                                    let selected = mode == active_mode;
                                    let label = mode.short_label();
                                    if ui
                                        .selectable_label(
                                            selected,
                                            format!(
                                                "{} {}  {}",
                                                mode.glyph(),
                                                label,
                                                mode.shortcut_hint()
                                            ),
                                        )
                                        .on_hover_text(mode.description())
                                        .clicked()
                                    {
                                        self.set_work_mode(mode);
                                        ui.close();
                                    }
                                }
                                ui.separator();
                                ui.label(
                                    egui::RichText::new("Specialized layouts")
                                        .small()
                                        .color(palette.text_muted),
                                );
                                for mode in [
                                    crate::editor::theme::WorkspaceProfile::AutomationOperator,
                                    crate::editor::theme::WorkspaceProfile::Accessibility,
                                ] {
                                    if ui
                                        .selectable_label(
                                            mode == active_mode,
                                            format!(
                                                "{} {}  {}",
                                                mode.glyph(),
                                                mode.label(),
                                                mode.shortcut_hint()
                                            ),
                                        )
                                        .on_hover_text(mode.description())
                                        .clicked()
                                    {
                                        self.set_work_mode(mode);
                                        ui.close();
                                    }
                                }
                            },
                        );
                    } else {
                        // `use_unified_header` is always true; this branch is kept
                        // as a placeholder for a potential lightweight header mode.
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        // Icon-only sidebar toggles + workspace switcher for a cleaner toolbar
                        let toggle_size = egui::vec2(26.0, 22.0);
                        // Right panel toggle
                        {
                            let icon = if self.right_sidebar_visible { "\u{25a3}" } else { "\u{25a2}" };
                            let hint = if self.right_sidebar_visible {
                                "Hide right panel  (Ctrl+Shift+E)"
                            } else {
                                "Show right panel  (Ctrl+Shift+E)"
                            };
                            let btn_rect = egui::Rect::from_min_size(ui.cursor().min, toggle_size);
                            let btn_id = ui.make_persistent_id("toggle_right");
                            let resp = ui.interact(btn_rect, btn_id, egui::Sense::click());
                            let fill = if resp.hovered() { palette.surface_hover } else { egui::Color32::TRANSPARENT };
                            let clicked = resp.clicked();
                            ui.painter().rect_filled(btn_rect, egui::CornerRadius::same(4), fill);
                            ui.painter().text(
                                btn_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                icon,
                                egui::FontId::proportional(13.0),
                                if self.right_sidebar_visible { palette.accent } else { palette.text_muted },
                            );
                            resp.on_hover_text(hint);
                            if clicked { self.toggle_right_sidebar(); }
                            ui.advance_cursor_after_rect(btn_rect);
                        }
                        // Left panel toggle
                        {
                            let icon = if self.left_sidebar_visible { "\u{25a3}" } else { "\u{25a2}" };
                            let hint = if self.left_sidebar_visible {
                                "Hide sidebar  (Ctrl+E)"
                            } else {
                                "Show sidebar  (Ctrl+E)"
                            };
                            let btn_rect = egui::Rect::from_min_size(ui.cursor().min, toggle_size);
                            let btn_id = ui.make_persistent_id("toggle_left");
                            let resp = ui.interact(btn_rect, btn_id, egui::Sense::click());
                            let fill = if resp.hovered() { palette.surface_hover } else { egui::Color32::TRANSPARENT };
                            let clicked = resp.clicked();
                            ui.painter().rect_filled(btn_rect, egui::CornerRadius::same(4), fill);
                            ui.painter().text(
                                btn_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                icon,
                                egui::FontId::proportional(13.0),
                                if self.left_sidebar_visible { palette.accent } else { palette.text_muted },
                            );
                            resp.on_hover_text(hint);
                            if clicked { self.toggle_left_sidebar(); }
                            ui.advance_cursor_after_rect(btn_rect);
                        }
                        ui.add_space(4.0);
                        // Workspace switcher — compact folder icon button
                        {
                            let btn_rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(28.0, 22.0));
                            let btn_id = ui.make_persistent_id("ws_switcher");
                            let resp = ui.interact(btn_rect, btn_id, egui::Sense::click());
                            let fill = if resp.hovered() { palette.surface_hover } else { egui::Color32::TRANSPARENT };
                            let clicked = resp.clicked();
                            ui.painter().rect_filled(btn_rect, egui::CornerRadius::same(4), fill);
                            ui.painter().text(
                                btn_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "\u{1f4c2}",
                                egui::FontId::proportional(12.0),
                                palette.text_muted,
                            );
                            resp.on_hover_text("Switch workspace  (Ctrl+Shift+W)");
                            if clicked {
                                self.workspace_switcher_open = true;
                                self.workspace_switcher_selected = 0;
                                self.workspace_switcher_just_opened = true;
                            }
                            ui.advance_cursor_after_rect(btn_rect);
                        }
                    });
                });
                ui.add_space(2.0);
            });

        // Symbol enclosing the cursor — used to highlight the Outline entry and
        // to drive the symbol context panel below.
        let mut active_symbol = None;
        if let Some(active_id) = &self.active_tab {
            if let Some(buf) = self.buffers.get(active_id) {
                if let Some((line, _col)) = cursor_pos {
                    active_symbol = get_active_symbol(buf.content(), line);
                }
            }
        }

        // Breadcrumb strip: workspace-relative path + enclosing symbol for the
        // active editor, giving quick orientation in deep trees.
        let active_editor_path: Option<PathBuf> = self
            .active_tab
            .as_ref()
            .and_then(|id| self.tabs.iter().find(|t| &t.id == id))
            .and_then(|t| t.editor_path().cloned());
        if self.show_breadcrumbs {
            if let Some(path) = active_editor_path {
                let ws_root = self.workspace_root.clone();
                let symbol_for_click = active_symbol.clone();
                egui::Panel::top("breadcrumb")
                    .frame(
                        egui::Frame::new()
                            .fill(palette.bg_secondary)
                            .stroke(egui::Stroke::new(0.0, palette.border))
                            .inner_margin(egui::Margin::symmetric(8, 0)),
                    )
                    .show(ui, |ui: &mut egui::Ui| {
                    ui.add_space(2.0);
                    ui.horizontal(|ui: &mut egui::Ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let rel = path.strip_prefix(&ws_root).unwrap_or(&path);
                        let components: Vec<String> = rel
                            .components()
                            .map(|c| c.as_os_str().to_string_lossy().to_string())
                            .collect();
                        let last = components.len().saturating_sub(1);
                        for (i, comp) in components.iter().enumerate() {
                            if i > 0 {
                                ui.label(
                                    egui::RichText::new("\u{203a}")
                                        .color(palette.text_muted)
                                        .weak(),
                                );
                            }
                            if i == last {
                                ui.label(egui::RichText::new(comp).color(palette.text).strong());
                            } else {
                                let seg_resp = ui.label(
                                    egui::RichText::new(comp).color(palette.text_muted),
                                );
                                if seg_resp.hovered() {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                let seg_clicked = seg_resp.clicked();
                                seg_resp.on_hover_text(format!("Reveal {} in file tree", comp));
                                if seg_clicked {
                                    // Set file tree filter to show this path component.
                                    let filter_path: String = components[..=i].join("/");
                                    self.file_tree_filter = filter_path;
                                    // Ensure left sidebar is visible to show filtered tree.
                                    if !self.left_sidebar_visible {
                                        self.toggle_left_sidebar();
                                    }
                                }
                            }
                        }
                        if let Some(symbol) = &symbol_for_click {
                            ui.label(
                                egui::RichText::new("\u{203a}")
                                    .color(palette.text_muted)
                                    .weak(),
                            );
                            if ui
                                .link(egui::RichText::new(symbol).color(palette.accent))
                                .on_hover_text("Re-center on this symbol")
                                .clicked()
                            {
                                self.jump_to_symbol_name(symbol);
                            }
                        }
                    });
                    ui.add_space(2.0);
                });
            } // end show_breadcrumbs
        }

        if self.left_sidebar_visible {
            // Activity bar (narrow icon strip on far left)
            egui::Panel::left("activity_bar")
                .resizable(false)
                .default_size(56.0)
                .frame(
                    egui::Frame::new()
                        .fill(palette.bg_primary)
                        .stroke(egui::Stroke::new(1.0, palette.border)),
                )
                .show(ui, |ui: &mut egui::Ui| {
                    ui.set_min_width(56.0);
                    ui.vertical_centered(|ui| {
                        ui.add_space(8.0);

                        // Activity bar icons - 8 main categories with recognizable glyphs and labels
                        let activities = [
                            ("\u{2263}", "Files", "Files", "Ctrl+E"),
                            ("\u{2315}", "Search", "Search", "Ctrl+Shift+F"),
                            ("\u{2387}", "Git", "Git", "Ctrl+G"),
                            ("\u{25ef}", "Chat", "Chat", "Ctrl+J"),
                            ("\u{2699}", "Build", "Build", "Ctrl+B"),
                            ("\u{229b}", "Agents", "Agents", "Ctrl+D"),
                            ("\u{25a1}", "Knowledge", "Know", "Ctrl+K"),
                            ("\u{229e}", "Workspace", "Work", "Ctrl+Shift+X"),
                        ];

                        for (i, (icon, label, short_label, shortcut)) in activities.iter().enumerate() {
                            let is_selected = self.activity_bar_selection == i;
                            let icon_size = egui::vec2(48.0, 48.0);
                            let rect = egui::Rect::from_min_size(ui.cursor().min, icon_size);
                            ui.advance_cursor_after_rect(rect);
                            let id = ui.make_persistent_id(i);
                            let interact_resp = ui.interact(
                                rect,
                                id,
                                egui::Sense::click(),
                            );
                            let hovered = interact_resp.hovered();

                            // Background fill based on state
                            if is_selected {
                                ui.painter().rect_filled(
                                    rect,
                                    egui::CornerRadius::same(6),
                                    palette.accent.gamma_multiply(0.12),
                                );
                            } else if hovered {
                                ui.painter().rect_filled(
                                    rect,
                                    egui::CornerRadius::same(6),
                                    palette.surface_hover,
                                );
                            }

                            // Selection indicator bar on the left edge
                            if is_selected {
                                let bar_rect = egui::Rect::from_min_size(
                                    egui::pos2(rect.min.x, rect.min.y + 4.0),
                                    egui::vec2(2.5, rect.height() - 8.0),
                                );
                                ui.painter().rect_filled(bar_rect, 1.0, palette.accent);
                            }

                            // Icon glyph centered in upper portion
                            let icon_color = if is_selected {
                                palette.accent
                            } else if hovered {
                                palette.text
                            } else {
                                palette.text_muted
                            };
                            let icon_pos = egui::pos2(
                                rect.center().x,
                                rect.min.y + 14.0,
                            );
                            ui.painter().text(
                                icon_pos,
                                egui::Align2::CENTER_CENTER,
                                *icon,
                                egui::FontId::proportional(15.0),
                                icon_color,
                            );

                            // Small text label below icon
                            let label_color = if is_selected {
                                palette.accent
                            } else if hovered {
                                palette.text
                            } else {
                                palette.text_muted.gamma_multiply(0.7)
                            };
                            let label_pos = egui::pos2(
                                rect.center().x,
                                rect.min.y + 34.0,
                            );
                            ui.painter().text(
                                label_pos,
                                egui::Align2::CENTER_CENTER,
                                *short_label,
                                egui::FontId::proportional(8.5),
                                label_color,
                            );

                            if interact_resp.clicked() {
                                self.activity_bar_selection = i;
                            }
                            interact_resp.on_hover_text(format!("{}  ({})", label, shortcut));
                            ui.add_space(1.0);
                        }

                        // Spacer to push user identity to bottom
                        ui.add_space(ui.available_height() - 90.0);

                        // Settings gear icon
                        {
                            let gear_size = egui::vec2(32.0, 28.0);
                            let gear_rect = egui::Rect::from_min_size(ui.cursor().min, gear_size);
                            let gear_id = ui.make_persistent_id("activity_gear");
                            let gear_resp = ui.interact(gear_rect, gear_id, egui::Sense::click());
                            let gear_fill = if gear_resp.hovered() { palette.surface_hover } else { egui::Color32::TRANSPARENT };
                            let gear_clicked = gear_resp.clicked();
                            ui.painter().rect_filled(gear_rect, egui::CornerRadius::same(6), gear_fill);
                            ui.painter().text(
                                gear_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "\u{2699}",
                                egui::FontId::proportional(14.0),
                                if gear_resp.hovered() { palette.text } else { palette.text_muted.gamma_multiply(0.6) },
                            );
                            gear_resp.on_hover_text("Settings  (Ctrl+,)");
                            if gear_clicked {
                                self.focus_panel(TabKind::Settings);
                            }
                            ui.advance_cursor_after_rect(gear_rect);
                        }

                        // User identity at bottom of activity bar — derived from workspace name
                        let workspace_name = self
                            .workspace_root
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("V");
                        let initials: String = workspace_name
                            .chars()
                            .filter(|c| c.is_uppercase())
                            .take(2)
                            .collect();
                        let display_initials = if initials.len() >= 2 {
                            initials
                        } else {
                            workspace_name.chars().take(2).collect::<String>()
                        };
                        egui::Frame::new()
                            .fill(palette.accent.gamma_multiply(0.2))
                            .corner_radius(egui::CornerRadius::same(14))
                            .inner_margin(egui::Margin::same(6))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(display_initials)
                                        .size(FONT_CAPTION)
                                        .strong()
                                        .color(palette.accent),
                                );
                            });
                    });
                });

            // Main sidebar panel (content changes based on activity bar selection)
            let panel_response = egui::Panel::left("left_sidebar")
                .resizable(true)
                .default_size(self.left_sidebar_width)
                .show(ui, |ui: &mut egui::Ui| {
                    // Clamp sidebar width to prevent runaway expansion
                    let w = ui.available_width().clamp(180.0, 420.0);
                    ui.set_max_width(w);
                    self.left_sidebar_width = w;

                    // Workspace header: name + branch at top of sidebar
                    {
                        let ws_name = self.workspace_root.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("Workspace");
                        let branch = get_git_branch(&self.workspace_root);
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(ws_name)
                                    .strong()
                                    .size(13.0)
                                    .color(palette.text),
                            );
                            if let Some(br) = &branch {
                                ui.label(
                                    egui::RichText::new(format!("\u{2387} {}", br))
                                        .size(9.0)
                                        .color(palette.text_muted),
                                );
                            }
                        });
                        ui.add_space(2.0);
                        ui.separator();
                        ui.add_space(2.0);
                    }

                    // Render content based on activity bar selection
                    match self.activity_bar_selection {
                        0 => self.render_files_category(ui, palette),
                        1 => self.render_search_category(ui, palette),
                        2 => self.render_git_category(ui, palette),
                        3 => self.render_chat_category(ui, palette),
                        4 => self.render_build_category(ui, palette),
                        5 => self.render_agents_category(ui, palette),
                        6 => self.render_knowledge_category(ui, palette),
                        7 => self.render_workspace_category(ui, palette),
                        _ => self.render_files_category(ui, palette),
                    }
                });
            self.left_sidebar_width = panel_response.response.rect.width().clamp(180.0, 420.0);
        }

        if self.right_sidebar_visible {
            // Fetch the (TTL-cached) site map once, outside the panel closure, so the
            // symbol-context section below never re-reads index.json per frame.
            let panel_site_map = self.cached_site_map(std::time::Duration::from_secs(3));
            let right_mode_cfg =
                crate::editor::mode_config::mode_config_for(self.appearance.profile);
            let right_panels = right_mode_cfg.right_panels();
            let panel_response = egui::Panel::right("right_sidebar")
                .resizable(true)
                .default_size(self.right_sidebar_width)
                .frame(
                    egui::Frame::new()
                        .fill(palette.bg_secondary)
                        .stroke(egui::Stroke::new(1.0, palette.border))
                        .inner_margin(egui::Margin::symmetric(6, 4)),
                )
                .show(ui, |ui: &mut egui::Ui| {
                    self.right_sidebar_width = ui.available_width().clamp(220.0, 600.0);
                    ui.add_space(2.0);

                    // Mode-specific right panel header
                    ui.horizontal(|ui| {
                        for panel in right_panels {
                            ui.label(
                                egui::RichText::new(format!("{} {}", panel.icon, panel.label))
                                    .size(9.0)
                                    .strong()
                                    .color(palette.accent),
                            );
                        }
                    });
                    ui.add_space(2.0);
                    ui.separator();
                    ui.add_space(4.0);

                    self.smart_sidebar.clear();
                    if self.build_errors_count > 0 {
                        self.smart_sidebar.add_diagnostic(0, true, "workspace", 0, 0, "Build errors require attention");
                    }

                    // -- Active changes (collapsible) --
                    if let Some(change_preview) = &active_change_preview {
                        self.smart_sidebar.add_quick_action(0, "Review current changes", &change_preview.file_label, 0);
                        let changes_header = if self.right_changes_collapsed { "\u{25b8} Changes" } else { "\u{25be} Changes" };
                        if ui.add(egui::Button::new(egui::RichText::new(changes_header).size(10.0).strong().color(palette.warning)).frame(false)).clicked() {
                            self.right_changes_collapsed = !self.right_changes_collapsed;
                        }
                        if !self.right_changes_collapsed {
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(&change_preview.file_label).strong().color(palette.warning));
                                    ui.label(egui::RichText::new(format!("+{} -{}", change_preview.added_lines, change_preview.removed_lines)).small().color(palette.text_muted));
                                });
                                ui.horizontal(|ui| {
                                    if ui.small_button("Save").clicked() { self.save_active(); }
                                    if ui.small_button("Revert").clicked() { self.revert_active_from_disk(); }
                                    if ui.small_button("Stage").clicked() { self.stage_active_file(); }
                                    if ui.small_button("Diff").clicked() { self.show_full_diff = true; }
                                });
                                // Show a short diff preview inline
                                if !change_preview.preview.is_empty() {
                                    ui.add_space(4.0);
                                    egui::Frame::new()
                                        .fill(palette.bg_secondary)
                                        .corner_radius(egui::CornerRadius::same(3))
                                        .inner_margin(egui::Margin::symmetric(6, 4))
                                        .show(ui, |ui| {
                                            egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                                                ui.label(egui::RichText::new(change_preview.preview.as_str()).monospace().size(10.0).color(palette.text_muted));
                                            });
                                        });
                                }
                            });
                        }
                        ui.separator();
                    }

                    // -- Symbol context (collapsible) --
                    // Recompute callers/deps only when the active symbol changes; the
                    // site map itself is cached (TTL) so we never re-read index.json per frame.
                    if self.cached_relation_symbol != active_symbol {
                        if let Some(symbol) = &active_symbol {
                            if let Some(sm) = &panel_site_map {
                                let symbol_hash = hash_str(symbol);
                                let resolve = |hashes: Vec<u64>| {
                                    let mut names: Vec<String> = hashes
                                        .iter()
                                        .map(|h| {
                                            sm.resolve_string(*h)
                                                .filter(|s| !s.is_empty())
                                                .unwrap_or_else(|| format!("{:016x}", h))
                                        })
                                        .filter(|s| !s.contains('/') && !s.contains('\\'))
                                        .collect();
                                    names.sort();
                                    names.dedup();
                                    names
                                };
                                self.cached_callers = resolve(sm.get_callers(symbol_hash));
                                self.cached_deps = resolve(sm.get_dependencies(symbol_hash));
                            } else {
                                self.cached_callers.clear();
                                self.cached_deps.clear();
                            }
                        } else {
                            self.cached_callers.clear();
                            self.cached_deps.clear();
                        }
                        self.cached_relation_symbol = active_symbol.clone();
                    }
                    if let Some(symbol) = &active_symbol {
                        self.smart_sidebar.add_symbol(0, symbol, "active-buffer", cursor_pos.map(|(line, _)| line as u32).unwrap_or(0), 0);
                        let sym_header = if self.right_symbol_collapsed {
                            format!("\u{25b8} {}()", symbol)
                        } else {
                            format!("\u{25be} {}()", symbol)
                        };
                        if ui.add(egui::Button::new(egui::RichText::new(&sym_header).size(10.0).strong().color(palette.accent)).frame(false)).clicked() {
                            self.right_symbol_collapsed = !self.right_symbol_collapsed;
                        }
                        if !self.right_symbol_collapsed {
                            if !self.cached_callers.is_empty() {
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new(format!("Callers ({})", self.cached_callers.len()))
                                        .small()
                                        .strong()
                                        .color(palette.text_muted),
                                );
                                for name in self.cached_callers.clone() {
                                    if ui
                                        .link(egui::RichText::new(format!("\u{2192} {}", name)).size(11.0))
                                        .clicked()
                                    {
                                        self.jump_to_symbol_name(&name);
                                    }
                                }
                            }
                            if !self.cached_deps.is_empty() {
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new(format!("Dependencies ({})", self.cached_deps.len()))
                                        .small()
                                        .strong()
                                        .color(palette.text_muted),
                                );
                                for name in self.cached_deps.clone() {
                                    if ui
                                        .link(egui::RichText::new(format!("\u{2192} {}", name)).size(11.0))
                                        .clicked()
                                    {
                                        self.jump_to_symbol_name(&name);
                                    }
                                }
                            }
                        }
                    } else {
                        ui.vertical_centered(|ui| {
                            ui.add_space(24.0);
                            ui.label(
                                egui::RichText::new("\u{25cc}")
                                    .size(28.0)
                                    .color(palette.text_muted.gamma_multiply(0.6)),
                            );
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(
                                    "Place your cursor on a symbol\nto inspect its callers and dependencies",
                                )
                                .small()
                                .color(palette.text_muted),
                            );
                        });
                    }

                    ui.separator();
                    let sidebar_snapshot = crate::editor::smart_sidebar::SmartSidebarSnapshot::new(&self.smart_sidebar);
                    crate::editor::smart_sidebar::render_smart_sidebar(ui, &sidebar_snapshot, palette);
                });
            self.right_sidebar_width = panel_response.response.rect.width().clamp(220.0, 600.0);
        }

        let branch = get_git_branch(&self.workspace_root);
        let build_ok = self.build_errors_count == 0;
        let model_name = if self.selected_model.is_empty() {
            "default"
        } else {
            &self.selected_model
        };
        let sb_actions = crate::editor::status_bar::StatusBar::show(
            ui,
            palette,
            branch.as_deref(),
            cursor_pos,
            build_ok,
            &self.status_message,
            &format!(
                "{} {}",
                self.appearance.profile.glyph(),
                self.appearance.profile.short_label()
            ),
            self.provider.label(),
            model_name,
        );

        // Handle status bar click actions.
        if sb_actions.clicked_mode {
            // Cycle to the next workspace profile.
            let current = self.appearance.profile;
            let all = crate::editor::theme::WorkspaceProfile::ALL;
            let next_idx = all
                .iter()
                .position(|&p| p == current)
                .map(|i| (i + 1) % all.len())
                .unwrap_or(0);
            self.set_work_mode(all[next_idx]);
            self.apply_appearance(ui.ctx());
        }
        if sb_actions.clicked_build {
            // Open diagnostics (Problems tab in bottom panel).
            self.bottom_panel_state.collapsed = false;
            self.bottom_panel_state.active_tab = crate::editor::bottom_panel::TAB_PROBLEMS;
        }
        if sb_actions.clicked_position {
            // Open go-to-line dialog.
            self.goto_line_open = true;
            self.goto_line_just_opened = true;
            self.goto_line_input.clear();
        }
        if sb_actions.clicked_provider {
            // Open settings tab.
            self.toggle_panel(TabKind::Settings);
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(palette.bg_primary))
            .show(ui, |ui| {
            if let Some(mut dock_state) = self.dock_state.take() {
                let mut viewer = TabViewerImpl { app: self };
                let mut dock_style = egui_dock::Style::from_egui(ui.style().as_ref());
                dock_style.tab_bar.bg_fill = palette.bg_secondary;
                dock_style.tab_bar.inner_margin = egui::Margin::symmetric(4, 2);
                dock_style.tab_bar.hline_color = palette.border;
                dock_style.tab.active.bg_fill = palette.bg_primary;
                dock_style.tab.active.text_color = palette.text;
                dock_style.tab.inactive.bg_fill = palette.bg_secondary;
                dock_style.tab.inactive.text_color = palette.text_muted;
                dock_style.tab.hovered.bg_fill = palette.surface_hover;
                dock_style.separator.width = 2.0;
                dock_style.separator.color_idle = palette.border;
                dock_style.separator.color_hovered = palette.accent.gamma_multiply(0.3);
                dock_style.main_surface_border_stroke = egui::Stroke::new(0.0, palette.border);
                egui_dock::DockArea::new(&mut dock_state)
                    .style(dock_style)
                    .show_inside(ui, &mut viewer);
                self.dock_state = Some(dock_state);
            } else if self.tabs.is_empty() {
                // Welcome screen when no tabs are open
                let ws_name = self.workspace_root.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Workspace")
                    .to_string();
                let ws_path = self.workspace_root.display().to_string();
                let branch = get_git_branch(&self.workspace_root);
                let dir_count = self.file_tree.as_ref()
                    .and_then(|t| t.children.as_ref())
                    .map(|c| c.len())
                    .unwrap_or(0);
                // Collect recently closed file paths for the welcome screen
                let recent_paths: Vec<String> = self.closed_editor_paths.iter()
                    .take(5)
                    .map(|p| {
                        p.strip_prefix(&self.workspace_root)
                            .unwrap_or(p)
                            .display()
                            .to_string()
                    })
                    .collect();

                egui::ScrollArea::vertical()
                    .id_salt("welcome_scroll")
                    .show(ui, |ui| {
                        ui.add_space(ui.available_height() * 0.08);

                        // Hero section: brand + tagline
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("VELOCITY")
                                    .size(32.0)
                                    .strong()
                                    .color(palette.accent),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new("AI-first development environment")
                                    .size(13.0)
                                    .color(palette.text_muted),
                            );
                        });
                        ui.add_space(28.0);

                        // Workspace info card
                        let card_w = 360.0;
                        egui::Frame::new()
                            .fill(palette.bg_secondary)
                            .stroke(egui::Stroke::new(0.5, palette.border))
                            .corner_radius(egui::CornerRadius::same(10))
                            .inner_margin(egui::Margin::symmetric(16, 12))
                            .show(ui, |ui| {
                                ui.set_max_width(card_w);
                                ui.horizontal(|ui| {
                                    // Folder icon
                                    ui.label(
                                        egui::RichText::new("\u{1f4c1}")
                                            .size(18.0)
                                            .color(palette.accent),
                                    );
                                    ui.add_space(6.0);
                                    ui.vertical(|ui| {
                                        ui.label(
                                            egui::RichText::new(ws_name)
                                                .strong()
                                                .size(14.0)
                                                .color(palette.text),
                                        );
                                        ui.label(
                                            egui::RichText::new(&ws_path)
                                                .size(9.0)
                                                .color(palette.text_muted),
                                        );
                                    });
                                });
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    if let Some(br) = &branch {
                                        egui::Frame::new()
                                            .fill(palette.bg_tertiary)
                                            .corner_radius(egui::CornerRadius::same(4))
                                            .inner_margin(egui::Margin::symmetric(6, 2))
                                            .show(ui, |ui| {
                                                ui.label(
                                                    egui::RichText::new(format!("\u{2387} {}", br))
                                                        .size(9.0)
                                                        .color(palette.text_muted),
                                                );
                                            });
                                    }
                                    if dir_count > 0 {
                                        ui.add_space(4.0);
                                        ui.label(
                                            egui::RichText::new(format!("{} items", dir_count))
                                                .size(9.0)
                                                .color(palette.text_disabled),
                                        );
                                    }
                                });
                            });
                        ui.add_space(24.0);

                        // Quick actions with keyboard hints
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("GET STARTED")
                                    .size(9.0)
                                    .strong()
                                    .color(palette.text_disabled),
                            );
                        });
                        ui.add_space(8.0);

                        let btn_w = 280.0;
                        // Primary action: New Task (accent fill)
                        {
                            let btn_rect = egui::Rect::from_min_size(
                                ui.cursor().min,
                                egui::vec2(btn_w, 36.0),
                            );
                            let btn_id = ui.make_persistent_id("welcome_new_task");
                            let resp = ui.interact(btn_rect, btn_id, egui::Sense::click());
                            let fill = if resp.hovered() {
                                palette.accent.gamma_multiply(0.85)
                            } else {
                                palette.accent
                            };
                            ui.painter().rect_filled(btn_rect, egui::CornerRadius::same(8), fill);
                            // Button text
                            ui.painter().text(
                                egui::pos2(btn_rect.min.x + 14.0, btn_rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                "\u{2795}  New Task",
                                egui::FontId::proportional(13.0),
                                palette.text_on_accent,
                            );
                            // Shortcut on right
                            ui.painter().text(
                                egui::pos2(btn_rect.max.x - 14.0, btn_rect.center().y),
                                egui::Align2::RIGHT_CENTER,
                                "Ctrl+J",
                                egui::FontId::proportional(10.0),
                                palette.text_on_accent.gamma_multiply(0.6),
                            );
                            if resp.clicked() {
                                self.toggle_panel(TabKind::Chat);
                            }
                            ui.advance_cursor_after_rect(btn_rect);
                        }
                        ui.add_space(4.0);

                        // Secondary actions
                        let secondary_actions = [
                            ("\u{1f4c2}  Open File", "Ctrl+O", "open_file"),
                            ("\u{2315}  Quick Open", "Ctrl+P", "quick_open"),
                            ("\u{2699}  Build Project", "Ctrl+B", "build"),
                            ("\u{21f3}  Command Palette", "Ctrl+Shift+P", "cmd_palette"),
                        ];
                        for (label, shortcut, id) in &secondary_actions {
                            let btn_rect = egui::Rect::from_min_size(
                                ui.cursor().min,
                                egui::vec2(btn_w, 30.0),
                            );
                            let btn_id = ui.make_persistent_id(*id);
                            let resp = ui.interact(btn_rect, btn_id, egui::Sense::click());
                            let fill = if resp.hovered() {
                                palette.bg_tertiary
                            } else {
                                palette.bg_secondary
                            };
                            ui.painter().rect_filled(
                                btn_rect,
                                egui::CornerRadius::same(8),
                                fill,
                            );
                            ui.painter().rect_stroke(
                                btn_rect,
                                egui::CornerRadius::same(8),
                                egui::Stroke::new(0.5, palette.border),
                                egui::StrokeKind::Inside,
                            );
                            ui.painter().text(
                                egui::pos2(btn_rect.min.x + 14.0, btn_rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                *label,
                                egui::FontId::proportional(12.0),
                                palette.text,
                            );
                            ui.painter().text(
                                egui::pos2(btn_rect.max.x - 14.0, btn_rect.center().y),
                                egui::Align2::RIGHT_CENTER,
                                *shortcut,
                                egui::FontId::proportional(9.0),
                                palette.text_disabled,
                            );
                            if resp.clicked() {
                                match *id {
                                    "open_file" => { self.open_file_dialog(); }
                                    "quick_open" => { self.quick_open.open = true; }
                                    "build" => { self.build_active(); }
                                    "cmd_palette" => { self.open_command_palette(); }
                                    _ => {}
                                }
                            }
                            ui.advance_cursor_after_rect(btn_rect);
                            ui.add_space(3.0);
                        }

                        ui.add_space(20.0);

                        // Recent files section (from pre-collected paths)
                        if !recent_paths.is_empty() {
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    egui::RichText::new("RECENT FILES")
                                        .size(9.0)
                                        .strong()
                                        .color(palette.text_disabled),
                                );
                            });
                            ui.add_space(6.0);
                            for path in &recent_paths {
                                let p = std::path::Path::new(path);
                                let file_name = p.file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or(path);
                                let dir = p.parent()
                                    .and_then(|d| d.to_str())
                                    .unwrap_or("");
                                let display = if dir.is_empty() || dir == "." {
                                    file_name.to_string()
                                } else {
                                    format!("{}  \u{203a}  {}", file_name, dir)
                                };
                                let resp = ui.add(
                                    egui::Button::new(
                                        egui::RichText::new(format!("\u{203a}  {}", display))
                                            .size(11.0)
                                            .color(palette.text_muted),
                                    )
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE)
                                    .min_size(egui::vec2(btn_w, 22.0)),
                                );
                                if resp.hovered() {
                                    resp.on_hover_text(path);
                                }
                                ui.add_space(1.0);
                            }
                            ui.add_space(12.0);
                        }

                        // Keyboard hints footer
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("Ctrl+S Save  \u{00b7}  Ctrl+` Terminal  \u{00b7}  F1 Shortcuts")
                                    .size(9.0)
                                    .color(palette.text_disabled),
                            );
                        });
                        ui.add_space(12.0);
                    });
            }
        });

        // --- Bottom Panel: Terminal | Problems | Debug | Output ---
        if !self.bottom_panel_state.collapsed {
            egui::Panel::bottom("ide_bottom_panel")
                .default_size(self.bottom_panel_state.panel_height)
                .resizable(true)
                .frame(
                    egui::Frame::new()
                        .fill(palette.bg_secondary)
                        .stroke(egui::Stroke::new(1.0, palette.border))
                        .inner_margin(egui::Margin::symmetric(6, 2)),
                )
                .show(ui, |ui: &mut egui::Ui| {
                    self.bottom_panel_state.panel_height = ui
                        .available_height()
                        .clamp(80.0, crate::editor::bottom_panel::MAX_PANEL_HEIGHT);
                    // Tab strip
                    let tab_labels = ["Terminal", "Problems", "Debug", "Output", "Checkpoints"];
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        for (i, label) in tab_labels.iter().enumerate() {
                            let is_active = i == self.bottom_panel_state.active_tab;
                            let color = if is_active {
                                palette.accent
                            } else {
                                palette.text_muted
                            };
                            // Badge for problems tab
                            let display = if i == crate::editor::bottom_panel::TAB_PROBLEMS
                                && (self.bottom_panel_state.error_count > 0
                                    || self.bottom_panel_state.warning_count > 0)
                            {
                                format!(
                                    "{} ({}/{})",
                                    label,
                                    self.bottom_panel_state.error_count,
                                    self.bottom_panel_state.warning_count
                                )
                            } else {
                                label.to_string()
                            };
                            let mut text = egui::RichText::new(display).size(10.0).color(color);
                            if is_active {
                                text = text.strong();
                            }
                            let btn = egui::Button::new(text)
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE)
                                .min_size(egui::vec2(0.0, 24.0));
                            let resp = ui.add(btn);
                            // Accent underline for active tab
                            if is_active {
                                let rect = resp.rect;
                                let underline = egui::Rect::from_min_size(
                                    egui::pos2(rect.min.x + 2.0, rect.max.y - 1.0),
                                    egui::vec2(rect.width() - 4.0, 2.0),
                                );
                                ui.painter().rect_filled(
                                    underline,
                                    egui::CornerRadius::same(1),
                                    palette.accent,
                                );
                            }
                            if resp.clicked() {
                                self.bottom_panel_state.active_tab = i;
                            }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .small_button(
                                    egui::RichText::new("\u{2715}")
                                        .size(9.0)
                                        .color(palette.text_muted),
                                )
                                .clicked()
                            {
                                self.bottom_panel_state.collapsed = true;
                            }
                        });
                    });
                    ui.separator();

                    match self.bottom_panel_state.active_tab {
                        crate::editor::bottom_panel::TAB_TERMINAL => {
                            // Terminal tab - use real TerminalState
                            if !self.terminal_spawned {
                                self.terminal_state.spawn_shell();
                                self.terminal_spawned = true;
                            }
                            self.terminal_state.show(ui, &palette);
                        }
                        crate::editor::bottom_panel::TAB_PROBLEMS => {
                            // Problems tab
                            let content_h = ui.available_height();
                            egui::ScrollArea::vertical()
                                .max_height(content_h)
                                .show(ui, |ui| {
                                    let ec = self.bottom_panel_state.error_count;
                                    let wc = self.bottom_panel_state.warning_count;
                                    if ec == 0 && wc == 0 {
                                        ui.add_space(16.0);
                                        ui.vertical_centered(|ui| {
                                            ui.label(
                                                egui::RichText::new("\u{2714}")
                                                    .size(22.0)
                                                    .color(palette.success),
                                            );
                                            ui.add_space(4.0);
                                            ui.label(
                                                egui::RichText::new("No problems detected")
                                                    .size(12.0)
                                                    .strong()
                                                    .color(palette.text),
                                            );
                                            ui.add_space(2.0);
                                            ui.label(
                                                egui::RichText::new("Your code is clean. Keep going!")
                                                    .size(10.0)
                                                    .color(palette.text_muted),
                                            );
                                        });
                                    } else {
                                        ui.horizontal(|ui| {
                                            if ec > 0 {
                                                ui.colored_label(
                                                    palette.error,
                                                    format!("\u{2716} {} error(s)", ec),
                                                );
                                            }
                                            if wc > 0 {
                                                ui.colored_label(
                                                    palette.warning,
                                                    format!("\u{26A0} {} warning(s)", wc),
                                                );
                                            }
                                        });
                                        for msg in &self.bottom_panel_state.diagnostic_messages {
                                            ui.label(
                                                egui::RichText::new(msg)
                                                    .monospace()
                                                    .size(9.0)
                                                    .color(palette.text),
                                            );
                                        }
                                    }
                                });
                        }
                        crate::editor::bottom_panel::TAB_DEBUG => {
                            // Debug tab
                            self.render_debug_panel(ui, palette);
                        }
                        crate::editor::bottom_panel::TAB_OUTPUT => {
                            // Output tab - agent metrics
                            let has_agent_activity =
                                self.agent_active || !self.pending_approvals.is_empty();
                            if has_agent_activity {
                                let snapshot = RenderSnapshot::new(&self.agent_ui_state);
                                render_agent_metrics(ui, &snapshot, palette);
                                if !self.pending_approvals.is_empty() {
                                    ui.separator();
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "{} pending approvals",
                                                self.pending_approvals.len()
                                            ))
                                            .small()
                                            .color(palette.warning),
                                        );
                                        if ui.small_button("Approve all").clicked() {
                                            self.approve_all_pending_tools();
                                        }
                                        if ui.small_button("Decline all").clicked() {
                                            self.reject_all_pending_tools();
                                        }
                                    });
                                }
                            } else {
                                // Show build/command output
                                let content_h = ui.available_height();
                                egui::ScrollArea::vertical()
                                    .max_height(content_h)
                                    .show(ui, |ui| {
                                        if !self.command_output.is_empty() {
                                            ui.label(
                                                egui::RichText::new(&self.command_output)
                                                    .monospace()
                                                    .size(9.0)
                                                    .color(palette.text),
                                            );
                                        } else {
                                            ui.add_space(12.0);
                                            ui.vertical_centered(|ui| {
                                                ui.label(
                                                    egui::RichText::new("\u{2699}")
                                                        .size(20.0)
                                                        .color(palette.text_muted.gamma_multiply(0.6)),
                                                );
                                                ui.add_space(4.0);
                                                ui.label(
                                                    egui::RichText::new("No output yet")
                                                        .size(11.0)
                                                        .strong()
                                                        .color(palette.text),
                                                );
                                                ui.add_space(2.0);
                                                ui.label(
                                                    egui::RichText::new("Run a build (Ctrl+B) or command to see output here")
                                                        .size(9.0)
                                                        .color(palette.text_muted),
                                                );
                                            });
                                        }
                                    });
                            }
                        }
                        crate::editor::bottom_panel::TAB_CHECKPOINTS => {
                            // Checkpoints tab
                            let content_h = ui.available_height();
                            egui::ScrollArea::vertical()
                                .max_height(content_h)
                                .show(ui, |ui| {
                                    self.render_checkpoints(ui);
                                });
                        }
                        _ => {}
                    }
                });
        }

        self.command_palette_ui(&ctx);
        self.quick_open_ui(&ctx);
        self.workspace_switcher_ui(&ctx);
        self.goto_line_ui(&ctx);
        self.goto_symbol_ui(&ctx);
        self.references_ui(&ctx);
        self.suggestion_panel_ui(&ctx);
        self.file_dialog_ui(&ctx);
        self.save_as_dialog_ui(&ctx);
        self.confirm_close_dialog_ui(&ctx);
        self.shortcuts_overlay_ui(&ctx);
        self.full_diff_ui(&ctx);
        self.toasts.ui(&ctx, palette);
    }
}
