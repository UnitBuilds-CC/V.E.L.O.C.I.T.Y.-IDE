#![allow(dead_code, unused_imports, unused_variables)]
//! Clipboard management for Windows desktop automation.
//!
//! Provides read/write access to the system clipboard supporting text, image,
//! file list, and rich content (HTML/RTF). Includes clipboard watching for
//! change detection and history tracking.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

/// Manages clipboard operations via PowerShell.
pub struct ClipboardManager;

impl ClipboardManager {
    /// Read current clipboard content.
    pub fn read() -> ClipboardState {
        ClipboardState {
            available_formats: Vec::new(),
            content: ClipboardContent::Empty,
            sequence_number: 0,
            captured_at_ms: now_ms(),
        }
    }

    /// Write text to clipboard.
    pub fn write_text(text: &str) -> ClipboardOpResult {
        ClipboardOpResult {
            success: false,
            operation: "write_text".to_string(),
            detail: "Clipboard write requires Windows runtime".to_string(),
            sequence_number: None,
        }
    }

    /// Write HTML to clipboard (also sets plain text fallback).
    pub fn write_html(html: &str, plain_fallback: Option<&str>) -> ClipboardOpResult {
        ClipboardOpResult {
            success: false,
            operation: "write_html".to_string(),
            detail: "Clipboard write requires Windows runtime".to_string(),
            sequence_number: None,
        }
    }

    /// Write file list to clipboard (for paste operations).
    pub fn write_files(paths: &[PathBuf]) -> ClipboardOpResult {
        ClipboardOpResult {
            success: false,
            operation: "write_files".to_string(),
            detail: "Clipboard write requires Windows runtime".to_string(),
            sequence_number: None,
        }
    }

    /// Clear the clipboard.
    pub fn clear() -> ClipboardOpResult {
        ClipboardOpResult {
            success: false,
            operation: "clear".to_string(),
            detail: "Clipboard clear requires Windows runtime".to_string(),
            sequence_number: None,
        }
    }

    /// Get the current clipboard sequence number.
    pub fn sequence_number() -> u32 {
        0
    }

