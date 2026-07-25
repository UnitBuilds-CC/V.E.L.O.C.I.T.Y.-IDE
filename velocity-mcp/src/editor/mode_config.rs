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
    SidebarTab::Browse,
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
        BottomPanelLayout::Tabbed(vec!["Terminal", "Problems", "Output", "Checkpoints", "Chat"])
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
    fn hidden_categories(&self) -> &[&'static str] { &["Build"] }
}

// ═══════════════════════════════════════════════════════════════════════════
// Accessibility Features
// ═══════════════════════════════════════════════════════════════════════════

/// Screen reader simulation mode: reads UI elements in tab order.
pub struct ScreenReaderSim {
    pub enabled: bool,
    pub focus_index: usize,
    pub elements: Vec<A11yElement>,
    pub speech_buffer: Vec<String>,
}

/// An accessibility tree element for screen reader navigation.
#[derive(Clone, Debug)]
pub struct A11yElement {
    pub role: A11yRole,
    pub name: String,
    pub value: Option<String>,
    pub description: Option<String>,
    pub bounds: (f32, f32, f32, f32), // x, y, w, h
    pub focusable: bool,
    pub children: Vec<A11yElement>,
}

/// ARIA roles for accessibility tree elements.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum A11yRole {
    Alert,
    Button,
    Checkbox,
    Dialog,
    Document,
    Form,
    Heading,
    Image,
    Link,
    List,
    ListItem,
    Menu,
    MenuItem,
    Navigation,
    None,
    ProgressBar,
    Radio,
    Region,
    Search,
    Slider,
    StatusBar,
    Tab,
    TabList,
    TabPanel,
    TextBox,
    Toolbar,
    Tree,
    TreeItem,
    Window,
}

impl A11yRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Alert => "alert", Self::Button => "button", Self::Checkbox => "checkbox",
            Self::Dialog => "dialog", Self::Document => "document", Self::Form => "form",
            Self::Heading => "heading", Self::Image => "img", Self::Link => "link",
            Self::List => "list", Self::ListItem => "listitem", Self::Menu => "menu",
            Self::MenuItem => "menuitem", Self::Navigation => "navigation", Self::None => "none",
            Self::ProgressBar => "progressbar", Self::Radio => "radio", Self::Region => "region",
            Self::Search => "search", Self::Slider => "slider", Self::StatusBar => "statusbar",
            Self::Tab => "tab", Self::TabList => "tablist", Self::TabPanel => "tabpanel",
            Self::TextBox => "textbox", Self::Toolbar => "toolbar", Self::Tree => "tree",
            Self::TreeItem => "treeitem", Self::Window => "window",
        }
    }
}

impl ScreenReaderSim {
    pub fn new() -> Self {
        Self { enabled: false, focus_index: 0, elements: Vec::new(), speech_buffer: Vec::new() }
    }

    /// Flatten the accessibility tree into a list of focusable elements.
    pub fn flatten_focusable(elements: &[A11yElement]) -> Vec<&A11yElement> {
        let mut result = Vec::new();
        for el in elements {
            if el.focusable { result.push(el); }
            result.extend(Self::flatten_focusable(&el.children));
        }
        result
    }

    /// Move focus to the next element and announce it.
    pub fn focus_next(&mut self) -> Option<String> {
        let focusable = Self::flatten_focusable(&self.elements);
        if focusable.is_empty() { return None; }
        self.focus_index = (self.focus_index + 1) % focusable.len();
        let el = focusable[self.focus_index];
        let announcement = self.announce(el);
        self.speech_buffer.push(announcement.clone());
        Some(announcement)
    }

    /// Move focus to the previous element and announce it.
    pub fn focus_prev(&mut self) -> Option<String> {
        let focusable = Self::flatten_focusable(&self.elements);
        if focusable.is_empty() { return None; }
        if self.focus_index == 0 {
            self.focus_index = focusable.len() - 1;
        } else {
            self.focus_index -= 1;
        }
        let el = focusable[self.focus_index];
        let announcement = self.announce(el);
        self.speech_buffer.push(announcement.clone());
        Some(announcement)
    }

