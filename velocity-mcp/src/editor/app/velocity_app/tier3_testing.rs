//! Tier-3 panels: AI test generator and improvement engine.
//!
//! Extracted verbatim from `tier3_panels.rs` (no logic changes).

use super::struct_def::VelocityApp;
use crate::editor::theme::{
    CARD_INNER_MARGIN, CARD_RADIUS, FONT_CAPTION, FONT_SMALL, ITEM_SPACING, SECTION_SPACING,
};
use eframe::egui;
use egui::RichText;

impl VelocityApp {
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Test Generator -- coverage analysis and test generation
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    pub fn render_test_generator_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let total = self.test_generator.analysis.total_functions;
        let tested = self.test_generator.analysis.tested_functions;
        let coverage = self.test_generator.analysis.coverage_percent;

        Self::tier3_header(
            ui,
            "Test Generator",
            &format!(
                "{}/{} functions tested \u{00b7} {:.1}% coverage",
                tested, total, coverage
            ),
            palette.accent,
            palette.text_muted,
        );

        // Controls
        let mut analyze = false;
        let mut generate = false;
        ui.horizontal(|ui| {
            if ui
                .button(RichText::new("\u{1f50d} Analyze Coverage").size(FONT_SMALL))
                .clicked()
            {
                analyze = true;
            }
            if ui
                .button(RichText::new("\u{2728} Generate Tests").size(FONT_SMALL))
                .clicked()
            {
                generate = true;
            }
            ui.label(
                RichText::new(format!(
                    "{} test(s) generated",
                    self.test_generator.generated_tests.len()
                ))
                .size(FONT_CAPTION)
                .color(palette.text_muted),
            );
        });
        ui.add_space(6.0);

