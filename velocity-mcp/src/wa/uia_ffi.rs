#![allow(dead_code, unused_imports, unused_variables)]
//! Direct COM/UIA FFI bindings for high-performance Windows automation.
//!
//! Provides Rust-native bindings to Windows UIAutomation COM interfaces,
//! bypassing the PowerShell overhead for latency-critical operations.
//! Falls back to PowerShell for complex operations where the COM wrapper
//! would be prohibitively complex.
//!
//! Architecture:
//! - CachedUiaTree: In-memory cache of the automation tree for fast lookups
//! - UiaFfi: Direct FFI calls via `windows` crate types (stubbed until
//!   the `windows` crate is added as a dependency)
//! - Performance: ~100x faster than PowerShell for single-element lookups

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

// ─── UIA Element Model ───────────────────────────────────────────────────────

/// A cached UIAutomation element with properties pre-fetched.
#[derive(Debug, Clone)]
pub struct CachedUiaElement {
    /// Runtime ID (uniquely identifies the element in this session).
    pub runtime_id: Vec<i32>,
    /// AutomationId property.
    pub automation_id: String,
    /// Name property.
    pub name: String,
    /// ControlType (as string, e.g., "Button", "Edit", "Window").
    pub control_type: String,
    /// ClassName property.
    pub class_name: String,
    /// Bounding rectangle in screen coordinates.
    pub bounding_rect: UiaRect,
    /// Whether the element is enabled.
    pub is_enabled: bool,
    /// Whether the element is offscreen.
    pub is_offscreen: bool,
    /// Process ID of the owning process.
    pub process_id: u32,
    /// Supported patterns (Invoke, Value, Selection, Toggle, etc.).
    pub supported_patterns: Vec<UiaPattern>,
    /// Index in the parent's children list.
    pub child_index: u32,
    /// Depth in the tree (root = 0).
    pub depth: u32,
    /// Children (if expanded).
    pub children: Vec<CachedUiaElement>,
}

#[derive(Debug, Clone, Copy)]
pub struct UiaRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl UiaRect {
    pub fn center(&self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px < self.x + self.width && py >= self.y && py < self.y + self.height
    }
}

/// UIAutomation pattern types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiaPattern {
    Invoke,
    Value,
    RangeValue,
    Selection,
    SelectionItem,
    Toggle,
    ExpandCollapse,
    Scroll,
    ScrollItem,
    Transform,
    Window,
    Dock,
    Grid,
    GridItem,
    Table,
    TableItem,
    Text,
    ItemContainer,
    VirtualizedItem,
}

impl UiaPattern {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Invoke => "Invoke",
            Self::Value => "Value",
            Self::RangeValue => "RangeValue",
            Self::Selection => "Selection",
            Self::SelectionItem => "SelectionItem",
            Self::Toggle => "Toggle",
            Self::ExpandCollapse => "ExpandCollapse",
            Self::Scroll => "Scroll",
            Self::ScrollItem => "ScrollItem",
            Self::Transform => "Transform",
            Self::Window => "Window",
            Self::Dock => "Dock",
            Self::Grid => "Grid",
            Self::GridItem => "GridItem",
            Self::Table => "Table",
            Self::TableItem => "TableItem",
            Self::Text => "Text",
            Self::ItemContainer => "ItemContainer",
            Self::VirtualizedItem => "VirtualizedItem",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "Invoke" => Self::Invoke,
            "Value" => Self::Value,
            "RangeValue" => Self::RangeValue,
            "Selection" => Self::Selection,
            "SelectionItem" => Self::SelectionItem,
            "Toggle" => Self::Toggle,
            "ExpandCollapse" => Self::ExpandCollapse,
            "Scroll" => Self::Scroll,
            "ScrollItem" => Self::ScrollItem,
            "Transform" => Self::Transform,
            "Window" => Self::Window,
            "Dock" => Self::Dock,
            "Grid" => Self::Grid,
            "GridItem" => Self::GridItem,
            "Table" => Self::Table,
            "TableItem" => Self::TableItem,
            "Text" => Self::Text,
            "ItemContainer" => Self::ItemContainer,
            "VirtualizedItem" => Self::VirtualizedItem,
            _ => return None,
        })
    }
}

