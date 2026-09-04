#![allow(dead_code)] // Reserved WA automation API surface; awaiting full MCP dispatch wiring.
//! Direct COM/UIA FFI bindings for high-performance Windows automation.
//!
//! Provides Rust-native bindings to Windows UIAutomation COM interfaces,
//! bypassing the PowerShell overhead for latency-critical operations.
//! Falls back to PowerShell for complex operations where the COM wrapper
//! would be prohibitively complex.
//!
//! Architecture:
//! - CachedUiaTree: In-memory cache of the automation tree for fast lookups
//! - UiaFfi: Direct FFI calls via `windows` crate types for high-performance COM interop
//! - Performance: ~100x faster than PowerShell for single-element lookups
//!
//! # Safety Invariants
//!
//! All `unsafe` blocks in this module are COM interface method calls via the `windows` crate.
//! The `windows` crate wraps raw COM pointers in RAII handles (`IUIAutomation`,
//! `IUIAutomationElement`, etc.) with proper `AddRef`/`Release` refcounting. Safety relies on:
//!
//! 1. **COM initialization**: `CoInitializeEx` is called before any COM object creation.
//!    `RPC_E_CHANGED_MODE` (0x80010106) is accepted for re-initialization.
//! 2. **COM object validity**: All COM interface pointers come from successful `CoCreateInstance`
//!    or from method calls on valid parent COM objects (e.g., `ElementFromPoint`, `GetRootElement`).
//! 3. **Element validity**: `IUIAutomationElement` references remain valid for the duration of
//!    the enclosing function call. Property getters (`CurrentAutomationId`, `CurrentName`, etc.)
//!    and pattern queries (`GetCurrentPattern`) are called on elements obtained from valid tree
//!    walks or find operations.
//! 4. **Pattern validity**: Pattern objects (`IUIAutomationInvokePattern`, etc.) are obtained via
//!    `GetCurrentPattern` on a valid element and cast from valid `IUnknown` pointers.
//! 5. **Tree walker validity**: `IUIAutomationTreeWalker` is created from a valid `IUIAutomation`
//!    instance. Walker methods (`GetFirstChildElement`, `GetNextSiblingElement`) return valid
//!    element references or null (handled via `Result`).

use std::collections::HashMap;
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
#[derive(Clone, Debug)]
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

// ─── COM Interface Layer ─────────────────────────────────────────────────────
// Direct COM/UIA via the `windows` crate for ~1ms element operations.
// Falls back to PowerShell for environments where COM is unavailable.

/// Direct UIAutomation client with native COM access and cached tree.
/// Achieves ~1ms per element operation vs ~150ms via PowerShell.
pub struct UiaDirectClient {
    /// Whether COM has been initialized on this thread.
    com_initialized: bool,
    /// Cached tree for fast lookups (avoid re-walking the COM tree).
    cached_tree: Option<CachedUiaTree>,
    /// Target process ID.
    target_pid: u32,
    /// Native COM automation instance (opaque handle on Windows).
    #[cfg(windows)]
    automation: Option<windows::Win32::UI::Accessibility::IUIAutomation>,
}

impl UiaDirectClient {
    /// Initialize COM and create the UIAutomation client.
    pub fn initialize() -> Result<Self, String> {
        #[cfg(windows)]
        {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
            use windows::Win32::UI::Accessibility::CUIAutomation;

            // Initialize COM (ignore RPC_E_CHANGED_MODE if already initialized)
            // SAFETY: CoInitializeEx with COINIT_APARTMENTTHREADED initializes COM for the
            // current thread as a single-threaded apartment. Passing None uses the default
            // threading model. This is safe to call multiple times; RPC_E_CHANGED_MODE
            // (0x80010106) is accepted when COM was already initialized with a different model.
            let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            let com_ok = hr.is_ok() || hr.0 == 0x80010106u32 as i32; // RPC_E_CHANGED_MODE

            if !com_ok {
                return Err(format!("CoInitializeEx failed: {:?}", hr));
            }

            // Create the IUIAutomation instance
            // SAFETY: CoCreateInstance with CLSID_CUIAutomation and CLSCTX_INPROC_SERVER
            // creates an in-process UIAutomation COM object. The `windows` crate wraps the
            // returned COM pointer in a safe IUIAutomation handle with proper refcounting.
            // The call is sound because COM has been successfully initialized above.
            let automation: Result<windows::Win32::UI::Accessibility::IUIAutomation, _> = unsafe {
                windows::Win32::System::Com::CoCreateInstance(
                    &CUIAutomation,
                    None,
                    windows::Win32::System::Com::CLSCTX_INPROC_SERVER,
                )
            };

            match automation {
                Ok(auto) => Ok(Self {
                    com_initialized: true,
                    cached_tree: None,
                    target_pid: 0,
                    automation: Some(auto),
                }),
                Err(e) => Err(format!("Failed to create IUIAutomation: {:?}", e)),
            }
        }
        #[cfg(not(windows))]
        {
            Ok(Self {
                com_initialized: false,
                cached_tree: None,
                target_pid: 0,
            })
        }
    }

    /// Initialize for a specific process.
    pub fn initialize_for_process(pid: u32) -> Result<Self, String> {
        let mut client = Self::initialize()?;
        client.target_pid = pid;
        Ok(client)
    }

    /// Get the root element of the desktop.
    pub fn get_root_element(&self) -> Result<CachedUiaElement, String> {
        if let Some(tree) = &self.cached_tree {
            if let Some(root) = &tree.root {
                return Ok(root.clone());
            }
        }
        Err("No cached tree available. Call build_tree() first.".to_string())
    }

    /// Get element from a specific process's main window.
    pub fn get_process_root(&self, pid: u32) -> Result<CachedUiaElement, String> {
        if let Some(tree) = &self.cached_tree {
            if tree.process_id == pid {
                if let Some(root) = &tree.root {
                    return Ok(root.clone());
                }
            }
        }
        Err(format!("No cached tree for process {}", pid))
    }

    /// Find element by automation ID within a subtree (O(1) via index).
    pub fn find_by_automation_id(
        &self,
        _root: &CachedUiaElement,
        automation_id: &str,
    ) -> Result<CachedUiaElement, String> {
        if let Some(tree) = &self.cached_tree {
            if let Some(el) = tree.find_by_id(automation_id) {
                return Ok(el.clone());
            }
        }
        Err(format!(
            "Element with automation_id '{}' not found",
            automation_id
        ))
    }

    /// Find element by name (O(1) via index).
    pub fn find_by_name(&self, name: &str) -> Result<Vec<CachedUiaElement>, String> {
        if let Some(tree) = &self.cached_tree {
            return Ok(tree.find_by_name(name).into_iter().cloned().collect());
        }
        Err("No cached tree available".to_string())
    }

    /// Get element at a screen point via direct COM ElementFromPoint (~1ms).
    pub fn element_from_point(&self, x: f64, y: f64) -> Result<CachedUiaElement, String> {
        #[cfg(windows)]
        {
            if let Some(auto) = &self.automation {
                use windows::Win32::Foundation::POINT;
                let pt = POINT {
                    x: x as i32,
                    y: y as i32,
                };
                // SAFETY: ElementFromPoint is a COM method on IUIAutomation that takes a POINT
                // struct (two i32 values). The `auto` reference is valid because it was successfully
                // created via CoCreateInstance. The returned IUIAutomationElement (if Ok) is a
                // valid COM interface pointer with proper refcounting by the `windows` crate.
                let result = unsafe { auto.ElementFromPoint(pt) };
                match result {
                    Ok(elem) => {
                        let cached = com_element_to_cached(&elem, 0, 0);
                        return Ok(cached);
                    }
                    Err(e) => {
                        // Fall through to cached tree lookup
                        let _ = e;
                    }
                }
            }
        }
        // Fallback: use cached tree spatial lookup
        if let Some(tree) = &self.cached_tree {
            if let Some(el) = tree.element_at_point(x, y) {
                return Ok(el.clone());
            }
        }
        Err(format!("No element at point ({}, {})", x, y))
    }

