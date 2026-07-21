//! Rendering layer for zero-allocation agentic UI state.
//!
//! This module renders the thinking panel, approvals, and metrics
//! using immutable snapshots from AgentUiState. No mutations occur during render.

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

/// Render thinking panel (immutable render)
pub fn render_thinking_panel(ui: &mut egui::Ui, snapshot: &RenderSnapshot, palette: (u8, u8, u8)) {
    let frame = egui::Frame::new()
        .fill(egui::Color32::from_rgb(25, 27, 39))
        .inner_margin(8.0)
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(33, 36, 51)));

    frame.show(ui, |ui| {
        // Header
        ui.horizontal(|ui| {
            let status = if snapshot.state.thinking.current_phase.is_some() {
                "🧠 Thinking Thread ⟳"
            } else {
                "🧠 Thinking Thread"
            };
            ui.label(
                egui::RichText::new(status)
                    .size(12.0)
                    .strong()
                    .color(egui::Color32::from_rgb(226, 227, 243)),
            );
        });

        if snapshot.state.thinking.expanded {
            ui.separator();

            // Render thinking steps (read-only from ring buffer)
            egui::ScrollArea::vertical()
                .max_height(180.0)
                .show(ui, |ui| {
                    if snapshot.state.thinking.step_count() == 0 {
                        ui.label(
                            egui::RichText::new("Waiting for agent thinking...")
                                .size(10.0)
                                .color(egui::Color32::from_rgb(125, 131, 166)),
                        );
                    } else {
                        // Iterate over ring buffer in order (zero-copy)
                        for (_, entry) in snapshot.state.thinking.visible_steps() {
                            let phase_icon = match entry.phase {
                                ThinkingPhase::Analysis => "🔍",
                                ThinkingPhase::Planning => "📋",
                                ThinkingPhase::Execution => "⚡",
                                ThinkingPhase::Verification => "✓",
                            };

                            let status_icon = if entry.completed { "✓" } else { "→" };
                            let phase_name = match entry.phase {
                                ThinkingPhase::Analysis => "Analyzing",
                                ThinkingPhase::Planning => "Planning",
                                ThinkingPhase::Execution => "Executing",
                                ThinkingPhase::Verification => "Verifying",
                            };

                            ui.horizontal(|ui| {
                                ui.label(status_icon);
                                ui.label(phase_icon);
                                ui.label(
                                    egui::RichText::new(phase_name)
                                        .size(10.0)
                                        .color(egui::Color32::from_rgb(168, 85, 247)),
                                );
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{:.1}s",
                                        entry.timestamp_ms as f32 / 1000.0
                                    ))
                                    .size(9.0)
                                    .color(egui::Color32::from_rgb(125, 131, 166)),
                                );
                            });

                            // Get and display step text (zero-copy via text pool)
                            let text = snapshot
                                .state
                                .thinking
                                .get_step_text(entry.text_offset as usize);
                            if !text.is_empty() {
                                ui.label(
                                    egui::RichText::new(text)
                                        .size(9.0)
                                        .color(egui::Color32::from_rgb(226, 227, 243)),
                                );
                            }
                            ui.add_space(4.0);
                        }
                    }
                });
        }
    });
}

/// Render pending approvals (immutable render)
pub fn render_pending_approvals(ui: &mut egui::Ui, snapshot: &RenderSnapshot) {
    let pending_count = snapshot.state.approvals.pending_count();
    if pending_count == 0 {
        return;
    }

    ui.group(|ui| {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "⏳ {} Pending Approval{}",
                    pending_count,
                    if pending_count == 1 { "" } else { "s" }
                ))
                .size(11.0)
                .strong()
                .color(egui::Color32::from_rgb(250, 204, 21)),
            );

            // Render approval cards (read-only from ring buffer)
            for i in 0..snapshot.state.approvals.total_count() {
                let tool_name = snapshot.state.approvals.get_tool_name(i);
                if tool_name.is_empty() {
                    continue;
                }

                ui.horizontal(|ui| {
                    ui.label("⚙️");
                    ui.label(
                        egui::RichText::new(format!("Tool: {}", tool_name))
                            .size(10.0)
                            .color(egui::Color32::from_rgb(226, 227, 243)),
                    );
                });
            }
        });
    });
}

/// Render agent metrics bar (immutable render)
pub fn render_agent_metrics(ui: &mut egui::Ui, snapshot: &RenderSnapshot) {
    let metrics = &snapshot.state.metrics;

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;

        // State icon
        let state_icon = match metrics.state {
            AgentState::Idle => "⊘",
            AgentState::Running => "▶",
            AgentState::Thinking => "🧠",
            AgentState::Waiting => "⏸",
        };
        ui.label(state_icon);

        ui.separator();

        // Token budget
        let budget_pct = metrics.budget_percentage();
        let warning = metrics.warning_level();
        let warning_icon = match warning {
            WarningLevel::Ok => "✓",
            WarningLevel::Caution => "⚠",
            WarningLevel::Warning => "⚠",
            WarningLevel::Critical => "✕",
        };

        ui.label(
            egui::RichText::new(format!(
                "{} Tokens: {}/{} ({:.0}%)",
                warning_icon, metrics.tokens_used, metrics.tokens_max, budget_pct as f32
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
                    egui::RichText::new("🧠 Thinking: ON")
                        .size(10.0)
                        .color(egui::Color32::from_rgb(168, 85, 247)),
                );
            } else {
                ui.label(
                    egui::RichText::new("🧠 Thinking: OFF")
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
