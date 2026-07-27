#![allow(dead_code)] // Reserved WA automation API surface; awaiting full MCP dispatch wiring.
//! Browser↔Desktop bridge for unified cross-context automation workflows.
//!
//! Coordinates between the velocity-browser CDP/WebSocket automation layer
//! and the desktop WA UIAutomation layer to enable workflows that span
//! browser and native app interactions (e.g., download→open, copy→paste,
//! upload via file dialog).

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

// ─── Bridge Model ────────────────────────────────────────────────────────────

/// A cross-context automation workflow step.
#[derive(Debug, Clone)]
pub enum BridgeStep {
    /// Execute an action in the browser context (CDP).
    Browser(BrowserAction),
    /// Execute an action in the desktop context (WA).
    Desktop(DesktopAction),
    /// Wait for a condition across both contexts.
    CrossContextWait(CrossContextCondition),
    /// Transfer data between contexts.
    DataTransfer(DataTransferOp),
}

/// Browser-side action.
#[derive(Debug, Clone)]
pub enum BrowserAction {
    /// Navigate to a URL.
    Navigate { url: String },
    /// Click an element by selector.
    Click { selector: String },
    /// Type into an element.
    Type { selector: String, text: String },
    /// Trigger file download.
    Download { url: String, expected_filename: Option<String> },
    /// Trigger file upload (opens file dialog).
    TriggerUpload { input_selector: String },
    /// Execute JavaScript.
    EvalJs { script: String },
    /// Wait for an element to appear.
    WaitForElement { selector: String, timeout_ms: u64 },
}

/// Desktop-side action.
#[derive(Debug, Clone)]
pub enum DesktopAction {
    /// Open a file with its default application.
    OpenFile { path: PathBuf },
    /// Focus a window by title.
    FocusWindow { title_contains: String },
    /// Type into the focused element.
    TypeText { text: String },
    /// Handle a file dialog (set path and confirm).
    HandleFileDialog { path: PathBuf },
    /// Click a UI element by name.
    ClickElement { name: String, role: Option<String> },
    /// Copy selection to clipboard.
    CopyToClipboard,
    /// Paste from clipboard.
    PasteFromClipboard,
}

/// Cross-context wait condition.
#[derive(Debug, Clone)]
pub enum CrossContextCondition {
    /// Wait for a file to appear on disk (e.g., after download).
    FileAppears { path: PathBuf, timeout: Duration },
    /// Wait for a window to appear (after launching from browser).
    WindowAppears { title_contains: String, timeout: Duration },
    /// Wait for the browser to navigate to a URL pattern.
    BrowserNavigates { url_contains: String, timeout: Duration },
    /// Wait for clipboard to contain specific text.
    ClipboardContains { text: String, timeout: Duration },
    /// Wait for a process to start.
    ProcessStarts { name: String, timeout: Duration },
}

/// Data transfer between contexts.
#[derive(Debug, Clone)]
pub enum DataTransferOp {
    /// Copy text from browser element to clipboard, then paste in desktop app.
    BrowserToDesktop { browser_selector: String, desktop_target: String },
    /// Copy from desktop app to clipboard, then paste in browser element.
    DesktopToBrowser { desktop_source: String, browser_selector: String },
    /// Save downloaded file and open with desktop app.
    DownloadAndOpen { download_url: String, app_exe: Option<String> },
    /// Read text from desktop app for use in browser.
    ReadDesktopText { element_name: String },
}

// ─── Bridge Workflow ─────────────────────────────────────────────────────────

/// A complete cross-context workflow.
#[derive(Debug, Clone)]
pub struct BridgeWorkflow {
    /// Workflow name.
    pub name: String,
    /// Ordered steps.
    pub steps: Vec<BridgeStep>,
    /// Global timeout for the entire workflow.
    pub timeout: Duration,
    /// Whether to abort on first failure.
    pub fail_fast: bool,
}

