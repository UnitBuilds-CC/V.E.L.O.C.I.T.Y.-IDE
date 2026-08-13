#![allow(dead_code)] // Reserved WA automation API surface; awaiting full MCP dispatch wiring.
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
    Html {
        html: String,
        source_url: Option<String>,
    },
    /// RTF rich text.
    Rtf(String),
    /// Image as raw RGBA pixels.
    Image {
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    },
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

    /// Write HTML to clipboard as real CF_HTML (also sets a plain-text fallback
    /// so non-HTML-aware targets still paste something sensible).
    pub fn write_html(html: &str, plain_fallback: Option<&str>) -> ClipboardOpResult {
        #[cfg(target_os = "windows")]
        {
            write_html_native(html, plain_fallback)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = plain_fallback;
            ClipboardOpResult {
                success: false,
                operation: "write_html".into(),
                detail: "Clipboard write requires Windows".into(),
                sequence_number: None,
            }
        }
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

// ─── Format builders (pure, platform-independent, unit-tested) ───────────────

/// Build a CF_HTML clipboard payload (UTF-8 bytes) per Microsoft's HTML
/// Clipboard Format spec. The byte offsets in the header are computed against
/// the final buffer; fixed-width (10-digit) offset fields keep the header a
/// stable length across the two-pass fill.
pub(crate) fn format_cf_html(fragment: &str, source_url: Option<&str>) -> Vec<u8> {
    const PREFIX: &str = "<html>\r\n<body>\r\n<!--StartFragment-->";
    const SUFFIX: &str = "<!--EndFragment-->\r\n</body>\r\n</html>";
    let source_line = match source_url {
        Some(u) => format!("SourceURL:{u}\r\n"),
        None => String::new(),
    };
    let make_header = |sh: usize, eh: usize, sf: usize, ef: usize| {
        format!(
            "Version:0.9\r\nStartHTML:{sh:010}\r\nEndHTML:{eh:010}\r\n\
             StartFragment:{sf:010}\r\nEndFragment:{ef:010}\r\n{source_line}"
        )
    };
    let header_len = make_header(0, 0, 0, 0).len();
    let start_html = header_len;
    let start_fragment = header_len + PREFIX.len();
    let end_fragment = start_fragment + fragment.len();
    let end_html = end_fragment + SUFFIX.len();
    let header = make_header(start_html, end_html, start_fragment, end_fragment);
    let mut out = String::with_capacity(end_html);
    out.push_str(&header);
    out.push_str(PREFIX);
    out.push_str(fragment);
    out.push_str(SUFFIX);
    out.into_bytes()
}

/// Build a CF_HDROP payload: a `DROPFILES` header (20 bytes) followed by a
/// double-null-terminated UTF-16LE list of file paths (`fWide = TRUE`).
pub(crate) fn build_hdrop_payload(paths: &[PathBuf]) -> Vec<u8> {
    let mut out = Vec::new();
    // DROPFILES { DWORD pFiles; POINT pt; BOOL fNC; BOOL fWide; }
    out.extend_from_slice(&20u32.to_le_bytes()); // pFiles: offset to file list
    out.extend_from_slice(&0i32.to_le_bytes()); // pt.x
    out.extend_from_slice(&0i32.to_le_bytes()); // pt.y
    out.extend_from_slice(&0u32.to_le_bytes()); // fNC = FALSE
    out.extend_from_slice(&1u32.to_le_bytes()); // fWide = TRUE (Unicode paths)
    for p in paths {
        for u in p.to_string_lossy().encode_utf16() {
            out.extend_from_slice(&u.to_le_bytes());
        }
        out.extend_from_slice(&0u16.to_le_bytes()); // null-terminate this path
    }
    out.extend_from_slice(&0u16.to_le_bytes()); // final double-null terminator
    out
}

// ─── Native Win32 Implementation ─────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod native {
    use super::*;
    use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
    use windows::Win32::System::DataExchange::*;
    use windows::Win32::System::Memory::*;
    use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

    // Clipboard format constants
    const CF_UNICODETEXT: u32 = 13;
    const CF_HDROP: u32 = 15;

    /// Read clipboard content using native Win32 API.
    pub fn read_clipboard_native() -> ClipboardState {
        let seq = get_sequence_number_native();
        let mut formats = Vec::new();
        let mut content = ClipboardContent::Empty;

        // SAFETY: Win32 clipboard read sequence.
        // - OpenClipboard(HWND::default()) associates the clipboard with the current task;
        //   if it fails (another window has it open), we return early with an empty state.
        // - GetClipboardData returns an HGLOBAL owned by the clipboard; we cast its HANDLE
        //   to HGLOBAL via `handle.0 as *mut _`, which is valid because CF_UNICODETEXT /
        //   CF_HDROP always yield a global-memory handle.
        // - GlobalLock pins the handle and returns a stable pointer valid until GlobalUnlock.
        //   We scan for the NUL terminator in the UTF-16 text case, then build a slice via
        //   from_raw_parts with the measured length — the pointer remains valid because
        //   GlobalUnlock is called only after the slice is consumed.
        // - For CF_HDROP, DragQueryFileW internally locks the handle; we pass index 0xFFFFFFFF
        //   to get the count, then iterate with per-file calls. The HDROP cast from HANDLE
        //   is valid because CF_HDROP always yields a DROPFILES global.
        // - CloseClipboard is always called before returning, releasing the clipboard lock.
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
                if let Ok(handle) = GetClipboardData(CF_HDROP) {
                    // The CF_HDROP handle is an HGLOBAL to a DROPFILES struct;
                    // DragQueryFileW walks it for us (it locks internally).
                    let hdrop = HDROP(handle.0 as *mut _);
                    // Index 0xFFFFFFFF asks for the file count.
                    let count = DragQueryFileW(hdrop, u32::MAX, None);
                    let mut files = Vec::with_capacity(count as usize);
                    for i in 0..count {
                        // A null buffer returns the required length in UTF-16
                        // code units (excluding the terminating NUL).
                        let needed = DragQueryFileW(hdrop, i, None);
                        if needed == 0 {
                            continue;
                        }
                        let mut buf = vec![0u16; needed as usize + 1];
                        let written = DragQueryFileW(hdrop, i, Some(&mut buf));
                        if written > 0 {
                            let s = String::from_utf16_lossy(&buf[..written as usize]);
                            files.push(PathBuf::from(s));
                        }
                    }
                    if !files.is_empty() {
                        content = ClipboardContent::Files(files);
                    }
                }
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
        // SAFETY: Win32 clipboard write sequence for CF_UNICODETEXT.
        // - OpenClipboard(HWND::default()) opens the clipboard for this task; early return on failure.
        // - The text is encoded to UTF-16 with a NUL terminator, so `wide.len()` includes the
        //   terminator and `byte_len = wide.len() * 2` is the exact byte count needed.
        // - GlobalAlloc(GMEM_MOVEABLE, byte_len) allocates a moveable global buffer of exactly
        //   the right size. On failure we close the clipboard and return an error.
        // - GlobalLock pins the buffer; copy_nonoverlapping copies `wide.len()` u16 elements
        //   from the Vec (valid source for `wide.len()` elements) into the locked pointer
        //   (valid destination for `byte_len / 2 = wide.len()` u16 elements). Regions do not
        //   overlap (separate allocations). GlobalUnlock is called immediately after.
        // - SetClipboardData transfers ownership of the HGLOBAL to the system clipboard;
        //   the handle must not be freed after success. CloseClipboard releases the lock.
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
            let byte_len = wide.len().saturating_mul(2);

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

    /// Copy `bytes` into a moveable global buffer and hand it to the clipboard
    /// under `format`. The clipboard must already be open. On success, ownership
    /// of the global transfers to the system (do not free it).
    ///
    /// SAFETY: The clipboard must be open (caller guarantees). `bytes` is a valid
    /// `&[u8]` slice. GlobalAlloc creates a buffer of `bytes.len()` bytes; GlobalLock
    /// pins it. copy_nonoverlapping is valid because source and destination are
    /// separate allocations of at least `bytes.len()` bytes. On success,
    /// SetClipboardData transfers ownership of the HGLOBAL to the system.
    unsafe fn set_clipboard_blob(format: u32, bytes: &[u8]) -> Result<(), String> {
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len())
            .map_err(|e| format!("GlobalAlloc failed: {:?}", e))?;
        let ptr = GlobalLock(handle);
        if ptr.is_null() {
            return Err("GlobalLock failed".into());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
        let _ = GlobalUnlock(handle);
        SetClipboardData(format, HANDLE(handle.0 as *mut _))
            .map(|_| ())
            .map_err(|e| format!("SetClipboardData failed: {:?}", e))
    }

    /// Write HTML to the clipboard as CF_HTML plus a CF_UNICODETEXT fallback so
    /// that non-HTML-aware targets still receive readable text.
    pub fn write_html_native(html: &str, plain_fallback: Option<&str>) -> ClipboardOpResult {
        let cf_html = format_cf_html(html, None);
        let fallback: Vec<u16> = plain_fallback
            .unwrap_or(html)
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: Win32 clipboard write sequence for HTML + text fallback.
        // - OpenClipboard(HWND::default()) opens the clipboard; early return on failure.
        // - RegisterClipboardFormatW("HTML Format") registers/looks up the CF_HTML format ID.
        //   A return of 0 means registration failed; we skip the HTML blob in that case.
        // - set_clipboard_blob internally allocates GlobalAlloc, locks, copies, and calls
        //   SetClipboardData. The cf_html Vec<u8> is a valid byte slice. On success,
        //   ownership of the HGLOBAL transfers to the clipboard.
        // - The fallback text is reinterpreted from Vec<u16> to &[u8] via from_raw_parts:
        //   `fallback.as_ptr()` is valid for `fallback.len()` u16 elements = `fallback.len() * 2`
        //   bytes. The Vec outlives the set_clipboard_blob call.
        // - CloseClipboard is always called before returning.
        unsafe {
            if OpenClipboard(HWND::default()).is_err() {
                return ClipboardOpResult {
                    success: false,
                    operation: "write_html".into(),
                    detail: "Failed to open clipboard".into(),
                    sequence_number: None,
                };
            }
            let _ = EmptyClipboard();

            let html_format = RegisterClipboardFormatW(windows::core::w!("HTML Format"));
            if html_format != 0 {
                if let Err(e) = set_clipboard_blob(html_format, &cf_html) {
                    let _ = CloseClipboard();
                    return ClipboardOpResult {
                        success: false,
                        operation: "write_html".into(),
                        detail: e,
                        sequence_number: None,
                    };
                }
            }

            let fallback_bytes = std::slice::from_raw_parts(
                fallback.as_ptr() as *const u8,
                fallback.len().saturating_mul(2),
            );
            let text_result = set_clipboard_blob(CF_UNICODETEXT, fallback_bytes);
            let _ = CloseClipboard();

            match text_result {
                Ok(()) => ClipboardOpResult {
                    success: true,
                    operation: "write_html".into(),
                    detail: "ok".into(),
                    sequence_number: Some(get_sequence_number_native()),
                },
                Err(e) => ClipboardOpResult {
                    success: false,
                    operation: "write_html".into(),
                    detail: e,
                    sequence_number: None,
                },
            }
        }
    }

    /// Write a file list to the clipboard as CF_HDROP (real drag-drop payload,
    /// so Explorer and other shell targets accept a subsequent paste).
    pub fn write_files_native(paths: &[PathBuf]) -> ClipboardOpResult {
        let payload = build_hdrop_payload(paths);
        // SAFETY: Win32 clipboard write for CF_HDROP.
        // - OpenClipboard(HWND::default()) opens the clipboard; early return on failure.
        // - build_hdrop_payload produces a valid DROPFILES + double-null-terminated UTF-16
        //   file list. set_clipboard_blob allocates a global buffer, copies the payload,
        //   and transfers ownership via SetClipboardData.
        // - CloseClipboard is always called before returning.
        unsafe {
            if OpenClipboard(HWND::default()).is_err() {
                return ClipboardOpResult {
                    success: false,
                    operation: "write_files".into(),
                    detail: "Failed to open clipboard".into(),
                    sequence_number: None,
                };
            }
            let _ = EmptyClipboard();
            let result = set_clipboard_blob(CF_HDROP, &payload);
            let _ = CloseClipboard();
            match result {
                Ok(()) => ClipboardOpResult {
                    success: true,
                    operation: "write_files".into(),
                    detail: "ok".into(),
                    sequence_number: Some(get_sequence_number_native()),
                },
                Err(e) => ClipboardOpResult {
                    success: false,
                    operation: "write_files".into(),
                    detail: e,
                    sequence_number: None,
                },
            }
        }
    }

    /// Clear the clipboard.
    pub fn clear_clipboard_native() -> ClipboardOpResult {
        // SAFETY: Win32 clipboard clear sequence.
        // - OpenClipboard(HWND::default()) opens the clipboard; early return on failure.
        // - EmptyClipboard removes all content. CloseClipboard always follows, releasing
        //   the clipboard lock regardless of EmptyClipboard's result.
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
                detail: if result.is_ok() {
                    "ok".into()
                } else {
                    "EmptyClipboard failed".into()
                },
                sequence_number: Some(get_sequence_number_native()),
            }
        }
    }

    /// Get clipboard sequence number.
    pub fn get_sequence_number_native() -> u32 {
        // SAFETY: GetClipboardSequenceNumber is a pure query that returns a monotonically
        // increasing u32. It takes no parameters and has no pointer or lifetime requirements.
        unsafe { GetClipboardSequenceNumber() }
    }
}

