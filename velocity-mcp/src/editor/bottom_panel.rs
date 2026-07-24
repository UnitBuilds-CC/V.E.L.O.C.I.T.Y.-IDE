#![allow(dead_code)]

//! Bottom Panel - Mode-specific bottom panel layouts and renderers.
//!
//! The bottom panel can be Tabbed (multiple tabs), Split (two panes), or a
//! Dashboard (full-width agent grid). Each mode declares which variant it uses
//! via its `ModeConfig::bottom_layout()`.

use crate::editor::theme::IdePalette;
use eframe::egui;

// ═══════════════════════════════════════════════════════════════════════════
// BottomPanelLayout
// ═══════════════════════════════════════════════════════════════════════════

/// Layout variants for the bottom panel region.
#[derive(Clone, Debug)]
pub enum BottomPanelLayout {
    /// Multiple tabs rendered as a tab strip.
    Tabbed(Vec<&'static str>),
    /// Left/right split with named panes.
    Split {
        left: &'static str,
        right: &'static str,
    },
    /// Full-width dashboard (agent grid with status cards).
    Dashboard,
}

// ═══════════════════════════════════════════════════════════════════════════
// Bottom Panel State
// ═══════════════════════════════════════════════════════════════════════════

/// Tracks the active tab index for Tabbed layouts and split ratios.
pub struct BottomPanelState {
    pub active_tab: usize,
    pub split_ratio: f32,
    pub panel_height: f32,
    pub collapsed: bool,
}

impl Default for BottomPanelState {
    fn default() -> Self {
        Self {
            active_tab: 0,
            split_ratio: 0.55,
            panel_height: 240.0,
            collapsed: false,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Bottom Panel Renderer
// ═══════════════════════════════════════════════════════════════════════════

/// Render the bottom panel according to the layout configuration.
/// Returns true if the panel is expanded (visible).
pub fn render_bottom_panel_frame(
    ui: &mut egui::Ui,
    layout: &BottomPanelLayout,
    state: &mut BottomPanelState,
    palette: IdePalette,
) -> bool {
    if state.collapsed {
        ui.horizontal(|ui| {
            let expand_btn = ui.small_button(
                egui::RichText::new("▲ Panel").size(9.0).color(palette.text_muted),
            );
            if expand_btn.clicked() {
                state.collapsed = false;
            }
        });
        return false;
    }

    match layout {
        BottomPanelLayout::Tabbed(tabs) => {
            render_tabbed_bottom(ui, tabs, state, palette);
        }
        BottomPanelLayout::Split { left, right } => {
            render_split_bottom(ui, left, right, state, palette);
        }
        BottomPanelLayout::Dashboard => {
            render_dashboard_bottom(ui, state, palette);
        }
    }
    true
}

fn render_tabbed_bottom(
    ui: &mut egui::Ui,
    tabs: &[&'static str],
    state: &mut BottomPanelState,
    palette: IdePalette,
) {
    // Tab strip
    ui.horizontal(|ui| {
        for (i, tab_label) in tabs.iter().enumerate() {
            let is_active = i == state.active_tab;
            let text = egui::RichText::new(*tab_label)
                .size(10.0)
                .color(if is_active { palette.accent } else { palette.text_muted });
            if ui.selectable_label(is_active, text).clicked() {
                state.active_tab = i;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(egui::RichText::new("▼").size(9.0).color(palette.text_muted)).clicked() {
                state.collapsed = true;
            }
        });
    });
    ui.separator();

    // Content area - render based on the active tab label
    let active_label = tabs.get(state.active_tab).copied().unwrap_or("Unknown");
    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
        match active_label {
            "Terminal" => {
                ui.label(egui::RichText::new("$ ").monospace().size(10.0).color(palette.accent));
                ui.label(egui::RichText::new("Terminal session ready. Output will appear here.").monospace().size(9.0).color(palette.text_muted));
            }
            "Problems" => {
                ui.label(egui::RichText::new("No problems detected in workspace.").size(9.0).color(palette.success));
            }
            "Output" => {
                ui.label(egui::RichText::new("Build/run output will appear here.").size(9.0).color(palette.text_muted));
            }
            "Chat" => {
                ui.label(egui::RichText::new("Agent chat — use the main Chat panel for interaction.").size(9.0).color(palette.text_muted));
            }
            "Audit Results" => {
                ui.label(egui::RichText::new("Run an accessibility audit to see results.").size(9.0).color(palette.text_muted));
            }
            "Keyboard Nav Map" => {
                ui.label(egui::RichText::new("Tab order and focus trap analysis.").size(9.0).color(palette.text_muted));
                ui.label(egui::RichText::new("Navigate the page with Tab to build the map.").size(9.0).color(palette.text_muted));
            }
            _ => {
                ui.label(egui::RichText::new(format!("[{}]", active_label)).size(9.0).color(palette.text_muted));
            }
        }
    });
}

/// Data passed into the split bottom panel for real content.
pub struct SplitPanelData<'a> {
    pub left_content: &'a str,
    pub right_content: &'a str,
}

fn render_split_bottom(
    ui: &mut egui::Ui,
    left_label: &str,
    right_label: &str,
    state: &mut BottomPanelState,
    palette: IdePalette,
) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("{} │ {}", left_label, right_label)).size(10.0).color(palette.text_muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(egui::RichText::new("▼").size(9.0).color(palette.text_muted)).clicked() {
                state.collapsed = true;
            }
        });
    });
    ui.separator();

    let available = ui.available_width();
    let left_width = available * state.split_ratio;

    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::Vec2::new(left_width, ui.available_height()),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                ui.label(egui::RichText::new(left_label).size(10.0).strong().color(palette.accent));
                ui.add_space(4.0);
                egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                    ui.label(egui::RichText::new("Live action preview — actions will appear as they execute.")
                        .monospace().size(9.0).color(palette.text_muted));
                });
            },
        );
        ui.separator();
        ui.allocate_ui_with_layout(
            egui::Vec2::new(available - left_width - 8.0, ui.available_height()),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                ui.label(egui::RichText::new(right_label).size(10.0).strong().color(palette.accent));
                ui.add_space(4.0);
                egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                    ui.label(egui::RichText::new("Console output stream.")
                        .monospace().size(9.0).color(palette.text_muted));
                });
            },
        );
    });
}

