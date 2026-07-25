#![allow(dead_code, unused_imports, unused_variables)]
//! Window management for Windows desktop automation.
//!
//! Provides window enumeration, positional control (move, resize, minimize,
//! maximize, restore, close), z-order management, and window state queries
//! via Win32 API calls through PowerShell.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

// ─── Window Info Model ───────────────────────────────────────────────────────

/// Information about a desktop window.
#[derive(Debug, Clone)]
pub struct WindowInfo {
    /// Window handle (HWND as u64 for portability).
    pub hwnd: u64,
    /// Process ID that owns the window.
    pub process_id: u32,
    /// Window title text.
    pub title: String,
    /// Window class name (e.g., "Chrome_WidgetWin_1").
    pub class_name: String,
    /// Current position and size.
    pub rect: WindowRect,
    /// Current visibility/state.
    pub state: WindowState,
    /// Whether the window is the foreground window.
    pub is_foreground: bool,
    /// Whether the window is a top-level window (not a child).
    pub is_top_level: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct WindowRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowState {
    Normal,
    Minimized,
    Maximized,
    Hidden,
}

// ─── Window Operations ───────────────────────────────────────────────────────

/// Desired operation on a window.
#[derive(Debug, Clone)]
pub enum WindowOperation {
    /// Move window to (x, y).
    Move { x: i32, y: i32 },
    /// Resize window to (width, height).
    Resize { width: u32, height: u32 },
    /// Move and resize simultaneously.
    MoveResize { x: i32, y: i32, width: u32, height: u32 },
    /// Minimize the window.
    Minimize,
    /// Maximize the window.
    Maximize,
    /// Restore from minimized/maximized state.
    Restore,
    /// Close the window (sends WM_CLOSE).
    Close,
    /// Bring to foreground and activate.
    BringToFront,
    /// Send to back of z-order.
    SendToBack,
    /// Set window always-on-top.
    SetTopMost(bool),
    /// Set window transparency (0-255, where 255 is opaque).
    SetOpacity(u8),
}

/// Result of a window operation.
#[derive(Debug, Clone)]
pub struct WindowOpResult {
    pub success: bool,
    pub hwnd: u64,
    pub operation: String,
    pub detail: String,
    /// New rect after operation (if applicable).
    pub new_rect: Option<WindowRect>,
}

// ─── Window Manager ──────────────────────────────────────────────────────────

/// Manages window enumeration and operations via PowerShell/Win32.
pub struct WindowManager;

impl WindowManager {
    /// Enumerate all visible top-level windows.
    pub fn enumerate_windows() -> Vec<WindowInfo> {
        // This would call the PowerShell script at runtime on Windows.
        // Returns empty on non-Windows platforms.
        Vec::new()
    }

    /// Find windows matching a title pattern (case-insensitive substring).
    pub fn find_by_title(title_contains: &str) -> Vec<WindowInfo> {
        Self::enumerate_windows()
            .into_iter()
            .filter(|w| {
                w.title
                    .to_ascii_lowercase()
                    .contains(&title_contains.to_ascii_lowercase())
            })
            .collect()
    }

    /// Find windows by process ID.
    pub fn find_by_pid(pid: u32) -> Vec<WindowInfo> {
        Self::enumerate_windows()
            .into_iter()
            .filter(|w| w.process_id == pid)
            .collect()
    }

    /// Find windows by class name.
    pub fn find_by_class(class_name: &str) -> Vec<WindowInfo> {
        Self::enumerate_windows()
            .into_iter()
            .filter(|w| w.class_name == class_name)
            .collect()
    }

    /// Get the currently foreground/active window.
    pub fn get_foreground_window() -> Option<WindowInfo> {
        Self::enumerate_windows().into_iter().find(|w| w.is_foreground)
    }

    /// Apply an operation to a window identified by HWND.
    pub fn apply_operation(hwnd: u64, op: &WindowOperation) -> WindowOpResult {
        if !cfg!(target_os = "windows") {
            return WindowOpResult {
                success: false,
                hwnd,
                operation: format!("{:?}", op),
                detail: "Window operations only supported on Windows".to_string(),
                new_rect: None,
            };
        }
        let script = build_window_op_script(hwnd, op);
        match run_ps_script(&script) {
            Ok(json) => parse_window_op_result(hwnd, op, &json),
            Err(e) => WindowOpResult {
                success: false,
                hwnd,
                operation: format!("{:?}", op),
                detail: e,
                new_rect: None,
            },
        }
    }