#[cfg(target_os = "windows")]
use native::{
    clear_clipboard_native, get_sequence_number_native, read_clipboard_native, write_files_native,
    write_html_native, write_text_native,
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

    /// Live round-trip through the real Win32 clipboard: writes a CF_HDROP
    /// payload and reads it back via `DragQueryFileW`. Ignored by default
    /// because it clobbers the interactive clipboard; run explicitly with
    /// `cargo test --bin velocity_mcp -- --ignored hdrop_live_round_trip`.
    ///
    /// The system clipboard is a shared, contended resource (clipboard
    /// history and cloud-sync agents may grab it mid-test), so the
    /// write/read is retried a few times before we treat a miss as a real
    /// extraction failure.
    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "touches the live system clipboard; run manually"]
    fn hdrop_live_round_trip_extracts_written_files() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("alpha.txt");
        // A space exercises wide-path handling through DragQueryFileW.
        let b = dir.path().join("beta log.txt");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();

        let mut last: Option<ClipboardContent> = None;
        for attempt in 0..5 {
            let written = ClipboardManager::write_files(&[a.clone(), b.clone()]);
            assert!(written.success, "write_files failed: {}", written.detail);

            let state = ClipboardManager::read();
            if let ClipboardContent::Files(files) = &state.content {
                assert!(
                    state.available_formats.iter().any(|f| f == "CF_HDROP"),
                    "CF_HDROP not advertised: {:?}",
                    state.available_formats
                );
                assert_eq!(files.len(), 2, "expected two files, got {:?}", files);
                assert!(
                    files.iter().any(|p| p == &a),
                    "missing {:?} in {:?}",
                    a,
                    files
                );
                assert!(
                    files.iter().any(|p| p == &b),
                    "missing {:?} in {:?}",
                    b,
                    files
                );
                return;
            }
            last = Some(state.content);
            std::thread::sleep(std::time::Duration::from_millis(50 * (attempt + 1)));
        }
        panic!(
            "clipboard never returned our files after retries; last content: {:?}",
            last
        );
    }

    #[test]
    fn clipboard_content_variants() {
        let contents = [
            ClipboardContent::Text("hello".to_string()),
            ClipboardContent::Html {
                html: "<b>bold</b>".to_string(),
                source_url: None,
            },
            ClipboardContent::Rtf("{\\rtf1}".to_string()),
            ClipboardContent::Image {
                width: 100,
                height: 100,
                pixels: vec![0; 40000],
            },
            ClipboardContent::Files(vec![PathBuf::from("test.txt")]),
            ClipboardContent::Raw {
                format_name: "Custom".to_string(),
                data: vec![1, 2, 3],
            },
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

    #[test]
    fn cf_html_offsets_are_consistent() {
        let bytes = format_cf_html("<b>hi</b>", None);
        let text = String::from_utf8(bytes).unwrap();
        // Header advertises fixed-width offsets that must index the real buffer.
        let field = |name: &str| -> usize {
            let line = text.lines().find(|l| l.starts_with(name)).unwrap();
            line[name.len()..].trim().parse().unwrap()
        };
        let start_html = field("StartHTML:");
        let end_html = field("EndHTML:");
        let start_fragment = field("StartFragment:");
        let end_fragment = field("EndFragment:");
        assert_eq!(end_html, text.len());
        assert!(start_html < start_fragment);
        assert!(start_fragment < end_fragment);
        assert!(end_fragment <= end_html);
        // The fragment offsets must bracket exactly the caller's HTML.
        assert_eq!(&text[start_fragment..end_fragment], "<b>hi</b>");
        assert_eq!(&text[start_html..start_html + 6], "<html>");
    }

    #[test]
    fn cf_html_includes_source_url_when_given() {
        let text = String::from_utf8(format_cf_html("<p>x</p>", Some("https://ex.com"))).unwrap();
        assert!(text.contains("SourceURL:https://ex.com"));
        // Offsets still land on the fragment despite the extra header line.
        let sf: usize = text
            .lines()
            .find(|l| l.starts_with("StartFragment:"))
            .unwrap()["StartFragment:".len()..]
            .trim()
            .parse()
            .unwrap();
        let ef: usize = text
            .lines()
            .find(|l| l.starts_with("EndFragment:"))
            .unwrap()["EndFragment:".len()..]
            .trim()
            .parse()
            .unwrap();
        assert_eq!(&text[sf..ef], "<p>x</p>");
    }

    #[test]
    fn hdrop_payload_has_dropfiles_header_and_double_null() {
        let payload = build_hdrop_payload(&[PathBuf::from("C:/a.txt"), PathBuf::from("C:/b.txt")]);
        // pFiles offset == 20, fWide == 1.
        assert_eq!(&payload[0..4], &20u32.to_le_bytes());
        assert_eq!(&payload[16..20], &1u32.to_le_bytes());
        // Ends with a double-null (two zero u16 = 4 zero bytes: per-path null + terminator).
        assert_eq!(&payload[payload.len() - 4..], &[0u8, 0, 0, 0]);
        // Decode the wide list back and confirm both paths round-trip.
        let wide: Vec<u16> = payload[20..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let joined: String = String::from_utf16(&wide).unwrap();
        assert!(joined.contains("C:/a.txt"));
        assert!(joined.contains("C:/b.txt"));
    }

    #[test]
    fn hdrop_empty_list_is_just_header_and_terminator() {
        let payload = build_hdrop_payload(&[]);
        assert_eq!(payload.len(), 20 + 2); // header + single terminating null u16
    }
}