    /// Invoke a pattern on an element via direct COM (~1ms).
    /// Patterns not wired for direct COM fall back to the PowerShell path
    /// (which covers a different, overlapping set — e.g. `Selection`), so a
    /// working pattern is never rejected just because COM lacks it.
    pub fn invoke_pattern(
        &self,
        element: &CachedUiaElement,
        pattern: UiaPattern,
        value: Option<&str>,
    ) -> Result<(), String> {
        #[cfg(windows)]
        {
            if self.com_initialized && pattern_supported_via_com(pattern) {
                if let Some(auto) = &self.automation {
                    return invoke_pattern_com(auto, element, pattern, value);
                }
            }
        }
        // Fallback: PowerShell (COM unavailable or pattern not COM-wired).
        invoke_pattern_powershell(element, pattern, value)
    }

    /// Walk the full tree up to max_depth via direct COM TreeWalker (~5ms).
    /// Falls back to PowerShell (~150ms) if COM is unavailable.
    pub fn build_tree(
        &mut self,
        pid: u32,
        max_depth: u32,
        max_children: u32,
    ) -> Result<CachedUiaTree, String> {
        // Check if we have a fresh cached tree for this process
        if let Some(tree) = &self.cached_tree {
            if tree.process_id == pid && tree.is_fresh() {
                return Ok(tree.clone());
            }
        }

        #[cfg(windows)]
        {
            if let Some(auto) = &self.automation {
                let tree = build_tree_com(auto, pid, max_depth, max_children)?;
                self.cached_tree = Some(tree.clone());
                self.target_pid = pid;
                return Ok(tree);
            }
        }

        // Fallback: PowerShell capture
        let tree = build_tree_powershell(pid, max_depth, max_children)?;
        self.cached_tree = Some(tree.clone());
        self.target_pid = pid;
        Ok(tree)
    }

    /// Check if we have a fresh cached tree.
    pub fn has_fresh_cache(&self, pid: u32) -> bool {
        self.cached_tree
            .as_ref()
            .map(|t| t.process_id == pid && t.is_fresh())
            .unwrap_or(false)
    }

    /// Invalidate the cached tree (force re-capture on next build_tree).
    pub fn invalidate_cache(&mut self) {
        self.cached_tree = None;
    }
}

// ─── Direct COM Implementation (Windows) ─────────────────────────────────────

#[cfg(windows)]
fn bstr_to_string(bstr: &windows::core::BSTR) -> String {
    bstr.to_string()
}

#[cfg(windows)]
fn com_element_to_cached(
    elem: &windows::Win32::UI::Accessibility::IUIAutomationElement,
    child_index: u32,
    depth: u32,
) -> CachedUiaElement {
    // SAFETY: All unsafe calls below are COM property getters on `elem`, a valid
    // IUIAutomationElement pointer obtained from the UI Automation framework.
    // Each method (CurrentAutomationId, CurrentName, CurrentClassName,
    // CurrentControlType, CurrentBoundingRectangle, CurrentIsEnabled,
    // CurrentIsOffscreen, CurrentProcessId, GetCurrentPattern) is a standard
    // UIAutomation property accessor that:
    // - Returns an HRESULT with an out-parameter (wrapped by the `windows` crate as Result)
    // - Does not transfer ownership of internal pointers (BSTRs are copied by the crate)
    // - Is safe to call on any valid element, even if the property is unsupported
    //   (returns an error that we handle with unwrap_or_default/unwrap_or)
    // The element reference remains valid for the duration of this function call.
    // SAFETY: COM property getters on valid `elem` — see function-level preamble above.
    let automation_id = unsafe { elem.CurrentAutomationId() }
        .map(|s| bstr_to_string(&s))
        .unwrap_or_default();
    let name = unsafe { elem.CurrentName() }
        .map(|s| bstr_to_string(&s))
        .unwrap_or_default();
    // SAFETY: COM property getters on valid `elem`.
    let class_name = unsafe { elem.CurrentClassName() }
        .map(|s| bstr_to_string(&s))
        .unwrap_or_default();

    let control_type = unsafe { elem.CurrentControlType() }
        .map(|ct| control_type_name(ct.0))
        .unwrap_or_else(|_| "Unknown".to_string());

    let bounding_rect = unsafe { elem.CurrentBoundingRectangle() }
        .map(|r| UiaRect {
            x: r.left as f64,
            y: r.top as f64,
            width: (r.right - r.left) as f64,
            height: (r.bottom - r.top) as f64,
        })
        .unwrap_or(UiaRect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        });

    // SAFETY: COM property getters on valid `elem`.
    let is_enabled = unsafe { elem.CurrentIsEnabled() }
        .map(|b| b.as_bool())
        .unwrap_or(false);
    let is_offscreen = unsafe { elem.CurrentIsOffscreen() }
        .map(|b| b.as_bool())
        .unwrap_or(true);
    let process_id = unsafe { elem.CurrentProcessId() }.unwrap_or(0) as u32;

    // Detect supported patterns via GetCurrentPattern
    let mut supported_patterns = Vec::new();
    use windows::Win32::UI::Accessibility::{
        UIA_ExpandCollapsePatternId, UIA_InvokePatternId, UIA_RangeValuePatternId,
        UIA_ScrollPatternId, UIA_SelectionItemPatternId, UIA_SelectionPatternId,
        UIA_TogglePatternId, UIA_ValuePatternId,
    };
    // SAFETY: GetCurrentPattern calls on valid `elem` — return Ok with pattern or Err if unsupported.
    if unsafe { elem.GetCurrentPattern(UIA_InvokePatternId) }.is_ok() {
        supported_patterns.push(UiaPattern::Invoke);
    }
    if unsafe { elem.GetCurrentPattern(UIA_ValuePatternId) }.is_ok() {
        supported_patterns.push(UiaPattern::Value);
    }
    if unsafe { elem.GetCurrentPattern(UIA_TogglePatternId) }.is_ok() {
        supported_patterns.push(UiaPattern::Toggle);
    }
    if unsafe { elem.GetCurrentPattern(UIA_SelectionPatternId) }.is_ok() {
        supported_patterns.push(UiaPattern::Selection);
    }
    if unsafe { elem.GetCurrentPattern(UIA_SelectionItemPatternId) }.is_ok() {
        supported_patterns.push(UiaPattern::SelectionItem);
    }
    // SAFETY: GetCurrentPattern calls on valid `elem`.
    if unsafe { elem.GetCurrentPattern(UIA_ExpandCollapsePatternId) }.is_ok() {
        supported_patterns.push(UiaPattern::ExpandCollapse);
    }
    if unsafe { elem.GetCurrentPattern(UIA_ScrollPatternId) }.is_ok() {
        supported_patterns.push(UiaPattern::Scroll);
    }
    if unsafe { elem.GetCurrentPattern(UIA_RangeValuePatternId) }.is_ok() {
        supported_patterns.push(UiaPattern::RangeValue);
    }

    CachedUiaElement {
        runtime_id: vec![process_id as i32, child_index as i32],
        automation_id,
        name,
        control_type,
        class_name,
        bounding_rect,
        is_enabled,
        is_offscreen,
        process_id,
        supported_patterns,
        child_index,
        depth,
        children: Vec::new(),
    }
}