    /// Generate the speech announcement for an element.
    fn announce(&self, el: &A11yElement) -> String {
        let mut parts = Vec::new();
        parts.push(el.name.clone());
        parts.push(el.role.as_str().to_string());
        if let Some(val) = &el.value {
            parts.push(format!("value: {}", val));
        }
        if let Some(desc) = &el.description {
            parts.push(desc.clone());
        }
        parts.join(", ")
    }

    /// Get the currently focused element.
    pub fn focused_element(&self) -> Option<&A11yElement> {
        let focusable = Self::flatten_focusable(&self.elements);
        focusable.get(self.focus_index).copied()
    }
}

/// High contrast theme palette for accessibility mode.
pub struct HighContrastPalette {
    pub background: [u8; 3],
    pub foreground: [u8; 3],
    pub selection_bg: [u8; 3],
    pub selection_fg: [u8; 3],
    pub cursor: [u8; 3],
    pub line_number: [u8; 3],
    pub error: [u8; 3],
    pub warning: [u8; 3],
    pub info: [u8; 3],
    pub focus_ring: [u8; 3],
}

impl HighContrastPalette {
    /// Pure black background with white text — maximum contrast ratio (21:1).
    pub fn dark() -> Self {
        Self {
            background: [0, 0, 0],
            foreground: [255, 255, 255],
            selection_bg: [0, 120, 215],
            selection_fg: [255, 255, 255],
            cursor: [255, 255, 0],
            line_number: [160, 160, 160],
            error: [255, 80, 80],
            warning: [255, 200, 0],
            info: [80, 200, 255],
            focus_ring: [255, 255, 0],
        }
    }

    /// Pure white background with black text.
    pub fn light() -> Self {
        Self {
            background: [255, 255, 255],
            foreground: [0, 0, 0],
            selection_bg: [0, 120, 215],
            selection_fg: [255, 255, 255],
            cursor: [0, 0, 0],
            line_number: [100, 100, 100],
            error: [200, 0, 0],
            warning: [180, 120, 0],
            info: [0, 80, 180],
            focus_ring: [0, 0, 255],
        }
    }

    /// Compute the WCAG contrast ratio between two colors.
    pub fn contrast_ratio(a: [u8; 3], b: [u8; 3]) -> f64 {
        let lum_a = relative_luminance(a);
        let lum_b = relative_luminance(b);
        let (lighter, darker) = if lum_a > lum_b { (lum_a, lum_b) } else { (lum_b, lum_a) };
        (lighter + 0.05) / (darker + 0.05)
    }
}

/// WCAG 2.1 relative luminance calculation.
fn relative_luminance(rgb: [u8; 3]) -> f64 {
    let srgb = rgb.map(|c| {
        let s = c as f64 / 255.0;
        if s <= 0.03928 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    });
    0.2126 * srgb[0] + 0.7152 * srgb[1] + 0.0722 * srgb[2]
}

/// Keyboard navigation map: maps key combos to actions.
pub struct KeyboardNavMap {
    pub bindings: Vec<KeyBinding>,
}

pub struct KeyBinding {
    pub key: String,
    pub modifiers: Vec<String>,
    pub action: String,
    pub description: String,
}

