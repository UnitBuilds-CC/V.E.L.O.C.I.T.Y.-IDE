#![cfg_attr(not(windows), allow(dead_code))]
// Win32-backed window surface: wired and live on Windows (force-warn clean); the OS APIs are absent on other platforms.
//! Window management for Windows desktop automation.
//!
//! Provides window enumeration, positional control (move, resize, minimize,
//! maximize, restore, close), z-order management, and window state queries
//! via direct Win32 API calls (zero PowerShell overhead).

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    MoveResize {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
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
        Self::enumerate_windows()
            .into_iter()
            .find(|w| w.is_foreground)
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

    /// Work out the grid cell for each of `count` windows tiled across a
    /// `monitor_width` x `monitor_height` area.
    ///
    /// Separated from [`WindowManager::tile_windows`] so the layout can be
    /// asserted without moving any real window.
    pub fn tile_rects(
        count: usize,
        monitor_width: u32,
        monitor_height: u32,
        columns: Option<u32>,
    ) -> Vec<WindowRect> {
        if count == 0 {
            return Vec::new();
        }
        let cols = Self::tile_columns(count, columns);
        let rows = (count as u32).div_ceil(cols);
        let tile_w = monitor_width / cols;
        let tile_h = monitor_height / rows;

        (0..count)
            .map(|i| WindowRect {
                x: ((i as u32 % cols) * tile_w) as i32,
                y: ((i as u32 / cols) * tile_h) as i32,
                width: tile_w,
                height: tile_h,
            })
            .collect()
    }

    /// Column count actually used for a tile grid: honour the caller's request,
    /// clamped to something meaningful, or fall back to a square-ish layout.
    fn tile_columns(count: usize, columns: Option<u32>) -> u32 {
        let upper = count.max(1) as u32;
        match columns {
            Some(c) => c.clamp(1, upper),
            None => ((count as f64).sqrt().ceil() as u32).clamp(1, upper),
        }
    }

    /// Arrange `hwnds` in a grid across a `monitor_width` x `monitor_height` area.
    ///
    /// `columns` honours the caller's request (bug #37: `wa_window_tile` advertised
    /// "2-column, 3-column, or custom" but the value was never read and the grid
    /// was always `sqrt(n)`). `None` keeps the automatic square-ish layout.
    pub fn tile_windows(
        hwnds: &[u64],
        monitor_width: u32,
        monitor_height: u32,
        columns: Option<u32>,
    ) -> Vec<WindowOpResult> {
        Self::tile_rects(hwnds.len(), monitor_width, monitor_height, columns)
            .into_iter()
            .zip(hwnds.iter())
            .map(|(cell, &hwnd)| {
                Self::apply_operation(
                    hwnd,
                    &WindowOperation::MoveResize {
                        x: cell.x,
                        y: cell.y,
                        width: cell.width,
                        height: cell.height,
                    },
                )
            })
            .collect()
    }

    /// Cascade windows with offset.
    pub fn cascade_windows(
        hwnds: &[u64],
        start_x: i32,
        start_y: i32,
        offset: i32,
    ) -> Vec<WindowOpResult> {
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
        // SAFETY: GetForegroundWindow is a pure query with no pointer requirements;
        // it simply returns the handle of the foreground window (or null).
        let fg_hwnd = unsafe { GetForegroundWindow() };

        // SAFETY: EnumWindows takes a callback function pointer and an LPARAM.
        // We pass `&mut windows as *mut Vec<WindowInfo> as isize` as the LPARAM.
        // The pointer is valid for the duration of the EnumWindows call because
        // `windows` is a local variable that outlives the call. The callback
        // (enum_windows_callback) casts it back to `&mut Vec<WindowInfo>`, which
        // is sound because EnumWindows is synchronous and single-threaded.
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

    // SAFETY: This is a Win32 callback invoked by EnumWindows. The `lparam` is a pointer
    // to `Vec<WindowInfo>` that was passed by `enumerate_windows_native`. It is valid
    // for the duration of the enumeration because the Vec outlives the EnumWindows call.
    // All Win32 API calls (IsWindowVisible, GetWindowTextLengthW, GetWindowTextW,
    // GetClassNameW, GetWindowThreadProcessId, GetWindowRect, IsIconic, IsZoomed)
    // take a valid HWND from the enumeration callback and operate on it safely.
    // GetWindowTextW and GetClassNameW write into pre-allocated buffers of sufficient size.
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
                width: (rect.right - rect.left).max(0) as u32,
                height: (rect.bottom - rect.top).max(0) as u32,
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

        // SAFETY: All Win32 calls in this block take `hwnd` constructed from the caller's
        // `hwnd_val: u64`. If the value is an invalid HWND, the Win32 calls will simply
        // fail gracefully (return false/Err). Each call is documented:
        // - GetWindowRect: writes into a stack-allocated RECT, valid for the call.
        // - MoveWindow: takes position/size integers; no pointer requirements.
        // - ShowWindow: takes an HWND and a show command; no pointer requirements.
        // - SendMessageW: sends WM_CLOSE with zeroed WPARAM/LPARAM.
        // - SetForegroundWindow: takes an HWND.
        // - SetWindowPos: takes an HWND, insertion order, and integers.
        // - GetWindowLongW/SetWindowLongW: read/modify extended window styles.
        // - SetLayeredWindowAttributes: takes an HWND, color key, alpha byte, and flags.
        let success = unsafe {
            match op {
                WindowOperation::Move { x, y } => {
                    let mut rect = RECT::default();
                    let _ = GetWindowRect(hwnd, &mut rect);
                    MoveWindow(
                        hwnd,
                        *x,
                        *y,
                        rect.right - rect.left,
                        rect.bottom - rect.top,
                        true,
                    )
                    .is_ok()
                }
                WindowOperation::Resize { width, height } => {
                    let mut rect = RECT::default();
                    let _ = GetWindowRect(hwnd, &mut rect);
                    MoveWindow(
                        hwnd,
                        rect.left,
                        rect.top,
                        *width as i32,
                        *height as i32,
                        true,
                    )
                    .is_ok()
                }
                WindowOperation::MoveResize {
                    x,
                    y,
                    width,
                    height,
                } => MoveWindow(hwnd, *x, *y, *width as i32, *height as i32, true).is_ok(),
                WindowOperation::Minimize => ShowWindow(hwnd, SW_MINIMIZE).as_bool(),
                WindowOperation::Maximize => ShowWindow(hwnd, SW_MAXIMIZE).as_bool(),
                WindowOperation::Restore => ShowWindow(hwnd, SW_RESTORE).as_bool(),
                WindowOperation::Close => {
                    let _ = SendMessageW(
                        hwnd,
                        WM_CLOSE,
                        windows::Win32::Foundation::WPARAM(0),
                        windows::Win32::Foundation::LPARAM(0),
                    );
                    true
                }
                WindowOperation::BringToFront => SetForegroundWindow(hwnd).as_bool(),
                WindowOperation::SendToBack => SetWindowPos(
                    hwnd,
                    HWND_BOTTOM,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                )
                .is_ok(),
                WindowOperation::SetTopMost(true) => SetWindowPos(
                    hwnd,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                )
                .is_ok(),
                WindowOperation::SetTopMost(false) => SetWindowPos(
                    hwnd,
                    HWND_NOTOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                )
                .is_ok(),
                WindowOperation::SetOpacity(alpha) => {
                    // Set WS_EX_LAYERED style
                    let style = GetWindowLongW(hwnd, GWL_EXSTYLE);
                    SetWindowLongW(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED.0 as i32);
                    SetLayeredWindowAttributes(
                        hwnd,
                        windows::Win32::Foundation::COLORREF(0),
                        *alpha,
                        LWA_ALPHA,
                    )
                    .is_ok()
                }
            }
        };

        // Get new rect after operation
        // SAFETY: GetWindowRect writes into a stack-allocated RECT. The `hwnd` may be
        // invalid, in which case GetWindowRect returns Err and we produce None.
        let new_rect = unsafe {
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_ok() {
                Some(WindowRect {
                    x: rect.left,
                    y: rect.top,
                    width: (rect.right - rect.left).max(0) as u32,
                    height: (rect.bottom - rect.top).max(0) as u32,
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
        let results = WindowManager::tile_windows(&[1, 2, 3, 4], 1920, 1080, None);
        assert_eq!(results.len(), 4);
    }

    #[test]
    fn tile_rects_falls_back_to_a_square_grid() {
        let cells = WindowManager::tile_rects(4, 1920, 1080, None);
        assert_eq!(cells.len(), 4);
        assert_eq!(
            cells[0],
            WindowRect {
                x: 0,
                y: 0,
                width: 960,
                height: 540
            }
        );
        assert_eq!(
            cells[1],
            WindowRect {
                x: 960,
                y: 0,
                width: 960,
                height: 540
            }
        );
        assert_eq!(
            cells[3],
            WindowRect {
                x: 960,
                y: 540,
                width: 960,
                height: 540
            }
        );
    }

    // Bug #37: the `columns` argument was advertised but never read, so asking
    // for a 1-column stack silently produced a square grid.
    #[test]
    fn tile_rects_honours_an_explicit_column_count() {
        let stacked = WindowManager::tile_rects(4, 1920, 1080, Some(1));
        assert_eq!(stacked.len(), 4);
        for (i, cell) in stacked.iter().enumerate() {
            assert_eq!(cell.x, 0, "single column must not advance x: {cell:?}");
            assert_eq!(cell.y, (i as i32) * 270, "rows must stack: {cell:?}");
            assert_eq!((cell.width, cell.height), (1920, 270));
        }

        let three = WindowManager::tile_rects(5, 900, 600, Some(3));
        // 3 columns -> 2 rows, so cells wrap on the fourth window.
        assert_eq!(
            three[2],
            WindowRect {
                x: 600,
                y: 0,
                width: 300,
                height: 300
            }
        );
        assert_eq!(
            three[3],
            WindowRect {
                x: 0,
                y: 300,
                width: 300,
                height: 300
            }
        );
        assert_eq!(
            three[4],
            WindowRect {
                x: 300,
                y: 300,
                width: 300,
                height: 300
            }
        );
    }

    #[test]
    fn tile_rects_clamps_nonsensical_column_requests() {
        // More columns than windows would divide the area by zero-sized cells.
        let cells = WindowManager::tile_rects(2, 800, 600, Some(9));
        assert_eq!(cells.len(), 2);
        assert_eq!(
            cells[0],
            WindowRect {
                x: 0,
                y: 0,
                width: 400,
                height: 600
            }
        );
        assert_eq!(
            cells[1],
            WindowRect {
                x: 400,
                y: 0,
                width: 400,
                height: 600
            }
        );

        assert_eq!(WindowManager::tile_rects(3, 900, 300, Some(0)).len(), 3);
        // Column 0 would have been an infinite/zero division before the clamp.
        let zero = WindowManager::tile_rects(3, 900, 300, Some(0));
        assert_eq!(
            zero[2],
            WindowRect {
                x: 0,
                y: 200,
                width: 900,
                height: 100
            }
        );
    }

    #[test]
    fn tile_rects_covers_every_window_without_shrinking() {
        for count in [1usize, 3, 8, 12] {
            let cells = WindowManager::tile_rects(count, 2048, 1104, None);
            assert_eq!(cells.len(), count, "count {count}");
            assert!(
                cells.iter().all(|c| c.width > 0 && c.height > 0),
                "count {count} produced a degenerate cell: {cells:?}"
            );
            // Every origin must stay inside the monitor.
            assert!(
                cells
                    .iter()
                    .all(|c| (c.x as u32) < 2048 && (c.y as u32) < 1104),
                "count {count} tiled off-screen: {cells:?}"
            );
        }
        assert!(WindowManager::tile_rects(0, 100, 100, Some(3)).is_empty());
    }

    #[test]
    fn tile_windows_returns_one_result_per_handle() {
        // Unreachable handles fail cleanly rather than panicking, and the
        // per-window accounting still lines up with the request.
        let results = WindowManager::tile_windows(&[1, 2], 800, 600, Some(1));
        assert_eq!(results.len(), 2);
        for (i, r) in results.iter().enumerate() {
            assert_eq!(r.hwnd, (i + 1) as u64);
        }
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
            rect: WindowRect {
                x: 0,
                y: 0,
                width: 800,
                height: 600,
            },
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
            WindowOperation::Resize {
                width: 800,
                height: 600,
            },
            WindowOperation::MoveResize {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
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

    // apply_operation executes the real native path and reports success/failure
    // rather than a hardcoded stub. An invalid HWND (0) must fail cleanly while
    // still echoing the operation for audit.
    #[cfg(target_os = "windows")]
    #[test]
    fn apply_operation_on_invalid_hwnd_reports_failure() {
        let result = WindowManager::apply_operation(0, &WindowOperation::Move { x: 10, y: 10 });
        assert_eq!(result.hwnd, 0);
        assert!(result.operation.contains("Move"));
        assert!(
            !result.success,
            "operation on a null hwnd should not succeed"
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn apply_operation_reports_unsupported_off_windows() {
        let result = WindowManager::apply_operation(1, &WindowOperation::Minimize);
        assert!(!result.success);
        assert!(result.operation.contains("Minimize"));
    }
}
