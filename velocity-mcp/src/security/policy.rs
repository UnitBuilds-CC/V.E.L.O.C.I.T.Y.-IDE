//! Security policy enforcement layer.
//!
//! Defines the IDE's security boundaries and enforces them at every operation
//! boundary: file access, process spawning, request sizes, URL schemes, and
//! environment variable propagation.
//!
//! # Usage
//!
//! ```ignore
//! use crate::security::policy::{PolicyEnforcer, SecurityPolicy};
//!
//! let policy = PolicyEnforcer::default_policy();
//! let enforcer = PolicyEnforcer::new(policy);
//!
//! // Validate file operations stay within workspace
//! enforcer.validate_file_path(&path)?;
//!
//! // Validate process spawns don't launch dangerous binaries
//! enforcer.validate_process_spawn("cmd", &["/c", "dir"])?;
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

// ─── Violation kinds ──────────────────────────────────────────────────────────

/// Categories of security policy violations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViolationKind {
    /// Path traversal attempt (e.g., `../etc/passwd`).
    PathTraversal,
    /// Attempt to spawn a blocked process.
    ProcessBlocked,
    /// File exceeds maximum allowed size.
    FileTooLarge,
    /// Request exceeds maximum allowed size.
    RequestTooLarge,
    /// URL scheme not allowed.
    UrlSchemeBlocked,
    /// Sensitive environment variable would leak to child process.
    EnvVarLeak,
    /// File operation would escape workspace root.
    WorkspaceEscape,
}

impl std::fmt::Display for ViolationKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PathTraversal => write!(f, "PathTraversal"),
            Self::ProcessBlocked => write!(f, "ProcessBlocked"),
            Self::FileTooLarge => write!(f, "FileTooLarge"),
            Self::RequestTooLarge => write!(f, "RequestTooLarge"),
            Self::UrlSchemeBlocked => write!(f, "UrlSchemeBlocked"),
            Self::EnvVarLeak => write!(f, "EnvVarLeak"),
            Self::WorkspaceEscape => write!(f, "WorkspaceEscape"),
        }
    }
}

// ─── Policy violation ─────────────────────────────────────────────────────────

/// A recorded security policy violation.
#[derive(Debug, Clone)]
pub struct PolicyViolation {
    /// The kind of violation.
    pub kind: ViolationKind,
    /// Human-readable details about what was rejected.
    pub details: String,
    /// When the violation occurred (monotonic timestamp).
    pub timestamp: Instant,
}

impl std::fmt::Display for PolicyViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.kind, self.details)
    }
}

// ─── Security policy ──────────────────────────────────────────────────────────

/// Defines the IDE's security boundaries.
///
/// All operations (file I/O, process spawns, network requests) are validated
/// against this policy before execution.
#[derive(Debug, Clone)]
pub struct SecurityPolicy {
    /// Paths the IDE is allowed to access.
    pub allowed_workspace_paths: Vec<PathBuf>,
    /// Processes that should never be spawned (e.g., "rm", "format", "powershell").
    pub blocked_process_names: Vec<String>,
    /// Maximum file size for read/write operations (default 50 MB).
    pub max_file_size_bytes: u64,
    /// Maximum IPC/API request size (default 10 MB).
    pub max_request_size_bytes: usize,
    /// URL schemes allowed (default: http, https only).
    pub allowed_url_schemes: Vec<String>,
    /// Environment variables that should never be passed to child processes.
    pub blocked_env_vars: Vec<String>,
    /// If true, all file operations must stay within workspace root.
    pub require_workspace_containment: bool,
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self {
            allowed_workspace_paths: Vec::new(),
            blocked_process_names: vec![
                // Destructive system commands
                "format".into(),
                "fdisk".into(),
                "diskpart".into(),
                // Dangerous shell interpreters (when used for arbitrary code)
                "eval".into(),
                // Network tools that could exfiltrate data
                "nc".into(),
                "ncat".into(),
                "netcat".into(),
            ],
            max_file_size_bytes: 50 * 1024 * 1024, // 50 MB
            max_request_size_bytes: 10 * 1024 * 1024, // 10 MB
            allowed_url_schemes: vec!["http".into(), "https".into()],
            blocked_env_vars: vec![
                // Credentials and secrets
                "AWS_SECRET_ACCESS_KEY".into(),
                "AWS_SESSION_TOKEN".into(),
                "AZURE_CLIENT_SECRET".into(),
                "GITHUB_TOKEN".into(),
                "GITLAB_TOKEN".into(),
                "NPM_TOKEN".into(),
                "CARGO_REGISTRY_TOKEN".into(),
                "DATABASE_URL".into(),
                "DB_PASSWORD".into(),
                "PRIVATE_KEY".into(),
                "SSH_PASSPHRASE".into(),
                // API keys (generic patterns)
                "API_SECRET".into(),
                "ENCRYPTION_KEY".into(),
                "MASTER_KEY".into(),
            ],
            require_workspace_containment: true,
        }
    }
}