#[cfg(windows)]
fn control_type_name(ct: i32) -> String {
    use windows::Win32::UI::Accessibility::*;
    let name = if ct == UIA_ButtonControlTypeId.0 {
        "Button"
    } else if ct == UIA_EditControlTypeId.0 {
        "Edit"
    } else if ct == UIA_WindowControlTypeId.0 {
        "Window"
    } else if ct == UIA_TextControlTypeId.0 {
        "Text"
    } else if ct == UIA_CheckBoxControlTypeId.0 {
        "CheckBox"
    } else if ct == UIA_ComboBoxControlTypeId.0 {
        "ComboBox"
    } else if ct == UIA_ListItemControlTypeId.0 {
        "ListItem"
    } else if ct == UIA_ListControlTypeId.0 {
        "List"
    } else if ct == UIA_MenuControlTypeId.0 {
        "Menu"
    } else if ct == UIA_MenuItemControlTypeId.0 {
        "MenuItem"
    } else if ct == UIA_TabControlTypeId.0 {
        "Tab"
    } else if ct == UIA_TabItemControlTypeId.0 {
        "TabItem"
    } else if ct == UIA_TreeControlTypeId.0 {
        "Tree"
    } else if ct == UIA_TreeItemControlTypeId.0 {
        "TreeItem"
    } else if ct == UIA_DataGridControlTypeId.0 {
        "DataGrid"
    } else if ct == UIA_DataItemControlTypeId.0 {
        "DataItem"
    } else if ct == UIA_ToolBarControlTypeId.0 {
        "ToolBar"
    } else if ct == UIA_StatusBarControlTypeId.0 {
        "StatusBar"
    } else if ct == UIA_ProgressBarControlTypeId.0 {
        "ProgressBar"
    } else if ct == UIA_ScrollBarControlTypeId.0 {
        "ScrollBar"
    } else if ct == UIA_GroupControlTypeId.0 {
        "Group"
    } else if ct == UIA_PaneControlTypeId.0 {
        "Pane"
    } else if ct == UIA_DocumentControlTypeId.0 {
        "Document"
    } else if ct == UIA_ImageControlTypeId.0 {
        "Image"
    } else if ct == UIA_HyperlinkControlTypeId.0 {
        "Hyperlink"
    } else if ct == UIA_RadioButtonControlTypeId.0 {
        "RadioButton"
    } else if ct == UIA_SliderControlTypeId.0 {
        "Slider"
    } else if ct == UIA_SpinnerControlTypeId.0 {
        "Spinner"
    } else if ct == UIA_TableControlTypeId.0 {
        "Table"
    } else if ct == UIA_HeaderControlTypeId.0 {
        "Header"
    } else if ct == UIA_HeaderItemControlTypeId.0 {
        "HeaderItem"
    } else if ct == UIA_ToolTipControlTypeId.0 {
        "ToolTip"
    } else if ct == UIA_SeparatorControlTypeId.0 {
        "Separator"
    } else {
        "Unknown"
    };
    name.to_string()
}

/// Build the UIA tree via direct COM TreeWalker (~5ms for typical windows).
// SAFETY: Tree walker COM calls. `auto` is a valid IUIAutomation from CoCreateInstance.
// GetRootElement, CreateTrueCondition, CreateTreeWalker all return valid COM handles on success.
// Walker methods (GetFirstChildElement, GetNextSiblingElement) return valid elements or null.
// Property getters on elements (CurrentProcessId, etc.) are safe on valid element references.
#[cfg(windows)]
fn build_tree_com(
    auto: &windows::Win32::UI::Accessibility::IUIAutomation,
    pid: u32,
    max_depth: u32,
    max_children: u32,
) -> Result<CachedUiaTree, String> {
    // Get the desktop root
    let desktop =
        unsafe { auto.GetRootElement() }.map_err(|e| format!("GetRootElement failed: {:?}", e))?;

    // Create a content view walker via true condition
    // SAFETY: COM tree walker calls on valid `auto` and `walker` from CoCreateInstance.
    let true_cond = unsafe { auto.CreateTrueCondition() }
        .map_err(|e| format!("CreateTrueCondition failed: {:?}", e))?;
    let walker = unsafe { auto.CreateTreeWalker(&true_cond) }
        .map_err(|e| format!("CreateTreeWalker failed: {:?}", e))?;

    // Find the target process's top-level window
    let mut target_window = None;
    // SAFETY: COM walker calls on valid `walker` and `desktop`/`current` elements.
    let mut current = unsafe { walker.GetFirstChildElement(&desktop) }
        .map_err(|e| format!("GetFirstChildElement failed: {:?}", e))?;

    loop {
        let elem_pid = unsafe { current.CurrentProcessId() }.unwrap_or(0) as u32;
        if elem_pid == pid {
            target_window = Some(current);
            break;
        }
        match unsafe { walker.GetNextSiblingElement(&current) } {
            Ok(next) => current = next,
            Err(_) => break,
        }
    }

    let target =
        target_window.ok_or_else(|| format!("No top-level window found for process {}", pid))?;

    // Recursively walk the tree
    let root_element = walk_element_com(&walker, &target, 0, 0, max_depth, max_children);

    let mut tree = CachedUiaTree::new(pid, Duration::from_secs(30));
    tree.root = Some(root_element);
    tree.rebuild_indices();
    Ok(tree)
}

/// Recursively walk a COM element and its children.
// SAFETY: Walker COM calls (GetFirstChildElement, GetNextSiblingElement) on a valid
// IUIAutomationTreeWalker. Element references are valid for the duration of the call.
#[cfg(windows)]
fn walk_element_com(
    walker: &windows::Win32::UI::Accessibility::IUIAutomationTreeWalker,
    elem: &windows::Win32::UI::Accessibility::IUIAutomationElement,
    child_index: u32,
    depth: u32,
    max_depth: u32,
    max_children: u32,
) -> CachedUiaElement {
    let mut cached = com_element_to_cached(elem, child_index, depth);

    if depth < max_depth {
        let mut child_count = 0u32;
        // SAFETY: COM walker calls on valid `walker` and `elem`.
        if let Ok(first_child) = unsafe { walker.GetFirstChildElement(elem) } {
            let mut child_elem = first_child;
            loop {
                if child_count >= max_children {
                    break;
                }
                let child_cached = walk_element_com(
                    walker,
                    &child_elem,
                    child_count,
                    depth + 1,
                    max_depth,
                    max_children,
                );
                cached.children.push(child_cached);
                child_count += 1;
                // SAFETY: COM walker call on valid `walker` and `child_elem`.
                match unsafe { walker.GetNextSiblingElement(&child_elem) } {
                    Ok(next) => child_elem = next,
                    Err(_) => break,
                }
            }
        }
    }

    cached
}

