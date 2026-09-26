use eframe::egui;
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
                                let is_current = project_path == self.workspace_root;
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
                                            self.save_workspace_preferences();
                                            self.workspace_root = project_path.clone();
                                            self.restore_workspace_preferences();
                                            self.status_message = format!("Switched to {}", name);
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
                            let is_current =
                                self.projects.get(selected) == Some(&self.workspace_root);
                            if !is_current {
                                if let Some(path) = self.projects.get(selected).cloned() {
                                    self.save_workspace_preferences();
                                    self.workspace_root = path;
                                    self.restore_workspace_preferences();
                                    self.status_message = "Switched workspace".to_string();
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
                            let msg = if self.goto_symbol_entries.is_empty() {
                                "No symbols indexed yet \u{2014} run the indexer first"
                            } else {
                                "No matching symbols"
                            };
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
                        egui::ScrollArea::vertical()
                            .max_width(220.0)
                            .show(ui, |ui| {
                                Self::render_file_tree_node(
                                    ui,
                                    &tree,
                                    &workspace_root,
                                    &mut path_string,
                                    palette,
                                );
                            });
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
    ) {
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
                egui::CollapsingHeader::new(
                    // No manual "▸": CollapsingHeader already paints its own
                    // disclosure triangle, and the U+25B8 glyph isn't covered by the
                    // bundled fonts, so the hand-added arrow rendered as a tofu box.
                    egui::RichText::new(dir_name).size(10.0).color(palette.text),
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
                        );
                    }
                });
            }
        } else {
            let rel = node
                .path
                .strip_prefix(workspace_root)
                .unwrap_or(&node.path)
                .to_string_lossy()
                .to_string();
            let icon = crate::editor::search::icon_for_path(&node.path);
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(format!("{} {}", icon, rel))
                            .size(9.0)
                            .color(palette.text),
                    )
                    .frame(false),
                )
                .clicked()
            {
                *path_string = rel;
            }
        }
    }

    /// Render a file tree node with a case-insensitive filter. Directories are
    /// expanded only if they (or their descendants) contain a match.
    pub(crate) fn render_file_tree_node_filtered(
        ui: &mut egui::Ui,
        node: &FileNode,
        workspace_root: &Path,
        path_string: &mut String,
        palette: crate::editor::theme::IdePalette,
        filter: &str,
    ) {
        let filter_lower = filter.to_lowercase();
        Self::render_file_tree_node_filtered_inner(
            ui,
            node,
            workspace_root,
            path_string,
            palette,
            &filter_lower,
        );
    }

    fn render_file_tree_node_filtered_inner(
        ui: &mut egui::Ui,
        node: &FileNode,
        workspace_root: &Path,
        path_string: &mut String,
        palette: crate::editor::theme::IdePalette,
        filter_lower: &str,
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
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(format!("{} {}", icon, rel))
                                .size(9.0)
                                .color(palette.text),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    *path_string = rel;
                }
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
}