/// Result of a bridge workflow step.
#[derive(Debug, Clone)]
pub struct BridgeStepResult {
    pub step_index: usize,
    pub context: String, // "browser" or "desktop" or "cross"
    pub success: bool,
    pub detail: String,
    pub elapsed: Duration,
    pub data: Option<String>,
}

/// Result of a complete workflow execution.
#[derive(Debug, Clone)]
pub struct BridgeWorkflowResult {
    pub workflow_name: String,
    pub succeeded: bool,
    pub total_elapsed: Duration,
    pub steps: Vec<BridgeStepResult>,
    pub stopped_at: Option<usize>,
}

// ─── Bridge Executor ─────────────────────────────────────────────────────────

/// Executes cross-context workflows.
pub struct BridgeExecutor {
    /// Download directory for browser downloads.
    pub download_dir: PathBuf,
    /// Default timeout for cross-context waits.
    pub default_timeout: Duration,
}

impl BridgeExecutor {
    pub fn new(download_dir: PathBuf) -> Self {
        Self {
            download_dir,
            default_timeout: Duration::from_secs(30),
        }
    }

    /// Execute a complete workflow.
    pub fn execute(&self, workflow: &BridgeWorkflow) -> BridgeWorkflowResult {
        let start = Instant::now();
        let mut step_results = Vec::with_capacity(workflow.steps.len());
        let mut stopped_at = None;

        for (i, step) in workflow.steps.iter().enumerate() {
            let step_start = Instant::now();
            let result = self.execute_step_inner(step, i);
            let step_result = BridgeStepResult {
                step_index: i,
                context: result.0,
                success: result.1,
                detail: result.2,
                elapsed: step_start.elapsed(),
                data: result.3,
            };
            let success = step_result.success;
            step_results.push(step_result);
            if !success && workflow.fail_fast {
                stopped_at = Some(i);
                break;
            }
        }

        BridgeWorkflowResult {
            workflow_name: workflow.name.clone(),
            succeeded: stopped_at.is_none(),
            total_elapsed: start.elapsed(),
            steps: step_results,
            stopped_at,
        }
    }

    /// Execute a single step.
    pub fn execute_step(&self, step: &BridgeStep) -> BridgeStepResult {
        let start = Instant::now();
        let (context, success, detail, data) = self.execute_step_inner(step, 0);
        BridgeStepResult {
            step_index: 0,
            context,
            success,
            detail,
            elapsed: start.elapsed(),
            data,
        }
    }

    fn execute_step_inner(&self, step: &BridgeStep, _index: usize) -> (String, bool, String, Option<String>) {
        if !cfg!(target_os = "windows") {
            return ("unknown".into(), false, "Bridge executor requires Windows runtime".into(), None);
        }
        match step {
            BridgeStep::Browser(action) => self.execute_browser_action(action),
            BridgeStep::Desktop(action) => self.execute_desktop_action(action),
            BridgeStep::CrossContextWait(condition) => self.execute_cross_wait(condition),
            BridgeStep::DataTransfer(transfer) => self.execute_data_transfer(transfer),
        }
    }

    fn execute_browser_action(&self, action: &BrowserAction) -> (String, bool, String, Option<String>) {
        // Browser actions are delegated to the CDP layer; here we generate
        // the instruction payload for the browser bridge to pick up.
        match action {
            BrowserAction::Navigate { url } =>
                ("browser".into(), true, format!("navigate:{}", url), None),
            BrowserAction::Click { selector } =>
                ("browser".into(), true, format!("click:{}", selector), None),
            BrowserAction::Type { selector, text } =>
                ("browser".into(), true, format!("type:{}:{}", selector, text), None),
            BrowserAction::Download { url, expected_filename } => {
                let detail = format!("download:{}", url);
                ("browser".into(), true, detail, expected_filename.clone())
            }
            BrowserAction::TriggerUpload { input_selector } =>
                ("browser".into(), true, format!("trigger_upload:{}", input_selector), None),
            BrowserAction::EvalJs { script } =>
                ("browser".into(), true, format!("eval:{}", script.chars().take(100).collect::<String>()), None),
            BrowserAction::WaitForElement { selector, timeout_ms } =>
                ("browser".into(), true, format!("wait_element:{}:{}ms", selector, timeout_ms), None),
        }
    }

