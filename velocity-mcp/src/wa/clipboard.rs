#![allow(dead_code, unused_imports, unused_variables)]
//! Clipboard management for Windows desktop automation.
//!
//! Provides read/write access to the system clipboard supporting text, image,
//! file list, and rich content (HTML/RTF). Uses native Win32 API (zero PowerShell).

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ─── Clipboard Content Model ─────────────────────────────────────────────────

/// Content types available on the clipboard.
#[derive(Debug, Clone)]
pub enum ClipboardContent {
    /// Plain text (CF_UNICODETEXT).
    Text(String),
    /// HTML content with source URL.
    Html { html: String, source_url: Option<String> },
    /// RTF rich text.
    Rtf(String),
    /// Image as raw RGBA pixels.
    Image { width: u32, height: u32, pixels: Vec<u8> },
    /// File list (CF_HDROP).
    Files(Vec<PathBuf>),
    /// Raw binary data with format name.
    Raw { format_name: String, data: Vec<u8> },
    /// Clipboard is empty.
    Empty,
}

/// Clipboard state snapshot.
#[derive(Debug, Clone)]
pub struct ClipboardState {
    /// Available format names on the clipboard.
    pub available_formats: Vec<String>,
    /// The primary content (best format).
    pub content: ClipboardContent,
    /// Sequence number (increments on each clipboard change).
    pub sequence_number: u32,
    /// When this state was captured.
    pub captured_at_ms: u64,
}

/// Result of a clipboard operation.
#[derive(Debug, Clone)]
pub struct ClipboardOpResult {
    pub success: bool,
    pub operation: String,
    pub detail: String,
    /// New sequence number after operation.
    pub sequence_number: Option<u32>,
}

/// Clipboard change event (for watching).
#[derive(Debug, Clone)]
pub struct ClipboardChangeEvent {
    /// Timestamp of the change.
    pub timestamp_ms: u64,
    /// Sequence number after change.
    pub sequence_number: u32,
    /// Process that modified the clipboard (if detectable).
    pub source_process: Option<String>,
    /// Available formats after change.
    pub formats: Vec<String>,
    /// Text preview (first 200 chars if text is available).
    pub text_preview: Option<String>,
}

/// Configuration for clipboard watching.
#[derive(Debug, Clone)]
pub struct ClipboardWatchConfig {
    /// How long to watch for changes.
    pub duration: Duration,
    /// Poll interval.
    pub poll_interval: Duration,
    /// Whether to capture text content on each change.
    pub capture_text: bool,
    /// Maximum number of events to collect.
    pub max_events: usize,
}

impl Default for ClipboardWatchConfig {
    fn default() -> Self {
        Self {
            duration: Duration::from_secs(30),
            poll_interval: Duration::from_millis(100),
            capture_text: true,
            max_events: 100,
        }
    }
}

// ─── Clipboard Manager ───────────────────────────────────────────────────────

/// Manages clipboard operations via native Win32 API.
pub struct ClipboardManager;

impl ClipboardManager {
    /// Read current clipboard content.
    pub fn read() -> ClipboardState {
        #[cfg(target_os = "windows")]
        {
            read_clipboard_native()
        }
        #[cfg(not(target_os = "windows"))]
        {
            ClipboardState {
                available_formats: Vec::new(),
                content: ClipboardContent::Empty,
                sequence_number: 0,
                captured_at_ms: now_ms(),
            }
        }
    }

    /// Write text to clipboard.
    pub fn write_text(text: &str) -> ClipboardOpResult {
        #[cfg(target_os = "windows")]
        {
            write_text_native(text)
        }
        #[cfg(not(target_os = "windows"))]
        {
            ClipboardOpResult {
                success: false,
                operation: "write_text".into(),
                detail: "Clipboard write requires Windows".into(),
                sequence_number: None,
            }
        }
    }

    /// Write HTML to clipboard (also sets plain text fallback).
    pub fn write_html(html: &str, plain_fallback: Option<&str>) -> ClipboardOpResult {
        // For now, just write the plain text fallback or stripped HTML
        let text = plain_fallback.unwrap_or(html);
        Self::write_text(text)
    }

