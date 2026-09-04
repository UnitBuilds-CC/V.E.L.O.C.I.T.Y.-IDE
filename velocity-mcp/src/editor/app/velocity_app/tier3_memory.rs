//! Tier-3 panels: agent, persistent, and shared memory.
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
    // Agent Memory -- persistent per-member knowledge store
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    pub fn render_agent_memory_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let total_memories: usize = self
            .agent_memory
            .stores
            .iter()
            .map(|s| s.memories.len())
            .sum();
        let member_count = self.agent_memory.stores.len();

        Self::tier3_header(
            ui,
            "Agent Memory",
            &format!(
                "{} member(s) \u{00b7} {} memories",
                member_count, total_memories
            ),
            palette.accent,
            palette.text_muted,
        );

        // Controls
        let mut load = false;
        let mut save = false;
        ui.horizontal(|ui| {
            if ui
                .button(RichText::new("\u{1f504} Load All").size(FONT_SMALL))
                .clicked()
            {
                load = true;
            }
            if ui
                .button(RichText::new("\u{1f4be} Save All").size(FONT_SMALL))
                .clicked()
            {
                save = true;
            }
            ui.label(
                RichText::new("Encrypted with NDA")
                    .size(FONT_CAPTION)
                    .color(palette.text_muted),
            );
        });
        ui.add_space(6.0);

        // Member stores
        if self.agent_memory.stores.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("\u{1f9e0}")
                        .size(22.0)
                        .color(palette.text_muted.gamma_multiply(0.5)),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("No agent memories yet")
                        .size(FONT_SMALL)
                        .strong()
                        .color(palette.text),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new("Memories are created during agent execution")
                        .size(9.0)
                        .color(palette.text_muted.gamma_multiply(0.7)),
                );
            });
        } else {
            egui::ScrollArea::vertical()
                .id_salt("agent_memory_scroll")
                .show(ui, |ui| {
                    for store in &self.agent_memory.stores {
                        egui::CollapsingHeader::new(
                            RichText::new(format!(
                                "\u{1f464} {} ({} memories)",
                                store.member_id,
                                store.memories.len()
                            ))
                            .size(FONT_SMALL)
                            .strong(),
                        )
                        .default_open(false)
                        .show(ui, |ui| {
                            for mem in &store.memories {
                                egui::Frame::new()
                                    .fill(palette.bg_secondary)
                                    .corner_radius(CARD_RADIUS)
                                    .inner_margin(CARD_INNER_MARGIN)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(&mem.title)
                                                    .size(FONT_SMALL)
                                                    .strong()
                                                    .color(palette.text),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        RichText::new(&mem.category)
                                                            .size(FONT_CAPTION)
                                                            .monospace()
                                                            .color(palette.accent),
                                                    );
                                                },
                                            );
                                        });
                                        ui.label(
                                            RichText::new(&mem.content)
                                                .size(FONT_CAPTION)
                                                .color(palette.text_muted),
                                        );
                                        if !mem.keywords.is_empty() {
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    RichText::new("Keywords:")
                                                        .size(FONT_CAPTION)
                                                        .color(palette.text_muted),
                                                );
                                                ui.label(
                                                    RichText::new(mem.keywords.join(", "))
                                                        .size(FONT_CAPTION)
                                                        .color(palette.text_muted),
                                                );
                                            });
                                        }
                                    });
                                ui.add_space(ITEM_SPACING);
                            }
                        });
                        ui.add_space(ITEM_SPACING);
                    }
                });
        }

        if load {
            self.agent_memory.load_all();
            self.toasts
                .push(crate::editor::toast::Toast::success(format!(
                    "Loaded {} member store(s)",
                    self.agent_memory.stores.len()
                )));
        }
        if save {
            self.agent_memory.save_all();
            self.toasts
                .push(crate::editor::toast::Toast::success("All memories saved"));
        }
    }

    pub fn render_shared_memory_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let entry_count = self.shared_memory.entries.len();
        let annotation_count = self.shared_memory.annotations.len();

        Self::tier3_header(
            ui,
            "Shared Memory",
            &format!("{entry_count} entries \u{00b7} {annotation_count} annotations"),
            palette.accent,
            palette.text_muted,
        );

        ui.label(
            RichText::new(
                "Shared knowledge base for multi-agent collaboration. Agents can publish and query knowledge entries across team boundaries.",
            )
            .size(FONT_CAPTION)
            .color(palette.text_muted),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("shared_memory_scroll")
            .show(ui, |ui| {
                if entry_count == 0 {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("\u{1f4da}")
                                .size(24.0)
                                .color(palette.text_muted.gamma_multiply(0.5)),
                        );
                        ui.add_space(ITEM_SPACING);
                        ui.label(
                            RichText::new("No shared knowledge entries yet.")
                                .size(FONT_SMALL)
                                .color(palette.text_muted),
                        );
                    });
                } else {
                    for (id, entry) in self.shared_memory.entries.iter().take(20) {
                        egui::Frame::new()
                            .fill(palette.bg_tertiary)
                            .corner_radius(CARD_RADIUS)
                            .inner_margin(CARD_INNER_MARGIN)
                            .show(ui, |ui| {
                                ui.label(RichText::new(&entry.title).strong().color(palette.text));
                                ui.label(
                                    RichText::new(format!("[{id}] {:?}", entry.category))
                                        .small()
                                        .color(palette.text_muted),
                                );
                                let preview: String = entry.content.chars().take(120).collect();
                                ui.label(RichText::new(preview).size(9.0).color(palette.text));
                            });
                        ui.add_space(ITEM_SPACING);
                    }
                }
            });
    }

    pub fn render_persistent_memory_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let entry_count = self.persistent_memory.len();

        Self::tier3_header(
            ui,
            "Persistent Memory",
            &format!("{entry_count} entries \u{00b7} NDA-encrypted at rest"),
            palette.accent,
            palette.text_muted,
        );

        ui.label(
            RichText::new(
                "Long-term memory store encrypted with NDA at rest. Agents can remember, recall, reinforce, and forget entries across sessions.",
            )
            .size(FONT_CAPTION)
            .color(palette.text_muted),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("persistent_memory_scroll")
            .show(ui, |ui| {
                ui.label(
                    RichText::new(format!("Storage: {} / max entries", entry_count))
                        .size(FONT_CAPTION)
                        .color(palette.text_muted),
                );
                ui.add_space(ITEM_SPACING);

                if entry_count == 0 {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("\u{1f512}")
                                .size(24.0)
                                .color(palette.text_muted.gamma_multiply(0.5)),
                        );
                        ui.add_space(ITEM_SPACING);
                        ui.label(
                            RichText::new(
                                "Memory is empty.\nAgents will populate it during execution.",
                            )
                            .size(FONT_SMALL)
                            .color(palette.text_muted),
                        );
                    });
                } else {
                    ui.label(
                        RichText::new("STORED ENTRIES")
                            .small()
                            .strong()
                            .color(palette.accent),
                    );
                    ui.add_space(2.0);
                    for entry in self.persistent_memory.iter().take(30) {
                        egui::Frame::new()
                            .fill(palette.bg_tertiary)
                            .corner_radius(CARD_RADIUS)
                            .inner_margin(CARD_INNER_MARGIN)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("\u{1f512}")
                                            .size(FONT_SMALL)
                                            .color(palette.text_muted),
                                    );
                                    ui.label(
                                        RichText::new(&entry.key).strong().color(palette.text),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "accessed \u{00d7}{}",
                                            entry.access_count
                                        ))
                                        .small()
                                        .color(palette.text_muted),
                                    );
                                });
                                let preview: String = entry.content.chars().take(100).collect();
                                ui.label(RichText::new(preview).size(9.0).color(palette.text));
                            });
                        ui.add_space(ITEM_SPACING);
                    }
                    if entry_count > 30 {
                        ui.label(
                            RichText::new(format!("... and {} more entries", entry_count - 30))
                                .size(FONT_CAPTION)
                                .color(palette.text_muted),
                        );
                    }
                }
            });
    }
}
