use egui;
use std::path::{Path, PathBuf};

use super::super::helpers::*;
use super::super::types::*;
use super::actions::{fuzzy_match_indices, fuzzy_subsequence};
use super::struct_def::VelocityApp;

impl VelocityApp {
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
                            .hint_text("Search commands\u{2026}")
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
                                            fmt.underline = egui::Stroke::new(1.0, palette.warning);
                                        }
                                        job.append(ch.encode_utf8(&mut buf), 0.0, fmt);
                                    }
                                    let resp = ui.selectable_label(selected, job);
                                    // Keep the keyboard-selected row in view.
                                    if selected {
                                        resp.scroll_to_me(Some(egui::Align::Center));
                                    }
                                    if let Some(shortcut) = cmd.shortcut {
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new(shortcut)
                                                        .small()
                                                        .monospace()
                                                        .color(
                                                            palette.text_muted.gamma_multiply(0.8),
                                                        ),
                                                );
                                            },
                                        );
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
                                    // U+2715 (✕) is not covered by the bundled fonts (tofu),
                                    // so render the close glyph through the Phosphor family.
                                    let close = egui::RichText::new(egui_phosphor::regular::X)
                                        .font(crate::editor::theme::icon_font_id(13.0));
                                    if ui.button(close).clicked() {
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
                        egui::ScrollArea::vertical()
                            .max_height(440.0)
                            .show(ui, |ui| {
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
                                        ui.label(
                                            egui::RichText::new(cmd.label).color(palette.text),
                                        );
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
                                                        egui::RichText::new("\u{2014}")
                                                            .small()
                                                            .color(
                                                                palette
                                                                    .text_muted
                                                                    .gamma_multiply(0.5),
                                                            ),
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

    /// "Clean Build Artifacts…" overlay (Ctrl+Shift+K): lists every recognized
    /// build-artifact tree, pre-checks the safe ones, shows review-classified
    /// trees disabled with their reason, and reclaims the selection behind a
    /// one-step confirm. Scanning and cleaning run on background threads; this
    /// render pass only drains their results, so a huge tree never blocks a
    /// frame.
    pub fn disk_hygiene_ui(&mut self, ctx: &egui::Context) {
        if !self.hygiene.open {
            return;
        }
        self.handle_hygiene_events();
        if self.hygiene.scanning || self.hygiene.cleaning {
            // Keep polling the channel while work is in flight.
            ctx.request_repaint();
        }
        let palette = self.palette();
        // Selection total, computed up front so the confirm button can name
        // the exact amount it is about to delete.
        let selected_bytes: u64 = self
            .hygiene
            .report
            .as_ref()
            .map(|r| {
                r.entries
                    .iter()
                    .filter(|e| self.hygiene.checked.contains(&e.relative_path))
                    .map(|e| e.size_bytes)
                    .sum()
            })
            .unwrap_or(0);
        let busy = self.hygiene.scanning || self.hygiene.cleaning;
        let mut open = true;
        let mut toggle: Option<String> = None;
        let mut want_dry = false;
        let mut want_confirm_arm = false;
        let mut want_reclaim = false;
        let mut want_disarm = false;

        egui::Area::new(egui::Id::new("disk_hygiene_overlay_area"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .fill(ui.visuals().code_bg_color)
                    .stroke(ui.visuals().window_stroke)
                    .inner_margin(egui::Margin::same(16))
                    .corner_radius(egui::CornerRadius::same(12))
                    .show(ui, |ui| {
                        ui.set_width(640.0);
                        ui.horizontal(|ui| {
                            ui.heading("Clean Build Artifacts");
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let close = egui::RichText::new(egui_phosphor::regular::X)
                                        .font(crate::editor::theme::icon_font_id(13.0));
                                    if ui.button(close).clicked() {
                                        open = false;
                                    }
                                },
                            );
                        });
                        if let Some(report) = &self.hygiene.report {
                            let reclaimable =
                                crate::disk_hygiene::format_bytes(report.total_reclaimable_bytes);
                            let free = crate::disk_hygiene::format_bytes(report.free_space_bytes);
                            let safe_count = report
                                .entries
                                .iter()
                                .filter(|e| e.safety == crate::disk_hygiene::Safety::Safe)
                                .count();
                            ui.label(
                                egui::RichText::new(format!(
                                    "Reclaimable {reclaimable} in {safe_count} tree(s) · Free: {free}"
                                ))
                                .small()
                                .color(palette.text_muted),
                            );
                        } else {
                            ui.label(
                                egui::RichText::new("Scanning workspace for artifact trees...")
                                    .small()
                                    .color(palette.text_muted),
                            );
                        }
                        ui.add_space(6.0);
                        ui.separator();
                        egui::ScrollArea::vertical()
                            .max_height(340.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                            let Some(report) = &self.hygiene.report else {
                                return;
                            };
                            if report.entries.is_empty() {
                                ui.label(
                                    egui::RichText::new(
                                        "No build-artifact trees found in this workspace.",
                                    )
                                    .italics()
                                    .color(palette.text_muted),
                                );
                                return;
                            }
                            for entry in &report.entries {
                                let is_safe =
                                    entry.safety == crate::disk_hygiene::Safety::Safe;
                                ui.horizontal(|ui| {
                                    if is_safe {
                                        let mut checked =
                                            self.hygiene.checked.contains(&entry.relative_path);
                                        if ui
                                            .checkbox(
                                                &mut checked,
                                                egui::RichText::new(&entry.relative_path)
                                                    .monospace()
                                                    .size(12.0),
                                            )
                                            .changed()
                                        {
                                            toggle = Some(entry.relative_path.clone());
                                        }
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new(
                                                        crate::disk_hygiene::format_bytes(
                                                            entry.size_bytes,
                                                        ),
                                                    )
                                                    .size(11.0)
                                                    .color(palette.text),
                                                );
                                            },
                                        );
                                    } else {
                                        // Review rows are display-only: the
                                        // reason is the whole point of them.
                                        ui.add_enabled_ui(false, |ui| {
                                            let mut never = false;
                                            ui.checkbox(
                                                &mut never,
                                                egui::RichText::new(&entry.relative_path)
                                                    .monospace()
                                                    .size(12.0),
                                            );
                                        });
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new(
                                                        entry
                                                            .reason
                                                            .as_deref()
                                                            .unwrap_or("review only"),
                                                    )
                                                    .size(10.0)
                                                    .color(palette.text_muted),
                                                );
                                            },
                                        );
                                    }
                                });
                            }
                            });
                        if let Some(line) = &self.hygiene.last_result {
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(line)
                                    .small()
                                    .color(palette.accent),
                            );
                        }
                        ui.add_space(8.0);
                        ui.separator();
                        ui.horizontal(|ui| {
                            if busy {
                                ui.spinner();
                                ui.label(
                                    egui::RichText::new(if self.hygiene.scanning {
                                        "Scanning..."
                                    } else {
                                        "Working..."
                                    })
                                    .small()
                                    .color(palette.text_muted),
                                );
                                return;
                            }
                            if self.hygiene.confirming {
                                let total =
                                    crate::disk_hygiene::format_bytes(selected_bytes);
                                if ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new(format!(
                                                "Confirm: delete {total}"
                                            ))
                                            .strong(),
                                        )
                                        .fill(palette.error),
                                    )
                                    .clicked()
                                {
                                    want_reclaim = true;
                                }
                                if ui.button("Cancel").clicked() {
                                    want_disarm = true;
                                }
                            } else {
                                let has_selection = !self.hygiene.checked.is_empty()
                                    && self.hygiene.report.is_some();
                                if ui
                                    .add_enabled(
                                        has_selection,
                                        egui::Button::new("Dry run"),
                                    )
                                    .clicked()
                                {
                                    want_dry = true;
                                }
                                if ui
                                    .add_enabled(
                                        has_selection && selected_bytes > 0,
                                        egui::Button::new(egui::RichText::new(format!(
                                            "Reclaim {}",
                                            crate::disk_hygiene::format_bytes(selected_bytes)
                                        )))
                                        .fill(palette.accent),
                                    )
                                    .clicked()
                                {
                                    want_confirm_arm = true;
                                }
                            }
                        });
                    });
            });

        // Apply the clicks collected during render (the closure only borrows
        // `self` immutably; state changes live here).
        if let Some(path) = toggle {
            if let Some(pos) = self.hygiene.checked.iter().position(|p| *p == path) {
                self.hygiene.checked.remove(pos);
            } else {
                self.hygiene.checked.push(path);
            }
        }
        if want_dry {
            self.start_hygiene_clean(true);
        }
        if want_confirm_arm {
            self.hygiene.confirming = true;
        }
        if want_disarm {
            self.hygiene.confirming = false;
        }
        if want_reclaim {
            self.hygiene.confirming = false;
            self.start_hygiene_clean(false);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            open = false;
            self.hygiene.confirming = false;
        }
        self.hygiene.open = open;
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

        self.quick_open.selected = self
            .quick_open
            .selected
            .min(filtered.len().saturating_sub(1));
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
                            .hint_text("Go to file\u{2026} (type to filter)")
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
                    let row_height =
                        ui.text_style_height(&egui::TextStyle::Body) + ui.spacing().item_spacing.y;
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
                            let icon =
                                crate::editor::search::icon_for_path(std::path::Path::new(&file));
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
                            self.quick_open.selected =
                                (self.quick_open.selected + 1) % filtered.len();
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

    /// Ctrl+Shift+W workspace switcher: quickly switch between known projects.
    pub fn workspace_switcher_ui(&mut self, ctx: &egui::Context) {
        if !self.workspace_switcher_open {
            return;
        }

        let palette = self.palette();
        let area = egui::Area::new(egui::Id::new("workspace_switcher_area"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_TOP, egui::Vec2::new(0.0, 80.0));

        let mut switcher_open = self.workspace_switcher_open;
        let project_count = self.projects.len();
        self.workspace_switcher_selected = self
            .workspace_switcher_selected
            .min(project_count.saturating_sub(1));

        area.show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(ui.visuals().code_bg_color)
                .stroke(ui.visuals().window_stroke)
                .inner_margin(egui::Margin::same(10))
                .corner_radius(egui::CornerRadius::same(12))
                .show(ui, |ui| {
                    ui.set_width(480.0);
                    ui.label(
                        egui::RichText::new("Switch Workspace")
                            .strong()
                            .color(palette.accent),
                    );
                    ui.add_space(6.0);
                    ui.separator();

                    if self.projects.is_empty() {
                        ui.add_space(18.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("No projects configured")
                                    .color(palette.text_muted),
                            );
                            ui.label(
                                egui::RichText::new("Use the Projects sidebar to add a workspace.")
                                    .small()
                                    .color(palette.text_muted),
                            );
                        });
                        ui.add_space(18.0);
                    } else {
                        let row_height = ui.text_style_height(&egui::TextStyle::Body)
                            + ui.spacing().item_spacing.y;
                        let mut scroll = egui::ScrollArea::vertical().max_height(280.0);
                        if self.workspace_switcher_just_opened {
                            scroll = scroll.vertical_scroll_offset(0.0);
                            self.workspace_switcher_just_opened = false;
                        }
                        scroll.show_rows(ui, row_height, project_count, |ui, row_range| {
                            for row in row_range {
                                let project_path = self.projects[row].clone();
                                let name = project_path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_else(|| project_path.to_string_lossy().to_string());
                                let is_current = crate::editor::file_ops::same_editor_path(
                                    &project_path,
                                    &self.workspace_root,
                                );
                                let selected = row == self.workspace_switcher_selected;

                                ui.horizontal(|ui| {
                                    let icon = if is_current { "\u{25cf}" } else { "\u{25cb}" };
                                    ui.label(egui::RichText::new(icon).size(11.0).color(
                                        if is_current {
                                            palette.success
                                        } else {
                                            palette.text_muted
                                        },
                                    ));
                                    let text = egui::RichText::new(&name).color(if selected {
                                        palette.accent
                                    } else {
                                        palette.text
                                    });
                                    let resp = ui.selectable_label(selected, text);
                                    if resp.clicked() {
                                        // Switch to this project.
                                        if !is_current {
                                            self.switch_workspace_to(project_path.clone());
                                        }
                                        switcher_open = false;
                                    }
                                });
                            }
                        });
                    }

                    // Keyboard navigation.
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        if project_count > 0 {
                            let selected = self.workspace_switcher_selected;
                            let is_current = self.projects.get(selected).is_some_and(|p| {
                                crate::editor::file_ops::same_editor_path(p, &self.workspace_root)
                            });
                            if !is_current {
                                if let Some(path) = self.projects.get(selected).cloned() {
                                    self.switch_workspace_to(path);
                                }
                            }
                        }
                        switcher_open = false;
                    } else if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                        if project_count > 0 {
                            self.workspace_switcher_selected =
                                (self.workspace_switcher_selected + 1) % project_count;
                        }
                    } else if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                        if project_count > 0 {
                            self.workspace_switcher_selected = self
                                .workspace_switcher_selected
                                .checked_sub(1)
                                .unwrap_or(project_count - 1);
                        }
                    } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        switcher_open = false;
                    }
                });
        });

        self.workspace_switcher_open = switcher_open;
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
                            .hint_text("Line number\u{2026}")
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

    /// Shift+F12 find-references results popup: list LSP references and jump to
    /// the selected one. Arrow keys navigate, Enter jumps, Escape closes.
    pub fn references_ui(&mut self, ctx: &egui::Context) {
        if !self.references_open {
            return;
        }
        let palette = self.palette();
        let area = egui::Area::new(egui::Id::new("references_area"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_TOP, egui::Vec2::new(0.0, 80.0));

        let mut open = self.references_open;
        let mut chosen: Option<usize> = None;
        let count = self.references_results.len();
        self.references_selected = self.references_selected.min(count.saturating_sub(1));

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            open = false;
        } else if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            if count > 0 {
                self.references_selected = (self.references_selected + 1) % count;
            }
        } else if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            if count > 0 {
                self.references_selected = (self.references_selected + count - 1) % count;
            }
        } else if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            chosen = Some(self.references_selected);
            open = false;
        }

        let results = self.references_results.clone();
        let selected = self.references_selected;
        area.show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(ui.visuals().code_bg_color)
                .stroke(ui.visuals().window_stroke)
                .inner_margin(egui::Margin::same(10))
                .corner_radius(egui::CornerRadius::same(12))
                .show(ui, |ui| {
                    ui.set_width(520.0);
                    ui.label(
                        egui::RichText::new(format!("References ({count})"))
                            .size(13.0)
                            .strong()
                            .color(palette.accent),
                    );
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .max_height(320.0)
                        .show(ui, |ui| {
                            for (idx, (path, line)) in results.iter().enumerate() {
                                let label = format!("{}:{}", path.display(), line);
                                let is_sel = idx == selected;
                                let mut clicked = false;
                                ui.horizontal(|ui| {
                                    ui.colored_label(
                                        if is_sel {
                                            palette.accent
                                        } else {
                                            palette.text_muted
                                        },
                                        "\u{1F50D}",
                                    );
                                    if ui.selectable_label(is_sel, &label).clicked() {
                                        clicked = true;
                                    }
                                });
                                if clicked {
                                    chosen = Some(idx);
                                    open = false;
                                }
                            }
                        });
                });
        });

        self.references_open = open;
        if let Some(idx) = chosen {
            if let Some((path, line)) = self.references_results.get(idx).cloned() {
                self.push_nav_location();
                self.open_editor(Some(path));
                self.pending_cursor_line = Some(line);
            }
        }
    }

    /// LSP call-hierarchy overlay: lists the callers (incoming) or callees
    /// (outgoing) of the symbol under the caret, labelled by function name and
    /// site. Arrow keys move the selection, Enter (or a click) opens the chosen
    /// site in the editor, Escape dismisses — the same interaction contract as
    /// the references panel, but preserving each node's name.
    pub fn call_hierarchy_ui(&mut self, ctx: &egui::Context) {
        if !self.call_hierarchy_open {
            return;
        }
        let palette = self.palette();
        let area = egui::Area::new(egui::Id::new("call_hierarchy_area"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_TOP, egui::Vec2::new(0.0, 80.0));

        let mut open = self.call_hierarchy_open;
        let mut chosen: Option<usize> = None;
        let count = self.call_hierarchy_items.len();
        self.call_hierarchy_selected = self.call_hierarchy_selected.min(count.saturating_sub(1));

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            open = false;
        } else if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            if count > 0 {
                self.call_hierarchy_selected = (self.call_hierarchy_selected + 1) % count;
            }
        } else if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            if count > 0 {
                self.call_hierarchy_selected = (self.call_hierarchy_selected + count - 1) % count;
            }
        } else if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            chosen = Some(self.call_hierarchy_selected);
            open = false;
        }

        let title = if self.call_hierarchy_incoming {
            "Incoming Calls"
        } else {
            "Outgoing Calls"
        };
        let incoming = self.call_hierarchy_incoming;
        let glyph = if incoming { "\u{21B0}" } else { "\u{21B1}" };
        let items = self.call_hierarchy_items.clone();
        let selected = self.call_hierarchy_selected;
        area.show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(ui.visuals().code_bg_color)
                .stroke(ui.visuals().window_stroke)
                .inner_margin(egui::Margin::same(10))
                .corner_radius(egui::CornerRadius::same(12))
                .show(ui, |ui| {
                    ui.set_width(560.0);
                    ui.label(
                        egui::RichText::new(format!("{title} ({count})"))
                            .size(13.0)
                            .strong()
                            .color(palette.accent),
                    );
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .max_height(320.0)
                        .show(ui, |ui| {
                            for (idx, item) in items.iter().enumerate() {
                                let site = format!("{}:{}", item.file.display(), item.line + 1);
                                let label = format!("{}  \u{2014}  {}", item.name, site);
                                let is_sel = idx == selected;
                                let mut clicked = false;
                                ui.horizontal(|ui| {
                                    ui.colored_label(
                                        if is_sel {
                                            palette.accent
                                        } else {
                                            palette.text_muted
                                        },
                                        glyph,
                                    );
                                    if ui.selectable_label(is_sel, &label).clicked() {
                                        clicked = true;
                                    }
                                });
                                if clicked {
                                    chosen = Some(idx);
                                    open = false;
                                }
                            }
                        });
                });
        });

        self.call_hierarchy_open = open;
        if let Some(idx) = chosen {
            if let Some(item) = self.call_hierarchy_items.get(idx).cloned() {
                self.push_nav_location();
                self.open_editor(Some(item.file));
                self.pending_cursor_line = Some(item.line + 1);
            }
        }
    }

    /// F2 LSP rename-symbol overlay: type the new name; Enter (or the Rename
    /// button) applies the server's workspace edit across every touched file,
    /// Escape (or Cancel) aborts. Focus jumps to the input on open.
    pub fn rename_ui(&mut self, ctx: &egui::Context) {
        if !self.rename_open {
            return;
        }
        let palette = self.palette();
        let area = egui::Area::new(egui::Id::new("rename_area"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_TOP, egui::Vec2::new(0.0, 80.0));

        let mut commit = false;
        let mut cancel = false;
        area.show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(ui.visuals().code_bg_color)
                .stroke(ui.visuals().window_stroke)
                .inner_margin(egui::Margin::same(10))
                .corner_radius(egui::CornerRadius::same(12))
                .show(ui, |ui| {
                    ui.set_width(420.0);
                    ui.label(
                        egui::RichText::new("Rename symbol")
                            .size(13.0)
                            .strong()
                            .color(palette.accent),
                    );
                    ui.add_space(4.0);
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.rename_input)
                            .hint_text("new symbol name")
                            .desired_width(420.0),
                    );
                    if self.rename_just_opened {
                        response.request_focus();
                        self.rename_just_opened = false;
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("Rename").color(palette.accent),
                            ))
                            .clicked()
                        {
                            commit = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
        });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        let entered = ctx.input(|i| i.key_pressed(egui::Key::Enter));
        if commit || entered {
            self.commit_rename();
        } else if cancel {
            self.rename_open = false;
            self.status_message = "Rename cancelled".into();
        }
    }

    /// Alt+Enter LSP code-action popup: list the quick fixes/refactorings the
    /// server offers for the caret line. Arrow keys navigate, Enter (or a
    /// click) applies the selected action's workspace edit, Escape closes.
    pub fn code_actions_ui(&mut self, ctx: &egui::Context) {
        if !self.code_actions_open {
            return;
        }
        let palette = self.palette();
        let area = egui::Area::new(egui::Id::new("code_actions_area"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_TOP, egui::Vec2::new(0.0, 80.0));

        let mut open = self.code_actions_open;
        let mut chosen: Option<usize> = None;
        let count = self.code_actions.len();
        self.code_action_selected = self.code_action_selected.min(count.saturating_sub(1));

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            open = false;
        } else if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            if count > 0 {
                self.code_action_selected = (self.code_action_selected + 1) % count;
            }
        } else if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            if count > 0 {
                self.code_action_selected = (self.code_action_selected + count - 1) % count;
            }
        } else if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            chosen = Some(self.code_action_selected);
            open = false;
        }

        let entries: Vec<(String, String, bool)> = self
            .code_actions
            .iter()
            .map(|a| {
                let has_edit = a.edit.as_ref().is_some_and(|e| !e.is_empty());
                // `via_command` marks actions resolved by running a server
                // command (they still apply, just through executeCommand).
                (
                    a.title.clone(),
                    a.kind.clone(),
                    !has_edit && a.command.is_some(),
                )
            })
            .collect();
        let selected = self.code_action_selected;
        area.show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(ui.visuals().code_bg_color)
                .stroke(ui.visuals().window_stroke)
                .inner_margin(egui::Margin::same(10))
                .corner_radius(egui::CornerRadius::same(12))
                .show(ui, |ui| {
                    ui.set_width(520.0);
                    ui.label(
                        egui::RichText::new(format!("Code Actions ({count})"))
                            .size(13.0)
                            .strong()
                            .color(palette.accent),
                    );
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .max_height(320.0)
                        .show(ui, |ui| {
                            for (idx, (title, kind, via_command)) in entries.iter().enumerate() {
                                let is_sel = idx == selected;
                                // Command-only actions run via
                                // `workspace/executeCommand`; tag them so the
                                // user knows applying one does a server round-trip.
                                let label = format!(
                                    "{}{}",
                                    title,
                                    if *via_command {
                                        format!("  [{kind} \u{00b7} server command]")
                                    } else {
                                        String::new()
                                    }
                                );
                                let mut clicked = false;
                                ui.horizontal(|ui| {
                                    ui.colored_label(
                                        if is_sel {
                                            palette.accent
                                        } else {
                                            palette.text_muted
                                        },
                                        "\u{1F4A1}",
                                    );
                                    if ui.selectable_label(is_sel, &label).clicked() {
                                        clicked = true;
                                    }
                                });
                                if clicked {
                                    chosen = Some(idx);
                                    open = false;
                                }
                            }
                        });
                });
        });

        self.code_actions_open = open;
        if let Some(idx) = chosen {
            self.code_action_selected = idx.min(count.saturating_sub(1));
            self.apply_selected_code_action();
        }
    }

    /// Render the live LSP parameter-hint popup for the call the caret sits
    /// inside. Self-dismisses: it clears once the caret leaves the call's
    /// parentheses (the closing `)` is typed, the buffer changes, or the user
    /// hits Escape), so it never lingers as stale chrome. The active parameter
    /// is emphasized inside the monospace signature label; overloads are paged
    /// by the server and shown as an `i/n` badge.
    pub fn signature_help_ui(&mut self, ctx: &egui::Context) {
        if self.signature_help.is_none() {
            return;
        }

        // Recompute whether the caret is still inside an unclosed call. If the
        // user closed the parens or moved away, drop the hint.
        let still_inside = self
            .active_tab
            .as_ref()
            .and_then(|id| self.buffers.get(id).map(|b| b.content().to_owned()))
            .map(|content| {
                let off = crate::editor::line_ops::offset_of_line_col(
                    &content,
                    self.current_cursor_line,
                    self.current_cursor_col,
                );
                let before: String = content.chars().take(off).collect();
                crate::editor::lsp_client::caret_inside_parens(&before)
            })
            .unwrap_or(false);
        if !still_inside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.signature_help = None;
            return;
        }

        let palette = self.palette();
        let Some(help) = self.signature_help.clone() else {
            return;
        };
        let Some(sig) = help.active().cloned() else {
            return;
        };
        let active_param = help.active_param_label().map(|s| s.to_string());
        let total = help.signatures.len();
        let idx = help.active_signature;

        let area = egui::Area::new(egui::Id::new("signature_help_area"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_BOTTOM, egui::Vec2::new(0.0, -48.0));
        area.show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(ui.visuals().code_bg_color)
                .stroke(ui.visuals().window_stroke)
                .inner_margin(egui::Margin::same(8))
                .corner_radius(egui::CornerRadius::same(8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let label = &sig.label;
                        let highlight = active_param
                            .as_deref()
                            .filter(|p| !p.is_empty())
                            .and_then(|p| label.find(p));
                        match highlight {
                            Some(pos) => {
                                let plen = active_param.as_deref().unwrap_or("").len();
                                let end = (pos + plen).min(label.len());
                                ui.label(
                                    egui::RichText::new(&label[..pos])
                                        .monospace()
                                        .color(palette.text),
                                );
                                ui.label(
                                    egui::RichText::new(&label[pos..end])
                                        .monospace()
                                        .strong()
                                        .color(palette.accent),
                                );
                                ui.label(
                                    egui::RichText::new(&label[end..])
                                        .monospace()
                                        .color(palette.text),
                                );
                            }
                            None => {
                                ui.label(
                                    egui::RichText::new(label).monospace().color(palette.text),
                                );
                            }
                        }
                        if total > 1 {
                            ui.separator();
                            ui.colored_label(palette.text_muted, format!("{}/{}", idx + 1, total));
                        }
                    });
                    let doc = sig.documentation.trim();
                    if !doc.is_empty() {
                        ui.add_space(2.0);
                        ui.label(
                            egui::RichText::new(doc)
                                .size(11.0)
                                .color(palette.text_muted),
                        );
                    }
                });
        });
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
        // Feed in language-server symbols: debounced `workspace/symbol`
        // dispatch + non-blocking polling; a merge rewrites the entry list
        // and forces the re-filter below in the same frame.
        self.update_goto_symbol_lsp(&query, ctx);
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
                            .hint_text("Go to symbol\u{2026} (type to filter)")
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
                            // An empty list has three honest explanations:
                            // the language server is still loading or has no
                            // provider (its note), or the sitemap is empty.
                            let note = self.goto_symbol_lsp_note.clone();
                            let msg = note.unwrap_or_else(|| {
                                if self.goto_symbol_entries.is_empty() {
                                    "No symbols indexed yet \u{2014} run the indexer first"
                                } else {
                                    "No matching symbols"
                                }
                                .to_string()
                            });
                            ui.label(egui::RichText::new(msg).color(palette.text_muted));
                        });
                        ui.add_space(18.0);
                    }

                    // Virtualized: render only the visible rows.
                    let row_height =
                        ui.text_style_height(&egui::TextStyle::Body) + ui.spacing().item_spacing.y;
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
                            let icon = crate::editor::search::icon_for_path(std::path::Path::new(
                                &entry.file,
                            ));
                            let file_label = entry.file.clone();
                            let name = entry.name.clone();
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("\u{0192}")
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

    /// Ctrl+Tab most-recently-used tab switcher.
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
        if !cmd_held {
            let chosen = self.mru.order.get(self.mru.selected).cloned();
            self.mru.open = false;
            if let Some(id) = chosen {
                self.activate_tab_by_id(&id);
            }
            return;
        }
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
                        egui::ScrollArea::vertical()
                            .max_height(320.0)
                            .show(ui, |ui| {
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

    /// Answer the open-file prompt with the path it was given, and load it.
    ///
    /// Both prompts label their box "relative to workspace" and neither one used
    /// to enforce that: `workspace_root.join(typed)` returns `typed` untouched
    /// when it is absolute, and walks out of the tree when it holds `..`, so the
    /// dialog would load -- and, on the save side, write -- anywhere on disk.
    /// The bridge's own `OpenFile` command already refuses exactly that, which
    /// made the prompt the weaker of two doors to the same file; a driver that
    /// cannot pass `gui_open_file` can type the path into the dialog instead.
    /// So the same rule now applies here, since a person typing a path and a
    /// driver answering the prompt over the bridge are the same caller.
    ///
    /// Returns the file that was opened. On refusal the prompt stays up, because
    /// a mistyped path is worth correcting rather than destroying; a caller that
    /// has given up uses `dismiss_transient_ui`.
    ///
    /// Extracted from the render closure so the path that actually loads a
    /// buffer is reachable without painting a frame and clicking a button.
    pub fn submit_open_dialog(&mut self, typed: &str) -> Result<PathBuf, String> {
        let typed = typed.trim();
        if typed.is_empty() {
            return Err("Nothing was typed into the Open File prompt.".to_string());
        }
        let resolved = crate::security::sanitize::sanitize_path(typed, &self.workspace_root)
            .map_err(|e| format!("Cannot open {typed}: {e}"))?;
        // Canonicalising is how containment is decided, but the verbatim prefix it
        // hands back would end up in the tab title and every later path join.
        let resolved = super::gui_commands::plain_windows_path(&resolved);
        if !resolved.is_file() {
            return Err(format!("{} is not a file.", resolved.display()));
        }
        self.open_editor(Some(resolved.clone()));
        self.pending_open_path = None;
        Ok(resolved)
    }

    /// Answer the save-as prompt with the path it was given, and write there.
    ///
    /// Confined to the workspace for the same reason as
    /// [`Self::submit_open_dialog`], and this one is the harder case: the old
    /// button wrote the buffer wherever the typed string pointed and then
    /// re-pointed the tab at it, so a single prompt could move an editor's
    /// on-disk home outside the project it belongs to.
    ///
    /// Unlike the button it replaces, a failed write leaves the prompt up: the
    /// path is what the caller got wrong, so discarding it destroys their work.
    pub fn submit_save_as_dialog(&mut self, typed: &str) -> Result<PathBuf, String> {
        let typed = typed.trim();
        if typed.is_empty() {
            return Err("Nothing was typed into the Save As prompt.".to_string());
        }
        let id = self
            .active_tab
            .clone()
            .ok_or_else(|| "No active editor to save".to_string())?;
        let resolved = crate::security::sanitize::sanitize_path(typed, &self.workspace_root)
            .map_err(|e| format!("Cannot save as {typed}: {e}"))?;
        let resolved = super::gui_commands::plain_windows_path(&resolved);
        if resolved.is_dir() {
            return Err(format!("{} is a directory.", resolved.display()));
        }
        // `save_buffer_to` reports an OS failure in the status bar and as a toast,
        // so the refusal only has to stop the tab being re-pointed at nothing.
        if !self.save_buffer_to(&id, &resolved) {
            return Err(format!("Could not write {}.", resolved.display()));
        }
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            if let TabKind::Editor { ref mut path, .. } = tab.kind {
                *path = Some(resolved.clone());
            }
        }
        self.pending_save_as_path = None;
        Ok(resolved)
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
        let palette = self.palette();
        let workspace_root = self.workspace_root.clone();
        egui::Window::new("Open File")
            .open(&mut open)
            .resizable(true)
            .collapsible(false)
            .default_size((480.0, 360.0))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("Workspace")
                                .size(9.0)
                                .strong()
                                .color(palette.text_muted),
                        );
                        ui.add_space(2.0);
                        let tree = build_file_tree(&workspace_root);
                        let decorations = self.git_state.decorations(&workspace_root);
                        let mut tree_actions = Vec::new();
                        egui::ScrollArea::vertical()
                            .max_width(220.0)
                            .show(ui, |ui| {
                                Self::render_file_tree_node(
                                    ui,
                                    &tree,
                                    &workspace_root,
                                    &mut path_string,
                                    palette,
                                    &mut tree_actions,
                                    &decorations,
                                );
                            });
                        if !tree_actions.is_empty() {
                            self.run_tree_actions(ctx, tree_actions);
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("File path (relative to workspace):")
                                .size(9.0)
                                .color(palette.text_muted),
                        );
                        ui.add_space(2.0);
                        if ui
                            .add(egui::TextEdit::singleline(&mut path_string).desired_width(200.0))
                            .changed()
                        {
                            self.pending_open_path = Some(PathBuf::from(&path_string));
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Open").clicked() {
                                if let Err(e) = self.submit_open_dialog(&path_string) {
                                    self.status_message = e;
                                }
                            }
                            if ui.button("Cancel").clicked() {
                                self.pending_open_path = None;
                            }
                        });
                    });
                });
            });
        if !open {
            self.pending_open_path = None;
        }
    }

    pub(crate) fn render_file_tree_node(
        ui: &mut egui::Ui,
        node: &FileNode,
        workspace_root: &Path,
        path_string: &mut String,
        palette: crate::editor::theme::IdePalette,
        actions: &mut Vec<crate::editor::file_ops::TreeAction>,
        decorations: &std::collections::HashMap<PathBuf, crate::editor::git_ui::GitFileStatus>,
    ) {
        /// VS Code-style: a changed file (or folder) wears its status color.
        fn deco_color(
            decorations: &std::collections::HashMap<PathBuf, crate::editor::git_ui::GitFileStatus>,
            path: &Path,
        ) -> Option<egui::Color32> {
            decorations.get(path).map(|s| {
                egui::Color32::from_rgb(
                    s.decoration_rgb().0,
                    s.decoration_rgb().1,
                    s.decoration_rgb().2,
                )
            })
        }
        if node.is_dir {
            if let Some(children) = &node.children {
                let dir_name = if node.path == workspace_root {
                    workspace_root
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string()
                } else {
                    node.name.clone()
                };
                let collapsing = egui::CollapsingHeader::new(
                    // No manual "▸": CollapsingHeader already paints its own
                    // disclosure triangle, and the U+25B8 glyph isn't covered by the
                    // bundled fonts, so the hand-added arrow rendered as a tofu box.
                    egui::RichText::new(dir_name)
                        .size(10.0)
                        .color(deco_color(decorations, &node.path).unwrap_or(palette.text)),
                )
                .default_open(node.path == workspace_root)
                .show(ui, |ui| {
                    for child in children {
                        Self::render_file_tree_node(
                            ui,
                            child,
                            workspace_root,
                            path_string,
                            palette,
                            actions,
                            decorations,
                        );
                    }
                });
                let header = collapsing.header_response;
                let header = match decorations.get(&node.path) {
                    Some(status) => {
                        header.on_hover_text(format!("Contains {} changes", status.label()))
                    }
                    None => header,
                };
                header.context_menu(|ui| Self::tree_context_menu(ui, &node.path, true, actions));
            }
        } else {
            let rel = node
                .path
                .strip_prefix(workspace_root)
                .unwrap_or(&node.path)
                .to_string_lossy()
                .to_string();
            let icon = crate::editor::search::icon_for_path(&node.path);
            let file_color = deco_color(decorations, &node.path).unwrap_or(palette.text);
            let resp = ui.add(
                egui::Button::new(
                    egui::RichText::new(format!("{} {}", icon, rel))
                        .size(9.0)
                        .color(file_color),
                )
                .frame(false),
            );
            if resp.clicked() {
                *path_string = rel;
            }
            let resp = match decorations.get(&node.path) {
                Some(status) => resp.on_hover_text(status.label()),
                None => resp,
            };
            resp.context_menu(|ui| Self::tree_context_menu(ui, &node.path, false, actions));
        }
    }

    /// Rows of the explorer right-click menu. Purely declarative: each entry
    /// pushes a [`TreeAction`](crate::editor::file_ops::TreeAction) for
    /// `render_file_tree_subpanel` to execute centrally (with dialogs and
    /// confirmations) — the menu itself never touches the filesystem.
    fn tree_context_menu(
        ui: &mut egui::Ui,
        path: &Path,
        is_dir: bool,
        actions: &mut Vec<crate::editor::file_ops::TreeAction>,
    ) {
        use crate::editor::file_ops::TreeAction;
        let danger =
            egui::RichText::new(format!("{} Delete\u{2026}", egui_phosphor::regular::TRASH))
                .color(egui::Color32::from_rgb(205, 92, 92));
        if is_dir {
            if ui
                .button(format!(
                    "{} New File\u{2026}",
                    egui_phosphor::regular::FILE_PLUS
                ))
                .clicked()
            {
                actions.push(TreeAction::NewFile(path.to_path_buf()));
                ui.close();
            }
            if ui
                .button(format!(
                    "{} New Folder\u{2026}",
                    egui_phosphor::regular::FOLDER_PLUS
                ))
                .clicked()
            {
                actions.push(TreeAction::NewFolder(path.to_path_buf()));
                ui.close();
            }
            ui.separator();
        }
        if ui.button("Copy Path").clicked() {
            actions.push(TreeAction::CopyPath(path.to_path_buf()));
            ui.close();
        }
        if ui.button("Rename\u{2026}").clicked() {
            actions.push(TreeAction::Rename(path.to_path_buf()));
            ui.close();
        }
        if ui.button(danger).clicked() {
            actions.push(TreeAction::Delete(path.to_path_buf()));
            ui.close();
        }
    }

    /// Execute the actions collected from explorer context menus (from both
    /// the sidebar tree and the Open File dialog). Clipboard acts immediately;
    /// naming acts open the entry dialog; delete opens a confirmation.
    pub fn run_tree_actions(
        &mut self,
        ctx: &egui::Context,
        actions: Vec<crate::editor::file_ops::TreeAction>,
    ) {
        use crate::editor::file_ops::{FileEntryDialog, TreeAction};
        for action in actions {
            match action {
                TreeAction::CopyPath(p) => {
                    ctx.copy_text(p.display().to_string());
                    self.status_message = format!("Copied path: {}", p.display());
                }
                TreeAction::NewFile(_) | TreeAction::NewFolder(_) | TreeAction::Rename(_) => {
                    self.file_entry_dialog = FileEntryDialog::for_action(&action);
                }
                TreeAction::Delete(p) => self.pending_tree_delete = Some(p),
            }
        }
    }

    /// The name prompt behind New File / New Folder / Rename. Applies the
    /// tested `file_ops` helpers; errors keep the dialog open with the typed
    /// text so the user can fix the name in place.
    pub fn file_entry_dialog_ui(&mut self, ctx: &egui::Context) {
        use crate::editor::file_ops::{create_file_on_disk, create_folder_on_disk, rename_on_disk};
        let Some(dialog) = self.file_entry_dialog.clone() else {
            return;
        };
        let palette = self.palette();
        let mut open = true;
        let mut dismissed = false;
        let mut value = dialog.value.clone();
        let mut accept = false;
        egui::Window::new(dialog.title())
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                let target = match &dialog.mode {
                    crate::editor::file_ops::FileEntryMode::NewFile { dir }
                    | crate::editor::file_ops::FileEntryMode::NewFolder { dir } => {
                        format!("in {}", dir.display())
                    }
                    crate::editor::file_ops::FileEntryMode::Rename { path } => {
                        format!("{}", path.display())
                    }
                };
                ui.label(
                    egui::RichText::new(&target)
                        .size(9.0)
                        .color(palette.text_muted),
                );
                let resp = ui.add_sized(
                    [260.0, 0.0],
                    egui::TextEdit::singleline(&mut value).hint_text("name"),
                );
                resp.request_focus();
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("OK").clicked() || (enter && resp.has_focus()) {
                    accept = true;
                }
                if ui.button("Cancel").clicked() {
                    dismissed = true;
                }
            });
        if dismissed {
            open = false;
        }
        if accept {
            let result = match &dialog.mode {
                crate::editor::file_ops::FileEntryMode::NewFile { dir } => {
                    create_file_on_disk(dir, &value).map(|_| None)
                }
                crate::editor::file_ops::FileEntryMode::NewFolder { dir } => {
                    create_folder_on_disk(dir, &value).map(|_| None)
                }
                crate::editor::file_ops::FileEntryMode::Rename { path } => {
                    rename_on_disk(path, &value).map(Some)
                }
            };
            match result {
                Ok(renamed_to) => {
                    self.file_entry_dialog = None;
                    self.request_tree_refresh();
                    if let (Some(new), crate::editor::file_ops::FileEntryMode::Rename { path }) =
                        (renamed_to, &dialog.mode)
                    {
                        self.reroute_tab_after_rename(path, &new);
                    }
                }
                Err(e) => {
                    // Keep the dialog open, preserving the typed text.
                    if let Some(d) = &mut self.file_entry_dialog {
                        d.value = value;
                    }
                    self.toasts.push(crate::editor::toast::Toast::error(e));
                }
            }
        } else if !open {
            self.file_entry_dialog = None;
        }
    }

    /// After a successful rename, move any open (clean) tab from the old path
    /// to the new one. Dirty tabs are left untouched — their unsaved buffer
    /// still belongs to the old path until the user saves it.
    fn reroute_tab_after_rename(&mut self, old: &Path, new: &Path) {
        let affected: Vec<crate::editor::app::types::TabId> = self
            .tabs
            .iter()
            .filter(|t| t.editor_path() == Some(&old.to_path_buf()))
            .map(|t| t.id.clone())
            .collect();
        if affected.is_empty() {
            return;
        }
        let mut reopened = false;
        for id in affected {
            if self.tab_is_dirty(&id) {
                continue;
            }
            self.close_tab(&id);
            reopened = true;
        }
        if reopened {
            self.open_editor(Some(new.to_path_buf()));
            self.rebuild_dock();
        }
    }

    /// Explicit confirmation before any on-disk delete; also closes clean
    /// editor tabs that pointed at the removed entry.
    pub fn confirm_tree_delete_ui(&mut self, ctx: &egui::Context) {
        use crate::editor::file_ops::delete_on_disk;
        let Some(path) = self.pending_tree_delete.clone() else {
            return;
        };
        let palette = self.palette();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        let mut open = true;
        let mut dismissed = false;
        let mut confirm = false;
        egui::Window::new(format!("Delete {name}?"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(format!("{}", path.display()))
                        .monospace()
                        .size(9.0)
                        .color(palette.text_muted),
                );
                ui.label(
                    egui::RichText::new(
                        "This removes the entry from disk (folders recursively), not the OS trash.",
                    )
                    .size(10.0)
                    .color(palette.text),
                );
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("Delete")
                                .color(egui::Color32::from_rgb(205, 92, 92)),
                        ))
                        .clicked()
                    {
                        confirm = true;
                    }
                    if ui.button("Cancel").clicked() {
                        dismissed = true;
                    }
                });
            });
        if dismissed {
            open = false;
        }
        if confirm {
            match delete_on_disk(&path, &self.workspace_root) {
                Ok(()) => {
                    // Close clean tabs on the deleted path so no editor points
                    // at a ghost. Dirty tabs stay (unsaved work is the user's).
                    let affected: Vec<crate::editor::app::types::TabId> = self
                        .tabs
                        .iter()
                        .filter(|t| {
                            t.editor_path()
                                .map(|p| {
                                    p.as_path() == path.as_path()
                                        || (path.is_dir() && p.starts_with(&path))
                                })
                                .unwrap_or(false)
                        })
                        .map(|t| t.id.clone())
                        .collect();
                    let mut any = false;
                    for id in affected {
                        if !self.tab_is_dirty(&id) {
                            self.close_tab(&id);
                            any = true;
                        }
                    }
                    if any {
                        self.rebuild_dock();
                    }
                    self.toasts
                        .push(crate::editor::toast::Toast::success(format!(
                            "Deleted {name}"
                        )));
                    self.request_tree_refresh();
                }
                Err(e) => {
                    self.toasts.push(crate::editor::toast::Toast::error(e));
                }
            }
            self.pending_tree_delete = None;
        } else if !open {
            self.pending_tree_delete = None;
        }
    }

    /// Render a file tree node with a case-insensitive filter. Directories are
    /// expanded only if they (or their descendants) contain a match. Filtered
    /// rows get the same right-click menu as the unfiltered tree.
    pub(crate) fn render_file_tree_node_filtered(
        ui: &mut egui::Ui,
        node: &FileNode,
        workspace_root: &Path,
        path_string: &mut String,
        palette: crate::editor::theme::IdePalette,
        filter: &str,
        actions: &mut Vec<crate::editor::file_ops::TreeAction>,
        decorations: &std::collections::HashMap<PathBuf, crate::editor::git_ui::GitFileStatus>,
    ) {
        let filter_lower = filter.to_lowercase();
        Self::render_file_tree_node_filtered_inner(
            ui,
            node,
            workspace_root,
            path_string,
            palette,
            &filter_lower,
            actions,
            decorations,
        );
    }

    fn render_file_tree_node_filtered_inner(
        ui: &mut egui::Ui,
        node: &FileNode,
        workspace_root: &Path,
        path_string: &mut String,
        palette: crate::editor::theme::IdePalette,
        filter_lower: &str,
        actions: &mut Vec<crate::editor::file_ops::TreeAction>,
        decorations: &std::collections::HashMap<PathBuf, crate::editor::git_ui::GitFileStatus>,
    ) -> bool {
        if node.is_dir {
            if let Some(children) = &node.children {
                let mut any_child_matched = false;
                for child in children {
                    if Self::render_file_tree_node_filtered_inner(
                        ui,
                        child,
                        workspace_root,
                        path_string,
                        palette,
                        filter_lower,
                        actions,
                        decorations,
                    ) {
                        any_child_matched = true;
                    }
                }
                return any_child_matched;
            }
            false
        } else {
            let rel = node
                .path
                .strip_prefix(workspace_root)
                .unwrap_or(&node.path)
                .to_string_lossy()
                .to_string();
            let name_lower = node.name.to_lowercase();
            if name_lower.contains(filter_lower) || rel.to_lowercase().contains(filter_lower) {
                let icon = crate::editor::search::icon_for_path(&node.path);
                let status = decorations.get(&node.path);
                let text_color = status
                    .map(|s| {
                        let (r, g, b) = s.decoration_rgb();
                        egui::Color32::from_rgb(r, g, b)
                    })
                    .unwrap_or(palette.text);
                let resp = ui.add(
                    egui::Button::new(
                        egui::RichText::new(format!("{} {}", icon, rel))
                            .size(9.0)
                            .color(text_color),
                    )
                    .frame(false),
                );
                if resp.clicked() {
                    *path_string = rel;
                }
                let resp = match status {
                    Some(status) => resp.on_hover_text(status.label()),
                    None => resp,
                };
                resp.context_menu(|ui| Self::tree_context_menu(ui, &node.path, false, actions));
                true
            } else {
                false
            }
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
                        if let Err(e) = self.submit_save_as_dialog(&path_string) {
                            self.status_message = e;
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
                    egui::RichText::new(format!("\u{201c}{name}\u{201d} has unsaved changes."))
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
                    self.active_tab = Some(id);
                    self.pending_close_tab = None;
                    self.save_active_as();
                }
            }
            Some("discard") => {
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
                if let Some(cp) = &active_change_preview {
                    ui.label(
                        egui::RichText::new(format!(
                            "{}  (+{} / -{})",
                            cp.file_label, cp.added_lines, cp.removed_lines
                        ))
                        .strong()
                        .color(palette.warning),
                    );
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(cp.full_diff.as_str())
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

    /// The transient overlays currently up, in a fixed order.
    ///
    /// This is the one list that says what counts as transient: it backs both
    /// the bridge's read-only report and [`Self::dismiss_transient_ui`], so a
    /// driver can never be told about fewer popups than the dismiss route
    /// stands down.
    pub fn open_transient_ui(&self) -> Vec<&'static str> {
        let mut up = Vec::new();
        if self.command_palette.open {
            up.push("command palette");
        }
        if self.quick_open.open {
            up.push("quick open");
        }
        if self.mru.open {
            up.push("tab switcher");
        }
        if self.workspace_switcher_open {
            up.push("workspace switcher");
        }
        if self.goto_line_open {
            up.push("go to line");
        }
        if self.goto_symbol_open {
            up.push("go to symbol");
        }
        if self.references_open {
            up.push("references");
        }
        if self.rename_open {
            up.push("rename symbol");
        }
        if self.code_actions_open {
            up.push("code actions");
        }
        if self.show_shortcuts {
            up.push("keyboard shortcuts");
        }
        if self.hygiene.open {
            up.push("disk hygiene");
        }
        if self.show_full_diff {
            up.push("full diff");
        }
        // These three are modelled as a pending value rather than a flag, where
        // anything at all means the prompt is on screen.
        if self.pending_open_path.is_some() {
            up.push("open file dialog");
        }
        if self.pending_save_as_path.is_some() {
            up.push("save as dialog");
        }
        if self.pending_close_tab.is_some() {
            up.push("close confirmation");
        }
        // Find/replace lives on the buffer, not the window, so it is only up
        // when the focused editor is showing it.
        if let Some(id) = &self.active_tab {
            if self
                .buffers
                .get(id)
                .is_some_and(|buf| buf.find_replace.visible)
            {
                up.push("find and replace");
            }
        }
        up
    }

    /// Stand down every transient popup, the way a person tapping Escape out of
    /// a stack of dialogs would.
    ///
    /// Each `*_ui` above handles its own Escape, but only while it owns the
    /// frame, which means the key works for someone watching the window and not
    /// for a driver several calls away that has lost track of what it raised.
    /// This is the same reset applied from outside the frame loop, and it is
    /// deliberately exhaustive: clearing `quick_open` while leaving
    /// `command_palette` up would just move the stall somewhere else.
    ///
    /// Nothing is accepted on the way out. The open-file, save-as and
    /// close-tab prompts are cancelled rather than confirmed -- the same result
    /// as their Cancel buttons -- so a driver can raise a destructive prompt
    /// over the bridge and stand it back down without a file being touched.
    ///
    /// Returns the overlays it closed, so a caller can tell "nothing was open"
    /// from "four things just went away".
    pub fn dismiss_transient_ui(&mut self) -> Vec<&'static str> {
        let closed = self.open_transient_ui();
        if closed.is_empty() {
            return closed;
        }
        self.command_palette.open = false;
        self.quick_open.open = false;
        self.mru.open = false;
        self.workspace_switcher_open = false;
        self.goto_line_open = false;
        self.goto_symbol_open = false;
        self.references_open = false;
        self.rename_open = false;
        self.code_actions_open = false;
        self.show_shortcuts = false;
        // Standing down the hygiene overlay cancels it — same as its Cancel
        // button or Escape: nothing is deleted on the way out, even mid-confirm.
        self.hygiene.open = false;
        self.hygiene.confirming = false;
        self.show_full_diff = false;
        self.pending_open_path = None;
        self.pending_save_as_path = None;
        // Cancelling the prompt leaves the dirty tab open, exactly as pressing
        // Cancel in it would.
        self.pending_close_tab = None;
        if let Some(id) = self.active_tab.clone() {
            if let Some(buf) = self.buffers.get_mut(&id) {
                buf.find_replace.close();
            }
        }
        closed
    }

    /// Git branch switcher overlay: filterable list of branches with
    /// keyboard navigation, Enter to checkout, and 'n' to create new.
    pub fn branch_switcher_ui(&mut self, ctx: &egui::Context) {
        if !self.branch_switcher_open {
            return;
        }
        let palette = self.palette();
        let area = egui::Area::new(egui::Id::new("branch_switcher_area"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_TOP, egui::Vec2::new(0.0, 100.0));

        let mut open = self.branch_switcher_open;
        let mut checkout: Option<String> = None;
        let mut create: Option<String> = None;

        area.show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(ui.visuals().code_bg_color)
                .stroke(ui.visuals().window_stroke)
                .inner_margin(egui::Margin::same(10))
                .corner_radius(egui::CornerRadius::same(12))
                .show(ui, |ui| {
                    ui.set_width(320.0);

                    if self.branch_creating {
                        // ── Create-new-branch mode ──
                        ui.label(
                            egui::RichText::new("Create new branch")
                                .size(13.0)
                                .strong()
                                .color(palette.accent),
                        );
                        ui.add_space(4.0);
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut self.branch_new_name)
                                .hint_text("Branch name\u{2026}")
                                .desired_width(300.0),
                        );
                        if self.branch_just_opened {
                            resp.request_focus();
                            self.branch_just_opened = false;
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            create = Some(self.branch_new_name.trim().to_string());
                            open = false;
                        } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            // Go back to list mode.
                            self.branch_creating = false;
                            self.branch_just_opened = true;
                        }
                    } else {
                        // ── Branch list mode ──
                        ui.label(
                            egui::RichText::new("Switch Branch")
                                .size(13.0)
                                .strong()
                                .color(palette.accent),
                        );
                        ui.add_space(4.0);
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut self.branch_filter)
                                .hint_text("Filter branches\u{2026}")
                                .desired_width(300.0),
                        );
                        if self.branch_just_opened {
                            resp.request_focus();
                            self.branch_just_opened = false;
                        }
                        ui.add_space(4.0);

                        let visible = self.visible_branches();
                        // Clamp selection.
                        if self.branch_selected >= visible.len() {
                            self.branch_selected = visible.len().saturating_sub(1);
                        }

                        let max_items = 12;
                        let scroll_offset = self.branch_selected.saturating_sub(max_items - 1);
                        let window =
                            &visible[scroll_offset..(scroll_offset + max_items).min(visible.len())];

                        for (i, name) in window.iter().enumerate() {
                            let abs_idx = scroll_offset + i;
                            let is_sel = abs_idx == self.branch_selected;
                            let text = if is_sel {
                                egui::RichText::new(name.as_str())
                                    .size(12.0)
                                    .strong()
                                    .color(palette.text)
                            } else {
                                egui::RichText::new(name.as_str())
                                    .size(12.0)
                                    .color(palette.text_muted)
                            };
                            let row = ui.add(egui::Label::new(text).selectable(false));
                            if is_sel {
                                let rect = row.rect;
                                ui.painter().rect_filled(
                                    egui::Rect::from_min_size(
                                        egui::pos2(rect.min.x - 4.0, rect.min.y),
                                        egui::vec2(ui.available_width() + 8.0, rect.height()),
                                    ),
                                    3.0,
                                    palette.accent.gamma_multiply(0.15),
                                );
                            }
                            if row.clicked() {
                                checkout = Some(name.clone());
                                open = false;
                            }
                        }

                        // Keyboard navigation.
                        if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                            self.branch_selected =
                                (self.branch_selected + 1).min(visible.len().saturating_sub(1));
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                            self.branch_selected = self.branch_selected.saturating_sub(1);
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            if let Some(name) = visible.get(self.branch_selected) {
                                checkout = Some(name.clone());
                                open = false;
                            }
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            open = false;
                        }
                        // 'n' opens create-new mode (only when filter is empty so
                        // typing 'n' in the filter still works).
                        if self.branch_filter.is_empty()
                            && ui.input(|i| i.key_pressed(egui::Key::N))
                        {
                            self.branch_creating = true;
                            self.branch_just_opened = true;
                            self.branch_new_name.clear();
                        }
                    }

                    // Hint line.
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("↑↓ navigate · Enter checkout · n new · Esc close")
                            .size(10.0)
                            .color(palette.text_muted),
                    );
                });
        });

        if let Some(name) = checkout {
            self.branch_selected = self
                .branch_list
                .iter()
                .position(|b| b == &name)
                .unwrap_or(0);
            self.do_checkout_branch();
        }
        if let Some(name) = create {
            self.branch_new_name = name;
            self.do_create_branch();
        }
        self.branch_switcher_open = open;
    }
}
