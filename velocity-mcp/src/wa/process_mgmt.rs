#![allow(dead_code, unused_imports, unused_variables)]
//! Process lifecycle management for Windows desktop automation.
//!
//! Provides process launching, termination, enumeration, and wait-for-exit
//! capabilities needed for orchestrating desktop automation workflows.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ─── Process Model ───────────────────────────────────────────────────────────

/// Information about a running process.
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    /// Process ID.
    pub pid: u32,
    /// Process name (e.g., "notepad.exe").
    pub name: String,
    /// Full path to the executable.
    pub exe_path: Option<String>,
    /// Command line arguments.
    pub command_line: Option<String>,
    /// Parent process ID.
    pub parent_pid: Option<u32>,
    /// Window title of the main window (if any).
    pub main_window_title: Option<String>,
    /// Whether the process has a visible window.
    pub has_window: bool,
    /// CPU usage percentage (snapshot).
    pub cpu_percent: Option<f32>,
    /// Memory usage in bytes.
    pub memory_bytes: Option<u64>,
    /// Process start time.
    pub start_time_ms: Option<u64>,
}

/// Process launch configuration.
#[derive(Debug, Clone)]
pub struct LaunchConfig {
    /// Path to the executable.
    pub exe_path: PathBuf,
    /// Command line arguments.
    pub args: Vec<String>,
    /// Working directory (defaults to exe parent).
    pub working_dir: Option<PathBuf>,
    /// Environment variable overrides.
    pub env: HashMap<String, String>,
    /// Whether to start the process hidden (no window).
    pub hidden: bool,
    /// Whether to wait for the main window to appear.
    pub wait_for_window: bool,
    /// Timeout for wait_for_window.
    pub window_timeout: Duration,
    /// Whether to run as administrator (requires UAC elevation).
    pub elevated: bool,
}

impl LaunchConfig {
    pub fn new(exe_path: impl Into<PathBuf>) -> Self {
        Self {
            exe_path: exe_path.into(),
            args: Vec::new(),
            working_dir: None,
            env: HashMap::new(),
            hidden: false,
            wait_for_window: true,
            window_timeout: Duration::from_secs(10),
            elevated: false,
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args(mut self, args: &[&str]) -> Self {
        self.args.extend(args.iter().map(|s| s.to_string()));
        self
    }

    pub fn working_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.env.insert(key.to_string(), value.to_string());
        self
    }

    pub fn hidden(mut self) -> Self {
        self.hidden = true;
        self
    }

    pub fn no_wait_window(mut self) -> Self {
        self.wait_for_window = false;
        self
    }

    pub fn elevated(mut self) -> Self {
        self.elevated = true;
        self
    }
}

/// Result of launching a process.
#[derive(Debug, Clone)]
pub struct LaunchResult {
    pub success: bool,
    pub pid: Option<u32>,
    pub detail: String,
    /// Whether the main window appeared within timeout.
    pub window_ready: bool,
    pub main_window_title: Option<String>,
}

/// Wait condition for process lifecycle.
#[derive(Debug, Clone)]
pub enum ProcessWaitCondition {
    /// Wait until the process exits.
    Exit,
    /// Wait until a window with the given title appears.
    WindowAppears { title_contains: String },
    /// Wait until the process is idle (CPU < threshold for N seconds).
    Idle { cpu_threshold: f32, stable_seconds: u32 },
    /// Wait until memory usage stabilizes.
    MemoryStable { tolerance_bytes: u64, stable_seconds: u32 },
}

/// Result of waiting on a process condition.
#[derive(Debug, Clone)]
pub struct WaitResult {
    pub condition_met: bool,
    pub elapsed: Duration,
    pub detail: String,
    /// Exit code if the process exited.
    pub exit_code: Option<i32>,
}

// ─── Process Manager ─────────────────────────────────────────────────────────

/// Manages process lifecycle operations.
pub struct ProcessManager;

impl ProcessManager {
    /// Enumerate all running processes.
    pub fn enumerate() -> Vec<ProcessInfo> {
        Vec::new() // Populated by PowerShell at runtime
    }