    fn execute_desktop_action(&self, action: &DesktopAction) -> (String, bool, String, Option<String>) {
        match action {
            DesktopAction::OpenFile { path } => {
                let script = build_open_file_script(path);
                match run_ps_script(&script) {
                    Ok(json) => ("desktop".into(), true, json, None),
                    Err(e) => ("desktop".into(), false, e, None),
                }
            }
            DesktopAction::FocusWindow { title_contains } => {
                let script = format!(
                    "$w = Get-Process | Where-Object {{ $_.MainWindowTitle -like '*{}*' }} | Select-Object -First 1; \
                     if ($null -ne $w) {{ ConvertTo-Json @{{ success = $true; pid = $w.Id }} -Compress }} else {{ ConvertTo-Json @{{ success = $false }} -Compress }}",
                    title_contains.replace('\'', "''")
                );
                match run_ps_script(&script) {
                    Ok(json) => ("desktop".into(), true, json, None),
                    Err(e) => ("desktop".into(), false, e, None),
                }
            }
            DesktopAction::TypeText { text } => {
                let script = format!(
                    "Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait('{}')",
                    text.replace('\'', "''")
                );
                match run_ps_script(&script) {
                    Ok(_) => ("desktop".into(), true, "typed".into(), None),
                    Err(e) => ("desktop".into(), false, e, None),
                }
            }
            DesktopAction::HandleFileDialog { path } => {
                let script = format!(
                    "Add-Type -AssemblyName System.Windows.Forms; Start-Sleep -Milliseconds 500; \
                     [System.Windows.Forms.SendKeys]::SendWait('{}')",
                    path.to_string_lossy().replace('\'', "''")
                );
                match run_ps_script(&script) {
                    Ok(_) => ("desktop".into(), true, "file_dialog_handled".into(), None),
                    Err(e) => ("desktop".into(), false, e, None),
                }
            }
            DesktopAction::ClickElement { name, role } => {
                let name_esc = name.replace('\'', "''");
                let script = if let Some(role_str) = role {
                    let role_esc = role_str.replace('\'', "''");
                    format!(
                        r#"Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$root = [System.Windows.Automation.AutomationElement]::RootElement
$el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
    (New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::NameProperty, '{name_esc}')))
if ($null -eq $el) {{
    ConvertTo-Json @{{ success = $false; detail = "element not found: {name_esc}" }} -Compress
}} else {{
    try {{
        $invoke = $el.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
        $invoke.Invoke()
        ConvertTo-Json @{{ success = $true; name = '{name_esc}'; role = '{role_esc}' }} -Compress
    }} catch {{
        # Fallback: try click via InvokePattern on parent
        ConvertTo-Json @{{ success = $false; detail = $_.Exception.Message }} -Compress
    }}
}}"#
                    )
                } else {
                    format!(
                        r#"Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$root = [System.Windows.Automation.AutomationElement]::RootElement
$el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
    (New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::NameProperty, '{name_esc}')))
if ($null -eq $el) {{
    ConvertTo-Json @{{ success = $false; detail = "element not found: {name_esc}" }} -Compress
}} else {{
    try {{
        $invoke = $el.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
        $invoke.Invoke()
        ConvertTo-Json @{{ success = $true; name = '{name_esc}' }} -Compress
    }} catch {{
        ConvertTo-Json @{{ success = $false; detail = $_.Exception.Message }} -Compress
    }}
}}"#
                    )
                };
                match run_ps_script(&script) {
                    Ok(json) => ("desktop".into(), true, json, None),
                    Err(e) => ("desktop".into(), false, e, None),
                }
            }
            DesktopAction::CopyToClipboard => {
                let script = build_clipboard_transfer_script("copy", 200);
                match run_ps_script(&script) {
                    Ok(json) => ("desktop".into(), true, json, None),
                    Err(e) => ("desktop".into(), false, e, None),
                }
            }
            DesktopAction::PasteFromClipboard => {
                let script = build_clipboard_transfer_script("paste", 200);
                match run_ps_script(&script) {
                    Ok(json) => ("desktop".into(), true, json, None),
                    Err(e) => ("desktop".into(), false, e, None),
                }
            }
        }
    }

    fn execute_cross_wait(&self, condition: &CrossContextCondition) -> (String, bool, String, Option<String>) {
        match condition {
            CrossContextCondition::FileAppears { path, timeout } => {
                let script = build_wait_for_file_script(path, timeout.as_millis() as u64);
                match run_ps_script(&script) {
                    Ok(json) => ("cross".into(), true, json, None),
                    Err(e) => ("cross".into(), false, e, None),
                }
            }
            CrossContextCondition::WindowAppears { title_contains, timeout } => {
                let deadline_ms = timeout.as_millis() as u64;
                let script = format!(
                    "$deadline = [Environment]::TickCount64 + {deadline_ms}; \
                     $found = $false; \
                     while ([Environment]::TickCount64 -lt $deadline) {{ \
                         $w = Get-Process | Where-Object {{ $_.MainWindowTitle -like '*{title}*' }}; \
                         if ($null -ne $w) {{ $found = $true; break }}; \
                         Start-Sleep -Milliseconds 200 \
                     }}; \
                     ConvertTo-Json @{{ found = $found }} -Compress",
                    title = title_contains.replace('\'', "''")
                );
                match run_ps_script(&script) {
                    Ok(json) => ("cross".into(), true, json, None),
                    Err(e) => ("cross".into(), false, e, None),
                }
            }
            CrossContextCondition::BrowserNavigates { url_contains, timeout } =>
                ("cross".into(), true, format!("wait_browser_nav:{}:{}ms", url_contains, timeout.as_millis()), None),
            CrossContextCondition::ClipboardContains { text, timeout } => {
                let script = format!(
                    "Add-Type -AssemblyName System.Windows.Forms; \
                     $deadline = [Environment]::TickCount64 + {timeout_ms}; \
                     $found = $false; \
                     while ([Environment]::TickCount64 -lt $deadline) {{ \
                         $clip = [System.Windows.Forms.Clipboard]::GetText(); \
                         if ($clip -like '*{text}*') {{ $found = $true; break }}; \
                         Start-Sleep -Milliseconds 200 \
                     }}; \
                     ConvertTo-Json @{{ found = $found }} -Compress",
                    timeout_ms = timeout.as_millis() as u64,
                    text = text.replace('\'', "''")
                );
                match run_ps_script(&script) {
                    Ok(json) => ("cross".into(), true, json, None),
                    Err(e) => ("cross".into(), false, e, None),
                }
            }
            CrossContextCondition::ProcessStarts { name, timeout } => {
                let script = format!(
                    "$deadline = [Environment]::TickCount64 + {timeout_ms}; \
                     $found = $false; \
                     while ([Environment]::TickCount64 -lt $deadline) {{ \
                         $p = Get-Process -Name '{name}' -ErrorAction SilentlyContinue; \
                         if ($null -ne $p) {{ $found = $true; break }}; \
                         Start-Sleep -Milliseconds 200 \
                     }}; \
                     ConvertTo-Json @{{ found = $found }} -Compress",
                    timeout_ms = timeout.as_millis() as u64,
                    name = name.replace('\'', "''")
                );
                match run_ps_script(&script) {
                    Ok(json) => ("cross".into(), true, json, None),
                    Err(e) => ("cross".into(), false, e, None),
                }
            }
        }
    }

    fn execute_data_transfer(&self, transfer: &DataTransferOp) -> (String, bool, String, Option<String>) {
        match transfer {
            DataTransferOp::BrowserToDesktop { browser_selector, .. } => {
                // Copy from browser (delegated), then paste on desktop
                let copy_script = build_clipboard_transfer_script("copy", 300);
                match run_ps_script(&copy_script) {
                    Ok(json) => ("cross".into(), true, format!("browser_to_desktop:{}", browser_selector), Some(json)),
                    Err(e) => ("cross".into(), false, e, None),
                }
            }
            DataTransferOp::DesktopToBrowser { desktop_source, .. } => {
                let paste_script = build_clipboard_transfer_script("paste", 300);
                match run_ps_script(&paste_script) {
                    Ok(json) => ("cross".into(), true, format!("desktop_to_browser:{}", desktop_source), Some(json)),
                    Err(e) => ("cross".into(), false, e, None),
                }
            }
            DataTransferOp::DownloadAndOpen { download_url, app_exe } => {
                // Determine filename from URL
                let filename = download_url.rsplit('/').next().unwrap_or("download");
                let dest = self.download_dir.join(filename);
                let dest_str = dest.to_string_lossy().replace('\'', "''");
                let url_esc = download_url.replace('\'', "''");

                // Build download script
                let script = format!(
                    r#"$url = '{url_esc}'
$dest = '{dest_str}'
try {{
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    $wc = New-Object System.Net.WebClient
    $wc.DownloadFile($url, $dest)
    if (Test-Path $dest) {{
        $size = (Get-Item $dest).Length
        {open_clause}
        ConvertTo-Json @{{ success = $true; path = $dest; size = $size }} -Compress
    }} else {{
        ConvertTo-Json @{{ success = $false; detail = "download completed but file not found" }} -Compress
    }}
}} catch {{
    ConvertTo-Json @{{ success = $false; detail = $_.Exception.Message }} -Compress
}}"#,
                    open_clause = if let Some(app) = app_exe {
                        let app_esc = app.replace('\'', "''");
                        format!("Start-Process -FilePath '{}' -ArgumentList $dest", app_esc)
                    } else {
                        "Start-Process -FilePath $dest".to_string()
                    }
                );
                match run_ps_script(&script) {
                    Ok(json) => ("cross".into(), true, json, Some(dest_str)),
                    Err(e) => ("cross".into(), false, e, None),
                }
            }
            DataTransferOp::ReadDesktopText { element_name } => {
                let script = format!(
                    "Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes; \
                     $root = [System.Windows.Automation.AutomationElement]::RootElement; \
                     $el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, \
                         (New-Object System.Windows.Automation.PropertyCondition( \
                         [System.Windows.Automation.AutomationElement]::NameProperty, '{}'))); \
                     if ($null -ne $el) {{ \
                         $text = $el.Current.Name; \
                         ConvertTo-Json @{{ success = $true; text = $text }} -Compress \
                     }} else {{ ConvertTo-Json @{{ success = $false }} -Compress }}",
                    element_name.replace('\'', "''")
                );
                match run_ps_script(&script) {
                    Ok(json) => ("cross".into(), true, json, None),
                    Err(e) => ("cross".into(), false, e, None),
                }
            }
        }
    }
}