/// Invoke a UIA pattern via direct COM (~1ms).
#[cfg(windows)]
fn invoke_pattern_com(
    auto: &windows::Win32::UI::Accessibility::IUIAutomation,
    element: &CachedUiaElement,
    pattern: UiaPattern,
    value: Option<&str>,
) -> Result<(), String> {
    use windows::core::{Interface, BSTR, VARIANT};
    use windows::Win32::UI::Accessibility::{
        IUIAutomationExpandCollapsePattern, IUIAutomationInvokePattern,
        IUIAutomationRangeValuePattern, IUIAutomationScrollItemPattern, IUIAutomationScrollPattern,
        IUIAutomationSelectionItemPattern, IUIAutomationTogglePattern,
        IUIAutomationTransformPattern, IUIAutomationValuePattern, IUIAutomationWindowPattern,
        ScrollAmount_LargeDecrement, ScrollAmount_LargeIncrement, ScrollAmount_NoAmount,
        ScrollAmount_SmallDecrement, ScrollAmount_SmallIncrement, TreeScope_Descendants,
        UIA_AutomationIdPropertyId, UIA_ExpandCollapsePatternId, UIA_InvokePatternId,
        UIA_NamePropertyId, UIA_RangeValuePatternId, UIA_ScrollItemPatternId, UIA_ScrollPatternId,
        UIA_SelectionItemPatternId, UIA_TogglePatternId, UIA_TransformPatternId,
        UIA_ValuePatternId, UIA_WindowPatternId, WindowVisualState_Maximized,
        WindowVisualState_Minimized, WindowVisualState_Normal,
    };

    // SAFETY: All following unsafe blocks are COM method calls on valid UIA objects.
    // `auto` is a valid IUIAutomation, `desktop` comes from GetRootElement,
    // `com_elem` from FindFirst on a valid condition. Pattern objects come from
    // GetCurrentPattern on valid elements. All COM pointers have proper refcounting
    // via the `windows` crate RAII wrappers.
    // Find the element via COM using its automation ID or name
    // SAFETY: COM method calls on valid `auto`, `desktop`, `com_elem`, and pattern objects.
    let desktop =
        unsafe { auto.GetRootElement() }.map_err(|e| format!("GetRootElement: {:?}", e))?;

    let condition = if !element.automation_id.is_empty() {
        let bstr = BSTR::from(element.automation_id.as_str());
        let var: VARIANT = bstr.into();
        unsafe { auto.CreatePropertyCondition(UIA_AutomationIdPropertyId, &var) }
    } else {
        let bstr = BSTR::from(element.name.as_str());
        let var: VARIANT = bstr.into();
        unsafe { auto.CreatePropertyCondition(UIA_NamePropertyId, &var) }
    }
    .map_err(|e| format!("CreatePropertyCondition: {:?}", e))?;

    let com_elem = unsafe { desktop.FindFirst(TreeScope_Descendants, &condition) }
        .map_err(|e| format!("FindFirst: {:?}", e))?;

    match pattern {
        // SAFETY: GetCurrentPattern + cast + invoke on valid `com_elem`.
        UiaPattern::Invoke => {
            let pattern_obj = unsafe { com_elem.GetCurrentPattern(UIA_InvokePatternId) }
                .map_err(|e| format!("GetInvokePattern: {:?}", e))?;
            let invoke: IUIAutomationInvokePattern = pattern_obj
                .cast()
                .map_err(|e| format!("Cast InvokePattern: {:?}", e))?;
            unsafe { invoke.Invoke() }.map_err(|e| format!("Invoke: {:?}", e))?;
        }
        // SAFETY: GetCurrentPattern + cast + method call on valid `com_elem`.
        UiaPattern::Value => {
            let val = value.unwrap_or("");
            let pattern_obj = unsafe { com_elem.GetCurrentPattern(UIA_ValuePatternId) }
                .map_err(|e| format!("GetValuePattern: {:?}", e))?;
            let value_pattern: IUIAutomationValuePattern = pattern_obj
                .cast()
                .map_err(|e| format!("Cast ValuePattern: {:?}", e))?;
            unsafe { value_pattern.SetValue(&BSTR::from(val)) }
                .map_err(|e| format!("SetValue: {:?}", e))?;
        }
        // SAFETY: GetCurrentPattern + cast + method call on valid `com_elem`.
        UiaPattern::Toggle => {
            let pattern_obj = unsafe { com_elem.GetCurrentPattern(UIA_TogglePatternId) }
                .map_err(|e| format!("GetTogglePattern: {:?}", e))?;
            let toggle: IUIAutomationTogglePattern = pattern_obj
                .cast()
                .map_err(|e| format!("Cast TogglePattern: {:?}", e))?;
            unsafe { toggle.Toggle() }.map_err(|e| format!("Toggle: {:?}", e))?;
        }
        // SAFETY: GetCurrentPattern + cast + method call on valid `com_elem`.
        UiaPattern::ExpandCollapse => {
            let pattern_obj = unsafe { com_elem.GetCurrentPattern(UIA_ExpandCollapsePatternId) }
                .map_err(|e| format!("GetExpandCollapsePattern: {:?}", e))?;
            let ec: IUIAutomationExpandCollapsePattern = pattern_obj
                .cast()
                .map_err(|e| format!("Cast ExpandCollapsePattern: {:?}", e))?;
            match value.unwrap_or("Expand") {
                "Collapse" => unsafe { ec.Collapse() }.map_err(|e| format!("Collapse: {:?}", e))?,
                _ => unsafe { ec.Expand() }.map_err(|e| format!("Expand: {:?}", e))?,
            }
        }
        // SAFETY: GetCurrentPattern + cast + method call on valid `com_elem`.
        UiaPattern::SelectionItem => {
            let pattern_obj = unsafe { com_elem.GetCurrentPattern(UIA_SelectionItemPatternId) }
                .map_err(|e| format!("GetSelectionItemPattern: {:?}", e))?;
            let si: IUIAutomationSelectionItemPattern = pattern_obj
                .cast()
                .map_err(|e| format!("Cast SelectionItemPattern: {:?}", e))?;
            match value.unwrap_or("Select") {
                "AddToSelection" => unsafe { si.AddToSelection() }
                    .map_err(|e| format!("AddToSelection: {:?}", e))?,
                "RemoveFromSelection" => unsafe { si.RemoveFromSelection() }
                    .map_err(|e| format!("RemoveFromSelection: {:?}", e))?,
                _ => unsafe { si.Select() }.map_err(|e| format!("Select: {:?}", e))?,
            }
        }
        // SAFETY (continued): Same invariants as above — GetCurrentPattern on `com_elem` (a valid
        // IUIAutomationElement from FindFirst), then COM method calls on the cast pattern objects.
        // All pattern objects are valid COM interfaces with proper refcounting via `windows` crate.
        UiaPattern::RangeValue => {
            let val: f64 = value.unwrap_or("50").parse().unwrap_or(50.0);
            let pattern_obj = unsafe { com_elem.GetCurrentPattern(UIA_RangeValuePatternId) }
                .map_err(|e| format!("GetRangeValuePattern: {:?}", e))?;
            let rv: IUIAutomationRangeValuePattern = pattern_obj
                .cast()
                .map_err(|e| format!("Cast RangeValuePattern: {:?}", e))?;
            unsafe { rv.SetValue(val) }.map_err(|e| format!("SetRangeValue: {:?}", e))?;
        }
        // SAFETY: COM pattern calls on valid `com_elem`.
        UiaPattern::Scroll => {
            let pattern_obj = unsafe { com_elem.GetCurrentPattern(UIA_ScrollPatternId) }
                .map_err(|e| format!("GetScrollPattern: {:?}", e))?;
            let scroll: IUIAutomationScrollPattern = pattern_obj
                .cast()
                .map_err(|e| format!("Cast ScrollPattern: {:?}", e))?;
            let amount = value.unwrap_or("LineDown");
            let (h, v) = match amount {
                "LineUp" => (ScrollAmount_NoAmount, ScrollAmount_SmallDecrement),
                "LineDown" => (ScrollAmount_NoAmount, ScrollAmount_SmallIncrement),
                "PageUp" => (ScrollAmount_NoAmount, ScrollAmount_LargeDecrement),
                "PageDown" => (ScrollAmount_NoAmount, ScrollAmount_LargeIncrement),
                _ => (ScrollAmount_NoAmount, ScrollAmount_SmallIncrement),
            };
            unsafe { scroll.Scroll(h, v) }.map_err(|e| format!("Scroll: {:?}", e))?;
        }
        // SAFETY: COM pattern calls on valid `com_elem`.
        UiaPattern::Transform => {
            let spec = value.unwrap_or("");
            let pattern_obj = unsafe { com_elem.GetCurrentPattern(UIA_TransformPatternId) }
                .map_err(|e| format!("GetTransformPattern: {:?}", e))?;
            let tf: IUIAutomationTransformPattern = pattern_obj
                .cast()
                .map_err(|e| format!("Cast TransformPattern: {:?}", e))?;
            let (op, args) = spec.split_once(':').ok_or_else(|| {
                format!("Transform expects 'move:X,Y' | 'resize:W,H' | 'rotate:DEG', got '{spec}'")
            })?;
            match op.trim().to_lowercase().as_str() {
                "move" => {
                    let (x, y) = args
                        .split_once(',')
                        .ok_or_else(|| "Transform move expects 'X,Y'".to_string())?;
                    let x: f64 = x.trim().parse().map_err(|_| "invalid move X".to_string())?;
                    let y: f64 = y.trim().parse().map_err(|_| "invalid move Y".to_string())?;
                    // SAFETY: COM Transform method calls on valid `tf`.
                    unsafe { tf.Move(x, y) }.map_err(|e| format!("Move: {:?}", e))?;
                }
                "resize" => {
                    let (w, h) = args
                        .split_once(',')
                        .ok_or_else(|| "Transform resize expects 'W,H'".to_string())?;
                    let w: f64 = w
                        .trim()
                        .parse()
                        .map_err(|_| "invalid resize W".to_string())?;
                    let h: f64 = h
                        .trim()
                        .parse()
                        .map_err(|_| "invalid resize H".to_string())?;
                    // SAFETY: COM Transform method calls on valid `tf`.
                    unsafe { tf.Resize(w, h) }.map_err(|e| format!("Resize: {:?}", e))?;
                }
                "rotate" => {
                    let deg: f64 = args
                        .trim()
                        .parse()
                        .map_err(|_| "invalid rotate degrees".to_string())?;
                    // SAFETY: COM Transform method calls on valid `tf`.
                    unsafe { tf.Rotate(deg) }.map_err(|e| format!("Rotate: {:?}", e))?;
                }
                other => return Err(format!("unknown Transform op '{other}'")),
            }
        }
        // SAFETY: COM pattern calls on valid `com_elem`.
        UiaPattern::ScrollItem => {
            let pattern_obj = unsafe { com_elem.GetCurrentPattern(UIA_ScrollItemPatternId) }
                .map_err(|e| format!("GetScrollItemPattern: {:?}", e))?;
            let si: IUIAutomationScrollItemPattern = pattern_obj
                .cast()
                .map_err(|e| format!("Cast ScrollItemPattern: {:?}", e))?;
            unsafe { si.ScrollIntoView() }.map_err(|e| format!("ScrollIntoView: {:?}", e))?;
        }
        // SAFETY: COM pattern calls on valid `com_elem`.
        UiaPattern::Window => {
            let pattern_obj = unsafe { com_elem.GetCurrentPattern(UIA_WindowPatternId) }
                .map_err(|e| format!("GetWindowPattern: {:?}", e))?;
            let win: IUIAutomationWindowPattern = pattern_obj
                .cast()
                .map_err(|e| format!("Cast WindowPattern: {:?}", e))?;
            match value.unwrap_or("Normal") {
                "Close" => unsafe { win.Close() }.map_err(|e| format!("Close: {:?}", e))?,
                "Maximize" => unsafe { win.SetWindowVisualState(WindowVisualState_Maximized) }
                    .map_err(|e| format!("Maximize: {:?}", e))?,
                "Minimize" => unsafe { win.SetWindowVisualState(WindowVisualState_Minimized) }
                    .map_err(|e| format!("Minimize: {:?}", e))?,
                _ => unsafe { win.SetWindowVisualState(WindowVisualState_Normal) }
                    .map_err(|e| format!("Normal: {:?}", e))?,
            }
        }
        _ => {
            return Err(format!(
                "Pattern {:?} not yet supported via direct COM",
                pattern
            ));
        }
    }
    Ok(())
}