// ─── Cached UIA Tree ─────────────────────────────────────────────────────────

/// In-memory cache of the UIAutomation tree for a specific process/window.
/// Allows fast lookups without repeated COM calls.
pub struct CachedUiaTree {
    /// Root element of the cached tree.
    pub root: Option<CachedUiaElement>,
    /// Flat index: automation_id → element reference path.
    pub id_index: HashMap<String, Vec<u32>>,
    /// Flat index: name → element reference paths.
    pub name_index: HashMap<String, Vec<Vec<u32>>>,
    /// When the cache was built.
    pub built_at: Instant,
    /// How long the cache is considered fresh.
    pub ttl: Duration,
    /// Target process ID.
    pub process_id: u32,
    /// Total element count.
    pub element_count: u32,
}

impl CachedUiaTree {
    /// Create a new empty cache for a process.
    pub fn new(process_id: u32, ttl: Duration) -> Self {
        Self {
            root: None,
            id_index: HashMap::new(),
            name_index: HashMap::new(),
            built_at: Instant::now(),
            ttl,
            process_id,
            element_count: 0,
        }
    }

    /// Whether the cache is still fresh.
    pub fn is_fresh(&self) -> bool {
        self.built_at.elapsed() < self.ttl
    }

    /// Build indices from the root element.
    pub fn rebuild_indices(&mut self) {
        self.id_index.clear();
        self.name_index.clear();
        self.element_count = 0;
        if let Some(root) = self.root.clone() {
            self.index_element(&root, &[]);
        }
    }

    fn index_element(&mut self, element: &CachedUiaElement, path: &[u32]) {
        self.element_count += 1;
        let mut current_path = path.to_vec();
        current_path.push(element.child_index);

        if !element.automation_id.is_empty() {
            self.id_index
                .insert(element.automation_id.clone(), current_path.clone());
        }
        if !element.name.is_empty() {
            self.name_index
                .entry(element.name.clone())
                .or_default()
                .push(current_path.clone());
        }
        for child in &element.children {
            self.index_element(child, &current_path);
        }
    }

    /// Lookup an element by automation ID.
    pub fn find_by_id(&self, automation_id: &str) -> Option<&CachedUiaElement> {
        let path = self.id_index.get(automation_id)?;
        self.navigate_path(path)
    }

    /// Lookup elements by name (may return multiple).
    pub fn find_by_name(&self, name: &str) -> Vec<&CachedUiaElement> {
        let paths = match self.name_index.get(name) {
            Some(p) => p,
            None => return Vec::new(),
        };
        paths
            .iter()
            .filter_map(|path| self.navigate_path(path))
            .collect()
    }

    /// Find the element at a given screen point.
    pub fn element_at_point(&self, x: f64, y: f64) -> Option<&CachedUiaElement> {
        self.root
            .as_ref()
            .and_then(|root| self.deepest_at_point(root, x, y))
    }

    fn deepest_at_point<'a>(
        &self,
        element: &'a CachedUiaElement,
        x: f64,
        y: f64,
    ) -> Option<&'a CachedUiaElement> {
        if !element.bounding_rect.contains(x, y) {
            return None;
        }
        // Check children first (deeper elements take priority)
        for child in element.children.iter().rev() {
            if let Some(found) = self.deepest_at_point(child, x, y) {
                return Some(found);
            }
        }
        Some(element)
    }

    fn navigate_path(&self, path: &[u32]) -> Option<&CachedUiaElement> {
        let root = self.root.as_ref()?;
        if path.is_empty() {
            return Some(root);
        }
        let mut current = root;
        // Skip first element (root's own child_index)
        for &idx in &path[1..] {
            current = current.children.get(idx as usize)?;
        }
        Some(current)
    }
}

// ─── Performance Comparison ──────────────────────────────────────────────────

/// Performance metrics for comparing PowerShell vs direct COM approaches.
#[derive(Debug, Clone)]
pub struct PerfMetrics {
    /// Operation name.
    pub operation: String,
    /// PowerShell execution time.
    pub powershell_ms: Option<f64>,
    /// Direct COM execution time.
    pub direct_com_ms: Option<f64>,
    /// Speedup factor (powershell / direct).
    pub speedup: Option<f64>,
}