// ─── PowerShell Scripts ──────────────────────────────────────────────────────

/// Build a script to wait for a file to appear on disk.
pub fn build_wait_for_file_script(path: &PathBuf, timeout_ms: u64) -> String {
    let path_str = path.to_string_lossy().replace('\'', "''");
    format!(
        r#"
$path = '{path_str}'
$deadline = [Environment]::TickCount64 + {timeout_ms}
$found = $false
while ([Environment]::TickCount64 -lt $deadline) {{
    if (Test-Path $path) {{
        $file = Get-Item $path
        # Wait for file to stop growing (download complete)
        $size1 = $file.Length
        Start-Sleep -Milliseconds 500
        $file.Refresh()
        if ($file.Length -eq $size1 -and $file.Length -gt 0) {{
            $found = $true
            break
        }}
    }}
    Start-Sleep -Milliseconds 200
}}
ConvertTo-Json @{{ found = $found; path = $path; size = if ($found) {{ (Get-Item $path).Length }} else {{ 0 }} }} -Compress
"#
    )
}

/// Build a script to open a file with its default application.
pub fn build_open_file_script(path: &PathBuf) -> String {
    let path_str = path.to_string_lossy().replace('\'', "''");
    format!(
        r#"
$path = '{path_str}'
$proc = Start-Process -FilePath $path -PassThru
Start-Sleep -Seconds 1
$result = @{{
    success = ($null -ne $proc)
    pid = if ($null -ne $proc) {{ $proc.Id }} else {{ 0 }}
    path = $path
}}
ConvertTo-Json $result -Compress
"#
    )
}

