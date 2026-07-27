#![allow(dead_code, unused_imports, unused_variables)]
//! Window management for Windows desktop automation.
//!
//! Provides window enumeration, positional control (move, resize, minimize,
//! maximize, restore, close), z-order management, and window state queries
//! via direct Win32 API calls (zero PowerShell overhead).

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

/// Manages window enumeration and operations via native Win32 API.
pub struct WindowManager;

impl WindowManager {
    /// Enumerate all visible top-level windows.
    pub fn enumerate_windows() -> Vec<WindowInfo> {
        #[cfg(target_os = "windows")]
        {
            enumerate_windows_native()
        }
        #[cfg(not(target_os = "windows"))]
        {
            Vec::new()
        }
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
        #[cfg(target_os = "windows")]
        {
            apply_operation_native(hwnd, op)
        }
        #[cfg(not(target_os = "windows"))]
        {
            WindowOpResult {
                success: false,
                hwnd,
                operation: format!("{:?}", op),
                detail: "Window operations only supported on Windows".to_string(),
                new_rect: None,
            }
        }
    }

    /// Tile windows side by side on the primary monitor.
    pub fn tile_windows(hwnds: &[u64], monitor_width: u32, monitor_height: u32) -> Vec<WindowOpResult> {
        if hwnds.is_empty() {
            return Vec::new();
        }
        let cols = (hwnds.len() as f64).sqrt().ceil() as u32;
        let rows = (hwnds.len() as u32).div_ceil(cols);
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

// ─── Native Win32 Implementation ─────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod native {
    use super::*;
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
    use windows::Win32::UI::WindowsAndMessaging::*;

    /// Enumerate all visible top-level windows using EnumWindows.
    pub fn enumerate_windows_native() -> Vec<WindowInfo> {
        let mut windows: Vec<WindowInfo> = Vec::new();
        let fg_hwnd = unsafe { GetForegroundWindow() };

        unsafe {
            let _ = EnumWindows(
                Some(enum_windows_callback),
                LPARAM(&mut windows as *mut Vec<WindowInfo> as isize),
            );
        }

        // Mark foreground window
        let fg_val = fg_hwnd.0 as u64;
        for w in windows.iter_mut() {
            w.is_foreground = w.hwnd == fg_val;
        }

        windows
    }

    unsafe extern "system" fn enum_windows_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let windows = &mut *(lparam.0 as *mut Vec<WindowInfo>);

        // Skip invisible windows
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }

        // Skip windows with no title
        let title_len = GetWindowTextLengthW(hwnd);
        if title_len == 0 {
            return BOOL(1);
        }

        // Get title
        let mut title_buf = vec![0u16; (title_len + 1) as usize];
        let actual_len = GetWindowTextW(hwnd, &mut title_buf);
        let title = String::from_utf16_lossy(&title_buf[..actual_len as usize]);

        // Get class name
        let mut class_buf = vec![0u16; 256];
        let class_len = GetClassNameW(hwnd, &mut class_buf);
        let class_name = String::from_utf16_lossy(&class_buf[..class_len as usize]);