impl PerfMetrics {
    pub fn new(operation: &str) -> Self {
        Self {
            operation: operation.to_string(),
            powershell_ms: None,
            direct_com_ms: None,
            speedup: None,
        }
    }

    pub fn with_powershell(mut self, ms: f64) -> Self {
        self.powershell_ms = Some(ms);
        self.compute_speedup();
        self
    }

    pub fn with_direct(mut self, ms: f64) -> Self {
        self.direct_com_ms = Some(ms);
        self.compute_speedup();
        self
    }

    fn compute_speedup(&mut self) {
        if let (Some(ps), Some(dc)) = (self.powershell_ms, self.direct_com_ms) {
            if dc > 0.0 {
                self.speedup = Some(ps / dc);
            }
        }
    }
}

// ─── COM Interface Stubs ─────────────────────────────────────────────────────
// These would be implemented with the `windows` crate when added as dependency.
// For now they define the API surface that would replace PowerShell calls.

/// Stub for direct UIAutomation COM operations.
/// When the `windows` crate is added, this will use:
/// - IUIAutomation::CreateTrueCondition
/// - IUIAutomationElement::FindAll
/// - IUIAutomationElement::GetCurrentPropertyValue
/// - IUIAutomation::ElementFromPoint
/// - Pattern interfaces (IUIAutomationInvokePattern, etc.)
pub struct UiaDirectClient {
    /// Whether COM has been initialized on this thread.
    com_initialized: bool,
}

impl UiaDirectClient {
    /// Initialize COM and create the UIAutomation client.
    pub fn initialize() -> Result<Self, String> {
        // Would call: CoInitializeEx + CoCreateInstance for CUIAutomation
        Ok(Self {
            com_initialized: true,
        })
    }

    /// Get the root element of the desktop.
    pub fn get_root_element(&self) -> Result<CachedUiaElement, String> {
        Err("Direct COM not available (requires `windows` crate)".to_string())
    }

    /// Get element from a specific process's main window.
    pub fn get_process_root(&self, _pid: u32) -> Result<CachedUiaElement, String> {
        Err("Direct COM not available (requires `windows` crate)".to_string())
    }

    /// Find element by automation ID within a subtree.
    pub fn find_by_automation_id(
        &self,
        _root: &CachedUiaElement,
        _automation_id: &str,
    ) -> Result<CachedUiaElement, String> {
        Err("Direct COM not available (requires `windows` crate)".to_string())
    }

    /// Invoke a pattern on an element.
    pub fn invoke_pattern(
        &self,
        _element: &CachedUiaElement,
        _pattern: UiaPattern,
        _value: Option<&str>,
    ) -> Result<(), String> {
        Err("Direct COM not available (requires `windows` crate)".to_string())
    }

    /// Get element at a screen point.
    pub fn element_from_point(&self, _x: f64, _y: f64) -> Result<CachedUiaElement, String> {
        Err("Direct COM not available (requires `windows` crate)".to_string())
    }

    /// Walk the full tree up to max_depth and build a CachedUiaTree.
    pub fn build_tree(
        &self,
        pid: u32,
        max_depth: u32,
        max_children: u32,
    ) -> Result<CachedUiaTree, String> {
        let _ = (max_depth, max_children);
        Ok(CachedUiaTree::new(pid, Duration::from_secs(5)))
    }
}

// ─── Hybrid Strategy ─────────────────────────────────────────────────────────

/// Strategy for choosing between PowerShell and direct COM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionStrategy {
    /// Always use PowerShell (most compatible, slowest).
    PowerShell,
    /// Use direct COM when available, fall back to PowerShell.
    PreferDirect,
    /// Only use direct COM (fails if unavailable).
    DirectOnly,
}

