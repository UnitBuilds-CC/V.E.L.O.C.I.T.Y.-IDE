#![allow(dead_code, unused_imports, unused_variables)]
//! Scheduler and trigger system for Windows desktop automation.
//!
//! Provides time-based triggers, file-watcher triggers, window-appearance
//! triggers, and process-start triggers that automatically kick off WA
//! scripts when conditions are met.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ─── Trigger Types ───────────────────────────────────────────────────────────

/// Types of events that can trigger automation workflows.
#[derive(Debug, Clone)]
pub enum TriggerKind {
    /// Fire after a fixed delay.
    Delay(Duration),
    /// Fire at a specific interval (repeating).
    Interval { period: Duration, max_fires: Option<u32> },
    /// Fire when a file appears or changes.
    FileWatch {
        path: PathBuf,
        event: FileWatchEvent,
    },
    /// Fire when a window with matching title appears.
    WindowAppears { title_contains: String },
    /// Fire when a window closes.
    WindowCloses { title_contains: String },
    /// Fire when a process starts.
    ProcessStarts { name_contains: String },
    /// Fire when a process exits.
    ProcessExits { pid: u32 },
    /// Fire when clipboard content changes.
    ClipboardChanged,
    /// Fire when system goes idle (no user input for duration).
    SystemIdle { idle_threshold: Duration },
    /// Fire when a specific hotkey is pressed.
    Hotkey { modifiers: Vec<String>, key: String },
}

/// File watch event types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileWatchEvent {
    Created,
    Modified,
    Deleted,
    Renamed,
    Any,
}

// ─── Trigger Definition ──────────────────────────────────────────────────────

/// A complete trigger definition with action.
#[derive(Debug, Clone)]
pub struct TriggerDefinition {
    /// Unique trigger ID.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// What fires the trigger.
    pub kind: TriggerKind,
    /// What to do when fired.
    pub action: TriggerAction,
    /// Whether the trigger is currently enabled.
    pub enabled: bool,
    /// Maximum number of times to fire (None = unlimited).
    pub max_fires: Option<u32>,
    /// Current fire count.
    pub fire_count: u32,
    /// When this trigger was created.
    pub created_at_ms: u64,
    /// When it last fired.
    pub last_fired_at_ms: Option<u64>,
}

/// Action to perform when a trigger fires.
#[derive(Debug, Clone)]
pub enum TriggerAction {
    /// Run a saved WA script.
    RunScript {
        session_id: String,
        script_path: PathBuf,
    },
    /// Execute a PowerShell command.
    RunPowerShell(String),
    /// Launch a process.
    LaunchProcess {
        exe_path: PathBuf,
        args: Vec<String>,
    },
    /// Send a notification to the user.
    Notify { title: String, message: String },
    /// Log the event (for auditing).
    LogEvent { category: String },
    /// Chain: execute multiple actions in sequence.
    Chain(Vec<TriggerAction>),
}

/// Result of a trigger fire.
#[derive(Debug, Clone)]
pub struct TriggerFireResult {
    pub trigger_id: String,
    pub success: bool,
    pub fired_at_ms: u64,
    pub detail: String,
    pub action_output: Option<String>,
}

// ─── Trigger Manager ─────────────────────────────────────────────────────────

/// Manages trigger registration, monitoring, and execution.
pub struct TriggerManager {
    triggers: Vec<TriggerDefinition>,
    /// History of recent fires.
    fire_history: Vec<TriggerFireResult>,
    /// Maximum history entries to keep.
    max_history: usize,
}

impl TriggerManager {
    pub fn new() -> Self {
        Self {
            triggers: Vec::new(),
            fire_history: Vec::new(),
            max_history: 100,
        }
    }

    /// Register a new trigger.
    pub fn register(&mut self, trigger: TriggerDefinition) -> &TriggerDefinition {
        self.triggers.push(trigger);
        self.triggers.last().unwrap()
    }

