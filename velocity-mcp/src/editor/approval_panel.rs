//! Inline tool approval cards for non-blocking agent workflows
//! 
//! Replaces modal dialogs with inline rendered approval cards
//! that stay in the chat flow without interrupting the user.

use eframe::egui;
use crate::editor::theme::IdePalette;

/// Action the user can take on a pending tool
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalAction {
    Approve,
    Deny,
    Modify,
    Preview,
}

/// Represents a pending tool approval in the chat
#[derive(Clone, Debug)]
pub struct ToolApprovalCard {
    pub tool_name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub expected_outcome: String,
    pub estimated_cost: f32,
    pub estimated_duration_secs: u32,
    pub auto_approve: bool,
    pub show_details: bool,
}

impl ToolApprovalCard {
    pub fn new(
        tool_name: String,
        description: String,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            tool_name,
            description,
            parameters,
            expected_outcome: String::from("Process data and return results"),
            estimated_cost: 0.001,
            estimated_duration_secs: 5,
            auto_approve: false,
            show_details: false,
        }
    }

    pub fn with_outcome(mut self, outcome: String) -> Self {
        self.expected_outcome = outcome;
        self
    }

    pub fn with_cost(mut self, cost: f32) -> Self {
        self.estimated_cost = cost;
        self
    }

    pub fn with_duration(mut self, secs: u32) -> Self {
        self.estimated_duration_secs = secs;
        self
    }
}

/// Manages pending approvals in the chat
pub struct ApprovalManager {
    pub pending: Vec<ToolApprovalCard>,
    pub auto_approve_all: bool,
    pub preferences: std::collections::HashMap<String, bool>, // tool_name -> auto_approve
}

impl Default for ApprovalManager {
    fn default() -> Self {
        Self {
            pending: Vec::new(),
            auto_approve_all: false,
            preferences: std::collections::HashMap::new(),
        }
    }
}

impl ApprovalManager {
    pub fn add_approval(&mut self, card: ToolApprovalCard) {
        // Check preferences
        if let Some(&should_auto_approve) = self.preferences.get(&card.tool_name) {
            let mut card = card;
            card.auto_approve = should_auto_approve;
            self.pending.push(card);
        } else if self.auto_approve_all {
            let mut card = card;
            card.auto_approve = true;
            self.pending.push(card);
        } else {
            self.pending.push(card);
        }
    }

    pub fn remove_approval(&mut self, index: usize) {
        if index < self.pending.len() {
            self.pending.remove(index);
        }
    }

    pub fn set_tool_preference(&mut self, tool_name: String, auto_approve: bool) {
        self.preferences.insert(tool_name, auto_approve);
    }

    pub fn has_pending(&self) -> bool {
        self.pending.iter().any(|c| !c.auto_approve)
    }

    pub fn pending_count(&self) -> usize {
        self.pending.iter().filter(|c| !c.auto_approve).count()
    }
}

