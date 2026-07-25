#![allow(dead_code)]

//! Toolbar Actions - Mode-specific toolbar button definitions and renderers.
//!
//! Each mode provides its own set of toolbar actions. The toolbar render loop
//! iterates the active `ModeConfig`'s toolbar_actions list rather than a fixed
//! hardcoded set.

use crate::editor::theme::IdePalette;
use eframe::egui;

// ═══════════════════════════════════════════════════════════════════════════
// ToolbarAction Descriptor
// ═══════════════════════════════════════════════════════════════════════════

/// Describes a single toolbar action button.
#[derive(Clone, Debug)]
pub struct ToolbarAction {
    pub id: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
    pub shortcut: Option<&'static str>,
    pub category: &'static str,
}

impl ToolbarAction {
    /// Tooltip text combining label and shortcut.
    pub fn tooltip(&self) -> String {
        match self.shortcut {
            Some(sc) => format!("{} ({})", self.label, sc),
            None => self.label.to_string(),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Toolbar Renderer
// ═══════════════════════════════════════════════════════════════════════════

/// Render the mode-specific toolbar buttons. Returns the `id` of any button
/// that was clicked this frame, or None.
///
/// Uses icon-only buttons with tooltips so the toolbar stays compact even
/// when a mode has many actions.  The full label and shortcut appear on
/// hover, keeping the bar visually light.
pub fn render_mode_toolbar(
    ui: &mut egui::Ui,
    actions: &[ToolbarAction],
    palette: IdePalette,
) -> Option<&'static str> {
    let mut clicked: Option<&'static str> = None;

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for action in actions {
            let icon_text = egui::RichText::new(action.icon)
                .size(13.0)
                .color(palette.text);
            let btn = ui.small_button(icon_text);
            if btn.clicked() {
                clicked = Some(action.id);
            }
            btn.on_hover_text(action.tooltip());
        }
    });

    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolbar_action_tooltip_with_shortcut() {
        let action = ToolbarAction {
            id: "run",
            label: "Run",
            icon: "▶",
            shortcut: Some("Ctrl+R"),
            category: "Build",
        };
        assert_eq!(action.tooltip(), "Run (Ctrl+R)");
    }

    #[test]
    fn toolbar_action_tooltip_no_shortcut() {
        let action = ToolbarAction {
            id: "test",
            label: "Test",
            icon: "✓",
            shortcut: None,
            category: "Build",
        };
        assert_eq!(action.tooltip(), "Test");
    }
}
