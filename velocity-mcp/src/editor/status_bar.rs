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

        Panel::bottom("status_bar")
            .frame(
                egui::Frame::new()
                    .fill(palette.bg_secondary)
                    .stroke(egui::Stroke::new(0.0, palette.border)),
            )
            .show(ui, |ui: &mut egui::Ui| {
            // Accent top border — 1px line across the full width
            {
                let rect = ui.available_rect_before_wrap();
                let top_line = egui::Rect::from_min_size(
                    egui::pos2(rect.min.x, rect.min.y),
                    egui::vec2(rect.width(), 1.0),
                );
                ui.painter().rect_filled(top_line, 0, palette.accent.gamma_multiply(0.4));
            }

            ui.add_space(3.0);
            ui.horizontal(|ui: &mut egui::Ui| {
                ui.spacing_mut().item_spacing.x = 2.0;

                // Mode badge — accent pill
                {
                    let mode_pill = egui::Frame::new()
                        .fill(palette.accent.gamma_multiply(0.12))
                        .corner_radius(egui::CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(6, 1));
                    let mode_response = mode_pill.show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(mode)
                                .size(11.0)
                                .strong()
                                .color(palette.accent),
                        )
                    }).inner;
                    if mode_response.clicked() {
                        actions.clicked_mode = true;
                    }
                    if mode_response.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    mode_response.on_hover_text("Switch workspace mode");
                }

                ui.add_space(4.0);

                // Build indicator — subtle pill
                {
                    let (icon, color) = if build_ok {
                        ("\u{2714}", palette.success)
                    } else {
                        ("\u{2716}", palette.error)
                    };
                    let build_bg = if build_ok {
                        palette.success.gamma_multiply(0.10)
                    } else {
                        palette.error.gamma_multiply(0.10)
                    };
                    let build_pill = egui::Frame::new()
                        .fill(build_bg)
                        .corner_radius(egui::CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(5, 1));
                    let build_response = build_pill.show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(format!("{} build", icon))
                                .color(color)
                                .size(11.0),
                        )
                    }).inner;
                    if build_response.clicked() {
                        actions.clicked_build = true;
                    }
                    if build_response.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    build_response.on_hover_text("View diagnostics");
                }

                if let Some(b) = branch {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(format!("\u{2387} {}", b))
                            .size(11.0)
                            .color(palette.text_muted),
                    );
                }

                if let Some((line, col)) = position {
                    ui.add_space(4.0);
                    let pos_response = ui.label(
                        egui::RichText::new(format!("Ln {}, Col {}", line + 1, col + 1))
                            .size(11.0)
                            .color(palette.text_muted),
                    );
                    if pos_response.clicked() {
                        actions.clicked_position = true;
                    }
                    if pos_response.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    pos_response.on_hover_text("Go to line");
                }

                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui: &mut egui::Ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        // Provider / model pill
                        let model_short = if model_label.len() > 24 {
                            format!(
                                "\u{2026}{}",
                                &model_label[model_label.len().saturating_sub(23)..]
                            )
                        } else {
                            model_label.to_string()
                        };
                        let provider_pill = egui::Frame::new()
                            .fill(palette.bg_tertiary)
                            .corner_radius(egui::CornerRadius::same(3))
                            .inner_margin(egui::Margin::symmetric(5, 1));
                        let provider_response = provider_pill.show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(format!("{} / {}", provider_label, model_short))
                                    .monospace()
                                    .size(10.0)
                                    .color(palette.text_muted),
                            )
                        }).inner;
                        if provider_response.clicked() {
                            actions.clicked_provider = true;
                        }
                        if provider_response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        provider_response.on_hover_text("Open settings");

                        // Status message (right-aligned, before provider)
                        if !status.is_empty() {
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(status)
                                    .size(11.0)
                                    .color(palette.text_muted),
                            );
                        }
                    },
                );
            });
            ui.add_space(3.0);
        });

        actions
    }
}
