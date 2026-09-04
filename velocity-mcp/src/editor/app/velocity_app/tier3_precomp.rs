//! Tier-3 panels: speculative pre-computation cache (warm + view).
//!
//! Extracted verbatim from `tier3_panels.rs` (no logic changes).

use eframe::egui;
use egui::RichText;
use super::struct_def::VelocityApp;
use crate::editor::theme::{FONT_CAPTION};

impl VelocityApp {
    /// Pre-index the currently open editor files into the speculative cache
    /// under the manual slot (task id 0) and report a summary.
    pub fn warm_precompute_cache(&mut self) {
        let files: Vec<std::path::PathBuf> = self
            .tabs
            .iter()
            .filter_map(|t| t.editor_path().cloned())
            .collect();
        if files.is_empty() {
            self.toasts.push(crate::editor::toast::Toast::info(
                "No open files to pre-index",
            ));
            return;
        }
        let result =
            crate::editor::speculative_precomp::precompute_files(&self.workspace_root, &files);
        let summary = format!(
            "Pre-indexed {} file(s), {} symbols",
            result.files.len(),
            result.total_symbols
        );
        self.precomp_cache.store(0, result);
        self.toasts
            .push(crate::editor::toast::Toast::success(summary));
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Speculative Precomputation -- cache status and contents
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    pub fn render_precomp_cache_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();

        Self::tier3_header(
            ui,
            "Precomputation Cache",
            "Speculative context pre-indexing",
            palette.accent,
            palette.text_muted,
        );

        ui.label(
            RichText::new(
                "Pre-indexes scoped files before agent workers spawn, providing warm context caches that accelerate agent execution.",
            )
            .size(FONT_CAPTION)
            .color(palette.text_muted),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("precomp_cache_scroll")
            .show(ui, |ui| {
                ui.label(
                    RichText::new("CACHE STATUS")
                        .small()
                        .strong()
                        .color(palette.accent),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new("\u{2022} Automatic: runs before each agent task")
                        .size(FONT_CAPTION)
                        .color(palette.text),
                );
                ui.label(
                    RichText::new("\u{2022} Background: does not block UI")
                        .size(FONT_CAPTION)
                        .color(palette.text),
                );
                ui.label(
                    RichText::new("\u{2022} Per-task: keyed by task ID")
                        .size(FONT_CAPTION)
                        .color(palette.text),
                );
                ui.add_space(6.0);

                ui.label(
                    RichText::new("Each cached entry contains:")
                        .size(FONT_CAPTION)
                        .strong()
                        .color(palette.text_muted),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new("\u{2022} File paths and line counts")
                        .size(FONT_CAPTION)
                        .color(palette.text),
                );
                ui.label(
                    RichText::new("\u{2022} Symbol outlines")
                        .size(FONT_CAPTION)
                        .color(palette.text),
                );
                ui.label(
                    RichText::new("\u{2022} Import lists")
                        .size(FONT_CAPTION)
                        .color(palette.text),
                );
                ui.label(
                    RichText::new("\u{2022} Top-level summaries")
                        .size(FONT_CAPTION)
                        .color(palette.text),
                );
            });
    }
}