        // Configuration
        egui::CollapsingHeader::new(RichText::new("Configuration").size(FONT_SMALL).strong())
            .default_open(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Max tests per run:").size(9.0));
                    ui.add(
                        egui::DragValue::new(&mut self.test_generator.config.max_tests_per_run)
                            .range(1..=100)
                            .speed(1),
                    );
                });
                ui.checkbox(
                    &mut self.test_generator.config.public_only,
                    "Public functions only",
                );
                ui.checkbox(
                    &mut self.test_generator.config.include_assertions,
                    "Include assertion placeholders",
                );
            });
        ui.add_space(6.0);

        // Untested functions list
        if !self.test_generator.analysis.untested_functions.is_empty() {
            ui.label(
                RichText::new("UNTESTED FUNCTIONS")
                    .small()
                    .strong()
                    .color(palette.warning),
            );
            egui::ScrollArea::vertical()
                .id_salt("test_gen_untested_scroll")
                .max_height(200.0)
                .show(ui, |ui| {
                    for func in &self.test_generator.analysis.untested_functions {
                        let vis_badge = match func.visibility {
                            crate::editor::test_generator::Visibility::Public => {
                                ("pub", palette.success)
                            }
                            crate::editor::test_generator::Visibility::Private => {
                                ("priv", palette.text_muted)
                            }
                            crate::editor::test_generator::Visibility::CrateLocal => {
                                ("crate", palette.text_muted)
                            }
                        };
                        egui::Frame::new()
                            .fill(palette.bg_secondary)
                            .corner_radius(CARD_RADIUS)
                            .inner_margin(CARD_INNER_MARGIN)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(vis_badge.0)
                                            .size(FONT_CAPTION)
                                            .monospace()
                                            .color(vis_badge.1),
                                    );
                                    ui.label(
                                        RichText::new(&func.name)
                                            .size(FONT_SMALL)
                                            .strong()
                                            .color(palette.text),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(format!(
                                                    "{}:{}",
                                                    func.file.display(),
                                                    func.line
                                                ))
                                                .size(FONT_CAPTION)
                                                .color(palette.text_muted),
                                            );
                                        },
                                    );
                                });
                                ui.label(
                                    RichText::new(&func.signature)
                                        .size(FONT_CAPTION)
                                        .monospace()
                                        .color(palette.text_muted),
                                );
                            });
                        ui.add_space(ITEM_SPACING);
                    }
                });
        }

        // Generated tests preview
        if !self.test_generator.generated_tests.is_empty() {
            ui.add_space(SECTION_SPACING);
            ui.label(
                RichText::new("GENERATED TESTS")
                    .small()
                    .strong()
                    .color(palette.success),
            );
            let mut copy_idx: Option<usize> = None;
            egui::ScrollArea::vertical()
                .id_salt("test_gen_results_scroll")
                .max_height(200.0)
                .show(ui, |ui| {
                    for (idx, test) in self.test_generator.generated_tests.iter().enumerate() {
                        egui::Frame::new()
                            .fill(palette.bg_secondary)
                            .corner_radius(CARD_RADIUS)
                            .inner_margin(CARD_INNER_MARGIN)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&test.test_name)
                                            .size(FONT_SMALL)
                                            .strong()
                                            .color(palette.text),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(format!(
                                                    "{:.0}% confidence",
                                                    test.confidence * 100.0
                                                ))
                                                .size(FONT_CAPTION)
                                                .color(palette.text_muted),
                                            );
                                        },
                                    );
                                });
                                ui.label(
                                    RichText::new(format!("for {}", test.function_name))
                                        .size(FONT_CAPTION)
                                        .color(palette.text_muted),
                                );
                                if ui
                                    .small_button(RichText::new("Copy code").size(8.0))
                                    .clicked()
                                {
                                    copy_idx = Some(idx);
                                }
                            });
                        ui.add_space(ITEM_SPACING);
                    }
                });
            if let Some(idx) = copy_idx {
                if let Some(test) = self.test_generator.generated_tests.get(idx) {
                    ui.ctx().copy_text(test.test_body.clone());
                    self.toasts
                        .push(crate::editor::toast::Toast::success(format!(
                            "Copied {} to clipboard",
                            test.test_name
                        )));
                }
            }
        }

        if analyze {
            let ws = self.workspace_root.clone();
            self.test_generator.analyze_coverage(&ws);
            self.toasts
                .push(crate::editor::toast::Toast::success(format!(
                    "Coverage analysis complete: {:.1}%",
                    self.test_generator.analysis.coverage_percent
                )));
        }
        if generate {
            let tests = self.test_generator.generate_tests();
            self.test_generator.generated_tests = tests;
            self.toasts
                .push(crate::editor::toast::Toast::success(format!(
                    "Generated {} test(s)",
                    self.test_generator.generated_tests.len()
                )));
        }
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Agent Subsystem Panels
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

    pub fn render_improvement_engine_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let failure_count = self.improvement_engine.failure_count();
        let has_data = self.improvement_engine.has_data();
        let directives = self.improvement_engine.analyze();

        Self::tier3_header(
            ui,
            "Self-Improvement Engine",
            &format!("{failure_count} failure(s) recorded"),
            palette.accent,
            palette.text_muted,
        );

        ui.label(
            RichText::new(
                "Tracks failures during agent execution, classifies them into categories, and generates prompt refinements to avoid repeating mistakes.",
            )
            .size(FONT_CAPTION)
            .color(palette.text_muted),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("improvement_engine_scroll")
            .show(ui, |ui| {
                if !has_data {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("\u{2699}")
                                .size(24.0)
                                .color(palette.text_muted.gamma_multiply(0.5)),
                        );
                        ui.add_space(ITEM_SPACING);
                        ui.label(
                            RichText::new(
                                "No failures recorded this session.\nThe engine is idle.",
                            )
                            .size(FONT_SMALL)
                            .color(palette.text_muted),
                        );
                    });
                } else {
                    ui.label(
                        RichText::new(format!("Generated {} directive(s):", directives.len()))
                            .small()
                            .strong()
                            .color(palette.accent),
                    );
                    ui.add_space(ITEM_SPACING);
                    for d in &directives {
                        egui::Frame::new()
                            .fill(palette.bg_tertiary)
                            .corner_radius(CARD_RADIUS)
                            .inner_margin(CARD_INNER_MARGIN)
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(format!("{:?}", d.category))
                                        .small()
                                        .strong()
                                        .color(palette.warning),
                                );
                                ui.label(RichText::new(&d.directive).size(9.0).color(palette.text));
                                ui.label(
                                    RichText::new(format!(
                                        "confidence: {:.0}% \u{00b7} {} occurrence(s)",
                                        d.confidence * 100.0,
                                        d.occurrences
                                    ))
                                    .small()
                                    .color(palette.text_muted),
                                );
                            });
                        ui.add_space(ITEM_SPACING);
                    }
                }
            });
    }
}