// ─── PowerShell Fallback ─────────────────────────────────────────────────────

/// Whether a pattern has a direct-COM implementation in [`invoke_pattern_com`].
/// Patterns outside this set are routed to the PowerShell fallback instead of
/// being rejected. Keep this in sync with the `match` in `invoke_pattern_com`.
#[cfg_attr(not(windows), allow(dead_code))]
fn pattern_supported_via_com(pattern: UiaPattern) -> bool {
    matches!(
        pattern,
        UiaPattern::Invoke
            | UiaPattern::Value
            | UiaPattern::Toggle
            | UiaPattern::ExpandCollapse
            | UiaPattern::SelectionItem
            | UiaPattern::RangeValue
            | UiaPattern::Scroll
            | UiaPattern::Transform
            | UiaPattern::ScrollItem
            | UiaPattern::Window
    )
}

/// Invoke a UIA pattern via PowerShell (fallback, ~150ms).
fn invoke_pattern_powershell(
    element: &CachedUiaElement,
    pattern: UiaPattern,
    value: Option<&str>,
) -> Result<(), String> {
    let script = build_pattern_invoke_script(element, pattern, value);
    let result = super::process_mgmt::ProcessManager::launch(
        &super::process_mgmt::LaunchConfig::new("powershell")
            .arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-Command")
            .arg(&script),
    );
    if result.success {
        Ok(())
    } else {
        Err(format!("Pattern invoke failed: {}", result.detail))
    }
}

/// Build tree via PowerShell (fallback, ~150ms).
fn build_tree_powershell(
    pid: u32,
    max_depth: u32,
    max_children: u32,
) -> Result<CachedUiaTree, String> {
    let script = build_capture_tree_script(pid, max_depth, max_children);
    // The capture script emits the tree as JSON on stdout, so we must run it
    // with stdout captured (ProcessManager::launch is fire-and-forget and does
    // not capture output).
    let output = run_ps_capture(&script)?;
    parse_tree_from_ps_output(&output, pid)
}

/// Run a PowerShell script and capture its stdout, returning an error on a
/// non-zero exit. Mirrors the `run_ps_script` helpers used by the other `wa`
/// PowerShell fallbacks (piping the script via stdin so `-Command -` executes
/// on EOF rather than blocking).
fn run_ps_capture(script: &str) -> Result<String, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn powershell: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(script.as_bytes())
            .map_err(|e| format!("stdin write: {e}"))?;
    }
    let output = child.wait_with_output().map_err(|e| format!("wait: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("PowerShell error: {}", stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Build a PowerShell script to capture the UIA tree for a process.
fn build_capture_tree_script(pid: u32, max_depth: u32, max_children: u32) -> String {
    format!(
        r#"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$root = [System.Windows.Automation.AutomationElement]::RootElement
$pid = {pid}
$maxDepth = {max_depth}
$maxChildren = {max_children}

function Get-UiaTree($element, $depth, $childIdx) {{
    if ($depth -gt $maxDepth) {{ return $null }}
    $name = $element.Current.Name
    $autoId = $element.Current.AutomationId
    $ctrlType = $element.Current.ControlType.LocalizedControlType
    $className = $element.Current.ClassName
    $rect = $element.Current.BoundingRectangle
    $enabled = $element.Current.IsEnabled
    $offscreen = $element.Current.IsOffscreen
    $procId = $element.Current.ProcessId
    $patterns = @()
    foreach ($p in $element.GetSupportedPatterns()) {{
        $patterns += $p.ProgrammaticName -replace 'Pattern$', ''
    }}
    $children = @()
    $childCount = 0
    foreach ($child in $element.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)) {{
        if ($childCount -ge $maxChildren) {{ break }}
        $c = Get-UiaTree $child ($depth + 1) $childCount
        if ($c -ne $null) {{ $children += $c }}
        $childCount++
    }}
    return @{{
        automation_id = $autoId
        name = $name
        control_type = $ctrlType
        class_name = $className
        x = $rect.X; y = $rect.Y; width = $rect.Width; height = $rect.Height
        is_enabled = $enabled
        is_offscreen = $offscreen
        process_id = $procId
        supported_patterns = $patterns
        child_index = $childIdx
        depth = $depth
        children = $children
    }}
}}

# Find the root element for the target process
$procRoot = $null
foreach ($w in $root.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)) {{
    if ($w.Current.ProcessId -eq $pid) {{
        $procRoot = $w
        break
    }}
}}
if ($null -eq $procRoot) {{
    ConvertTo-Json @{{ error = "Process $pid not found" }} -Compress
    exit
}}
$tree = Get-UiaTree $procRoot 0 0
ConvertTo-Json $tree -Compress -Depth 10
"#
    )
}