// ─── Policy enforcer ──────────────────────────────────────────────────────────

/// Enforces security policies at operation boundaries.
///
/// Thread-safe: can be shared across threads via `Arc<PolicyEnforcer>`.
pub struct PolicyEnforcer {
    policy: SecurityPolicy,
    violations: std::sync::Mutex<Vec<PolicyViolation>>,
    start_time: Instant,
}

impl PolicyEnforcer {
    /// Create a new enforcer with the given policy.
    pub fn new(policy: SecurityPolicy) -> Self {
        Self {
            policy,
            violations: std::sync::Mutex::new(Vec::new()),
            start_time: Instant::now(),
        }
    }

    /// Returns sensible default policy for the IDE.
    pub fn default_policy() -> SecurityPolicy {
        SecurityPolicy::default()
    }

    /// Validate that a file path is allowed by policy.
    ///
    /// Checks:
    /// - No path traversal (`..` components)
    /// - If workspace containment is required, path must be within an allowed workspace path
    pub fn validate_file_path(&self, path: &Path) -> Result<(), PolicyViolation> {
        // Check for path traversal components.
        for component in path.components() {
            if matches!(component, std::path::Component::ParentDir) {
                let violation = PolicyViolation {
                    kind: ViolationKind::PathTraversal,
                    details: format!(
                        "path contains traversal component (..): {}",
                        path.display()
                    ),
                    timestamp: Instant::now(),
                };
                self.record_violation(violation.clone());
                return Err(violation);
            }
        }

        // Check workspace containment if required.
        if self.policy.require_workspace_containment && !self.policy.allowed_workspace_paths.is_empty() {
            let is_contained = self.policy.allowed_workspace_paths.iter().any(|root| {
                path.starts_with(root)
            });

            if !is_contained {
                let violation = PolicyViolation {
                    kind: ViolationKind::WorkspaceEscape,
                    details: format!(
                        "path {} is not within any allowed workspace path",
                        path.display()
                    ),
                    timestamp: Instant::now(),
                };
                self.record_violation(violation.clone());
                return Err(violation);
            }
        }

        Ok(())
    }

    /// Validate that a process spawn is allowed by policy.
    ///
    /// Checks if the command name is in the blocked process list.
    pub fn validate_process_spawn(
        &self,
        command: &str,
        args: &[&str],
    ) -> Result<(), PolicyViolation> {
        let command_name = Path::new(command)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(command);

        // Strip extension on Windows (e.g., "cmd.exe" -> "cmd").
        let command_base = command_name
            .strip_suffix(".exe")
            .or_else(|| command_name.strip_suffix(".cmd"))
            .or_else(|| command_name.strip_suffix(".bat"))
            .unwrap_or(command_name);

        let is_blocked = self.policy.blocked_process_names.iter().any(|blocked| {
            blocked.eq_ignore_ascii_case(command_base)
        });

        if is_blocked {
            let violation = PolicyViolation {
                kind: ViolationKind::ProcessBlocked,
                details: format!(
                    "process {:?} is blocked (args: {:?})",
                    command, args
                ),
                timestamp: Instant::now(),
            };
            self.record_violation(violation.clone());
            return Err(violation);
        }

        Ok(())
    }

    /// Validate that a file size is within policy limits.
    pub fn validate_file_size(&self, size: u64) -> Result<(), PolicyViolation> {
        if size > self.policy.max_file_size_bytes {
            let violation = PolicyViolation {
                kind: ViolationKind::FileTooLarge,
                details: format!(
                    "file size {} bytes exceeds maximum {} bytes",
                    size, self.policy.max_file_size_bytes
                ),
                timestamp: Instant::now(),
            };
            self.record_violation(violation.clone());
            return Err(violation);
        }
        Ok(())
    }

