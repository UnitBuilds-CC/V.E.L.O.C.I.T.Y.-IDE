//! Sandboxed process execution for NDA payloads and untrusted code.
//!
//! Combines capability-based security with OS-level resource isolation:
//!
//! - **Capability model**: `ProcessCapabilities` defines what a sandboxed process
//!   may access (file paths, network, interpreters). Violations are recorded.
//! - **Isolated temp directory**: Each execution gets a fresh temp dir that is
//!   cleaned up after completion (even on error/panic).
//! - **Panic catching**: Internal setup uses `catch_unwind`.
//! - **Output size limits**: stdout/stderr capped to prevent OOM.
//! - **Execution timeout**: Hard deadline with process kill (30s default).
//! - **Job Object limits** (Windows): Memory cap enforced via Windows Job Objects.
//! - **Audit trail**: Violations and executions logged to the audit system.

use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ─── Constants ────────────────────────────────────────────────────────────────

/// Maximum execution time for sandboxed processes.
const SANDBOX_TIMEOUT: Duration = Duration::from_secs(30);

/// Maximum captured output size (1 MB). Prevents OOM from runaway stdout.
const MAX_OUTPUT_SIZE: usize = 1_048_576;

/// Maximum stderr capture size (256 KB).
const MAX_STDERR_SIZE: usize = 262_144;

/// Maximum memory for a sandboxed process (256 MB). Windows Job Object limit.
const MAX_PROCESS_MEMORY: usize = 256 * 1024 * 1024;

// ─── Capability Model ─────────────────────────────────────────────────────────

/// Defines what a sandboxed process is allowed to do.
///
/// Each capability can be individually enabled or restricted. The default
/// `restricted()` profile is used for all NDA payload execution.
#[derive(Debug, Clone)]
pub struct ProcessCapabilities {
    /// Allowed file system paths (process can read/write within these).
    /// Empty vec = no file system access outside sandbox temp dir.
    pub allowed_paths: Vec<PathBuf>,
    /// Whether network access is permitted.
    pub allow_network: bool,
    /// Allowed interpreter programs (e.g., "python", "node").
    /// Empty vec = only explicitly approved programs.
    pub allowed_interpreters: Vec<String>,
    /// Whether BinaryPayload (.NET) execution is allowed.
    pub allow_binary_payload: bool,
    /// Maximum memory in bytes (0 = use default limit).
    pub max_memory_bytes: usize,
}

impl ProcessCapabilities {
    /// Default restricted profile for NDA execution:
    /// - No network access
    /// - No file system outside sandbox temp dir
    /// - All standard interpreters allowed
    /// - Binary payload allowed
    pub fn restricted() -> Self {
        Self {
            allowed_paths: Vec::new(),
            allow_network: false,
            allowed_interpreters: vec![
                "python".into(),
                "node".into(),
                "powershell".into(),
                "bash".into(),
                "cmd.exe".into(),
                "dotnet".into(),
            ],
            allow_binary_payload: true,
            max_memory_bytes: MAX_PROCESS_MEMORY,
        }
    }

    /// Permissive profile: everything allowed (for trusted content).
    pub fn permissive() -> Self {
        Self {
            allowed_paths: Vec::new(),
            allow_network: true,
            allowed_interpreters: Vec::new(),
            allow_binary_payload: true,
            max_memory_bytes: 0,
        }
    }

    /// Add an allowed file system path.
    pub fn with_allowed_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.allowed_paths.push(path.into());
        self
    }

    /// Set network access.
    pub fn with_network(mut self, allow: bool) -> Self {
        self.allow_network = allow;
        self
    }

    /// Check if a file path is allowed.
    pub fn is_path_allowed(&self, path: &Path) -> bool {
        if self.allowed_paths.is_empty() && self.allow_network {
            return true;
        }
        // Try canonicalized comparison first (prevents symlink bypass)
        if let Ok(canon) = std::fs::canonicalize(path) {
            for allowed in &self.allowed_paths {
                let allowed_canon =
                    std::fs::canonicalize(allowed).unwrap_or_else(|_| allowed.clone());
                if canon.starts_with(&allowed_canon) {
                    return true;
                }
            }
        }
        // Fall back to lexical check for non-existent paths
        for allowed in &self.allowed_paths {
            if path.starts_with(allowed) {
                return true;
            }
        }
        false
    }

    /// Check if an interpreter program is allowed.
    pub fn is_interpreter_allowed(&self, program: &str) -> bool {
        if self.allowed_interpreters.is_empty() {
            return true;
        }
        self.allowed_interpreters
            .iter()
            .any(|p| p.eq_ignore_ascii_case(program))
    }
}

