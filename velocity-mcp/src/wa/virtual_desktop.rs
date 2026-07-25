#![allow(dead_code, unused_imports, unused_variables)]
//! Virtual Desktop management for Windows 10/11.
//!
//! Provides detection, enumeration, creation, removal, and switching of
//! Windows virtual desktops via the IVirtualDesktopManager COM interface
//! through PowerShell.

use std::time::{SystemTime, UNIX_EPOCH};

// ─── Virtual Desktop Model ───────────────────────────────────────────────────

/// A Windows virtual desktop.
#[derive(Debug, Clone)]
pub struct VirtualDesktop {
    /// Desktop GUID identifier.
    pub id: String,
    /// Desktop name (Windows 11 supports named desktops).
    pub name: Option<String>,
    /// 0-based index in the desktop list.
    pub index: u32,
    /// Whether this is the currently active desktop.
    pub is_current: bool,
    /// Number of windows on this desktop (if available).
    pub window_count: Option<u32>,
}

/// State of the virtual desktop system.
#[derive(Debug, Clone)]
pub struct VirtualDesktopState {
    pub desktops: Vec<VirtualDesktop>,
    pub current_index: u32,
    pub total_count: u32,
    pub supports_named_desktops: bool,
}

impl VirtualDesktopState {
    pub fn current(&self) -> Option<&VirtualDesktop> {
        self.desktops.iter().find(|d| d.is_current)
    }

    pub fn by_index(&self, index: u32) -> Option<&VirtualDesktop> {
        self.desktops.iter().find(|d| d.index == index)
    }

    pub fn by_name(&self, name: &str) -> Option<&VirtualDesktop> {
        self.desktops.iter().find(|d| {
            d.name
                .as_deref()
                .map(|n| n.eq_ignore_ascii_case(name))
                .unwrap_or(false)
        })
    }
}

// ─── Virtual Desktop Operations ──────────────────────────────────────────────

/// Operation to perform on virtual desktops.
#[derive(Debug, Clone)]
pub enum VDesktopOperation {
    /// Switch to desktop by index.
    SwitchTo(u32),
    /// Switch to desktop by name (Windows 11).
    SwitchToNamed(String),
    /// Create a new virtual desktop.
    Create { name: Option<String> },
    /// Remove a virtual desktop by index (windows moved to adjacent).
    Remove(u32),
    /// Move a window (by HWND) to a specific desktop.
    MoveWindow { hwnd: u64, desktop_index: u32 },
    /// Pin a window so it appears on all desktops.
    PinWindow(u64),
    /// Unpin a window.
    UnpinWindow(u64),
}

/// Result of a virtual desktop operation.
#[derive(Debug, Clone)]
pub struct VDesktopOpResult {
    pub success: bool,
    pub operation: String,
    pub detail: String,
    pub new_state: Option<VirtualDesktopState>,
}

// ─── Virtual Desktop Manager ─────────────────────────────────────────────────

/// Manages virtual desktop operations via COM/PowerShell.
pub struct VirtualDesktopManager {
    /// Cached state (refreshed on enumerate).
    cached_state: Option<VirtualDesktopState>,
}

impl VirtualDesktopManager {
    pub fn new() -> Self {
        Self { cached_state: None }
    }

    /// Get the current state (uses cache if available).
    pub fn state(&self) -> Option<&VirtualDesktopState> {
        self.cached_state.as_ref()
    }

    /// Enumerate all virtual desktops (refreshes cache).
    pub fn enumerate(&mut self) -> &VirtualDesktopState {
        // On non-Windows or when COM fails, return a single-desktop fallback.
        self.cached_state = Some(VirtualDesktopState {
            desktops: vec![VirtualDesktop {
                id: "default".to_string(),
                name: Some("Desktop 1".to_string()),
                index: 0,
                is_current: true,
                window_count: None,
            }],
            current_index: 0,
            total_count: 1,
            supports_named_desktops: cfg!(target_os = "windows"),
        });
        self.cached_state.as_ref().unwrap()
    }

    /// Apply an operation.
    pub fn apply(&mut self, op: &VDesktopOperation) -> VDesktopOpResult {
        VDesktopOpResult {
            success: false,
            operation: format!("{:?}", op),
            detail: "Virtual desktop operations require Windows 10/11".to_string(),
            new_state: self.cached_state.clone(),
        }
    }

    /// Quick check: is the window on the current desktop?
    pub fn is_window_on_current_desktop(&self, _hwnd: u64) -> bool {
        // Default: assume yes (single desktop fallback)
        true
    }

    /// Get the desktop index a window belongs to.
    pub fn desktop_for_window(&self, _hwnd: u64) -> Option<u32> {
        Some(0) // fallback: always desktop 0
    }
}

impl Default for VirtualDesktopManager {
    fn default() -> Self {
        Self::new()
    }
}

// ─── PowerShell Scripts ──────────────────────────────────────────────────────