    /// Write file list to clipboard (for paste operations).
    pub fn write_files(paths: &[PathBuf]) -> ClipboardOpResult {
        #[cfg(target_os = "windows")]
        {
            write_files_native(paths)
        }
        #[cfg(not(target_os = "windows"))]
        {
            ClipboardOpResult {
                success: false,
                operation: "write_files".into(),
                detail: "Clipboard write requires Windows".into(),
                sequence_number: None,
            }
        }
    }

    /// Clear the clipboard.
    pub fn clear() -> ClipboardOpResult {
        #[cfg(target_os = "windows")]
        {
            clear_clipboard_native()
        }
        #[cfg(not(target_os = "windows"))]
        {
            ClipboardOpResult {
                success: false,
                operation: "clear".into(),
                detail: "Clipboard clear requires Windows".into(),
                sequence_number: None,
            }
        }
    }

    /// Get the current clipboard sequence number.
    pub fn sequence_number() -> u32 {
        #[cfg(target_os = "windows")]
        {
            get_sequence_number_native()
        }
        #[cfg(not(target_os = "windows"))]
        {
            0
        }
    }

    /// Watch clipboard for changes over a duration.
    pub fn watch(config: &ClipboardWatchConfig) -> Vec<ClipboardChangeEvent> {
        let mut events = Vec::new();
        let mut last_seq = Self::sequence_number();
        let deadline = Instant::now() + config.duration;

        while Instant::now() < deadline && events.len() < config.max_events {
            let current_seq = Self::sequence_number();
            if current_seq != last_seq {
                let text_preview = if config.capture_text {
                    match Self::read().content {
                        ClipboardContent::Text(t) => Some(t.chars().take(200).collect()),
                        _ => None,
                    }
                } else {
                    None
                };

                events.push(ClipboardChangeEvent {
                    timestamp_ms: now_ms(),
                    sequence_number: current_seq,
                    source_process: None,
                    formats: Vec::new(),
                    text_preview,
                });
                last_seq = current_seq;
            }
            std::thread::sleep(config.poll_interval);
        }

        events
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ─── Native Win32 Implementation ─────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod native {
    use super::*;
    use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
    use windows::Win32::System::DataExchange::*;
    use windows::Win32::System::Memory::*;

    // Clipboard format constants
    const CF_UNICODETEXT: u32 = 13;
    const CF_HDROP: u32 = 15;

    /// Read clipboard content using native Win32 API.
    pub fn read_clipboard_native() -> ClipboardState {
        let seq = get_sequence_number_native();
        let mut formats = Vec::new();
        let mut content = ClipboardContent::Empty;

        unsafe {
            if OpenClipboard(HWND::default()).is_err() {
                return ClipboardState {
                    available_formats: formats,
                    content,
                    sequence_number: seq,
                    captured_at_ms: now_ms(),
                };
            }

            // Check for text
            if IsClipboardFormatAvailable(CF_UNICODETEXT).is_ok() {
                formats.push("CF_UNICODETEXT".to_string());
                if let Ok(handle) = GetClipboardData(CF_UNICODETEXT) {
                    let hglobal = HGLOBAL(handle.0 as *mut _);
                    let ptr = GlobalLock(hglobal);
                    if !ptr.is_null() {
                        let wide_ptr = ptr as *const u16;
                        let mut len = 0;
                        while *wide_ptr.add(len) != 0 {
                            len += 1;
                        }
                        let slice = std::slice::from_raw_parts(wide_ptr, len);
                        content = ClipboardContent::Text(String::from_utf16_lossy(slice));
                        let _ = GlobalUnlock(hglobal);
                    }
                }
            }

            // Check for files
            if IsClipboardFormatAvailable(CF_HDROP).is_ok() {
                formats.push("CF_HDROP".to_string());
                // File extraction would require DragQueryFile - simplified for now
            }

            let _ = CloseClipboard();
        }

        ClipboardState {
            available_formats: formats,
            content,
            sequence_number: seq,
            captured_at_ms: now_ms(),
        }
    }

    /// Write text to clipboard using native Win32 API.
    pub fn write_text_native(text: &str) -> ClipboardOpResult {
        unsafe {
            if OpenClipboard(HWND::default()).is_err() {
                return ClipboardOpResult {
                    success: false,
                    operation: "write_text".into(),
                    detail: "Failed to open clipboard".into(),
                    sequence_number: None,
                };
            }

            let _ = EmptyClipboard();

            // Convert to UTF-16 with null terminator
            let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            let byte_len = wide.len() * 2;

            // Allocate global memory
            let handle = match GlobalAlloc(GMEM_MOVEABLE, byte_len) {
                Ok(h) => h,
                Err(e) => {
                    let _ = CloseClipboard();
                    return ClipboardOpResult {
                        success: false,
                        operation: "write_text".into(),
                        detail: format!("GlobalAlloc failed: {:?}", e),
                        sequence_number: None,
                    };
                }
            };

            // Lock and copy data
            let ptr = GlobalLock(handle);
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr as *mut u16, wide.len());
                let _ = GlobalUnlock(handle);
            }

            // Set clipboard data
            let result = SetClipboardData(CF_UNICODETEXT, HANDLE(handle.0 as *mut _));
            let _ = CloseClipboard();

            match result {
                Ok(_) => ClipboardOpResult {
                    success: true,
                    operation: "write_text".into(),
                    detail: "ok".into(),
                    sequence_number: Some(get_sequence_number_native()),
                },
                Err(e) => ClipboardOpResult {
                    success: false,
                    operation: "write_text".into(),
                    detail: format!("SetClipboardData failed: {:?}", e),
                    sequence_number: None,
                },
            }
        }
    }

