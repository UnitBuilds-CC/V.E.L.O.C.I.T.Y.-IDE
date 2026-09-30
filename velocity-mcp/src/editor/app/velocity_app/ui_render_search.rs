//! Search panel rendering for `VelocityApp`.
//!
//! Extracted verbatim from `ui_render.rs` (no logic changes).
use super::struct_def::VelocityApp;
use eframe::egui;

impl VelocityApp {
    /// Run the workspace search with the current query and matching toggles,
    /// refreshing the hit list. Shared by the Enter handler, the debounce tick,
    /// the toggle buttons, and the suggested-query chips so every path honors
    /// `search_opts` identically.
    pub fn run_literal_search(&mut self) {
        let hits = crate::editor::search::project_search(
            &self.workspace_root,
            &self.search_query,
            100,
            self.search_opts,
            &self.search_include,
        );
        self.update_search_hits(hits);
    }

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
                                self.run_literal_search();
                            }
                        }
                    });
                    // Matching-mode toggles: same Aa / whole-word / regex semantics
                    // as the in-file find bar. Clicking one re-runs the current query
                    // immediately so results always reflect the visible mode.
                    ui.horizontal(|ui| {
                        let toggles: [(&str, bool, &str); 3] = [
                            ("Aa", self.search_opts.case_sensitive, "Match case"),
                            ("ab|", self.search_opts.whole_word, "Whole word"),
                            (".*", self.search_opts.use_regex, "Regular expression"),
                        ];
                        for (idx, (label, active, hover)) in toggles.iter().enumerate() {
                            let color = if *active { palette.accent } else { palette.text_muted };
                            if ui
                                .small_button(
                                    egui::RichText::new(*label)
                                        .monospace()
                                        .size(10.0)
                                        .color(color),
                                )
                                .on_hover_text(*hover)
                                .clicked()
                            {
                                match idx {
                                    0 => self.search_opts.case_sensitive = !self.search_opts.case_sensitive,
                                    1 => self.search_opts.whole_word = !self.search_opts.whole_word,
                                    _ => self.search_opts.use_regex = !self.search_opts.use_regex,
                                }
                                if !self.search_query.is_empty() && !self.semantic_search_active {
                                    self.search_pending_since = None;
                                    self.run_literal_search();
                                }
                            }
                        }
                    });
                    // "Files to include" glob list (comma-separated): scopes both
                    // the search walk and Replace All, re-running through the same
                    // debounce path as the query field.
                    ui.horizontal(|ui| {
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut self.search_include)
                                .hint_text("files to include\u{2026} e.g. src/**, *.rs")
                                .desired_width(ui.available_width() - 10.0),
                        );
                        if resp.changed()
                            && !self.search_query.is_empty()
                            && !self.semantic_search_active
                        {
                            self.search_pending_since = Some(std::time::Instant::now());
                        }
                    });
                    // Wrapping row with a reserve sized for the real button width: a
                    // too-small fixed reserve let "Replace All" run past the sidebar
                    // edge. When the sidebar is too narrow for field+button, the
                    // button wraps to the next line instead of clipping.
                    ui.horizontal_wrapped(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.replace_query)
                                .hint_text(if self.search_opts.use_regex {
                                    "Replace with\u{2026} ($1 groups)"
                                } else {
                                    "Replace with\u{2026}"
                                })
                                .desired_width((ui.available_width() - 120.0).max(60.0)),
                        );
                        let can_replace = !self.search_query.is_empty();
                        if ui
                            .add_enabled(can_replace, egui::Button::new("Replace All"))
                            .on_hover_text(
                                "Replace every match across the workspace, honoring the Aa / ab| / .* toggles above",
                            )
                            .clicked()
                        {
                            let summary = crate::editor::search::project_replace(
                                &self.workspace_root,
                                &self.search_query,
                                &self.replace_query,
                                self.search_opts,
                                &self.search_include,
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
                            self.run_literal_search();
                        }
                    });
                    // Run the debounced search once typing has settled (~250ms).
                    if let Some(since) = self.search_pending_since {
                        if since.elapsed() >= std::time::Duration::from_millis(250) {
                            self.search_pending_since = None;
                            if self.semantic_search_active {
                                self.run_semantic_search();
                            } else {
                                self.run_literal_search();
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
                                                self.run_literal_search();
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
                                let mut prev_file: Option<std::path::PathBuf> = None;
                                for (hit_idx, hit) in hits.iter().enumerate() {
                                    let (link_label, path_display, path_line, text_preview) =
                                        &display_data[hit_idx];
                                    // Group by file: a header row whenever the path
                                    // changes (the walk emits hits per file
                                    // consecutively), carrying a per-file replace
                                    // action so small scopes don't need "Replace All".
                                    if prev_file.as_ref() != Some(&hit.path) {
                                        prev_file = Some(hit.path.clone());
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                egui::RichText::new(hit
                                                    .path
                                                    .to_string_lossy()
                                                    .replace('\\', "/"))
                                                .size(10.0)
                                                .strong()
                                                .color(palette.accent),
                                            );
                                            // Same enable rule as "Replace All": the
                                            // button is visible but inert until both
                                            // fields have text, so the action stays
                                            // discoverable while invalid.
                                            let can_replace_file = !self.search_query.is_empty()
                                                && !self.replace_query.is_empty();
                                            if ui
                                                .add_enabled(
                                                    can_replace_file,
                                                    egui::Button::new("Replace in file"),
                                                )
                                                .on_hover_text(
                                                    "Replace every match in this file only",
                                                )
                                                .clicked()
                                            {
                                                let n = crate::editor::search::replace_in_file(
                                                    &self.workspace_root,
                                                    &hit.path,
                                                    &self.search_query,
                                                    &self.replace_query,
                                                    self.search_opts,
                                                );
                                                if n > 0 {
                                                    self.toasts.push(
                                                        crate::editor::toast::Toast::success(
                                                            format!(
                                                                "Replaced {n} occurrence(s) in {}",
                                                                hit.path.display()
                                                            ),
                                                        ),
                                                    );
                                                    self.run_literal_search();
                                                } else {
                                                    self.toasts.push(
                                                        crate::editor::toast::Toast::info(
                                                            "No matching occurrences in this file",
                                                        ),
                                                    );
                                                }
                                            }
                                        });
                                    }
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
