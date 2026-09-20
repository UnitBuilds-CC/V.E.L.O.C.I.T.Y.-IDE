//! Bottom Panel State - the tab strip's selection, buffers and actions.
//!
//! The panel used to declare a per-mode layout (`Tabbed`/`Split`/`Dashboard`)
//! and carry a renderer for each. `ModeConfig::bottom_layout()` was read by
//! nothing, so the whole renderer half went: `ui_render.rs` builds its own tab
//! strip from the `TAB_*` indices below and draws the content inline.

// ═══════════════════════════════════════════════════════════════════════════
// Bottom Panel State
// ═══════════════════════════════════════════════════════════════════════════

/// Tracks the active tab index for Tabbed layouts and split ratios.
pub struct BottomPanelState {
    pub active_tab: usize,
    pub split_ratio: f32,
    pub panel_height: f32,
    pub collapsed: bool,
    /// Terminal output buffer (from PTY or command execution).
    pub terminal_output: String,
    /// Terminal input line.
    pub terminal_input: String,
    /// Diagnostics error count (synced from DiagnosticsState).
    pub error_count: usize,
    /// Diagnostics warning count.
    pub warning_count: usize,
    /// Diagnostic messages for the Problems tab.
    pub diagnostic_messages: Vec<String>,
    /// Checkpoint restore/discard action requested by UI.
    pub checkpoint_action: Option<CheckpointAction>,
}

// Named indices for the bottom panel tabs so callers don't rely on magic numbers.
pub const TAB_TERMINAL: usize = 0;
pub const TAB_PROBLEMS: usize = 1;
pub const TAB_DEBUG: usize = 2;
pub const TAB_OUTPUT: usize = 3;
pub const TAB_CHECKPOINTS: usize = 4;

/// Maximum height the bottom panel can be resized to (prevents it from
/// consuming the entire window).
pub const MAX_PANEL_HEIGHT: f32 = 600.0;

/// Actions that the checkpoint UI can request (processed by VelocityApp).
#[derive(Debug, Clone)]
pub enum CheckpointAction {
    Restore(usize),
    Discard(usize),
}

impl Default for BottomPanelState {
    fn default() -> Self {
        Self {
            active_tab: 0,
            split_ratio: 0.55,
            panel_height: 240.0,
            collapsed: true,
            terminal_output: String::new(),
            terminal_input: String::new(),
            error_count: 0,
            warning_count: 0,
            diagnostic_messages: Vec::new(),
            checkpoint_action: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_is_collapsed() {
        let state = BottomPanelState::default();
        assert!(state.collapsed);
        assert_eq!(state.active_tab, TAB_TERMINAL);
    }
}