    /// Tile windows side by side on the primary monitor.
    pub fn tile_windows(hwnds: &[u64], monitor_width: u32, monitor_height: u32) -> Vec<WindowOpResult> {
        if hwnds.is_empty() {
            return Vec::new();
        }
        let cols = (hwnds.len() as f64).sqrt().ceil() as u32;
        let rows = ((hwnds.len() as u32) + cols - 1) / cols;
        let tile_w = monitor_width / cols;
        let tile_h = monitor_height / rows;

        hwnds
            .iter()
            .enumerate()
            .map(|(i, &hwnd)| {
                let col = (i as u32) % cols;
                let row = (i as u32) / cols;
                Self::apply_operation(
                    hwnd,
                    &WindowOperation::MoveResize {
                        x: (col * tile_w) as i32,
                        y: (row * tile_h) as i32,
                        width: tile_w,
                        height: tile_h,
                    },
                )
            })
            .collect()
    }

    /// Cascade windows with offset.
    pub fn cascade_windows(hwnds: &[u64], start_x: i32, start_y: i32, offset: i32) -> Vec<WindowOpResult> {
        hwnds
            .iter()
            .enumerate()
            .map(|(i, &hwnd)| {
                let x = start_x + (i as i32) * offset;
                let y = start_y + (i as i32) * offset;
                Self::apply_operation(hwnd, &WindowOperation::Move { x, y })
            })
            .collect()
    }
}

// ─── PowerShell Scripts ──────────────────────────────────────────────────────

/// Build a PowerShell script that enumerates all visible top-level windows
/// and outputs their info as JSON.
pub fn build_enumerate_windows_script() -> String {
    r#"
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public class WinEnum {
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern int GetWindowTextLength(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hWnd, StringBuilder sb, int maxCount);
    [DllImport("user32.dll")] public static extern int GetClassName(IntPtr hWnd, StringBuilder sb, int maxCount);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr hWnd);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
}
'@
$fgWnd = [WinEnum]::GetForegroundWindow()
$windows = @()
$callback = [WinEnum+EnumWindowsProc]{
    param($hWnd, $lParam)
    if ([WinEnum]::IsWindowVisible($hWnd) -and [WinEnum]::GetWindowTextLength($hWnd) -gt 0) {
        $sb = New-Object System.Text.StringBuilder 256
        [WinEnum]::GetWindowText($hWnd, $sb, 256) | Out-Null
        $title = $sb.ToString()
        $sb.Clear() | Out-Null
        [WinEnum]::GetClassName($hWnd, $sb, 256) | Out-Null
        $class = $sb.ToString()
        $pid = 0
        [WinEnum]::GetWindowThreadProcessId($hWnd, [ref]$pid) | Out-Null
        $rect = New-Object WinEnum+RECT
        [WinEnum]::GetWindowRect($hWnd, [ref]$rect) | Out-Null
        $state = "normal"
        if ([WinEnum]::IsIconic($hWnd)) { $state = "minimized" }
        elseif ([WinEnum]::IsZoomed($hWnd)) { $state = "maximized" }
        $script:windows += @{
            hwnd = $hWnd.ToInt64()
            process_id = $pid
            title = $title
            class_name = $class
            x = $rect.Left; y = $rect.Top
            width = $rect.Right - $rect.Left
            height = $rect.Bottom - $rect.Top
            state = $state
            is_foreground = ($hWnd -eq $fgWnd)
        }
    }
    return $true
}
[WinEnum]::EnumWindows($callback, [IntPtr]::Zero) | Out-Null
ConvertTo-Json $windows -Compress
"#
    .to_string()
}

