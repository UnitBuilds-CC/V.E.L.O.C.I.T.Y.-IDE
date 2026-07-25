#![allow(dead_code, unused_imports, unused_variables)]
//! File and folder dialog automation for Windows desktop automation.
//!
//! Handles interaction with common Open/Save/Browse dialogs, folder pickers,
//! and file system operations that bridge between the automation layer and
//! the Windows Explorer shell dialogs.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

// ─── Dialog Types ────────────────────────────────────────────────────────────

/// Types of file dialogs that can be automated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileDialogKind {
    /// Standard Open File dialog.
    Open,
    /// Standard Save As dialog.
    SaveAs,
    /// Folder/Browse picker dialog.
    FolderBrowse,
    /// Multi-select Open dialog.
    OpenMultiple,
}

/// Target file dialog to interact with.
#[derive(Debug, Clone)]
pub struct FileDialogTarget {
    /// Process ID that owns the dialog.
    pub process_id: Option<u32>,
    /// Window title pattern (case-insensitive contains).
    pub title_contains: Option<String>,
    /// Kind of dialog expected.
    pub kind: FileDialogKind,
    /// Timeout for dialog to appear.
    pub wait_timeout: Duration,
}

impl Default for FileDialogTarget {
    fn default() -> Self {
        Self {
            process_id: None,
            title_contains: None,
            kind: FileDialogKind::Open,
            wait_timeout: Duration::from_secs(5),
        }
    }
}

/// Operation to perform on a file dialog.
#[derive(Debug, Clone)]
pub enum FileDialogAction {
    /// Type a path into the filename field and click Open/Save.
    SetPath(PathBuf),
    /// Navigate to a folder in the dialog.
    NavigateToFolder(PathBuf),
    /// Select a file filter (e.g., "All Files (*.*)" or "Text Files (*.txt)").
    SetFilter(String),
    /// Click Cancel/Close.
    Cancel,
    /// Get the currently entered path.
    GetCurrentPath,
    /// Select multiple files (for OpenMultiple).
    SelectFiles(Vec<PathBuf>),
}

/// Result of a file dialog operation.
#[derive(Debug, Clone)]
pub struct FileDialogResult {
    pub success: bool,
    pub action: String,
    pub detail: String,
    /// The path that was ultimately selected/entered.
    pub selected_path: Option<PathBuf>,
    /// Multiple paths for multi-select.
    pub selected_paths: Vec<PathBuf>,
}

// ─── File Dialog Manager ─────────────────────────────────────────────────────

/// Manages file dialog automation.
pub struct FileDialogManager;

impl FileDialogManager {
    /// Detect if a file dialog is currently open.
    pub fn detect_dialog(target: &FileDialogTarget) -> Option<FileDialogInfo> {
        if !cfg!(target_os = "windows") { return None; }
        let script = build_detect_dialog_script(target);
        match run_ps_script(&script) {
            Ok(json) => parse_dialog_info(&json),
            Err(_) => None,
        }
    }

    /// Perform an action on a detected file dialog.
    pub fn perform(
        target: &FileDialogTarget,
        action: &FileDialogAction,
    ) -> FileDialogResult {
        if !cfg!(target_os = "windows") {
            return FileDialogResult {
                success: false, action: "unknown".into(),
                detail: "File dialog automation requires Windows runtime".into(),
                selected_path: None, selected_paths: Vec::new(),
            };
        }
        let script = build_file_dialog_script(target, action);
        match run_ps_script(&script) {
            Ok(json) => parse_dialog_result(&json, action),
            Err(e) => FileDialogResult {
                success: false,
                action: format!("{:?}", action),
                detail: e,
                selected_path: None,
                selected_paths: Vec::new(),
            },
        }
    }

    /// Convenience: Set path and confirm (Open/Save) in one step.
    pub fn quick_set_path(path: &Path, dialog_kind: FileDialogKind) -> FileDialogResult {
        Self::perform(
            &FileDialogTarget {
                kind: dialog_kind,
                ..Default::default()
            },
            &FileDialogAction::SetPath(path.to_path_buf()),
        )
    }

    /// Wait for a file dialog to appear, then interact with it.
    pub fn wait_and_set_path(
        path: &Path,
        target: &FileDialogTarget,
    ) -> FileDialogResult {
        if !cfg!(target_os = "windows") {
            return FileDialogResult {
                success: false, action: "wait_and_set_path".into(),
                detail: "File dialog automation requires Windows runtime".into(),
                selected_path: None, selected_paths: Vec::new(),
            };
        }
        // The build_file_dialog_script already includes a wait loop with timeout
        let action = FileDialogAction::SetPath(path.to_path_buf());
        Self::perform(target, &action)
    }
}