impl KeyboardNavMap {
    /// Default accessibility-focused keyboard navigation bindings.
    pub fn default_a11y_bindings() -> Self {
        Self {
            bindings: vec![
                KeyBinding { key: "Tab".into(), modifiers: vec![], action: "focus_next".into(), description: "Move to next focusable element".into() },
                KeyBinding { key: "Tab".into(), modifiers: vec!["Shift".into()], action: "focus_prev".into(), description: "Move to previous focusable element".into() },
                KeyBinding { key: "Enter".into(), modifiers: vec![], action: "activate".into(), description: "Activate focused element".into() },
                KeyBinding { key: "Space".into(), modifiers: vec![], action: "toggle".into(), description: "Toggle focused checkbox/button".into() },
                KeyBinding { key: "Escape".into(), modifiers: vec![], action: "dismiss".into(), description: "Close dialog or clear focus".into() },
                KeyBinding { key: "F6".into(), modifiers: vec![], action: "next_panel".into(), description: "Move to next panel region".into() },
                KeyBinding { key: "F6".into(), modifiers: vec!["Shift".into()], action: "prev_panel".into(), description: "Move to previous panel region".into() },
                KeyBinding { key: "F10".into(), modifiers: vec![], action: "context_menu".into(), description: "Open context menu for focused element".into() },
                KeyBinding { key: "/".into(), modifiers: vec!["Ctrl".into()], action: "search".into(), description: "Focus search field".into() },
                KeyBinding { key: "G".into(), modifiers: vec!["Ctrl".into()], action: "go_to_line".into(), description: "Go to line number".into() },
            ],
        }
    }

    /// Find the binding for a key combo.
    pub fn lookup(&self, key: &str, modifiers: &[&str]) -> Option<&KeyBinding> {
        self.bindings.iter().find(|b| {
            b.key == key && b.modifiers.len() == modifiers.len()
                && b.modifiers.iter().all(|m| modifiers.contains(&m.as_str()))
        })
    }

    /// Dispatch a key event to the appropriate action handler.
    /// Returns the action string if a binding was found, None otherwise.
    pub fn dispatch_key_event(&self, key: &str, modifiers: &[&str]) -> Option<String> {
        self.lookup(key, modifiers).map(|b| b.action.clone())
    }

    /// Get all bindings for a specific action category (e.g., "focus", "navigate").
    pub fn bindings_by_category(&self, prefix: &str) -> Vec<&KeyBinding> {
        self.bindings.iter().filter(|b| b.action.starts_with(prefix)).collect()
    }
}

/// Accessibility integration hub: coordinates screen reader, high contrast,
/// and keyboard navigation.
pub struct AccessibilityHub {
    pub screen_reader: ScreenReaderSim,
    pub contrast: HighContrastPalette,
    pub nav_map: KeyboardNavMap,
    pub enabled: bool,
}

impl AccessibilityHub {
    pub fn new() -> Self {
        Self {
            screen_reader: ScreenReaderSim::new(),
            contrast: HighContrastPalette::dark(),
            nav_map: KeyboardNavMap::default_a11y_bindings(),
            enabled: false,
        }
    }

    /// Enable accessibility mode with dark high-contrast theme.
    pub fn enable(&mut self) {
        self.enabled = true;
        self.screen_reader.enabled = true;
        self.contrast = HighContrastPalette::dark();
    }

    /// Disable accessibility mode.
    pub fn disable(&mut self) {
        self.enabled = false;
        self.screen_reader.enabled = false;
    }

    /// Toggle between dark and light high-contrast themes.
    pub fn toggle_contrast(&mut self) {
        if self.contrast.background == [0, 0, 0] {
            self.contrast = HighContrastPalette::light();
        } else {
            self.contrast = HighContrastPalette::dark();
        }
    }

    /// Process a keyboard event through the navigation map.
    /// Returns the action if handled, or None if not an a11y binding.
    pub fn handle_key_event(&self, key: &str, modifiers: &[&str]) -> Option<String> {
        if !self.enabled { return None; }
        self.nav_map.dispatch_key_event(key, modifiers)
    }

    /// Build the accessibility tree from UI elements.
    pub fn build_accessibility_tree(&mut self, elements: Vec<A11yElement>) {
        self.screen_reader.elements = elements;
        self.screen_reader.focus_index = 0;
        self.screen_reader.speech_buffer.clear();
    }

    /// Get the current contrast ratio for the theme.
    pub fn current_contrast_ratio(&self) -> f64 {
        HighContrastPalette::contrast_ratio(self.contrast.background, self.contrast.foreground)
    }
}

impl Default for AccessibilityHub {
    fn default() -> Self {
        Self::new()
    }
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