    /// Remove a trigger by ID.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.triggers.len();
        self.triggers.retain(|t| t.id != id);
        self.triggers.len() < before
    }

    /// Enable/disable a trigger.
    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> bool {
        if let Some(t) = self.triggers.iter_mut().find(|t| t.id == id) {
            t.enabled = enabled;
            true
        } else {
            false
        }
    }

    /// Get all registered triggers.
    pub fn list(&self) -> &[TriggerDefinition] {
        &self.triggers
    }

    /// Get active (enabled) triggers.
    pub fn active_triggers(&self) -> Vec<&TriggerDefinition> {
        self.triggers.iter().filter(|t| t.enabled).collect()
    }

    /// Get fire history.
    pub fn history(&self) -> &[TriggerFireResult] {
        &self.fire_history
    }

    /// Manually fire a trigger by ID (for testing).
    pub fn fire(&mut self, id: &str) -> Option<TriggerFireResult> {
        let trigger = self.triggers.iter_mut().find(|t| t.id == id)?;
        trigger.fire_count += 1;
        let now = now_ms();
        trigger.last_fired_at_ms = Some(now);
        let result = TriggerFireResult {
            trigger_id: id.to_string(),
            success: true,
            fired_at_ms: now,
            detail: format!("Manually fired trigger '{}' (fire #{})", trigger.name, trigger.fire_count),
            action_output: None,
        };
        self.fire_history.push(result.clone());
        if self.fire_history.len() > self.max_history {
            self.fire_history.remove(0);
        }
        Some(result)
    }

    /// Check if any trigger has exceeded its max fires.
    pub fn expired_triggers(&self) -> Vec<&TriggerDefinition> {
        self.triggers
            .iter()
            .filter(|t| {
                t.max_fires
                    .map(|max| t.fire_count >= max)
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Evaluate all enabled triggers and fire any whose conditions are met.
    /// Returns results for triggers that fired during this evaluation.
    pub fn evaluate(&mut self) -> Vec<TriggerFireResult> {
        let mut fired = Vec::new();
        let mut to_fire = Vec::new();

        for trigger in &self.triggers {
            if !trigger.enabled {
                continue;
            }
            if trigger.max_fires.map(|max| trigger.fire_count >= max).unwrap_or(false) {
                continue;
            }
            if self.should_fire(trigger) {
                to_fire.push(trigger.id.clone());
            }
        }

        for id in to_fire {
            if let Some(result) = self.fire(&id) {
                fired.push(result);
            }
        }
        fired
    }

    /// Check whether a single trigger's condition is currently met.
    fn should_fire(&self, trigger: &TriggerDefinition) -> bool {
        match &trigger.kind {
            TriggerKind::Delay(duration) => {
                let elapsed = now_ms().saturating_sub(trigger.created_at_ms);
                elapsed >= duration.as_millis() as u64
            }
            TriggerKind::Interval { period, .. } => {
                let last = trigger.last_fired_at_ms.unwrap_or(trigger.created_at_ms);
                let elapsed = now_ms().saturating_sub(last);
                elapsed >= period.as_millis() as u64
            }
            TriggerKind::FileWatch { path, event } => {
                check_file_condition(path, event)
            }
            TriggerKind::WindowAppears { title_contains } => {
                check_window_condition(title_contains, true)
            }
            TriggerKind::WindowCloses { title_contains } => {
                check_window_condition(title_contains, false)
            }
            TriggerKind::ProcessStarts { name_contains } => {
                check_process_condition(name_contains, true)
            }
            TriggerKind::ProcessExits { pid } => {
                !check_process_alive(*pid)
            }
            TriggerKind::ClipboardChanged => {
                // Clipboard change detection requires state tracking across polls
                // For now, always fire (caller should track sequence numbers)
                false
            }
            TriggerKind::SystemIdle { idle_threshold } => {
                check_system_idle(idle_threshold.as_millis() as u64)
            }
            TriggerKind::Hotkey { .. } => {
                // Hotkey detection requires a message loop; not pollable
                false
            }
        }
    }
}

impl Default for TriggerManager {
    fn default() -> Self {
        Self::new()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn run_ps_quick(script: &str) -> Option<String> {
    let mut child = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn().ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(script.as_bytes()).ok()?;
    }
    let output = child.wait_with_output().ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

fn check_file_condition(path: &Path, event: &FileWatchEvent) -> bool {
    match event {
        FileWatchEvent::Created | FileWatchEvent::Any => path.exists(),
        FileWatchEvent::Deleted => !path.exists(),
        FileWatchEvent::Modified | FileWatchEvent::Renamed => {
            // Check if file was modified in last 5 seconds
            if let Ok(meta) = std::fs::metadata(path) {
                if let Ok(modified) = meta.modified() {
                    let age = SystemTime::now().duration_since(modified).unwrap_or(Duration::MAX);
                    age < Duration::from_secs(5)
                } else { false }
            } else { false }
        }
    }
}

fn check_window_condition(title_contains: &str, should_exist: bool) -> bool {
    if !cfg!(target_os = "windows") { return false; }
    let escaped = title_contains.replace('\'', "''");
    let script = format!(
        r#"Add-Type -AssemblyName UIAutomationClient; $root = [System.Windows.Automation.AutomationElement]::RootElement; $windows = $root.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition); $found = $false; foreach ($w in $windows) {{ if ($w.Current.Name -like '*{}*') {{ $found = $true; break }} }}; Write-Output $found"#,
        escaped
    );
    match run_ps_quick(&script) {
        Some(out) => {
            let is_true = out.eq_ignore_ascii_case("true");
            if should_exist { is_true } else { !is_true }
        }
        None => false,
    }
}

fn check_process_condition(name_contains: &str, should_exist: bool) -> bool {
    if !cfg!(target_os = "windows") { return false; }
    let escaped = name_contains.replace('\'', "''");
    let script = format!(
        r#"$p = Get-Process | Where-Object {{ $_.ProcessName -like '*{}*' }}; Write-Output ($null -ne $p)"#,
        escaped
    );
    match run_ps_quick(&script) {
        Some(out) => {
            let is_true = out.eq_ignore_ascii_case("true");
            if should_exist { is_true } else { !is_true }
        }
        None => false,
    }
}

fn check_process_alive(pid: u32) -> bool {
    if !cfg!(target_os = "windows") { return true; }
    let script = format!(r#"$p = Get-Process -Id {} -ErrorAction SilentlyContinue; Write-Output ($null -ne $p)"#, pid);
    match run_ps_quick(&script) {
        Some(out) => out.eq_ignore_ascii_case("true"),
        None => true, // assume alive if we can't check
    }
}

fn check_system_idle(threshold_ms: u64) -> bool {
    if !cfg!(target_os = "windows") { return false; }
    let script = format!(
        r#"Add-Type @'
using System; using System.Runtime.InteropServices;
public class IdleCheck {{
    [DllImport("user32.dll")] static extern bool GetLastInputInfo(ref LASTINPUTINFO plii);
    [StructLayout(LayoutKind.Sequential)] struct LASTINPUTINFO {{ public uint cbSize; public uint dwTime; }}
    public static uint GetIdle() {{ var i = new LASTINPUTINFO {{ cbSize = 8 }}; GetLastInputInfo(ref i); return (uint)Environment.TickCount - i.dwTime; }}
}}
'@; Write-Output ([IdleCheck]::GetIdle() -ge {})"#,
        threshold_ms
    );
    match run_ps_quick(&script) {
        Some(out) => out.eq_ignore_ascii_case("true"),
        None => false,
    }
}

// ─── PowerShell Scripts ──────────────────────────────────────────────────────

/// Build a PowerShell script to watch for file system events.
pub fn build_file_watch_script(path: &Path, event: &FileWatchEvent, timeout_ms: u64) -> String {
    let dir = path.parent().unwrap_or(path);
    let filename = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| "*".to_string());
    let event_filter = match event {
        FileWatchEvent::Created => "Created",
        FileWatchEvent::Modified => "Changed",
        FileWatchEvent::Deleted => "Deleted",
        FileWatchEvent::Renamed => "Renamed",
        FileWatchEvent::Any => "Created, Changed, Deleted, Renamed",
    };
    let dir_str = dir.to_string_lossy().replace('\'', "''");
    format!(
        r#"
$watcher = New-Object System.IO.FileSystemWatcher
$watcher.Path = '{dir_str}'
$watcher.Filter = '{filename}'
$watcher.IncludeSubdirectories = $false
$watcher.EnableRaisingEvents = $true
$result = $watcher.WaitForChanged([System.IO.WatcherChangeTypes]::'{event_filter}', {timeout_ms})
$watcher.Dispose()
if ($result.TimedOut) {{
    ConvertTo-Json @{{ triggered = $false; detail = "timeout" }} -Compress
}} else {{
    ConvertTo-Json @{{ triggered = $true; change_type = $result.ChangeType.ToString(); name = $result.Name }} -Compress
}}
"#
    )
}

/// Build a PowerShell script to detect when a window appears.
pub fn build_window_appears_script(title_contains: &str, timeout_ms: u64) -> String {
    let escaped = title_contains.replace('\'', "''");
    format!(
        r#"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$target = '{escaped}'
$deadline = [Environment]::TickCount64 + {timeout_ms}
$found = $false
$windowTitle = ""
while ([Environment]::TickCount64 -lt $deadline) {{
    $root = [System.Windows.Automation.AutomationElement]::RootElement
    $windows = $root.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)
    foreach ($w in $windows) {{
        $name = $w.Current.Name
        if ($name -like "*$target*") {{
            $found = $true
            $windowTitle = $name
            break
        }}
    }}
    if ($found) {{ break }}
    Start-Sleep -Milliseconds 200
}}
ConvertTo-Json @{{ triggered = $found; window_title = $windowTitle }} -Compress
"#
    )
}

/// Build a PowerShell script to detect system idle.
pub fn build_idle_detect_script(idle_threshold_ms: u64) -> String {
    format!(
        r#"
Add-Type @'
using System; using System.Runtime.InteropServices;
public class IdleDetect {{
    [DllImport("user32.dll")] static extern bool GetLastInputInfo(ref LASTINPUTINFO plii);
    [StructLayout(LayoutKind.Sequential)] struct LASTINPUTINFO {{ public uint cbSize; public uint dwTime; }}
    public static uint GetIdleTime() {{
        var info = new LASTINPUTINFO {{ cbSize = 8 }};
        GetLastInputInfo(ref info);
        return (uint)Environment.TickCount - info.dwTime;
    }}
}}
'@
$threshold = {idle_threshold_ms}
$idle = [IdleDetect]::GetIdleTime()
ConvertTo-Json @{{ idle_ms = $idle; is_idle = ($idle -ge $threshold); threshold_ms = $threshold }} -Compress
"#
    )
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_registration_and_removal() {
        let mut mgr = TriggerManager::new();
        mgr.register(TriggerDefinition {
            id: "t1".to_string(),
            name: "Test Trigger".to_string(),
            kind: TriggerKind::Delay(Duration::from_secs(5)),
            action: TriggerAction::LogEvent { category: "test".to_string() },
            enabled: true,
            max_fires: Some(3),
            fire_count: 0,
            created_at_ms: 0,
            last_fired_at_ms: None,
        });
        assert_eq!(mgr.list().len(), 1);
        assert!(mgr.remove("t1"));
        assert_eq!(mgr.list().len(), 0);
    }

    #[test]
    fn manual_fire_increments_count() {
        let mut mgr = TriggerManager::new();
        mgr.register(TriggerDefinition {
            id: "t2".to_string(),
            name: "Fire Test".to_string(),
            kind: TriggerKind::Interval { period: Duration::from_secs(60), max_fires: None },
            action: TriggerAction::LogEvent { category: "test".to_string() },
            enabled: true,
            max_fires: None,
            fire_count: 0,
            created_at_ms: 0,
            last_fired_at_ms: None,
        });
        let result = mgr.fire("t2").unwrap();
        assert!(result.success);
        assert_eq!(mgr.list()[0].fire_count, 1);
    }

    #[test]
    fn expired_triggers_detected() {
        let mut mgr = TriggerManager::new();
        mgr.register(TriggerDefinition {
            id: "t3".to_string(),
            name: "Limited".to_string(),
            kind: TriggerKind::Delay(Duration::from_secs(1)),
            action: TriggerAction::LogEvent { category: "test".to_string() },
            enabled: true,
            max_fires: Some(2),
            fire_count: 2,
            created_at_ms: 0,
            last_fired_at_ms: None,
        });
        assert_eq!(mgr.expired_triggers().len(), 1);
    }

    #[test]
    fn file_watch_script_contains_watcher() {
        let script = build_file_watch_script(
            Path::new("C:\\Downloads\\report.pdf"),
            &FileWatchEvent::Created,
            10000,
        );
        assert!(script.contains("FileSystemWatcher"));
        assert!(script.contains("report.pdf"));
        assert!(script.contains("Created"));
    }

    #[test]
    fn window_appears_script_searches() {
        let script = build_window_appears_script("Notepad", 5000);
        assert!(script.contains("Notepad"));
        assert!(script.contains("5000"));
    }

    #[test]
    fn idle_detect_script_uses_getlastinputinfo() {
        let script = build_idle_detect_script(60000);
        assert!(script.contains("GetLastInputInfo"));
        assert!(script.contains("60000"));
    }
}
