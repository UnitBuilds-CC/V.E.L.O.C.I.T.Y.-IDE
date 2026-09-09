//! Tier-3 panels: knowledge base, snippets, and semantic search.
//!
//! Extracted verbatim from `tier3_panels.rs` (no logic changes).

use super::struct_def::VelocityApp;
use super::tier3_common::{primary_button, secondary_button};
use crate::editor::theme::{
    CARD_INNER_MARGIN, CARD_RADIUS, FONT_BODY, FONT_CAPTION, FONT_SMALL, ITEM_SPACING,
    SECTION_SPACING,
};
use eframe::egui;
use egui::RichText;

impl VelocityApp {
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Knowledge -- unified RAG store (ingest + search)
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    pub fn render_knowledge_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let sources = self.knowledge_base.sources();
        Self::tier3_header(
            ui,
            "Knowledge",
            &format!(
                "{} source(s) \u{00b7} {} chunk(s)",
                sources.len(),
                self.knowledge_base.chunk_count()
            ),
            palette.accent,
            palette.text_muted,
        );

        // Ingestion: a path field (file or folder) plus whole-workspace index.
        let mut ingest_path = false;
        let mut ingest_workspace = false;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.knowledge_ingest_input)
                    .hint_text("path to a file or folder\u{2026}")
                    .desired_width(ui.available_width() - 190.0),
            );
            if secondary_button(ui, palette, "Ingest").clicked() {
                ingest_path = true;
            }
            if primary_button(ui, palette, "Index workspace").clicked() {
                ingest_workspace = true;
            }
        });
        ui.add_space(6.0);

        // Search box.
        let mut do_search = false;
        ui.horizontal(|ui| {
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.knowledge_query)
                    .hint_text("search knowledge\u{2026}")
                    .desired_width(ui.available_width() - 70.0),
            );
            if ui
                .button(RichText::new("Search").size(FONT_SMALL))
                .clicked()
                || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
            {
                do_search = true;
            }
        });
        ui.add_space(6.0);

        // Ranked results.
        egui::ScrollArea::vertical()
            .id_salt("knowledge_results_scroll")
            .max_height(220.0)
            .show(ui, |ui| {
                if self.knowledge_results.is_empty() {
                    ui.add_space(ITEM_SPACING);
                    ui.label(
                        RichText::new(
                            "Search your knowledge base above, or ingest content to get started.",
                        )
                        .size(FONT_CAPTION)
                        .color(palette.text_muted),
                    );
                }
                for hit in &self.knowledge_results {
                    egui::Frame::new()
                        .fill(palette.bg_secondary)
                        .corner_radius(CARD_RADIUS)
                        .inner_margin(CARD_INNER_MARGIN)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("{}#{}", hit.source, hit.ordinal))
                                        .size(FONT_CAPTION)
                                        .strong()
                                        .color(palette.accent),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            RichText::new(format!("{:.3}", hit.score))
                                                .size(FONT_CAPTION)
                                                .color(palette.text_muted),
                                        );
                                    },
                                );
                            });
                            ui.label(RichText::new(&hit.snippet).size(9.0).color(palette.text));
                        });
                    ui.add_space(ITEM_SPACING);
                }
            });

        ui.add_space(SECTION_SPACING);
        let mut clear_all = false;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("SOURCES")
                    .small()
                    .strong()
                    .color(palette.accent),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !self.knowledge_base.is_empty()
                    && ui
                        .small_button(RichText::new("Clear all").size(8.0))
                        .clicked()
                {
                    clear_all = true;
                }
            });
        });
        let mut remove: Option<String> = None;
        egui::ScrollArea::vertical()
            .id_salt("knowledge_sources_scroll")
            .max_height(160.0)
            .show(ui, |ui| {
                if sources.is_empty() {
                    ui.label(
                        RichText::new("No sources ingested yet.")
                            .size(FONT_CAPTION)
                            .color(palette.text_muted),
                    );
                }
                for (source, count) in &sources {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(source).size(9.0).color(palette.text));
                        ui.label(
                            RichText::new(format!("({count})"))
                                .size(FONT_CAPTION)
                                .color(palette.text_muted),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .small_button(RichText::new("\u{2716}").size(8.0))
                                .clicked()
                            {
                                remove = Some(source.clone());
                            }
                        });
                    });
                }
            });

        // Deferred mutations (avoid borrowing self during rendering).
        if ingest_path {
            let raw = self.knowledge_ingest_input.trim().to_string();
            if !raw.is_empty() {
                let ws = self.workspace_root.clone();
                let candidate = std::path::PathBuf::from(&raw);
                let path = if candidate.is_absolute() {
                    candidate
                } else {
                    ws.join(&candidate)
                };
                if path.is_dir() {
                    let (files, chunks) = self.knowledge_base.ingest_dir(&ws, &path);
                    if let Err(e) = self.knowledge_base.save(&ws) {
                        Self::persist_err(&mut self.toasts, "knowledge_base", &e);
                    }
                    self.toasts.push(crate::editor::toast::Toast::info(format!(
                        "Ingested {files} file(s), {chunks} chunk(s)"
                    )));
                } else {
                    match self.knowledge_base.ingest_path(&ws, &path) {
                        Ok(added) => {
                            if let Err(e) = self.knowledge_base.save(&ws) {
                                Self::persist_err(&mut self.toasts, "knowledge_base", &e);
                            }
                            self.toasts.push(crate::editor::toast::Toast::info(format!(
                                "Ingested {added} chunk(s)"
                            )));
                        }
                        Err(e) => self.toasts.push(crate::editor::toast::Toast::error(e)),
                    }
                }
            }
        }
        if ingest_workspace {
            let ws = self.workspace_root.clone();
            let (files, chunks) = self.knowledge_base.ingest_dir(&ws, &ws);
            if let Err(e) = self.knowledge_base.save(&ws) {
                Self::persist_err(&mut self.toasts, "knowledge_base", &e);
            }
            self.toasts.push(crate::editor::toast::Toast::info(format!(
                "Indexed workspace: {files} file(s), {chunks} chunk(s)"
            )));
        }
        if do_search {
            let q = self.knowledge_query.clone();
            self.knowledge_results = self.knowledge_base.search(&q, 20);
        }
        if clear_all {
            self.knowledge_base.clear();
            self.knowledge_results.clear();
            let ws = self.workspace_root.clone();
            if let Err(e) = self.knowledge_base.save(&ws) {
                Self::persist_err(&mut self.toasts, "knowledge_base", &e);
            }
            self.toasts
                .push(crate::editor::toast::Toast::info("Knowledge base cleared"));
        }
        if let Some(src) = remove {
            if self.knowledge_base.remove_source(&src) {
                let ws = self.workspace_root.clone();
                if let Err(e) = self.knowledge_base.save(&ws) {
                    Self::persist_err(&mut self.toasts, "knowledge_base", &e);
                }
                self.toasts
                    .push(crate::editor::toast::Toast::info(format!("Removed {src}")));
            }
        }
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Semantic Search -- TF-IDF based code search
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    pub fn render_semantic_search_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();

        Self::tier3_header(
            ui,
            "Semantic Search",
            if self.semantic_index.is_some() {
                "Index built"
            } else {
                "Not indexed"
            },
            palette.accent,
            palette.text_muted,
        );

        // Controls
        let mut build_index = false;
        ui.horizontal(|ui| {
            if self.semantic_index.is_none() {
                if primary_button(
                    ui,
                    palette,
                    format!("{} Build Index", egui_phosphor::regular::HAMMER),
                )
                .clicked()
                {
                    build_index = true;
                }
            } else if secondary_button(
                ui,
                palette,
                format!("{} Rebuild Index", egui_phosphor::regular::ARROWS_CLOCKWISE),
            )
            .clicked()
            {
                build_index = true;
            }
            ui.label(
                RichText::new("TF-IDF semantic search")
                    .size(FONT_CAPTION)
                    .color(palette.text_muted),
            );
        });
        ui.add_space(6.0);

        if self.semantic_index.is_none() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(egui_phosphor::regular::MAGNIFYING_GLASS)
                        .size(26.0)
                        .color(palette.text_muted),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Semantic index not built")
                        .size(FONT_BODY)
                        .strong()
                        .color(palette.text),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new(
                        "Build the TF-IDF index to enable semantic search across the workspace.",
                    )
                    .size(FONT_CAPTION)
                    .color(palette.text_muted),
                );
                ui.add_space(ITEM_SPACING);
                if primary_button(
                    ui,
                    palette,
                    format!("{} Build Index", egui_phosphor::regular::HAMMER),
                )
                .clicked()
                {
                    build_index = true;
                }
            });
        } else {
            ui.label(
                RichText::new(
                    "Semantic search is active. Use the Search panel with semantic mode enabled.",
                )
                .size(FONT_CAPTION)
                .color(palette.text_muted),
            );
        }

        if build_index {
            let ws = self.workspace_root.clone();
            self.semantic_index = Some(crate::editor::semantic_search::SemanticIndex::build(&ws));
            self.toasts
                .push(crate::editor::toast::Toast::success("Semantic index built"));
        }
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Snippets -- code snippet library browser
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    pub fn render_snippets_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let count = self.snippet_collection.snippets.len();

        Self::tier3_header(
            ui,
            "Snippets",
            &format!("{} snippet(s) loaded", count),
            palette.accent,
            palette.text_muted,
        );

        // Search box
        ui.horizontal(|ui| {
            ui.label(RichText::new("Search:").size(9.0).color(palette.text_muted));
            ui.add(
                egui::TextEdit::singleline(&mut self.snippet_search_query)
                    .hint_text("filter snippets...")
                    .desired_width(ui.available_width() - 60.0),
            );
        });
        ui.add_space(6.0);

        if self.snippet_collection.snippets.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(egui_phosphor::regular::CODE)
                        .size(26.0)
                        .color(palette.text_muted),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("No snippets loaded")
                        .size(FONT_BODY)
                        .strong()
                        .color(palette.text),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new("Snippets load from .velocity/snippets.json in your workspace.")
                        .size(FONT_CAPTION)
                        .color(palette.text_muted),
                );
                ui.add_space(ITEM_SPACING);
                if primary_button(
                    ui,
                    palette,
                    format!(
                        "{} Reload snippets",
                        egui_phosphor::regular::ARROWS_CLOCKWISE
                    ),
                )
                .clicked()
                {
                    let path = self.workspace_root.join(".velocity").join("snippets.json");
                    self.snippet_collection =
                        crate::editor::snippets::SnippetCollection::load_from_file(&path);
                }
            });
        } else {
            egui::ScrollArea::vertical()
                .id_salt("snippets_scroll")
                .show(ui, |ui| {
                    let snippets_to_show: Vec<_> = if self.snippet_search_query.is_empty() {
                        self.snippet_collection.snippets.iter().collect()
                    } else {
                        self.snippet_collection
                            .matching(&self.snippet_search_query)
                            .into_iter()
                            .collect()
                    };

                    for snippet in snippets_to_show {
                        egui::Frame::new()
                            .fill(palette.bg_secondary)
                            .corner_radius(CARD_RADIUS)
                            .inner_margin(CARD_INNER_MARGIN)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&snippet.name)
                                            .size(FONT_SMALL)
                                            .strong()
                                            .color(palette.text),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if let Some(scope) = &snippet.scope {
                                                ui.label(
                                                    RichText::new(scope)
                                                        .size(FONT_CAPTION)
                                                        .monospace()
                                                        .color(palette.accent),
                                                );
                                            }
                                        },
                                    );
                                });
                                ui.label(
                                    RichText::new(format!("Prefix: {}", snippet.prefix))
                                        .size(FONT_CAPTION)
                                        .monospace()
                                        .color(palette.text_muted),
                                );
                                if let Some(desc) = &snippet.description {
                                    ui.label(
                                        RichText::new(desc).size(8.0).color(palette.text_muted),
                                    );
                                }
                            });
                        ui.add_space(ITEM_SPACING);
                    }
                });
        }
    }
}
