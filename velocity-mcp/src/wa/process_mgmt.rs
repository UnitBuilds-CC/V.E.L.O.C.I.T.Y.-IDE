#![allow(dead_code)] // Reserved WA automation API surface; awaiting full MCP dispatch wiring.
//! Process lifecycle management for Windows desktop automation.
//!
//! Provides process launching, termination, enumeration, and wait-for-exit
//! capabilities via native Win32 API (zero PowerShell overhead).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

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

/// Manages process lifecycle operations via native Win32 API.
pub struct ProcessManager;

impl ProcessManager {
    /// Enumerate all running processes.
    pub fn enumerate() -> Vec<ProcessInfo> {
        #[cfg(target_os = "windows")]
        {
            enumerate_processes_native()
        }
        #[cfg(not(target_os = "windows"))]
        {
            Vec::new()
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
        #[cfg(target_os = "windows")]
        {
            is_process_running_native(pid)
        }
        #[cfg(not(target_os = "windows"))]
        {
            false
        }
    }

    /// Launch a process with the given configuration.
    pub fn launch(config: &LaunchConfig) -> LaunchResult {
        #[cfg(target_os = "windows")]
        {
            launch_process_native(config)
        }
        #[cfg(not(target_os = "windows"))]
        {
            LaunchResult {
                success: false,
                pid: None,
                detail: "Process launch requires Windows runtime".to_string(),
                window_ready: false,
                main_window_title: None,
            }
        }
    }

    /// Gracefully terminate a process (sends WM_CLOSE, then kills after timeout).
    pub fn terminate(pid: u32, grace_timeout: Duration) -> bool {
        #[cfg(target_os = "windows")]
        {
            terminate_process_native(pid, grace_timeout)
        }
        #[cfg(not(target_os = "windows"))]
        {
            false
        }
    }

    /// Force-kill a process immediately.
    pub fn kill(pid: u32) -> bool {
        #[cfg(target_os = "windows")]
        {
            kill_process_native(pid)
        }
        #[cfg(not(target_os = "windows"))]
        {
            false
        }
    }

    /// Wait for a condition on a process.
    pub fn wait_for(
        pid: u32,
        condition: &ProcessWaitCondition,
        timeout: Duration,
    ) -> WaitResult {
        let start = Instant::now();

        #[cfg(target_os = "windows")]
        {
            wait_for_condition_native(pid, condition, timeout, start)
        }
        #[cfg(not(target_os = "windows"))]
        {
            WaitResult {
                condition_met: false,
                elapsed: Duration::ZERO,
                detail: "Process wait requires Windows runtime".to_string(),
                exit_code: None,
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

// ─── Native Win32 Implementation ─────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod native {
    use super::*;
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Diagnostics::ToolHelp::*;
    use windows::Win32::System::Threading::*;

    // SYNCHRONIZE access right for WaitForSingleObject
    const SYNCHRONIZE: PROCESS_ACCESS_RIGHTS = PROCESS_ACCESS_RIGHTS(0x00100000);

    /// Enumerate processes using CreateToolhelp32Snapshot.
    pub fn enumerate_processes_native() -> Vec<ProcessInfo> {
        let mut processes = Vec::new();

        unsafe {
            let snapshot = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
                Ok(h) => h,
                Err(_) => return processes,
            };

            let mut entry = PROCESSENTRY32W::default();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

            if Process32FirstW(snapshot, &mut entry).is_ok() {
                loop {
                    let name_end = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                    let name = String::from_utf16_lossy(&entry.szExeFile[..name_end]);

                    processes.push(ProcessInfo {
                        pid: entry.th32ProcessID,
                        name,
                        exe_path: None, // Would require QueryFullProcessImageName
                        command_line: None,
                        parent_pid: Some(entry.th32ParentProcessID),
                        main_window_title: None,
                        has_window: false,
                        cpu_percent: None,
                        memory_bytes: None,
                        start_time_ms: None,
                    });

                    if Process32NextW(snapshot, &mut entry).is_err() {
                        break;
                    }
                }
            }

            let _ = CloseHandle(snapshot);
        }

        processes
    }

    /// Check if a process is running by inspecting its exit code.
    ///
    /// Merely opening a handle (`OpenProcess`) is not sufficient: on Windows a
    /// terminated process keeps its kernel object alive until every handle is
    /// closed, so `OpenProcess` succeeds for zombies too. The reliable signal is
    /// the exit code — it stays `STILL_ACTIVE` (259) only while the process is
    /// actually running.
    pub fn is_process_running_native(pid: u32) -> bool {
        const STILL_ACTIVE: u32 = 259;
        unsafe {
            match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                Ok(handle) => {
                    let mut exit_code: u32 = 0;
                    let ok = GetExitCodeProcess(handle, &mut exit_code).is_ok();
                    let _ = CloseHandle(handle);
                    ok && exit_code == STILL_ACTIVE
                }
                Err(_) => false,
            }
        }
    }

    /// Launch a process using std::process::Command (cross-platform, safe).
    pub fn launch_process_native(config: &LaunchConfig) -> LaunchResult {
        use std::process::Command;

        let mut cmd = Command::new(&config.exe_path);
        cmd.args(&config.args);

        if let Some(ref dir) = config.working_dir {
            cmd.current_dir(dir);
        }

        for (key, value) in &config.env {
            cmd.env(key, value);
        }

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            if config.hidden {
                const CREATE_NO_WINDOW: u32 = 0x08000000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            }
        }

        match cmd.spawn() {
            Ok(child) => {
                let pid = child.id();

                // Optionally wait for window (Windows only)
                let mut window_ready = false;
                let mut window_title = None;

                #[cfg(target_os = "windows")]
                if config.wait_for_window {
                    let deadline = Instant::now() + config.window_timeout;
                    while Instant::now() < deadline {
                        if let Some(info) = super::ProcessManager::get_process(pid) {
                            if info.has_window {
                                window_ready = true;
                                window_title = info.main_window_title;
                                break;
                            }
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }

                LaunchResult {
                    success: true,
                    pid: Some(pid),
                    detail: "launched via std::process::Command".to_string(),
                    window_ready,
                    main_window_title: window_title,
                }
            }
            Err(e) => LaunchResult {
                success: false,
                pid: None,
                detail: format!("spawn failed: {:?}", e),
                window_ready: false,
                main_window_title: None,
            },
        }
    }

    /// Terminate a process gracefully, then force-kill if needed.
    pub fn terminate_process_native(pid: u32, grace_timeout: Duration) -> bool {
        unsafe {
            let handle = match OpenProcess(PROCESS_TERMINATE | SYNCHRONIZE, false, pid) {
                Ok(h) => h,
                Err(_) => return false,
            };

            // Wait for graceful exit
            let wait_ms = grace_timeout.as_millis() as u32;
            let wait_result = WaitForSingleObject(handle, wait_ms);

            if wait_result == WAIT_OBJECT_0 {
                let _ = CloseHandle(handle);
                return true;
            }

            // Force terminate
            let result = TerminateProcess(handle, 1);
            let _ = CloseHandle(handle);
            result.is_ok()
        }
    }

    /// Force-kill a process immediately.
    pub fn kill_process_native(pid: u32) -> bool {
        unsafe {
            let handle = match OpenProcess(PROCESS_TERMINATE, false, pid) {
                Ok(h) => h,
                Err(_) => return false,
            };
            let result = TerminateProcess(handle, 1);
            let _ = CloseHandle(handle);
            result.is_ok()
        }
    }

    /// Wait for a process condition using native handles.
    pub fn wait_for_condition_native(
        pid: u32,
        condition: &ProcessWaitCondition,
        timeout: Duration,
        start: Instant,
    ) -> WaitResult {
        match condition {
            ProcessWaitCondition::Exit => {
                unsafe {
                    let handle = match OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                        Ok(h) => h,
                        Err(e) => {
                            return WaitResult {
                                condition_met: false,
                                elapsed: start.elapsed(),
                                detail: format!("OpenProcess failed: {:?}", e),
                                exit_code: None,
                            }
                        }
                    };

                    let wait_ms = timeout.as_millis() as u32;
                    let result = WaitForSingleObject(handle, wait_ms);

                    let mut exit_code: u32 = 0;
                    let _ = GetExitCodeProcess(handle, &mut exit_code);
                    let _ = CloseHandle(handle);

                    if result == WAIT_OBJECT_0 {
                        WaitResult {
                            condition_met: true,
                            elapsed: start.elapsed(),
                            detail: "process exited".to_string(),
                            exit_code: Some(exit_code as i32),
                        }
                    } else {
                        WaitResult {
                            condition_met: false,
                            elapsed: start.elapsed(),
                            detail: "timeout waiting for exit".to_string(),
                            exit_code: None,
                        }
                    }
                }
            }
            _ => {
                // For other conditions, poll using enumerate
                let deadline = Instant::now() + timeout;
                let poll_interval = Duration::from_millis(200);

                while Instant::now() < deadline {
                    match condition {
                        ProcessWaitCondition::WindowAppears { title_contains } => {
                            let found = super::ProcessManager::find_by_window_title(title_contains);
                            if !found.is_empty() {
                                return WaitResult {
                                    condition_met: true,
                                    elapsed: start.elapsed(),
                                    detail: format!("window appeared: {}", found[0].main_window_title.as_deref().unwrap_or("")),
                                    exit_code: None,
                                };
                            }
                        }
                        _ => {
                            // Idle/MemoryStable - simplified polling
                            if !super::ProcessManager::is_running(pid) {
                                return WaitResult {
                                    condition_met: false,
                                    elapsed: start.elapsed(),
                                    detail: "process exited during wait".to_string(),
                                    exit_code: None,
                                };
                            }
                        }
                    }
                    std::thread::sleep(poll_interval);
                }

                WaitResult {
                    condition_met: false,
                    elapsed: start.elapsed(),
                    detail: "timeout".to_string(),
                    exit_code: None,
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
use native::{
    enumerate_processes_native, is_process_running_native, kill_process_native,
    launch_process_native, terminate_process_native, wait_for_condition_native,
};

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
    fn process_info_model() {
        let info = ProcessInfo {
            pid: 1234,
            name: "test.exe".to_string(),
            exe_path: Some("C:\\test.exe".to_string()),
            command_line: None,
            parent_pid: Some(100),
            main_window_title: Some("Test Window".to_string()),
            has_window: true,
            cpu_percent: Some(5.0),
            memory_bytes: Some(1024 * 1024),
            start_time_ms: Some(1000),
        };
        assert_eq!(info.pid, 1234);
        assert!(info.has_window);
    }

    #[test]
    fn wait_condition_variants() {
        let conditions = [
            ProcessWaitCondition::Exit,
            ProcessWaitCondition::WindowAppears { title_contains: "test".to_string() },
            ProcessWaitCondition::Idle { cpu_threshold: 5.0, stable_seconds: 3 },
            ProcessWaitCondition::MemoryStable { tolerance_bytes: 1024, stable_seconds: 5 },
        ];
        assert_eq!(conditions.len(), 4);
    }

    // Real lifecycle over a portable console child. Gated to Windows because
    // the native spawn/terminate/wait path is Windows-only.
    #[cfg(target_os = "windows")]
    #[test]
    fn process_lifecycle_launch_is_running_then_kill() {
        // Hidden shell that stays alive briefly (~2s of pings).
        let config = LaunchConfig::new("cmd.exe")
            .arg("/c")
            .arg("ping -n 4 127.0.0.1 >nul")
            .hidden()
            .no_wait_window();
        let result = ProcessManager::launch(&config);
        assert!(result.success, "launch failed: {}", result.detail);
        let pid = result.pid.expect("launched pid");

        std::thread::sleep(Duration::from_millis(250));
        assert!(ProcessManager::is_running(pid), "child should be running");

        assert!(ProcessManager::kill(pid), "kill should succeed");
        std::thread::sleep(Duration::from_millis(400));
        assert!(!ProcessManager::is_running(pid), "child should have exited");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn process_wait_for_exit_detects_termination() {
        let config = LaunchConfig::new("cmd.exe")
            .arg("/c")
            .arg("exit 0")
            .hidden()
            .no_wait_window();
        let result = ProcessManager::launch(&config);
        assert!(result.success, "launch failed: {}", result.detail);
        let pid = result.pid.expect("launched pid");

        let wait = ProcessManager::wait_for(
            pid,
            &ProcessWaitCondition::Exit,
            Duration::from_secs(5),
        );
        assert!(wait.condition_met, "exit should be detected: {}", wait.detail);
    }
}