/// Render a single tool approval card inline in chat
pub fn render_approval_card(
    ui: &mut egui::Ui,
    card: &mut ToolApprovalCard,
    palette: IdePalette,
    _index: usize,
) -> Option<ApprovalAction> {
    let mut action: Option<ApprovalAction> = None;

    // Card container
    let frame = egui::Frame::new()
        .fill(palette.bg_tertiary)
        .stroke(egui::Stroke::new(1.5, palette.accent.gamma_multiply(0.5)))
        .inner_margin(12.0);

    frame.show(ui, |ui| {
        ui.vertical(|ui| {
            // Header with tool name and status
            ui.horizontal(|ui| {
                ui.colored_label(palette.accent, "⚙️");
                ui.label(
                    egui::RichText::new(format!("Pending Tool: {}", card.tool_name))
                        .size(12.0)
                        .strong()
                        .color(palette.text),
                );

                if card.auto_approve {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new("(auto-approved)")
                                .size(9.0)
                                .color(palette.success),
                        );
                    });
                }
            });

            ui.separator();

            // Description
            ui.horizontal(|ui| {
                ui.label("Description:");
                ui.label(
                    egui::RichText::new(&card.description)
                        .color(palette.text_muted)
                        .size(10.0),
                );
            });

            // Expected outcome
            ui.horizontal(|ui| {
                ui.label("Expected:");
                ui.label(
                    egui::RichText::new(&card.expected_outcome)
                        .color(palette.text_muted)
                        .size(10.0),
                );
            });

            // Key parameters (simplified view)
            ui.horizontal(|ui| {
                ui.label("Cost:");
                ui.colored_label(
                    palette.warning,
                    format!("${:.4}", card.estimated_cost),
                );
                ui.separator();
                ui.label("~Duration:");
                ui.label(format!("{}s", card.estimated_duration_secs));
            });

            // Details toggle
            ui.horizontal(|ui| {
                if ui.small_button(if card.show_details { "Hide" } else { "Show" }).clicked() {
                    card.show_details = !card.show_details;
                }
                ui.label("Details");
            });

            if card.show_details {
                ui.separator();
                ui.label(
                    egui::RichText::new("Parameters:")
                        .size(10.0)
                        .strong(),
                );
                ui.text_edit_multiline(&mut card.parameters.to_string());
            }

            // Action buttons
            ui.separator();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;

                if ui.button(
                    egui::RichText::new("✓ Approve")
                        .color(palette.success)
                        .size(11.0),
                ).clicked() {
                    action = Some(ApprovalAction::Approve);
                }

                if ui.button(
                    egui::RichText::new("✕ Deny")
                        .color(palette.error)
                        .size(11.0),
                ).clicked() {
                    action = Some(ApprovalAction::Deny);
                }

                if ui.button(
                    egui::RichText::new("✎ Modify")
                        .color(palette.accent)
                        .size(11.0),
                ).clicked() {
                    action = Some(ApprovalAction::Modify);
                }

                if ui.button(
                    egui::RichText::new("👁 Preview")
                        .color(palette.text_muted)
                        .size(11.0),
                ).clicked() {
                    action = Some(ApprovalAction::Preview);
                }
            });

            // Per-tool auto-approve toggle
            ui.checkbox(&mut card.auto_approve, "Auto-approve this tool in future");
        });
    });

    action
}

/// Render all pending approvals in a compact mode (for chat panel)
pub fn render_pending_approvals(
    ui: &mut egui::Ui,
    manager: &mut ApprovalManager,
    palette: IdePalette,
) -> Vec<(usize, ApprovalAction)> {
    let mut actions: Vec<(usize, ApprovalAction)> = Vec::new();

    if manager.pending.is_empty() {
        return actions;
    }

    ui.group(|ui| {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "⏳ {} Pending Approval{}",
                    manager.pending_count(),
                    if manager.pending_count() == 1 { "" } else { "s" }
                ))
                .size(11.0)
                .strong()
                .color(palette.warning),
            );

            for (idx, card) in manager.pending.iter_mut().enumerate() {
                if let Some(action) = render_approval_card(ui, card, palette, idx) {
                    actions.push((idx, action));
                }
            }
        });
    });

    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_approval_card_creation() {
        let card = ToolApprovalCard::new(
            "browser.navigate".into(),
            "Navigate to website".into(),
            serde_json::json!({"url": "https://example.com"}),
        );

        assert_eq!(card.tool_name, "browser.navigate");
        assert!(!card.auto_approve);
    }

    #[test]
    fn test_approval_manager_preferences() {
        let mut manager = ApprovalManager::default();
        manager.set_tool_preference("browser.navigate".into(), true);

        let card = ToolApprovalCard::new(
            "browser.navigate".into(),
            "Test".into(),
            serde_json::json!({}),
        );
        manager.add_approval(card);

        assert_eq!(manager.pending.len(), 1);
        assert!(manager.pending[0].auto_approve);
    }
}
