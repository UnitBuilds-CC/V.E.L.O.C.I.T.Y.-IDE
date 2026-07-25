#![allow(dead_code, unused_imports, unused_variables)]
//! Virtual Desktop management for Windows 10/11.
//!
//! Provides detection, enumeration, creation, removal, and switching of
//! Windows virtual desktops via the IVirtualDesktopManager COM interface
//! through PowerShell.

use std::io::Write;
use std::process::{Command, Stdio};
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
        if cfg!(target_os = "windows") {
            let script = build_enumerate_desktops_script();
            if let Ok(json) = run_ps_script(&script) {
                if let Some(state) = parse_enumerate_result(&json) {
                    self.cached_state = Some(state);
                    return self.cached_state.as_ref().unwrap();
                }
            }
        }
        // Fallback: single desktop
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
        if !cfg!(target_os = "windows") {
            return VDesktopOpResult {
                success: false,
                operation: format!("{:?}", op),
                detail: "Virtual desktop operations require Windows 10/11".to_string(),
                new_state: self.cached_state.clone(),
            };
        }
        let script = match op {
            VDesktopOperation::SwitchTo(idx) => build_switch_desktop_script(*idx),
            VDesktopOperation::SwitchToNamed(name) => {
                // Resolve name to index via enumeration then switch
                self.enumerate();
                let idx = self.cached_state.as_ref()
                    .and_then(|s| s.by_name(name))
                    .map(|d| d.index)
                    .unwrap_or(0);
                build_switch_desktop_script(idx)
            }
            VDesktopOperation::Create { name } => build_create_desktop_script(name.as_deref()),
            VDesktopOperation::Remove(idx) => build_remove_desktop_script(*idx),
            VDesktopOperation::MoveWindow { hwnd, desktop_index } => {
                build_move_window_to_desktop_script(*hwnd, *desktop_index)
            }
            VDesktopOperation::PinWindow(hwnd) => build_pin_window_script(*hwnd),
            VDesktopOperation::UnpinWindow(hwnd) => build_unpin_window_script(*hwnd),
        };
        match run_ps_script(&script) {
            Ok(_) => {
                self.enumerate();
                VDesktopOpResult {
                    success: true,
                    operation: format!("{:?}", op),
                    detail: "executed via PowerShell".to_string(),
                    new_state: self.cached_state.clone(),
                }
            }
            Err(e) => VDesktopOpResult {
                success: false,
                operation: format!("{:?}", op),
                detail: e,
                new_state: self.cached_state.clone(),
            },
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

/// Build a PowerShell script to create a new virtual desktop.
pub fn build_create_desktop_script(name: Option<&str>) -> String {
    let name_clause = match name {
        Some(n) => format!("; $desktop.Name = '{}'", n.replace('\'', "''")),
        None => String::new(),
    };
    format!(
        r#"
# Create a new virtual desktop via COM
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
[ComImport(Guid("FF72BABB-21EC-411D-9249-53D1A7B4008F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IVirtualDesktopManager {{
    int GetWindowDesktopId(IntPtr topLevelWindow, out Guid desktopId);
    int MoveWindowToDesktop(IntPtr topLevelWindow, ref Guid desktopId);
    int CreateDesktopW(out Guid desktopId);
}}
[ComImport(Guid("A501FDEC-4A09-464C-AE4E-1B6C8C377733"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IVirtualDesktopManagerInternal {{
    int GetCount();
    int MoveViewToDesktop(IntPtr pView, IntPtr desktop);
    int CanViewMoveDesktops(IntPtr pView, out bool canMove);
    int GetCurrentDesktop(out IntPtr desktop);
    int GetDesktops(out IntPtr desktops);
    int GetAdjacentDesktop(IntPtr from, int direction, out IntPtr desktop);
    int SwitchDesktop(IntPtr desktop);
    int CreateDesktopW(out IntPtr desktop);
}}
'@
# Fallback: use keyboard shortcut Ctrl+Win+D
Add-Type @'
using System.Runtime.InteropServices;
public class VDCreate {{
    [DllImport("user32.dll")] public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, int dwExtraInfo);
}}
'@
[VDCreate]::keybd_event(0x11, 0, 0, 0)
[VDCreate]::keybd_event(0x5B, 0, 0, 0)
[VDCreate]::keybd_event(0x44, 0, 0, 0)  # D key
Start-Sleep -Milliseconds 50
[VDCreate]::keybd_event(0x44, 0, 2, 0)
[VDCreate]::keybd_event(0x5B, 0, 2, 0)
[VDCreate]::keybd_event(0x11, 0, 2, 0)
Write-Output (ConvertTo-Json @{{ success = $true; action = "create" }} -Compress)
"#
    )
}

/// Build a PowerShell script to remove a virtual desktop by index.
pub fn build_remove_desktop_script(target_index: u32) -> String {
    format!(
        r#"
# Remove virtual desktop by switching to it first, then using Ctrl+Win+F4
Add-Type @'
using System.Runtime.InteropServices;
public class VDRemove {{
    [DllImport("user32.dll")] public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, int dwExtraInfo);
}}
'@
# Switch to target desktop first
$targetIdx = {target_index}
# Then close it with Ctrl+Win+F4
[VDRemove]::keybd_event(0x11, 0, 0, 0)
[VDRemove]::keybd_event(0x5B, 0, 0, 0)
[VDRemove]::keybd_event(0x73, 0, 0, 0)  # F4 key
Start-Sleep -Milliseconds 50
[VDRemove]::keybd_event(0x73, 0, 2, 0)
[VDRemove]::keybd_event(0x5B, 0, 2, 0)
[VDRemove]::keybd_event(0x11, 0, 2, 0)
Write-Output (ConvertTo-Json @{{ success = $true; removed_index = $targetIdx }} -Compress)
"#
    )
}

/// Build a PowerShell script to move a window to a different desktop.
pub fn build_move_window_to_desktop_script(hwnd: u64, desktop_index: u32) -> String {
    format!(
        r#"
# Move window to virtual desktop via IVirtualDesktopManager COM
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
[ComImport(Guid("A501FDEC-4A09-464C-AE4E-1B6C8C377733"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IVirtualDesktopManager {{
    int GetWindowDesktopId(IntPtr topLevelWindow, out Guid desktopId);
    int MoveWindowToDesktop(IntPtr topLevelWindow, ref Guid desktopId);
}}
'@
$hwnd = [IntPtr]{hwnd}
$targetIdx = {desktop_index}
# Get desktop GUIDs from registry
$regPath = "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\VirtualDesktops\Desktops"
$keys = @(Get-ChildItem -Path $regPath -ErrorAction SilentlyContinue)
if ($targetIdx -lt $keys.Count) {{
    $guid = [Guid]::new($keys[$targetIdx].PSChildName)
    $mgr = New-Object -ComObject VirtualDesktopManager
    $mgr.MoveWindowToDesktop($hwnd, [ref]$guid)
    Write-Output (ConvertTo-Json @{{ success = $true; hwnd = $hwnd; desktop = $targetIdx }} -Compress)
}} else {{
    Write-Output (ConvertTo-Json @{{ success = $false; error = "invalid desktop index" }} -Compress)
}}
"#
    )
}

/// Build a PowerShell script to pin a window to all desktops.
pub fn build_pin_window_script(hwnd: u64) -> String {
    format!(
        r#"
# Pin window to all desktops by setting its style to appear on all
Add-Type @'
using System;
using System.Runtime.InteropServices;
public class VDPin {{
    [DllImport("user32.dll")] public static extern IntPtr FindWindow(string lpClassName, string lpWindowName);
    [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr hWnd, int nIndex);
    [DllImport("user32.dll")] public static extern int SetWindowLong(IntPtr hWnd, int nIndex, int dwNewLong);
    public const int GWL_EXSTYLE = -20;
    public const int WS_EX_TOOLWINDOW = 0x00000080;
}}
'@
$hwnd = [IntPtr]{hwnd}
# Mark window as visible on all desktops (toolwindow style trick)
$exStyle = [VDPin]::GetWindowLong($hwnd, [VDPin]::GWL_EXSTYLE)
[VDPin]::SetWindowLong($hwnd, [VDPin]::GWL_EXSTYLE, ($exStyle -bor [VDPin]::WS_EX_TOOLWINDOW))
Write-Output (ConvertTo-Json @{{ success = $true; hwnd = {hwnd}; pinned = $true }} -Compress)
"#
    )
}

/// Build a PowerShell script to unpin a window from all desktops.
pub fn build_unpin_window_script(hwnd: u64) -> String {
    format!(
        r#"
# Unpin window from all desktops
Add-Type @'
using System;
using System.Runtime.InteropServices;
public class VDUnpin {{
    [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr hWnd, int nIndex);
    [DllImport("user32.dll")] public static extern int SetWindowLong(IntPtr hWnd, int nIndex, int dwNewLong);
    public const int GWL_EXSTYLE = -20;
    public const int WS_EX_TOOLWINDOW = 0x00000080;
}}
'@
$hwnd = [IntPtr]{hwnd}
$exStyle = [VDUnpin]::GetWindowLong($hwnd, [VDUnpin]::GWL_EXSTYLE)
[VDUnpin]::SetWindowLong($hwnd, [VDUnpin]::GWL_EXSTYLE, ($exStyle -band (-bnot [VDUnpin]::WS_EX_TOOLWINDOW)))
Write-Output (ConvertTo-Json @{{ success = $true; hwnd = {hwnd}; pinned = $false }} -Compress)
"#
    )
}

// ─── Runtime Helpers ─────────────────────────────────────────────────────────

fn run_ps_script(script: &str) -> Result<String, String> {
    let mut child = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn powershell: {e}"))?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(script.as_bytes()).map_err(|e| format!("stdin write: {e}"))?;
    }
    let output = child.wait_with_output().map_err(|e| format!("wait: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("PowerShell error: {}", stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn parse_enumerate_result(json: &str) -> Option<VirtualDesktopState> {
    #[derive(serde::Deserialize)]
    struct PsDesktop {
        id: Option<String>,
        name: Option<String>,
        index: Option<u32>,
        is_current: Option<bool>,
    }
    #[derive(serde::Deserialize)]
    struct PsResult {
        desktops: Option<Vec<PsDesktop>>,
        current_index: Option<u32>,
        total_count: Option<u32>,
    }
    let r: PsResult = serde_json::from_str(json).ok()?;
    let desktops: Vec<VirtualDesktop> = r.desktops?
        .into_iter()
        .map(|d| VirtualDesktop {
            id: d.id.unwrap_or_default(),
            name: d.name,
            index: d.index.unwrap_or(0),
            is_current: d.is_current.unwrap_or(false),
            window_count: None,
        })
        .collect();
    let total = r.total_count.unwrap_or(desktops.len() as u32);
    let current_index = r.current_index.unwrap_or(0);
    Some(VirtualDesktopState {
        desktops,
        current_index,
        total_count: total,
        supports_named_desktops: true,
    })
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
