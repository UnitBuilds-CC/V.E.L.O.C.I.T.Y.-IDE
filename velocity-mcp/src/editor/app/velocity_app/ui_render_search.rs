//! Search panel rendering for `VelocityApp`.
//!
//! Extracted verbatim from `ui_render.rs` (no logic changes).
use super::struct_def::VelocityApp;
use eframe::egui;

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
                    // Title + toggle on one wrapping row. A full-size `heading` beside a
                    // right-to-left button in a non-wrapping `horizontal` let the wide
                    // heading eat the row so the toggle overflowed the sidebar edge; a
                    // smaller strong label in a `horizontal_wrapped` row keeps both on one
                    // line when there's room and drops the toggle below when it's narrow.
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            egui::RichText::new("Search & Replace")
                                .strong()
                                .size(12.0)
                                .color(palette.text),
                        );
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
                                self.update_search_hits(crate::editor::search::project_search(
                                    &self.workspace_root,
                                    &self.search_query,
                                    100,
                                ));
                            }
                        }
                    });
                    // Wrapping row with a reserve sized for the real button width: a
                    // too-small fixed reserve let "Replace All" run past the sidebar
                    // edge. When the sidebar is too narrow for field+button, the
                    // button wraps to the next line instead of clipping.
                    ui.horizontal_wrapped(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.replace_query)
                                .hint_text("Replace with\u{2026}")
                                .desired_width((ui.available_width() - 120.0).max(60.0)),
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
                            self.update_search_hits(crate::editor::search::project_search(
                                &self.workspace_root,
                                &self.search_query,
                                100,
                            ));
                        }
                    });
                    // Run the debounced search once typing has settled (~250ms).
                    if let Some(since) = self.search_pending_since {
                        if since.elapsed() >= std::time::Duration::from_millis(250) {
                            self.search_pending_since = None;
                            if self.semantic_search_active {
                                self.run_semantic_search();
                            } else {
                                self.update_search_hits(crate::editor::search::project_search(
                                    &self.workspace_root,
                                    &self.search_query,
                                    100,
                                ));
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
                                                self.update_search_hits(
                                                    crate::editor::search::project_search(
                                                        &self.workspace_root,
                                                        &self.search_query,
                                                        100,
                                                    ),
                                                );
                                            }
                                        }
                                    });
                                } else {
                                    ui.vertical_centered(|ui| {
                                        ui.add_space(20.0);
                                        ui.label(
                                            egui::RichText::new(format!(
                                                // Show the searched scope so a stale
                                                // workspace root is immediately visible.
                                                "No results for \"{}\" in {}",
                                                self.search_query,
                                                self.workspace_root.display()
                                            ))
                                            .color(palette.text_muted),
                                        );
                                    });
                                }
                            } else {
                                ui.label(
                                    egui::RichText::new(&self.search_count_label)
                                        .small()
                                        .color(palette.text_muted),
                                );
                                // Pre-extract display strings from the cache into a
                                // local Vec so the closure below doesn't borrow self
                                // immutably (which would conflict with the mutable
                                // self access needed for click handling). The clones
                                // are cheap compared to the format!() calls they
                                // replace — and only happen when the search panel is
                                // visible.
                                let display_data: Vec<_> = self
                                    .search_hit_cache
                                    .iter()
                                    .map(|d| {
                                        (
                                            d.link_label.clone(),
                                            d.path_display.clone(),
                                            d.path_line.clone(),
                                            d.text_preview.clone(),
                                        )
                                    })
                                    .collect();
                                for (hit_idx, hit) in hits.iter().enumerate() {
                                    let (link_label, path_display, path_line, text_preview) =
                                        &display_data[hit_idx];
                                    ui.group(|ui| {
                                        ui.set_max_width(ui.available_width());
                                        if ui.link(link_label).on_hover_text(path_display).clicked()
                                        {
                                            let abs_path = self.workspace_root.join(&hit.path);
                                            self.push_nav_location();
                                            self.open_editor(Some(abs_path));
                                            self.pending_cursor_line = Some(hit.line);
                                        }
                                        ui.label(
                                            egui::RichText::new(path_line)
                                                .size(9.0)
                                                .color(palette.text_muted),
                                        );
                                        ui.label(
                                            egui::RichText::new(text_preview)
                                                .monospace()
                                                .size(11.0),
                                        );
                                    });
                                }
                            }
                        });
                });
            });
    }
}
