use eframe::egui;
use std::path::PathBuf;

use crate::editor::agent_ui_render::{render_agent_metrics, RenderSnapshot};
use crate::editor::task_timeline::render_task_timeline;

use super::super::helpers::*;
use super::super::render::TabViewerImpl;
use super::super::types::*;
use super::struct_def::VelocityApp;

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
        ui.label(
            egui::RichText::new("\u{1F4BE} Workspace Checkpoints")
                .size(10.0)
                .strong()
                .color(palette.accent),
        );
        ui.add_space(4.0);

        if !self.checkpoint_manager.enabled {
            ui.label(
                egui::RichText::new("Checkpointing disabled (no .git repository)")
                    .size(9.0)
                    .color(palette.text_muted),
            );
            return;
        }

        if self.checkpoint_manager.checkpoints.is_empty() {
            ui.label(
                egui::RichText::new(
                    "No checkpoints yet. They are created automatically before agent operations.",
                )
                .size(9.0)
                .color(palette.text_muted),
            );
            return;
        }

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
            // â”€â”€â”€ Panel toggle shortcuts â”€â”€â”€
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
            // â”€â”€â”€ IDE Editor Shortcuts â”€â”€â”€
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
        // Minimal top bar: File/Edit/View/Help only. All other navigation moves to
        // the command palette (Ctrl+Shift+P) or the task-centric left sidebar.
        egui::Panel::top("toolbar")
            .frame(
                egui::Frame::new()
                    .fill(palette.bg_secondary)
                    .stroke(egui::Stroke::new(1.0, palette.border))
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ui, |ui: &mut egui::Ui| {
                ui.set_min_height(32.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;

                    // Standard menu bar: File / Edit / View / Help
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
                        ui.separator();
                        if ui.button("Settings  Ctrl+,").clicked() {
                            self.focus_panel(TabKind::Settings);
                            ui.close();
                        }
                    });

                    ui.menu_button("Edit", |ui| {
                        if ui.button("Command Palette  Ctrl+Shift+P").clicked() {
                            self.open_command_palette();
                            ui.close();
                        }
                        if ui.button("Find & Replace  Ctrl+Shift+F").clicked() {
                            self.focus_panel(TabKind::Search);
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Keyboard Shortcuts  F1").clicked() {
                            self.show_shortcuts = true;
                            ui.close();
                        }
                    });

                    ui.menu_button("View", |ui| {
                        if ui.button("Toggle Sidebar  Ctrl+E").clicked() {
                            self.toggle_left_sidebar();
                            ui.close();
                        }
                        if ui.button("Toggle Right Panel  Ctrl+Shift+E").clicked() {
                            self.toggle_right_sidebar();
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Chat  Ctrl+J").clicked() {
                            self.focus_panel(TabKind::Chat);
                            ui.close();
                        }
                        if ui.button("Terminal  Ctrl+`").clicked() {
                            self.focus_panel(TabKind::Terminal);
                            ui.close();
                        }
                        if ui.button("Output").clicked() {
                            self.focus_panel(TabKind::Output);
                            ui.close();
                        }
                        ui.separator();
                        // Workspace mode switcher
                        ui.label(
                            egui::RichText::new("Workspaces")
                                .small()
                                .color(palette.text_muted),
                        );
                        let active_mode = self.appearance.profile;
                        for mode in [
                            crate::editor::theme::WorkspaceProfile::Coder,
                            crate::editor::theme::WorkspaceProfile::MissionControl,
                            crate::editor::theme::WorkspaceProfile::AutomationOperator,
                            crate::editor::theme::WorkspaceProfile::Accessibility,
                        ] {
                            let selected = mode == active_mode;
                            if ui
                                .selectable_label(selected, format!("{} {}", mode.glyph(), mode.short_label()))
                                .on_hover_text(mode.description())
                                .clicked()
                            {
                                self.set_work_mode(mode);
                                ui.close();
                            }
                        }
                    });

                    ui.menu_button("Help", |ui| {
                        if ui.button("Documentation").clicked() {
                            ui.close();
                        }
                        if ui.button("Report Issue").clicked() {
                            ui.close();
                        }
                        ui.separator();
                        ui.label(
                            egui::RichText::new("Velocity IDE v2.1.0")
                                .small()
                                .color(palette.text_muted),
                        );
                    });

                    // Right-aligned: minimal toggle buttons
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        // Sidebar toggles as compact icons
                        let left_icon = if self.left_sidebar_visible { "\u{25C0}" } else { "\u{25B6}" };
                        if ui
                            .small_button(left_icon)
                            .on_hover_text("Toggle sidebar  (Ctrl+E)")
                            .clicked()
                        {
                            self.toggle_left_sidebar();
                        }
                        let right_icon = if self.right_sidebar_visible { "\u{25B6}" } else { "\u{25C0}" };
                        if ui
                            .small_button(right_icon)
                            .on_hover_text("Toggle right panel  (Ctrl+Shift+E)")
                            .clicked()
                        {
                            self.toggle_right_sidebar();
                        }
                    });
                });
            });

        // Symbol enclosing the cursor â€” used to highlight the Outline entry and
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
                egui::Panel::top("breadcrumb").show(ui, |ui: &mut egui::Ui| {
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
                                ui.label(egui::RichText::new(comp).color(palette.text_muted));
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
            let panel_response = egui::Panel::left("left_sidebar")
                .resizable(true)
                .default_size(self.left_sidebar_width)
                .show(ui, |ui: &mut egui::Ui| {
                    // Clamp sidebar width to prevent runaway expansion
                    let w = ui.available_width().clamp(220.0, 380.0);
                    ui.set_max_width(w);
                    self.left_sidebar_width = w;

                    // ─ Primary CTA: New Task ─
                    ui.add_space(8.0);
                    let new_task_clicked = ui
                        .add_sized(
                            [ui.available_width(), 36.0],
                            egui::Button::new(
                                egui::RichText::new("+  New Task")
                                    .strong()
                                    .size(14.0),
                            )
                            .fill(palette.accent.gamma_multiply(0.25))
                            .stroke(egui::Stroke::new(1.0, palette.accent)),
                        )
                        .clicked();
                    if new_task_clicked {
                        self.open_command_palette();
                    }
                    ui.add_space(12.0);

                    // ── Workspaces Section ──
                    ui.label(
                        egui::RichText::new("Workspaces")
                            .small()
                            .strong()
                            .color(palette.text_muted),
                    );
                    ui.add_space(6.0);

                    // Current workspace card (highlighted)
                    let current_name = self
                        .workspace_root
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    egui::Frame::new()
                        .fill(palette.bg_tertiary)
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                // Workspace icon
                                ui.label(
                                    egui::RichText::new("\u{1F4C1}")
                                        .size(16.0),
                                );
                                ui.vertical(|ui| {
                                    ui.label(
                                        egui::RichText::new(&current_name)
                                            .strong()
                                            .size(13.0),
                                    );
                                    ui.label(
                                        egui::RichText::new("Active workspace")
                                            .small()
                                            .color(palette.text_muted),
                                    );
                                });
                            });
                        });
                    ui.add_space(4.0);

                    // Other projects (compact list)
                    let projects = self.projects.clone();
                    for proj in &projects {
                        if proj == &self.workspace_root {
                            continue;
                        }
                        let name = proj
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        let resp = ui
                            .horizontal(|ui| {
                                ui.add_space(8.0);
                                ui.label(
                                    egui::RichText::new("\u{1F4C1}")
                                        .size(14.0)
                                        .color(palette.text_muted),
                                );
                                ui.add_space(6.0);
                                ui.label(
                                    egui::RichText::new(&name)
                                        .size(12.0)
                                        .color(palette.text),
                                );
                            });
                        if resp.response.clicked() {
                            if proj.is_dir() {
                                self.workspace_root = proj.clone();
                                self.reload_workspace_provider_settings();
                                self.restore_workspace_preferences();
                                self.apply_appearance(&ctx);
                                let _ = self.agent_tx.send(
                                    crate::agent::UiToAgentMessage::SetWorkspace(proj.clone()),
                                );
                                let _ = self.agent_tx.send(
                                    crate::agent::UiToAgentMessage::ApplySessionState {
                                        provider: self.provider,
                                        model: self.selected_model.clone(),
                                        thinking: self.thinking_enabled,
                                    },
                                );
                                self.status_message = format!(
                                    "Switched to {:?}",
                                    proj.file_name().unwrap_or_default()
                                );
                            }
                        }
                    }

                    // Add project button
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        if ui
                            .small_button(
                                egui::RichText::new("+ Add project")
                                    .size(11.0)
                                    .color(palette.accent),
                            )
                            .clicked()
                        {
                            self.show_add_project_ui = !self.show_add_project_ui;
                        }
                    });
                    if self.show_add_project_ui {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.new_project_path_input)
                                    .desired_width(140.0)
                                    .hint_text("/path/to/project")
                                    .font(egui::FontId::new(11.0, egui::FontFamily::Monospace)),
                            );
                            if ui.small_button("Add").clicked() {
                                let p = std::path::PathBuf::from(self.new_project_path_input.trim());
                                if p.is_dir() && !self.projects.contains(&p) {
                                    self.projects.push(p.clone());
                                    self.workspace_root = p;
                                    self.reload_workspace_provider_settings();
                                    self.restore_workspace_preferences();
                                    self.apply_appearance(&ctx);
                                    let _ = self.agent_tx.send(
                                        crate::agent::UiToAgentMessage::SetWorkspace(
                                            self.workspace_root.clone(),
                                        ),
                                    );
                                    self.status_message = format!(
                                        "Added project {:?}",
                                        self.workspace_root.file_name().unwrap_or_default()
                                    );
                                } else {
                                    self.toasts.push(crate::editor::toast::Toast::error(
                                        "Path does not exist or already in list",
                                    ));
                                }
                                self.new_project_path_input.clear();
                                self.show_add_project_ui = false;
                            }
                            if ui.small_button("Cancel").clicked() {
                                self.new_project_path_input.clear();
                                self.show_add_project_ui = false;
                            }
                        });
                    }

                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(8.0);

                    // ── Quick Navigation ──
                    ui.label(
                        egui::RichText::new("Navigate")
                            .small()
                            .strong()
                            .color(palette.text_muted),
                    );
                    ui.add_space(4.0);

                    let nav_items = [
                        ("\u{1F50D}", "Search", "Ctrl+Shift+F"),
                        ("\u{1F4AC}", "Chat", "Ctrl+J"),
                        ("\u{1F4C2}", "File Tree", ""),
                        ("\u{1F504}", "Git", ""),
                    ];
                    for (icon, label, shortcut) in &nav_items {
                        let resp = ui
                            .horizontal(|ui| {
                                ui.add_space(8.0);
                                ui.label(egui::RichText::new(*icon).size(13.0));
                                ui.add_space(6.0);
                                ui.label(
                                    egui::RichText::new(*label)
                                        .size(12.0)
                                        .color(palette.text),
                                );
                                if !shortcut.is_empty() {
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(
                                            egui::RichText::new(*shortcut)
                                                .size(9.0)
                                                .color(palette.text_muted),
                                        );
                                    });
                                }
                            });
                        if resp.response.clicked() {
                            match *label {
                                "Search" => self.focus_panel(TabKind::Search),
                                "Chat" => self.focus_panel(TabKind::Chat),
                                "File Tree" => {
                                    // Switch to file tree tab
                                    let mode_cfg = crate::editor::mode_config::mode_config_for(self.appearance.profile);
                                    let sidebar_tabs = mode_cfg.left_tabs();
                                    if let Some(idx) = sidebar_tabs.iter().position(|t| matches!(t, crate::editor::sidebar_tabs::SidebarTab::Files)) {
                                        self.left_sidebar_tab = idx;
                                    }
                                }
                                "Git" => {
                                    let mode_cfg = crate::editor::mode_config::mode_config_for(self.appearance.profile);
                                    let sidebar_tabs = mode_cfg.left_tabs();
                                    if let Some(idx) = sidebar_tabs.iter().position(|t| matches!(t, crate::editor::sidebar_tabs::SidebarTab::Git)) {
                                        self.left_sidebar_tab = idx;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }

                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(8.0);

                    // ── Bottom Section: Knowledge, Automations, Extensions ──
                    let bottom_items = [
                        ("\u{1F4DA}", "Knowledge Center"),
                        ("\u{2699}\u{FE0F}", "Automations"),
                        ("\u{1F9E9}", "Extensions"),
                    ];
                    for (icon, label) in &bottom_items {
                        ui.horizontal(|ui| {
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new(*icon).size(13.0));
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(*label)
                                    .size(12.0)
                                    .color(palette.text),
                            );
                        });
                    }

                    // ── User Identity (anchored at bottom) ──
                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        // Avatar circle
                        egui::Frame::new()
                            .fill(palette.accent.gamma_multiply(0.3))
                            .corner_radius(egui::CornerRadius::same(12))
                            .inner_margin(egui::Margin::same(6))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new("IV")
                                        .size(11.0)
                                        .strong()
                                        .color(palette.accent),
                                );
                            });
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new("Ian Visser")
                                    .size(12.0)
                                    .strong(),
                            );
                            ui.label(
                                egui::RichText::new("UnitBuilds")
                                    .size(10.0)
                                    .color(palette.text_muted),
                            );
                        });
                    });
                });
            self.left_sidebar_width = panel_response.response.rect.width().clamp(220.0, 380.0);
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
                .show(ui, |ui: &mut egui::Ui| {
                    self.right_sidebar_width = ui.available_width().clamp(220.0, 600.0);
                    ui.add_space(4.0);

                    // Mode-specific right panel header
                    ui.horizontal(|ui| {
                        for panel in right_panels {
                            ui.label(
                                egui::RichText::new(format!("{} {}", panel.icon, panel.label))
                                    .size(9.0)
                                    .color(palette.text_muted),
                            );
                        }
                    });
                    ui.separator();

                    self.smart_sidebar.clear();
                    if self.build_errors_count > 0 {
                        self.smart_sidebar.add_diagnostic(0, true, "workspace", 0, 0, "Build errors require attention");
                    }

                    // â”€â”€ Active changes (collapsible) â”€â”€
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

                    // â”€â”€ Symbol context (collapsible) â”€â”€
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

        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(mut dock_state) = self.dock_state.take() {
                let mut viewer = TabViewerImpl { app: self };
                egui_dock::DockArea::new(&mut dock_state)
                    .style(egui_dock::Style::from_egui(ui.style().as_ref()))
                    .show_inside(ui, &mut viewer);
                self.dock_state = Some(dock_state);
            }
        });

        // â”€â”€â”€ Bottom Panel: Terminal | Problems | Debug | Output â”€â”€â”€
        if !self.bottom_panel_state.collapsed {
            egui::Panel::bottom("ide_bottom_panel")
                .default_size(self.bottom_panel_state.panel_height)
                .resizable(true)
                .show(ui, |ui: &mut egui::Ui| {
                    self.bottom_panel_state.panel_height = ui
                        .available_height()
                        .clamp(80.0, crate::editor::bottom_panel::MAX_PANEL_HEIGHT);
                    // Tab strip
                    let tab_labels = ["Terminal", "Problems", "Debug", "Output", "Checkpoints"];
                    ui.horizontal(|ui| {
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
                            if ui.selectable_label(is_active, text).clicked() {
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
                                        ui.label(
                                            egui::RichText::new("No problems detected.")
                                                .size(10.0)
                                                .color(palette.success),
                                        );
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
                                            ui.label(
                                                egui::RichText::new(
                                                    "Build output will appear here.",
                                                )
                                                .size(9.0)
                                                .color(palette.text_muted),
                                            );
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

        // ── Floating Input Bar ──
        // Modern chat-style input at the bottom of the main area.
        // Replaces the scattered command palette / chat panel / terminal model.
        let input_bar_height = 56.0;
        egui::Panel::bottom("input_bar")
            .frame(
                egui::Frame::new()
                    .fill(palette.bg_secondary)
                    .stroke(egui::Stroke::new(1.0, palette.border))
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ui, |ui: &mut egui::Ui| {
                ui.set_min_height(input_bar_height);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;

                    // Attachment button
                    if ui
                        .small_button(
                            egui::RichText::new("\u{1F4CE}")
                                .size(14.0)
                                .color(palette.text_muted),
                        )
                        .on_hover_text("Attach file or image")
                        .clicked()
                    {
                        self.open_file_dialog();
                    }

                    // Main input field
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.chat_input)
                            .hint_text("Continue this task...")
                            .desired_width(ui.available_width() - 200.0)
                            .font(egui::FontId::new(13.0, egui::FontFamily::Proportional)),
                    );

                    // Submit on Enter
                    if response.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        && !self.chat_input.trim().is_empty()
                    {
                        let message = self.chat_input.clone();
                        self.chat_input.clear();
                        // Send to agent
                        let _ = self.agent_tx.send(
                            crate::agent::UiToAgentMessage::UserPrompt(message),
                        );
                    }

                    // Access level dropdown
                    ui.menu_button(
                        egui::RichText::new("\u{26A0} Full access")
                            .size(11.0)
                            .color(palette.warning),
                        |ui| {
                            if ui.button("Full access").clicked() {
                                ui.close();
                            }
                            if ui.button("Read only").clicked() {
                                ui.close();
                            }
                            if ui.button("Ask first").clicked() {
                                ui.close();
                            }
                        },
                    );

                    // Model selector
                    ui.menu_button(
                        egui::RichText::new(format!("\u{26A1} {}", 
                            if self.selected_model.is_empty() { "default" } else { &self.selected_model }
                        ))
                        .size(11.0)
                        .color(palette.accent),
                        |ui| {
                            ui.label(
                                egui::RichText::new("Select model")
                                    .small()
                                    .color(palette.text_muted),
                            );
                            ui.separator();
                            // Show available models from provider
                            for model in &self.available_models {
                                let selected = model.id == self.selected_model;
                                if ui.selectable_label(selected, &model.label).clicked() {
                                    self.selected_model = model.id.clone();
                                    let _ = self.agent_tx.send(
                                        crate::agent::UiToAgentMessage::ApplySessionState {
                                            provider: self.provider,
                                            model: self.selected_model.clone(),
                                            thinking: self.thinking_enabled,
                                        },
                                    );
                                    ui.close();
                                }
                            }
                        },
                    );

                    // Mic button (placeholder for voice input)
                    if ui
                        .small_button(
                            egui::RichText::new("\u{1F3A4}")
                                .size(14.0)
                                .color(palette.text_muted),
                        )
                        .on_hover_text("Voice input (coming soon)")
                        .clicked()
                    {
                        // Voice input not yet implemented
                    }

                    // Send button
                    let can_send = !self.chat_input.trim().is_empty();
                    if ui
                        .add_enabled(
                            can_send,
                            egui::Button::new(
                                egui::RichText::new("\u{27A4}")
                                    .size(16.0)
                                    .strong(),
                            )
                            .fill(if can_send { palette.accent } else { palette.bg_tertiary })
                            .min_size(egui::Vec2::new(32.0, 32.0)),
                        )
                        .clicked()
                        && can_send
                    {
                        let message = self.chat_input.clone();
                        self.chat_input.clear();
                        let _ = self.agent_tx.send(
                            crate::agent::UiToAgentMessage::UserPrompt(message),
                        );
                    }
                });
            });

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
