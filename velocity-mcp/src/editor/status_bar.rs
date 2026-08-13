use crate::editor::theme::IdePalette;
use eframe::egui::{self, Panel, Ui};

/// Actions triggered by clicking status bar elements.
#[derive(Default)]
pub struct StatusBarActions {
    pub clicked_mode: bool,
    pub clicked_build: bool,
    pub clicked_position: bool,
    pub clicked_provider: bool,
}

pub struct StatusBar;

impl StatusBar {
    pub fn show(
        ui: &mut Ui,
        palette: IdePalette,
        branch: Option<&str>,
        position: Option<(usize, usize)>,
        build_ok: bool,
        status: &str,
        mode: &str,
        provider_label: &str,
        model_label: &str,
    ) -> StatusBarActions {
        let mut actions = StatusBarActions::default();

        Panel::bottom("status_bar").show(ui, |ui: &mut egui::Ui| {
            // Subtle dot separator instead of hard vertical rules — less clutter.
            let dot = |ui: &mut egui::Ui| {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("\u{00b7}")
                        .size(12.0)
                        .color(palette.text_muted.gamma_multiply(0.6)),
                );
                ui.add_space(8.0);
            };

            ui.add_space(2.0);
            ui.horizontal(|ui: &mut egui::Ui| {
                // Mode badge (glyph + short name) — clickable to cycle modes.
                let mode_response = ui.label(
                    egui::RichText::new(mode)
                        .size(12.0)
                        .strong()
                        .color(palette.accent),
                );
                if mode_response.clicked() {
                    actions.clicked_mode = true;
                }
                if mode_response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                dot(ui);

                let (icon, color) = if build_ok {
                    ("\u{2714}", palette.success)
                } else {
                    ("\u{2716}", palette.error)
                };

                let build_response = ui.label(
                    egui::RichText::new(format!("{} build", icon))
                        .color(color)
                        .size(12.0),
                );
                if build_response.clicked() {
                    actions.clicked_build = true;
                }
                if build_response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }

                if let Some(b) = branch {
                    dot(ui);
                    ui.label(
                        egui::RichText::new(format!("\u{2387} {}", b))
                            .size(12.0)
                            .color(palette.text_muted),
                    );
                }

                if let Some((line, col)) = position {
                    dot(ui);
                    let pos_response = ui.label(
                        egui::RichText::new(format!("Ln {}, Col {}", line + 1, col + 1))
                            .size(12.0)
                            .color(palette.text_muted),
                    );
                    if pos_response.clicked() {
                        actions.clicked_position = true;
                    }
                    if pos_response.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                }

                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui: &mut egui::Ui| {
                        // Provider / model — moved here from the toolbar so
                        // the toolbar stays icon-clean.
                        let model_short = if model_label.len() > 24 {
                            format!(
                                "\u{2026}{}",
                                &model_label[model_label.len().saturating_sub(23)..]
                            )
                        } else {
                            model_label.to_string()
                        };
                        let provider_response = ui.label(
                            egui::RichText::new(format!("{} / {}", provider_label, model_short))
                                .monospace()
                                .size(11.0)
                                .color(palette.text_muted.gamma_multiply(0.8)),
                        );
                        if provider_response.clicked() {
                            actions.clicked_provider = true;
                        }
                        if provider_response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new("Ctrl+Shift+P")
                                .monospace()
                                .size(11.0)
                                .color(palette.text_muted.gamma_multiply(0.7)),
                        );
                        ui.add_space(8.0);
                        // Status message (right-aligned, before provider)
                        if !status.is_empty() {
                            ui.label(
                                egui::RichText::new(status)
                                    .size(12.0)
                                    .color(palette.text_muted),
                            );
                            ui.add_space(8.0);
                        }
                    },
                );
            });
            ui.add_space(2.0);
        });

        actions
    }
}