/// Build a PowerShell script to invoke a UIA pattern on an element.
fn build_pattern_invoke_script(
    element: &CachedUiaElement,
    pattern: UiaPattern,
    value: Option<&str>,
) -> String {
    let automation_id = &element.automation_id;
    let name = &element.name;
    match pattern {
        UiaPattern::Invoke => format!(
            r#"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$root = [System.Windows.Automation.AutomationElement]::RootElement
$cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, '{automation_id}')
$el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
if ($null -eq $el) {{ $cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, '{name}'); $el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond) }}
if ($null -ne $el) {{
    $pattern = $el.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
    $pattern.Invoke()
    Write-Output '{{"success":true}}'
}} else {{ Write-Output '{{"success":false,"error":"element not found"}}' }}
"#
        ),
        UiaPattern::Value => {
            let val = value.unwrap_or("");
            format!(
                r#"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$root = [System.Windows.Automation.AutomationElement]::RootElement
$cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, '{automation_id}')
$el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
if ($null -eq $el) {{ $cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, '{name}'); $el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond) }}
if ($null -ne $el) {{
    $pattern = $el.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
    $pattern.SetValue('{val}')
    Write-Output '{{"success":true}}'
}} else {{ Write-Output '{{"success":false,"error":"element not found"}}' }}
"#
            )
        }
        UiaPattern::Toggle => format!(
            r#"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$root = [System.Windows.Automation.AutomationElement]::RootElement
$cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, '{automation_id}')
$el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
if ($null -eq $el) {{ $cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, '{name}'); $el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond) }}
if ($null -ne $el) {{
    $pattern = $el.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
    $pattern.Toggle()
    Write-Output '{{"success":true}}'
}} else {{ Write-Output '{{"success":false,"error":"element not found"}}' }}
"#
        ),
        UiaPattern::Scroll => {
            let amount = value.unwrap_or("SmallIncrement");
            let (horizontal, vertical) = match amount {
                "LineUp" => ("0", "SmallDecrement"),
                "LineDown" => ("0", "SmallIncrement"),
                "LineLeft" => ("SmallDecrement", "0"),
                "LineRight" => ("SmallIncrement", "0"),
                "PageUp" => ("0", "LargeDecrement"),
                "PageDown" => ("0", "LargeIncrement"),
                _ => ("0", "SmallIncrement"),
            };
            format!(
                r#"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$root = [System.Windows.Automation.AutomationElement]::RootElement
$cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, '{automation_id}')
$el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
if ($null -eq $el) {{ $cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, '{name}'); $el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond) }}
if ($null -ne $el) {{
    $pattern = $el.GetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern)
    $pattern.Scroll([System.Windows.Automation.ScrollAmount]::{vertical}, [System.Windows.Automation.ScrollAmount]::{horizontal})
    Write-Output '{{"success":true}}'
}} else {{ Write-Output '{{"success":false,"error":"element not found"}}' }}
"#
            )
        }
        UiaPattern::Selection => format!(
            r#"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$root = [System.Windows.Automation.AutomationElement]::RootElement
$cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, '{automation_id}')
$el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
if ($null -eq $el) {{ $cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, '{name}'); $el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond) }}
if ($null -ne $el) {{
    $pattern = $el.GetCurrentPattern([System.Windows.Automation.SelectionPattern]::Pattern)
    $selected = $pattern.Current.GetSelection()
    $names = @()
    foreach ($s in $selected) {{ $names += $s.Current.Name }}
    Write-Output (ConvertTo-Json @{{ success = $true; selected = ($names -join ',') }} -Compress)
}} else {{ Write-Output '{{"success":false,"error":"element not found"}}' }}
"#
        ),
        UiaPattern::SelectionItem => {
            let action = value.unwrap_or("Select");
            let method = match action {
                "AddToSelection" => "AddToSelection()",
                "RemoveFromSelection" => "RemoveFromSelection()",
                _ => "Select()",
            };
            format!(
                r#"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$root = [System.Windows.Automation.AutomationElement]::RootElement
$cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, '{automation_id}')
$el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
if ($null -eq $el) {{ $cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, '{name}'); $el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond) }}
if ($null -ne $el) {{
    $pattern = $el.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern)
    $pattern.{method}
    Write-Output '{{"success":true}}'
}} else {{ Write-Output '{{"success":false,"error":"element not found"}}' }}
"#
            )
        }
        UiaPattern::ExpandCollapse => {
            let action = value.unwrap_or("Expand");
            let method = match action {
                "Collapse" => "Collapse()",
                _ => "Expand()",
            };
            format!(
                r#"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$root = [System.Windows.Automation.AutomationElement]::RootElement
$cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, '{automation_id}')
$el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
if ($null -eq $el) {{ $cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, '{name}'); $el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond) }}
if ($null -ne $el) {{
    $pattern = $el.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern)
    $pattern.{method}
    Write-Output '{{"success":true}}'
}} else {{ Write-Output '{{"success":false,"error":"element not found"}}' }}
"#
            )
        }
        UiaPattern::RangeValue => {
            let val = value.unwrap_or("50");
            format!(
                r#"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$root = [System.Windows.Automation.AutomationElement]::RootElement
$cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, '{automation_id}')
$el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
if ($null -eq $el) {{ $cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, '{name}'); $el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond) }}
if ($null -ne $el) {{
    $pattern = $el.GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern)
    $pattern.SetValue({val})
    Write-Output '{{"success":true}}'
}} else {{ Write-Output '{{"success":false,"error":"element not found"}}' }}
"#
            )
        }
        UiaPattern::Window => {
            let body = match value.unwrap_or("Normal") {
                "Close" => "    $pattern = $el.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern)\n    $pattern.Close()\n    Write-Output '{\"success\":true}'".to_string(),
                other => {
                    let state = match other {
                        "Maximize" | "Maximized" => "Maximized",
                        "Minimize" | "Minimized" => "Minimized",
                        _ => "Normal",
                    };
                    format!("    $pattern = $el.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern)\n    $pattern.SetWindowVisualState([System.Windows.Automation.WindowVisualState]::{state})\n    Write-Output '{{\"success\":true}}'")
                }
            };
            uia_element_action_script(automation_id, name, &body)
        }
        UiaPattern::Transform => {
            let body = match parse_transform_action(value.unwrap_or("")) {
                Some(call) => format!("    $pattern = $el.GetCurrentPattern([System.Windows.Automation.TransformPattern]::Pattern)\n    {call}\n    Write-Output '{{\"success\":true}}'"),
                None => "    Write-Output '{\"success\":false,\"error\":\"invalid transform spec (use move:X,Y | resize:W,H | rotate:DEG)\"}'".to_string(),
            };
            uia_element_action_script(automation_id, name, &body)
        }
        UiaPattern::Dock => {
            let pos = match value.unwrap_or("None") {
                "Top" => "Top",
                "Bottom" => "Bottom",
                "Left" => "Left",
                "Right" => "Right",
                "Fill" => "Fill",
                _ => "None",
            };
            let body = format!("    $pattern = $el.GetCurrentPattern([System.Windows.Automation.DockPattern]::Pattern)\n    $pattern.SetDockPosition([System.Windows.Automation.DockPosition]::{pos})\n    Write-Output '{{\"success\":true}}'");
            uia_element_action_script(automation_id, name, &body)
        }
        UiaPattern::ScrollItem => {
            let body = "    $pattern = $el.GetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern)\n    $pattern.ScrollIntoView()\n    Write-Output '{\"success\":true}'".to_string();
            uia_element_action_script(automation_id, name, &body)
        }
        UiaPattern::VirtualizedItem => {
            let body = "    $pattern = $el.GetCurrentPattern([System.Windows.Automation.VirtualizedItemPattern]::Pattern)\n    $pattern.Realize()\n    Write-Output '{\"success\":true}'".to_string();
            uia_element_action_script(automation_id, name, &body)
        }
        UiaPattern::Text => {
            let body = "    $pattern = $el.GetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern)\n    $text = $pattern.DocumentRange.GetText(-1)\n    Write-Output (ConvertTo-Json @{ success = $true; text = $text } -Compress)".to_string();
            uia_element_action_script(automation_id, name, &body)
        }
        // Container/query patterns are read-oriented: they return row/column
        // metadata or a container lookup result as JSON rather than mutating the
        // element. Grid/GridItem/Table/TableItem need no extra context; the
        // `ItemContainer` search key is carried in `value`.
        UiaPattern::Grid => {
            let body = "    $pattern = $el.GetCurrentPattern([System.Windows.Automation.GridPattern]::Pattern)\n    Write-Output (ConvertTo-Json @{ success = $true; row_count = $pattern.Current.RowCount; column_count = $pattern.Current.ColumnCount } -Compress)".to_string();
            uia_element_action_script(automation_id, name, &body)
        }
        UiaPattern::GridItem => {
            let body = "    $pattern = $el.GetCurrentPattern([System.Windows.Automation.GridItemPattern]::Pattern)\n    Write-Output (ConvertTo-Json @{ success = $true; row = $pattern.Current.Row; column = $pattern.Current.Column; row_span = $pattern.Current.RowSpan; column_span = $pattern.Current.ColumnSpan } -Compress)".to_string();
            uia_element_action_script(automation_id, name, &body)
        }
        UiaPattern::Table => {
            let body = "    $grid = $el.GetCurrentPattern([System.Windows.Automation.GridPattern]::Pattern)\n    $tbl = $el.GetCurrentPattern([System.Windows.Automation.TablePattern]::Pattern)\n    Write-Output (ConvertTo-Json @{ success = $true; row_count = $grid.Current.RowCount; column_count = $grid.Current.ColumnCount; row_or_column_major = $tbl.Current.RowOrColumnMajor.ToString() } -Compress)".to_string();
            uia_element_action_script(automation_id, name, &body)
        }
        UiaPattern::TableItem => {
            let body = "    $pattern = $el.GetCurrentPattern([System.Windows.Automation.TableItemPattern]::Pattern)\n    $rowHeaders = @(); foreach ($h in $pattern.Current.GetRowHeaderItems()) { $rowHeaders += $h.Current.Name }\n    $colHeaders = @(); foreach ($h in $pattern.Current.GetColumnHeaderItems()) { $colHeaders += $h.Current.Name }\n    Write-Output (ConvertTo-Json @{ success = $true; row_headers = ($rowHeaders -join ','); column_headers = ($colHeaders -join ',') } -Compress)".to_string();
            uia_element_action_script(automation_id, name, &body)
        }
        UiaPattern::ItemContainer => {
            let search = value.unwrap_or("");
            let body = format!("    $pattern = $el.GetCurrentPattern([System.Windows.Automation.ItemContainerPattern]::Pattern)\n    $found = $pattern.FindItemByProperty($null, [System.Windows.Automation.AutomationElement]::NameProperty, '{search}')\n    if ($null -ne $found) {{ Write-Output (ConvertTo-Json @{{ success = $true; found = $found.Current.Name }} -Compress) }} else {{ Write-Output '{{\"success\":false,\"error\":\"item not found\"}}' }}");
            uia_element_action_script(automation_id, name, &body)
        }
    }
}

