//! Enhanced status bar with real-time agent metrics
//! 
//! Displays:
//! - Agent state (Idle/Running/Thinking)
//! - Token usage (current / max)
//! - Estimated cost
//! - LLM provider and model
//! - Thinking status

use eframe::egui;
use crate::editor::theme::IdePalette;

/// Current state of the agent
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentState {
    Idle,
    Running,
    Thinking,
    Waiting,
}

impl AgentState {
    pub fn icon(self) -> &'static str {
        match self {
            Self::Idle => "⊘",
            Self::Running => "▶",
            Self::Thinking => "🧠",
            Self::Waiting => "⏸",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Running => "Running",
            Self::Thinking => "Thinking",
            Self::Waiting => "Waiting",
        }
    }

    pub fn color(self, palette: IdePalette) -> egui::Color32 {
        match self {
            Self::Idle => palette.text_muted,
            Self::Running => palette.success,
            Self::Thinking => palette.accent,
            Self::Waiting => palette.warning,
        }
    }
}

/// Real-time agent metrics
#[derive(Clone, Debug)]
pub struct AgentMetrics {
    pub state: AgentState,
    pub tokens_used: u32,
    pub tokens_max: u32,
    pub estimated_cost: f32,
    pub estimated_cost_max: f32,
    pub provider: String,
    pub model: String,
    pub thinking_enabled: bool,
    pub tool_call_count: u32,
    pub last_tool_duration_ms: u32,
}

impl Default for AgentMetrics {
    fn default() -> Self {
        Self {
            state: AgentState::Idle,
            tokens_used: 0,
            tokens_max: 10000,
            estimated_cost: 0.0,
            estimated_cost_max: 0.5,
            provider: "Cloudflare".into(),
            model: "kimi-k2.7-code".into(),
            thinking_enabled: false,
            tool_call_count: 0,
            last_tool_duration_ms: 0,
        }
    }
}

impl AgentMetrics {
    /// Calculate token budget percentage (0-100)
    pub fn budget_percentage(&self) -> f32 {
        if self.tokens_max == 0 {
            0.0
        } else {
            (self.tokens_used as f32 / self.tokens_max as f32) * 100.0
        }
    }

    /// Calculate cost budget percentage (0-100)
    pub fn cost_percentage(&self) -> f32 {
        if self.estimated_cost_max == 0.0 {
            0.0
        } else {
            (self.estimated_cost / self.estimated_cost_max) * 100.0
        }
    }

    /// Get warning level based on budget usage
    pub fn budget_warning_level(&self) -> WarningLevel {
        let pct = self.budget_percentage();
        if pct >= 90.0 {
            WarningLevel::Critical
        } else if pct >= 75.0 {
            WarningLevel::Warning
        } else if pct >= 50.0 {
            WarningLevel::Caution
        } else {
            WarningLevel::Ok
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WarningLevel {
    Ok,
    Caution,
    Warning,
    Critical,
}

impl WarningLevel {
    pub fn color(self, palette: IdePalette) -> egui::Color32 {
        match self {
            Self::Ok => palette.success,
            Self::Caution => egui::Color32::from_rgb(250, 204, 21),  // Yellow
            Self::Warning => egui::Color32::from_rgb(248, 113, 113), // Red-ish
            Self::Critical => palette.error,
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Ok => "✓",
            Self::Caution => "⚠",
            Self::Warning => "⚠",
            Self::Critical => "✕",
        }
    }
}

/// Render the agent status bar
pub fn render_agent_status_bar(
    ui: &mut egui::Ui,
    metrics: &AgentMetrics,
    palette: IdePalette,
) {
    egui::Panel::bottom("agent_status_bar")
        .frame(egui::Frame::new().fill(palette.bg_secondary).inner_margin(6.0))
        .show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;

                // Agent State
                ui.colored_label(
                    metrics.state.color(palette),
                    format!("{} {}", metrics.state.icon(), metrics.state.label()),
                );

                ui.separator();

                // LLM Provider & Model
                ui.label(
                    egui::RichText::new(format!(
                        "Model: {} | {}",
                        metrics.model, metrics.provider
                    ))
                    .size(10.0)
                    .color(palette.text_muted),
                );

                ui.separator();

                // Thinking Status
                if metrics.thinking_enabled {
                    ui.colored_label(
                        palette.accent,
                        "🧠 Thinking: ON",
                    );
                } else {
                    ui.label(
                        egui::RichText::new("🧠 Thinking: OFF")
                            .size(10.0)
                            .color(palette.text_muted),
                    );
                }

                ui.separator();

                // Token Budget
                let token_warning = metrics.budget_warning_level();
                let token_pct = metrics.budget_percentage();
                ui.colored_label(
                    token_warning.color(palette),
                    format!(
                        "{} Tokens: {}/{} ({:.0}%)",
                        token_warning.icon(),
                        metrics.tokens_used,
                        metrics.tokens_max,
                        token_pct
                    ),
                );

                // Token progress bar
                let progress = token_pct / 100.0;
                let progress_color = token_warning.color(palette);
                ui.add(
                    egui::ProgressBar::new(progress.min(1.0))
                        .text(format!("{:.0}%", token_pct))
                        .fill(progress_color)
                        .desired_width(80.0),
                );

                ui.separator();

                // Cost
                let cost_warning = metrics.budget_warning_level();
                let cost_pct = metrics.cost_percentage();
                ui.colored_label(
                    cost_warning.color(palette),
                    format!(
                        "Cost: ${:.4} / ${:.2} ({:.0}%)",
                        metrics.estimated_cost,
                        metrics.estimated_cost_max,
                        cost_pct
                    ),
                );

                ui.separator();

                // Tool statistics
                ui.label(
                    egui::RichText::new(format!(
                        "Tools: {} | Last: {}ms",
                        metrics.tool_call_count, metrics.last_tool_duration_ms
                    ))
                    .size(10.0)
                    .color(palette.text_muted),
                );

                // Spacer
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new("Agent Ready")
                            .size(10.0)
                            .color(palette.success),
                    );
                });
            });
        });
}

/// Compact version of agent status (for top bar)
pub fn render_agent_status_compact(
    ui: &mut egui::Ui,
    metrics: &AgentMetrics,
    palette: IdePalette,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;

        // State icon
        ui.colored_label(
            metrics.state.color(palette),
            format!("{}", metrics.state.icon()),
        );

        // Token meter
        let token_warning = metrics.budget_warning_level();
        ui.colored_label(
            token_warning.color(palette),
            format!("{:.0}%", metrics.budget_percentage()),
        );

        // Cost
        ui.label(
            egui::RichText::new(format!("${:.4}", metrics.estimated_cost))
                .size(9.0)
                .color(palette.text_muted),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_metrics_budget_calculation() {
        let mut metrics = AgentMetrics::default();
        metrics.tokens_used = 5000;
        metrics.tokens_max = 10000;

        assert_eq!(metrics.budget_percentage(), 50.0);
        assert_eq!(metrics.budget_warning_level(), WarningLevel::Caution);
    }

    #[test]
    fn test_warning_levels() {
        let mut metrics = AgentMetrics::default();

        metrics.tokens_used = 100;
        assert_eq!(metrics.budget_warning_level(), WarningLevel::Ok);

        metrics.tokens_used = 5000;
        assert_eq!(metrics.budget_warning_level(), WarningLevel::Caution);

        metrics.tokens_used = 8000;
        assert_eq!(metrics.budget_warning_level(), WarningLevel::Warning);

        metrics.tokens_used = 9500;
        assert_eq!(metrics.budget_warning_level(), WarningLevel::Critical);
    }
}
