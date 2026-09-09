//! Tier-3 panels: voice-to-task and multimodal input.
//!
//! Extracted verbatim from `tier3_panels.rs` (no logic changes).

use super::struct_def::VelocityApp;
use super::tier3_common::primary_button;
use crate::editor::theme::{
    CARD_INNER_MARGIN, CARD_RADIUS, FONT_CAPTION, FONT_SMALL, ITEM_SPACING,
};
use eframe::egui;
use egui::RichText;

impl VelocityApp {
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Voice -- voice-to-task input
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    pub fn render_voice_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let listening = self.voice_input.listening;
        Self::tier3_header(
            ui,
            "Voice Commands",
            &format!(
                "{:.0}% recognized \u{00b7} {} total",
                self.voice_input.accuracy(),
                self.voice_input.total_commands
            ),
            palette.accent,
            palette.text_muted,
        );

        ui.horizontal(|ui| {
            let (label, color) = if listening {
                (
                    format!("{} Listening", egui_phosphor::regular::MICROPHONE),
                    palette.error,
                )
            } else {
                (
                    format!(
                        "{} Start listening",
                        egui_phosphor::regular::MICROPHONE_SLASH
                    ),
                    palette.text_muted,
                )
            };
            if ui
                .button(RichText::new(label).size(FONT_SMALL).color(color))
                .clicked()
            {
                self.voice_input.toggle_listening();
            }
        });
        ui.add_space(6.0);

        // Manual transcription entry (reuses last_transcription as scratch input).
        let mut parse = false;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.voice_input.last_transcription)
                    .hint_text("Type a phrase, e.g. 'run tests'\u{2026}")
                    .desired_width(ui.available_width() - 96.0),
            );
            if primary_button(
                ui,
                palette,
                format!("{} Parse", egui_phosphor::regular::ARROW_RIGHT),
            )
            .clicked()
            {
                parse = true;
            }
        });
        ui.add_space(6.0);

        if let Some(cmd) = &self.voice_input.last_command {
            egui::Frame::new()
                .fill(palette.bg_secondary)
                .corner_radius(CARD_RADIUS)
                .inner_margin(CARD_INNER_MARGIN)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Intent:").size(9.0).color(palette.text_muted));
                        ui.label(
                            RichText::new(cmd.intent.label())
                                .size(FONT_SMALL)
                                .strong()
                                .color(palette.accent),
                        );
                        ui.label(
                            RichText::new(format!("({:.0}%)", cmd.confidence * 100.0))
                                .size(FONT_CAPTION)
                                .color(palette.text_muted),
                        );
                    });
                    if let Some(target) = cmd.parameters.get("target") {
                        ui.label(
                            RichText::new(format!("Target: {target}"))
                                .size(FONT_CAPTION)
                                .color(palette.text),
                        );
                    }
                });
            ui.add_space(6.0);
        }

        ui.label(
            RichText::new("HISTORY")
                .small()
                .strong()
                .color(palette.accent),
        );
        let mut picked: Option<String> = None;
        egui::ScrollArea::vertical()
            .id_salt("voice_history_scroll")
            .show(ui, |ui| {
                if self.voice_input.command_history.is_empty() {
                    ui.add_space(8.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new(egui_phosphor::regular::MICROPHONE)
                                .size(18.0)
                                .color(palette.text_muted.gamma_multiply(0.5)),
                        );
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new("No commands parsed yet")
                                .size(FONT_CAPTION)
                                .color(palette.text_muted),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("Try an example:")
                                .size(FONT_CAPTION)
                                .color(palette.text_muted.gamma_multiply(0.8)),
                        );
                        ui.add_space(2.0);
                        for phrase in [
                            "run tests",
                            "build the project",
                            "explain this file",
                            "open settings",
                        ] {
                            if ui
                                .button(
                                    RichText::new(format!(
                                        "{} {}",
                                        egui_phosphor::regular::ARROW_RIGHT,
                                        phrase
                                    ))
                                    .size(FONT_CAPTION),
                                )
                                .clicked()
                            {
                                picked = Some(phrase.to_string());
                            }
                        }
                    });
                }
                for cmd in self.voice_input.command_history.iter().rev() {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(cmd.intent.label())
                                .size(FONT_CAPTION)
                                .color(palette.accent),
                        );
                        ui.label(
                            RichText::new(&cmd.raw_text)
                                .size(FONT_CAPTION)
                                .color(palette.text_muted),
                        );
                    });
                }
            });

        if let Some(phrase) = picked {
            self.voice_input.last_transcription = phrase;
            parse = true;
        }

        if parse {
            let text = self.voice_input.last_transcription.clone();
            if !text.trim().is_empty() {
                let intent = self
                    .voice_input
                    .process_transcription(&text)
                    .intent
                    .label()
                    .to_string();
                self.toasts.push(crate::editor::toast::Toast::info(format!(
                    "Parsed intent: {intent}"
                )));
            }
        }
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Multimodal Attachments
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    pub fn render_multimodal_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();

        Self::tier3_header(
            ui,
            "Multimodal Attachments",
            &format!("{} file(s) attached", self.multimodal_attachments.len()),
            palette.accent,
            palette.text_muted,
        );

        ui.label(
            RichText::new(
                "Attach images, documents, or audio files to chat turns. Images are encoded as data: URLs for vision models; documents use OCR fallback.",
            )
            .size(FONT_CAPTION)
            .color(palette.text_muted),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("multimodal_scroll")
            .show(ui, |ui| {
                if self.multimodal_attachments.is_empty() {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new(egui_phosphor::regular::PAPERCLIP)
                                .size(24.0)
                                .color(palette.text_muted.gamma_multiply(0.5)),
                        );
                        ui.add_space(ITEM_SPACING);
                        ui.label(
                            RichText::new(
                                "No attachments yet.\nUse the Chat panel to attach files.",
                            )
                            .size(FONT_SMALL)
                            .color(palette.text_muted),
                        );
                    });
                } else {
                    for att in &self.multimodal_attachments {
                        let kind_color = match att.kind {
                            crate::editor::multimodal::AttachmentKind::Image => palette.success,
                            crate::editor::multimodal::AttachmentKind::Document => palette.accent,
                            crate::editor::multimodal::AttachmentKind::Audio => palette.warning,
                        };
                        egui::Frame::new()
                            .fill(palette.bg_tertiary)
                            .corner_radius(CARD_RADIUS)
                            .inner_margin(CARD_INNER_MARGIN)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(egui_phosphor::regular::CIRCLE)
                                            .color(kind_color),
                                    );
                                    ui.label(
                                        RichText::new(att.kind.label())
                                            .small()
                                            .strong()
                                            .color(kind_color),
                                    );
                                    ui.label(
                                        RichText::new(&att.mime).small().color(palette.text_muted),
                                    );
                                });
                                ui.label(
                                    RichText::new(att.path.display().to_string())
                                        .size(FONT_CAPTION)
                                        .color(palette.text),
                                );
                                ui.label(
                                    RichText::new(format!("{} bytes", att.data.len()))
                                        .small()
                                        .color(palette.text_muted),
                                );
                            });
                        ui.add_space(ITEM_SPACING);
                    }
                }
            });
    }
}
