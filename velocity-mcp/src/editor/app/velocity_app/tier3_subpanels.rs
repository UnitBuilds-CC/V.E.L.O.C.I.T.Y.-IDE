//! Tier-3 panels: sidebar subpanels (navigation, source control, chat, build, account).
//!
//! Extracted verbatim from `tier3_panels.rs` (no logic changes).

use super::struct_def::VelocityApp;
use super::tier3_common::{format_count, primary_button, secondary_button};
use crate::editor::app::types::TabKind;
use crate::editor::task_timeline::{render_mission_activity_feed, render_task_timeline};
use crate::editor::theme::{
    IdePalette, FONT_BODY, FONT_CAPTION, FONT_SMALL, ITEM_SPACING, SECTION_SPACING,
};
use egui;
use egui::RichText;

impl VelocityApp {
    // ── Activity Bar Sub-Panels (full implementations) ──

    /// Drop the cached tree and rebuild it in the background. Used by the
    /// refresh button and after any on-disk change from the explorer menu.
    pub fn request_tree_refresh(&mut self) {
        self.file_tree = None;
        let root = self.workspace_root.clone();
        let tx = self.file_tree_tx.clone();
        std::thread::spawn(move || {
            let tree = super::super::helpers::build_file_tree(&root);
            let _ = tx.send((tree, Some(std::time::SystemTime::now())));
        });
    }

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
                    .small_button(egui_phosphor::regular::ARROWS_CLOCKWISE)
                    .on_hover_text("Refresh tree")
                    .clicked()
                {
                    self.request_tree_refresh();
                }
            });
        });
        ui.add_space(ITEM_SPACING);

        // File filter input
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(egui_phosphor::regular::MAGNIFYING_GLASS)
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
            if !self.file_tree_filter.is_empty()
                && ui
                    .small_button(
                        RichText::new(egui_phosphor::regular::X)
                            .size(9.0)
                            .color(palette.text_muted),
                    )
                    .clicked()
            {
                self.file_tree_filter.clear();
            }
        });
        ui.add_space(ITEM_SPACING);

        if let Some(tree) = &self.file_tree {
            let mut path_string = String::new();
            let mut tree_actions = Vec::new();
            let decorations = self.git_state.decorations(&self.workspace_root);
            let filter = self.file_tree_filter.clone();
            egui::ScrollArea::vertical().show(ui, |ui| {
                if filter.is_empty() {
                    Self::render_file_tree_node(
                        ui,
                        tree,
                        &self.workspace_root,
                        &mut path_string,
                        palette,
                        &mut tree_actions,
                        &decorations,
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
                        &mut tree_actions,
                        &decorations,
                    );
                }
            });
            // Right-click menu requests execute centrally (with dialogs and
            // confirmations), never inside the row renderer.
            if !tree_actions.is_empty() {
                self.run_tree_actions(ui.ctx(), tree_actions);
            }
            // Clicking a file row only records its relative path (the renderer
            // has no `self`); open it here now that the borrow on `file_tree`
            // has ended. Previously this string was written and discarded, so
            // clicking files in the tree did nothing.
            if !path_string.is_empty() {
                let path = self.workspace_root.join(&path_string);
                if path.is_file() {
                    self.open_editor(Some(path));
                }
            }
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
                    RichText::new(egui_phosphor::regular::BOOKMARK)
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
                                .small_button(egui_phosphor::regular::X)
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
                    RichText::new(egui_phosphor::regular::STAR)
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
                                .small_button(egui_phosphor::regular::X)
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

    /// Document Outline for the active editor: a language-light list of the
    /// file's top-level definitions, each row clickable to jump the caret to
    /// its line. The sidebar counterpart of the breadcrumb's enclosing symbol —
    /// it shows the whole-file structure at a glance, like the Outline view in
    /// every major editor. Synchronous (keyword scan, no language server), so it
    /// is populated the instant a file is open and works offline for any
    /// language the keyword table covers.
    pub fn render_outline_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        // Snapshot the symbols up front: reading `self.buffers` borrows `self`
        // immutably, but a row click runs a `&mut self` jump, so the list is
        // computed before the draw closure and the navigation happens after it
        // (the same collect-then-run discipline as the editor context menu).
        let active_symbols: Option<(String, Vec<crate::editor::search::FileSymbol>)> = self
            .active_tab
            .as_ref()
            .and_then(|id| self.buffers.get(id))
            .map(|buf| {
                let label = buf
                    .path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "untitled".to_string());
                (
                    label,
                    crate::editor::search::extract_file_symbols(buf.content()),
                )
            });

        let Some((file_label, symbols)) = active_symbols else {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(egui_phosphor::regular::LIST_BULLETS)
                        .size(24.0)
                        .color(palette.text_muted.gamma_multiply(0.5)),
                );
                ui.add_space(ITEM_SPACING);
                ui.label(
                    RichText::new("Open a file to see its outline")
                        .color(palette.text_muted)
                        .size(FONT_SMALL),
                );
            });
            return;
        };

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} · {} symbol(s)", file_label, symbols.len()))
                    .size(FONT_SMALL)
                    .color(palette.text_muted),
            );
        });
        ui.add_space(ITEM_SPACING);

        if symbols.is_empty() {
            ui.add_space(12.0);
            ui.label(
                RichText::new("No top-level definitions found")
                    .color(palette.text_muted)
                    .size(FONT_SMALL),
            );
            return;
        }

        let mut jump_line: Option<usize> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for sym in &symbols {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(sym.kind)
                            .size(9.0)
                            .color(palette.accent)
                            .monospace(),
                    );
                    if ui
                        .selectable_label(
                            false,
                            RichText::new(&sym.name)
                                .size(FONT_SMALL)
                                .color(palette.text),
                        )
                        .on_hover_text(format!("Go to line {}", sym.line))
                        .clicked()
                    {
                        jump_line = Some(sym.line);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{}", sym.line))
                                .size(9.0)
                                .color(palette.text_muted.gamma_multiply(0.7)),
                        );
                    });
                });
            }
        });

        if let Some(line) = jump_line {
            self.jump_to_line_in_active(line);
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
                    RichText::new(format!(
                        "{} {}",
                        egui_phosphor::regular::ARROWS_LEFT_RIGHT,
                        b
                    ))
                    .size(FONT_SMALL)
                    .strong()
                    .color(palette.accent),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(egui_phosphor::regular::ARROWS_CLOCKWISE)
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
                    RichText::new(egui_phosphor::regular::CHECK_CIRCLE)
                        .size(24.0)
                        .color(palette.success.gamma_multiply(0.6)),
                );
                ui.add_space(ITEM_SPACING);
                ui.label(
                    RichText::new("Working tree clean")
                        .color(palette.text_muted)
                        .size(FONT_SMALL),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new("Changes appear here as you edit and stage files.")
                        .color(palette.text_muted.gamma_multiply(0.7))
                        .size(FONT_CAPTION),
                );
                ui.add_space(ITEM_SPACING);
                if primary_button(
                    ui,
                    palette,
                    format!(
                        "{} Refresh status",
                        egui_phosphor::regular::ARROWS_CLOCKWISE
                    ),
                )
                .on_hover_text("Re-scan the working tree for changes")
                .clicked()
                {
                    self.git_state.refresh(&self.workspace_root);
                }
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
            // Reserve room for the commit area below: an unconstrained
            // ScrollArea swallows the panel's whole remaining height, pushing
            // the Commit / Stage All controls off the bottom edge.
            let list_height = (ui.available_height() - 130.0).max(80.0);
            // The list borrows `git_state.entries` immutably, so a row's
            // stage/unstage can't run git in-draw. Record the toggle and apply
            // it once the ScrollArea has released the borrow.
            let mut pending_toggle: Option<(std::path::PathBuf, bool)> = None;
            // Likewise, selecting a file for its diff is deferred out of the
            // loop so loading it can take `&mut self` freely.
            let mut pending_diff: Option<std::path::PathBuf> = None;
            egui::ScrollArea::vertical()
                .id_salt("git_changes_list_scroll")
                .max_height(list_height)
                .show(ui, |ui| {
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
                        let row_path = entry.path.clone();
                        let row_staged = entry.staged;
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(icon).size(FONT_SMALL).strong().color(color));
                            if entry.staged {
                                ui.label(RichText::new("S").size(8.0).color(palette.accent));
                            }
                            let name_resp = ui.label(
                                RichText::new(rel.display().to_string())
                                    .size(FONT_SMALL)
                                    .color(palette.text),
                            );
                            // Click the file to open its diff below; the stage
                            // button on the right handles itself.
                            if name_resp.clicked() {
                                pending_diff = Some(row_path.clone());
                            }
                            // Right-aligned stage / unstage toggle that runs the
                            // real git command, so the panel does what it shows.
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let glyph = if row_staged {
                                        egui_phosphor::regular::MINUS_CIRCLE
                                    } else {
                                        egui_phosphor::regular::PLUS_CIRCLE
                                    };
                                    let tip = if row_staged {
                                        "Unstage Changes"
                                    } else {
                                        "Stage Changes"
                                    };
                                    if ui.small_button(glyph).on_hover_text(tip).clicked() {
                                        pending_toggle = Some((row_path, row_staged));
                                    }
                                },
                            );
                        });
                    }
                });
            if let Some((path, was_staged)) = pending_toggle {
                if was_staged {
                    self.git_state.unstage_file(&self.workspace_root, &path);
                } else {
                    self.git_state.stage_file(&self.workspace_root, &path);
                }
                self.status_message =
                    crate::editor::git_ui::GitState::stage_toggle_message(!was_staged).to_string();
            }
            if let Some(path) = pending_diff {
                self.load_scm_diff(&path);
            }

            // Diff viewer: the selected file's changes, coloured add/delete.
            if let Some(label) = self.scm_diff_label() {
                ui.add_space(SECTION_SPACING);
                ui.separator();
                let mut close_clicked = false;
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("Diff: {label}"))
                            .size(FONT_SMALL)
                            .strong()
                            .color(palette.text),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button(egui_phosphor::regular::X)
                            .on_hover_text("Close diff")
                            .clicked()
                        {
                            close_clicked = true;
                        }
                    });
                });
                if close_clicked {
                    self.scm_diff_path = None;
                    self.scm_diff_lines.clear();
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("scm_diff_scroll")
                        .max_height(180.0)
                        .show(ui, |ui| {
                            for line in &self.scm_diff_lines {
                                let (color, sign) = match line.kind {
                                    crate::editor::diff_view::DiffLineKind::Add => {
                                        (palette.success, "+")
                                    }
                                    crate::editor::diff_view::DiffLineKind::Delete => {
                                        (palette.error, "-")
                                    }
                                    crate::editor::diff_view::DiffLineKind::HunkHeader => {
                                        (palette.accent, "")
                                    }
                                    crate::editor::diff_view::DiffLineKind::Context => {
                                        (palette.text_muted, " ")
                                    }
                                };
                                let text = if matches!(
                                    line.kind,
                                    crate::editor::diff_view::DiffLineKind::HunkHeader
                                ) {
                                    line.text.clone()
                                } else {
                                    format!("{sign}{}", line.text)
                                };
                                ui.label(
                                    RichText::new(text)
                                        .size(FONT_CAPTION)
                                        .monospace()
                                        .color(color),
                                );
                            }
                        });
                }
            }

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
                // A commit needs a staged file and a message; the pure blocker
                // says which, so the button explains itself instead of lying.
                let blocker = self.git_state.commit_blocker();
                let inner = ui.add_enabled_ui(blocker.is_none(), |ui| {
                    primary_button(ui, palette, "Commit")
                });
                let commit_resp = match blocker {
                    Some(why) => inner.response.on_hover_text(why),
                    None => inner.response,
                };
                if commit_resp.clicked() {
                    match self.git_state.commit(&self.workspace_root) {
                        Ok(()) => self.status_message = "Committed changes".to_string(),
                        Err(e) => self.status_message = format!("Commit failed: {e}"),
                    }
                }
                if secondary_button(ui, palette, "Stage All").clicked() {
                    self.git_state.stage_all(&self.workspace_root);
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
                    .small_button(egui_phosphor::regular::ARROWS_CLOCKWISE)
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
                            RichText::new(egui_phosphor::regular::ARROWS_LEFT_RIGHT)
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
                        RichText::new(format!(
                            "{} {} ahead",
                            egui_phosphor::regular::ARROW_UP,
                            self.git_state.ahead
                        ))
                        .size(FONT_SMALL)
                        .color(palette.success),
                    );
                }
                if self.git_state.behind > 0 {
                    ui.label(
                        RichText::new(format!(
                            "{} {} behind",
                            egui_phosphor::regular::ARROW_DOWN,
                            self.git_state.behind
                        ))
                        .size(FONT_SMALL)
                        .color(palette.warning),
                    );
                }
            });
        }

        if let Some(err) = &self.git_state.last_error {
            ui.add_space(SECTION_SPACING);
            ui.label(
                RichText::new(format!("{} {}", egui_phosphor::regular::WARNING, err))
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
                    .small_button(egui_phosphor::regular::ARROWS_CLOCKWISE)
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
                    RichText::new(egui_phosphor::regular::SCROLL)
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
                RichText::new(format!("{} {}", egui_phosphor::regular::CIRCLE, status))
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
                if !self.chat.messages.is_empty()
                    && ui
                        .small_button(egui_phosphor::regular::X)
                        .on_hover_text("Clear conversation")
                        .clicked()
                {
                    self.chat.messages.clear();
                }
                // Thinking toggle
                if self.chat.thinking_supported {
                    let think_resp = ui
                        .selectable_label(
                            self.chat.thinking_enabled,
                            RichText::new(egui_phosphor::regular::BRAIN)
                                .size(FONT_SMALL)
                                .color(if self.chat.thinking_enabled {
                                    palette.accent
                                } else {
                                    palette.text_muted
                                }),
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
                        RichText::new(egui_phosphor::regular::CHAT_CIRCLE)
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
                .button(RichText::new(egui_phosphor::regular::PAPER_PLANE_TILT).size(FONT_BODY))
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
                    RichText::new(egui_phosphor::regular::PAPERCLIP)
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
                ui.add_space(ITEM_SPACING);
                if primary_button(
                    ui,
                    palette,
                    format!("{} Go to Chat", egui_phosphor::regular::CHAT_CIRCLE),
                )
                .clicked()
                {
                    self.focus_panel(TabKind::Chat);
                }
            });
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut to_remove: Vec<usize> = Vec::new();
                for (i, att) in self.multimodal_attachments.iter().enumerate() {
                    let kind_label = att.kind.label();
                    let kind_icon = match att.kind {
                        crate::editor::multimodal::AttachmentKind::Image => {
                            egui_phosphor::regular::IMAGE
                        }
                        crate::editor::multimodal::AttachmentKind::Audio => {
                            egui_phosphor::regular::MUSIC_NOTE
                        }
                        crate::editor::multimodal::AttachmentKind::Document => {
                            egui_phosphor::regular::FILE_TEXT
                        }
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
                                .small_button(egui_phosphor::regular::X)
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

    /// Nodes sub-panel on the Build rail: every registered remote machine with
    /// its effective status, and the three operations the panel owns - ping,
    /// select as build target, forget. Below the list: an add-node form and a
    /// run-any-command box for the current target.
    ///
    /// Ping, add, and remove go through the same `instance_*` tool handlers the
    /// agent uses, so the GUI never grows a private dialect against the drone
    /// protocol; network calls run on background threads and this frame only
    /// reads the registry file (small, local).
    pub fn render_nodes_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        use crate::agent::instance_tools::{instances_path, now_secs, wrap_for_work_dir};
        use crate::agent::instances::{InstanceRegistry, InstanceStatus};

        let registry = InstanceRegistry::load(&instances_path(&self.workspace_root));
        let now = now_secs();

        // ── Routing header: where Ctrl+B/CtrlR go right now ──
        let target_name: Option<String> = self
            .nodes
            .target_id
            .as_deref()
            .and_then(|id| registry.get(id))
            .map(|r| r.name.clone());
        ui.horizontal(|ui| {
            let (text, color) = match &target_name {
                Some(n) => (format!("Builds route to: {n}"), palette.accent),
                None => (
                    "Builds route to: this machine".to_string(),
                    palette.text_muted,
                ),
            };
            ui.label(RichText::new(text).size(FONT_SMALL).color(color));
            if target_name.is_some() && ui.small_button("Local").clicked() {
                self.set_build_target(None);
            }
        });
        ui.add_space(ITEM_SPACING);

        // ── Node rows (collect-then-run: a click mutates `self`) ──
        enum RowAction {
            Ping(String),
            Target(String),
            Forget(String),
        }
        let mut actions: Vec<RowAction> = Vec::new();

        if registry.instances.is_empty() {
            ui.add_space(12.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(egui_phosphor::regular::SQUARES_FOUR)
                        .size(22.0)
                        .color(palette.text_muted.gamma_multiply(0.5)),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("No nodes registered")
                        .size(FONT_SMALL)
                        .strong()
                        .color(palette.text),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new("Add one below, or ask the agent to run instance_deploy.")
                        .size(9.0)
                        .color(palette.text_muted),
                );
            });
        } else {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height((ui.available_height() * 0.45).max(140.0))
                .show(ui, |ui| {
                    for rec in &registry.instances {
                        let eff = rec.effective_status(now);
                        let (icon, color) = match eff {
                            InstanceStatus::Online => {
                                (egui_phosphor::regular::CHECK, palette.success)
                            }
                            InstanceStatus::Degraded => {
                                (egui_phosphor::regular::WARNING, palette.warning)
                            }
                            InstanceStatus::Offline => (egui_phosphor::regular::X, palette.error),
                            InstanceStatus::Unknown => {
                                (egui_phosphor::regular::QUESTION, palette.text_muted)
                            }
                        };
                        let is_target = self.nodes.target_id.as_deref() == Some(rec.id.as_str());
                        let pinging = self.nodes.pinging.iter().any(|p| p == &rec.id);
                        ui.push_id(&rec.id, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(icon).size(FONT_BODY).color(color));
                                let title = if is_target {
                                    format!("{} \u{2605}", rec.name)
                                } else {
                                    rec.name.clone()
                                };
                                ui.label(
                                    RichText::new(title)
                                        .size(FONT_SMALL)
                                        .strong()
                                        .color(palette.text),
                                );
                                if !rec.enabled {
                                    ui.label(
                                        RichText::new("disabled")
                                            .size(9.0)
                                            .color(palette.text_muted),
                                    );
                                }
                            });
                            ui.label(
                                RichText::new(format!(
                                    "{} · {} · {} · {} in flight",
                                    rec.drone_addr,
                                    rec.platform.label(),
                                    eff.label(),
                                    rec.in_flight
                                ))
                                .size(9.0)
                                .color(palette.text_muted),
                            );
                            ui.horizontal(|ui| {
                                let ping_label = if pinging { "Pinging..." } else { "Ping" };
                                if ui
                                    .add_enabled(
                                        !pinging,
                                        egui::Button::new(RichText::new(ping_label).size(9.0)),
                                    )
                                    .clicked()
                                {
                                    actions.push(RowAction::Ping(rec.id.clone()));
                                }
                                let target_label =
                                    if is_target { "Targeted" } else { "Build here" };
                                if ui
                                    .add_enabled(
                                        !is_target,
                                        egui::Button::new(RichText::new(target_label).size(9.0)),
                                    )
                                    .clicked()
                                {
                                    actions.push(RowAction::Target(rec.id.clone()));
                                }
                                if ui
                                    .button(RichText::new("Forget").size(9.0).color(palette.error))
                                    .clicked()
                                {
                                    actions.push(RowAction::Forget(rec.id.clone()));
                                }
                            });
                            ui.separator();
                        });
                    }
                });
        }

        for action in actions {
            match action {
                RowAction::Ping(id) => self.ping_node(id),
                RowAction::Target(id) => {
                    self.set_build_target(Some(id));
                }
                RowAction::Forget(id) => self.forget_node(&id),
            }
        }

        // ── Add-node form ──
        ui.add_space(ITEM_SPACING);
        if ui
            .button(
                RichText::new(format!("{} Add node", egui_phosphor::regular::PLUS))
                    .size(FONT_SMALL),
            )
            .clicked()
        {
            self.nodes.show_add = !self.nodes.show_add;
        }
        if self.nodes.show_add {
            // Stretch inputs to the panel width so hint text isn't truncated
            // (the rail is resizable, so hard-coded pixel widths always end up
            // either too narrow on a collapsed rail or wastefully short on a
            // widened one).
            ui.add_sized(
                [ui.available_width(), 22.0],
                egui::TextEdit::singleline(&mut self.nodes.add_name)
                    .hint_text("name")
                    .desired_width(f32::INFINITY),
            );
            ui.add_sized(
                [ui.available_width(), 22.0],
                egui::TextEdit::singleline(&mut self.nodes.add_addr)
                    .hint_text("drone address host:port")
                    .desired_width(f32::INFINITY),
            );
            ui.add_sized(
                [ui.available_width(), 22.0],
                egui::TextEdit::singleline(&mut self.nodes.add_token)
                    .hint_text("bearer token (optional)")
                    .password(true)
                    .desired_width(f32::INFINITY),
            );
            // Work-dir row: text field + Browse button. The button opens a
            // remote-directory modal that lists the drone's filesystem so
            // the operator picks a path instead of typing one blind.
            let browse_w = 62.0;
            let field_w = (ui.available_width() - browse_w - ui.spacing().item_spacing.x).max(60.0);
            ui.horizontal(|ui| {
                ui.add_sized(
                    [field_w, 22.0],
                    egui::TextEdit::singleline(&mut self.nodes.add_dir)
                        .hint_text("work dir on the node")
                        .desired_width(field_w),
                );
                if ui
                    .add_enabled(
                        !self.nodes.add_addr.trim().is_empty(),
                        egui::Button::new(RichText::new("Browse…").size(FONT_SMALL)),
                    )
                    .clicked()
                {
                    self.open_node_browser();
                }
            });
            ui.horizontal(|ui| {
                if ui.small_button("Register").clicked() {
                    self.add_node();
                }
                if ui.small_button("Cancel").clicked() {
                    self.nodes.show_add = false;
                }
            });
        }

        // ── Run a command on the target (builds/tests/etc. beyond the routed
        //    default) ──
        ui.add_space(SECTION_SPACING);
        ui.label(
            RichText::new("Run on target")
                .size(FONT_SMALL)
                .strong()
                .color(palette.text),
        );
        ui.add_sized(
            [ui.available_width(), 22.0],
            egui::TextEdit::singleline(&mut self.nodes.exec_cmd)
                .hint_text("e.g. cargo test --release")
                .desired_width(f32::INFINITY),
        );
        let can_run = self.routed_node().is_some()
            && !self.nodes.exec_busy
            && !self.nodes.exec_cmd.trim().is_empty();
        let run_label = if self.nodes.exec_busy {
            "Running..."
        } else {
            "Run on node"
        };
        if ui
            .add_enabled(
                can_run,
                egui::Button::new(RichText::new(run_label).size(FONT_SMALL)),
            )
            .clicked()
        {
            if let (Some(id), Some(node)) = (self.nodes.target_id.clone(), self.routed_node()) {
                let cmd = wrap_for_work_dir(&node.work_dir, self.nodes.exec_cmd.trim()).to_string();
                self.run_on_node(id, cmd, false);
            }
        }

        if !self.nodes.status_line.is_empty() {
            ui.add_space(ITEM_SPACING);
            ui.label(
                RichText::new(&self.nodes.status_line)
                    .size(9.0)
                    .color(palette.text_muted),
            );
        }
    }

    /// Cockpit-style remote-directory picker: a floating modal that lists the
    /// drone's filesystem so the operator can pick a work-dir instead of
    /// typing one. Called from `ui_render.rs` on every frame (guarded by
    /// `browse_open`) so it can appear above any other panel.
    pub fn render_node_browser(&mut self, ctx: &egui::Context) {
        if !self.nodes.browse_open {
            return;
        }
        let palette = self.palette();
        let area = egui::Area::new(egui::Id::new("node_browser_area"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO);

        // Collect-then-run: a click inside the closure mutates `self`, but
        // egui's closure has already borrowed it. Actions are queued and
        // applied after the area is drawn.
        enum Nav {
            Enter(String),
            Up,
            Go(String),
            Refresh,
            Select,
            Cancel,
        }
        let mut action: Option<Nav> = None;

        area.show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(ui.visuals().code_bg_color)
                .stroke(ui.visuals().window_stroke)
                .inner_margin(egui::Margin::same(12))
                .corner_radius(egui::CornerRadius::same(10))
                .show(ui, |ui| {
                    ui.set_width(460.0);
                    ui.label(
                        RichText::new("Select remote folder")
                            .size(FONT_BODY)
                            .strong()
                            .color(palette.accent),
                    );
                    ui.label(
                        RichText::new(format!("via {}", self.nodes.browse_addr))
                            .size(9.0)
                            .color(palette.text_muted),
                    );
                    ui.add_space(6.0);

                    // Path bar: editable text + Go + Up + Refresh.
                    ui.horizontal(|ui| {
                        let resp = ui.add_sized(
                            [300.0, 22.0],
                            egui::TextEdit::singleline(&mut self.nodes.browse_input)
                                .hint_text("/path/to/dir")
                                .desired_width(300.0),
                        );
                        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if (resp.lost_focus() && enter) || ui.small_button("Go").clicked() {
                            action = Some(Nav::Go(self.nodes.browse_input.clone()));
                        }
                        if ui
                            .add_enabled(
                                self.nodes.browse_parent.is_some(),
                                egui::Button::new(RichText::new("Up").size(FONT_SMALL)),
                            )
                            .clicked()
                        {
                            action = Some(Nav::Up);
                        }
                        if ui.small_button("Refresh").clicked() {
                            action = Some(Nav::Refresh);
                        }
                    });

                    // Name filter (client-side; the listing is already cached).
                    ui.add_sized(
                        [436.0, 20.0],
                        egui::TextEdit::singleline(&mut self.nodes.browse_filter)
                            .hint_text("filter names\u{2026}")
                            .desired_width(436.0),
                    );
                    ui.add_space(4.0);

                    if let Some(err) = &self.nodes.browse_error {
                        ui.label(RichText::new(err).size(FONT_SMALL).color(palette.error));
                        ui.add_space(4.0);
                    }
                    if self.nodes.browse_loading {
                        ui.label(
                            RichText::new("Loading\u{2026}")
                                .size(FONT_SMALL)
                                .color(palette.text_muted),
                        );
                    }
                    if self.nodes.browse_truncated {
                        ui.label(
                            RichText::new(
                                "Listing truncated at 500 entries — go deeper to narrow.",
                            )
                            .size(9.0)
                            .color(palette.warning),
                        );
                    }

                    let filter = self.nodes.browse_filter.to_lowercase();
                    let entries = self.nodes.browse_entries.clone();
                    egui::ScrollArea::vertical()
                        .id_salt("node_browser_list")
                        .auto_shrink([false, false])
                        .max_height(280.0)
                        .show(ui, |ui| {
                            for e in entries.iter() {
                                if !filter.is_empty() && !e.name.to_lowercase().contains(&filter) {
                                    continue;
                                }
                                let icon = if e.is_dir {
                                    egui_phosphor::regular::FOLDER_SIMPLE
                                } else {
                                    egui_phosphor::regular::FILE
                                };
                                let color = if e.is_dir {
                                    palette.text
                                } else {
                                    palette.text_muted
                                };
                                let label = if e.is_dir {
                                    format!("{}/", e.name)
                                } else {
                                    e.name.clone()
                                };
                                let btn = ui.add(
                                    egui::Button::new(
                                        RichText::new(format!("{icon}  {label}"))
                                            .size(FONT_SMALL)
                                            .color(color),
                                    )
                                    .fill(egui::Color32::TRANSPARENT),
                                );
                                if btn.clicked() && e.is_dir {
                                    action = Some(Nav::Enter(e.path.clone()));
                                } else if btn.hovered() && !e.is_dir {
                                    btn.on_hover_text("Files are not selectable as work dirs");
                                }
                            }
                            if entries.is_empty() && !self.nodes.browse_loading {
                                ui.label(
                                    RichText::new("(empty directory)")
                                        .size(FONT_SMALL)
                                        .italics()
                                        .color(palette.text_muted),
                                );
                            }
                        });

                    ui.add_space(6.0);
                    ui.separator();
                    ui.label(
                        RichText::new(format!("Current: {}", self.nodes.browse_path))
                            .size(9.0)
                            .color(palette.text_muted),
                    );
                    ui.horizontal(|ui| {
                        if ui
                            .button(
                                RichText::new("Select this folder")
                                    .strong()
                                    .size(FONT_SMALL),
                            )
                            .clicked()
                        {
                            action = Some(Nav::Select);
                        }
                        if ui.small_button("Cancel").clicked() {
                            action = Some(Nav::Cancel);
                        }
                    });
                });
        });

        match action {
            Some(Nav::Enter(p)) => self.request_browse_listing(p),
            Some(Nav::Up) => {
                if let Some(p) = self.nodes.browse_parent.clone() {
                    self.request_browse_listing(p);
                }
            }
            Some(Nav::Go(p)) => self.request_browse_listing(p),
            Some(Nav::Refresh) => {
                let p = self.nodes.browse_path.clone();
                self.request_browse_listing(p);
            }
            Some(Nav::Select) => self.accept_node_browser(),
            Some(Nav::Cancel) => self.close_node_browser(),
            None => {}
        }
        // Escape closes the modal without selecting.
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.close_node_browser();
        }
    }

    pub fn render_build_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        // Status indicator — only show after a build has been triggered
        let has_built = !self.status_message.is_empty();
        if has_built {
            let build_ok = self.build_errors_count == 0;
            ui.horizontal(|ui| {
                let (icon, color) = if build_ok {
                    (egui_phosphor::regular::CHECK, palette.success)
                } else {
                    (egui_phosphor::regular::X, palette.error)
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
                    RichText::new(egui_phosphor::regular::HAMMER)
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
                RichText::new(format!("{} Build", egui_phosphor::regular::PLAY))
                    .size(FONT_SMALL)
                    .color(palette.text),
            );
            if ui.add(build_btn).clicked() {
                self.status_message = "Building\u{2026}".to_string();
            }
            let run_btn = egui::Button::new(
                RichText::new(format!("{} Run", egui_phosphor::regular::PLAY))
                    .size(FONT_SMALL)
                    .color(palette.text),
            );
            if ui.add(run_btn).clicked() {
                self.status_message = "Running\u{2026}".to_string();
            }
            let stop_btn = egui::Button::new(
                RichText::new(format!("{} Stop", egui_phosphor::regular::STOP))
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
                RichText::new(format!(
                    "{} {}",
                    egui_phosphor::regular::CHECK,
                    snapshot.done_tasks
                ))
                .size(FONT_SMALL)
                .color(palette.success),
            );
            ui.label(
                RichText::new(format!(
                    "{} {}",
                    egui_phosphor::regular::X,
                    snapshot.failed_tasks
                ))
                .size(FONT_SMALL)
                .color(palette.error),
            );
            ui.label(
                RichText::new(format!(
                    "{} {}",
                    egui_phosphor::regular::PLAY,
                    snapshot.running_tasks
                ))
                .size(FONT_SMALL)
                .color(palette.warning),
            );
            ui.label(
                RichText::new(format!(
                    "{} {}",
                    egui_phosphor::regular::DOTS_THREE,
                    snapshot.pending_tasks
                ))
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
                    RichText::new(format!("{} running", egui_phosphor::regular::CIRCLE))
                        .size(9.0)
                        .color(palette.success),
                );
            }
        });
        if snapshot.has_dependency_cycle {
            ui.label(
                RichText::new(format!(
                    "{} Dependency cycle detected",
                    egui_phosphor::regular::WARNING
                ))
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

        // Mission activity: the same event stream the Timeline panel draws, but
        // scoped to whichever task Mission Control currently has selected (or
        // the whole mission when nothing is selected).
        ui.add_space(SECTION_SPACING);
        let selected_task_id = self.mission_control.selected_task_id;
        let activity_snapshot =
            crate::editor::task_timeline::TaskTimelineSnapshot::new(&self.task_timeline);
        egui::ScrollArea::vertical()
            .id_salt("mission_activity_feed_scroll")
            .max_height(220.0)
            .show(ui, |ui| {
                render_mission_activity_feed(ui, &activity_snapshot, selected_task_id, 40, palette);
            });
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
                    RichText::new(egui_phosphor::regular::LOCK)
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
            // Entries are keyed by the editor tab that owns the document, so
            // clicking one jumps to that tab. The activation is deferred until
            // the scroll area finishes, because the list borrows `nda_docs`.
            let activate = egui::ScrollArea::vertical()
                .show(ui, |ui| {
                    let mut activate = None;
                    for (tab_id, doc) in &self.nda_docs {
                        let title = doc.doc.title().unwrap_or("Untitled").to_string();
                        let status = if doc.sealed {
                            format!("{} Sealed", egui_phosphor::regular::LOCK)
                        } else {
                            format!("{} Open", egui_phosphor::regular::LOCK_OPEN)
                        };
                        let dirty_mark = if doc.dirty { " *" } else { "" };
                        let frame = egui::Frame::new()
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
                                    let rel =
                                        path.strip_prefix(&self.workspace_root).unwrap_or(path);
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
                        if frame
                            .response
                            .interact(egui::Sense::click())
                            .on_hover_text("Open this document's tab")
                            .clicked()
                        {
                            activate = Some(tab_id.clone());
                        }
                        ui.add_space(2.0);
                    }
                    activate
                })
                .inner;
            if let Some(id) = activate {
                self.activate_tab_by_id(&id);
            }
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
                    RichText::new(egui_phosphor::regular::PUZZLE_PIECE)
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
                RichText::new(egui_phosphor::regular::MAGNIFYING_GLASS)
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
            if !self.skill_filter.is_empty()
                && ui
                    .small_button(
                        RichText::new(egui_phosphor::regular::X)
                            .size(9.0)
                            .color(palette.text_muted),
                    )
                    .clicked()
            {
                self.skill_filter.clear();
            }
        });
        ui.add_space(ITEM_SPACING);

        if self.skill_files.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(egui_phosphor::regular::TARGET).size(24.0).color(palette.text_muted.gamma_multiply(0.5)));
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
                    RichText::new(egui_phosphor::regular::CHART_BAR)
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
                    ui.label(
                        RichText::new(format!(
                            "{} requests \u{00b7}",
                            format_count(total_requests)
                        ))
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
                                    "{} {} in",
                                    egui_phosphor::regular::ARROW_UP,
                                    format_count(usage.tokens_in)
                                ))
                                .size(9.0)
                                .color(palette.text_muted),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "{} {} out",
                                    egui_phosphor::regular::ARROW_DOWN,
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
