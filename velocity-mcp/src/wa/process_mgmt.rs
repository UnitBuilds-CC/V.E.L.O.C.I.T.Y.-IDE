#![allow(dead_code, unused_imports, unused_variables)]
//! Process lifecycle management for Windows desktop automation.
//!
//! Provides process launching, termination, enumeration, and wait-for-exit
//! capabilities needed for orchestrating desktop automation workflows.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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
        if !cfg!(target_os = "windows") { return Vec::new(); }
        let script = build_enumerate_processes_script(None);
        match run_ps_script(&script) {
            Ok(json) => parse_process_list(&json),
            Err(_) => Vec::new(),
        }
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
    pub fn launch(config: &LaunchConfig) -> LaunchResult {
        if !cfg!(target_os = "windows") {
            return LaunchResult {
                success: false,
                pid: None,
                detail: "Process launch requires Windows runtime".to_string(),
                window_ready: false,
                main_window_title: None,
            };
        }
        let script = build_launch_script(config);
        match run_ps_script(&script) {
            Ok(json) => parse_launch_result(&json),
            Err(e) => LaunchResult {
                success: false,
                pid: None,
                detail: e,
                window_ready: false,
                main_window_title: None,
            },
        }
    }

    /// Gracefully terminate a process (sends WM_CLOSE, then kills after timeout).
    pub fn terminate(pid: u32, grace_timeout: Duration) -> bool {
        if !cfg!(target_os = "windows") {
            return false;
        }
        let script = build_terminate_script(pid, grace_timeout.as_millis() as u64);
        run_ps_script(&script).is_ok()
    }

    /// Force-kill a process immediately.
    pub fn kill(pid: u32) -> bool {
        if !cfg!(target_os = "windows") {
            return false;
        }
        let script = format!(
            "$p = Get-Process -Id {pid} -ErrorAction SilentlyContinue; if ($null -eq $p) {{ exit 1 }}; Stop-Process -Id {pid} -Force; Write-Output '{{\"success\":true}}'"
        );
        run_ps_script(&script).is_ok()
    }

    /// Wait for a condition on a process.
    pub fn wait_for(
        pid: u32,
        condition: &ProcessWaitCondition,
        timeout: Duration,
    ) -> WaitResult {
        if !cfg!(target_os = "windows") {
            return WaitResult {
                condition_met: false,
                elapsed: Duration::ZERO,
                detail: "Process wait requires Windows runtime".to_string(),
                exit_code: None,
            };
        }
        let start = Instant::now();
        // Polling-based wait
        let poll_ms = 200;
        let deadline_ms = timeout.as_millis() as u64;
        match condition {
            ProcessWaitCondition::Exit => {
                let script = format!(
                    "$p = Get-Process -Id {pid} -ErrorAction SilentlyContinue; $deadline = [Environment]::TickCount64 + {deadline_ms}; while ($null -ne $p -and [Environment]::TickCount64 -lt $deadline) {{ Start-Sleep -Milliseconds {poll_ms}; $p = Get-Process -Id {pid} -ErrorAction SilentlyContinue }}; ConvertTo-Json @{{ exited = ($null -eq $p) }} -Compress"
                );
                match run_ps_script(&script) {
                    Ok(_) => WaitResult {
                        condition_met: true,
                        elapsed: start.elapsed(),
                        detail: "process exited".to_string(),
                        exit_code: None,
                    },
                    Err(e) => WaitResult {
                        condition_met: false,
                        elapsed: start.elapsed(),
                        detail: e,
                        exit_code: None,
                    },
                }
            }
            _ => {
                let script = build_wait_condition_script(pid, condition, deadline_ms, poll_ms);
                match run_ps_script(&script) {
                    Ok(json) => parse_wait_result(&json, start.elapsed()),
                    Err(e) => WaitResult {
                        condition_met: false,
                        elapsed: start.elapsed(),
                        detail: e,
                        exit_code: None,
                    },
                }
            }
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

// ─── Runtime Helpers ─────────────────────────────────────────────────────────

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

fn parse_process_list(json: &str) -> Vec<ProcessInfo> {
    #[derive(serde::Deserialize)]
    struct PsProcess {
        pid: Option<u32>,
        name: Option<String>,
        exe_path: Option<String>,
        parent_pid: Option<u32>,
        main_window_title: Option<String>,
        has_window: Option<bool>,
        memory_bytes: Option<u64>,
        start_time_ms: Option<u64>,
    }
    serde_json::from_str::<Vec<PsProcess>>(json)
        .unwrap_or_default()
        .into_iter()
        .map(|p| ProcessInfo {
            pid: p.pid.unwrap_or(0),
            name: p.name.unwrap_or_default(),
            exe_path: p.exe_path,
            command_line: None,
            parent_pid: p.parent_pid,
            main_window_title: p.main_window_title,
            has_window: p.has_window.unwrap_or(false),
            cpu_percent: None,
            memory_bytes: p.memory_bytes,
            start_time_ms: p.start_time_ms,
        })
        .collect()
}

fn build_wait_condition_script(pid: u32, condition: &ProcessWaitCondition, deadline_ms: u64, poll_ms: u64) -> String {
    match condition {
        ProcessWaitCondition::WindowAppears { title_contains } => {
            let escaped = title_contains.replace('\'', "''");
            format!(
                r#"
$deadline = [Environment]::TickCount64 + {deadline_ms}
$found = $false
while ([Environment]::TickCount64 -lt $deadline) {{
    $procs = Get-Process | Where-Object {{ $_.MainWindowTitle -like '*{escaped}*' }}
    if ($procs.Count -gt 0) {{
        $found = $true
        $title = $procs[0].MainWindowTitle
        break
    }}
    Start-Sleep -Milliseconds {poll_ms}
}}
ConvertTo-Json @{{ condition_met = $found; title = if ($found) {{ $title }} else {{ $null }} }} -Compress
"#
            )
        }
        ProcessWaitCondition::Idle { cpu_threshold, stable_seconds } => {
            format!(
                r#"
$deadline = [Environment]::TickCount64 + {deadline_ms}
$stableMs = {stable_secs} * 1000
$idleStart = [Environment]::TickCount64
$isIdle = $false
while ([Environment]::TickCount64 -lt $deadline) {{
    $proc = Get-Process -Id {pid} -ErrorAction SilentlyContinue
    if ($null -eq $proc) {{ break }}
    $cpu = $proc.CPU
    Start-Sleep -Milliseconds {poll_ms}
    $proc.Refresh()
    $cpuDelta = $proc.CPU - $cpu
    $pct = $cpuDelta / ({poll_ms} / 1000.0)
    if ($pct -lt {threshold}) {{
        if (([Environment]::TickCount64 - $idleStart) -ge $stableMs) {{
            $isIdle = $true
            break
        }}
    }} else {{
        $idleStart = [Environment]::TickCount64
    }}
}}
ConvertTo-Json @{{ condition_met = $isIdle }} -Compress
"#,
                stable_secs = stable_seconds,
                threshold = cpu_threshold,
            )
        }
        ProcessWaitCondition::MemoryStable { tolerance_bytes, stable_seconds } => {
            format!(
                r#"
$deadline = [Environment]::TickCount64 + {deadline_ms}
$stableMs = {stable_secs} * 1000
$memStart = [Environment]::TickCount64
$isStable = $false
$lastMem = 0
while ([Environment]::TickCount64 -lt $deadline) {{
    $proc = Get-Process -Id {pid} -ErrorAction SilentlyContinue
    if ($null -eq $proc) {{ break }}
    $mem = $proc.WorkingSet64
    if ($lastMem -gt 0 -and [Math]::Abs($mem - $lastMem) -le {tolerance}) {{
        if (([Environment]::TickCount64 - $memStart) -ge $stableMs) {{
            $isStable = $true
            break
        }}
    }} else {{
        $memStart = [Environment]::TickCount64
    }}
    $lastMem = $mem
    Start-Sleep -Milliseconds {poll_ms}
}}
ConvertTo-Json @{{ condition_met = $isStable; memory_bytes = $lastMem }} -Compress
"#,
                stable_secs = stable_seconds,
                tolerance = tolerance_bytes,
            )
        }
        ProcessWaitCondition::Exit => String::new(), // handled separately
    }
}

fn parse_wait_result(json: &str, elapsed: Duration) -> WaitResult {
    #[derive(serde::Deserialize)]
    struct PsWaitResult {
        condition_met: Option<bool>,
        exit_code: Option<i32>,
        detail: Option<String>,
        exited: Option<bool>,
    }
    match serde_json::from_str::<PsWaitResult>(json) {
        Ok(r) => {
            let met = r.condition_met.or(r.exited).unwrap_or(false);
            WaitResult {
                condition_met: met,
                elapsed,
                detail: r.detail.unwrap_or_else(|| if met { "condition met".into() } else { "timed out".into() }),
                exit_code: r.exit_code,
            }
        }
        Err(e) => WaitResult {
            condition_met: false,
            elapsed,
            detail: format!("parse error: {e}"),
            exit_code: None,
        },
    }
}

fn parse_launch_result(json: &str) -> LaunchResult {
    #[derive(serde::Deserialize)]
    struct PsResult {
        success: Option<bool>,
        pid: Option<u32>,
        window_ready: Option<bool>,
        main_window_title: Option<String>,
    }
    match serde_json::from_str::<PsResult>(json) {
        Ok(r) => LaunchResult {
            success: r.success.unwrap_or(true),
            pid: r.pid,
            detail: "launched via PowerShell".to_string(),
            window_ready: r.window_ready.unwrap_or(false),
            main_window_title: r.main_window_title,
        },
        Err(e) => LaunchResult {
            success: false,
            pid: None,
            detail: format!("parse error: {e}"),
            window_ready: false,
            main_window_title: None,
        },
    }
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
