//! Tier-3 panels: extension registry browser + lifecycle actions.
//!
//! Extracted verbatim from `tier3_panels.rs` (no logic changes).

use super::struct_def::VelocityApp;
use super::tier3_common::{icon_label_job, primary_button_job};
use crate::editor::extensions::ExtensionState;
use crate::editor::theme::{
    CARD_INNER_MARGIN, CARD_RADIUS, FONT_BODY, FONT_CAPTION, FONT_SMALL, ITEM_SPACING,
};
use eframe::egui;
use egui::RichText;

/// A deferred mutation captured while rendering the extensions list (avoids
/// borrowing `self` mutably during immutable iteration).
enum ExtAction {
    Activate(String),
    Disable(String),
}

impl VelocityApp {
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Extensions -- registry manager
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    pub fn render_extensions_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let active = self.extension_registry.active_count();
        let total = self.extension_registry.extensions.len();
        Self::tier3_header(
            ui,
            "Extensions",
            &format!("{active} active \u{00b7} {total} installed"),
            palette.accent,
            palette.text_muted,
        );

        let mut rescan = false;
        let mut pending: Option<ExtAction> = None;

        ui.horizontal(|ui| {
            // ARROWS_CLOCKWISE collides with an Inter PUA alternate in the shared
            // proportional font (rendered as a stray dot before the label), so the
            // button label goes through the two-run icon+text layout.
            if primary_button_job(
                ui,
                palette,
                icon_label_job(
                    egui_phosphor::regular::ARROWS_CLOCKWISE,
                    "Rescan",
                    FONT_SMALL,
                    palette.text_on_accent,
                ),
            )
            .clicked()
            {
                rescan = true;
            }
            ui.label(
                RichText::new(".velocity/extensions/")
                    .monospace()
                    .size(FONT_CAPTION)
                    .color(palette.text_muted),
            );
        });
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("extensions_scroll")
            .show(ui, |ui| {
                if self.extension_registry.extensions.is_empty() {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new(egui_phosphor::regular::PUZZLE_PIECE)
                                .size(26.0)
                                .color(palette.text_muted),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("No extensions installed")
                                .size(FONT_BODY)
                                .strong()
                                .color(palette.text),
                        );
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new("Drop a manifest folder into .velocity/extensions/")
                                .size(FONT_CAPTION)
                                .color(palette.text_muted),
                        );
                        ui.add_space(ITEM_SPACING);
                        if primary_button_job(
                            ui,
                            palette,
                            icon_label_job(
                                egui_phosphor::regular::ARROWS_CLOCKWISE,
                                "Rescan",
                                FONT_SMALL,
                                palette.text_on_accent,
                            ),
                        )
                        .clicked()
                        {
                            let ws = self.workspace_root.clone();
                            self.extension_registry.scan(&ws);
                        }
                    });
                    return;
                }

                for ext in &self.extension_registry.extensions {
                    let (badge, badge_color) = match ext.state {
                        ExtensionState::Active => ("\u{25cf} active", palette.success),
                        ExtensionState::Installed => ("\u{25cb} installed", palette.text_muted),
                        ExtensionState::Disabled => ("\u{25cb} disabled", palette.warning),
                        ExtensionState::Error => ("\u{2716} error", palette.error),
                    };
                    egui::Frame::new()
                        .fill(palette.bg_secondary)
                        .corner_radius(CARD_RADIUS)
                        .inner_margin(CARD_INNER_MARGIN)
                        .stroke(egui::Stroke::new(0.5, palette.border))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(&ext.manifest.name)
                                        .strong()
                                        .size(FONT_BODY)
                                        .color(palette.text),
                                );
                                ui.label(
                                    RichText::new(format!("v{}", ext.manifest.version))
                                        .size(9.0)
                                        .color(palette.text_muted),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(RichText::new(badge).size(9.0).color(badge_color));
                                    },
                                );
                            });
                            if let Some(desc) = &ext.manifest.description {
                                ui.label(RichText::new(desc).size(9.0).color(palette.text_muted));
                            }
                            let cmds = ext.manifest.contributes.commands.len();
                            let kbs = ext.manifest.contributes.keybindings.len();
                            ui.label(
                                RichText::new(format!(
                                    "{cmds} command(s) \u{00b7} {kbs} keybinding(s)"
                                ))
                                .size(FONT_CAPTION)
                                .color(palette.text_muted),
                            );
                            ui.horizontal(|ui| {
                                if ext.state != ExtensionState::Active
                                    && ui
                                        .small_button(RichText::new("Activate").size(9.0))
                                        .clicked()
                                {
                                    pending = Some(ExtAction::Activate(ext.manifest.id.clone()));
                                }
                                if ext.state == ExtensionState::Active
                                    && ui
                                        .small_button(RichText::new("Disable").size(9.0))
                                        .clicked()
                                {
                                    pending = Some(ExtAction::Disable(ext.manifest.id.clone()));
                                }
                            });
                            if let Some(err) = &ext.error {
                                ui.label(RichText::new(err).size(8.0).color(palette.error));
                            }
                        });
                    ui.add_space(ITEM_SPACING);
                }
            });

        if rescan {
            let ws = self.workspace_root.clone();
            self.extension_registry.scan(&ws);
            self.toasts
                .push(crate::editor::toast::Toast::info("Extensions rescanned"));
        }
        match pending {
            Some(ExtAction::Activate(id)) => {
                if let Err(e) = self.extension_registry.activate(&id) {
                    self.toasts.push(crate::editor::toast::Toast::error(e));
                }
            }
            Some(ExtAction::Disable(id)) => self.extension_registry.disable(&id),
            None => {}
        }
    }
}
