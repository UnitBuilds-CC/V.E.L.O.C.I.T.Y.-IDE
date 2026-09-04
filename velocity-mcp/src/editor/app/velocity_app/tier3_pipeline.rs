//! Tier-3 panels: deploy pipeline visualization.
//!
//! Extracted verbatim from `tier3_panels.rs` (no logic changes).

use super::struct_def::VelocityApp;
use crate::editor::deploy_pipeline::{PipelineStage, StageStatus};
use crate::editor::theme::{FONT_CAPTION, FONT_SMALL};
use eframe::egui;
use egui::RichText;

impl VelocityApp {
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Pipeline -- build/test/deploy manager
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    pub fn render_pipeline_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        self.init_deploy_pipeline();

        let (status, target, deployments): (String, String, usize) = self
            .deploy_pipeline
            .as_ref()
            .map(|p| {
                (
                    p.status_label().to_string(),
                    p.config.deploy_target.clone(),
                    p.deployments.len(),
                )
            })
            .unwrap_or_default();

        Self::tier3_header(
            ui,
            "Deploy Pipeline",
            &format!("{status} \u{00b7} target: {target}"),
            palette.accent,
            palette.text_muted,
        );

        let mut run = false;
        let mut deploy = false;
        let mut rollback = false;
        ui.horizontal(|ui| {
            if ui
                .button(RichText::new("\u{25b6} Run build+test").size(FONT_SMALL))
                .clicked()
            {
                run = true;
            }
            if ui
                .button(RichText::new("\u{25b2} Deploy").size(FONT_SMALL))
                .clicked()
            {
                deploy = true;
            }
            if ui
                .add_enabled(
                    deployments >= 2,
                    egui::Button::new(RichText::new("\u{27f2} Rollback").size(FONT_SMALL)),
                )
                .clicked()
            {
                rollback = true;
            }
        });
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("pipeline_scroll")
            .show(ui, |ui| {
                if let Some(pipeline) = &self.deploy_pipeline {
                    for stage in PipelineStage::all() {
                        if let Some(sr) = pipeline.stages.iter().find(|s| s.stage == *stage) {
                            let (icon, color) = match &sr.status {
                                StageStatus::Passed => ("\u{2714}", palette.success),
                                StageStatus::Failed(_) => ("\u{2716}", palette.error),
                                StageStatus::Running => ("\u{22ef}", palette.warning),
                                StageStatus::Skipped => ("\u{21b7}", palette.text_muted),
                                StageStatus::Pending => ("\u{25cb}", palette.text_muted),
                            };
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(icon).size(FONT_SMALL).color(color));
                                ui.label(
                                    RichText::new(stage.label())
                                        .size(FONT_SMALL)
                                        .strong()
                                        .color(palette.text),
                                );
                                if let Some(ms) = sr.duration_ms {
                                    ui.label(
                                        RichText::new(format!("{ms} ms"))
                                            .size(FONT_CAPTION)
                                            .color(palette.text_muted),
                                    );
                                }
                            });
                        }
                    }
                    ui.add_space(6.0);

                    if !pipeline.deployments.is_empty() {
                        ui.label(
                            RichText::new("DEPLOYMENTS")
                                .small()
                                .strong()
                                .color(palette.accent),
                        );
                        ui.add_space(2.0);
                        for dep in pipeline.deployments.iter().rev().take(10) {
                            let color = match dep.status {
                                StageStatus::Passed => palette.success,
                                StageStatus::Failed(_) => palette.error,
                                _ => palette.text_muted,
                            };
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("#{}", dep.id))
                                        .size(FONT_CAPTION)
                                        .color(palette.text_muted),
                                );
                                ui.label(
                                    RichText::new(&dep.version)
                                        .monospace()
                                        .size(FONT_CAPTION)
                                        .color(palette.text),
                                );
                                ui.label(RichText::new(&dep.target).size(8.0).color(color));
                            });
                        }
                    }
                }
            });

        if run {
            self.trigger_deploy();
        }
        if deploy {
            if let Some(pipeline) = &mut self.deploy_pipeline {
                match pipeline.deploy() {
                    Ok(()) => self.toasts.push(crate::editor::toast::Toast::success(
                        "Deploy stage complete",
                    )),
                    Err(e) => self.toasts.push(crate::editor::toast::Toast::error(e)),
                }
            }
        }
        if rollback {
            self.rollback_deploy();
        }
    }
}