/// Build a script for clipboard-based data transfer.
pub fn build_clipboard_transfer_script(direction: &str, wait_ms: u64) -> String {
    format!(
        r#"
Add-Type -AssemblyName System.Windows.Forms
$direction = '{direction}'
if ($direction -eq 'copy') {{
    [System.Windows.Forms.SendKeys]::SendWait('^c')
    Start-Sleep -Milliseconds {wait_ms}
    $text = [System.Windows.Forms.Clipboard]::GetText()
    ConvertTo-Json @{{ success = $true; direction = 'copy'; text = $text }} -Compress
}} else {{
    [System.Windows.Forms.SendKeys]::SendWait('^v')
    Start-Sleep -Milliseconds {wait_ms}
    ConvertTo-Json @{{ success = $true; direction = 'paste' }} -Compress
}}
"#
    )
}

fn run_ps_script(script: &str) -> Result<String, String> {
    let mut child = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn powershell: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(script.as_bytes()).map_err(|e| format!("stdin write: {e}"))?;
    }
    let output = child.wait_with_output().map_err(|e| format!("wait: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("PowerShell error: {}", stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_construction() {
        let workflow = BridgeWorkflow {
            name: "download-and-open".to_string(),
            steps: vec![
                BridgeStep::Browser(BrowserAction::Download {
                    url: "https://example.com/file.pdf".to_string(),
                    expected_filename: Some("file.pdf".to_string()),
                }),
                BridgeStep::CrossContextWait(CrossContextCondition::FileAppears {
                    path: PathBuf::from("C:\\Downloads\\file.pdf"),
                    timeout: Duration::from_secs(30),
                }),
                BridgeStep::Desktop(DesktopAction::OpenFile {
                    path: PathBuf::from("C:\\Downloads\\file.pdf"),
                }),
            ],
            timeout: Duration::from_secs(60),
            fail_fast: true,
        };
        assert_eq!(workflow.steps.len(), 3);
    }

    #[test]
    fn wait_for_file_script_includes_path() {
        let script = build_wait_for_file_script(
            &PathBuf::from("C:\\Downloads\\test.pdf"),
            10000,
        );
        assert!(script.contains("test.pdf"));
        assert!(script.contains("Test-Path"));
        assert!(script.contains("10000"));
    }

    #[test]
    fn open_file_script_uses_start_process() {
        let script = build_open_file_script(&PathBuf::from("C:\\file.docx"));
        assert!(script.contains("Start-Process"));
        assert!(script.contains("file.docx"));
    }

    #[test]
    fn clipboard_transfer_script_handles_copy() {
        let script = build_clipboard_transfer_script("copy", 200);
        assert!(script.contains("^c"));
        assert!(script.contains("GetText"));
    }

    #[test]
    fn bridge_executor_defaults() {
        let executor = BridgeExecutor::new(PathBuf::from("C:\\Downloads"));
        assert_eq!(executor.default_timeout, Duration::from_secs(30));
    }
}
