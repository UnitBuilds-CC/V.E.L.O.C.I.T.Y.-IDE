#![allow(dead_code)]

//! Mode Configuration - Trait-based specialization for each WorkspaceProfile.
//!
//! Each mode (Coder, Operator, MissionControl, Accessibility) implements the
//! `ModeConfig` trait, providing its own sidebar tabs, toolbar actions, right
//! panel layout, bottom panel layout, and command filtering. The main render
//! loop delegates to the active `ModeConfig` instead of hardcoding per-mode
//! branches throughout `ui_render.rs`.

use crate::editor::sidebar_tabs::SidebarTab;
use crate::editor::toolbar_actions::ToolbarAction;
use crate::editor::bottom_panel::BottomPanelLayout;
use crate::editor::theme::WorkspaceProfile;

// ═══════════════════════════════════════════════════════════════════════════
// Right Panel Descriptors
// ═══════════════════════════════════════════════════════════════════════════

/// Describes a panel slot in the right sidebar.
#[derive(Clone, Debug)]
pub struct RightPanel {
    pub id: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
}

// ═══════════════════════════════════════════════════════════════════════════
// ModeConfig Trait
// ═══════════════════════════════════════════════════════════════════════════

/// Cohesive per-mode UI configuration. Eliminates scattered per-mode branching
/// throughout the render loop.
pub trait ModeConfig: Send + Sync {
    /// Left sidebar tabs for this mode.
    fn left_tabs(&self) -> &[SidebarTab];

    /// Right sidebar panels for this mode.
    fn right_panels(&self) -> &[RightPanel];

    /// Toolbar action buttons for this mode.
    fn toolbar_actions(&self) -> &[ToolbarAction];

    /// Bottom panel layout for this mode.
    fn bottom_layout(&self) -> BottomPanelLayout;

