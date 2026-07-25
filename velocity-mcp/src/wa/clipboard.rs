#![allow(dead_code, unused_imports, unused_variables)]
//! Clipboard management for Windows desktop automation.
//!
//! Provides read/write access to the system clipboard supporting text, image,
//! file list, and rich content (HTML/RTF). Includes clipboard watching for
//! change detection and history tracking.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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
        if !cfg!(target_os = "windows") {
            return ClipboardState {
                available_formats: Vec::new(),
                content: ClipboardContent::Empty,
                sequence_number: 0,
                captured_at_ms: now_ms(),
            };
        }
        let script = build_read_clipboard_script();
        match run_ps_script(&script) {
            Ok(json) => parse_clipboard_read_result(&json),
            Err(_) => ClipboardState {
                available_formats: Vec::new(),
                content: ClipboardContent::Empty,
                sequence_number: 0,
                captured_at_ms: now_ms(),
            },
        }
    }

    /// Write text to clipboard.
    pub fn write_text(text: &str) -> ClipboardOpResult {
        if !cfg!(target_os = "windows") {
            return ClipboardOpResult {
                success: false, operation: "write_text".into(),
                detail: "Clipboard write requires Windows".into(), sequence_number: None,
            };
        }
        let script = build_write_text_script(text);
        match run_ps_script(&script) {
            Ok(json) => parse_clipboard_write_result(&json, "write_text"),
            Err(e) => ClipboardOpResult { success: false, operation: "write_text".into(), detail: e, sequence_number: None },
        }
    }

    /// Write HTML to clipboard (also sets plain text fallback).
    pub fn write_html(html: &str, plain_fallback: Option<&str>) -> ClipboardOpResult {
        if !cfg!(target_os = "windows") {
            return ClipboardOpResult {
                success: false, operation: "write_html".into(),
                detail: "Clipboard write requires Windows".into(), sequence_number: None,
            };
        }
        let script = build_write_html_script(html, plain_fallback);
        match run_ps_script(&script) {
            Ok(json) => parse_clipboard_write_result(&json, "write_html"),
            Err(e) => ClipboardOpResult { success: false, operation: "write_html".into(), detail: e, sequence_number: None },
        }
    }

    /// Write file list to clipboard (for paste operations).
    pub fn write_files(paths: &[PathBuf]) -> ClipboardOpResult {
        if !cfg!(target_os = "windows") {
            return ClipboardOpResult {
                success: false, operation: "write_files".into(),
                detail: "Clipboard write requires Windows".into(), sequence_number: None,
            };
        }
        let script = build_write_files_script(paths);
        match run_ps_script(&script) {
            Ok(json) => parse_clipboard_write_result(&json, "write_files"),
            Err(e) => ClipboardOpResult { success: false, operation: "write_files".into(), detail: e, sequence_number: None },
        }
    }

    /// Clear the clipboard.
    pub fn clear() -> ClipboardOpResult {
        if !cfg!(target_os = "windows") {
            return ClipboardOpResult {
                success: false, operation: "clear".into(),
                detail: "Clipboard clear requires Windows".into(), sequence_number: None,
            };
        }
        let script = build_clear_clipboard_script();
        match run_ps_script(&script) {
            Ok(json) => parse_clipboard_write_result(&json, "clear"),
            Err(e) => ClipboardOpResult { success: false, operation: "clear".into(), detail: e, sequence_number: None },
        }
    }

    /// Get the current clipboard sequence number.
    pub fn sequence_number() -> u32 {
        if !cfg!(target_os = "windows") { return 0; }
        let script = build_read_clipboard_script();
        match run_ps_script(&script) {
            Ok(json) => parse_clipboard_read_result(&json).sequence_number,
            Err(_) => 0,
        }
    }

    /// Watch clipboard for changes over a duration.
    pub fn watch(config: &ClipboardWatchConfig) -> Vec<ClipboardChangeEvent> {
        if !cfg!(target_os = "windows") { return Vec::new(); }
        let script = build_watch_clipboard_script(
            config.duration.as_millis() as u64,
            config.poll_interval.as_millis() as u64,
        );
        match run_ps_script(&script) {
            Ok(json) => parse_watch_events(&json),
            Err(_) => Vec::new(),
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

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

fn parse_clipboard_read_result(json: &str) -> ClipboardState {
    #[derive(serde::Deserialize)]
    struct PsClipResult {
        sequence_number: Option<u32>,
        content_type: Option<String>,
        content: Option<serde_json::Value>,
        formats: Option<Vec<String>>,
    }
    match serde_json::from_str::<PsClipResult>(json) {
        Ok(r) => {
            let content = match r.content_type.as_deref() {
                Some("text") => ClipboardContent::Text(
                    r.content.as_ref().and_then(|v| v.as_str()).unwrap_or("").to_string()
                ),
                Some("files") => {
                    let files = r.content.as_ref()
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(PathBuf::from)).collect())
                        .unwrap_or_default();
                    ClipboardContent::Files(files)
                },
                _ => ClipboardContent::Empty,
            };
            ClipboardState {
                available_formats: r.formats.unwrap_or_default(),
                content,
                sequence_number: r.sequence_number.unwrap_or(0),
                captured_at_ms: now_ms(),
            }
        }
        Err(_) => ClipboardState {
            available_formats: Vec::new(),
            content: ClipboardContent::Empty,
            sequence_number: 0,
            captured_at_ms: now_ms(),
        },
    }
}

fn parse_clipboard_write_result(json: &str, op: &str) -> ClipboardOpResult {
    #[derive(serde::Deserialize)]
    struct PsWriteResult {
        success: Option<bool>,
        sequence_number: Option<u32>,
    }
    match serde_json::from_str::<PsWriteResult>(json) {
        Ok(r) => ClipboardOpResult {
            success: r.success.unwrap_or(true),
            operation: op.to_string(),
            detail: "ok".to_string(),
            sequence_number: r.sequence_number,
        },
        Err(e) => ClipboardOpResult {
            success: false, operation: op.to_string(),
            detail: format!("parse error: {e}"), sequence_number: None,
        },
    }
}

fn parse_watch_events(json: &str) -> Vec<ClipboardChangeEvent> {
    #[derive(serde::Deserialize)]
    struct PsEvent {
        timestamp_ms: Option<u64>,
        sequence_number: Option<u32>,
        text_preview: Option<String>,
    }
    serde_json::from_str::<Vec<PsEvent>>(json)
        .unwrap_or_default()
        .into_iter()
        .map(|e| ClipboardChangeEvent {
            timestamp_ms: e.timestamp_ms.unwrap_or(0),
            sequence_number: e.sequence_number.unwrap_or(0),
            source_process: None,
            formats: Vec::new(),
            text_preview: e.text_preview,
        })
        .collect()
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