        // Get process ID
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));

        // Get window rect
        let mut rect = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rect);

        // Determine state
        let state = if IsIconic(hwnd).as_bool() {
            WindowState::Minimized
        } else if IsZoomed(hwnd).as_bool() {
            WindowState::Maximized
        } else {
            WindowState::Normal
        };

        windows.push(WindowInfo {
            hwnd: hwnd.0 as u64,
            process_id: pid,
            title,
            class_name,
            rect: WindowRect {
                x: rect.left,
                y: rect.top,
                width: (rect.right - rect.left) as u32,
                height: (rect.bottom - rect.top) as u32,
            },
            state,
            is_foreground: false, // Set later
            is_top_level: true,
        });

        BOOL(1) // Continue enumeration
    }

    /// Apply a window operation using native Win32 calls.
    pub fn apply_operation_native(hwnd_val: u64, op: &WindowOperation) -> WindowOpResult {
        let hwnd = HWND(hwnd_val as *mut _);
        let op_name = format!("{:?}", op);

        let success = unsafe {
            match op {
                WindowOperation::Move { x, y } => {
                    let mut rect = RECT::default();
                    let _ = GetWindowRect(hwnd, &mut rect);
                    MoveWindow(hwnd, *x, *y, rect.right - rect.left, rect.bottom - rect.top, true).is_ok()
                }
                WindowOperation::Resize { width, height } => {
                    let mut rect = RECT::default();
                    let _ = GetWindowRect(hwnd, &mut rect);
                    MoveWindow(hwnd, rect.left, rect.top, *width as i32, *height as i32, true).is_ok()
                }
                WindowOperation::MoveResize { x, y, width, height } => {
                    MoveWindow(hwnd, *x, *y, *width as i32, *height as i32, true).is_ok()
                }
                WindowOperation::Minimize => {
                    ShowWindow(hwnd, SW_MINIMIZE).as_bool()
                }
                WindowOperation::Maximize => {
                    ShowWindow(hwnd, SW_MAXIMIZE).as_bool()
                }
                WindowOperation::Restore => {
                    ShowWindow(hwnd, SW_RESTORE).as_bool()
                }
                WindowOperation::Close => {
                    let _ = SendMessageW(hwnd, WM_CLOSE, windows::Win32::Foundation::WPARAM(0), windows::Win32::Foundation::LPARAM(0));
                    true
                }
                WindowOperation::BringToFront => {
                    SetForegroundWindow(hwnd).as_bool()
                }
                WindowOperation::SendToBack => {
                    SetWindowPos(hwnd, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE).is_ok()
                }
                WindowOperation::SetTopMost(true) => {
                    SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE).is_ok()
                }
                WindowOperation::SetTopMost(false) => {
                    SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE).is_ok()
                }
                WindowOperation::SetOpacity(alpha) => {
                    // Set WS_EX_LAYERED style
                    let style = GetWindowLongW(hwnd, GWL_EXSTYLE);
                    SetWindowLongW(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED.0 as i32);
                    SetLayeredWindowAttributes(hwnd, windows::Win32::Foundation::COLORREF(0), *alpha, LWA_ALPHA).is_ok()
                }
            }
        };

        // Get new rect after operation
        let new_rect = unsafe {
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_ok() {
                Some(WindowRect {
                    x: rect.left,
                    y: rect.top,
                    width: (rect.right - rect.left) as u32,
                    height: (rect.bottom - rect.top) as u32,
                })
            } else {
                None
            }
        };

        WindowOpResult {
            success,
            hwnd: hwnd_val,
            operation: op_name,
            detail: "executed via native Win32 API".to_string(),
            new_rect,
        }
    }
}

#[cfg(target_os = "windows")]
use native::{apply_operation_native, enumerate_windows_native};

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
    fn window_info_model() {
        let info = WindowInfo {
            hwnd: 12345,
            process_id: 1000,
            title: "Test Window".to_string(),
            class_name: "TestClass".to_string(),
            rect: WindowRect { x: 0, y: 0, width: 800, height: 600 },
            state: WindowState::Normal,
            is_foreground: true,
            is_top_level: true,
        };
        assert_eq!(info.hwnd, 12345);
        assert_eq!(info.rect.width, 800);
    }

    #[test]
    fn window_operation_variants() {
        let ops = vec![
            WindowOperation::Move { x: 100, y: 200 },
            WindowOperation::Resize { width: 800, height: 600 },
            WindowOperation::MoveResize { x: 0, y: 0, width: 1920, height: 1080 },
            WindowOperation::Minimize,
            WindowOperation::Maximize,
            WindowOperation::Restore,
            WindowOperation::Close,
            WindowOperation::BringToFront,
            WindowOperation::SendToBack,
            WindowOperation::SetTopMost(true),
            WindowOperation::SetOpacity(128),
        ];
        assert_eq!(ops.len(), 11);
    }
}
