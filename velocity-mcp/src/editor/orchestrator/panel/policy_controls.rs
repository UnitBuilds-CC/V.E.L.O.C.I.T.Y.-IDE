use super::struct_def::OrchestratorPanel;
use crate::automation::{AgentTaskKind, DecompositionStyle, InstructionRegistry};
use crate::editor::theme::IdePalette;
use eframe::egui;
use egui::Ui;
use std::path::Path;

impl OrchestratorPanel {
    pub fn set_selected_policy_kind(&mut self, kind: AgentTaskKind) {
        if self.policy_editor.kind != kind {
            self.policy_editor.kind = kind;
            self.policy_editor.loaded_policy_id.clear();
        }
    }

    pub fn ensure_policy_editor_loaded(&mut self, workspace_root: &Path) {
        let registry = InstructionRegistry::open(workspace_root);
        let policies = registry.policies_for_kind(self.policy_editor.kind);
        let desired_policy_id = registry
            .policy_for_kind(self.policy_editor.kind)
            .or_else(|| policies.first().copied())
            .map(|policy| policy.id.clone())
            .unwrap_or_default();

        if self.policy_editor.selected_policy_id.is_empty() {
            self.policy_editor.selected_policy_id = desired_policy_id.clone();
        }
        let load_policy_id = if registry
            .get_policy(&self.policy_editor.selected_policy_id)
            .filter(|policy| policy.task_kind == self.policy_editor.kind)
            .is_some()
        {
            self.policy_editor.selected_policy_id.clone()
        } else {
            desired_policy_id
        };

        if self.policy_editor.loaded_policy_id == load_policy_id {
            return;
        }

        if let Some(policy) = registry.get_policy(&load_policy_id) {
            self.policy_editor.selected_policy_id = policy.id.clone();
            self.policy_editor.loaded_policy_id = policy.id.clone();
            self.policy_editor.draft_label = policy.label.clone();
            self.policy_editor.draft_template_id = policy.instruction_template_id.clone();
            self.policy_editor.draft_style = policy.decomposition_style;
            self.policy_editor.draft_expectations = policy.shared_expectations.join("\n");
            self.policy_editor.status = format!(
                "Editing policy '{}' for {}.",
                policy.label,
                self.policy_editor.kind.as_str()
            );
        }
    }

