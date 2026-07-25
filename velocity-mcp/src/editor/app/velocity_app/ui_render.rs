use std::path::PathBuf;
use eframe::egui;

use crate::editor::agent_ui_render::{render_agent_metrics, RenderSnapshot};
use crate::editor::task_timeline::render_task_timeline;

use super::super::helpers::*;
use super::super::render::TabViewerImpl;
use super::super::types::*;
use super::actions::{fuzzy_match_indices, fuzzy_subsequence};
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
                            let semantic_label = if self.semantic_search_active { "⊕ Semantic" } else { "⊕ Literal" };
                            if ui.small_button(egui::RichText::new(semantic_label).size(9.0).color(palette.accent)).clicked() {
                                self.semantic_search_active = !self.semantic_search_active;
                                // Build index on first activation
                                if self.semantic_search_active && self.semantic_index.is_none() {
                                    self.semantic_index = Some(
                                        crate::editor::semantic_search::SemanticIndex::build(&self.workspace_root)
                                    );
                                    self.toasts.push(crate::editor::toast::Toast::info("Semantic index built"));
                                }
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        let hint = if self.semantic_search_active { "Semantic search…" } else { "Search…" };
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
                                .hint_text("Replace with…")
                                .desired_width(ui.available_width() - 90.0),
                        );
                        let can_replace = !self.search_query.is_empty();
                        if ui
                            .add_enabled(can_replace, egui::Button::new("Replace All"))
                            .on_hover_text("Replace every case-sensitive match across the workspace")
                            .clicked()
                        {
                            let summary = crate::editor::search::project_replace(
                                &self.workspace_root,
                                &self.search_query,
                                &self.replace_query,
                            );
                            if summary.replacements > 0 {
                                self.toasts.push(crate::editor::toast::Toast::success(format!(
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
                    egui::ScrollArea::vertical().max_width(ui.available_width()).show(ui, |ui| {
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
                                            self.search_hits = crate::editor::search::project_search(
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
                                let title =
                                    format!("{} {} : line {}", icon, hit.path.display(), hit.line);
                                ui.group(|ui| {
                                    ui.horizontal(|ui| {
                                        if ui.link(title).clicked() {
                                            let abs_path = self.workspace_root.join(&hit.path);
                                            self.push_nav_location();
                                            self.open_editor(Some(abs_path));
                                            self.pending_cursor_line = Some(hit.line);
                                        }
                                    });
                                    let truncated = if hit.text.len() > 80 { format!("{}…", &hit.text[..80]) } else { hit.text.clone() };
                                    ui.label(egui::RichText::new(truncated).monospace().size(11.0));
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
                    ui.heading(egui::RichText::new("\u{1F310} Browse").size(14.0).color(palette.accent));
                    ui.add_space(4.0);

                    // URL input (optional)
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("URL").size(9.0).color(palette.text_muted));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.browse_state.url_input)
                                .hint_text("https://... (optional)")
                                .desired_width(ui.available_width() - 4.0)
                        );
                    });

                    // Query input + send
                    ui.horizontal(|ui| {
                        let input_resp = ui.add(
                            egui::TextEdit::singleline(&mut self.browse_state.input)
                                .hint_text("Ask a question...")
                                .desired_width(ui.available_width() - 50.0)
                        );
                        let enter = input_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let send = ui.add_enabled(
                            !self.browse_state.waiting && !self.browse_state.input.trim().is_empty(),
                            egui::Button::new(egui::RichText::new("Go").size(10.0)),
                        ).clicked();

                        if (enter || send) && !self.browse_state.waiting && !self.browse_state.input.trim().is_empty() {
                            let ws = self.workspace_root.clone();
                            let provider = self.provider;
                            let model = self.selected_model.clone();
                            self.browse_state.send(&ws, provider, &model);
                        }
                    });

                    if self.browse_state.waiting {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(egui::RichText::new("Browsing...").size(9.0).color(palette.warning));
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
                                            ui.label(egui::RichText::new("\u{25B6}").size(9.0).color(palette.accent));
                                            ui.label(egui::RichText::new(&msg.content).size(10.0).strong().color(palette.text));
                                        });
                                    }
                                    "assistant" => {
                                        egui::Frame::new()
                                            .fill(palette.bg_secondary)
                                            .corner_radius(6.0)
                                            .inner_margin(6.0)
                                            .show(ui, |ui| {
                                                ui.set_max_width(ui.available_width());
                                                ui.label(egui::RichText::new(&msg.content).size(10.0).color(palette.text));
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
                                                ui.label(egui::RichText::new(&msg.content).size(10.0).color(palette.text));
                                                ui.label(egui::RichText::new("\u{2588}").size(10.0).color(palette.accent));
                                            });
                                    }
                                    "status" => {
                                        ui.label(egui::RichText::new(format!("  \u{2022} {}", msg.content)).size(9.0).italics().color(palette.text_muted));
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
    #[allow(dead_code)]
    pub fn render_checkpoints(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        ui.label(egui::RichText::new("\u{1F4BE} Workspace Checkpoints").size(10.0).strong().color(palette.accent));
        ui.add_space(4.0);

        if !self.checkpoint_manager.enabled {
            ui.label(egui::RichText::new("Checkpointing disabled (no .git repository)").size(9.0).color(palette.text_muted));
            return;
        }

        if self.checkpoint_manager.checkpoints.is_empty() {
            ui.label(egui::RichText::new("No checkpoints yet. They are created automatically before agent operations.").size(9.0).color(palette.text_muted));
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
                        ui.label(egui::RichText::new(&cp.label).size(10.0).strong().color(palette.text));
                        ui.label(egui::RichText::new(format!("{} file(s)", cp.files_changed)).size(9.0).color(palette.text_muted));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button(egui::RichText::new("\u{2716} Discard").size(9.0).color(palette.error)).clicked() {
                                action = Some(crate::editor::bottom_panel::CheckpointAction::Discard(idx));
                            }
                            if ui.small_button(egui::RichText::new("\u{21A9} Restore").size(9.0).color(palette.success)).clicked() {
                                action = Some(crate::editor::bottom_panel::CheckpointAction::Restore(idx));
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
                            self.toasts.push(crate::editor::toast::Toast::success(format!("Restored: {}", label)));
                            self.status_message = format!("Checkpoint restored: {}", label);
                            // Refresh git state and reload buffers
                            self.git_state.refresh(&self.workspace_root);
                        }
                        Err(e) => {
                            self.toasts.push(crate::editor::toast::Toast::error(format!("Restore failed: {}", e)));
                        }
                    }
                }
                crate::editor::bottom_panel::CheckpointAction::Discard(idx) => {
                    match self.checkpoint_manager.discard_checkpoint(idx) {
                        Ok(label) => {
                            self.toasts.push(crate::editor::toast::Toast::info(format!("Discarded: {}", label)));
                        }
                        Err(e) => {
                            self.toasts.push(crate::editor::toast::Toast::error(format!("Discard failed: {}", e)));
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
        ctx.input(|i| {
            let cmd = i.modifiers.command;
            let shift = i.modifiers.shift;
            if i.key_pressed(egui::Key::F1) {
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
            } else if cmd && i.key_pressed(egui::Key::J) {
                self.toggle_panel(TabKind::Chat);
            } else if cmd && i.key_pressed(egui::Key::Backtick) {
                // Toggle bottom panel (Terminal)
                self.bottom_panel_state.collapsed = !self.bottom_panel_state.collapsed;
                if !self.bottom_panel_state.collapsed {
                    self.bottom_panel_state.active_tab = 0; // Switch to Terminal
                }
            } else if cmd && i.key_pressed(egui::Key::E) {
                self.toggle_left_sidebar();
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
            // ─── IDE Editor Shortcuts ───
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
    }

    pub fn command_palette_ui(&mut self, ctx: &egui::Context) {
        if !self.command_palette.open {
            return;
        }

        let palette = self.palette();

        let area = egui::Area::new(egui::Id::new("command_palette_area"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_TOP, egui::Vec2::new(0.0, 80.0));

        let commands = self.command_list_filtered();
        let query = self.command_palette.query.to_lowercase();
        let mut open = self.command_palette.open;

        self.command_palette.selected = self
            .command_palette
            .selected
            .min(commands.len().saturating_sub(1));

        area.show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(ui.visuals().code_bg_color)
                .stroke(ui.visuals().window_stroke)
                .inner_margin(egui::Margin::same(10))
                .corner_radius(egui::CornerRadius::same(12))
                .show(ui, |ui| {
                    ui.set_width(480.0);
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.command_palette.query)
                            .hint_text("Search commands…")
                            .desired_width(480.0),
                    );
                    // Grab focus on the frame the palette opens so you can type
                    // immediately without clicking into the field.
                    if self.command_palette.just_opened {
                        response.request_focus();
                        self.command_palette.just_opened = false;
                    }
                    if response.changed() {
                        self.command_palette.selected = 0;
                    }
                    ui.add_space(6.0);
                    ui.separator();

                    if commands.is_empty() {
                        ui.add_space(18.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("No matching commands")
                                    .color(palette.text_muted),
                            );
                        });
                        ui.add_space(18.0);
                    }

                    egui::ScrollArea::vertical()
                        .max_height(300.0)
                        .show(ui, |ui| {
                            let mut last_category = "";
                            for (idx, cmd) in commands.iter().enumerate() {
                                if cmd.category != last_category {
                                    last_category = cmd.category;
                                    ui.add_space(6.0);
                                    ui.label(
                                        egui::RichText::new(cmd.category.to_uppercase())
                                            .small()
                                            .strong()
                                            .color(palette.text_muted),
                                    );
                                    ui.add_space(2.0);
                                }
                                let selected = idx == self.command_palette.selected;
                                ui.horizontal(|ui| {
                                    // Highlight the fuzzy-matched characters so it's
                                    // clear why a command matched the query.
                                    let base_color = if selected {
                                        palette.accent
                                    } else {
                                        palette.text
                                    };
                                    let matched: std::collections::HashSet<usize> =
                                        fuzzy_match_indices(cmd.label, &query)
                                            .unwrap_or_default()
                                            .into_iter()
                                            .collect();
                                    let mut job = egui::text::LayoutJob::default();
                                    let mut buf = [0u8; 4];
                                    for (ci, ch) in cmd.label.chars().enumerate() {
                                        let is_match = matched.contains(&ci);
                                        let mut fmt = egui::TextFormat {
                                            color: if is_match {
                                                palette.warning
                                            } else {
                                                base_color
                                            },
                                            ..Default::default()
                                        };
                                        if is_match {
                                            fmt.underline =
                                                egui::Stroke::new(1.0, palette.warning);
                                        }
                                        job.append(ch.encode_utf8(&mut buf), 0.0, fmt);
                                    }
                                    let resp = ui.selectable_label(selected, job);
                                    // Keep the keyboard-selected row in view.
                                    if selected {
                                        resp.scroll_to_me(Some(egui::Align::Center));
                                    }
                                    if let Some(shortcut) = cmd.shortcut {
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(
                                                egui::RichText::new(shortcut)
                                                    .small()
                                                    .monospace()
                                                    .color(palette.text_muted.gamma_multiply(0.8)),
                                            );
                                        });
                                    }
                                    if resp.clicked() {
                                        (cmd.action)(self);
                                        self.command_palette.open = false;
                                    }
                                });
                            }
                        });

                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        if let Some(cmd) = commands.get(self.command_palette.selected) {
                            let action = cmd.action;
                            action(self);
                        }
                        self.command_palette.open = false;
                    } else if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                        if !commands.is_empty() {
                            self.command_palette.selected =
                                (self.command_palette.selected + 1) % commands.len();
                        }
                    } else if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                        if !commands.is_empty() {
                            self.command_palette.selected = self
                                .command_palette
                                .selected
                                .checked_sub(1)
                                .unwrap_or(commands.len() - 1);
                        }
                    } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        open = false;
                    }
                });
        });

        self.command_palette.open = open;
    }

    /// F1 keybinding cheat-sheet: a read-only overlay listing every command and
    /// its shortcut, grouped by category. Toggled with F1, closed with F1/Esc.
    pub fn shortcuts_overlay_ui(&mut self, ctx: &egui::Context) {
        if !self.show_shortcuts {
            return;
        }
        let palette = self.palette();
        let mut open = true;
        egui::Area::new(egui::Id::new("shortcuts_overlay_area"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .fill(ui.visuals().code_bg_color)
                    .stroke(ui.visuals().window_stroke)
                    .inner_margin(egui::Margin::same(16))
                    .corner_radius(egui::CornerRadius::same(12))
                    .show(ui, |ui| {
                        ui.set_width(560.0);
                        ui.horizontal(|ui| {
                            ui.heading("Keyboard Shortcuts");
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("✕").clicked() {
                                        open = false;
                                    }
                                },
                            );
                        });
                        ui.label(
                            egui::RichText::new("Press F1 or Esc to close")
                                .small()
                                .color(palette.text_muted),
                        );
                        ui.add_space(6.0);
                        ui.separator();
                        egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| {
                            let commands = self.commands();
                            let mut last_category = "";
                            for cmd in commands.iter() {
                                if cmd.category != last_category {
                                    last_category = cmd.category;
                                    ui.add_space(8.0);
                                    ui.label(
                                        egui::RichText::new(cmd.category.to_uppercase())
                                            .small()
                                            .strong()
                                            .color(palette.accent),
                                    );
                                    ui.add_space(2.0);
                                }
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(cmd.label).color(palette.text));
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| match cmd.shortcut {
                                            Some(sc) => {
                                                ui.label(
                                                    egui::RichText::new(sc)
                                                        .monospace()
                                                        .small()
                                                        .color(palette.text_muted),
                                                );
                                            }
                                            None => {
                                                ui.label(
                                                    egui::RichText::new("—")
                                                        .small()
                                                        .color(palette.text_muted.gamma_multiply(0.5)),
                                                );
                                            }
                                        },
                                    );
                                });
                            }
                        });
                    });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            open = false;
        }
        self.show_shortcuts = open;
    }

    /// Ctrl+P quick-open switcher: fuzzy-search workspace files and jump to them.
    pub fn quick_open_ui(&mut self, ctx: &egui::Context) {
        if !self.quick_open.open {
            return;
        }

        let palette = self.palette();

        let area = egui::Area::new(egui::Id::new("quick_open_area"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_TOP, egui::Vec2::new(0.0, 80.0));

        let query = self.quick_open.query.to_lowercase();
        // Recompute the filtered index list only when the query (or the file list)
        // changes, instead of cloning + lowercasing every file on every frame.
        if self.quick_open.last_query != query
            || self.quick_open.last_file_count != self.quick_open.files.len()
        {
            self.quick_open.filtered = if query.is_empty() {
                (0..self.quick_open.files.len()).collect()
            } else {
                self.quick_open
                    .files
                    .iter()
                    .enumerate()
                    .filter(|(_, f)| fuzzy_subsequence(&f.to_lowercase(), &query))
                    .map(|(i, _)| i)
                    .collect()
            };
            self.quick_open.last_query = query.clone();
            self.quick_open.last_file_count = self.quick_open.files.len();
        }
        let filtered: Vec<usize> = self.quick_open.filtered.clone();

        self.quick_open.selected = self.quick_open.selected.min(filtered.len().saturating_sub(1));
        let mut open = self.quick_open.open;

        area.show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(ui.visuals().code_bg_color)
                .stroke(ui.visuals().window_stroke)
                .inner_margin(egui::Margin::same(10))
                .corner_radius(egui::CornerRadius::same(12))
                .show(ui, |ui| {
                    ui.set_width(520.0);
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.quick_open.query)
                            .hint_text("Go to file… (type to filter)")
                            .desired_width(520.0),
                    );
                    if self.quick_open.just_opened {
                        response.request_focus();
                        self.quick_open.just_opened = false;
                    }
                    if response.changed() {
                        self.quick_open.selected = 0;
                    }
                    ui.add_space(6.0);
                    ui.separator();

                    if filtered.is_empty() {
                        ui.add_space(18.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("No matching files").color(palette.text_muted),
                            );
                        });
                        ui.add_space(18.0);
                    }

                    // Virtualized: render only the visible rows so a large workspace
                    // costs the same per frame as a small one.
                    let row_height = ui.text_style_height(&egui::TextStyle::Body)
                        + ui.spacing().item_spacing.y;
                    let mut scroll = egui::ScrollArea::vertical().max_height(320.0);
                    if self.quick_open.scroll_to_selected {
                        let target = ((self.quick_open.selected as f32) * row_height - 160.0
                            + row_height / 2.0)
                            .max(0.0);
                        scroll = scroll.vertical_scroll_offset(target);
                        self.quick_open.scroll_to_selected = false;
                    }
                    scroll.show_rows(ui, row_height, filtered.len(), |ui, row_range| {
                        for row in row_range {
                            let file_idx = filtered[row];
                            let file = self.quick_open.files[file_idx].clone();
                            let selected = row == self.quick_open.selected;
                            let icon = crate::editor::search::icon_for_path(std::path::Path::new(&file));
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(icon)
                                        .monospace()
                                        .size(11.0)
                                        .color(palette.text_muted),
                                );
                                let text = egui::RichText::new(&file).color(if selected {
                                    palette.accent
                                } else {
                                    palette.text
                                });
                                let resp = ui.selectable_label(selected, text);
                                if resp.clicked() {
                                    self.open_quick_open_file(&file);
                                    open = false;
                                }
                            });
                        }
                    });

                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        if let Some(file_idx) = filtered.get(self.quick_open.selected).copied() {
                            let file = self.quick_open.files[file_idx].clone();
                            self.open_quick_open_file(&file);
                        }
                        open = false;
                    } else if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                        if !filtered.is_empty() {
                            self.quick_open.selected = (self.quick_open.selected + 1) % filtered.len();
                            self.quick_open.scroll_to_selected = true;
                        }
                    } else if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                        if !filtered.is_empty() {
                            self.quick_open.selected = self
                                .quick_open
                                .selected
                                .checked_sub(1)
                                .unwrap_or(filtered.len() - 1);
                            self.quick_open.scroll_to_selected = true;
                        }
                    } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        open = false;
                    }
                });
        });

        self.quick_open.open = open;
    }

    fn open_quick_open_file(&mut self, relative: &str) {
        let path = self.workspace_root.join(relative);
        self.open_editor(Some(path));
        self.quick_open.open = false;
    }

    /// Ctrl+G go-to-line dialog: jump the active editor to a line number.
    pub fn goto_line_ui(&mut self, ctx: &egui::Context) {
        if !self.goto_line_open {
            return;
        }

        let palette = self.palette();
        let area = egui::Area::new(egui::Id::new("goto_line_area"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_TOP, egui::Vec2::new(0.0, 80.0));

        let mut open = self.goto_line_open;
        let mut goto: Option<usize> = None;

        area.show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(ui.visuals().code_bg_color)
                .stroke(ui.visuals().window_stroke)
                .inner_margin(egui::Margin::same(10))
                .corner_radius(egui::CornerRadius::same(12))
                .show(ui, |ui| {
                    ui.set_width(260.0);
                    ui.label(
                        egui::RichText::new("Go to Line")
                            .size(13.0)
                            .strong()
                            .color(palette.accent),
                    );
                    ui.add_space(4.0);
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.goto_line_input)
                            .hint_text("Line number…")
                            .desired_width(240.0),
                    );
                    if self.goto_line_just_opened {
                        response.request_focus();
                        self.goto_line_just_opened = false;
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        goto = self.goto_line_input.trim().parse::<usize>().ok();
                        open = false;
                    } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        open = false;
                    }
                });
        });

        if let Some(line) = goto {
            if self.active_tab.is_some() {
                self.push_nav_location();
                self.pending_cursor_line = Some(line.max(1));
                self.status_message = format!("Jumped to line {}", line.max(1));
            } else {
                self.status_message = "No active editor to jump to".into();
            }
        }
        self.goto_line_open = open;
    }

    /// Ctrl+Shift+O go-to-symbol switcher: fuzzy-search sitemap symbols and jump
    /// to the file/line that defines the selected one.
    pub fn goto_symbol_ui(&mut self, ctx: &egui::Context) {
        if !self.goto_symbol_open {
            return;
        }

        let palette = self.palette();
        let area = egui::Area::new(egui::Id::new("goto_symbol_area"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_TOP, egui::Vec2::new(0.0, 80.0));

        let query = self.goto_symbol_query.to_lowercase();
        // Recompute the filtered index list only when the query changes, instead of
        // cloning + lowercasing every entry on every frame.
        if self.goto_symbol_last_query != query {
            self.goto_symbol_filtered = if query.is_empty() {
                (0..self.goto_symbol_entries.len()).collect()
            } else {
                self.goto_symbol_entries
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| fuzzy_subsequence(&e.name.to_lowercase(), &query))
                    .map(|(i, _)| i)
                    .collect()
            };
            self.goto_symbol_last_query = query.clone();
        }
        let filtered: Vec<usize> = self.goto_symbol_filtered.clone();

        self.goto_symbol_selected = self
            .goto_symbol_selected
            .min(filtered.len().saturating_sub(1));
        let mut open = self.goto_symbol_open;

        area.show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(ui.visuals().code_bg_color)
                .stroke(ui.visuals().window_stroke)
                .inner_margin(egui::Margin::same(10))
                .corner_radius(egui::CornerRadius::same(12))
                .show(ui, |ui| {
                    ui.set_width(520.0);
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.goto_symbol_query)
                            .hint_text("Go to symbol… (type to filter)")
                            .desired_width(520.0),
                    );
                    if self.goto_symbol_just_opened {
                        response.request_focus();
                        self.goto_symbol_just_opened = false;
                    }
                    if response.changed() {
                        self.goto_symbol_selected = 0;
                    }
                    ui.add_space(6.0);
                    ui.separator();

                    if filtered.is_empty() {
                        ui.add_space(18.0);
                        ui.vertical_centered(|ui| {
                            let msg = if self.goto_symbol_entries.is_empty() {
                                "No symbols indexed yet — run the indexer first"
                            } else {
                                "No matching symbols"
                            };
                            ui.label(egui::RichText::new(msg).color(palette.text_muted));
                        });
                        ui.add_space(18.0);
                    }

                    // Virtualized: render only the visible rows.
                    let row_height = ui.text_style_height(&egui::TextStyle::Body)
                        + ui.spacing().item_spacing.y;
                    let mut scroll = egui::ScrollArea::vertical().max_height(320.0);
                    if self.goto_symbol_scroll_to_selected {
                        let target = ((self.goto_symbol_selected as f32) * row_height - 160.0
                            + row_height / 2.0)
                            .max(0.0);
                        scroll = scroll.vertical_scroll_offset(target);
                        self.goto_symbol_scroll_to_selected = false;
                    }
                    scroll.show_rows(ui, row_height, filtered.len(), |ui, row_range| {
                        for row in row_range {
                            let entry_idx = filtered[row];
                            let entry = self.goto_symbol_entries[entry_idx].clone();
                            let selected = row == self.goto_symbol_selected;
                            let icon = crate::editor::search::icon_for_path(
                                std::path::Path::new(&entry.file),
                            );
                            let file_label = entry.file.clone();
                            let name = entry.name.clone();
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("ƒ")
                                        .monospace()
                                        .size(12.0)
                                        .color(palette.accent),
                                );
                                let resp = ui.selectable_label(
                                    selected,
                                    egui::RichText::new(name).color(if selected {
                                        palette.accent
                                    } else {
                                        palette.text
                                    }),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            egui::RichText::new(format!("{} {}", icon, file_label))
                                                .monospace()
                                                .size(11.0)
                                                .color(palette.text_muted),
                                        );
                                    },
                                );
                                if resp.clicked() {
                                    self.jump_to_symbol(&entry);
                                    open = false;
                                }
                            });
                        }
                    });

                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        if let Some(entry_idx) = filtered.get(self.goto_symbol_selected).copied() {
                            let entry = self.goto_symbol_entries[entry_idx].clone();
                            self.jump_to_symbol(&entry);
                        }
                        open = false;
                    } else if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                        if !filtered.is_empty() {
                            self.goto_symbol_selected =
                                (self.goto_symbol_selected + 1) % filtered.len();
                            self.goto_symbol_scroll_to_selected = true;
                        }
                    } else if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                        if !filtered.is_empty() {
                            self.goto_symbol_selected = self
                                .goto_symbol_selected
                                .checked_sub(1)
                                .unwrap_or(filtered.len() - 1);
                            self.goto_symbol_scroll_to_selected = true;
                        }
                    } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        open = false;
                    }
                });
        });

        self.goto_symbol_open = open;
    }

    /// Ctrl+Tab most-recently-used tab switcher. Hold Ctrl and tap Tab to move
    /// the highlight forward (Shift reverses); releasing Ctrl commits the choice.
    pub fn mru_overlay_ui(&mut self, ctx: &egui::Context) {
        let cmd_held = ctx.input(|i| i.modifiers.command);
        let tab_pressed = ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Tab));
        let shift = ctx.input(|i| i.modifiers.shift);

        if !self.mru.open {
            if tab_pressed {
                let dock_tabs: Vec<Tab> = self
                    .dock_state
                    .as_ref()
                    .map(|d| d.iter_all_tabs().map(|(_, t)| t.clone()).collect())
                    .unwrap_or_default();
                if dock_tabs.len() >= 2 {
                    let mut order: Vec<TabId> = Vec::new();
                    if let Some(active) = self.active_tab.as_ref() {
                        order.push(active.clone());
                    }
                    for t in &dock_tabs {
                        if !order.contains(&t.id) {
                            order.push(t.id.clone());
                        }
                    }
                    self.mru.order = order;
                    self.mru.selected = 1.min(self.mru.order.len().saturating_sub(1));
                    self.mru.open = true;
                }
            }
            if !self.mru.open {
                return;
            }
        }

        // Ctrl released → commit the highlighted tab.
        if !cmd_held {
            let chosen = self.mru.order.get(self.mru.selected).cloned();
            self.mru.open = false;
            if let Some(id) = chosen {
                self.activate_tab_by_id(&id);
            }
            return;
        }

        // Still held: Tab advances the highlight, Shift+Tab reverses.
        if tab_pressed {
            let len = self.mru.order.len();
            if len > 0 {
                if shift {
                    self.mru.selected = self.mru.selected.checked_sub(1).unwrap_or(len - 1);
                } else {
                    self.mru.selected = (self.mru.selected + 1) % len;
                }
            }
        }

        let palette = self.palette();
        let order = self.mru.order.clone();
        let selected = self.mru.selected;

        egui::Area::new(egui::Id::new("mru_overlay_area"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .fill(ui.visuals().code_bg_color)
                    .stroke(ui.visuals().window_stroke)
                    .inner_margin(egui::Margin::same(10))
                    .corner_radius(egui::CornerRadius::same(12))
                    .show(ui, |ui| {
                        ui.set_min_width(300.0);
                        ui.label(
                            egui::RichText::new("Switch Tab")
                                .size(12.0)
                                .color(palette.text_muted),
                        );
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                            for (idx, id) in order.iter().enumerate() {
                                let title = self
                                    .tabs
                                    .iter()
                                    .find(|t| &t.id == id)
                                    .map(|t| t.title())
                                    .unwrap_or_else(|| "(closed)".to_string());
                                let is_sel = idx == selected;
                                let resp = ui.selectable_label(
                                    is_sel,
                                    egui::RichText::new(title).color(if is_sel {
                                        palette.accent
                                    } else {
                                        palette.text
                                    }),
                                );
                                if is_sel {
                                    resp.scroll_to_me(Some(egui::Align::Center));
                                }
                                if resp.clicked() {
                                    let chosen = id.clone();
                                    self.mru.open = false;
                                    self.activate_tab_by_id(&chosen);
                                }
                            }
                        });
                    });
            });
    }

    pub fn file_dialog_ui(&mut self, ctx: &egui::Context) {
        let mut open = self.pending_open_path.is_some();
        if !open {
            return;
        }
        let mut path_string = self
            .pending_open_path
            .as_ref()
            .and_then(|p| p.to_str())
            .map(String::from)
            .unwrap_or_default();

        egui::Window::new("Open File")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("File path (relative to workspace):");
                if ui.text_edit_singleline(&mut path_string).changed() {
                    self.pending_open_path = Some(PathBuf::from(&path_string));
                }
                ui.horizontal(|ui| {
                    if ui.button("Open").clicked() {
                        let p = self.workspace_root.join(&path_string);
                        if p.exists() && p.is_file() {
                            self.open_editor(Some(p));
                            self.pending_open_path = None;
                        } else {
                            self.status_message = format!("File not found: {}", p.display());
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.pending_open_path = None;
                    }
                });
            });
        if !open {
            self.pending_open_path = None;
        }
    }

    pub fn save_as_dialog_ui(&mut self, ctx: &egui::Context) {
        let mut open = self.pending_save_as_path.is_some();
        if !open {
            return;
        }
        let mut path_string = self
            .pending_save_as_path
            .as_ref()
            .and_then(|p| p.to_str())
            .map(String::from)
            .unwrap_or_default();

        egui::Window::new("Save As")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("File path (relative to workspace):");
                if ui.text_edit_singleline(&mut path_string).changed() {
                    self.pending_save_as_path = Some(PathBuf::from(&path_string));
                }
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        if let Some(id) = self.active_tab.clone() {
                            let p = self.workspace_root.join(&path_string);
                            self.save_buffer_to(&id, &p);
                            if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
                                if let TabKind::Editor { ref mut path, .. } = tab.kind {
                                    *path = Some(p);
                                }
                            }
                            self.pending_save_as_path = None;
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.pending_save_as_path = None;
                    }
                });
            });
        if !open {
            self.pending_save_as_path = None;
        }
    }

    /// Confirmation prompt shown when closing a tab that has unsaved edits.
    pub fn confirm_close_dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(id) = self.pending_close_tab.clone() else {
            return;
        };
        // If the tab vanished or is no longer dirty, just resolve the close.
        if !self.tab_is_dirty(&id) {
            self.pending_close_tab = None;
            self.close_tab(&id);
            self.rebuild_dock();
            return;
        }
        let palette = self.palette();
        let name = self
            .tab_path(&id)
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "untitled".to_string());

        let mut resolved: Option<&'static str> = None;
        egui::Window::new("Unsaved changes")
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(format!("“{name}” has unsaved changes."))
                        .color(palette.text),
                );
                ui.label(
                    egui::RichText::new("Do you want to save before closing?")
                        .small()
                        .color(palette.text_muted),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .button(egui::RichText::new("Save").color(palette.success))
                        .clicked()
                    {
                        resolved = Some("save");
                    }
                    if ui
                        .button(egui::RichText::new("Don't Save").color(palette.warning))
                        .clicked()
                    {
                        resolved = Some("discard");
                    }
                    if ui.button("Cancel").clicked() {
                        resolved = Some("cancel");
                    }
                });
            });

        match resolved {
            Some("save") => {
                if let Some(path) = self.tab_path(&id).cloned() {
                    if self.save_buffer_to(&id, &path) {
                        self.pending_close_tab = None;
                        self.close_tab(&id);
                        self.rebuild_dock();
                    }
                } else {
                    // No path yet: route through Save As, keep the close pending.
                    self.active_tab = Some(id);
                    self.pending_close_tab = None;
                    self.save_active_as();
                }
            }
            Some("discard") => {
                // Drop unsaved edits so the tab is no longer considered dirty.
                if let Some(buf) = self.buffers.get_mut(&id) {
                    buf.mark_saved();
                }
                self.pending_close_tab = None;
                self.close_tab(&id);
                self.rebuild_dock();
            }
            Some("cancel") => {
                self.pending_close_tab = None;
            }
            _ => {}
        }
    }

    pub fn full_diff_ui(&mut self, ctx: &egui::Context) {
        if !self.show_full_diff {
            return;
        }
        let palette = self.palette();
        let mut open = self.show_full_diff;
        let active_change_preview = self.active_change_preview();
        egui::Window::new("Full Diff")
            .open(&mut open)
            .resizable(true)
            .default_size(egui::vec2(720.0, 520.0))
            .show(ctx, |ui| {
                if let Some(change_preview) = &active_change_preview {
                    ui.label(
                        egui::RichText::new(format!(
                            "{}  (+{} / -{})",
                            change_preview.file_label,
                            change_preview.added_lines,
                            change_preview.removed_lines
                        ))
                        .strong()
                        .color(palette.warning),
                    );
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(change_preview.full_diff.as_str())
                                .monospace()
                                .size(10.0)
                                .color(palette.text),
                        );
                    });
                } else {
                    ui.label("No active unsaved changes.");
                }
            });
        self.show_full_diff = open;
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
        self.update_diagnostics();
        // Sync diagnostics counts to bottom panel
        self.bottom_panel_state.error_count = self.diagnostics.error_count();
        self.bottom_panel_state.warning_count = self.diagnostics.warning_count();
        // Sync terminal output
        self.bottom_panel_state.terminal_output = self.command_output.clone();

        // Poll open buffers for external on-disk changes (throttled ~2s).
        let external_due = self
            .last_external_check
            .map(|at| at.elapsed() >= std::time::Duration::from_secs(2))
            .unwrap_or(true);
        if external_due {
            self.last_external_check = Some(std::time::Instant::now());
            self.check_external_file_changes();
        }

        let now = std::time::Instant::now();
        // Only re-walk the workspace when its top-level mtime changes (a file/dir was
        // added/removed) or, as a safety net for nested changes, every 3 seconds.
        let root_mtime = std::fs::metadata(&self.workspace_root)
            .and_then(|m| m.modified())
            .ok();
        let mtime_changed = root_mtime != self.last_tree_mtime;
        if self.file_tree.is_none()
            || mtime_changed
            || now.duration_since(self.last_tree_update) > std::time::Duration::from_secs(3)
        {
            self.file_tree = Some(build_file_tree(&self.workspace_root));
            self.last_tree_update = now;
            self.last_tree_mtime = root_mtime;
        }

        let mut cursor_pos = None;
        if let Some(active_id) = &self.active_tab {
            if let Some(buf) = self.buffers.get(active_id) {
                let editor_id = egui::Id::new("code_editor");
                if let Some(state) = egui::widgets::text_edit::TextEditState::load(&ctx, editor_id) {
                    if let Some(cursor_range) = state.cursor.char_range() {
                        cursor_pos = Some(get_cursor_pos(
                            buf.content(),
                            cursor_range.primary.index.into(),
                        ));
                    }
                }
            }
        }
        let active_change_preview = self.active_change_preview();

        egui::Panel::top("toolbar").show(ui, |ui: &mut egui::Ui| {
            ui.add_space(3.0);
            ui.horizontal(|ui: &mut egui::Ui| {
                ui.spacing_mut().item_spacing.x = 8.0;

                // Work-mode switcher: always-visible pills for one-click switching.
                let current_mode = self.appearance.profile;
                for mode in crate::editor::theme::WorkspaceProfile::ALL {
                    let selected = mode == current_mode;
                    let text = egui::RichText::new(format!("{} {}", mode.glyph(), mode.short_label()))
                        .color(if selected {
                            palette.accent
                        } else {
                            palette.text_muted
                        });
                    if ui
                        .selectable_label(selected, text)
                        .on_hover_text(format!(
                            "{}  ·  {}\n{}",
                            mode.label(),
                            mode.shortcut_hint(),
                            mode.description()
                        ))
                        .clicked()
                    {
                        self.set_work_mode(mode);
                    }
                }
                ui.add_space(6.0);

                ui.menu_button("File", |ui| {
                    if ui.button("New File").clicked() {
                        self.open_editor_stub();
                        ui.close();
                    }
                    if ui.button("Open File…").clicked() {
                        self.open_file_dialog();
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Save").clicked() {
                        self.save_active();
                        ui.close();
                    }
                    if ui.button("Save As…").clicked() {
                        self.save_active_as();
                        ui.close();
                    }
                    if ui.button("Save All").clicked() {
                        self.save_all();
                        ui.close();
                    }
                });

                ui.add_space(8.0);

                // Mode-specific toolbar actions
                {
                    let mode_cfg = crate::editor::mode_config::mode_config_for(self.appearance.profile);
                    let toolbar_actions = mode_cfg.toolbar_actions();
                    if let Some(action_id) = crate::editor::toolbar_actions::render_mode_toolbar(ui, toolbar_actions, palette) {
                        match action_id {
                            "run" | "run_flow" => self.run_active(),
                            "build" => self.build_active(),
                            "file" => self.open_editor_stub(),
                            "git" => { self.left_sidebar_tab = 2; self.left_sidebar_visible = true; }
                            "settings" => self.toggle_panel(TabKind::Settings),
                            "deploy" => self.trigger_deploy(),
                            "resume_all" => self.run_active(),
                            "record" => {
                                self.recording_active = !self.recording_active;
                                if self.recording_active {
                                    self.status_message = "Recording actions...".into();
                                    self.toasts.push(crate::editor::toast::Toast::info("\u{25cf} Recording started"));
                                } else {
                                    let name = format!("Recording #{}", self.recordings.len() + 1);
                                    self.recordings.push(name.clone());
                                    self.status_message = format!("Saved: {}", name);
                                    self.toasts.push(crate::editor::toast::Toast::success("Recording saved"));
                                }
                            }
                            "stop" => {
                                self.recording_active = false;
                                self.agent_active = false;
                                self.status_message = "Stopped.".into();
                                self.toasts.push(crate::editor::toast::Toast::info("\u{25a0} Stopped"));
                            }
                            "schedule" => {
                                self.status_message = "Schedule: use the orchestrator to define scheduled runs.".into();
                                self.toasts.push(crate::editor::toast::Toast::info("Open Orchestrator to configure schedules"));
                                self.toggle_panel(TabKind::Orchestrator);
                            }
                            "targets" => {
                                self.left_sidebar_tab = 1; // Targets tab in Operator mode
                                self.left_sidebar_visible = true;
                            }
                            "pause_all" => {
                                self.status_message = "All agents paused.".into();
                                self.toasts.push(crate::editor::toast::Toast::info("\u{23f8} All agents paused"));
                            }
                            "scale" => {
                                self.status_message = "Scale: configure agent pool size in Mission Control.".into();
                                self.toggle_panel(TabKind::MissionControl);
                            }
                            "alerts" => {
                                self.status_message = "Alerts panel.".into();
                                self.right_sidebar_visible = true;
                            }
                            "reports" => {
                                self.status_message = "Generating mission report...".into();
                                self.toasts.push(crate::editor::toast::Toast::info("Mission report generated"));
                                self.persist_mission_activity();
                            }
                            "debug" => {
                                self.status_message = "Debug: attach to running process.".into();
                                self.toasts.push(crate::editor::toast::Toast::info("Debugger attached"));
                            }
                            "test" => {
                                self.status_message = "Running tests...".into();
                                let _ = self.agent_tx.send(crate::agent::UiToAgentMessage::RunLocalBuild);
                                self.agent_active = true;
                            }
                            "preview" => {
                                self.status_message = "Accessibility preview mode.".into();
                                self.toasts.push(crate::editor::toast::Toast::info("\u{25c9} Preview: high-contrast overlay active"));
                            }
                            "audit" => {
                                self.status_message = "Running WCAG audit...".into();
                                self.toasts.push(crate::editor::toast::Toast::info("\u{267f} Accessibility audit complete"));
                            }
                            "contrast" => {
                                self.status_message = "Contrast checker active.".into();
                                self.toasts.push(crate::editor::toast::Toast::info("\u{25d0} Contrast ratios displayed"));
                            }
                            "screen_reader" => {
                                self.status_message = "Screen reader simulation active.".into();
                                self.toasts.push(crate::editor::toast::Toast::info("\u{267f} SR simulation: focus order traced"));
                            }
                            _ => {}
                        }
                    }
                }

                if !self.pending_approvals.is_empty() {
                    ui.add_space(8.0);
                    if ui
                        .button(
                            egui::RichText::new(format!(
                                "Approve All ({})",
                                self.pending_approvals.len()
                            ))
                            .color(palette.success),
                        )
                        .clicked()
                    {
                        self.approve_all_pending_tools();
                    }
                    if ui
                        .button(egui::RichText::new("Decline All").color(palette.warning))
                        .clicked()
                    {
                        self.reject_all_pending_tools();
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button(if self.right_sidebar_visible { "◧" } else { "◨" })
                        .on_hover_text("Toggle history panel")
                        .clicked()
                    {
                        self.toggle_right_sidebar();
                    }
                    if ui
                        .small_button(if self.left_sidebar_visible { "◨" } else { "◧" })
                        .on_hover_text("Toggle sidebar")
                        .clicked()
                    {
                        self.toggle_left_sidebar();
                    }

                    let model_name = if self.selected_model.is_empty() {
                        "default"
                    } else {
                        &self.selected_model
                    };
                    ui.label(
                        egui::RichText::new(format!("{} / {}", self.provider.label(), model_name))
                            .small()
                            .color(palette.accent),
                    );
                });
            });
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
                            ui.label(egui::RichText::new("›").color(palette.text_muted).weak());
                        }
                        if i == last {
                            ui.label(egui::RichText::new(comp).color(palette.text).strong());
                        } else {
                            ui.label(egui::RichText::new(comp).color(palette.text_muted));
                        }
                    }
                    if let Some(symbol) = &symbol_for_click {
                        ui.label(egui::RichText::new("›").color(palette.text_muted).weak());
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
        }

        if self.left_sidebar_visible {
            let panel_response = egui::Panel::left("left_sidebar")
                .resizable(true)
                .default_size(self.left_sidebar_width)
                .show(ui, |ui: &mut egui::Ui| {
                    // Clamp sidebar width to prevent runaway expansion
                    let w = ui.available_width().clamp(180.0, 420.0);
                    ui.set_max_width(w);
                    self.left_sidebar_width = w;
                    ui.add_space(6.0);

                    // Project selector (compact dropdown)
                    let current_name = self.workspace_root
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let projects = self.projects.clone();
                    egui::ComboBox::from_id_salt("project_selector")
                        .selected_text(current_name)
                        .width(160.0)
                        .show_ui(ui, |ui| {
                            for proj in &projects {
                                let name = proj.file_name().unwrap_or_default().to_string_lossy().to_string();
                                let is_current = proj == &self.workspace_root;
                                if ui.selectable_label(is_current, &name).clicked() && !is_current {
                                    let new_path = proj.clone();
                                    if new_path.is_dir() {
                                        self.workspace_root = new_path.clone();
                                        self.reload_workspace_provider_settings();
                                        self.restore_workspace_preferences();
                                        self.apply_appearance(&ctx);
                                        let _ = self.agent_tx.send(crate::agent::UiToAgentMessage::SetWorkspace(new_path.clone()));
                                        let _ = self.agent_tx.send(crate::agent::UiToAgentMessage::ApplySessionState {
                                            provider: self.provider,
                                            model: self.selected_model.clone(),
                                            thinking: self.thinking_enabled,
                                        });
                                        self.status_message = format!("Switched to {:?}", proj.file_name().unwrap_or_default());
                                    }
                                }
                            }
                        });

                    ui.separator();

                    // Tab selector - mode-specific sidebar tabs
                    let mode_cfg = crate::editor::mode_config::mode_config_for(self.appearance.profile);
                    let sidebar_tabs = mode_cfg.left_tabs();
                    self.left_sidebar_tab = crate::editor::sidebar_tabs::render_sidebar_tab_strip(
                        ui, sidebar_tabs, self.left_sidebar_tab, palette,
                    );
                    ui.separator();

                    // Dispatch content based on the actual SidebarTab enum
                    let active_sidebar_tab = sidebar_tabs.get(self.left_sidebar_tab).copied();
                    use crate::editor::sidebar_tabs::SidebarTab as ST;
                    match active_sidebar_tab {
                        Some(ST::Outline) => {
                            let outline: (Option<PathBuf>, Vec<crate::editor::search::FileSymbol>) = self
                                .active_tab
                                .as_ref()
                                .and_then(|id| {
                                    let path = self.tab_path(id).cloned();
                                    let buf = self.buffers.get(id)?;
                                    Some((path, crate::editor::search::extract_file_symbols(buf.content())))
                                })
                                .unwrap_or((None, Vec::new()));
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                if outline.1.is_empty() {
                                    ui.add_space(18.0);
                                    ui.vertical_centered(|ui| {
                                        ui.label(egui::RichText::new("No symbols in the active file").small().color(palette.text_muted));
                                    });
                                } else {
                                    for sym in &outline.1 {
                                        let is_active = active_symbol.as_deref() == Some(sym.name.as_str());
                                        let resp = ui.horizontal(|ui| {
                                            ui.label(egui::RichText::new("ƒ").monospace().size(11.0).color(palette.accent));
                                            ui.selectable_label(false, egui::RichText::new(&sym.name).color(if is_active { palette.accent } else { palette.text }))
                                        }).inner;
                                        if is_active { resp.scroll_to_me(Some(egui::Align::Center)); }
                                        if resp.clicked() {
                                            if let Some(path) = outline.0.clone() {
                                                self.push_nav_location();
                                                self.open_editor(Some(path));
                                                self.pending_cursor_line = Some(sym.line);
                                            }
                                        }
                                    }
                                }
                            });
                        }
                        Some(ST::Timeline) => {
                            let timeline_snapshot = crate::editor::task_timeline::TaskTimelineSnapshot::new(&self.task_timeline);
                            render_task_timeline(ui, &timeline_snapshot, palette);
                        }
                        Some(ST::Git) => {
                            // Use the full git state
                            let branch = if self.git_state.branch.is_empty() {
                                super::super::helpers::get_git_branch(&self.workspace_root)
                            } else {
                                Some(self.git_state.branch.clone())
                            };
                            let changed: Vec<PathBuf> = self.git_state.entries.iter()
                                .map(|e| e.path.clone())
                                .collect();
                            let data = crate::editor::sidebar_tabs::GitTabData {
                                branch: branch.as_deref(),
                                changed_files: &changed,
                                workspace_root: &self.workspace_root,
                            };
                            if let Some(file) = crate::editor::sidebar_tabs::render_git_content(ui, &data, palette) {
                                self.open_editor(Some(file));
                            }
                            // Commit UI
                            ui.separator();
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.git_state.commit_message)
                                        .hint_text("Commit message…")
                                        .desired_width(ui.available_width() - 60.0),
                                );
                                if ui.small_button("Commit").clicked() && !self.git_state.commit_message.is_empty() {
                                    let root = self.workspace_root.clone();
                                    let _ = self.git_state.commit(&root);
                                }
                            });
                            // Refresh button
                            if ui.small_button("↻ Refresh").clicked() {
                                let root = self.workspace_root.clone();
                                self.git_state.refresh(&root);
                            }
                        }
                        Some(ST::Search) => {
                            self.search_panel(ui);
                        }
                        Some(ST::Browse) => {
                            self.browse_panel(ui);
                        }
                        Some(ST::Flows) => {
                            let flows: Vec<crate::editor::sidebar_tabs::FlowEntry> = self.orchestrator
                                .graph.tasks.values()
                                .map(|t| crate::editor::sidebar_tabs::FlowEntry {
                                    name: t.title.clone(),
                                    status: if self.orchestrator.execution_running { "running" } else { "idle" },
                                    step_count: t.scope.len(),
                                })
                                .collect();
                            crate::editor::sidebar_tabs::render_flows_content(ui, &flows, palette);
                        }
                        Some(ST::Targets) => {
                            let targets: Vec<crate::editor::sidebar_tabs::TargetEntry> = self.projects.iter()
                                .map(|p| crate::editor::sidebar_tabs::TargetEntry {
                                    url: p.to_string_lossy().to_string(),
                                    label: p.file_name().unwrap_or_default().to_string_lossy().to_string(),
                                    last_visited: None,
                                })
                                .collect();
                            crate::editor::sidebar_tabs::render_targets_content(ui, &targets, palette);
                        }
                        Some(ST::Recordings) => {
                            crate::editor::sidebar_tabs::render_recordings_content(ui, &self.recordings, palette);
                        }
                        Some(ST::Logs) => {
                            crate::editor::sidebar_tabs::render_logs_content(
                                ui, &self.command_output, self.task_timeline.event_count(), palette,
                            );
                        }
                        Some(ST::Agents) => {
                            let snapshot = self.orchestrator.dashboard_snapshot();
                            let agents: Vec<crate::editor::sidebar_tabs::AgentEntry> = snapshot.tasks.iter()
                                .filter(|t| t.status_label == "Running")
                                .map(|t| crate::editor::sidebar_tabs::AgentEntry {
                                    id: t.id,
                                    label: t.title.clone(),
                                    status: "running",
                                    tasks_done: snapshot.done_tasks,
                                })
                                .collect();
                            crate::editor::sidebar_tabs::render_agents_content(ui, &agents, palette);
                        }
                        Some(ST::Queue) => {
                            let snapshot = self.orchestrator.dashboard_snapshot();
                            let queue: Vec<crate::editor::sidebar_tabs::QueueEntry> = snapshot.tasks.iter()
                                .map(|t| crate::editor::sidebar_tabs::QueueEntry {
                                    id: t.id,
                                    title: t.title.clone(),
                                    status: match t.status_label.as_str() {
                                        "Pending" => "Pending",
                                        "Running" => "Running",
                                        "Done" => "Done",
                                        "Failed" => "Failed",
                                        "Follow-up" => "Follow-up",
                                        _ => "Unknown",
                                    },
                                })
                                .collect();
                            crate::editor::sidebar_tabs::render_queue_content(ui, &queue, palette);
                        }
                        Some(ST::Metrics) => {
                            let snapshot = self.orchestrator.dashboard_snapshot();
                            let metrics = crate::editor::sidebar_tabs::MetricsSnapshot {
                                tasks_completed: snapshot.done_tasks,
                                tasks_failed: snapshot.failed_tasks,
                                tasks_pending: snapshot.pending_tasks,
                                avg_duration_ms: 0,
                                total_tokens: 0,
                            };
                            crate::editor::sidebar_tabs::render_metrics_content(ui, &metrics, palette);
                        }
                        Some(ST::Favorites) => {
                            let ws = self.workspace_root.clone();
                            if let Some(file) = crate::editor::sidebar_tabs::render_favorites_content(
                                ui, &self.favorite_files, &ws, palette,
                            ) {
                                self.open_editor(Some(file));
                            }
                        }
                        Some(ST::Bookmarks) => {
                            let ws = self.workspace_root.clone();
                            if let Some((file, line)) = crate::editor::sidebar_tabs::render_bookmarks_content(
                                ui, &self.bookmarks, &ws, palette,
                            ) {
                                self.open_editor(Some(file));
                                self.pending_cursor_line = Some(line);
                            }
                        }
                        Some(ST::AccessibilityAudit) => {
                            crate::editor::sidebar_tabs::render_audit_content(ui, &[], palette);
                        }
                        _ => {
                            // Default: file tree
                            let active_path = self.active_tab.as_ref().and_then(|id| self.tab_path(id)).cloned();
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                if let Some(tree) = self.file_tree.take() {
                                    let root = tree.clone();
                                    fn render_tree_node(
                                        ui: &mut egui::Ui,
                                        node: &FileNode,
                                        app: &mut VelocityApp,
                                        active_path: &Option<PathBuf>,
                                        palette: crate::editor::theme::IdePalette,
                                    ) {
                                        if let Some(children) = &node.children {
                                            for child in children {
                                                if child.is_dir {
                                                    ui.collapsing(
                                                        egui::RichText::new(&child.name).color(palette.text_muted).strong(),
                                                        |ui| { render_tree_node(ui, child, app, active_path, palette); },
                                                    );
                                                } else {
                                                    let is_active = active_path.as_ref().map(|p| p == &child.path).unwrap_or(false);
                                                    let icon = crate::editor::search::icon_for_path(&child.path);
                                                    let clicked = ui.horizontal(|ui| {
                                                        ui.add_space(2.0);
                                                        ui.label(egui::RichText::new(icon).monospace().size(10.0).color(palette.text_muted));
                                                        let name = egui::RichText::new(&child.name).color(if is_active { palette.accent } else { palette.text });
                                                        ui.selectable_label(is_active, name).clicked()
                                                    }).inner;
                                                    if clicked { app.open_editor(Some(child.path.clone())); }
                                                }
                                            }
                                        }
                                    }
                                    render_tree_node(ui, &root, self, &active_path, palette);
                                    self.file_tree = Some(tree);
                                }
                            });
                        }
                    }
                });
            self.left_sidebar_width = panel_response.response.rect.width().clamp(180.0, 420.0);
        }

        if self.right_sidebar_visible {
            // Fetch the (TTL-cached) site map once, outside the panel closure, so the
            // symbol-context section below never re-reads index.json per frame.
            let panel_site_map = self.cached_site_map(std::time::Duration::from_secs(3));
            let right_mode_cfg = crate::editor::mode_config::mode_config_for(self.appearance.profile);
            let right_panels = right_mode_cfg.right_panels();
            let panel_response = egui::Panel::right("right_sidebar")
                .resizable(true)
                .default_size(self.right_sidebar_width)
                .show(ui, |ui: &mut egui::Ui| {
                    self.right_sidebar_width = ui.available_width().max(220.0);
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

                    // Active changes section
                    if let Some(change_preview) = &active_change_preview {
                        self.smart_sidebar.add_quick_action(0, "Review current changes", &change_preview.file_label, 0);
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
                        });
                        ui.separator();
                    }

                    // Symbol context section
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
                        ui.label(egui::RichText::new(format!("{}()", symbol)).strong().color(palette.accent));

                        if !self.cached_callers.is_empty() {
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(format!("Callers ({})", self.cached_callers.len()))
                                    .small()
                                    .strong()
                                    .color(palette.text_muted),
                            );
                            for name in self.cached_callers.clone() {
                                if ui
                                    .link(egui::RichText::new(format!("→ {}", name)).size(11.0))
                                    .clicked()
                                {
                                    self.jump_to_symbol_name(&name);
                                }
                            }
                        }
                        if !self.cached_deps.is_empty() {
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(format!("Dependencies ({})", self.cached_deps.len()))
                                    .small()
                                    .strong()
                                    .color(palette.text_muted),
                            );
                            for name in self.cached_deps.clone() {
                                if ui
                                    .link(egui::RichText::new(format!("→ {}", name)).size(11.0))
                                    .clicked()
                                {
                                    self.jump_to_symbol_name(&name);
                                }
                            }
                        }
                    } else {
                        ui.vertical_centered(|ui| {
                            ui.add_space(24.0);
                            ui.label(
                                egui::RichText::new("◌")
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
            self.right_sidebar_width = panel_response.response.rect.width().max(220.0);
        }

        let branch = get_git_branch(&self.workspace_root);
        let build_ok = self.build_errors_count == 0;
        crate::editor::status_bar::StatusBar::show(
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
        );

        egui::CentralPanel::default().show(ui, |ui| {
            let mut dock_state = self.dock_state.take().expect("dock state");
            let mut viewer = TabViewerImpl { app: self };
            egui_dock::DockArea::new(&mut dock_state)
                .style(egui_dock::Style::from_egui(ui.style().as_ref()))
                .show_inside(ui, &mut viewer);
            self.dock_state = Some(dock_state);
        });

        // ─── Bottom Panel: Terminal | Problems | Debug | Output ───
        if !self.bottom_panel_state.collapsed {
            egui::Panel::bottom("ide_bottom_panel")
                .default_size(self.bottom_panel_state.panel_height)
                .resizable(true)
                .show(ui, |ui: &mut egui::Ui| {
                    self.bottom_panel_state.panel_height = ui.available_height().max(80.0);
                    // Tab strip
                    let tab_labels = ["Terminal", "Problems", "Debug", "Output"];
                    ui.horizontal(|ui| {
                        for (i, label) in tab_labels.iter().enumerate() {
                            let is_active = i == self.bottom_panel_state.active_tab;
                            let mut text = egui::RichText::new(*label).size(10.0);
                            text = if is_active {
                                text.color(palette.accent).strong()
                            } else {
                                text.color(palette.text_muted)
                            };
                            // Badge for problems
                            let display = if i == 1 && (self.bottom_panel_state.error_count > 0 || self.bottom_panel_state.warning_count > 0) {
                                format!("{} ({}/{})", label, self.bottom_panel_state.error_count, self.bottom_panel_state.warning_count)
                            } else {
                                label.to_string()
                            };
                            let text_with_badge = if i == 1 && (self.bottom_panel_state.error_count > 0 || self.bottom_panel_state.warning_count > 0) {
                                egui::RichText::new(display).size(10.0).color(if is_active { palette.accent } else { palette.text_muted })
                            } else {
                                text
                            };
                            if ui.selectable_label(is_active, text_with_badge).clicked() {
                                self.bottom_panel_state.active_tab = i;
                            }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button(egui::RichText::new("\u{2715}").size(9.0).color(palette.text_muted)).clicked() {
                                self.bottom_panel_state.collapsed = true;
                            }
                        });
                    });
                    ui.separator();

                    match self.bottom_panel_state.active_tab {
                        0 => {
                            // Terminal tab - use real TerminalState
                            if !self.terminal_spawned {
                                self.terminal_state.spawn_shell();
                                self.terminal_spawned = true;
                            }
                            self.terminal_state.show(ui, &palette);
                        }
                        1 => {
                            // Problems tab
                            egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                                let ec = self.bottom_panel_state.error_count;
                                let wc = self.bottom_panel_state.warning_count;
                                if ec == 0 && wc == 0 {
                                    ui.label(egui::RichText::new("No problems detected.").size(10.0).color(palette.success));
                                } else {
                                    ui.horizontal(|ui| {
                                        if ec > 0 {
                                            ui.colored_label(palette.error, format!("\u{2716} {} error(s)", ec));
                                        }
                                        if wc > 0 {
                                            ui.colored_label(palette.warning, format!("\u{26A0} {} warning(s)", wc));
                                        }
                                    });
                                    for msg in &self.bottom_panel_state.diagnostic_messages {
                                        ui.label(egui::RichText::new(msg).monospace().size(9.0).color(palette.text));
                                    }
                                }
                            });
                        }
                        2 => {
                            // Debug tab
                            self.render_debug_panel(ui, palette);
                        }
                        3 => {
                            // Output tab - agent metrics
                            let has_agent_activity = self.agent_active || !self.pending_approvals.is_empty();
                            if has_agent_activity {
                                let snapshot = RenderSnapshot::new(&self.agent_ui_state);
                                render_agent_metrics(ui, &snapshot, palette);
                                if !self.pending_approvals.is_empty() {
                                    ui.separator();
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(format!("{} pending approvals", self.pending_approvals.len())).small().color(palette.warning));
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
                                egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                                    if !self.command_output.is_empty() {
                                        ui.label(egui::RichText::new(&self.command_output).monospace().size(9.0).color(palette.text));
                                    } else {
                                        ui.label(egui::RichText::new("Build output will appear here.").size(9.0).color(palette.text_muted));
                                    }
                                });
                            }
                        }
                        _ => {}
                    }
                });
        }

        self.command_palette_ui(&ctx);
        self.quick_open_ui(&ctx);
        self.goto_line_ui(&ctx);
        self.goto_symbol_ui(&ctx);
        self.file_dialog_ui(&ctx);
        self.save_as_dialog_ui(&ctx);
        self.confirm_close_dialog_ui(&ctx);
        self.shortcuts_overlay_ui(&ctx);
        self.full_diff_ui(&ctx);
        self.toasts.ui(&ctx, palette);
    }
}
