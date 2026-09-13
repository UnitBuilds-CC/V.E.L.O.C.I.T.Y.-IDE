//! Tier-3 panels: automation triggers + trigger label helpers.
//!
//! Extracted verbatim from `tier3_panels.rs` (no logic changes).

use super::struct_def::VelocityApp;
use super::tier3_common::human_secs;
use crate::editor::theme::{
    CARD_INNER_MARGIN, CARD_RADIUS, FONT_CAPTION, FONT_SMALL, ITEM_SPACING, SECTION_SPACING,
};
use eframe::egui;
use egui::RichText;

impl VelocityApp {
    pub fn render_triggers_panel(&mut self, ui: &mut egui::Ui) {
        use crate::editor::triggers::{
            now_secs, parse_schedule, Trigger, TriggerAction, TriggerKind,
        };
        let palette = self.palette();
        Self::tier3_header(
            ui,
            "Triggers",
            &format!(
                "{} trigger(s) \u{00b7} headless via --daemon",
                self.triggers.len()
            ),
            palette.accent,
            palette.text_muted,
        );

        // Add a schedule trigger: name Â· spec Â· prompt.
        let mut add = false;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.trigger_name_input)
                    .hint_text("name\u{2026}")
                    .desired_width(120.0),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.trigger_interval_input)
                    .hint_text("5m \u{00b7} 1h \u{00b7} daily@09:00")
                    .desired_width(140.0),
            );
            if ui.button(RichText::new("Add").size(FONT_SMALL)).clicked() {
                add = true;
            }
        });
        ui.add_space(ITEM_SPACING);
        ui.add(
            egui::TextEdit::multiline(&mut self.trigger_prompt_input)
                .hint_text("agent prompt to run when this schedule fires\u{2026}")
                .desired_rows(2)
                .desired_width(ui.available_width()),
        );
        if self.trigger_interval_input.trim().is_empty()
            || parse_schedule(self.trigger_interval_input.trim()).is_some()
        {
            // valid or empty -- no warning
        } else {
            ui.label(
                RichText::new("unrecognized schedule spec")
                    .size(FONT_CAPTION)
                    .color(palette.error),
            );
        }
        ui.add_space(SECTION_SPACING);

        // Trigger list.
        let now = now_secs();
        let mut toggle: Option<String> = None;
        let mut remove: Option<String> = None;
        let mut run_now: Option<String> = None;
        egui::ScrollArea::vertical()
            .id_salt("triggers_list_scroll")
            .max_height(320.0)
            .show(ui, |ui| {
                if self.triggers.is_empty() {
                    ui.label(
                        RichText::new("No triggers yet. Add a schedule above.")
                            .size(FONT_CAPTION)
                            .color(palette.text_muted),
                    );
                }
                for t in &self.triggers.triggers {
                    egui::Frame::new()
                        .fill(palette.bg_secondary)
                        .corner_radius(CARD_RADIUS)
                        .inner_margin(CARD_INNER_MARGIN)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let dot = if t.enabled { "\u{25cf}" } else { "\u{25cb}" };
                                ui.label(RichText::new(dot).size(FONT_SMALL).color(if t.enabled {
                                    palette.success
                                } else {
                                    palette.text_muted
                                }));
                                ui.label(
                                    RichText::new(&t.name)
                                        .size(FONT_SMALL)
                                        .strong()
                                        .color(palette.text),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .small_button(RichText::new("\u{2716}").size(8.0))
                                            .clicked()
                                        {
                                            remove = Some(t.id.clone());
                                        }
                                        let label = if t.enabled { "Disable" } else { "Enable" };
                                        if ui.small_button(RichText::new(label).size(8.0)).clicked()
                                        {
                                            toggle = Some(t.id.clone());
                                        }
                                        if ui
                                            .small_button(RichText::new("Run now").size(8.0))
                                            .clicked()
                                        {
                                            run_now = Some(t.id.clone());
                                        }
                                    },
                                );
                            });
                            ui.label(
                                RichText::new(trigger_kind_label(&t.kind))
                                    .size(FONT_CAPTION)
                                    .color(palette.accent),
                            );
                            ui.label(
                                RichText::new(trigger_action_label(&t.action))
                                    .size(FONT_CAPTION)
                                    .color(palette.text_muted),
                            );
                            let due = match t.seconds_until_due(now) {
                                Some(0) => "due now".to_string(),
                                Some(secs) => format!("next in {}", human_secs(secs)),
                                None => "external / manual".to_string(),
                            };
                            ui.label(RichText::new(due).size(8.0).color(palette.text_muted));
                        });
                    ui.add_space(ITEM_SPACING);
                }
            });

        // Deferred mutations (avoid borrowing self during rendering).
        if add {
            let name = self.trigger_name_input.trim().to_string();
            let spec = self.trigger_interval_input.trim().to_string();
            let prompt = self.trigger_prompt_input.trim().to_string();
            if name.is_empty() || spec.is_empty() || prompt.is_empty() {
                self.toasts.push(crate::editor::toast::Toast::error(
                    "Trigger needs a name, schedule spec, and prompt",
                ));
            } else if parse_schedule(&spec).is_none() {
                self.toasts.push(crate::editor::toast::Toast::error(format!(
                    "Invalid schedule spec: {spec}"
                )));
            } else {
                let id = format!("trg-{}", now_secs());
                self.triggers.add(Trigger::new(
                    id,
                    name.clone(),
                    TriggerKind::Schedule { interval: spec },
                    TriggerAction::AgentPrompt { prompt },
                ));
                let ws = self.workspace_root.clone();
                if let Err(e) = self.triggers.save(&ws) {
                    Self::persist_err(&mut self.toasts, "triggers", &e);
                }
                self.trigger_name_input.clear();
                self.trigger_interval_input.clear();
                self.trigger_prompt_input.clear();
                self.toasts.push(crate::editor::toast::Toast::info(format!(
                    "Added trigger '{name}'"
                )));
            }
        }
        if let Some(id) = toggle {
            self.triggers.toggle(&id);
            let ws = self.workspace_root.clone();
            if let Err(e) = self.triggers.save(&ws) {
                Self::persist_err(&mut self.toasts, "triggers", &e);
            }
        }
        if let Some(id) = remove {
            if self.triggers.remove(&id) {
                let ws = self.workspace_root.clone();
                if let Err(e) = self.triggers.save(&ws) {
                    Self::persist_err(&mut self.toasts, "triggers", &e);
                }
                self.toasts
                    .push(crate::editor::toast::Toast::info("Trigger removed"));
            }
        }
        if let Some(id) = run_now {
            let action = self.triggers.get(&id).map(|t| t.action.clone());
            match action {
                Some(TriggerAction::AgentPrompt { prompt }) => {
                    let _ = self
                        .agent_tx
                        .send(crate::agent::UiToAgentMessage::UserPrompt(prompt));
                    self.triggers.mark_run(&id, now_secs());
                    let ws = self.workspace_root.clone();
                    if let Err(e) = self.triggers.save(&ws) {
                        Self::persist_err(&mut self.toasts, "triggers", &e);
                    }
                    self.toasts.push(crate::editor::toast::Toast::info(
                        "Trigger dispatched to agent",
                    ));
                }
                Some(TriggerAction::RunWorkflow { workflow_id }) => {
                    if let Some(wf) = self.workflow_state.workflows.get(&workflow_id).cloned() {
                        let ws = self.workspace_root.clone();
                        let run = wf.execute(&ws);
                        self.triggers.mark_run(&id, now_secs());
                        let _ = self.triggers.save(&ws);
                        self.toasts.push(crate::editor::toast::Toast::info(format!(
                            "Workflow '{}' \u{2192} {}",
                            wf.name,
                            run.status.label()
                        )));
                    } else {
                        self.toasts.push(crate::editor::toast::Toast::error(format!(
                            "Unknown workflow '{workflow_id}'"
                        )));
                    }
                }
                None => {}
            }
        }
    }
}

fn trigger_kind_label(kind: &crate::editor::triggers::TriggerKind) -> String {
    use crate::editor::triggers::TriggerKind;
    match kind {
        TriggerKind::Schedule { interval } => format!("schedule \u{00b7} {interval}"),
        TriggerKind::FileWatch { path, glob } => format!("file-watch \u{00b7} {path}/{glob}"),
        TriggerKind::Webhook { .. } => "webhook".to_string(),
        TriggerKind::Manual => "manual".to_string(),
    }
}

fn trigger_action_label(action: &crate::editor::triggers::TriggerAction) -> String {
    use crate::editor::triggers::TriggerAction;
    match action {
        TriggerAction::RunWorkflow { workflow_id } => format!("\u{2192} workflow {workflow_id}"),
        TriggerAction::AgentPrompt { prompt } => {
            let p: String = prompt.chars().take(60).collect();
            format!("\u{2192} agent: {p}")
        }
    }
}