/// Information about a detected file dialog.
#[derive(Debug, Clone)]
pub struct FileDialogInfo {
    /// HWND of the dialog.
    pub hwnd: u64,
    /// Process owning the dialog.
    pub process_id: u32,
    /// Dialog title.
    pub title: String,
    /// Detected dialog kind.
    pub kind: FileDialogKind,
    /// Current folder path shown in the dialog.
    pub current_folder: Option<PathBuf>,
    /// Current filename field value.
    pub current_filename: Option<String>,
    /// Available file type filters.
    pub filters: Vec<String>,
}

// ─── PowerShell Scripts ──────────────────────────────────────────────────────

/// Build a PowerShell script to detect and interact with a file dialog.
pub fn build_file_dialog_script(target: &FileDialogTarget, action: &FileDialogAction) -> String {
    let title_filter = target
        .title_contains
        .as_deref()
        .unwrap_or("Open|Save|Browse|Select");
    let pid_clause = target
        .process_id
        .map(|p| format!("$targetPid = {p}"))
        .unwrap_or_else(|| "$targetPid = $null".to_string());
    let timeout_ms = target.wait_timeout.as_millis();

    let action_code = match action {
        FileDialogAction::SetPath(path) => {
            let path_str = path.to_string_lossy().replace('\'', "''");
            format!(
                r#"
$fileNameEdit = $dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
    (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty,
    [System.Windows.Automation.ControlType]::Edit)))
if ($null -ne $fileNameEdit) {{
    $valuePattern = $fileNameEdit.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
    $valuePattern.SetValue('{path_str}')
    Start-Sleep -Milliseconds 200
    # Find and click Open/Save button
    $buttons = $dialog.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::Button)))
    foreach ($btn in $buttons) {{
        $name = $btn.Current.Name
        if ($name -match '(Open|Save|Select Folder|OK)') {{
            $invokePattern = $btn.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
            $invokePattern.Invoke()
            break
        }}
    }}
}}
$result.action = 'set_path'
$result.path = '{path_str}'
"#
            )
        }
        FileDialogAction::Cancel => r#"
$buttons = $dialog.FindAll([System.Windows.Automation.TreeScope]::Descendants,
    (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty,
    [System.Windows.Automation.ControlType]::Button)))
foreach ($btn in $buttons) {
    if ($btn.Current.Name -match '(Cancel|Close)') {
        $invokePattern = $btn.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
        $invokePattern.Invoke()
        break
    }
}
$result.action = 'cancel'
"#
        .to_string(),
        FileDialogAction::NavigateToFolder(path) => {
            let folder_str = path.to_string_lossy().replace('\'', "''");
            format!(
                r#"
$addressBar = $dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
    (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, "1001")))
if ($null -ne $addressBar) {{
    $valuePattern = $addressBar.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
    $valuePattern.SetValue('{folder_str}')
    Add-Type -AssemblyName System.Windows.Forms
    [System.Windows.Forms.SendKeys]::SendWait('{{ENTER}}')
    Start-Sleep -Milliseconds 500
}}
$result.action = 'navigate'
$result.path = '{folder_str}'
"#
            )
        }
        FileDialogAction::SetFilter(filter) => {
            let filter_escaped = filter.replace('\'', "''");
            format!(
                r#"
$comboBoxes = $dialog.FindAll([System.Windows.Automation.TreeScope]::Descendants,
    (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty,
    [System.Windows.Automation.ControlType]::ComboBox)))
foreach ($combo in $comboBoxes) {{
    $expandPattern = $combo.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern)
    $expandPattern.Expand()
    Start-Sleep -Milliseconds 200
    $items = $combo.FindAll([System.Windows.Automation.TreeScope]::Children,
        [System.Windows.Automation.Condition]::TrueCondition)
    foreach ($item in $items) {{
        if ($item.Current.Name -like '*{filter_escaped}*') {{
            $selectPattern = $item.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern)
            $selectPattern.Select()
            break
        }}
    }}
    break
}}
$result.action = 'set_filter'
"#
            )
        }
        FileDialogAction::GetCurrentPath => r#"
$fileNameEdit = $dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
    (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty,
    [System.Windows.Automation.ControlType]::Edit)))
if ($null -ne $fileNameEdit) {
    $valuePattern = $fileNameEdit.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
    $result.path = $valuePattern.Current.Value
}
$result.action = 'get_path'
"#
        .to_string(),
        FileDialogAction::SelectFiles(_paths) => {
            // Multi-select via keyboard: type paths separated by quotes
            r#"$result.action = 'select_files'"#.to_string()
        }
    };

    format!(
        r#"
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
{pid_clause}
$timeout = {timeout_ms}
$titlePattern = '{title_filter}'
$deadline = [Environment]::TickCount64 + $timeout
$dialog = $null

while ([Environment]::TickCount64 -lt $deadline) {{
    $root = [System.Windows.Automation.AutomationElement]::RootElement
    $windows = $root.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)
    foreach ($w in $windows) {{
        $name = $w.Current.Name
        if ($null -ne $targetPid -and $w.Current.ProcessId -ne $targetPid) {{ continue }}
        if ($name -match $titlePattern) {{
            $dialog = $w
            break
        }}
    }}
    if ($null -ne $dialog) {{ break }}
    Start-Sleep -Milliseconds 100
}}

