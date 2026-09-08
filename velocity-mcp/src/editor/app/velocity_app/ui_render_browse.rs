//! Browse panel rendering for `VelocityApp`.
//!
//! Extracted verbatim from `ui_render.rs` (no logic changes).
use super::struct_def::VelocityApp;
use eframe::egui;

impl VelocityApp {
    pub fn browse_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        ui.set_max_width(ui.available_width());

        // Poll for progress updates
        self.browse_state.poll();

        egui::Frame::new()
            .inner_margin(egui::Margin::same(10))
            .fill(palette.bg_primary)
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.heading(
                        egui::RichText::new("\u{1F310} Browse")
                            .size(14.0)
                            .color(palette.accent),
                    );
                    ui.add_space(4.0);

                    // URL input (optional)
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("URL")
                                .size(9.0)
                                .color(palette.text_muted),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.browse_state.url_input)
                                .hint_text("https://... (optional)")
                                .desired_width(ui.available_width() - 4.0),
                        );
                    });

                    // Query input + send
                    ui.horizontal(|ui| {
                        let input_resp = ui.add(
                            egui::TextEdit::singleline(&mut self.browse_state.input)
                                .hint_text("Ask a question...")
                                .desired_width(ui.available_width() - 50.0),
                        );
                        let enter = input_resp.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let send = ui
                            .add_enabled(
                                !self.browse_state.waiting
                                    && !self.browse_state.input.trim().is_empty(),
                                egui::Button::new(egui::RichText::new("Go").size(10.0)),
                            )
                            .clicked();

                        if (enter || send)
                            && !self.browse_state.waiting
                            && !self.browse_state.input.trim().is_empty()
                        {
                            let ws = self.workspace_root.clone();
                            let provider = self.provider;
                            let model = self.selected_model.clone();
                            self.browse_state.send(&ws, provider, &model);
                        }
                    });

                    if self.browse_state.waiting {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(
                                egui::RichText::new("Browsing...")
                                    .size(9.0)
                                    .color(palette.warning),
                            );
                        });
                    }

                    ui.separator();

                    // Messages area
                    egui::ScrollArea::vertical()
                        .id_salt("browse_panel_scroll")
                        .stick_to_bottom(true)
                        .max_width(ui.available_width())
                        .show(ui, |ui| {
                            ui.set_max_width(ui.available_width());
                            for msg in &self.browse_state.messages {
                                match msg.role.as_str() {
                                    "user" => {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(
                                                egui::RichText::new("\u{25B6}")
                                                    .size(9.0)
                                                    .color(palette.accent),
                                            );
                                            ui.label(
                                                egui::RichText::new(&msg.content)
                                                    .size(10.0)
                                                    .strong()
                                                    .color(palette.text),
                                            );
                                        });
                                    }
                                    "assistant" => {
                                        egui::Frame::new()
                                            .fill(palette.bg_secondary)
                                            .corner_radius(6.0)
                                            .inner_margin(6.0)
                                            .show(ui, |ui| {
                                                ui.set_max_width(ui.available_width());
                                                ui.label(
                                                    egui::RichText::new(&msg.content)
                                                        .size(10.0)
                                                        .color(palette.text),
                                                );
                                            });
                                    }
                                    "streaming" => {
                                        egui::Frame::new()
                                            .fill(palette.bg_tertiary)
                                            .corner_radius(6.0)
                                            .inner_margin(6.0)
                                            .stroke(egui::Stroke::new(0.5, palette.accent))
                                            .show(ui, |ui| {
                                                ui.set_max_width(ui.available_width());
                                                ui.label(
                                                    egui::RichText::new(&msg.content)
                                                        .size(10.0)
                                                        .color(palette.text),
                                                );
                                                ui.label(
                                                    egui::RichText::new("\u{2588}")
                                                        .size(10.0)
                                                        .color(palette.accent),
                                                );
                                            });
                                    }
                                    "status" => {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "  \u{2022} {}",
                                                msg.content
                                            ))
                                            .size(9.0)
                                            .italics()
                                            .color(palette.text_muted),
                                        );
                                    }
                                    _ => {}
                                }
                                ui.add_space(3.0);
                            }
                        });
                });
            });
    }
}