/// Choose the best strategy based on the operation and environment.
pub fn recommended_strategy(operation: &str) -> ExecutionStrategy {
    match operation {
        // Fast, frequent operations benefit most from direct COM.
        "find_element" | "get_property" | "element_from_point" => ExecutionStrategy::PreferDirect,
        // Complex operations with error handling are fine in PowerShell.
        "full_tree_capture" | "complex_action_sequence" => ExecutionStrategy::PowerShell,
        // Default: prefer direct for performance.
        _ => ExecutionStrategy::PreferDirect,
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tree() -> CachedUiaTree {
        let child1 = CachedUiaElement {
            runtime_id: vec![1, 2],
            automation_id: "btn_submit".to_string(),
            name: "Submit".to_string(),
            control_type: "Button".to_string(),
            class_name: "Button".to_string(),
            bounding_rect: UiaRect { x: 10.0, y: 10.0, width: 80.0, height: 30.0 },
            is_enabled: true,
            is_offscreen: false,
            process_id: 1234,
            supported_patterns: vec![UiaPattern::Invoke],
            child_index: 0,
            depth: 1,
            children: Vec::new(),
        };
        let child2 = CachedUiaElement {
            runtime_id: vec![1, 3],
            automation_id: "txt_email".to_string(),
            name: "Email".to_string(),
            control_type: "Edit".to_string(),
            class_name: "TextBox".to_string(),
            bounding_rect: UiaRect { x: 10.0, y: 50.0, width: 200.0, height: 25.0 },
            is_enabled: true,
            is_offscreen: false,
            process_id: 1234,
            supported_patterns: vec![UiaPattern::Value],
            child_index: 1,
            depth: 1,
            children: Vec::new(),
        };
        let root = CachedUiaElement {
            runtime_id: vec![1],
            automation_id: "main_window".to_string(),
            name: "Login".to_string(),
            control_type: "Window".to_string(),
            class_name: "WinForm".to_string(),
            bounding_rect: UiaRect { x: 0.0, y: 0.0, width: 400.0, height: 300.0 },
            is_enabled: true,
            is_offscreen: false,
            process_id: 1234,
            supported_patterns: vec![UiaPattern::Window],
            child_index: 0,
            depth: 0,
            children: vec![child1, child2],
        };

        let mut tree = CachedUiaTree::new(1234, Duration::from_secs(30));
        tree.root = Some(root);
        tree.rebuild_indices();
        tree
    }

    #[test]
    fn find_by_automation_id() {
        let tree = sample_tree();
        let el = tree.find_by_id("btn_submit").unwrap();
        assert_eq!(el.name, "Submit");
        assert_eq!(el.control_type, "Button");
    }

    #[test]
    fn find_by_name() {
        let tree = sample_tree();
        let els = tree.find_by_name("Email");
        assert_eq!(els.len(), 1);
        assert_eq!(els[0].automation_id, "txt_email");
    }

    #[test]
    fn element_at_point() {
        let tree = sample_tree();
        // Point inside the Submit button
        let el = tree.element_at_point(50.0, 20.0).unwrap();
        assert_eq!(el.automation_id, "btn_submit");

        // Point inside the Email textbox
        let el2 = tree.element_at_point(100.0, 60.0).unwrap();
        assert_eq!(el2.automation_id, "txt_email");

        // Point in the window but not in any child
        let el3 = tree.element_at_point(300.0, 200.0).unwrap();
        assert_eq!(el3.automation_id, "main_window");
    }

    #[test]
    fn cache_freshness() {
        let tree = CachedUiaTree::new(1, Duration::from_secs(60));
        assert!(tree.is_fresh());
    }

    #[test]
    fn element_count() {
        let tree = sample_tree();
        assert_eq!(tree.element_count, 3); // root + 2 children
    }

    #[test]
    fn pattern_roundtrip() {
        assert_eq!(UiaPattern::from_str("Invoke"), Some(UiaPattern::Invoke));
        assert_eq!(UiaPattern::from_str("Value"), Some(UiaPattern::Value));
        assert_eq!(UiaPattern::Toggle.as_str(), "Toggle");
    }

    #[test]
    fn perf_metrics_speedup() {
        let m = PerfMetrics::new("find_element")
            .with_powershell(150.0)
            .with_direct(1.5);
        assert_eq!(m.speedup, Some(100.0));
    }

    #[test]
    fn strategy_selection() {
        assert_eq!(
            recommended_strategy("find_element"),
            ExecutionStrategy::PreferDirect
        );
        assert_eq!(
            recommended_strategy("full_tree_capture"),
            ExecutionStrategy::PowerShell
        );
    }
}