    /// Validate that a request size is within policy limits.
    pub fn validate_request_size(&self, size: usize) -> Result<(), PolicyViolation> {
        if size > self.policy.max_request_size_bytes {
            let violation = PolicyViolation {
                kind: ViolationKind::RequestTooLarge,
                details: format!(
                    "request size {} bytes exceeds maximum {} bytes",
                    size, self.policy.max_request_size_bytes
                ),
                timestamp: Instant::now(),
            };
            self.record_violation(violation.clone());
            return Err(violation);
        }
        Ok(())
    }

    /// Validate that a URL uses an allowed scheme.
    pub fn validate_url(&self, url: &str) -> Result<(), PolicyViolation> {
        let scheme_end = url.find("://").ok_or_else(|| PolicyViolation {
            kind: ViolationKind::UrlSchemeBlocked,
            details: format!("URL missing scheme: {:?}", url),
            timestamp: Instant::now(),
        })?;

        let scheme = url[..scheme_end].to_lowercase();

        let is_allowed = self.policy.allowed_url_schemes.iter().any(|allowed| {
            allowed.eq_ignore_ascii_case(&scheme)
        });

        if !is_allowed {
            let violation = PolicyViolation {
                kind: ViolationKind::UrlSchemeBlocked,
                details: format!(
                    "URL scheme {:?} is not allowed (allowed: {:?})",
                    scheme, self.policy.allowed_url_schemes
                ),
                timestamp: Instant::now(),
            };
            self.record_violation(violation.clone());
            return Err(violation);
        }

        Ok(())
    }

    /// Validate that environment variables don't leak sensitive values.
    pub fn validate_env_vars(&self, vars: &[String]) -> Result<(), PolicyViolation> {
        for var in vars {
            // Extract variable name (before '=' if present).
            let var_name = var.split('=').next().unwrap_or(var);

            let is_blocked = self.policy.blocked_env_vars.iter().any(|blocked| {
                blocked.eq_ignore_ascii_case(var_name)
            });

            if is_blocked {
                let violation = PolicyViolation {
                    kind: ViolationKind::EnvVarLeak,
                    details: format!(
                        "environment variable {:?} is blocked from propagation",
                        var_name
                    ),
                    timestamp: Instant::now(),
                };
                self.record_violation(violation.clone());
                return Err(violation);
            }
        }
        Ok(())
    }

    /// Record a violation for later reporting.
    fn record_violation(&self, violation: PolicyViolation) {
        if let Ok(mut violations) = self.violations.lock() {
            violations.push(violation);
        }
    }

    /// Generate a security report summarizing all violations.
    pub fn generate_report(&self) -> SecurityReport {
        let violations = self.violations.lock().map(|v| v.clone()).unwrap_or_default();
        let total = violations.len();

        let mut violations_by_kind: HashMap<ViolationKind, usize> = HashMap::new();
        for v in &violations {
            *violations_by_kind.entry(v.kind).or_insert(0) += 1;
        }

        let last_violation = violations.last().cloned();
        let uptime_seconds = self.start_time.elapsed().as_secs();

        SecurityReport {
            total_violations: total,
            violations_by_kind,
            last_violation,
            uptime_seconds,
        }
    }

    /// Get a reference to the underlying policy.
    pub fn policy(&self) -> &SecurityPolicy {
        &self.policy
    }

    /// Get the count of recorded violations.
    pub fn violation_count(&self) -> usize {
        self.violations.lock().map(|v| v.len()).unwrap_or(0)
    }
}

// ─── Security report ──────────────────────────────────────────────────────────

/// Periodic security summary with violation statistics.
#[derive(Debug, Clone)]
pub struct SecurityReport {
    /// Total number of violations recorded.
    pub total_violations: usize,
    /// Violations grouped by kind.
    pub violations_by_kind: HashMap<ViolationKind, usize>,
    /// The most recent violation, if any.
    pub last_violation: Option<PolicyViolation>,
    /// How long the enforcer has been running (seconds).
    pub uptime_seconds: u64,
}