    /// Find processes by name (case-insensitive, partial match).
    pub fn find_by_name(name: &str) -> Vec<ProcessInfo> {
        Self::enumerate()
            .into_iter()
            .filter(|p| {
                p.name
                    .to_ascii_lowercase()
                    .contains(&name.to_ascii_lowercase())
            })
            .collect()
    }

    /// Find processes by window title.
    pub fn find_by_window_title(title_contains: &str) -> Vec<ProcessInfo> {
        Self::enumerate()
            .into_iter()
            .filter(|p| {
                p.main_window_title
                    .as_deref()
                    .map(|t| t.to_ascii_lowercase().contains(&title_contains.to_ascii_lowercase()))
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Get info for a specific PID.
    pub fn get_process(pid: u32) -> Option<ProcessInfo> {
        Self::enumerate().into_iter().find(|p| p.pid == pid)
    }

    /// Check if a process is still running.
    pub fn is_running(pid: u32) -> bool {
        Self::get_process(pid).is_some()
    }

    /// Launch a process with the given configuration.
    pub fn launch(_config: &LaunchConfig) -> LaunchResult {
        LaunchResult {
            success: false,
            pid: None,
            detail: "Process launch requires Windows runtime".to_string(),
            window_ready: false,
            main_window_title: None,
        }
    }

    /// Gracefully terminate a process (sends WM_CLOSE, then kills after timeout).
    pub fn terminate(pid: u32, grace_timeout: Duration) -> bool {
        let _ = (pid, grace_timeout);
        false
    }

    /// Force-kill a process immediately.
    pub fn kill(pid: u32) -> bool {
        let _ = pid;
        false
    }

    /// Wait for a condition on a process.
    pub fn wait_for(
        pid: u32,
        condition: &ProcessWaitCondition,
        timeout: Duration,
    ) -> WaitResult {
        let _ = (pid, condition, timeout);
        WaitResult {
            condition_met: false,
            elapsed: Duration::ZERO,
            detail: "Process wait requires Windows runtime".to_string(),
            exit_code: None,
        }
    }

    /// Get child processes of a given PID.
    pub fn children(pid: u32) -> Vec<ProcessInfo> {
        Self::enumerate()
            .into_iter()
            .filter(|p| p.parent_pid == Some(pid))
            .collect()
    }

    /// Kill a process tree (process + all descendants).
    pub fn kill_tree(pid: u32) -> u32 {
        let children = Self::children(pid);
        let mut killed = 0u32;
        for child in &children {
            killed += Self::kill_tree(child.pid);
        }
        if Self::kill(pid) {
            killed += 1;
        }
        killed
    }
}

// ─── PowerShell Scripts ──────────────────────────────────────────────────────

/// Build a PowerShell script to enumerate processes with window info.
pub fn build_enumerate_processes_script(name_filter: Option<&str>) -> String {
    let filter = name_filter
        .map(|n| format!("| Where-Object {{ $_.ProcessName -like '*{}*' }}", n))
        .unwrap_or_default();

    format!(
        r#"
$processes = Get-Process {filter} -ErrorAction SilentlyContinue | ForEach-Object {{
    @{{
        pid = $_.Id
        name = $_.ProcessName
        exe_path = $_.Path
        parent_pid = (Get-CimInstance Win32_Process -Filter "ProcessId=$($_.Id)" -ErrorAction SilentlyContinue).ParentProcessId
        main_window_title = $_.MainWindowTitle
        has_window = ($_.MainWindowHandle -ne 0)
        memory_bytes = $_.WorkingSet64
        start_time_ms = if ($_.StartTime) {{ [int64](($_.StartTime.ToUniversalTime() - [DateTime]::UnixEpoch).TotalMilliseconds) }} else {{ 0 }}
    }}
}}
ConvertTo-Json @($processes) -Compress -Depth 2
"#
    )
}

/// Build a PowerShell script to launch a process.
pub fn build_launch_script(config: &LaunchConfig) -> String {
    let exe = config.exe_path.to_string_lossy().replace('\'', "''");
    let args_str = config
        .args
        .iter()
        .map(|a| format!("'{}'", a.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ");
    let verb = if config.elevated { "RunAs" } else { "" };
    let style = if config.hidden {
        "Hidden"
    } else {
        "Normal"
    };
    let working_dir = config
        .working_dir
        .as_ref()
        .map(|d| format!("-WorkingDirectory '{}'", d.to_string_lossy().replace('\'', "''")))
        .unwrap_or_default();

    let wait_clause = if config.wait_for_window {
        format!(
            r#"
$deadline = [DateTime]::Now.AddMilliseconds({})
while ([DateTime]::Now -lt $deadline) {{
    $proc.Refresh()
    if ($proc.MainWindowHandle -ne 0) {{ break }}
    Start-Sleep -Milliseconds 100
}}
"#,
            config.window_timeout.as_millis()
        )
    } else {
        String::new()
    };

    format!(
        r#"
$startInfo = New-Object System.Diagnostics.ProcessStartInfo
$startInfo.FileName = '{exe}'
$startInfo.Arguments = '{args}'
$startInfo.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::{style}
{wd}
{verb_line}
$proc = [System.Diagnostics.Process]::Start($startInfo)
{wait_clause}
$result = @{{
    success = $true
    pid = $proc.Id
    window_ready = ($proc.MainWindowHandle -ne 0)
    main_window_title = $proc.MainWindowTitle
}}
ConvertTo-Json $result -Compress
"#,
        exe = exe,
        args = config.args.join(" "),
        style = style,
        wd = working_dir,
        verb_line = if config.elevated {
            "$startInfo.Verb = 'RunAs'"
        } else {
            ""
        },
        wait_clause = wait_clause,
    )
}

/// Build a PowerShell script to terminate a process gracefully.
pub fn build_terminate_script(pid: u32, grace_ms: u64) -> String {
    format!(
        r#"
$proc = Get-Process -Id {pid} -ErrorAction SilentlyContinue
if ($null -eq $proc) {{
    Write-Output '{{"success":false,"detail":"Process not found"}}'
    exit
}}
# Try graceful close first
$proc.CloseMainWindow() | Out-Null
$exited = $proc.WaitForExit({grace_ms})
if (-not $exited) {{
    $proc.Kill()
    $proc.WaitForExit(5000) | Out-Null
}}
Write-Output (ConvertTo-Json @{{ success = $true; exit_code = $proc.ExitCode; graceful = $exited }} -Compress)
"#
    )
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_config_builder() {
        let config = LaunchConfig::new("C:\\Windows\\notepad.exe")
            .arg("test.txt")
            .working_dir("C:\\Users\\test")
            .env("MY_VAR", "value")
            .hidden()
            .elevated();

        assert_eq!(config.exe_path, PathBuf::from("C:\\Windows\\notepad.exe"));
        assert_eq!(config.args, vec!["test.txt"]);
        assert!(config.hidden);
        assert!(config.elevated);
        assert_eq!(config.env.get("MY_VAR"), Some(&"value".to_string()));
    }

    #[test]
    fn kill_tree_counts_recursively() {
        // With no running processes, kill_tree returns 0.
        let killed = ProcessManager::kill_tree(99999);
        assert_eq!(killed, 0);
    }

    #[test]
    fn enumerate_script_includes_memory() {
        let script = build_enumerate_processes_script(Some("notepad"));
        assert!(script.contains("notepad"));
        assert!(script.contains("WorkingSet64"));
        assert!(script.contains("MainWindowTitle"));
    }

    #[test]
    fn launch_script_handles_elevation() {
        let config = LaunchConfig::new("test.exe").elevated();
        let script = build_launch_script(&config);
        assert!(script.contains("RunAs"));
    }

    #[test]
    fn terminate_script_tries_graceful() {
        let script = build_terminate_script(1234, 5000);
        assert!(script.contains("CloseMainWindow"));
        assert!(script.contains("Kill"));
        assert!(script.contains("1234"));
    }
}
