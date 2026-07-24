use crate::editor::theme::IdePalette;
use eframe::egui::{self, Panel, Ui};

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
    ) {
        Panel::bottom("status_bar").show(ui, |ui: &mut egui::Ui| {
            // Subtle dot separator instead of hard vertical rules — less clutter.
            let dot = |ui: &mut egui::Ui| {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("·")
                        .size(12.0)
                        .color(palette.text_muted.gamma_multiply(0.6)),
                );
                ui.add_space(8.0);
            };

            ui.add_space(2.0);
            ui.horizontal(|ui: &mut egui::Ui| {
                // Always-on work-mode badge (glyph + name) so the current mode
                // is unmistakable; the caller supplies the mode's own glyph.
                ui.label(
                    egui::RichText::new(mode)
                        .size(12.0)
                        .strong()
                        .color(palette.accent),
                );
                dot(ui);

                let (icon, color) = if build_ok {
                    ("✔", palette.success)
                } else {
                    ("✖", palette.error)
                };

                ui.label(
                    egui::RichText::new(format!("{} build", icon))
                        .color(color)
                        .size(12.0),
                );

                if let Some(b) = branch {
                    dot(ui);
                    ui.label(
                        egui::RichText::new(format!("⎇ {}", b))
                            .size(12.0)
                            .color(palette.text_muted),
                    );
                }

                if let Some((line, col)) = position {
                    dot(ui);
                    ui.label(
                        egui::RichText::new(format!("Ln {}, Col {}", line + 1, col + 1))
                            .size(12.0)
                            .color(palette.text_muted),
                    );
                }

                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui: &mut egui::Ui| {
                        ui.label(
                            egui::RichText::new("Ctrl+Shift+P")
                                .monospace()
                                .size(11.0)
                                .color(palette.text_muted.gamma_multiply(0.7)),
                        );
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new(status)
                                .size(12.0)
                                .color(palette.text_muted),
                        );
                    },
                );
            });
            ui.add_space(2.0);
        });
    }
}
