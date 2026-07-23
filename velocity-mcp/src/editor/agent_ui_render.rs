//! Rendering layer for zero-allocation agentic UI state.
//!
//! This module renders agent metrics using immutable snapshots
//! from AgentUiState. No mutations occur during render.

use crate::editor::agent_ui_state::*;
use eframe::egui;

/// Immutable snapshot for rendering (zero-copy reference)
pub struct RenderSnapshot<'a> {
    pub state: &'a AgentUiState,
}

impl<'a> RenderSnapshot<'a> {
    pub fn new(state: &'a AgentUiState) -> Self {
        Self { state }
    }
}

/// Render agent metrics bar (immutable render)
pub fn render_agent_metrics(ui: &mut egui::Ui, snapshot: &RenderSnapshot) {
    let metrics = &snapshot.state.metrics;

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;

        // State icon
        let state_icon = match metrics.state {
            AgentState::Idle => "Idle",
            AgentState::Running => "Running",
            AgentState::Thinking => "Thinking",
            AgentState::Waiting => "Waiting",
        };
        ui.label(
            egui::RichText::new(state_icon)
                .size(10.0)
                .strong(),
        );

        ui.separator();

        // Token budget
        let budget_pct = metrics.budget_percentage();
        ui.label(
            egui::RichText::new(format!(
                "Tokens: {}/{} ({:.0}%)",
                metrics.tokens_used, metrics.tokens_max, budget_pct as f32
            ))
            .size(10.0),
        );

        // Progress bar
        ui.add(egui::ProgressBar::new((budget_pct as f32) / 100.0).desired_width(80.0));

        ui.separator();

        // Cost
        let cost_usd = metrics.estimated_cost as f32 * 0.0001;
        ui.label(egui::RichText::new(format!("Cost: ${:.4}", cost_usd)).size(10.0));

        ui.separator();

        // Tool stats
        ui.label(
            egui::RichText::new(format!(
                "Tools: {} | Last: {}ms",
                metrics.tool_call_count, metrics.last_tool_duration_ms
            ))
            .size(10.0),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if metrics.thinking_enabled {
                ui.label(
                    egui::RichText::new("Thinking: ON")
                        .size(10.0)
                        .color(egui::Color32::from_rgb(168, 85, 247)),
                );
            } else {
                ui.label(
                    egui::RichText::new("Thinking: OFF")
                        .size(10.0)
                        .color(egui::Color32::from_rgb(125, 131, 166)),
                );
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_snapshot_creation() {
        let state = AgentUiState::default();
        let snapshot = RenderSnapshot::new(&state);
        assert_eq!(snapshot.state.metrics.state, AgentState::Idle);
    }
}