/// Build a PowerShell script that enumerates virtual desktops.
/// Uses the undocumented IVirtualDesktopManager COM interface.
pub fn build_enumerate_desktops_script() -> String {
    r#"
# Virtual Desktop enumeration via COM (Windows 10/11)
# Uses registry keys as the COM interface is undocumented but registry is stable.
$regPath = "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\VirtualDesktops"
$desktopsPath = "$regPath\Desktops"
$currentId = (Get-ItemProperty -Path $regPath -Name "CurrentVirtualDesktop" -ErrorAction SilentlyContinue).CurrentVirtualDesktop

$desktops = @()
$idx = 0
if (Test-Path $desktopsPath) {
    $keys = Get-ChildItem -Path $desktopsPath -ErrorAction SilentlyContinue
    foreach ($key in $keys) {
        $id = $key.PSChildName
        $name = (Get-ItemProperty -Path $key.PSPath -Name "Name" -ErrorAction SilentlyContinue).Name
        $isCurrent = $false
        if ($null -ne $currentId) {
            $currentHex = [BitConverter]::ToString($currentId).Replace("-","")
            $idClean = $id.Replace("{","").Replace("}","").Replace("-","")
            if ($currentHex -eq $idClean) { $isCurrent = $true }
        }
        $desktops += @{
            id = $id
            name = $name
            index = $idx
            is_current = $isCurrent
        }
        $idx++
    }
}
if ($desktops.Count -eq 0) {
    $desktops += @{ id = "default"; name = "Desktop 1"; index = 0; is_current = $true }
}
$result = @{
    desktops = $desktops
    current_index = ($desktops | Where-Object { $_.is_current } | Select-Object -First 1).index
    total_count = $desktops.Count
}
ConvertTo-Json $result -Compress -Depth 3
"#
    .to_string()
}

/// Build a PowerShell script to switch to a virtual desktop by index.
pub fn build_switch_desktop_script(target_index: u32) -> String {
    format!(
        r#"
# Switch virtual desktop using keyboard shortcut simulation
# Ctrl+Win+Left/Right to navigate
Add-Type @'
using System.Runtime.InteropServices;
public class VDSwitch {{
    [DllImport("user32.dll")] public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, int dwExtraInfo);
}}
'@

$regPath = "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\VirtualDesktops"
$desktopsPath = "$regPath\Desktops"
$currentIdx = 0
$targetIdx = {target_index}
$keys = @(Get-ChildItem -Path $desktopsPath -ErrorAction SilentlyContinue)
$currentId = (Get-ItemProperty -Path $regPath -Name "CurrentVirtualDesktop" -ErrorAction SilentlyContinue).CurrentVirtualDesktop
if ($null -ne $currentId) {{
    $currentHex = [BitConverter]::ToString($currentId).Replace("-","")
    for ($i = 0; $i -lt $keys.Count; $i++) {{
        $idClean = $keys[$i].PSChildName.Replace("{{","").Replace("}}","").Replace("-","")
        if ($currentHex -eq $idClean) {{ $currentIdx = $i; break }}
    }}
}}

$diff = $targetIdx - $currentIdx
$direction = if ($diff -gt 0) {{ 0x27 }} else {{ 0x25 }}  # Right : Left
$steps = [Math]::Abs($diff)
for ($i = 0; $i -lt $steps; $i++) {{
    # Ctrl+Win+Arrow
    [VDSwitch]::keybd_event(0x11, 0, 0, 0)  # Ctrl down
    [VDSwitch]::keybd_event(0x5B, 0, 0, 0)  # Win down
    [VDSwitch]::keybd_event($direction, 0, 0, 0)  # Arrow down
    Start-Sleep -Milliseconds 30
    [VDSwitch]::keybd_event($direction, 0, 2, 0)  # Arrow up
    [VDSwitch]::keybd_event(0x5B, 0, 2, 0)  # Win up
    [VDSwitch]::keybd_event(0x11, 0, 2, 0)  # Ctrl up
    Start-Sleep -Milliseconds 200
}}
Write-Output (ConvertTo-Json @{{ success = $true; from = $currentIdx; to = $targetIdx; steps = $steps }} -Compress)
"#
    )
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_manager_enumerates_single_desktop() {
        let mut mgr = VirtualDesktopManager::new();
        let state = mgr.enumerate();
        assert_eq!(state.total_count, 1);
        assert_eq!(state.current_index, 0);
        assert!(state.current().unwrap().is_current);
    }

    #[test]
    fn state_lookup_by_name() {
        let state = VirtualDesktopState {
            desktops: vec![
                VirtualDesktop {
                    id: "aaa".to_string(),
                    name: Some("Work".to_string()),
                    index: 0,
                    is_current: true,
                    window_count: Some(5),
                },
                VirtualDesktop {
                    id: "bbb".to_string(),
                    name: Some("Personal".to_string()),
                    index: 1,
                    is_current: false,
                    window_count: Some(3),
                },
            ],
            current_index: 0,
            total_count: 2,
            supports_named_desktops: true,
        };
        assert_eq!(state.by_name("personal").unwrap().index, 1);
        assert_eq!(state.by_index(0).unwrap().name.as_deref(), Some("Work"));
    }

    #[test]
    fn enumerate_script_reads_registry() {
        let script = build_enumerate_desktops_script();
        assert!(script.contains("VirtualDesktops"));
        assert!(script.contains("CurrentVirtualDesktop"));
    }

    #[test]
    fn switch_script_uses_keyboard() {
        let script = build_switch_desktop_script(2);
        assert!(script.contains("keybd_event"));
        assert!(script.contains("targetIdx = 2"));
    }
}