    pub fn render_policy_controls(
        &mut self,
        ui: &mut Ui,
        workspace_root: &Path,
        palette: IdePalette,
    ) {
        let registry = InstructionRegistry::open(workspace_root);
        let kind = self.policy_editor.kind;
        let policies = registry.policies_for_kind(kind);
        let templates = registry.templates_for_kind(kind);

        ui.collapsing("⚙ Routing policy controls & editor", |ui: &mut egui::Ui| {
            ui.label(
                egui::RichText::new(&self.policy_editor.status)
                    .small()
                    .color(palette.text_muted),
            );

            egui::ComboBox::from_label("Task kind")
                .selected_text(kind.as_str())
                .show_ui(ui, |ui| {
                    for candidate in AgentTaskKind::ALL {
                        let selected = self.policy_editor.kind == candidate;
                        if ui.selectable_label(selected, candidate.as_str()).clicked() {
                            self.policy_editor.kind = candidate;
                            self.policy_editor.loaded_policy_id.clear();
                        }
                    }
                });

            let selected_policy_text = if self.policy_editor.selected_policy_id.is_empty() {
                "No policy".to_string()
            } else {
                self.policy_editor.selected_policy_id.clone()
            };
            egui::ComboBox::from_label("Preferred policy")
                .selected_text(selected_policy_text)
                .show_ui(ui, |ui| {
                    for policy in &policies {
                        let selected = self.policy_editor.selected_policy_id == policy.id;
                        if ui.selectable_label(selected, format!("{} ({})", policy.label, policy.id)).clicked() {
                            self.policy_editor.selected_policy_id = policy.id.clone();
                            self.policy_editor.loaded_policy_id.clear();
                        }
                    }
                });

            ui.horizontal(|ui| {
                if ui.button("Save preferred policy").clicked() {
                    let mut writable = InstructionRegistry::open(workspace_root);
                    writable.set_preferred_policy(self.policy_editor.kind, self.policy_editor.selected_policy_id.clone());
                    match writable.persist() {
                        Ok(()) => {
                            self.policy_editor.status = format!(
                                "Preferred policy for {} saved as '{}'.",
                                self.policy_editor.kind.as_str(),
                                self.policy_editor.selected_policy_id
                            );
                        }
                        Err(err) => {
                            self.policy_editor.status = format!("Failed to save preferred policy: {err}");
                        }
                    }
                }
                if ui.button("Reload policy").clicked() {
                    self.policy_editor.loaded_policy_id.clear();
                    self.ensure_policy_editor_loaded(workspace_root);
                }
            });

            ui.separator();
            ui.label(egui::RichText::new("Policy details").small().strong());
            ui.horizontal(|ui| {
                ui.label("Label:");
                ui.text_edit_singleline(&mut self.policy_editor.draft_label);
            });
            ui.horizontal(|ui| {
                ui.label("Template:");
                let selected_template = if self.policy_editor.draft_template_id.is_empty() {
                    "No template".to_string()
                } else {
                    self.policy_editor.draft_template_id.clone()
                };
                egui::ComboBox::from_id_salt("policy-template-select")
                    .selected_text(selected_template)
                    .show_ui(ui, |ui: &mut egui::Ui| {
                        for template in &templates {
                            let selected = self.policy_editor.draft_template_id == template.id;
                            if ui.selectable_label(selected, format!("{} ({})", template.label, template.id)).clicked() {
                                self.policy_editor.draft_template_id = template.id.clone();
                            }
                        }
                    });
            });
            ui.horizontal(|ui| {
                ui.label("Style:");
                egui::ComboBox::from_id_salt("policy-style-select")
                    .selected_text(self.policy_editor.draft_style.as_str())
                    .show_ui(ui, |ui: &mut egui::Ui| {
                        for style in DecompositionStyle::ALL {
                            let selected = self.policy_editor.draft_style == style;
                            if ui.selectable_label(selected, style.as_str()).clicked() {
                                self.policy_editor.draft_style = style;
                            }
                        }
                    });
            });
            ui.label("Shared expectations (one per line):");
            ui.add(
                egui::TextEdit::multiline(&mut self.policy_editor.draft_expectations)
                    .desired_rows(4)
                    .desired_width(f32::INFINITY),
            );

            if ui.button("Persist policy edits").clicked() {
                let mut writable = InstructionRegistry::open(workspace_root);
                let mut policy = match writable.get_policy(&self.policy_editor.selected_policy_id).cloned() {
                    Some(policy) => policy,
                    None => {
                        self.policy_editor.status = "Select a valid policy before persisting edits.".to_string();
                        return;
                    }
                };
                policy.label = self.policy_editor.draft_label.trim().to_string();
                policy.instruction_template_id = self.policy_editor.draft_template_id.trim().to_string();
                policy.decomposition_style = self.policy_editor.draft_style;
                policy.shared_expectations = self
                    .policy_editor
                    .draft_expectations
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(ToOwned::to_owned)
                    .collect();
                writable.upsert_policy(policy);
                writable.set_preferred_policy(self.policy_editor.kind, self.policy_editor.selected_policy_id.clone());
                match writable.persist() {
                    Ok(()) => {
                        self.policy_editor.loaded_policy_id.clear();
                        self.ensure_policy_editor_loaded(workspace_root);
                        self.policy_editor.status = format!(
                            "Persisted policy '{}' for {}. Re-run routed planning to apply changes.",
                            self.policy_editor.selected_policy_id,
                            self.policy_editor.kind.as_str()
                        );
                    }
                    Err(err) => {
                        self.policy_editor.status = format!("Failed to persist policy edits: {err}");
                    }
                }
            }
        });
    }
}
