//! Agentic-first layout configuration for V.E.L.O.C.I.T.Y. IDE
//! 
//! New layout structure:
//! ┌─────────────────────────────────────────────────────────────┐
//! │ Enhanced Status Bar (Agent State + Metrics)                 │
//! ├─────────────┬──────────────────────────────────────────────┤
//! │  Thinking   │ Main Chat Panel (70%)                        │
//! │  Thread +   │ - Agent Conversation                         │
//! │  Task Graph │ - Real-time Reasoning Display                │
//! │  (30%)      │ - Inline Tool Approvals                      │
//! ├─────────────┼──────────────────────────────────────────────┤
//! │  Context    │ Code Editor / Diff Viewer (Responsive)       │
//! │  Sidebar    │ - Shows edits made by agent                  │
//! │  (30%)      │ - Accept/Reject per-line changes             │
//! └─────────────┴──────────────────────────────────────────────┘

use egui_dock::{DockState, NodeIndex};

/// Represents the three main panes in the agentic layout
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LayoutPane {
    /// Left: Thinking thread, task graph, context
    ThinkingContext,
    /// Center: Agent chat and message stream
    AgentChat,
    /// Right: Code editor, diff viewer
    CodeEditor,
}

/// Initialize dock state with agentic-first layout
/// 
/// Creates a 3-pane layout optimized for agent-driven workflows:
/// - Thinking/Context on left (30%)
/// - Agent Chat in center (40%)
/// - Code Editor on right (30%)
pub fn create_agentic_layout<T: Clone + 'static>(
    thinking_tab: T,
    chat_tab: T,
    code_tab: T,
) -> DockState<T> {
    let mut dock = DockState::new(vec![thinking_tab, chat_tab, code_tab]);
    
    // Split root vertically into left (thinking) and right (chat + code)
    let [thinking, right_split] = dock.main_surface_mut().split_left(
        NodeIndex::root(),
        0.28, // 28% for thinking pane on left
        vec![],
    );
    
    // Split right side horizontally into chat (center) and code (right)
    let [_chat, _code] = dock.main_surface_mut().split_right(
        right_split,
        0.45, // 45% for code pane on right (leaves 55% for chat)
        vec![],
    );
    
    dock
}

/// Calculate responsive panel widths based on available space
pub fn calculate_panel_widths(available_width: f32) -> (f32, f32, f32) {
    let thinking_width = (available_width * 0.28).max(200.0).min(400.0);
    let code_width = (available_width * 0.30).max(250.0).min(600.0);
    let chat_width = available_width - thinking_width - code_width;
    (thinking_width, chat_width, code_width)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_panel_widths_calculation() {
        let (thinking, chat, code) = calculate_panel_widths(1200.0);
        assert!(thinking > 200.0);
        assert!(chat > 0.0);
        assert!(code > 250.0);
        assert_eq!(thinking + chat + code, 1200.0);
    }
}