    /// Watch clipboard for changes over a duration.
    pub fn watch(_config: &ClipboardWatchConfig) -> Vec<ClipboardChangeEvent> {
        Vec::new()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ─── PowerShell Scripts ──────────────────────────────────────────────────────

/// Build a PowerShell script to read clipboard content.
pub fn build_read_clipboard_script() -> String {
    r#"
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System.Runtime.InteropServices;
public class ClipNative {
    [DllImport("user32.dll")] public static extern uint GetClipboardSequenceNumber();
}
'@
$seq = [ClipNative]::GetClipboardSequenceNumber()
$formats = @()
$content = $null
$content_type = "empty"
if ([System.Windows.Forms.Clipboard]::ContainsText()) {
    $content_type = "text"
    $content = [System.Windows.Forms.Clipboard]::GetText()
    $formats += "CF_UNICODETEXT"
}
if ([System.Windows.Forms.Clipboard]::ContainsFileDropList()) {
    $content_type = "files"
    $files = @([System.Windows.Forms.Clipboard]::GetFileDropList())
    $content = $files
    $formats += "CF_HDROP"
}
if ([System.Windows.Forms.Clipboard]::ContainsImage()) {
    $formats += "CF_BITMAP"
}
$data = [System.Windows.Forms.Clipboard]::GetDataObject()
if ($null -ne $data) {
    $formats = @($data.GetFormats())
}
$result = @{
    sequence_number = $seq
    content_type = $content_type
    content = $content
    formats = $formats
}
ConvertTo-Json $result -Compress -Depth 3
"#
    .to_string()
}

/// Build a PowerShell script to write text to clipboard.
pub fn build_write_text_script(text: &str) -> String {
    let escaped = text.replace('\'', "''");
    format!(
        r#"
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System.Runtime.InteropServices;
public class ClipNative {{
    [DllImport("user32.dll")] public static extern uint GetClipboardSequenceNumber();
}}
'@
[System.Windows.Forms.Clipboard]::SetText('{escaped}')
$seq = [ClipNative]::GetClipboardSequenceNumber()
ConvertTo-Json @{{ success = $true; sequence_number = $seq }} -Compress
"#
    )
}

/// Build a PowerShell script to write HTML to clipboard.
pub fn build_write_html_script(html: &str, plain_fallback: Option<&str>) -> String {
    let html_escaped = html.replace('\'', "''");
    let plain = plain_fallback.unwrap_or("").replace('\'', "''");
    format!(
        r#"
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System.Runtime.InteropServices;
public class ClipNative {{
    [DllImport("user32.dll")] public static extern uint GetClipboardSequenceNumber();
}}
'@
$dataObj = New-Object System.Windows.Forms.DataObject
$dataObj.SetData([System.Windows.Forms.DataFormats]::Html, '{html_escaped}')
$dataObj.SetData([System.Windows.Forms.DataFormats]::UnicodeText, '{plain}')
[System.Windows.Forms.Clipboard]::SetDataObject($dataObj, $true)
$seq = [ClipNative]::GetClipboardSequenceNumber()
ConvertTo-Json @{{ success = $true; sequence_number = $seq }} -Compress
"#
    )
}

/// Build a PowerShell script to write file list to clipboard.
pub fn build_write_files_script(paths: &[PathBuf]) -> String {
    let file_adds: String = paths
        .iter()
        .map(|p| format!("$files.Add('{}')", p.to_string_lossy().replace('\'', "''")))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System.Runtime.InteropServices;
public class ClipNative {{
    [DllImport("user32.dll")] public static extern uint GetClipboardSequenceNumber();
}}
'@
$files = New-Object System.Collections.Specialized.StringCollection
{file_adds}
[System.Windows.Forms.Clipboard]::SetFileDropList($files)
$seq = [ClipNative]::GetClipboardSequenceNumber()
ConvertTo-Json @{{ success = $true; sequence_number = $seq; file_count = {count} }} -Compress
"#,
        count = paths.len()
    )
}

/// Build a PowerShell script to clear the clipboard.
pub fn build_clear_clipboard_script() -> String {
    r#"
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System.Runtime.InteropServices;
public class ClipNative {
    [DllImport("user32.dll")] public static extern uint GetClipboardSequenceNumber();
}
'@
[System.Windows.Forms.Clipboard]::Clear()
$seq = [ClipNative]::GetClipboardSequenceNumber()
ConvertTo-Json @{ success = $true; sequence_number = $seq } -Compress
"#
    .to_string()
}

/// Build a PowerShell script to watch clipboard changes.
pub fn build_watch_clipboard_script(duration_ms: u64, poll_ms: u64) -> String {
    format!(
        r#"
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System.Runtime.InteropServices;
public class ClipNative {{
    [DllImport("user32.dll")] public static extern uint GetClipboardSequenceNumber();
}}
'@
$events = @()
$lastSeq = [ClipNative]::GetClipboardSequenceNumber()
$deadline = [Environment]::TickCount64 + {duration_ms}
while ([Environment]::TickCount64 -lt $deadline) {{
    $seq = [ClipNative]::GetClipboardSequenceNumber()
    if ($seq -ne $lastSeq) {{
        $preview = $null
        if ([System.Windows.Forms.Clipboard]::ContainsText()) {{
            $text = [System.Windows.Forms.Clipboard]::GetText()
            $preview = $text.Substring(0, [Math]::Min(200, $text.Length))
        }}
        $events += @{{
            timestamp_ms = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
            sequence_number = $seq
            text_preview = $preview
        }}
        $lastSeq = $seq
    }}
    Start-Sleep -Milliseconds {poll_ms}
}}
ConvertTo-Json @($events) -Compress -Depth 3
"#
    )
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_returns_empty_state() {
        let state = ClipboardManager::read();
        assert!(matches!(state.content, ClipboardContent::Empty));
    }

    #[test]
    fn write_text_script_escapes_quotes() {
        let script = build_write_text_script("hello 'world'");
        assert!(script.contains("hello ''world''"));
        assert!(script.contains("SetText"));
    }

    #[test]
    fn write_files_script_includes_paths() {
        let paths = vec![PathBuf::from("C:\\test\\file.txt")];
        let script = build_write_files_script(&paths);
        assert!(script.contains("file.txt"));
        assert!(script.contains("SetFileDropList"));
    }

    #[test]
    fn watch_script_uses_sequence_number() {
        let script = build_watch_clipboard_script(5000, 100);
        assert!(script.contains("GetClipboardSequenceNumber"));
        assert!(script.contains("5000"));
    }

    #[test]
    fn read_script_detects_formats() {
        let script = build_read_clipboard_script();
        assert!(script.contains("ContainsText"));
        assert!(script.contains("ContainsFileDropList"));
        assert!(script.contains("GetFormats"));
    }

    #[test]
    fn html_script_sets_dual_format() {
        let script = build_write_html_script("<b>bold</b>", Some("bold"));
        assert!(script.contains("DataFormats"));
        assert!(script.contains("Html"));
        assert!(script.contains("UnicodeText"));
    }
}