// ─── Violation Tracking ───────────────────────────────────────────────────────

/// Category of sandbox violation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViolationCategory {
    FileSystem,
    Network,
    Interpreter,
    Memory,
    Timeout,
}

/// A single sandbox violation record.
#[derive(Debug, Clone)]
pub struct SandboxViolation {
    pub category: ViolationCategory,
    pub detail: String,
    pub timestamp_ms: u64,
}

// ─── Sandbox Result ───────────────────────────────────────────────────────────

/// Result of a sandboxed execution.
#[derive(Debug, Clone)]
pub struct SandboxResult {
    /// Captured stdout (truncated if exceeded MAX_OUTPUT_SIZE).
    pub stdout: String,
    /// Captured stderr (truncated if exceeded MAX_STDERR_SIZE).
    pub stderr: String,
    /// Process exit status (None if timed out or killed).
    pub exit_status: Option<ExitStatusInfo>,
    /// Wall-clock execution time in milliseconds.
    pub elapsed_ms: u64,
    /// Whether the process was killed due to timeout.
    pub timed_out: bool,
    /// Whether output was truncated due to size limits.
    pub output_truncated: bool,
    /// Sandbox violations recorded during execution.
    pub violations: Vec<SandboxViolation>,
}

/// Simplified exit status (not tied to std::process types for portability).
#[derive(Debug, Clone)]
pub struct ExitStatusInfo {
    pub code: Option<i32>,
    pub success: bool,
}

impl From<ExitStatus> for ExitStatusInfo {
    fn from(status: ExitStatus) -> Self {
        ExitStatusInfo {
            code: status.code(),
            success: status.success(),
        }
    }
}

// ─── Sandbox ──────────────────────────────────────────────────────────────────

/// An isolated execution environment with capability enforcement and automatic cleanup.
///
/// Creates a temp directory on construction and removes it on drop.
/// All sandboxed processes run with this directory as their working directory.
/// Capability violations are recorded and can be inspected after execution.
pub struct Sandbox {
    work_dir: PathBuf,
    capabilities: ProcessCapabilities,
    violations: Vec<SandboxViolation>,
    cleanup: bool,
}

impl Sandbox {
    /// Create a new sandbox with restricted capabilities.
    pub fn new() -> Result<Self, String> {
        Self::with_capabilities(ProcessCapabilities::restricted())
    }

    /// Create a sandbox with custom capabilities.
    pub fn with_capabilities(caps: ProcessCapabilities) -> Result<Self, String> {
        let work_dir = Self::create_isolated_dir()?;
        Ok(Self {
            work_dir,
            capabilities: caps,
            violations: Vec::new(),
            cleanup: true,
        })
    }

    fn create_isolated_dir() -> Result<PathBuf, String> {
        let base = std::env::temp_dir().join("velocity_sandbox");
        std::fs::create_dir_all(&base)
            .map_err(|e| format!("Failed to create sandbox base dir: {}", e))?;
        let dir_name = format!("exec_{}", random_suffix());
        let work_dir = base.join(dir_name);
        std::fs::create_dir_all(&work_dir)
            .map_err(|e| format!("Failed to create sandbox work dir: {}", e))?;
        Ok(work_dir)
    }

    /// Get the sandbox's working directory.
    pub fn work_dir(&self) -> &Path {
        &self.work_dir
    }

    /// Get the sandbox's capabilities.
    pub fn capabilities(&self) -> &ProcessCapabilities {
        &self.capabilities
    }