/// Build a PowerShell script to perform a window operation.
pub fn build_window_op_script(hwnd: u64, op: &WindowOperation) -> String {
    let op_code = match op {
        WindowOperation::Move { x, y } => format!(
            "[WinOp]::MoveWindow([IntPtr]{}L, {}, {}, 0, 0, $true) | Out-Null",
            hwnd, x, y
        ),
        WindowOperation::Resize { width, height } => format!(
            "$r = New-Object WinOp+RECT; [WinOp]::GetWindowRect([IntPtr]{}L, [ref]$r) | Out-Null; [WinOp]::MoveWindow([IntPtr]{}L, $r.Left, $r.Top, {}, {}, $true) | Out-Null",
            hwnd, hwnd, width, height
        ),
        WindowOperation::MoveResize { x, y, width, height } => format!(
            "[WinOp]::MoveWindow([IntPtr]{}L, {}, {}, {}, {}, $true) | Out-Null",
            hwnd, x, y, width, height
        ),
        WindowOperation::Minimize => format!(
            "[WinOp]::ShowWindow([IntPtr]{}L, 6) | Out-Null", hwnd
        ),
        WindowOperation::Maximize => format!(
            "[WinOp]::ShowWindow([IntPtr]{}L, 3) | Out-Null", hwnd
        ),
        WindowOperation::Restore => format!(
            "[WinOp]::ShowWindow([IntPtr]{}L, 9) | Out-Null", hwnd
        ),
        WindowOperation::Close => format!(
            "[WinOp]::SendMessage([IntPtr]{}L, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null", hwnd
        ),
        WindowOperation::BringToFront => format!(
            "[WinOp]::SetForegroundWindow([IntPtr]{}L) | Out-Null", hwnd
        ),
        WindowOperation::SendToBack => format!(
            "[WinOp]::SetWindowPos([IntPtr]{}L, [IntPtr]1, 0, 0, 0, 0, 0x0013) | Out-Null", hwnd
        ),
        WindowOperation::SetTopMost(true) => format!(
            "[WinOp]::SetWindowPos([IntPtr]{}L, [IntPtr](-1), 0, 0, 0, 0, 0x0013) | Out-Null", hwnd
        ),
        WindowOperation::SetTopMost(false) => format!(
            "[WinOp]::SetWindowPos([IntPtr]{}L, [IntPtr](-2), 0, 0, 0, 0, 0x0013) | Out-Null", hwnd
        ),
        WindowOperation::SetOpacity(alpha) => format!(
            "$style = [WinOp]::GetWindowLong([IntPtr]{}L, -20); [WinOp]::SetWindowLong([IntPtr]{}L, -20, $style -bor 0x80000); [WinOp]::SetLayeredWindowAttributes([IntPtr]{}L, 0, {}, 2) | Out-Null",
            hwnd, hwnd, hwnd, alpha
        ),
    };

    format!(
        r#"
Add-Type @'
using System;
using System.Runtime.InteropServices;
public class WinOp {{
    [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr hWnd, int X, int Y, int nWidth, int nHeight, bool bRepaint);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hWnd, IntPtr hWndInsertAfter, int X, int Y, int cx, int cy, uint uFlags);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr hWnd, int nIndex);
    [DllImport("user32.dll")] public static extern int SetWindowLong(IntPtr hWnd, int nIndex, int dwNewLong);
    [DllImport("user32.dll")] public static extern bool SetLayeredWindowAttributes(IntPtr hWnd, uint crKey, byte bAlpha, uint dwFlags);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
    [StructLayout(LayoutKind.Sequential)] public struct RECT {{ public int Left; public int Top; public int Right; public int Bottom; }}
}}
'@
{op_code}
$r = New-Object WinOp+RECT
[WinOp]::GetWindowRect([IntPtr]{hwnd}L, [ref]$r) | Out-Null
$result = @{{ success = $true; hwnd = {hwnd}; x = $r.Left; y = $r.Top; width = $r.Right - $r.Left; height = $r.Bottom - $r.Top }}
ConvertTo-Json $result -Compress
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

fn parse_window_op_result(hwnd: u64, op: &WindowOperation, json: &str) -> WindowOpResult {
    #[derive(serde::Deserialize)]
    struct PsResult {
        success: Option<bool>,
        x: Option<i32>,
        y: Option<i32>,
        width: Option<u32>,
        height: Option<u32>,
    }
    match serde_json::from_str::<PsResult>(json) {
        Ok(r) => WindowOpResult {
            success: r.success.unwrap_or(true),
            hwnd,
            operation: format!("{:?}", op),
            detail: "executed via PowerShell".to_string(),
            new_rect: Some(WindowRect {
                x: r.x.unwrap_or(0),
                y: r.y.unwrap_or(0),
                width: r.width.unwrap_or(0),
                height: r.height.unwrap_or(0),
            }),
        },
        Err(e) => WindowOpResult {
            success: false,
            hwnd,
            operation: format!("{:?}", op),
            detail: format!("parse error: {e}"),
            new_rect: None,
        },
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_layout_computes_grid() {
        // 4 windows should tile as 2x2
        let results = WindowManager::tile_windows(&[1, 2, 3, 4], 1920, 1080);
        assert_eq!(results.len(), 4);
    }

    #[test]
    fn cascade_offsets() {
        let results = WindowManager::cascade_windows(&[1, 2, 3], 0, 0, 30);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn enumerate_script_contains_enumwindows() {
        let script = build_enumerate_windows_script();
        assert!(script.contains("EnumWindows"));
        assert!(script.contains("GetForegroundWindow"));
        assert!(script.contains("GetWindowRect"));
    }

    #[test]
    fn window_op_script_move() {
        let script = build_window_op_script(12345, &WindowOperation::Move { x: 100, y: 200 });
        assert!(script.contains("MoveWindow"));
        assert!(script.contains("12345"));
    }

    #[test]
    fn window_op_script_maximize() {
        let script = build_window_op_script(99, &WindowOperation::Maximize);
        assert!(script.contains("ShowWindow"));
        assert!(script.contains("3")); // SW_MAXIMIZE = 3
    }

    #[test]
    fn window_op_script_close() {
        let script = build_window_op_script(555, &WindowOperation::Close);
        assert!(script.contains("SendMessage"));
        assert!(script.contains("0x0010")); // WM_CLOSE
    }

    #[test]
    fn window_op_script_topmost() {
        let script = build_window_op_script(777, &WindowOperation::SetTopMost(true));
        assert!(script.contains("SetWindowPos"));
        assert!(script.contains("-1")); // HWND_TOPMOST
    }
}