/// Agent card data for the dashboard.
pub struct DashboardAgentCard {
    pub id: u64,
    pub label: String,
    pub status: &'static str,
    pub tasks_done: usize,
    pub tasks_running: usize,
}

/// Render the dashboard bottom panel with real agent data.
pub fn render_dashboard_with_data(
    ui: &mut egui::Ui,
    agents: &[DashboardAgentCard],
    state: &mut BottomPanelState,
    palette: IdePalette,
) {
    render_dashboard_bottom_impl(ui, agents, state, palette);
}

fn render_dashboard_bottom(
    ui: &mut egui::Ui,
    state: &mut BottomPanelState,
    palette: IdePalette,
) {
    // Default with empty agent list (used when no orchestrator data is available)
    render_dashboard_bottom_impl(ui, &[], state, palette);
}

fn render_dashboard_bottom_impl(
    ui: &mut egui::Ui,
    agents: &[DashboardAgentCard],
    state: &mut BottomPanelState,
    palette: IdePalette,
) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Mission Dashboard").size(10.0).strong().color(palette.accent));
        if !agents.is_empty() {
            ui.label(egui::RichText::new(format!("({} agents)", agents.len())).size(9.0).color(palette.text_muted));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(egui::RichText::new("▼").size(9.0).color(palette.text_muted)).clicked() {
                state.collapsed = true;
            }
        });
    });
    ui.separator();

    if agents.is_empty() {
        ui.label(egui::RichText::new("No agents deployed. Use Deploy to launch agents.").size(9.0).color(palette.text_muted));
        return;
    }

    ui.horizontal_wrapped(|ui| {
        for agent in agents {
            let status_color = match agent.status {
                "running" => palette.success,
                "idle" => palette.text_muted,
                "failed" => palette.error,
                "blocked" => palette.warning,
                _ => palette.text_muted,
            };
            egui::Frame::new()
                .fill(palette.bg_tertiary)
                .corner_radius(6.0)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("●").size(8.0).color(status_color));
                        ui.label(egui::RichText::new(&agent.label).size(10.0).strong().color(palette.text));
                    });
                    ui.label(egui::RichText::new(format!("#{} · {}", agent.id, agent.status)).size(9.0).color(status_color));
                    if agent.tasks_running > 0 {
                        ui.label(egui::RichText::new(format!("{} running", agent.tasks_running)).size(8.0).color(palette.success));
                    }
                    ui.label(egui::RichText::new(format!("{} done", agent.tasks_done)).size(8.0).color(palette.text_muted));
                });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_is_expanded() {
        let state = BottomPanelState::default();
        assert!(!state.collapsed);
        assert_eq!(state.active_tab, 0);
    }

    #[test]
    fn tabbed_layout_construction() {
        let layout = BottomPanelLayout::Tabbed(vec!["A", "B", "C"]);
        assert!(matches!(layout, BottomPanelLayout::Tabbed(ref v) if v.len() == 3));
    }

    #[test]
    fn split_layout_construction() {
        let layout = BottomPanelLayout::Split { left: "L", right: "R" };
        assert!(matches!(layout, BottomPanelLayout::Split { left: "L", right: "R" }));
    }
}
