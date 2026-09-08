//! Checkpoints view rendering for `VelocityApp`.
//!
//! Extracted verbatim from `ui_render.rs` (no logic changes).
use super::struct_def::VelocityApp;
use eframe::egui;

impl VelocityApp {
    /// Render checkpoint list in the bottom panel Checkpoints tab.
    pub fn render_checkpoints(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();

        if !self.checkpoint_manager.enabled {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("\u{1f4be}")
                        .size(22.0)
                        .color(palette.text_muted.gamma_multiply(0.6)),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Checkpointing unavailable")
                        .size(11.0)
                        .strong()
                        .color(palette.text),
                );
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(
                        "Initialize a git repository to enable automatic checkpoints",
                    )
                    .size(9.0)
                    .color(palette.text_muted),
                );
            });
            return;
        }

        if self.checkpoint_manager.checkpoints.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("\u{1f4be}")
                        .size(22.0)
                        .color(palette.accent.gamma_multiply(0.6)),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("No checkpoints yet")
                        .size(11.0)
                        .strong()
                        .color(palette.text),
                );
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(
                        "Checkpoints are created automatically before agent operations",
                    )
                    .size(9.0)
                    .color(palette.text_muted),
                );
            });
            return;
        }

        // Header when checkpoints exist
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("\u{1f4be} Checkpoints")
                    .size(10.0)
                    .strong()
                    .color(palette.accent),
            );
            ui.label(
                egui::RichText::new(format!("({})", self.checkpoint_manager.checkpoints.len()))
                    .size(9.0)
                    .color(palette.text_muted),
            );
        });
        ui.add_space(4.0);

        let mut action: Option<crate::editor::bottom_panel::CheckpointAction> = None;
        for (idx, cp) in self.checkpoint_manager.checkpoints.iter().enumerate() {
            egui::Frame::new()
                .fill(palette.bg_secondary)
                .corner_radius(4.0)
                .inner_margin(6.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(&cp.label)
                                .size(10.0)
                                .strong()
                                .color(palette.text),
                        );
                        ui.label(
                            egui::RichText::new(format!("{} file(s)", cp.files_changed))
                                .size(9.0)
                                .color(palette.text_muted),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .small_button(
                                    egui::RichText::new("\u{2716} Discard")
                                        .size(9.0)
                                        .color(palette.error),
                                )
                                .clicked()
                            {
                                action = Some(
                                    crate::editor::bottom_panel::CheckpointAction::Discard(idx),
                                );
                            }
                            if ui
                                .small_button(
                                    egui::RichText::new("\u{21A9} Restore")
                                        .size(9.0)
                                        .color(palette.success),
                                )
                                .clicked()
                            {
                                action = Some(
                                    crate::editor::bottom_panel::CheckpointAction::Restore(idx),
                                );
                            }
                        });
                    });
                });
            ui.add_space(2.0);
        }

        // Process the action
        if let Some(act) = action {
            match act {
                crate::editor::bottom_panel::CheckpointAction::Restore(idx) => {
                    match self.checkpoint_manager.restore_checkpoint(idx) {
                        Ok(label) => {
                            self.toasts
                                .push(crate::editor::toast::Toast::success(format!(
                                    "Restored: {}",
                                    label
                                )));
                            self.status_message = format!("Checkpoint restored: {}", label);
                            // Refresh git state and reload buffers
                            self.git_state.refresh(&self.workspace_root);
                        }
                        Err(e) => {
                            self.toasts.push(crate::editor::toast::Toast::error(format!(
                                "Restore failed: {}",
                                e
                            )));
                        }
                    }
                }
                crate::editor::bottom_panel::CheckpointAction::Discard(idx) => {
                    match self.checkpoint_manager.discard_checkpoint(idx) {
                        Ok(label) => {
                            self.toasts.push(crate::editor::toast::Toast::info(format!(
                                "Discarded: {}",
                                label
                            )));
                        }
                        Err(e) => {
                            self.toasts.push(crate::editor::toast::Toast::error(format!(
                                "Discard failed: {}",
                                e
                            )));
                        }
                    }
                }
            }
        }
    }
}
