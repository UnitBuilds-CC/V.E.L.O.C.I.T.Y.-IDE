//! Tier-3 panels: live orchestration activity feed.
//!
//! Extracted verbatim from `tier3_panels.rs` (no logic changes).

use super::struct_def::VelocityApp;
use super::tier3_common::primary_button;
use crate::editor::theme::{
    CARD_INNER_MARGIN, CARD_RADIUS, FONT_BODY, FONT_CAPTION, FONT_SMALL, ITEM_SPACING,
};
use eframe::egui;
use egui::RichText;

impl VelocityApp {
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Activity -- live orchestration feed + pre-computation cache
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    pub fn render_activity_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let lo = &self.live_orchestration;
        Self::tier3_header(
            ui,
            "Live Activity",
            &format!(
                "up {} \u{00b7} {:.1} tasks/min",
                lo.session_uptime(),
                lo.throughput()
            ),
            palette.accent,
            palette.text_muted,
        );

        // Session stat strip.
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("\u{2714} {}", lo.total_tasks_completed))
                    .size(FONT_SMALL)
                    .color(palette.success),
            );
            ui.label(
                RichText::new(format!("\u{2716} {}", lo.total_tasks_failed))
                    .size(FONT_SMALL)
                    .color(palette.error),
            );
            ui.label(
                RichText::new(format!("\u{22ef} {} active", lo.worker_progress.len()))
                    .size(FONT_SMALL)
                    .color(palette.warning),
            );
        });
        ui.add_space(ITEM_SPACING);

        // Active worker progress bars.
        if !lo.worker_progress.is_empty() {
            ui.label(
                RichText::new("WORKERS")
                    .small()
                    .strong()
                    .color(palette.accent),
            );
            for wp in &lo.worker_progress {
                egui::Frame::new()
                    .fill(palette.bg_secondary)
                    .corner_radius(CARD_RADIUS)
                    .inner_margin(CARD_INNER_MARGIN)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("#{}", wp.task_id))
                                    .size(FONT_CAPTION)
                                    .color(palette.text_muted),
                            );
                            ui.label(
                                RichText::new(&wp.title)
                                    .size(FONT_SMALL)
                                    .strong()
                                    .color(palette.text),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new(wp.elapsed_label())
                                            .size(FONT_CAPTION)
                                            .color(palette.text_muted),
                                    );
                                },
                            );
                        });
                        ui.add(
                            egui::ProgressBar::new(wp.progress_fraction())
                                .desired_height(6.0)
                                .fill(palette.accent),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{} \u{00b7} {} file(s) changed",
                                wp.status_text, wp.files_changed
                            ))
                            .size(FONT_CAPTION)
                            .color(palette.text_muted),
                        );
                    });
                ui.add_space(ITEM_SPACING);
            }
            ui.add_space(ITEM_SPACING);
        }

        // Pre-computation cache (id 0 = manual workspace warm).
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("CONTEXT CACHE")
                    .small()
                    .strong()
                    .color(palette.accent),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(RichText::new("Warm from open files").size(9.0))
                    .clicked()
                {
                    self.warm_precompute_cache();
                }
            });
        });
        if let Some(result) = self.precomp_cache.peek(0) {
            ui.label(
                RichText::new(format!(
                    "{} file(s) \u{00b7} {} symbols \u{00b7} {} lines",
                    result.files.len(),
                    result.total_symbols,
                    result.total_lines
                ))
                .size(FONT_CAPTION)
                .color(palette.text_muted),
            );
        } else {
            ui.label(
                RichText::new("Cache empty \u{2014} warm it to pre-index open files.")
                    .size(FONT_CAPTION)
                    .color(palette.text_muted),
            );
        }
        ui.add_space(ITEM_SPACING);

        // Activity feed.
        let mut go_orchestrator = false;
        ui.label(RichText::new("FEED").small().strong().color(palette.accent));
        egui::ScrollArea::vertical()
            .id_salt("activity_feed_scroll")
            .stick_to_bottom(true)
            .show(ui, |ui| {
                let feed = self.live_orchestration.filtered_feed();
                if feed.is_empty() {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new(egui_phosphor::regular::ROBOT)
                                .size(24.0)
                                .color(palette.text_muted),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("No activity yet")
                                .size(FONT_BODY)
                                .strong()
                                .color(palette.text),
                        );
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new("Events appear here when agents are running tasks.")
                                .size(FONT_CAPTION)
                                .color(palette.text_muted),
                        );
                        ui.add_space(ITEM_SPACING);
                        if primary_button(
                            ui,
                            palette,
                            format!("{} Open Orchestrator", egui_phosphor::regular::ROBOT),
                        )
                        .clicked()
                        {
                            go_orchestrator = true;
                        }
                    });
                }
                for ev in feed {
                    let color = match ev.kind {
                        crate::editor::live_orchestration::ActivityEventKind::WorkerCompleted => palette.success,
                        crate::editor::live_orchestration::ActivityEventKind::WorkerFailed => palette.error,
                        crate::editor::live_orchestration::ActivityEventKind::WorkerBlocked
                        | crate::editor::live_orchestration::ActivityEventKind::InterventionQueued => palette.warning,
                        _ => palette.text_muted,
                    };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(ev.kind.icon()).size(FONT_SMALL).color(color));
                        ui.label(RichText::new(ev.kind.label()).size(8.0).color(color));
                        ui.label(RichText::new(&ev.message).size(9.0).color(palette.text));
                    });
                }
            });
        if go_orchestrator {
            self.focus_orchestrator_tab();
        }
    }
}