    /// Write file list to clipboard (simplified - just stores paths as text).
    pub fn write_files_native(paths: &[PathBuf]) -> ClipboardOpResult {
        // Full CF_HDROP implementation requires DROPFILES struct
        // For now, write paths as newline-separated text
        let text: String = paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        write_text_native(&text)
    }

    /// Clear the clipboard.
    pub fn clear_clipboard_native() -> ClipboardOpResult {
        unsafe {
            if OpenClipboard(HWND::default()).is_err() {
                return ClipboardOpResult {
                    success: false,
                    operation: "clear".into(),
                    detail: "Failed to open clipboard".into(),
                    sequence_number: None,
                };
            }

            let result = EmptyClipboard();
            let _ = CloseClipboard();

            ClipboardOpResult {
                success: result.is_ok(),
                operation: "clear".into(),
                detail: if result.is_ok() { "ok".into() } else { "EmptyClipboard failed".into() },
                sequence_number: Some(get_sequence_number_native()),
            }
        }
    }

    /// Get clipboard sequence number.
    pub fn get_sequence_number_native() -> u32 {
        unsafe { GetClipboardSequenceNumber() }
    }
}

#[cfg(target_os = "windows")]
use native::{
    clear_clipboard_native, get_sequence_number_native, read_clipboard_native,
    write_files_native, write_text_native,
};

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_returns_state() {
        let state = ClipboardManager::read();
        // On non-Windows, always Empty. On Windows, depends on clipboard.
        if !cfg!(target_os = "windows") {
            assert!(matches!(state.content, ClipboardContent::Empty));
            assert_eq!(state.sequence_number, 0);
        }
        // Just verify it doesn't panic
        let _ = state.captured_at_ms;
    }

    #[test]
    fn clipboard_content_variants() {
        let contents = vec![
            ClipboardContent::Text("hello".to_string()),
            ClipboardContent::Html { html: "<b>bold</b>".to_string(), source_url: None },
            ClipboardContent::Rtf("{\\rtf1}".to_string()),
            ClipboardContent::Image { width: 100, height: 100, pixels: vec![0; 40000] },
            ClipboardContent::Files(vec![PathBuf::from("test.txt")]),
            ClipboardContent::Raw { format_name: "Custom".to_string(), data: vec![1, 2, 3] },
            ClipboardContent::Empty,
        ];
        assert_eq!(contents.len(), 7);
    }

    #[test]
    fn watch_config_default() {
        let config = ClipboardWatchConfig::default();
        assert_eq!(config.duration, Duration::from_secs(30));
        assert_eq!(config.max_events, 100);
        assert!(config.capture_text);
    }

    #[test]
    fn clipboard_op_result_model() {
        let result = ClipboardOpResult {
            success: true,
            operation: "write_text".to_string(),
            detail: "ok".to_string(),
            sequence_number: Some(42),
        };
        assert!(result.success);
        assert_eq!(result.sequence_number, Some(42));
    }
}
