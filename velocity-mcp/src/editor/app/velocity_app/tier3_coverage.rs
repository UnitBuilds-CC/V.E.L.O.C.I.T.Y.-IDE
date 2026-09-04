//! Tier-3 panel: auto test-coverage analyzer.
//!
//! Extracted verbatim from `tier3_panels.rs` (no logic changes). Renders the
//! coverage panel and runs workspace/LSP coverage analysis, surfacing results
//! through the test-generator state on `VelocityApp`.

use eframe::egui;
use egui::RichText;

use super::struct_def::VelocityApp;
use crate::editor::theme::{
    CARD_INNER_MARGIN, CARD_RADIUS, FONT_CAPTION, FONT_SMALL, ITEM_SPACING,
};

impl VelocityApp {
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Coverage -- auto test-coverage analyzer
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    pub fn render_coverage_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        Self::tier3_header(
            ui,
            "Test Coverage",
            &self.test_generator.coverage_summary(),
            palette.accent,
            palette.text_muted,
        );

        let mut analyze = false;
        let mut analyze_lsp = false;
        let mut generate = false;
        ui.horizontal(|ui| {
            if ui.button(RichText::new("Analyze workspace").size(FONT_SMALL)).clicked() {
                analyze = true;
            }
            if ui
                .button(RichText::new("Analyze file (LSP)").size(FONT_SMALL))
                .on_hover_text("Discover testable functions in the active file via the language server's documentSymbol outline")
                .clicked()
            {
                analyze_lsp = true;
            }
            let has_gaps = !self.test_generator.analysis.untested_functions.is_empty();
            if ui
                .add_enabled(has_gaps, egui::Button::new(RichText::new("Generate skeletons").size(FONT_SMALL)))
                .clicked()
            {
                generate = true;
            }
            ui.checkbox(&mut self.test_generator.config.public_only, "Public only");
        });
        ui.add_space(ITEM_SPACING);
        
        let analysis = &self.test_generator.analysis;
        ui.add(
            egui::ProgressBar::new(analysis.coverage_percent / 100.0)
                .desired_height(8.0)
                .fill(palette.success)
                .text(format!("{:.1}% covered", analysis.coverage_percent)),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("coverage_scroll")
            .show(ui, |ui| {
                if !analysis.untested_functions.is_empty() {
                    ui.label(
                        RichText::new("UNTESTED FUNCTIONS")
                            .small()
                            .strong()
                            .color(palette.accent),
                    );
                    for func in analysis.untested_functions.iter().take(200) {
                        let file = func
                            .file
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(&func.name)
                                    .monospace()
                                    .size(FONT_CAPTION)
                                    .color(palette.text),
                            );
                            ui.label(
                                RichText::new(format!("{file}:{}", func.line))
                                    .size(FONT_CAPTION)
                                    .color(palette.text_muted),
                            );
                        });
                    }
                    ui.add_space(6.0);
                }

                if !self.test_generator.generated_tests.is_empty() {
                    ui.label(
                        RichText::new("GENERATED SKELETONS")
                            .small()
                            .strong()
                            .color(palette.accent),
                    );
                    for gen in &self.test_generator.generated_tests {
                        egui::Frame::new()
                            .fill(palette.bg_secondary)
                            .corner_radius(CARD_RADIUS)
                            .inner_margin(CARD_INNER_MARGIN)
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(&gen.test_name)
                                        .monospace()
                                        .size(FONT_CAPTION)
                                        .strong()
                                        .color(palette.accent),
                                );
                                ui.label(
                                    RichText::new(&gen.test_body)
                                        .monospace()
                                        .size(FONT_CAPTION)
                                        .color(palette.text_muted),
                                );
                            });
                        ui.add_space(ITEM_SPACING);
                    }
                }
            });

        if analyze {
            self.run_coverage_analysis();
        }
        if analyze_lsp {
            self.run_lsp_coverage_analysis();
        }
        if generate {
            let n = self.test_generator.generate_tests().len();
            self.toasts
                .push(crate::editor::toast::Toast::success(format!(
                    "Generated {n} test skeleton(s)"
                )));
        }
    }

    /// Analyze the workspace for test-coverage gaps.
    pub fn run_coverage_analysis(&mut self) {
        let ws = self.workspace_root.clone();
        self.test_generator.analyze_coverage(&ws);
        let summary = self.test_generator.coverage_summary();
        self.toasts.push(crate::editor::toast::Toast::info(summary));
    }

    /// Analyze the active file for test-coverage gaps using the language
    /// server's `documentSymbol` outline (T3c). Degrades gracefully when no
    /// file is open or no language server is available.
    pub fn run_lsp_coverage_analysis(&mut self) {
        let Some((path, ext, content)) = self.active_lsp_target() else {
            self.toasts.push(crate::editor::toast::Toast::info(
                "No file open for LSP coverage analysis",
            ));
            return;
        };
        let symbols = match self.lsp_manager.as_mut() {
            Some(lsp) => lsp.document_symbols(&ext, &path, &content),
            None => Vec::new(),
        };
        if symbols.is_empty() {
            self.toasts.push(crate::editor::toast::Toast::info(
                "No symbols from language server (server absent or timed out)",
            ));
            return;
        }
        self.test_generator.ingest_lsp_symbol_list(&path, &symbols);
        let summary = self.test_generator.coverage_summary();
        self.toasts
            .push(crate::editor::toast::Toast::success(format!(
                "LSP coverage: {summary}"
            )));
    }
}