$result = @{{ success = $false; detail = "dialog not found" }}
if ($null -ne $dialog) {{
    $result.success = $true
    $result.detail = "dialog found: " + $dialog.Current.Name
    {action_code}
}}
ConvertTo-Json $result -Compress -Depth 3
"#,
        title_filter = title_filter.replace('\'', "''"),
    )
}

/// Build a PowerShell script to detect a file dialog without interacting.
pub fn build_detect_dialog_script(target: &FileDialogTarget) -> String {
    let title_filter = target
        .title_contains
        .as_deref()
        .unwrap_or("Open|Save|Browse|Select");
    let pid_clause = target
        .process_id
        .map(|p| format!("$targetPid = {p}"))
        .unwrap_or_else(|| "$targetPid = $null".to_string());
    format!(
        r#"
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
{pid_clause}
$titlePattern = '{title_filter}'
$root = [System.Windows.Automation.AutomationElement]::RootElement
$windows = $root.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)
foreach ($w in $windows) {{
    $name = $w.Current.Name
    if ($null -ne $targetPid -and $w.Current.ProcessId -ne $targetPid) {{ continue }}
    if ($name -match $titlePattern) {{
        $result = @{{
            hwnd = 0
            process_id = $w.Current.ProcessId
            title = $name
            kind = "open"
        }}
        ConvertTo-Json $result -Compress
        exit
    }}
}}
Write-Output '{{"found":false}}'
"#,
        title_filter = title_filter.replace('\'', "''"),
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

fn parse_dialog_info(json: &str) -> Option<FileDialogInfo> {
    #[derive(serde::Deserialize)]
    struct PsDialogInfo {
        found: Option<bool>,
        hwnd: Option<u64>,
        process_id: Option<u32>,
        title: Option<String>,
        kind: Option<String>,
    }
    let info = serde_json::from_str::<PsDialogInfo>(json).ok()?;
    if info.found == Some(false) { return None; }
    Some(FileDialogInfo {
        hwnd: info.hwnd.unwrap_or(0),
        process_id: info.process_id.unwrap_or(0),
        title: info.title.unwrap_or_default(),
        kind: FileDialogKind::Open,
        current_folder: None,
        current_filename: None,
        filters: Vec::new(),
    })
}

fn parse_dialog_result(json: &str, action: &FileDialogAction) -> FileDialogResult {
    #[derive(serde::Deserialize)]
    struct PsDialogResult {
        success: Option<bool>,
        detail: Option<String>,
        path: Option<String>,
        action: Option<String>,
    }
    match serde_json::from_str::<PsDialogResult>(json) {
        Ok(r) => FileDialogResult {
            success: r.success.unwrap_or(false),
            action: r.action.unwrap_or_else(|| format!("{:?}", action)),
            detail: r.detail.unwrap_or_default(),
            selected_path: r.path.map(PathBuf::from),
            selected_paths: Vec::new(),
        },
        Err(e) => FileDialogResult {
            success: false,
            action: format!("{:?}", action),
            detail: format!("parse error: {e}"),
            selected_path: None,
            selected_paths: Vec::new(),
        },
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_target_defaults() {
        let target = FileDialogTarget::default();
        assert_eq!(target.kind, FileDialogKind::Open);
        assert_eq!(target.wait_timeout, Duration::from_secs(5));
    }

    #[test]
    fn set_path_script_includes_value_pattern() {
        let target = FileDialogTarget {
            process_id: Some(1234),
            ..Default::default()
        };
        let action = FileDialogAction::SetPath(PathBuf::from("C:\\test\\file.txt"));
        let script = build_file_dialog_script(&target, &action);
        assert!(script.contains("ValuePattern"));
        assert!(script.contains("file.txt"));
        assert!(script.contains("1234"));
    }

    #[test]
    fn cancel_script_finds_button() {
        let target = FileDialogTarget::default();
        let action = FileDialogAction::Cancel;
        let script = build_file_dialog_script(&target, &action);
        assert!(script.contains("Cancel"));
        assert!(script.contains("InvokePattern"));
    }

    #[test]
    fn navigate_script_uses_address_bar() {
        let target = FileDialogTarget::default();
        let action = FileDialogAction::NavigateToFolder(PathBuf::from("C:\\Users"));
        let script = build_file_dialog_script(&target, &action);
        assert!(script.contains("1001")); // Address bar automation ID
        assert!(script.contains("Users"));
    }

    #[test]
    fn quick_set_path_returns_result() {
        let result = FileDialogManager::quick_set_path(
            Path::new("C:\\test.txt"),
            FileDialogKind::SaveAs,
        );
        // Result depends on whether a dialog is open; just verify no panic
        let _ = result.success;
    }
}