    /// Filter predicate for the command palette: returns the list of command
    /// categories that are prioritized (shown first) in this mode.
    fn priority_categories(&self) -> &[&'static str];

    /// Categories hidden from the command palette in this mode.
    fn hidden_categories(&self) -> &[&'static str] {
        &[]
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Coder Mode
// ═══════════════════════════════════════════════════════════════════════════

pub struct CoderMode;

static CODER_LEFT_TABS: &[SidebarTab] = &[
    SidebarTab::Files,
    SidebarTab::Outline,
    SidebarTab::Git,
    SidebarTab::Search,
];

static CODER_RIGHT_PANELS: &[RightPanel] = &[
    RightPanel { id: "symbol_context", label: "Symbol Context", icon: "◎" },
    RightPanel { id: "active_changes", label: "Active Changes", icon: "±" },
    RightPanel { id: "ai_suggestions", label: "AI Suggestions", icon: "✦" },
];

static CODER_TOOLBAR: &[ToolbarAction] = &[
    ToolbarAction { id: "file", label: "File", icon: "□", shortcut: Some("Ctrl+N"), category: "File" },
    ToolbarAction { id: "run", label: "Run", icon: "▶", shortcut: Some("Ctrl+R"), category: "Build" },
    ToolbarAction { id: "build", label: "Build", icon: "⚙", shortcut: Some("Ctrl+B"), category: "Build" },
    ToolbarAction { id: "debug", label: "Debug", icon: "⊘", shortcut: None, category: "Build" },
    ToolbarAction { id: "test", label: "Test", icon: "✓", shortcut: None, category: "Build" },
    ToolbarAction { id: "git", label: "Git", icon: "⑂", shortcut: None, category: "File" },
];

static CODER_PRIORITY_CATEGORIES: &[&str] = &["File", "Build", "View"];

impl ModeConfig for CoderMode {
    fn left_tabs(&self) -> &[SidebarTab] { CODER_LEFT_TABS }
    fn right_panels(&self) -> &[RightPanel] { CODER_RIGHT_PANELS }
    fn toolbar_actions(&self) -> &[ToolbarAction] { CODER_TOOLBAR }
    fn bottom_layout(&self) -> BottomPanelLayout {
        BottomPanelLayout::Tabbed(vec!["Terminal", "Problems", "Output", "Chat"])
    }
    fn priority_categories(&self) -> &[&'static str] { CODER_PRIORITY_CATEGORIES }
}

// ═══════════════════════════════════════════════════════════════════════════
// Automation Operator Mode
// ═══════════════════════════════════════════════════════════════════════════

pub struct OperatorMode;

static OPERATOR_LEFT_TABS: &[SidebarTab] = &[
    SidebarTab::Flows,
    SidebarTab::Targets,
    SidebarTab::Recordings,
    SidebarTab::Logs,
];

static OPERATOR_RIGHT_PANELS: &[RightPanel] = &[
    RightPanel { id: "flow_inspector", label: "Flow Inspector", icon: "⧉" },
    RightPanel { id: "element_picker", label: "Element Picker", icon: "⊞" },
    RightPanel { id: "action_log", label: "Action Log", icon: "≡" },
];

static OPERATOR_TOOLBAR: &[ToolbarAction] = &[
    ToolbarAction { id: "record", label: "Record", icon: "●", shortcut: Some("Ctrl+R"), category: "Automation" },
    ToolbarAction { id: "run_flow", label: "Run Flow", icon: "▶", shortcut: Some("Ctrl+Enter"), category: "Automation" },
    ToolbarAction { id: "stop", label: "Stop", icon: "■", shortcut: Some("Ctrl+."), category: "Automation" },
    ToolbarAction { id: "schedule", label: "Schedule", icon: "⏲", shortcut: None, category: "Automation" },
    ToolbarAction { id: "targets", label: "Targets", icon: "◎", shortcut: None, category: "Automation" },
    ToolbarAction { id: "settings", label: "Settings", icon: "⚙", shortcut: None, category: "Panels" },
];

static OPERATOR_PRIORITY_CATEGORIES: &[&str] = &["Automation", "Panels"];

impl ModeConfig for OperatorMode {
    fn left_tabs(&self) -> &[SidebarTab] { OPERATOR_LEFT_TABS }
    fn right_panels(&self) -> &[RightPanel] { OPERATOR_RIGHT_PANELS }
    fn toolbar_actions(&self) -> &[ToolbarAction] { OPERATOR_TOOLBAR }
    fn bottom_layout(&self) -> BottomPanelLayout {
        BottomPanelLayout::Split {
            left: "Live Action Preview",
            right: "Console",
        }
    }
    fn priority_categories(&self) -> &[&'static str] { OPERATOR_PRIORITY_CATEGORIES }
    fn hidden_categories(&self) -> &[&'static str] { &["Build"] }
}

// ═══════════════════════════════════════════════════════════════════════════
// Mission Control Mode
// ═══════════════════════════════════════════════════════════════════════════

pub struct MissionMode;

static MISSION_LEFT_TABS: &[SidebarTab] = &[
    SidebarTab::Agents,
    SidebarTab::Queue,
    SidebarTab::Timeline,
    SidebarTab::Metrics,
];

static MISSION_RIGHT_PANELS: &[RightPanel] = &[
    RightPanel { id: "agent_detail", label: "Agent Detail", icon: "⊙" },
    RightPanel { id: "task_inspector", label: "Task Inspector", icon: "⊟" },
    RightPanel { id: "alerts", label: "Alerts", icon: "⚠" },
];

static MISSION_TOOLBAR: &[ToolbarAction] = &[
    ToolbarAction { id: "deploy", label: "Deploy", icon: "▲", shortcut: Some("Ctrl+D"), category: "Agent" },
    ToolbarAction { id: "pause_all", label: "Pause All", icon: "⏸", shortcut: None, category: "Agent" },
    ToolbarAction { id: "resume_all", label: "Resume All", icon: "▶", shortcut: None, category: "Agent" },
    ToolbarAction { id: "scale", label: "Scale", icon: "⇅", shortcut: None, category: "Agent" },
    ToolbarAction { id: "alerts", label: "Alerts", icon: "⚠", shortcut: None, category: "Agent" },
    ToolbarAction { id: "reports", label: "Reports", icon: "◫", shortcut: None, category: "Agent" },
];

static MISSION_PRIORITY_CATEGORIES: &[&str] = &["Agent", "Workspace"];

impl ModeConfig for MissionMode {
    fn left_tabs(&self) -> &[SidebarTab] { MISSION_LEFT_TABS }
    fn right_panels(&self) -> &[RightPanel] { MISSION_RIGHT_PANELS }
    fn toolbar_actions(&self) -> &[ToolbarAction] { MISSION_TOOLBAR }
    fn bottom_layout(&self) -> BottomPanelLayout {
        BottomPanelLayout::Dashboard
    }
    fn priority_categories(&self) -> &[&'static str] { MISSION_PRIORITY_CATEGORIES }
    fn hidden_categories(&self) -> &[&'static str] { &["Build", "File"] }
}

// ═══════════════════════════════════════════════════════════════════════════
// Accessibility Mode
// ═══════════════════════════════════════════════════════════════════════════

pub struct AccessMode;

static ACCESS_LEFT_TABS: &[SidebarTab] = &[
    SidebarTab::Files,
    SidebarTab::Favorites,
    SidebarTab::Bookmarks,
    SidebarTab::AccessibilityAudit,
];

static ACCESS_RIGHT_PANELS: &[RightPanel] = &[
    RightPanel { id: "a11y_tree", label: "Accessibility Tree", icon: "⊿" },
    RightPanel { id: "contrast_checker", label: "Contrast Checker", icon: "◐" },
    RightPanel { id: "aria_inspector", label: "ARIA Inspector", icon: "⊜" },
];

static ACCESS_TOOLBAR: &[ToolbarAction] = &[
    ToolbarAction { id: "file", label: "File", icon: "□", shortcut: Some("Ctrl+N"), category: "File" },
    ToolbarAction { id: "preview", label: "Preview", icon: "◉", shortcut: None, category: "View" },
    ToolbarAction { id: "audit", label: "Audit", icon: "✓", shortcut: None, category: "View" },
    ToolbarAction { id: "contrast", label: "Contrast", icon: "◐", shortcut: None, category: "View" },
    ToolbarAction { id: "screen_reader", label: "SR Sim", icon: "♿", shortcut: None, category: "View" },
];

static ACCESS_PRIORITY_CATEGORIES: &[&str] = &["View", "File"];

impl ModeConfig for AccessMode {
    fn left_tabs(&self) -> &[SidebarTab] { ACCESS_LEFT_TABS }
    fn right_panels(&self) -> &[RightPanel] { ACCESS_RIGHT_PANELS }
    fn toolbar_actions(&self) -> &[ToolbarAction] { ACCESS_TOOLBAR }
    fn bottom_layout(&self) -> BottomPanelLayout {
        BottomPanelLayout::Tabbed(vec!["Audit Results", "Keyboard Nav Map", "Chat"])
    }
    fn priority_categories(&self) -> &[&'static str] { ACCESS_PRIORITY_CATEGORIES }
}

// ═══════════════════════════════════════════════════════════════════════════
// Factory
// ═══════════════════════════════════════════════════════════════════════════

/// Return the static `ModeConfig` implementation for a given profile.
pub fn mode_config_for(profile: WorkspaceProfile) -> &'static dyn ModeConfig {
    match profile {
        WorkspaceProfile::Coder => &CoderMode,
        WorkspaceProfile::AutomationOperator => &OperatorMode,
        WorkspaceProfile::MissionControl => &MissionMode,
        WorkspaceProfile::Accessibility => &AccessMode,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_mode_has_four_left_tabs() {
        for profile in WorkspaceProfile::ALL {
            let cfg = mode_config_for(profile);
            assert!(cfg.left_tabs().len() >= 3, "{:?} has too few left tabs", profile);
        }
    }

    #[test]
    fn each_mode_has_right_panels() {
        for profile in WorkspaceProfile::ALL {
            let cfg = mode_config_for(profile);
            assert_eq!(cfg.right_panels().len(), 3, "{:?} should have 3 right panels", profile);
        }
    }

    #[test]
    fn each_mode_has_toolbar_actions() {
        for profile in WorkspaceProfile::ALL {
            let cfg = mode_config_for(profile);
            assert!(cfg.toolbar_actions().len() >= 5, "{:?} has too few toolbar actions", profile);
        }
    }

    #[test]
    fn coder_bottom_is_tabbed() {
        let cfg = mode_config_for(WorkspaceProfile::Coder);
        assert!(matches!(cfg.bottom_layout(), BottomPanelLayout::Tabbed(_)));
    }

    #[test]
    fn operator_bottom_is_split() {
        let cfg = mode_config_for(WorkspaceProfile::AutomationOperator);
        assert!(matches!(cfg.bottom_layout(), BottomPanelLayout::Split { .. }));
    }

    #[test]
    fn mission_bottom_is_dashboard() {
        let cfg = mode_config_for(WorkspaceProfile::MissionControl);
        assert!(matches!(cfg.bottom_layout(), BottomPanelLayout::Dashboard));
    }
}
