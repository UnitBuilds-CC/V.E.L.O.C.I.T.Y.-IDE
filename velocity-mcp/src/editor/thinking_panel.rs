//! Real-time thinking visualization panel for agentic workflows
//! 
//! Displays agent reasoning in real-time as it processes:
//! - Analysis phase (blue)
//! - Planning phase (yellow)
//! - Execution phase (green)
//! - Verification phase (cyan)

use eframe::egui;
use crate::editor::theme::IdePalette;

/// Represents different phases of agent thinking
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThinkingPhase {
    Analysis,
    Planning,
    Execution,
    Verification,
}

impl ThinkingPhase {
    pub fn color(self, palette: IdePalette) -> egui::Color32 {
        match self {
            Self::Analysis => egui::Color32::from_rgb(59, 130, 246),      // Blue
            Self::Planning => egui::Color32::from_rgb(168, 85, 247),      // Purple
            Self::Execution => egui::Color32::from_rgb(74, 222, 128),     // Green
            Self::Verification => egui::Color32::from_rgb(34, 211, 238),  // Cyan
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Analysis => "🔍",
            Self::Planning => "📋",
            Self::Execution => "⚡",
            Self::Verification => "✓",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Analysis => "Analyzing",
            Self::Planning => "Planning",
            Self::Execution => "Executing",
            Self::Verification => "Verifying",
        }
    }
}

/// Single step in the thinking process
#[derive(Clone, Debug)]
pub struct ThinkingStep {
    pub phase: ThinkingPhase,
    pub content: String,
    pub timestamp: std::time::Instant,
    pub completed: bool,
}

impl ThinkingStep {
    pub fn new(phase: ThinkingPhase, content: String) -> Self {
        Self {
            phase,
            content,
            timestamp: std::time::Instant::now(),
            completed: false,
        }
    }
}

/// Manages the thinking visualization state
pub struct ThinkingPanel {
    pub steps: Vec<ThinkingStep>,
    pub current_phase: Option<ThinkingPhase>,
    pub expanded: bool,
    pub auto_collapse: bool,
    pub current_step_content: String,
}

impl Default for ThinkingPanel {
    fn default() -> Self {
        Self {
            steps: Vec::new(),
            current_phase: None,
            expanded: true,
            auto_collapse: true,
            current_step_content: String::new(),
        }
    }
}

impl ThinkingPanel {
    pub fn add_step(&mut self, phase: ThinkingPhase, content: String) {
        self.steps.push(ThinkingStep::new(phase, content));
        self.current_phase = Some(phase);
    }

    pub fn append_to_current(&mut self, token: &str) {
        self.current_step_content.push_str(token);
        if let Some(last) = self.steps.last_mut() {
            last.content.push_str(token);
        }
    }

    pub fn complete_phase(&mut self) {
        if let Some(last) = self.steps.last_mut() {
            last.completed = true;
        }
    }

    pub fn clear(&mut self) {
        self.steps.clear();
        self.current_phase = None;
        self.current_step_content.clear();
    }

    pub fn is_active(&self) -> bool {
        self.current_phase.is_some() && !self.steps.is_empty()
    }
}

/// Render the thinking panel with real-time visualization
pub fn render_thinking_panel(
    ui: &mut egui::Ui,
    panel: &mut ThinkingPanel,
    palette: IdePalette,
) {
    let frame = egui::Frame::new()
        .fill(palette.bg_secondary)
        .inner_margin(8.0)
        .stroke(egui::Stroke::new(1.0, palette.border));

    frame.show(ui, |ui| {
        // Header with toggle
        ui.horizontal(|ui| {
            if ui.selectable_label(panel.expanded, "🧠 Thinking Thread").clicked() {
                panel.expanded = !panel.expanded;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let spinner = if panel.is_active() { "⟳ " } else { "" };
                ui.label(
                    egui::RichText::new(format!("{}Active", spinner))
                        .size(10.0)
                        .color(if panel.is_active() { palette.success } else { palette.text_muted }),
                );
            });
        });

        if panel.expanded {
            ui.separator();

            if panel.steps.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.label(
                        egui::RichText::new("Waiting for agent thinking...")
                            .size(11.0)
                            .color(palette.text_muted),
                    );
                });
            } else {
                // Scrollable thinking steps
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        for (idx, step) in panel.steps.iter().enumerate() {
                            render_thinking_step(ui, idx, step, palette);
                        }

                        // Current incomplete step
                        if let Some(phase) = panel.current_phase {
                            if !panel.steps.last().map(|s| s.completed).unwrap_or(false) {
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    let color = phase.color(palette);
                                    ui.colored_label(color, format!("{} {}", phase.icon(), phase.label()));
                                    ui.label(
                                        egui::RichText::new("(in progress)")
                                            .size(9.0)
                                            .color(palette.text_muted),
                                    );
                                });
                                ui.text_edit_multiline(&mut panel.current_step_content.as_str());
                            }
                        }
                    });
            }

            // Action buttons
            ui.separator();
            ui.horizontal(|ui| {
                if ui.small_button("🔗 Show Full Trace").clicked() {
                    // TODO: Open full thinking trace window
                }
                if ui.small_button("📋 Copy Output").clicked() {
                    // TODO: Copy thinking trace to clipboard
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.checkbox(&mut panel.auto_collapse, "Auto-collapse when done");
                });
            });
        }
    });
}

/// Render a single thinking step
fn render_thinking_step(
    ui: &mut egui::Ui,
    _idx: usize,
    step: &ThinkingStep,
    palette: IdePalette,
) {
    let elapsed = step.timestamp.elapsed().as_secs_f32();
    let status_icon = if step.completed { "✓" } else { "→" };
    let status_color = if step.completed { palette.success } else { palette.accent };

    ui.horizontal(|ui| {
        // Status indicator
        ui.colored_label(status_color, status_icon);

        // Phase badge
        ui.colored_label(step.phase.color(palette), step.phase.icon());
        ui.label(
            egui::RichText::new(step.phase.label())
                .size(10.0)
                .color(step.phase.color(palette)),
        );

        // Time
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("{:.1}s", elapsed))
                    .size(9.0)
                    .color(palette.text_muted),
            );
        });
    });

    // Content (truncated if too long)
    let truncated = if step.content.len() > 100 {
        format!("{}...", &step.content[..100])
    } else {
        step.content.clone()
    };

    ui.label(
        egui::RichText::new(truncated)
            .size(9.0)
            .color(palette.text),
    );

    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thinking_panel_initialization() {
        let panel = ThinkingPanel::default();
        assert!(panel.steps.is_empty());
        assert!(panel.current_phase.is_none());
        assert!(!panel.is_active());
    }

    #[test]
    fn test_thinking_step_addition() {
        let mut panel = ThinkingPanel::default();
        panel.add_step(ThinkingPhase::Analysis, "Analyzing request".into());

        assert_eq!(panel.steps.len(), 1);
        assert_eq!(panel.current_phase, Some(ThinkingPhase::Analysis));
        assert!(panel.is_active());
    }
}