    /// Get recorded violations.
    pub fn violations(&self) -> &[SandboxViolation] {
        &self.violations
    }

    /// Whether any violations have been recorded.
    pub fn is_clean(&self) -> bool {
        self.violations.is_empty()
    }

    /// Write a file into the sandbox's working directory.
    /// Path traversal is blocked — the resolved path must stay within work_dir.
    pub fn write_file(&self, name: &str, contents: &[u8]) -> Result<PathBuf, String> {
        let name_path = Path::new(name);
        if name_path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err("Path traversal detected in sandbox file write".to_string());
        }
        let path = self.work_dir.join(name);
        let canonical_work_dir = std::fs::canonicalize(&self.work_dir)
            .map_err(|e| format!("Failed to canonicalize sandbox dir: {}", e))?;
        let canonical_path = std::fs::canonicalize(path.parent().unwrap_or(&path))
            .map_err(|e| format!("Failed to canonicalize parent dir: {}", e))?;
        if !canonical_path.starts_with(&canonical_work_dir) {
            return Err("Path traversal detected in sandbox file write".to_string());
        }
        std::fs::write(&path, contents)
            .map_err(|e| format!("Failed to write file in sandbox: {}", e))?;
        Ok(path)
    }

    /// Check if a file path is accessible from this sandbox.
    /// Records a violation if access is denied.
    pub fn check_file_access(&mut self, path: &Path) -> Result<(), String> {
        if path.starts_with(&self.work_dir) {
            return Ok(());
        }
        if self.capabilities.is_path_allowed(path) {
            return Ok(());
        }
        self.record_violation(
            ViolationCategory::FileSystem,
            &format!("File system access to '{}'", path.display()),
        );
        Err(format!(
            "Security Violation: File system access to '{}' blocked by sandbox",
            path.display()
        ))
    }

    /// Check if network access is allowed.
    /// Records a violation if access is denied.
    pub fn check_network_access(&mut self, host: &str) -> Result<(), String> {
        if self.capabilities.allow_network {
            return Ok(());
        }
        self.record_violation(
            ViolationCategory::Network,
            &format!("Network access to '{}'", host),
        );
        Err(format!(
            "Security Violation: Network access to '{}' blocked by sandbox",
            host
        ))
    }

    /// Check if an interpreter program is allowed.
    /// Records a violation if the interpreter is not in the allowlist.
    pub fn check_interpreter(&mut self, program: &str) -> Result<(), String> {
        if self.capabilities.is_interpreter_allowed(program) {
            return Ok(());
        }
        self.record_violation(
            ViolationCategory::Interpreter,
            &format!("Interpreter '{}'", program),
        );
        Err(format!(
            "Security Violation: Interpreter '{}' not allowed by sandbox capabilities",
            program
        ))
    }

    fn record_violation(&mut self, category: ViolationCategory, detail: &str) -> String {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.violations.push(SandboxViolation {
            category,
            detail: detail.to_string(),
            timestamp_ms,
        });
        // Also log to security audit
        crate::security::audit::audit_log(crate::security::audit::SecurityEvent::AuthEvent {
            kind: "failure",
            detail: format!("{}: {}", category_label(&category), detail),
        });
        format!("Security Violation: {} blocked by sandbox", detail)
    }

    /// Execute a command inside the sandbox with capability enforcement.
    pub fn execute(&mut self, program: &str, args: &[String]) -> SandboxResult {
        let start = Instant::now();

        // Check interpreter capability
        if let Err(e) = self.check_interpreter(program) {
            return SandboxResult {
                stdout: String::new(),
                stderr: e,
                exit_status: None,
                elapsed_ms: start.elapsed().as_millis() as u64,
                timed_out: false,
                output_truncated: false,
                violations: self.violations.clone(),
            };
        }

        // Use catch_unwind to protect against panics in process setup
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.spawn_and_collect(program, args)
        }));

        let elapsed_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(Ok(mut sandbox_result)) => {
                sandbox_result.elapsed_ms = elapsed_ms;
                sandbox_result.violations = self.violations.clone();
                sandbox_result
            }
            Ok(Err(err_msg)) => SandboxResult {
                stdout: String::new(),
                stderr: err_msg,
                exit_status: None,
                elapsed_ms,
                timed_out: false,
                output_truncated: false,
                violations: self.violations.clone(),
            },
            Err(panic_err) => {
                let msg = if let Some(s) = panic_err.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_err.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown panic during sandboxed execution".to_string()
                };
                SandboxResult {
                    stdout: String::new(),
                    stderr: format!("Sandbox panic: {}", msg),
                    exit_status: None,
                    elapsed_ms,
                    timed_out: false,
                    output_truncated: false,
                    violations: self.violations.clone(),
                }
            }
        }
    }

    /// Spawn a child process and collect its output with size limits.
    fn spawn_and_collect(&self, program: &str, args: &[String]) -> Result<SandboxResult, String> {
        let mut cmd = Command::new(program);
        for arg in args {
            cmd.arg(arg);
        }
        cmd.current_dir(&self.work_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Prevent sensitive env vars from leaking into the sandbox
        cmd.env_remove("VELOCITY_CSHARP_ENGINE");

        // Block network access if not allowed (via env var hint to child)
        if !self.capabilities.allow_network {
            cmd.env("VELOCITY_SANDBOX_NO_NETWORK", "1");
        }

        // Set memory limit hint
        if self.capabilities.max_memory_bytes > 0 {
            cmd.env(
                "VELOCITY_SANDBOX_MEM_LIMIT",
                self.capabilities.max_memory_bytes.to_string(),
            );
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to start process '{}': {}", program, e))?;

        // Apply Job Object limits on Windows
        #[cfg(target_os = "windows")]
        apply_job_object_limits(&mut child, self.capabilities.max_memory_bytes);

        let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;
        let stderr = child.stderr.take().ok_or("Failed to capture stderr")?;

        // Collect stdout with size limit
        let stdout_reader = std::thread::spawn(move || {
            let mut buf = Vec::with_capacity(4096);
            let mut reader = BufReader::new(stdout);
            let mut limited = false;
            let mut total = 0usize;
            let mut chunk = [0u8; 4096];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        total += n;
                        if total <= MAX_OUTPUT_SIZE {
                            buf.extend_from_slice(&chunk[..n]);
                        } else {
                            limited = true;
                        }
                    }
                    Err(_) => break,
                }
            }
            let text = String::from_utf8_lossy(&buf).to_string();
            (text, limited)
        });

        // Collect stderr with size limit
        let mut stderr_buf = Vec::with_capacity(1024);
        let mut stderr_reader = BufReader::new(stderr);
        let mut stderr_truncated = false;
        let mut stderr_total = 0usize;
        {
            let mut chunk = [0u8; 1024];
            loop {
                match stderr_reader.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        stderr_total += n;
                        if stderr_total <= MAX_STDERR_SIZE {
                            stderr_buf.extend_from_slice(&chunk[..n]);
                        } else {
                            stderr_truncated = true;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
        let stderr_text = String::from_utf8_lossy(&stderr_buf).to_string();

        // Wait with timeout
        let (status, timed_out) = match wait_with_timeout(&mut child, SANDBOX_TIMEOUT) {
            Ok(s) => (Some(ExitStatusInfo::from(s)), false),
            Err(_) => (None, true),
        };

        let (stdout_text, stdout_truncated) = stdout_reader.join().unwrap_or_default();

        Ok(SandboxResult {
            stdout: stdout_text,
            stderr: stderr_text,
            exit_status: status,
            elapsed_ms: 0,
            timed_out,
            output_truncated: stdout_truncated || stderr_truncated,
            violations: Vec::new(),
        })
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        if self.cleanup {
            if let Err(e) = std::fs::remove_dir_all(&self.work_dir) {
                tracing::warn!(dir = %self.work_dir.display(), error = %e, "Failed to clean up sandbox directory");
            }
        }
    }
}

// ─── Windows Job Object Limits ────────────────────────────────────────────────

/// Apply Windows Job Object memory limits to a child process.
#[cfg(target_os = "windows")]
fn apply_job_object_limits(child: &mut std::process::Child, max_memory: usize) {
    use std::os::windows::io::AsRawHandle;

    extern "system" {
        fn CreateJobObjectW(
            lpJobAttributes: *mut std::ffi::c_void,
            lpName: *const u16,
        ) -> *mut std::ffi::c_void;
        fn AssignProcessToJobObject(
            hJob: *mut std::ffi::c_void,
            hProcess: *mut std::ffi::c_void,
        ) -> i32;
        fn SetInformationJobObject(
            hJob: *mut std::ffi::c_void,
            info_class: u32,
            info: *const std::ffi::c_void,
            info_len: u32,
        ) -> i32;
        fn CloseHandle(hObject: *mut std::ffi::c_void) -> i32;
    }

    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: u32 = 9;
    const JOB_OBJECT_LIMIT_WORKINGSET: u32 = 0x00000001;

    // SAFETY: Windows Job Object API calls use valid handles and correctly
    // sized structures. All raw pointers are either null (for defaults) or
    // derived from properly aligned stack-allocated buffers.
    unsafe {
        let job = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
        if job.is_null() {
            tracing::warn!(
                "Failed to create Job Object for sandbox; continuing without memory limits"
            );
            return;
        }

        let limit = if max_memory > 0 {
            max_memory
        } else {
            MAX_PROCESS_MEMORY
        };
        let min_set = limit / 2;
        let max_set = limit;

        let mut info = [0u8; 128];
        let base = info.as_mut_ptr();

        std::ptr::write_unaligned(base.add(24) as *mut u32, JOB_OBJECT_LIMIT_WORKINGSET);
        std::ptr::write_unaligned(base as *mut usize, min_set);
        std::ptr::write_unaligned(base.add(8) as *mut usize, max_set);

        let set_result = SetInformationJobObject(
            job,
            JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
            info.as_ptr() as *const std::ffi::c_void,
            info.len() as u32,
        );
        if set_result == 0 {
            tracing::warn!(
                error = std::io::Error::last_os_error().raw_os_error(),
                "Failed to set Job Object memory limits (min={}, max={})",
                min_set,
                max_set
            );
        }

        let process_handle = child.as_raw_handle();
        let assign_result = AssignProcessToJobObject(job, process_handle);
        if assign_result == 0 {
            tracing::warn!("Failed to assign process to Job Object");
        }

        let _ = CloseHandle(job);
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Result<ExitStatus, String> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {
                if start.elapsed() > timeout {
                    if let Err(e) = child.kill() {
                        tracing::debug!(error = %e, "Failed to kill timed-out sandbox process");
                    }
                    if let Err(e) = child.wait() {
                        tracing::debug!(error = %e, "child.wait() failed after killing timed-out process");
                    }
                    return Err(format!(
                        "Execution timed out after {} seconds",
                        timeout.as_secs()
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                if let Err(ke) = child.kill() {
                    tracing::debug!(error = %ke, "Failed to kill sandbox process after wait error");
                }
                return Err(format!("Failed to wait for process: {}", e));
            }
        }
    }
}

fn random_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    let pid = std::process::id() as u64;
    format!(
        "{:08x}",
        (nanos ^ (pid << 16) ^ count).wrapping_mul(2654435761)
    )
}

fn category_label(cat: &ViolationCategory) -> &'static str {
    match cat {
        ViolationCategory::FileSystem => "FileSystem",
        ViolationCategory::Network => "Network",
        ViolationCategory::Interpreter => "Interpreter",
        ViolationCategory::Memory => "Memory",
        ViolationCategory::Timeout => "Timeout",
    }
}

/// Sanitize an error message to prevent leaking internal paths or system details.
pub fn sanitize_error(msg: &str) -> String {
    let truncated = if msg.len() > 500 {
        let cut = msg.floor_char_boundary(500);
        format!("{}... [truncated]", &msg[..cut])
    } else {
        msg.to_string()
    };

    let mut sanitized = truncated;

    // Remove Windows-style paths (C:\...) by scanning for drive letter pattern
    let bytes = sanitized.as_bytes();
    let mut search_from = 0;
    while search_from + 2 < bytes.len() {
        // Look for pattern: uppercase letter + ':' + '\'
        let found = sanitized[search_from..]
            .find(|c: char| c.is_ascii_uppercase())
            .map(|i| i + search_from);
        let Some(idx) = found else { break };
        if idx + 2 < sanitized.len()
            && sanitized.as_bytes()[idx + 1] == b':'
            && sanitized.as_bytes()[idx + 2] == b'\\'
        {
            // Found a Windows path, find its end.
            // Skip past the drive letter colon (idx+2) to find the real terminator.
            let end = sanitized[idx + 2..]
                .find(|c: char| {
                    c.is_whitespace() || matches!(c, ':' | ',' | ';' | '"' | '\'' | ')' | ']')
                })
                .map(|e| idx + 2 + e)
                .unwrap_or(sanitized.len());
            sanitized = format!("{}<path>{}", &sanitized[..idx], &sanitized[end..]);
            break; // One replacement per pass; sufficient for error messages
        }
        // Not a drive letter, advance past this character
        search_from = idx + 1;
    }

    // Remove Unix-style paths (/home, /tmp, /var, /usr, /etc)
    for prefix in &["/home/", "/tmp/", "/var/", "/usr/", "/etc/"] {
        if let Some(idx) = sanitized.find(prefix) {
            let end = sanitized[idx..]
                .find(|c: char| {
                    c.is_whitespace() || matches!(c, ':' | ',' | ';' | '"' | '\'' | ')' | ']')
                })
                .map(|e| idx + e)
                .unwrap_or(sanitized.len());
            sanitized = format!("{}<path>{}", &sanitized[..idx], &sanitized[end..]);
            break;
        }
    }

    sanitized
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_creates_and_cleans_up_dir() {
        let work_dir;
        {
            let sandbox = Sandbox::new().unwrap();
            work_dir = sandbox.work_dir().to_path_buf();
            assert!(work_dir.exists());
        }
        assert!(!work_dir.exists());
    }

    #[test]
    fn test_sandbox_write_file() {
        let sandbox = Sandbox::new().unwrap();
        let path = sandbox.write_file("test.txt", b"hello").unwrap();
        assert!(path.exists());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
    }

    #[test]
    #[ignore] // Integration test: spawns processes, may hang on some platforms
    fn test_sandbox_execute_echo() {
        let mut sandbox = Sandbox::new().unwrap();
        let result = sandbox.execute("echo", &["hello sandbox".to_string()]);
        if result.exit_status.is_some() {
            assert!(result.stdout.contains("hello sandbox") || !result.stderr.is_empty());
        }
        assert!(!result.timed_out);
    }

    #[test]
    #[ignore] // Integration test: spawns processes
    fn test_sandbox_execute_nonexistent_program() {
        let mut sandbox = Sandbox::new().unwrap();
        let result = sandbox.execute("nonexistent_program_xyz_12345", &[]);
        assert!(result.exit_status.is_none());
        assert!(!result.stderr.is_empty());
    }

    #[test]
    fn test_sanitize_error_removes_paths() {
        let msg = "Error reading C:\\Users\\admin\\secret\\file.txt: permission denied";
        let sanitized = sanitize_error(msg);
        assert!(!sanitized.contains("C:\\Users"));
        assert!(!sanitized.contains("admin"));
        assert!(sanitized.contains("<path>"));
    }

    #[test]
    fn test_sanitize_error_removes_unix_paths() {
        let msg = "Failed to read /home/user/secret/data.csv: no such file";
        let sanitized = sanitize_error(msg);
        assert!(!sanitized.contains("/home/user"));
        assert!(!sanitized.contains("secret"));
        assert!(sanitized.contains("<path>"));
        assert!(sanitized.contains("no such file"));
    }

    #[test]
    fn test_sanitize_error_truncates_long_messages() {
        let msg = "x".repeat(1000);
        let sanitized = sanitize_error(&msg);
        assert!(sanitized.len() < 600);
        assert!(sanitized.contains("[truncated]"));
    }

    #[test]
    fn test_sandbox_output_size_limit() {
        assert_eq!(MAX_OUTPUT_SIZE, 1_048_576);
        assert_eq!(MAX_STDERR_SIZE, 262_144);
    }

    #[test]
    fn test_restricted_capabilities_defaults() {
        let caps = ProcessCapabilities::restricted();
        assert!(!caps.allow_network);
        assert!(caps.allow_binary_payload);
        assert!(caps.allowed_paths.is_empty());
        assert!(!caps.allowed_interpreters.is_empty());
    }

    #[test]
    fn test_permissive_capabilities() {
        let caps = ProcessCapabilities::permissive();
        assert!(caps.allow_network);
        assert!(caps.allowed_interpreters.is_empty());
    }

    #[test]
    fn test_capability_path_allowlisting() {
        let caps = ProcessCapabilities::restricted().with_allowed_path("C:\\projects");
        assert!(caps.is_path_allowed(Path::new("C:\\projects\\myfile.txt")));
        assert!(!caps.is_path_allowed(Path::new("C:\\Windows\\System32")));
    }

    #[test]
    fn test_capability_interpreter_check() {
        let caps = ProcessCapabilities::restricted();
        assert!(caps.is_interpreter_allowed("python"));
        assert!(caps.is_interpreter_allowed("node"));
        assert!(caps.is_interpreter_allowed("dotnet"));
        assert!(!caps.is_interpreter_allowed("ruby"));
    }

    #[test]
    fn test_sandbox_violation_tracking() {
        let mut sandbox = Sandbox::new().unwrap();
        assert!(sandbox.is_clean());
        let result = sandbox.check_network_access("evil.com");
        assert!(result.is_err());
        assert!(!sandbox.is_clean());
        assert_eq!(sandbox.violations().len(), 1);
        assert_eq!(sandbox.violations()[0].category, ViolationCategory::Network);
    }

    #[test]
    fn test_sandbox_file_access_within_workdir() {
        let mut sandbox = Sandbox::new().unwrap();
        let path = sandbox.work_dir().join("test.txt");
        assert!(sandbox.check_file_access(&path).is_ok());
        assert!(sandbox.is_clean());
    }

    #[test]
    fn test_sandbox_file_access_outside_workdir() {
        let mut sandbox = Sandbox::new().unwrap();
        let result = sandbox.check_file_access(Path::new("C:\\Windows\\System32\\config"));
        assert!(result.is_err());
        assert!(!sandbox.is_clean());
        assert_eq!(
            sandbox.violations()[0].category,
            ViolationCategory::FileSystem
        );
    }

    #[test]
    fn test_sandbox_interpreter_violation() {
        let mut sandbox = Sandbox::new().unwrap();
        let result = sandbox.check_interpreter("ruby");
        assert!(result.is_err());
        assert_eq!(
            sandbox.violations()[0].category,
            ViolationCategory::Interpreter
        );
    }

    #[test]
    #[ignore] // Integration test: spawns processes
    fn test_sandbox_execute_records_violation() {
        let mut sandbox = Sandbox::new().unwrap();
        let result = sandbox.execute("ruby", &["-e".to_string(), "puts 'hi'".to_string()]);
        assert!(result.exit_status.is_none());
        assert!(result.stderr.contains("Security Violation"));
        assert!(!result.violations.is_empty());
    }

    #[test]
    fn test_write_file_path_traversal() {
        let sandbox = Sandbox::new().unwrap();
        let result = sandbox.write_file("../../../etc/passwd", b"hacked");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Path traversal"));
    }

    #[test]
    fn test_category_labels() {
        assert_eq!(category_label(&ViolationCategory::FileSystem), "FileSystem");
        assert_eq!(category_label(&ViolationCategory::Network), "Network");
        assert_eq!(
            category_label(&ViolationCategory::Interpreter),
            "Interpreter"
        );
        assert_eq!(category_label(&ViolationCategory::Memory), "Memory");
        assert_eq!(category_label(&ViolationCategory::Timeout), "Timeout");
    }
}
