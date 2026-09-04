//! Tier-3 panels: plugin registry and skill-file management.
//!
//! Extracted verbatim from `tier3_panels.rs` (no logic changes).

use super::struct_def::VelocityApp;
use crate::editor::theme::{
    CARD_INNER_MARGIN, CARD_RADIUS, FONT_CAPTION, FONT_SMALL, ITEM_SPACING,
};
use eframe::egui;
use egui::RichText;

impl VelocityApp {
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Plugin Registry
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    pub fn render_plugin_registry_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let plugin_count = self.plugin_registry.count();
        let plugins = self.plugin_registry.list();
        let all_tools = self.plugin_registry.all_tools();

        Self::tier3_header(
            ui,
            "Plugin Registry",
            &format!(
                "{plugin_count} plugin(s) \u{00b7} {} tool(s)",
                all_tools.len()
            ),
            palette.accent,
            palette.text_muted,
        );

        ui.label(
            RichText::new("Plugins extend the IDE with additional tools and capabilities.")
                .size(FONT_CAPTION)
                .color(palette.text_muted),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("plugin_registry_scroll")
            .show(ui, |ui| {
                if plugins.is_empty() {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("\u{1f4e6}")
                                .size(24.0)
                                .color(palette.text_muted.gamma_multiply(0.5)),
                        );
                        ui.add_space(ITEM_SPACING);
                        ui.label(
                            RichText::new(
                                "No plugins loaded.\nPlace plugin crates in the workspace to discover them.",
                            )
                            .size(FONT_SMALL)
                            .color(palette.text_muted),
                        );
                    });
                } else {
                    for info in &plugins {
                        let status_color = if info.enabled {
                            palette.success
                        } else {
                            palette.text_muted
                        };
                        egui::Frame::new()
                            .fill(palette.bg_tertiary)
                            .corner_radius(CARD_RADIUS)
                            .inner_margin(CARD_INNER_MARGIN)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("\u{25cf}").color(status_color));
                                    ui.label(
                                        RichText::new(&info.name).strong().color(palette.text),
                                    );
                                    ui.label(
                                        RichText::new(format!("v{}", info.version))
                                            .small()
                                            .color(palette.text_muted),
                                    );
                                });
                                ui.label(
                                    RichText::new(&info.description)
                                        .size(FONT_CAPTION)
                                        .color(palette.text),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "{} tool(s): {}",
                                        info.tool_count,
                                        info.tool_names.join(", ")
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

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Skill Files
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    pub fn render_skill_files_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();

        Self::tier3_header(
            ui,
            "Skill Files",
            &format!("{} skill(s) loaded", self.skill_files.len()),
            palette.accent,
            palette.text_muted,
        );

        ui.label(
            RichText::new(
                "Skills are reusable capability definitions injected into agent system prompts when tasks are routed to team members.",
            )
            .size(FONT_CAPTION)
            .color(palette.text_muted),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("skill_files_scroll")
            .show(ui, |ui| {
                if self.skill_files.is_empty() {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("\u{1f4dc}")
                                .size(24.0)
                                .color(palette.text_muted.gamma_multiply(0.5)),
                        );
                        ui.add_space(ITEM_SPACING);
                        ui.label(
                            RichText::new(
                                "No skill files loaded.\nSkills are loaded from .velocity/skills/.",
                            )
                            .size(FONT_SMALL)
                            .color(palette.text_muted),
                        );
                    });
                } else {
                    for skill in &self.skill_files {
                        egui::Frame::new()
                            .fill(palette.bg_tertiary)
                            .corner_radius(CARD_RADIUS)
                            .inner_margin(CARD_INNER_MARGIN)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&skill.name).strong().color(palette.accent),
                                    );
                                    ui.label(
                                        RichText::new(format!("[{}]", skill.id))
                                            .small()
                                            .color(palette.text_muted),
                                    );
                                });
                                ui.label(
                                    RichText::new(&skill.description)
                                        .size(FONT_CAPTION)
                                        .color(palette.text),
                                );
                                // Show first 120 chars of body as preview
                                let preview: String = skill.body.chars().take(120).collect();
                                if preview.len() < skill.body.len() {
                                    ui.label(
                                        RichText::new(format!("{preview}\u{2026}"))
                                            .small()
                                            .monospace()
                                            .color(palette.text_muted),
                                    );
                                }
                            });
                        ui.add_space(ITEM_SPACING);
                    }
                }
            });
    }
}