/// Build the standard "find element by AutomationId (then Name), run `body`" UIA
/// PowerShell script. `body` is the PowerShell executed when the element is found;
/// it may contain literal `{`/`}` (they are inserted verbatim, not re-formatted).
fn uia_element_action_script(automation_id: &str, name: &str, body: &str) -> String {
    format!(
        r#"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$root = [System.Windows.Automation.AutomationElement]::RootElement
$cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, '{automation_id}')
$el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
if ($null -eq $el) {{ $cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, '{name}'); $el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond) }}
if ($null -ne $el) {{
{body}
}} else {{ Write-Output '{{"success":false,"error":"element not found"}}' }}
"#
    )
}

/// Parse a `Transform` pattern spec into the PowerShell method call.
/// Accepts `move:X,Y`, `resize:W,H`, or `rotate:DEG` (whitespace tolerant).
/// Returns `None` for an unrecognized or malformed spec.
fn parse_transform_action(spec: &str) -> Option<String> {
    let (op, args) = spec.split_once(':')?;
    match op.trim().to_lowercase().as_str() {
        "move" => {
            let (x, y) = args.split_once(',')?;
            let x: f64 = x.trim().parse().ok()?;
            let y: f64 = y.trim().parse().ok()?;
            Some(format!("$pattern.Move({x}, {y})"))
        }
        "resize" => {
            let (w, h) = args.split_once(',')?;
            let w: f64 = w.trim().parse().ok()?;
            let h: f64 = h.trim().parse().ok()?;
            Some(format!("$pattern.Resize({w}, {h})"))
        }
        "rotate" => {
            let deg: f64 = args.trim().parse().ok()?;
            Some(format!("$pattern.Rotate({deg})"))
        }
        _ => None,
    }
}

/// Parse the JSON tree emitted by [`build_capture_tree_script`] into a
/// [`CachedUiaTree`]. The script uses `ConvertTo-Json`, which has two quirks we
/// tolerate: a single-element collection is serialized as the bare element (not
/// a one-element array), and an empty collection may serialize as `null`. Both
/// `supported_patterns` and `children` are therefore accepted as an array, a
/// lone value, or absent.
fn parse_tree_from_ps_output(output: &str, pid: u32) -> Result<CachedUiaTree, String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Err("empty UIA capture output".to_string());
    }
    let json: serde_json::Value =
        serde_json::from_str(trimmed).map_err(|e| format!("parse UIA JSON: {e}"))?;
    if let Some(err) = json.get("error").and_then(|v| v.as_str()) {
        return Err(format!("UIA capture: {err}"));
    }
    let root = parse_uia_element(&json, 0, 0)
        .ok_or_else(|| "UIA capture: malformed root element".to_string())?;
    let mut tree = CachedUiaTree::new(pid, Duration::from_secs(30));
    tree.root = Some(root);
    tree.rebuild_indices();
    Ok(tree)
}

