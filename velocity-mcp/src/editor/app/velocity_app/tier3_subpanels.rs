//! Tier-3 panels: sidebar subpanels (navigation, source control, chat, build, account).
//!
//! Extracted verbatim from `tier3_panels.rs` (no logic changes).

use super::struct_def::VelocityApp;
use super::tier3_common::format_count;
use crate::editor::task_timeline::render_task_timeline;
use crate::editor::theme::{
    IdePalette, FONT_BODY, FONT_CAPTION, FONT_SMALL, ITEM_SPACING, SECTION_SPACING,
};
use eframe::egui;
use egui::RichText;

impl VelocityApp {
    // ── Activity Bar Sub-Panels (full implementations) ──

    pub fn render_file_tree_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        // Poll the background tree builder for updates.
        while let Ok((tree, _ts)) = self.file_tree_rx.try_recv() {
            self.file_tree = Some(tree);
            self.last_tree_update = std::time::Instant::now();
        }

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(
                    self.workspace_root
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                )
                .strong()
                .color(palette.text)
                .size(FONT_SMALL),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("\u{21bb}")
                    .on_hover_text("Refresh tree")
                    .clicked()
                {
                    self.file_tree = None;
                    let root = self.workspace_root.clone();
                    let tx = self.file_tree_tx.clone();
                    std::thread::spawn(move || {
                        let tree = super::super::helpers::build_file_tree(&root);
                        let _ = tx.send((tree, Some(std::time::SystemTime::now())));
                    });
                }
            });
        });
        ui.add_space(ITEM_SPACING);

        // File filter input
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("\u{2315}")
                    .size(FONT_SMALL)
                    .color(palette.text_muted),
            );
            let filter_width = if self.file_tree_filter.is_empty() {
                ui.available_width()
            } else {
                ui.available_width() - 20.0
            };
            ui.add(
                egui::TextEdit::singleline(&mut self.file_tree_filter)
                    .hint_text("Filter files\u{2026}")
                    .desired_width(filter_width)
                    .text_color(palette.text),
            );
            if !self.file_tree_filter.is_empty() {
                if ui
                    .small_button(
                        RichText::new("\u{2715}")
                            .size(9.0)
                            .color(palette.text_muted),
                    )
                    .clicked()
                {
                    self.file_tree_filter.clear();
                }
            }
        });
        ui.add_space(ITEM_SPACING);

        if let Some(tree) = &self.file_tree {
            let mut path_string = String::new();
            let filter = self.file_tree_filter.clone();
            egui::ScrollArea::vertical().show(ui, |ui| {
                if filter.is_empty() {
                    Self::render_file_tree_node(
                        ui,
                        tree,
                        &self.workspace_root,
                        &mut path_string,
                        palette,
                    );
                } else {
                    // Render filtered tree
                    Self::render_file_tree_node_filtered(
                        ui,
                        tree,
                        &self.workspace_root,
                        &mut path_string,
                        palette,
                        &filter,
                    );
                }
            });
        } else {
            ui.label(
                RichText::new("Building file tree\u{2026}")
                    .color(palette.text_muted)
                    .size(FONT_SMALL),
            );
            let root = self.workspace_root.clone();
            let tx = self.file_tree_tx.clone();
            std::thread::spawn(move || {
                let tree = super::super::helpers::build_file_tree(&root);
                let _ = tx.send((tree, Some(std::time::SystemTime::now())));
            });
            ui.ctx().request_repaint();
        }
    }

    pub fn render_bookmarks_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} bookmark(s)", self.bookmarks.len()))
                    .size(FONT_SMALL)
                    .color(palette.text_muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Clear").clicked() {
                    self.bookmarks.clear();
                }
            });
        });
        ui.add_space(ITEM_SPACING);

        if self.bookmarks.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("\u{1F516}")
                        .size(24.0)
                        .color(palette.text_muted.gamma_multiply(0.5)),
                );
                ui.add_space(ITEM_SPACING);
                ui.label(
                    RichText::new("No bookmarks yet")
                        .color(palette.text_muted)
                        .size(FONT_SMALL),
                );
                ui.label(
                    RichText::new("Add bookmarks from the Accessibility layout")
                        .color(palette.text_muted.gamma_multiply(0.7))
                        .size(9.0),
                );
            });
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut to_remove: Vec<usize> = Vec::new();
                for (i, bm) in self.bookmarks.iter().enumerate() {
                    let rel = bm
                        .file
                        .strip_prefix(&self.workspace_root)
                        .unwrap_or(&bm.file);
                    ui.horizontal(|ui| {
                        let label = if bm.label.is_empty() {
                            format!("{}:{}", rel.display(), bm.line)
                        } else {
                            bm.label.clone()
                        };
                        if ui
                            .selectable_label(
                                false,
                                RichText::new(&label).size(FONT_SMALL).color(palette.text),
                            )
                            .clicked()
                        {
                            self.pending_open_path = Some(bm.file.clone());
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .small_button("\u{2715}")
                                .on_hover_text("Remove")
                                .clicked()
                            {
                                to_remove.push(i);
                            }
                        });
                    });
                    ui.label(
                        RichText::new(format!("  {}:{}", rel.display(), bm.line))
                            .size(9.0)
                            .color(palette.text_muted.gamma_multiply(0.7)),
                    );
                }
                for &i in to_remove.iter().rev() {
                    self.bookmarks.remove(i);
                }
            });
        }
    }

    pub fn render_favorites_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} favorite(s)", self.favorite_files.len()))
                    .size(FONT_SMALL)
                    .color(palette.text_muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Clear").clicked() {
                    self.favorite_files.clear();
                }
            });
        });
        ui.add_space(ITEM_SPACING);

        if self.favorite_files.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("\u{2B50}")
                        .size(24.0)
                        .color(palette.text_muted.gamma_multiply(0.5)),
                );
                ui.add_space(ITEM_SPACING);
                ui.label(
                    RichText::new("No favorites yet")
                        .color(palette.text_muted)
                        .size(FONT_SMALL),
                );
                ui.label(
                    RichText::new("Star files from the editor tab context menu")
                        .color(palette.text_muted.gamma_multiply(0.7))
                        .size(9.0),
                );
            });
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut to_remove: Vec<usize> = Vec::new();
                for (i, f) in self.favorite_files.iter().enumerate() {
                    let rel = f.strip_prefix(&self.workspace_root).unwrap_or(f);
                    let name = rel
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let dir = rel
                        .parent()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(
                                false,
                                RichText::new(&name).size(FONT_SMALL).color(palette.text),
                            )
                            .clicked()
                        {
                            self.pending_open_path = Some(f.clone());
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .small_button("\u{2715}")
                                .on_hover_text("Remove")
                                .clicked()
                            {
                                to_remove.push(i);
                            }
                        });
                    });
                    if !dir.is_empty() {
                        ui.label(
                            RichText::new(format!("  {}", dir))
                                .size(9.0)
                                .color(palette.text_muted.gamma_multiply(0.7)),
                        );
                    }
                }
                for &i in to_remove.iter().rev() {
                    self.favorite_files.remove(i);
                }
            });
        }
    }

    pub fn render_code_graph_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let _action = self.graph_view.ui(ui, &self.workspace_root, palette);
    }

    pub fn render_git_changes_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let branch = if self.git_state.branch.is_empty() {
            super::super::helpers::get_git_branch(&self.workspace_root)
        } else {
            Some(self.git_state.branch.clone())
        };

        ui.horizontal(|ui| {
            if let Some(b) = &branch {
                ui.label(
                    RichText::new(format!("\u{e0a0} {}", b))
                        .size(FONT_SMALL)
                        .strong()
                        .color(palette.accent),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("\u{21bb}")
                    .on_hover_text("Refresh")
                    .clicked()
                {
                    self.git_state.refresh(&self.workspace_root);
                }
            });
        });
        ui.add_space(ITEM_SPACING);

        if self.git_state.entries.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("\u{2714}")
                        .size(24.0)
                        .color(palette.success.gamma_multiply(0.6)),
                );
                ui.add_space(ITEM_SPACING);
                ui.label(
                    RichText::new("Working tree clean")
                        .color(palette.text_muted)
                        .size(FONT_SMALL),
                );
            });
        } else {
            // Staged/unstaged summary strip
            let staged_count = self.git_state.entries.iter().filter(|e| e.staged).count();
            let unstaged_count = self.git_state.entries.len() - staged_count;
            ui.horizontal(|ui| {
                if staged_count > 0 {
                    egui::Frame::new()
                        .fill(palette.success.gamma_multiply(0.12))
                        .corner_radius(egui::CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(6, 2))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(format!("{} staged", staged_count))
                                    .size(9.0)
                                    .color(palette.success),
                            );
                        });
                }
                if unstaged_count > 0 {
                    egui::Frame::new()
                        .fill(palette.warning.gamma_multiply(0.12))
                        .corner_radius(egui::CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(6, 2))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(format!("{} unstaged", unstaged_count))
                                    .size(9.0)
                                    .color(palette.warning),
                            );
                        });
                }
            });
            ui.add_space(ITEM_SPACING);
            egui::ScrollArea::vertical().show(ui, |ui| {
                for entry in &self.git_state.entries {
                    let rel = entry
                        .path
                        .strip_prefix(&self.workspace_root)
                        .unwrap_or(&entry.path);
                    let icon = entry.status.icon();
                    let color = match entry.status {
                        crate::editor::git_ui::GitFileStatus::Modified => palette.warning,
                        crate::editor::git_ui::GitFileStatus::Added => palette.success,
                        crate::editor::git_ui::GitFileStatus::Deleted => palette.error,
                        crate::editor::git_ui::GitFileStatus::Conflicted => palette.error,
                        _ => palette.text_muted,
                    };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(icon).size(FONT_SMALL).strong().color(color));
                        if entry.staged {
                            ui.label(RichText::new("S").size(8.0).color(palette.accent));
                        }
                        ui.label(
                            RichText::new(rel.display().to_string())
                                .size(FONT_SMALL)
                                .color(palette.text),
                        );
                    });
                }
            });

            // Commit area
            ui.add_space(SECTION_SPACING);
            ui.separator();
            ui.add_space(ITEM_SPACING);
            ui.add(
                egui::TextEdit::multiline(&mut self.git_state.commit_message)
                    .hint_text("Commit message\u{2026}")
                    .desired_rows(2)
                    .desired_width(ui.available_width()),
            );
            ui.horizontal(|ui| {
                if ui
                    .button(RichText::new("Commit").size(FONT_SMALL))
                    .clicked()
                {
                    if !self.git_state.commit_message.trim().is_empty() {
                        self.status_message =
                            format!("Committing: {}", self.git_state.commit_message.trim());
                        self.git_state.commit_message.clear();
                    }
                }
                if ui
                    .button(RichText::new("Stage All").size(FONT_SMALL))
                    .clicked()
                {
                    self.status_message = "All files staged".to_string();
                }
            });
        }
    }

    pub fn render_branches_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let branch = if self.git_state.branch.is_empty() {
            super::super::helpers::get_git_branch(&self.workspace_root)
        } else {
            Some(self.git_state.branch.clone())
        };

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Branches")
                    .size(FONT_SMALL)
                    .strong()
                    .color(palette.text),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("\u{21bb}")
                    .on_hover_text("Refresh")
                    .clicked()
                {
                    self.git_state.refresh(&self.workspace_root);
                }
            });
        });
        ui.add_space(ITEM_SPACING);

        if let Some(b) = &branch {
            egui::Frame::new()
                .fill(palette.accent.gamma_multiply(0.1))
                .corner_radius(egui::CornerRadius::same(4))
                .inner_margin(egui::Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("\u{e0a0}")
                                .size(FONT_BODY)
                                .color(palette.accent),
                        );
                        ui.label(
                            RichText::new(b)
                                .size(FONT_SMALL)
                                .strong()
                                .color(palette.accent),
                        );
                        ui.label(
                            RichText::new("(current)")
                                .size(9.0)
                                .color(palette.text_muted),
                        );
                    });
                });
        }
        ui.add_space(SECTION_SPACING);

        // Ahead/behind info
        if self.git_state.ahead > 0 || self.git_state.behind > 0 {
            ui.horizontal(|ui| {
                if self.git_state.ahead > 0 {
                    ui.label(
                        RichText::new(format!("\u{2191} {} ahead", self.git_state.ahead))
                            .size(FONT_SMALL)
                            .color(palette.success),
                    );
                }
                if self.git_state.behind > 0 {
                    ui.label(
                        RichText::new(format!("\u{2193} {} behind", self.git_state.behind))
                            .size(FONT_SMALL)
                            .color(palette.warning),
                    );
                }
            });
        }

        if let Some(err) = &self.git_state.last_error {
            ui.add_space(SECTION_SPACING);
            ui.label(
                RichText::new(format!("\u{26a0} {}", err))
                    .size(9.0)
                    .color(palette.error),
            );
        }
    }

    pub fn render_commits_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} commit(s)", self.git_state.log.len()))
                    .size(FONT_SMALL)
                    .color(palette.text_muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("\u{21bb}")
                    .on_hover_text("Refresh log")
                    .clicked()
                {
                    self.git_state.refresh(&self.workspace_root);
                }
            });
        });
        ui.add_space(ITEM_SPACING);

        if self.git_state.log.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("\u{1f4dc}")
                        .size(22.0)
                        .color(palette.text_muted.gamma_multiply(0.5)),
                );
                ui.add_space(ITEM_SPACING);
                ui.label(
                    RichText::new("No commit history")
                        .color(palette.text)
                        .size(FONT_SMALL)
                        .strong(),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new("Ensure git is initialized in the workspace")
                        .color(palette.text_muted.gamma_multiply(0.7))
                        .size(9.0),
                );
            });
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for entry in &self.git_state.log {
                    egui::Frame::new()
                        .fill(palette.bg_secondary)
                        .corner_radius(egui::CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(6, 4))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(&entry.short_hash)
                                        .size(9.0)
                                        .monospace()
                                        .strong()
                                        .color(palette.accent),
                                );
                                ui.label(
                                    RichText::new(&entry.date)
                                        .size(9.0)
                                        .color(palette.text_muted),
                                );
                            });
                            ui.label(
                                RichText::new(&entry.message)
                                    .size(FONT_SMALL)
                                    .color(palette.text),
                            );
                            ui.label(
                                RichText::new(&entry.author)
                                    .size(9.0)
                                    .color(palette.text_muted.gamma_multiply(0.8)),
                            );
                        });
                    ui.add_space(2.0);
                }
            });
        }
    }

    pub fn render_chat_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        // Status bar with model selector
        ui.horizontal(|ui| {
            let status = if self.chat.agent_active {
                "Agent active"
            } else {
                "Ready"
            };
            let status_color = if self.chat.agent_active {
                palette.success
            } else {
                palette.text_muted
            };
            ui.label(
                RichText::new(format!("\u{25cf} {}", status))
                    .size(FONT_SMALL)
                    .color(status_color),
            );

            // Message count
            if !self.chat.messages.is_empty() {
                ui.label(
                    RichText::new(format!("{} msg(s)", self.chat.messages.len()))
                        .size(9.0)
                        .color(palette.text_muted.gamma_multiply(0.7)),
                );
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Clear conversation button
                if !self.chat.messages.is_empty() {
                    if ui
                        .small_button("\u{2715}")
                        .on_hover_text("Clear conversation")
                        .clicked()
                    {
                        self.chat.messages.clear();
                    }
                }
                // Thinking toggle
                if self.chat.thinking_supported {
                    let think_resp = ui
                        .selectable_label(
                            self.chat.thinking_enabled,
                            RichText::new("\u{1f9e0}").size(FONT_SMALL).color(
                                if self.chat.thinking_enabled {
                                    palette.accent
                                } else {
                                    palette.text_muted
                                },
                            ),
                        )
                        .on_hover_text(if self.chat.thinking_enabled {
                            "Thinking: ON"
                        } else {
                            "Thinking: OFF"
                        });
                    if think_resp.clicked() {
                        self.chat.thinking_enabled = !self.chat.thinking_enabled;
                    }
                }
            });
        });

        // Model selector
        if !self.chat.available_models.is_empty() {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Model:").size(9.0).color(palette.text_muted));
                egui::ComboBox::from_id_salt("chat_model_selector")
                    .selected_text(if self.chat.selected_model.is_empty() {
                        "Select model".to_string()
                    } else {
                        let label = self
                            .chat
                            .available_models
                            .iter()
                            .find(|m| m.id == self.chat.selected_model)
                            .map(|m| m.label.clone())
                            .unwrap_or_else(|| {
                                let parts: Vec<&str> =
                                    self.chat.selected_model.rsplitn(2, '/').collect();
                                parts[0].to_string()
                            });
                        label
                    })
                    .width(140.0)
                    .show_ui(ui, |ui| {
                        for model in &self.chat.available_models {
                            let is_selected = model.id == self.chat.selected_model;
                            let resp = ui.selectable_label(
                                is_selected,
                                RichText::new(&model.label).size(FONT_SMALL),
                            );
                            if resp.clicked() {
                                self.chat.selected_model = model.id.clone();
                            }
                        }
                    });
            });
        } else if !self.chat.selected_model.is_empty() {
            let short_model = self
                .chat
                .selected_model
                .rsplit('/')
                .next()
                .unwrap_or(&self.chat.selected_model);
            ui.label(
                RichText::new(short_model)
                    .size(9.0)
                    .color(palette.text_muted),
            );
        }
        ui.add_space(ITEM_SPACING);

        // Pending approvals
        if !self.chat.pending_approvals.is_empty() {
            egui::Frame::new()
                .fill(palette.warning.gamma_multiply(0.1))
                .stroke(egui::Stroke::new(1.0, palette.warning.gamma_multiply(0.3)))
                .corner_radius(egui::CornerRadius::same(4))
                .inner_margin(egui::Margin::symmetric(6, 4))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} pending approval(s)",
                            self.chat.pending_approvals.len()
                        ))
                        .size(FONT_SMALL)
                        .strong()
                        .color(palette.warning),
                    );
                });
            ui.add_space(ITEM_SPACING);
        }

        // Messages
        egui::ScrollArea::vertical().show(ui, |ui| {
            if self.chat.messages.is_empty() {
                ui.add_space(16.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("\u{1F4AC}")
                            .size(24.0)
                            .color(palette.text_muted.gamma_multiply(0.5)),
                    );
                    ui.add_space(ITEM_SPACING);
                    ui.label(
                        RichText::new("No messages yet")
                            .color(palette.text_muted)
                            .size(FONT_SMALL),
                    );
                    ui.label(
                        RichText::new("Start a conversation with the AI agent")
                            .color(palette.text_muted.gamma_multiply(0.7))
                            .size(9.0),
                    );
                });
            } else {
                for msg in &self.chat.messages {
                    let (role_label, color) = match msg.role {
                        crate::editor::chat_panel::ChatRole::User => ("You", palette.accent),
                        crate::editor::chat_panel::ChatRole::Agent => ("Agent", palette.success),
                        crate::editor::chat_panel::ChatRole::Thought => {
                            ("Thought", palette.text_muted)
                        }
                    };
                    egui::Frame::new()
                        .fill(palette.bg_secondary)
                        .corner_radius(egui::CornerRadius::same(4))
                        .inner_margin(egui::Margin::symmetric(6, 4))
                        .show(ui, |ui| {
                            ui.label(RichText::new(role_label).size(9.0).strong().color(color));
                            ui.label(
                                RichText::new(&msg.content)
                                    .size(FONT_SMALL)
                                    .color(palette.text),
                            );
                        });
                    ui.add_space(2.0);
                }
            }
        });

        // Input area
        ui.add_space(ITEM_SPACING);
        ui.separator();
        ui.add_space(ITEM_SPACING);
        let mut send = false;
        ui.horizontal(|ui| {
            let input_resp = ui.add(
                egui::TextEdit::singleline(&mut self.chat.input)
                    .hint_text("Message the agent\u{2026}")
                    .desired_width(ui.available_width() - 50.0),
            );
            if input_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                send = true;
            }
            if ui
                .button(RichText::new("\u{27a4}").size(FONT_BODY))
                .clicked()
            {
                send = true;
            }
        });
        if send && !self.chat.input.trim().is_empty() {
            self.chat
                .messages
                .push(crate::editor::chat_panel::UiChatMessage {
                    role: crate::editor::chat_panel::ChatRole::User,
                    content: self.chat.input.clone(),
                });
            self.chat.input.clear();
        }
    }

    pub fn render_voice_subpanel(&mut self, ui: &mut egui::Ui, _palette: IdePalette) {
        self.render_voice_panel(ui);
    }

    pub fn render_multimodal_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} attachment(s)",
                    self.multimodal_attachments.len()
                ))
                .size(FONT_SMALL)
                .color(palette.text_muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Clear all").clicked() {
                    self.multimodal_attachments.clear();
                }
            });
        });
        ui.add_space(ITEM_SPACING);

        // Attachment list
        if self.multimodal_attachments.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("\u{1f4ce}")
                        .size(24.0)
                        .color(palette.text_muted.gamma_multiply(0.5)),
                );
                ui.add_space(ITEM_SPACING);
                ui.label(
                    RichText::new("No attachments")
                        .color(palette.text_muted)
                        .size(FONT_SMALL),
                );
                ui.label(
                    RichText::new("Attach images, audio, or documents for the agent")
                        .color(palette.text_muted.gamma_multiply(0.7))
                        .size(9.0),
                );
            });
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut to_remove: Vec<usize> = Vec::new();
                for (i, att) in self.multimodal_attachments.iter().enumerate() {
                    let kind_label = att.kind.label();
                    let kind_icon = match att.kind {
                        crate::editor::multimodal::AttachmentKind::Image => "\u{1f5bc}",
                        crate::editor::multimodal::AttachmentKind::Audio => "\u{1f3b5}",
                        crate::editor::multimodal::AttachmentKind::Document => "\u{1f4c4}",
                    };
                    let file_name = att
                        .path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let size_kb = att.data.len() as f64 / 1024.0;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(kind_icon).size(FONT_BODY));
                        ui.label(
                            RichText::new(&file_name)
                                .size(FONT_SMALL)
                                .color(palette.text),
                        );
                        ui.label(
                            RichText::new(format!("{} ({:.1} KB)", kind_label, size_kb))
                                .size(9.0)
                                .color(palette.text_muted),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .small_button("\u{2715}")
                                .on_hover_text("Remove")
                                .clicked()
                            {
                                to_remove.push(i);
                            }
                        });
                    });
                }
                for &i in to_remove.iter().rev() {
                    self.multimodal_attachments.remove(i);
                }
            });
        }

        // Add attachment by path
        ui.add_space(SECTION_SPACING);
        ui.separator();
        ui.add_space(ITEM_SPACING);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Attach file:")
                    .size(FONT_SMALL)
                    .color(palette.text_muted),
            );
            let mut attach_path = String::new();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut attach_path)
                        .hint_text("path\u{2026}")
                        .desired_width(ui.available_width() - 60.0),
                )
                .lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && !attach_path.is_empty()
            {
                match crate::editor::multimodal::Attachment::load(&attach_path) {
                    Ok(att) => {
                        self.multimodal_attachments.push(att);
                        self.status_message = format!("Attached: {}", attach_path);
                    }
                    Err(e) => {
                        self.status_message = format!("Failed to attach: {}", e);
                    }
                }
            }
        });
    }

    pub fn render_build_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        // Status indicator — only show after a build has been triggered
        let has_built = !self.status_message.is_empty();
        if has_built {
            let build_ok = self.build_errors_count == 0;
            ui.horizontal(|ui| {
                let (icon, color) = if build_ok {
                    ("\u{2714}", palette.success)
                } else {
                    ("\u{2716}", palette.error)
                };
                ui.label(RichText::new(icon).size(FONT_BODY).color(color));
                if build_ok {
                    ui.label(
                        RichText::new("Build clean")
                            .size(FONT_SMALL)
                            .color(palette.success),
                    );
                } else {
                    ui.label(
                        RichText::new(format!("{} error(s)", self.build_errors_count))
                            .size(FONT_SMALL)
                            .color(palette.error),
                    );
                }
            });
            ui.add_space(ITEM_SPACING);
        } else {
            // Initial state — no build triggered yet
            ui.add_space(8.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("\u{1f3d7}")
                        .size(22.0)
                        .color(palette.accent.gamma_multiply(0.5)),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Ready to build")
                        .size(FONT_SMALL)
                        .strong()
                        .color(palette.text),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new("Click Build or Run to start")
                        .size(9.0)
                        .color(palette.text_muted),
                );
            });
            ui.add_space(ITEM_SPACING);
        }

        // Build controls
        ui.horizontal(|ui| {
            let build_btn = egui::Button::new(
                RichText::new("\u{25b6} Build")
                    .size(FONT_SMALL)
                    .color(palette.text),
            );
            if ui.add(build_btn).clicked() {
                self.status_message = "Building\u{2026}".to_string();
            }
            let run_btn = egui::Button::new(
                RichText::new("\u{25b6} Run")
                    .size(FONT_SMALL)
                    .color(palette.text),
            );
            if ui.add(run_btn).clicked() {
                self.status_message = "Running\u{2026}".to_string();
            }
            let stop_btn = egui::Button::new(
                RichText::new("\u{25a0} Stop")
                    .size(FONT_SMALL)
                    .color(palette.error),
            );
            if ui.add(stop_btn).clicked() {
                self.status_message = "Stopped".to_string();
            }
        });
        ui.add_space(ITEM_SPACING);

        // Status message
        if !self.status_message.is_empty() {
            egui::Frame::new()
                .fill(palette.bg_secondary)
                .corner_radius(egui::CornerRadius::same(3))
                .inner_margin(egui::Margin::symmetric(6, 4))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(&self.status_message)
                            .size(FONT_SMALL)
                            .color(palette.text_muted),
                    );
                });
        }

        // Build info
        ui.add_space(SECTION_SPACING);
        ui.separator();
        ui.add_space(ITEM_SPACING);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Provider:")
                    .size(9.0)
                    .color(palette.text_muted),
            );
            ui.label(
                RichText::new(self.provider.label())
                    .size(9.0)
                    .color(palette.text),
            );
        });
        if !self.selected_model.is_empty() {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Model:").size(9.0).color(palette.text_muted));
                ui.label(
                    RichText::new(&self.selected_model)
                        .size(9.0)
                        .color(palette.text),
                );
            });
        }
        if !self.gpu_name.is_empty() {
            ui.horizontal(|ui| {
                ui.label(RichText::new("GPU:").size(9.0).color(palette.text_muted));
                ui.label(RichText::new(&self.gpu_name).size(9.0).color(palette.text));
            });
        }
    }

    pub fn render_agent_roster_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let snapshot = self.orchestrator.dashboard_snapshot();

        // Summary strip
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("\u{2714} {}", snapshot.done_tasks))
                    .size(FONT_SMALL)
                    .color(palette.success),
            );
            ui.label(
                RichText::new(format!("\u{2716} {}", snapshot.failed_tasks))
                    .size(FONT_SMALL)
                    .color(palette.error),
            );
            ui.label(
                RichText::new(format!("\u{25b6} {}", snapshot.running_tasks))
                    .size(FONT_SMALL)
                    .color(palette.warning),
            );
            ui.label(
                RichText::new(format!("\u{22ef} {}", snapshot.pending_tasks))
                    .size(FONT_SMALL)
                    .color(palette.text_muted),
            );
        });
        ui.add_space(ITEM_SPACING);

        // Runtime status
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("Runtime: {}", snapshot.runtime_status))
                    .size(FONT_SMALL)
                    .color(palette.text),
            );
            if snapshot.execution_running {
                ui.label(
                    RichText::new("\u{25cf} running")
                        .size(9.0)
                        .color(palette.success),
                );
            }
        });
        if snapshot.has_dependency_cycle {
            ui.label(
                RichText::new("\u{26a0} Dependency cycle detected")
                    .size(9.0)
                    .color(palette.error),
            );
        }
        ui.add_space(ITEM_SPACING);

        // Active workers
        if snapshot.active_workers > 0 {
            ui.label(
                RichText::new(format!("{} active worker(s)", snapshot.active_workers))
                    .size(FONT_SMALL)
                    .strong()
                    .color(palette.text),
            );
        }

        // Task list
        if !snapshot.tasks.is_empty() {
            ui.add_space(ITEM_SPACING);
            egui::ScrollArea::vertical().show(ui, |ui| {
                for task in &snapshot.tasks {
                    let color = if task.status_label == "done" {
                        palette.success
                    } else if task.status_label == "failed" {
                        palette.error
                    } else {
                        palette.text
                    };
                    egui::Frame::new()
                        .fill(palette.bg_secondary)
                        .corner_radius(egui::CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(6, 3))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("#{}", task.id))
                                        .size(9.0)
                                        .monospace()
                                        .color(palette.text_muted),
                                );
                                ui.label(RichText::new(&task.title).size(FONT_SMALL).color(color));
                            });
                            if !task.description.is_empty() {
                                ui.label(
                                    RichText::new(&task.description)
                                        .size(9.0)
                                        .color(palette.text_muted),
                                );
                            }
                        });
                    ui.add_space(1.0);
                }
            });
        }
    }

    pub fn render_timeline_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let timeline_snapshot =
            crate::editor::task_timeline::TaskTimelineSnapshot::new(&self.task_timeline);
        render_task_timeline(ui, &timeline_snapshot, palette);
    }

    pub fn render_mission_metrics_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let snapshot = self.orchestrator.dashboard_snapshot();

        // Metrics grid
        let metrics = [
            (
                "Completed",
                format!("{}", snapshot.done_tasks),
                palette.success,
            ),
            (
                "Failed",
                format!("{}", snapshot.failed_tasks),
                palette.error,
            ),
            (
                "Running",
                format!("{}", snapshot.running_tasks),
                palette.warning,
            ),
            (
                "Pending",
                format!("{}", snapshot.pending_tasks),
                palette.text_muted,
            ),
            (
                "Blocked",
                format!("{}", snapshot.blocked_tasks),
                palette.text_muted,
            ),
            (
                "Workers",
                format!("{}", snapshot.active_workers),
                palette.accent,
            ),
        ];

        ui.columns(2, |cols| {
            for (i, (label, value, color)) in metrics.iter().enumerate() {
                let col = &mut cols[i % 2];
                egui::Frame::new()
                    .fill(palette.bg_secondary)
                    .corner_radius(egui::CornerRadius::same(4))
                    .inner_margin(egui::Margin::symmetric(8, 6))
                    .show(col, |ui| {
                        ui.label(RichText::new(value).size(16.0).strong().color(*color));
                        ui.label(RichText::new(*label).size(9.0).color(palette.text_muted));
                    });
            }
        });
        ui.add_space(SECTION_SPACING);

        // Status details
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Planning:")
                    .size(9.0)
                    .color(palette.text_muted),
            );
            ui.label(
                RichText::new(&snapshot.planning_status)
                    .size(9.0)
                    .color(palette.text),
            );
        });
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Runtime:")
                    .size(9.0)
                    .color(palette.text_muted),
            );
            ui.label(
                RichText::new(&snapshot.runtime_status)
                    .size(9.0)
                    .color(palette.text),
            );
        });
        if let Some(goal) = &snapshot.goal {
            ui.add_space(ITEM_SPACING);
            egui::Frame::new()
                .fill(palette.accent.gamma_multiply(0.08))
                .corner_radius(egui::CornerRadius::same(4))
                .inner_margin(egui::Margin::symmetric(6, 4))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("Goal")
                            .size(9.0)
                            .strong()
                            .color(palette.accent),
                    );
                    ui.label(RichText::new(goal).size(FONT_SMALL).color(palette.text));
                });
        }
    }

    pub fn render_wiki_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let _action = self
            .wiki_view
            .ui(ui, &self.workspace_root, &mut self.toasts, palette);
    }

    pub fn render_nda_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} sealed doc(s)", self.nda_docs.len()))
                    .size(FONT_SMALL)
                    .color(palette.text_muted),
            );
        });
        ui.add_space(ITEM_SPACING);

        if self.nda_docs.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("\u{1f512}")
                        .size(24.0)
                        .color(palette.text_muted.gamma_multiply(0.5)),
                );
                ui.add_space(ITEM_SPACING);
                ui.label(
                    RichText::new("No NDA documents open")
                        .color(palette.text_muted)
                        .size(FONT_SMALL),
                );
                ui.label(
                    RichText::new("Open .nda files from the workspace to view them here")
                        .color(palette.text_muted.gamma_multiply(0.7))
                        .size(9.0),
                );
            });
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (tab_id, doc) in &self.nda_docs {
                    let title = doc.doc.title().unwrap_or("Untitled").to_string();
                    let status = if doc.sealed {
                        "\u{1f512} Sealed"
                    } else {
                        "\u{1f513} Open"
                    };
                    let dirty_mark = if doc.dirty { " *" } else { "" };
                    egui::Frame::new()
                        .fill(palette.bg_secondary)
                        .corner_radius(egui::CornerRadius::same(4))
                        .inner_margin(egui::Margin::symmetric(8, 6))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("{}{}", title, dirty_mark))
                                        .size(FONT_SMALL)
                                        .strong()
                                        .color(palette.text),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(RichText::new(status).size(9.0).color(
                                            if doc.sealed {
                                                palette.warning
                                            } else {
                                                palette.text_muted
                                            },
                                        ));
                                    },
                                );
                            });
                            if let Some(path) = &doc.path {
                                let rel = path.strip_prefix(&self.workspace_root).unwrap_or(path);
                                ui.label(
                                    RichText::new(rel.display().to_string())
                                        .size(9.0)
                                        .color(palette.text_muted),
                                );
                            }
                            ui.label(
                                RichText::new(format!("{} triple(s)", doc.doc.triples.len()))
                                    .size(9.0)
                                    .color(palette.text_muted.gamma_multiply(0.8)),
                            );
                        });
                    ui.add_space(2.0);
                }
            });
        }
    }

    pub fn render_plugin_registry_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let plugins = self.plugin_registry.list();
        let all_tools = self.plugin_registry.all_tools();

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} plugin(s)", plugins.len()))
                    .size(FONT_SMALL)
                    .color(palette.text_muted),
            );
            ui.label(
                RichText::new(format!("\u{00b7} {} tool(s)", all_tools.len()))
                    .size(FONT_SMALL)
                    .color(palette.text_muted),
            );
        });
        ui.add_space(ITEM_SPACING);

        if plugins.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("\u{1f9e9}")
                        .size(24.0)
                        .color(palette.text_muted.gamma_multiply(0.5)),
                );
                ui.add_space(ITEM_SPACING);
                ui.label(
                    RichText::new("No plugins loaded")
                        .color(palette.text_muted)
                        .size(FONT_SMALL),
                );
                ui.label(
                    RichText::new("Plugins are discovered from the workspace plugins/ directory")
                        .color(palette.text_muted.gamma_multiply(0.7))
                        .size(9.0),
                );
            });
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for plugin in &plugins {
                    let enabled_mark = if plugin.enabled { "" } else { " (disabled)" };
                    let color = if plugin.enabled {
                        palette.text
                    } else {
                        palette.text_muted
                    };
                    egui::Frame::new()
                        .fill(palette.bg_secondary)
                        .corner_radius(egui::CornerRadius::same(4))
                        .inner_margin(egui::Margin::symmetric(8, 6))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("{}{}", plugin.name, enabled_mark))
                                        .size(FONT_SMALL)
                                        .strong()
                                        .color(color),
                                );
                                ui.label(
                                    RichText::new(&plugin.version)
                                        .size(9.0)
                                        .color(palette.text_muted),
                                );
                            });
                            if !plugin.description.is_empty() {
                                ui.label(
                                    RichText::new(&plugin.description)
                                        .size(FONT_SMALL)
                                        .color(palette.text_muted),
                                );
                            }
                            ui.horizontal(|ui| {
                                if !plugin.author.is_empty() {
                                    ui.label(
                                        RichText::new(format!("by {}", plugin.author))
                                            .size(9.0)
                                            .color(palette.text_muted.gamma_multiply(0.8)),
                                    );
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            RichText::new(format!("{} tool(s)", plugin.tool_count))
                                                .size(9.0)
                                                .color(palette.accent),
                                        );
                                    },
                                );
                            });
                            if !plugin.tool_names.is_empty() {
                                ui.label(
                                    RichText::new(plugin.tool_names.join(", "))
                                        .size(9.0)
                                        .color(palette.text_muted.gamma_multiply(0.7)),
                                );
                            }
                        });
                    ui.add_space(2.0);
                }
            });
        }
    }

    pub fn render_skills_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} skill(s)", self.skill_files.len()))
                    .size(FONT_SMALL)
                    .color(palette.text_muted),
            );
        });
        ui.add_space(ITEM_SPACING);

        // Search filter
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("\u{2315}")
                    .size(FONT_SMALL)
                    .color(palette.text_muted),
            );
            let filter_width = if self.skill_filter.is_empty() {
                ui.available_width()
            } else {
                ui.available_width() - 20.0
            };
            ui.add(
                egui::TextEdit::singleline(&mut self.skill_filter)
                    .hint_text("Filter skills\u{2026}")
                    .desired_width(filter_width)
                    .text_color(palette.text),
            );
            if !self.skill_filter.is_empty() {
                if ui
                    .small_button(
                        RichText::new("\u{2715}")
                            .size(9.0)
                            .color(palette.text_muted),
                    )
                    .clicked()
                {
                    self.skill_filter.clear();
                }
            }
        });
        ui.add_space(ITEM_SPACING);

        if self.skill_files.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("\u{1f3af}").size(24.0).color(palette.text_muted.gamma_multiply(0.5)));
                ui.add_space(ITEM_SPACING);
                ui.label(RichText::new("No skills defined").size(FONT_BODY).strong().color(palette.text));
                ui.add_space(2.0);
                ui.label(RichText::new("Skills are markdown files in .qoder/skills/ that teach agents new capabilities.").color(palette.text_muted).size(FONT_CAPTION));
            });
        } else {
            let filter_lower = self.skill_filter.to_lowercase();
            let matching_skills: Vec<_> = self
                .skill_files
                .iter()
                .filter(|s| {
                    filter_lower.is_empty()
                        || s.name.to_lowercase().contains(&filter_lower)
                        || s.id.to_lowercase().contains(&filter_lower)
                        || s.description.to_lowercase().contains(&filter_lower)
                })
                .collect();

            if matching_skills.is_empty() && !filter_lower.is_empty() {
                ui.add_space(SECTION_SPACING);
                ui.label(
                    RichText::new(format!("No skills match '{}'", self.skill_filter))
                        .size(FONT_SMALL)
                        .color(palette.text_muted),
                );
            } else {
                if !filter_lower.is_empty() {
                    ui.label(
                        RichText::new(format!("{} match(es)", matching_skills.len()))
                            .size(9.0)
                            .color(palette.text_muted),
                    );
                    ui.add_space(2.0);
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for skill in matching_skills {
                        egui::Frame::new()
                            .fill(palette.bg_secondary)
                            .corner_radius(egui::CornerRadius::same(4))
                            .inner_margin(egui::Margin::symmetric(8, 6))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&skill.name)
                                            .size(FONT_SMALL)
                                            .strong()
                                            .color(palette.text),
                                    );
                                    ui.label(
                                        RichText::new(&skill.id)
                                            .size(9.0)
                                            .monospace()
                                            .color(palette.text_muted),
                                    );
                                });
                                if !skill.description.is_empty() {
                                    ui.label(
                                        RichText::new(&skill.description)
                                            .size(FONT_SMALL)
                                            .color(palette.text_muted),
                                    );
                                }
                                // Preview first 120 chars of body
                                let preview: String = skill.body.chars().take(120).collect();
                                if preview.len() >= 120 {
                                    ui.label(
                                        RichText::new(format!("{}\u{2026}", preview))
                                            .size(9.0)
                                            .color(palette.text_muted.gamma_multiply(0.7)),
                                    );
                                }
                            });
                        ui.add_space(2.0);
                    }
                });
            }
        }
    }

    pub fn render_usage_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        if self.account_usage.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("\u{1f4ca}")
                        .size(24.0)
                        .color(palette.text_muted.gamma_multiply(0.5)),
                );
                ui.add_space(ITEM_SPACING);
                ui.label(
                    RichText::new("No usage data available")
                        .color(palette.text_muted)
                        .size(FONT_SMALL),
                );
            });
        } else {
            // Summary header
            let total_accounts = self.account_usage.len();
            let exhausted_count = self.account_usage.iter().filter(|u| u.exhausted).count();
            let total_requests: u32 = self.account_usage.iter().map(|u| u.requests).sum();
            let total_remaining: u32 = self.account_usage.iter().map(|u| u.remaining).sum();

            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{} account(s)", total_accounts))
                        .size(FONT_SMALL)
                        .strong()
                        .color(palette.text),
                );
                if exhausted_count > 0 {
                    egui::Frame::new()
                        .fill(palette.error.gamma_multiply(0.12))
                        .corner_radius(egui::CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(4, 1))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(format!("{} exhausted", exhausted_count))
                                    .size(9.0)
                                    .color(palette.error),
                            );
                        });
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{} total remaining", format_count(total_remaining)))
                            .size(9.0)
                            .color(palette.text_muted),
                    );
                });
            });
            ui.add_space(ITEM_SPACING);

            for usage in &self.account_usage {
                egui::Frame::new()
                    .fill(palette.bg_secondary)
                    .corner_radius(egui::CornerRadius::same(4))
                    .inner_margin(egui::Margin::symmetric(8, 6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(&usage.label)
                                    .size(FONT_SMALL)
                                    .strong()
                                    .color(palette.text),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let tier_color = if usage.exhausted {
                                        palette.error
                                    } else {
                                        palette.accent
                                    };
                                    egui::Frame::new()
                                        .fill(tier_color.gamma_multiply(0.15))
                                        .corner_radius(egui::CornerRadius::same(3))
                                        .inner_margin(egui::Margin::symmetric(4, 1))
                                        .show(ui, |ui| {
                                            ui.label(
                                                RichText::new(&usage.tier)
                                                    .size(9.0)
                                                    .color(tier_color),
                                            );
                                        });
                                },
                            );
                        });

                        // Requests bar
                        let pct = if usage.daily_limit > 0 {
                            (usage.requests as f32 / usage.daily_limit as f32).min(1.0)
                        } else {
                            0.0
                        };
                        let bar_color = if usage.exhausted {
                            palette.error
                        } else if pct > 0.8 {
                            palette.warning
                        } else {
                            palette.success
                        };
                        ui.add_space(ITEM_SPACING);
                        let bar_resp = ui.add_sized(
                            egui::Vec2::new(ui.available_width(), 6.0),
                            egui::Label::new(""),
                        );
                        let bg_rect = bar_resp.rect;
                        ui.painter().rect_filled(bg_rect, 3.0, palette.bg_tertiary);
                        let fill_rect = egui::Rect::from_min_size(
                            bg_rect.min,
                            egui::Vec2::new(bg_rect.width() * pct, 6.0),
                        );
                        ui.painter().rect_filled(fill_rect, 3.0, bar_color);

                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{} / {} requests",
                                    format_count(usage.requests),
                                    format_count(usage.daily_limit)
                                ))
                                .size(9.0)
                                .color(palette.text_muted),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "{} remaining",
                                            format_count(usage.remaining)
                                        ))
                                        .size(9.0)
                                        .color(
                                            if usage.exhausted {
                                                palette.error
                                            } else {
                                                palette.text_muted
                                            },
                                        ),
                                    );
                                },
                            );
                        });

                        // Token usage with formatted numbers
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "\u{2191} {} in",
                                    format_count(usage.tokens_in)
                                ))
                                .size(9.0)
                                .color(palette.text_muted),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "\u{2193} {} out",
                                    format_count(usage.tokens_out)
                                ))
                                .size(9.0)
                                .color(palette.text_muted),
                            );
                        });
                    });
                ui.add_space(ITEM_SPACING);
            }
        }
    }
}