impl std::fmt::Display for SecurityReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Security Report (uptime: {}s)", self.uptime_seconds)?;
        writeln!(f, "  Total violations: {}", self.total_violations)?;
        for (kind, count) in &self.violations_by_kind {
            writeln!(f, "    {}: {}", kind, count)?;
        }
        if let Some(ref last) = self.last_violation {
            writeln!(f, "  Last violation: {}", last)?;
        }
        Ok(())
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_enforcer() -> PolicyEnforcer {
        let mut policy = SecurityPolicy::default();
        policy.allowed_workspace_paths = vec![
            PathBuf::from("/workspace"),
            PathBuf::from("/home/user/project"),
        ];
        PolicyEnforcer::new(policy)
    }

    // --- File path validation tests ---

    #[test]
    fn validate_file_path_allows_workspace_path() {
        let enforcer = test_enforcer();
        let path = Path::new("/workspace/src/main.rs");
        assert!(enforcer.validate_file_path(path).is_ok());
    }

    #[test]
    fn validate_file_path_rejects_traversal() {
        let enforcer = test_enforcer();
        let path = Path::new("/workspace/../etc/passwd");
        let result = enforcer.validate_file_path(path);
        assert!(result.is_err());
        let violation = result.unwrap_err();
        assert_eq!(violation.kind, ViolationKind::PathTraversal);
    }

    #[test]
    fn validate_file_path_rejects_workspace_escape() {
        let enforcer = test_enforcer();
        let path = Path::new("/etc/passwd");
        let result = enforcer.validate_file_path(path);
        assert!(result.is_err());
        let violation = result.unwrap_err();
        assert_eq!(violation.kind, ViolationKind::WorkspaceEscape);
    }

    #[test]
    fn validate_file_path_allows_when_containment_disabled() {
        let mut policy = SecurityPolicy::default();
        policy.require_workspace_containment = false;
        let enforcer = PolicyEnforcer::new(policy);
        let path = Path::new("/etc/passwd");
        // Should pass (no traversal) even though outside workspace.
        assert!(enforcer.validate_file_path(path).is_ok());
    }

    #[test]
    fn validate_file_path_allows_when_no_workspace_paths() {
        let mut policy = SecurityPolicy::default();
        policy.allowed_workspace_paths = vec![];
        policy.require_workspace_containment = true;
        let enforcer = PolicyEnforcer::new(policy);
        let path = Path::new("/any/path/at/all");
        // With no allowed paths configured, containment check is skipped.
        assert!(enforcer.validate_file_path(path).is_ok());
    }

    // --- Process spawn validation tests ---

    #[test]
    fn validate_process_spawn_allows_safe_command() {
        let enforcer = test_enforcer();
        assert!(enforcer.validate_process_spawn("cargo", &["build"]).is_ok());
    }

    #[test]
    fn validate_process_spawn_rejects_blocked_process() {
        let enforcer = test_enforcer();
        let result = enforcer.validate_process_spawn("format", &["C:"]);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, ViolationKind::ProcessBlocked);
    }

    #[test]
    fn validate_process_spawn_rejects_with_extension() {
        let enforcer = test_enforcer();
        // "format.exe" should be blocked even with extension.
        let result = enforcer.validate_process_spawn("format.exe", &[]);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, ViolationKind::ProcessBlocked);
    }

    #[test]
    fn validate_process_spawn_case_insensitive() {
        let enforcer = test_enforcer();
        let result = enforcer.validate_process_spawn("FORMAT", &[]);
        assert!(result.is_err());
    }

    // --- File size validation tests ---

    #[test]
    fn validate_file_size_allows_small_file() {
        let enforcer = test_enforcer();
        assert!(enforcer.validate_file_size(1024).is_ok());
    }

    #[test]
    fn validate_file_size_rejects_oversized() {
        let enforcer = test_enforcer();
        let size = 100 * 1024 * 1024; // 100 MB
        let result = enforcer.validate_file_size(size);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, ViolationKind::FileTooLarge);
    }

    #[test]
    fn validate_file_size_allows_at_limit() {
        let enforcer = test_enforcer();
        let size = 50 * 1024 * 1024; // exactly 50 MB
        assert!(enforcer.validate_file_size(size).is_ok());
    }

    // --- Request size validation tests ---

    #[test]
    fn validate_request_size_allows_small_request() {
        let enforcer = test_enforcer();
        assert!(enforcer.validate_request_size(1024).is_ok());
    }

    #[test]
    fn validate_request_size_rejects_oversized() {
        let enforcer = test_enforcer();
        let size = 20 * 1024 * 1024; // 20 MB
        let result = enforcer.validate_request_size(size);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, ViolationKind::RequestTooLarge);
    }

    // --- URL validation tests ---

    #[test]
    fn validate_url_allows_https() {
        let enforcer = test_enforcer();
        assert!(enforcer.validate_url("https://example.com/api").is_ok());
    }

    #[test]
    fn validate_url_allows_http() {
        let enforcer = test_enforcer();
        assert!(enforcer.validate_url("http://localhost:8080").is_ok());
    }

    #[test]
    fn validate_url_rejects_file_scheme() {
        let enforcer = test_enforcer();
        let result = enforcer.validate_url("file:///etc/passwd");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, ViolationKind::UrlSchemeBlocked);
    }

    #[test]
    fn validate_url_rejects_javascript_scheme() {
        let enforcer = test_enforcer();
        let result = enforcer.validate_url("javascript:alert(1)");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, ViolationKind::UrlSchemeBlocked);
    }

    #[test]
    fn validate_url_rejects_missing_scheme() {
        let enforcer = test_enforcer();
        let result = enforcer.validate_url("example.com/path");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, ViolationKind::UrlSchemeBlocked);
    }

    // --- Environment variable validation tests ---

    #[test]
    fn validate_env_vars_allows_safe_vars() {
        let enforcer = test_enforcer();
        let vars = vec!["PATH=/usr/bin".into(), "HOME=/home/user".into()];
        assert!(enforcer.validate_env_vars(&vars).is_ok());
    }

    #[test]
    fn validate_env_vars_rejects_aws_secret() {
        let enforcer = test_enforcer();
        let vars = vec!["AWS_SECRET_ACCESS_KEY=secret123".into()];
        let result = enforcer.validate_env_vars(&vars);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, ViolationKind::EnvVarLeak);
    }

    #[test]
    fn validate_env_vars_rejects_github_token() {
        let enforcer = test_enforcer();
        let vars = vec!["GITHUB_TOKEN".into()];
        let result = enforcer.validate_env_vars(&vars);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, ViolationKind::EnvVarLeak);
    }

    #[test]
    fn validate_env_vars_case_insensitive() {
        let enforcer = test_enforcer();
        let vars = vec!["github_token".into()];
        let result = enforcer.validate_env_vars(&vars);
        assert!(result.is_err());
    }

    // --- Security report tests ---

    #[test]
    fn security_report_tracks_violations() {
        let enforcer = test_enforcer();
        // Trigger some violations.
        let _ = enforcer.validate_file_path(Path::new("/etc/passwd"));
        let _ = enforcer.validate_url("file:///etc/shadow");
        let _ = enforcer.validate_process_spawn("format", &[]);

        let report = enforcer.generate_report();
        assert_eq!(report.total_violations, 3);
        assert_eq!(report.violations_by_kind[&ViolationKind::WorkspaceEscape], 1);
        assert_eq!(report.violations_by_kind[&ViolationKind::UrlSchemeBlocked], 1);
        assert_eq!(report.violations_by_kind[&ViolationKind::ProcessBlocked], 1);
        assert!(report.last_violation.is_some());
    }

    #[test]
    fn security_report_zero_violations() {
        let enforcer = test_enforcer();
        let report = enforcer.generate_report();
        assert_eq!(report.total_violations, 0);
        assert!(report.last_violation.is_none());
        assert!(report.violations_by_kind.is_empty());
    }

    #[test]
    fn violation_count_increments() {
        let enforcer = test_enforcer();
        assert_eq!(enforcer.violation_count(), 0);
        let _ = enforcer.validate_file_path(Path::new("/outside/path"));
        assert_eq!(enforcer.violation_count(), 1);
        let _ = enforcer.validate_url("ftp://bad.com");
        assert_eq!(enforcer.violation_count(), 2);
    }

    #[test]
    fn default_policy_has_sensible_values() {
        let policy = PolicyEnforcer::default_policy();
        assert_eq!(policy.max_file_size_bytes, 50 * 1024 * 1024);
        assert_eq!(policy.max_request_size_bytes, 10 * 1024 * 1024);
        assert!(policy.require_workspace_containment);
        assert!(policy.allowed_url_schemes.contains(&"http".to_string()));
        assert!(policy.allowed_url_schemes.contains(&"https".to_string()));
        assert!(!policy.blocked_process_names.is_empty());
        assert!(!policy.blocked_env_vars.is_empty());
    }
}