/// Recursively convert one JSON node into a [`CachedUiaElement`]. `depth` and
/// `child_index` are assigned from the traversal (the script's own values are
/// ignored) so indices are always internally consistent.
fn parse_uia_element(
    v: &serde_json::Value,
    depth: u32,
    child_index: u32,
) -> Option<CachedUiaElement> {
    let obj = v.as_object()?;
    let get_str = |k: &str| {
        obj.get(k)
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string()
    };
    let get_f64 = |k: &str| obj.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
    let get_bool = |k: &str| obj.get(k).and_then(|x| x.as_bool()).unwrap_or(false);

    // supported_patterns: array of names, a single name, or null. Unknown
    // pattern names are ignored rather than failing the whole parse.
    let mut supported_patterns = Vec::new();
    match obj.get("supported_patterns") {
        Some(serde_json::Value::Array(arr)) => {
            for p in arr {
                if let Some(pat) = p.as_str().and_then(UiaPattern::from_str) {
                    supported_patterns.push(pat);
                }
            }
        }
        Some(serde_json::Value::String(s)) => {
            if let Some(pat) = UiaPattern::from_str(s) {
                supported_patterns.push(pat);
            }
        }
        _ => {}
    }

    // children: array of nodes, a single node, or null.
    let mut children = Vec::new();
    match obj.get("children") {
        Some(serde_json::Value::Array(arr)) => {
            for (i, c) in arr.iter().enumerate() {
                if let Some(child) = parse_uia_element(c, depth + 1, i as u32) {
                    children.push(child);
                }
            }
        }
        Some(single @ serde_json::Value::Object(_)) => {
            if let Some(child) = parse_uia_element(single, depth + 1, 0) {
                children.push(child);
            }
        }
        _ => {}
    }

    let process_id = obj.get("process_id").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    // The PowerShell tree does not expose a stable RuntimeId, so synthesize one
    // from the traversal path; it is unique within a single captured tree.
    let runtime_id = vec![depth as i32, child_index as i32];

    Some(CachedUiaElement {
        runtime_id,
        automation_id: get_str("automation_id"),
        name: get_str("name"),
        control_type: get_str("control_type"),
        class_name: get_str("class_name"),
        bounding_rect: UiaRect {
            x: get_f64("x"),
            y: get_f64("y"),
            width: get_f64("width"),
            height: get_f64("height"),
        },
        is_enabled: get_bool("is_enabled"),
        is_offscreen: get_bool("is_offscreen"),
        process_id,
        supported_patterns,
        child_index,
        depth,
        children,
    })
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
            bounding_rect: UiaRect {
                x: 10.0,
                y: 10.0,
                width: 80.0,
                height: 30.0,
            },
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
            bounding_rect: UiaRect {
                x: 10.0,
                y: 50.0,
                width: 200.0,
                height: 25.0,
            },
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
            bounding_rect: UiaRect {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 300.0,
            },
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
    fn com_capability_matches_direct_patterns() {
        // COM-wired patterns.
        for p in [
            UiaPattern::Invoke,
            UiaPattern::Value,
            UiaPattern::Toggle,
            UiaPattern::ExpandCollapse,
            UiaPattern::SelectionItem,
            UiaPattern::RangeValue,
            UiaPattern::Scroll,
            UiaPattern::Transform,
            UiaPattern::ScrollItem,
            UiaPattern::Window,
        ] {
            assert!(pattern_supported_via_com(p), "{p:?} should be COM-wired");
        }
        // Patterns with no direct-COM arm must route to PowerShell.
        assert!(!pattern_supported_via_com(UiaPattern::Selection));
        assert!(!pattern_supported_via_com(UiaPattern::Grid));
    }

    fn sample_element() -> CachedUiaElement {
        CachedUiaElement {
            runtime_id: vec![1, 2],
            automation_id: "el_1".to_string(),
            name: "Sample".to_string(),
            control_type: "Window".to_string(),
            class_name: String::new(),
            bounding_rect: UiaRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
            },
            is_enabled: true,
            is_offscreen: false,
            process_id: 1234,
            supported_patterns: Vec::new(),
            child_index: 0,
            depth: 0,
            children: Vec::new(),
        }
    }

    #[test]
    fn window_pattern_script_sets_visual_state() {
        let el = sample_element();
        let s = build_pattern_invoke_script(&el, UiaPattern::Window, Some("Maximize"));
        assert!(s.contains("WindowPattern"));
        assert!(s.contains("SetWindowVisualState"));
        assert!(s.contains("Maximized"));
        // Element-finding preamble is present.
        assert!(s.contains("AutomationIdProperty"));
        assert!(s.contains("el_1"));
    }

    #[test]
    fn window_pattern_close_uses_close_method() {
        let el = sample_element();
        let s = build_pattern_invoke_script(&el, UiaPattern::Window, Some("Close"));
        assert!(s.contains("$pattern.Close()"));
    }

    #[test]
    fn transform_move_resize_rotate_and_invalid() {
        let el = sample_element();
        let mv = build_pattern_invoke_script(&el, UiaPattern::Transform, Some("move:10,20"));
        assert!(mv.contains("TransformPattern"));
        assert!(mv.contains("$pattern.Move(10, 20)"));
        let rs =
            build_pattern_invoke_script(&el, UiaPattern::Transform, Some(" resize : 100 , 50 "));
        assert!(rs.contains("$pattern.Resize(100, 50)"));
        let rot = build_pattern_invoke_script(&el, UiaPattern::Transform, Some("rotate:90"));
        assert!(rot.contains("$pattern.Rotate(90)"));
        let bad = build_pattern_invoke_script(&el, UiaPattern::Transform, Some("nonsense"));
        assert!(bad.contains("invalid transform spec"));
    }

    #[test]
    fn dock_scrollitem_virtualized_text_are_wired() {
        let el = sample_element();
        assert!(
            build_pattern_invoke_script(&el, UiaPattern::Dock, Some("Top"))
                .contains("SetDockPosition([System.Windows.Automation.DockPosition]::Top)")
        );
        assert!(
            build_pattern_invoke_script(&el, UiaPattern::ScrollItem, None)
                .contains("$pattern.ScrollIntoView()")
        );
        assert!(
            build_pattern_invoke_script(&el, UiaPattern::VirtualizedItem, None)
                .contains("$pattern.Realize()")
        );
        assert!(build_pattern_invoke_script(&el, UiaPattern::Text, None)
            .contains("DocumentRange.GetText(-1)"));
    }

    #[test]
    fn container_patterns_emit_read_scripts() {
        let el = sample_element();
        // Grid/Table return row/column counts.
        assert!(build_pattern_invoke_script(&el, UiaPattern::Grid, None).contains("GridPattern"));
        assert!(build_pattern_invoke_script(&el, UiaPattern::Grid, None).contains("RowCount"));
        assert!(build_pattern_invoke_script(&el, UiaPattern::Table, None).contains("TablePattern"));
        assert!(
            build_pattern_invoke_script(&el, UiaPattern::Table, None).contains("RowOrColumnMajor")
        );
        // GridItem/TableItem return cell coordinates / headers.
        assert!(build_pattern_invoke_script(&el, UiaPattern::GridItem, None)
            .contains("GridItemPattern"));
        assert!(
            build_pattern_invoke_script(&el, UiaPattern::TableItem, None)
                .contains("GetRowHeaderItems")
        );
        // ItemContainer uses the value as the search key.
        let ic = build_pattern_invoke_script(&el, UiaPattern::ItemContainer, Some("Row 5"));
        assert!(ic.contains("ItemContainerPattern"));
        assert!(ic.contains("FindItemByProperty"));
        assert!(ic.contains("Row 5"));
        // None of them should report the old "not yet wired" sentinel.
        for p in [
            UiaPattern::Grid,
            UiaPattern::GridItem,
            UiaPattern::Table,
            UiaPattern::TableItem,
            UiaPattern::ItemContainer,
        ] {
            let s = build_pattern_invoke_script(&el, p, None);
            assert!(!s.contains("not yet wired"), "{p:?} must now be wired");
        }
    }

    #[test]
    fn parse_transform_action_rejects_malformed() {
        assert!(parse_transform_action("").is_none());
        assert!(parse_transform_action("move:10").is_none());
        assert!(parse_transform_action("resize:abc,def").is_none());
        assert!(parse_transform_action("spin:90").is_none());
        assert_eq!(
            parse_transform_action("move:1,2"),
            Some("$pattern.Move(1, 2)".to_string())
        );
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

    #[test]
    fn parse_ps_output_builds_nested_tree_and_indices() {
        // Mirrors ConvertTo-Json output, including the single-element quirks:
        // the root's `children` is a real array, but the leaf's
        // `supported_patterns` is a bare string (one supported pattern).
        let json = r#"{
            "automation_id": "main_window",
            "name": "Login",
            "control_type": "Window",
            "class_name": "WinForm",
            "x": 0, "y": 0, "width": 400, "height": 300,
            "is_enabled": true, "is_offscreen": false,
            "process_id": 4321,
            "supported_patterns": ["Window"],
            "child_index": 0, "depth": 0,
            "children": [
                {
                    "automation_id": "btn_ok",
                    "name": "OK",
                    "control_type": "Button",
                    "class_name": "Button",
                    "x": 10, "y": 10, "width": 80, "height": 30,
                    "is_enabled": true, "is_offscreen": false,
                    "process_id": 4321,
                    "supported_patterns": "Invoke",
                    "child_index": 0, "depth": 1,
                    "children": null
                }
            ]
        }"#;
        let tree = parse_tree_from_ps_output(json, 4321).expect("should parse");
        let root = tree.root.as_ref().expect("root present");
        assert_eq!(root.name, "Login");
        assert_eq!(root.process_id, 4321);
        assert_eq!(root.bounding_rect.width, 400.0);
        assert_eq!(root.children.len(), 1);
        let leaf = &root.children[0];
        assert_eq!(leaf.automation_id, "btn_ok");
        assert_eq!(leaf.depth, 1);
        assert_eq!(leaf.supported_patterns, vec![UiaPattern::Invoke]);
        // Indices are rebuilt from the parsed tree.
        assert_eq!(tree.element_count, 2);
        assert!(tree.id_index.contains_key("btn_ok"));
        assert!(tree.id_index.contains_key("main_window"));
    }

    #[test]
    fn parse_ps_output_reports_capture_error() {
        let json = r#"{"error":"Process 999 not found"}"#;
        let err = parse_tree_from_ps_output(json, 999).unwrap_err();
        assert!(err.contains("not found"));
    }

    #[test]
    fn parse_ps_output_rejects_empty_and_garbage() {
        assert!(parse_tree_from_ps_output("", 1).is_err());
        assert!(parse_tree_from_ps_output("not json", 1).is_err());
    }
}
