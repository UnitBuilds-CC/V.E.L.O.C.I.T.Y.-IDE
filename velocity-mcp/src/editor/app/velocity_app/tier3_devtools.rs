//! Tier-3 panels: LSP diagnostics, debugger, and inline AI suggestions.
//!
//! Extracted verbatim from `tier3_panels.rs` (no logic changes).

use eframe::egui;
use egui::RichText;
use super::struct_def::VelocityApp;
use crate::editor::theme::{FONT_CAPTION, FONT_SMALL, ITEM_SPACING, SECTION_SPACING};

impl VelocityApp {
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // LSP Client -- Language Server Protocol status and diagnostics
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    pub fn render_lsp_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();

        let (server_count, diag_count) = match &self.lsp_manager {
            Some(mgr) => (mgr.server_count(), mgr.diagnostics_count()),
            None => (0, 0),
        };

        let status_label = if server_count > 0 {
            format!(
                "{} server{}",
                server_count,
                if server_count == 1 { "" } else { "s" }
            )
        } else {
            "Not initialized".to_string()
        };

        Self::tier3_header(
            ui,
            "Language Servers",
            &status_label,
            palette.accent,
            palette.text_muted,
        );

        egui::ScrollArea::vertical()
            .id_salt("lsp_scroll")
            .show(ui, |ui| {
                if let Some(mgr) = &mut self.lsp_manager {
                    let snapshot = mgr.server_snapshot();

                    if snapshot.is_empty() {
                        ui.add_space(16.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new("\u{1f4c6}")
                                    .size(22.0)
                                    .color(palette.text_muted.gamma_multiply(0.5)),
                            );
                            ui.add_space(ITEM_SPACING);
                            ui.label(
                                RichText::new("No language servers detected")
                                    .size(FONT_SMALL)
                                    .strong()
                                    .color(palette.text),
                            );
                            ui.add_space(2.0);
                            ui.label(
                                RichText::new(
                                    "Add Cargo.toml or package.json to auto-start servers",
                                )
                                .size(9.0)
                                .color(palette.text_muted.gamma_multiply(0.7)),
                            );
                        });
                    } else {
                        // Diagnostics summary
                        ui.label(
                            RichText::new(format!(
                                "{} diagnostic{} across all files",
                                diag_count,
                                if diag_count == 1 { "" } else { "s" }
                            ))
                            .size(FONT_CAPTION)
                            .color(if diag_count > 0 {
                                palette.warning
                            } else {
                                palette.text_muted
                            }),
                        );
                        ui.add_space(SECTION_SPACING);
                        
                        // Per-server cards
                        for srv in &snapshot {
                            let alive_color = if srv.alive {
                                palette.success
                            } else {
                                palette.error
                            };
                            let init_label = if srv.initialized {
                                "initialized"
                            } else {
                                "starting..."
                            };

                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    // Status dot
                                    ui.label(
                                        RichText::new("\u{25cf}").size(FONT_SMALL).color(alive_color),
                                    );
                                    ui.label(
                                        RichText::new(&srv.language)
                                            .size(FONT_SMALL)
                                            .strong()
                                            .color(palette.text),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(init_label)
                                                    .size(FONT_CAPTION)
                                                    .color(palette.text_muted),
                                            );
                                        },
                                    );
                                });
                                ui.label(
                                    RichText::new(format!(
                                        "command: {}  \u{00b7}  extensions: {}",
                                        srv.command,
                                        srv.extensions.join(", ")
                                    ))
                                    .size(FONT_CAPTION)
                                    .color(palette.text_muted),
                                );
                            });
                            ui.add_space(ITEM_SPACING);
                        }
                    }
                } else {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("\u{25c7}")
                                .size(26.0)
                                .color(palette.text_muted),
                        );
                        ui.label(
                            RichText::new(
                                "LSP not initialized. LSP servers are configured per-language.",
                            )
                            .size(FONT_SMALL)
                            .color(palette.text_muted),
                        );
                    });
                }
            });
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Debugger -- DAP (Debug Adapter Protocol) controls
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    pub fn render_debugger_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();

        Self::tier3_header(
            ui,
            "Debugger",
            if self.dap_client.is_some() {
                "Connected"
            } else {
                "Not connected"
            },
            palette.accent,
            palette.text_muted,
        );

        if self.dap_client.is_none() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("\u{25c7}")
                        .size(26.0)
                        .color(palette.text_muted),
                );
                ui.label(
                    RichText::new("Debugger not connected. Use 'Debug: Attach' from the toolbar.")
                        .size(FONT_SMALL)
                        .color(palette.text_muted),
                );
            });
        } else {
            ui.label(
                RichText::new("Debugger is connected and ready for debugging.")
                    .size(FONT_CAPTION)
                    .color(palette.text_muted),
            );
            ui.add_space(6.0);

            // Debug controls -- defer DAP calls to avoid borrowing self during render.
            enum DbgAction {
                Continue,
                Pause,
                StepOver,
                StepInto,
                StepOut,
                Stop,
            }
            let mut dbg: Option<DbgAction> = None;

            ui.horizontal(|ui| {
                if ui
                    .button(RichText::new("\u{25b6} Continue").size(FONT_SMALL))
                    .clicked()
                {
                    dbg = Some(DbgAction::Continue);
                }
                if ui
                    .button(RichText::new("\u{23f9} Pause").size(FONT_SMALL))
                    .clicked()
                {
                    dbg = Some(DbgAction::Pause);
                }
                if ui
                    .button(RichText::new("\u{23ed} Step Over").size(FONT_SMALL))
                    .clicked()
                {
                    dbg = Some(DbgAction::StepOver);
                }
            });
            ui.add_space(ITEM_SPACING);
            ui.horizontal(|ui| {
                if ui
                    .button(RichText::new("\u{2935} Step Into").size(FONT_SMALL))
                    .clicked()
                {
                    dbg = Some(DbgAction::StepInto);
                }
                if ui
                    .button(RichText::new("\u{2934} Step Out").size(FONT_SMALL))
                    .clicked()
                {
                    dbg = Some(DbgAction::StepOut);
                }
                if ui
                    .button(RichText::new("\u{23f9} Stop").size(FONT_SMALL))
                    .clicked()
                {
                    dbg = Some(DbgAction::Stop);
                }
            });

            if let Some(dap) = &mut self.dap_client {
                if let Some(action) = dbg {
                    let result = match action {
                        DbgAction::Continue => dap.continue_execution(),
                        DbgAction::Pause => dap.pause(),
                        DbgAction::StepOver => dap.step_over(),
                        DbgAction::StepInto => dap.step_into(),
                        DbgAction::StepOut => dap.step_out(),
                        DbgAction::Stop => dap.stop(),
                    };
                    match result {
                        Ok(()) => {
                            let label = match action {
                                DbgAction::Continue => "Continue",
                                DbgAction::Pause => "Pause",
                                DbgAction::StepOver => "Step Over",
                                DbgAction::StepInto => "Step Into",
                                DbgAction::StepOut => "Step Out",
                                DbgAction::Stop => "Stop",
                            };
                            self.toasts
                                .push(crate::editor::toast::Toast::success(format!(
                                    "DAP: {label} sent"
                                )));
                        }
                        Err(e) => {
                            self.toasts.push(crate::editor::toast::Toast::error(e));
                        }
                    }
                }
            }
        }
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Inline Suggestions
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    pub fn render_inline_suggestions_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();

        let enabled = self.inline_suggestions.config.enabled;
        let total_shown = self.inline_suggestions.total_shown;
        let total_accepted = self.inline_suggestions.total_accepted;
        let total_dismissed = self.inline_suggestions.total_dismissed;
        let cache_entries = self.inline_suggestions.suggestion_cache.len();
        let recent_count = self.inline_suggestions.recent_suggestions.len();
        let has_current = self.inline_suggestions.current_suggestion.is_some();

        Self::tier3_header(
            ui,
            "Inline Suggestions",
            if enabled { "enabled" } else { "disabled" },
            if enabled {
                palette.success
            } else {
                palette.text_muted
            },
            palette.text_muted,
        );

        ui.label(
            RichText::new(
                "Ghost-text suggestions that appear inline as you type, powered by the completion engine. Press Tab to accept, Escape to dismiss.",
            )
            .size(FONT_CAPTION)
            .color(palette.text_muted),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("inline_suggestions_scroll")
            .show(ui, |ui| {
                // Configuration
                ui.label(
                    RichText::new("CONFIGURATION")
                        .small()
                        .strong()
                        .color(palette.accent),
                );
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Enabled:")
                            .size(FONT_CAPTION)
                            .color(palette.text_muted),
                    );
                    if ui
                        .checkbox(&mut self.inline_suggestions.config.enabled, "")
                        .changed()
                    {
                        // config updated
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Trigger delay:")
                            .size(FONT_CAPTION)
                            .color(palette.text_muted),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{}ms",
                            self.inline_suggestions.config.trigger_delay_ms
                        ))
                        .size(FONT_CAPTION)
                        .color(palette.text),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Max chars:")
                            .size(FONT_CAPTION)
                            .color(palette.text_muted),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{}",
                            self.inline_suggestions.config.max_suggestion_chars
                        ))
                        .size(FONT_CAPTION)
                        .color(palette.text),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Min confidence:")
                            .size(FONT_CAPTION)
                            .color(palette.text_muted),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{:.0}%",
                            self.inline_suggestions.config.min_confidence * 100.0
                        ))
                        .size(FONT_CAPTION)
                        .color(palette.text),
                    );
                });
                ui.add_space(6.0);

                // Statistics
                ui.label(
                    RichText::new("STATISTICS")
                        .small()
                        .strong()
                        .color(palette.accent),
                );
                ui.label(
                    RichText::new(format!("  \u{2022} Shown: {total_shown}"))
                        .size(FONT_CAPTION)
                        .color(palette.text),
                );
                ui.label(
                    RichText::new(format!("  \u{2022} Accepted: {total_accepted}"))
                        .size(FONT_CAPTION)
                        .color(palette.success),
                );
                ui.label(
                    RichText::new(format!("  \u{2022} Dismissed: {total_dismissed}"))
                        .size(FONT_CAPTION)
                        .color(palette.warning),
                );
                let accept_rate = if total_shown > 0 {
                    (total_accepted as f32 / total_shown as f32) * 100.0
                } else {
                    0.0
                };
                ui.label(
                    RichText::new(format!("  \u{2022} Accept rate: {accept_rate:.1}%"))
                        .size(FONT_CAPTION)
                        .color(palette.accent),
                );
                ui.add_space(ITEM_SPACING);
                
                // Cache info
                ui.label(
                    RichText::new("CACHE")
                        .small()
                        .strong()
                        .color(palette.accent),
                );
                ui.label(
                    RichText::new(format!("  \u{2022} Reuse cache: {cache_entries} entries"))
                        .size(FONT_CAPTION)
                        .color(palette.text),
                );
                ui.label(
                    RichText::new(format!("  \u{2022} Recent: {recent_count} entries"))
                        .size(FONT_CAPTION)
                        .color(palette.text),
                );
                let status = if has_current {
                    ("Pending suggestion", palette.warning)
                } else {
                    ("Idle \u{2014} waiting for trigger", palette.text_muted)
                };
                ui.label(
                    RichText::new(format!("  \u{2022} Status: {}", status.0))
                        .size(FONT_CAPTION)
                        .color(status.1),
                );
            });
    }
}
