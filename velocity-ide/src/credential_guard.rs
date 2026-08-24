//! Credential boundary protection — zeroizing secrets and env scrubbing.
//!
//! ## Threat model
//!
//! The IDE process holds multiple API keys and tokens in memory. The JIT
//! sandbox executes code in the same address space with `PAGE_EXECUTE_READWRITE`
//! memory. If the JIT compiler has a bug that allows out-of-bounds reads,
//! credentials in adjacent heap pages are exposed.
//!
//! This module provides:
//! - [`SecretString`] — a `String` wrapper that zeroizes its buffer on drop,
//!   preventing credentials from lingering in freed heap pages.
//! - [`CredentialScope`] — tracks all loaded credentials and can scrub them
//!   from memory when they're no longer needed.
//! - [`scrub_sensitive_env_vars`] — removes known sensitive environment
//!   variables after they've been loaded, reducing the attack surface for
//!   any code that enumerates the process environment.
//!
//! ## Usage
//!
//! ```ignore
//! use crate::credential_guard::{SecretString, scrub_sensitive_env_vars};
//!
//! // Load a secret — it will be zeroized when dropped.
//! let key = SecretString::from_env("VELOCITY_API_KEY")?;
//!
//! // After loading all credentials, scrub env vars to reduce exposure.
//! scrub_sensitive_env_vars();
//! ```

use std::fmt;
use serde::Serialize;

// ─── SecretString ──────────────────────────────────────────────────────────

/// A string that zeroizes its contents when dropped.
///
/// Unlike a plain `String`, `SecretString` overwrites the underlying buffer
/// with zeros before deallocation, preventing credentials from persisting
/// in freed heap pages (visible in core dumps, debugger memory views, or
/// to other code scanning the heap).
pub struct SecretString {
    inner: Vec<u8>,
}

impl SecretString {
    /// Create from a known secret value.
    pub fn new(secret: String) -> Self {
        Self {
            inner: secret.into_bytes(),
        }
    }

    /// Load from an environment variable.
    /// Returns `None` if the variable is not set.
    pub fn from_env(var: &str) -> Option<Self> {
        std::env::var(var).ok().map(Self::new)
    }

    /// Access the secret as a byte slice.
    pub fn as_bytes(&self) -> &[u8] {
        &self.inner
    }

    /// Access the secret as a string slice.
    pub fn as_str(&self) -> &str {
        // SAFETY: The inner Vec was constructed from a valid UTF-8 String.
        // We only ever write valid UTF-8 bytes (or zeros during zeroize).
        unsafe { std::str::from_utf8_unchecked(&self.inner) }
    }

    /// Zeroize the buffer without dropping.
    /// Call this when you want to explicitly clear the secret before it goes out of scope.
    pub fn zeroize(&mut self) {
        // Volatile write prevents the compiler from optimizing away the zeroing.
        for byte in self.inner.iter_mut() {
            unsafe {
                std::ptr::write_volatile(byte as *mut u8, 0);
            }
        }
    }

    /// Masked display for logging — shows first 4 and last 4 characters.
    pub fn masked(&self) -> String {
        let s = self.as_str();
        if s.len() <= 12 {
            return "****".to_string();
        }
        format!("{}...{}", &s[..4], &s[s.len() - 4..])
    }

    /// Length of the secret in bytes.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether the secret is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Check if the secret matches a given value (constant-time comparison).
    pub fn eq_secret(&self, other: &str) -> bool {
        let other_bytes = other.as_bytes();
        if self.inner.len() != other_bytes.len() {
            return false;
        }
        // Volatile comparison prevents early-exit timing attacks.
        let mut diff = 0u8;
        for (a, b) in self.inner.iter().zip(other_bytes.iter()) {
            diff |= a ^ b;
        }
        diff == 0
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        self.zeroize();
    }
}

// Never derive Debug for secret types — prevent accidental logging.
impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretString([REDACTED; {} bytes])", self.inner.len())
    }
}

impl Clone for SecretString {
    fn clone(&self) -> Self {
        // Clone creates a new independent copy that will also be zeroized on drop.
        Self {
            inner: self.inner.clone(),
        }
    }
}

// ─── Credential Scope ──────────────────────────────────────────────────────

/// Tracks all loaded credentials and can scrub them from memory.
///
/// Use this when you need to load multiple credentials for a session
/// and want to guarantee they're all cleared when done.
///
/// ```ignore
/// let mut scope = CredentialScope::new();
/// scope.load_env("VELOCITY_API_KEY");
/// scope.load_env("CF_ACCOUNT_1_TOKEN");
/// // ... use credentials ...
/// scope.scrub(); // zeroize all loaded secrets
/// ```
pub struct CredentialScope {
    secrets: Vec<SecretString>,
    env_vars_scrubbed: Vec<String>,
    /// Labels for each secret (e.g., env var name or description).
    labels: Vec<String>,
    /// Timestamp when the scope was created (seconds since UNIX epoch).
    created_at: u64,
    /// Timestamp when scrub() was last called (0 = never).
    scrubbed_at: u64,
}

impl CredentialScope {
    pub fn new() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            secrets: Vec::new(),
            env_vars_scrubbed: Vec::new(),
            labels: Vec::new(),
            created_at: now,
            scrubbed_at: 0,
        }
    }

    /// Load a secret from an environment variable and track it.
    pub fn load_env(&mut self, var: &str) -> Option<&SecretString> {
        if let Some(secret) = SecretString::from_env(var) {
            self.secrets.push(secret);
            self.labels.push(var.to_string());
            self.secrets.last()
        } else {
            None
        }
    }

    /// Load a secret from a known value and track it.
    pub fn load_value(&mut self, value: String) -> &SecretString {
        self.secrets.push(SecretString::new(value));
        self.labels.push(format!("secret_{}", self.secrets.len() - 1));
        self.secrets.last().unwrap()
    }

    /// Load a secret with a custom label.
    pub fn load_labeled(&mut self, label: &str, value: String) -> &SecretString {
        self.secrets.push(SecretString::new(value));
        self.labels.push(label.to_string());
        self.secrets.last().unwrap()
    }

    /// Get a specific secret by index.
    pub fn get(&self, index: usize) -> Option<&SecretString> {
        self.secrets.get(index)
    }

    /// Get the label for a secret by index.
    pub fn label(&self, index: usize) -> Option<&str> {
        self.labels.get(index).map(|s| s.as_str())
    }

    /// Get all loaded secrets.
    pub fn secrets(&self) -> &[SecretString] {
        &self.secrets
    }

    /// Zeroize all tracked secrets.
    pub fn scrub(&mut self) {
        for secret in &mut self.secrets {
            secret.zeroize();
        }
        self.secrets.clear();
        self.labels.clear();
        self.scrubbed_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }

    /// Record that an env var was scrubbed (for auditing).
    pub fn record_scrubbed_env(&mut self, var: String) {
        self.env_vars_scrubbed.push(var);
    }

    /// Number of secrets currently tracked.
    pub fn len(&self) -> usize {
        self.secrets.len()
    }

    /// Whether no secrets are tracked.
    pub fn is_empty(&self) -> bool {
        self.secrets.is_empty()
    }

    /// Timestamp when the scope was created (seconds since UNIX epoch).
    pub fn created_at(&self) -> u64 {
        self.created_at
    }

    /// Timestamp when scrub() was last called (0 = never).
    pub fn scrubbed_at(&self) -> u64 {
        self.scrubbed_at
    }

    /// Number of env vars that were scrubbed.
    pub fn scrubbed_env_count(&self) -> usize {
        self.env_vars_scrubbed.len()
    }

    /// Generate a diagnostic report (safe to log — no secret values).
    pub fn audit_report(&self) -> CredentialScopeReport {
        CredentialScopeReport {
            secret_count: self.secrets.len(),
            labels: self.labels.clone(),
            env_vars_scrubbed: self.env_vars_scrubbed.clone(),
            created_at: self.created_at,
            scrubbed_at: self.scrubbed_at,
            is_scrubbed: self.secrets.is_empty() && self.scrubbed_at > 0,
        }
    }
}

impl Drop for CredentialScope {
    fn drop(&mut self) {
        // Ensure all secrets are zeroized even if scrub() wasn't called explicitly.
        self.scrub();
    }
}

impl Default for CredentialScope {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for CredentialScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CredentialScope")
            .field("secrets_count", &self.secrets.len())
            .field("env_vars_scrubbed", &self.env_vars_scrubbed.len())
            .field("created_at", &self.created_at)
            .field("scrubbed_at", &self.scrubbed_at)
            .finish()
    }
}

/// Diagnostic report for a credential scope (safe to log — no secret values).
#[derive(Debug, Clone, Serialize)]
pub struct CredentialScopeReport {
    /// Number of secrets currently tracked.
    pub secret_count: usize,
    /// Labels for each secret (env var names or descriptions).
    pub labels: Vec<String>,
    /// Environment variables that were scrubbed.
    pub env_vars_scrubbed: Vec<String>,
    /// Timestamp when the scope was created (seconds since UNIX epoch).
    pub created_at: u64,
    /// Timestamp when scrub() was last called (0 = never).
    pub scrubbed_at: u64,
    /// Whether all secrets have been scrubbed.
    pub is_scrubbed: bool,
}

// ─── Environment Scrubbing ─────────────────────────────────────────────────

/// Environment variable names known to contain sensitive credentials.
/// These are removed from the process environment after loading.
const SENSITIVE_ENV_VARS: &[&str] = &[
    // Velocity router
    "VELOCITY_API_KEY",
    // Cloudflare Workers AI accounts
    // (dynamically generated CF_ACCOUNT_{N}_TOKEN — handled separately)
    // Common provider keys that may be in the environment
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "GOOGLE_AI_API_KEY",
    "MISTRAL_API_KEY",
    "COHERE_API_KEY",
    "XAI_API_KEY",
    "GITHUB_TOKEN",
    "GITHUB_API_KEY",
    // AWS credentials
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "AWS_SECURITY_TOKEN",
    // GCP credentials
    "GCP_SERVICE_ACCOUNT_KEY",
    "GOOGLE_APPLICATION_CREDENTIALS",
    // Azure credentials
    "AZURE_CLIENT_SECRET",
    "AZURE_CLIENT_ID",
    "AZURE_TENANT_ID",
    // Docker / container registries
    "DOCKER_PASSWORD",
    "DOCKER_TOKEN",
    "CR_PAT",
    // SSH agent socket — allows signing arbitrary data with loaded keys
    "SSH_AUTH_SOCK",
    // GPG agent
    "GPG_AGENT_INFO",
    // Generic secrets that may appear in CI/dev environments
    "SECRET_KEY",
    "PRIVATE_KEY",
    "JWT_SECRET",
    "SESSION_SECRET",
    "ENCRYPTION_KEY",
];

/// Socket/pipe environment variables that cross sandbox boundaries.
/// These allow JIT code to communicate with credential-holding agents.
const SOCKET_ENV_VARS: &[&str] = &[
    "SSH_AUTH_SOCK",
    "GPG_AGENT_INFO",
    "DBUS_SESSION_BUS_ADDRESS",
];

/// Remove known sensitive environment variables from the process environment.
///
/// Call this AFTER loading credentials into a [`CredentialScope`] or
/// [`SecretString`]. This prevents any code in the process (including
/// JIT-compiled closures) from accessing these secrets via `std::env::var`.
///
/// Also removes socket environment variables (SSH_AUTH_SOCK, GPG agent)
/// that allow communication with credential-holding external agents.
///
/// Returns the list of variables that were actually removed.
pub fn scrub_sensitive_env_vars() -> Vec<String> {
    let mut removed = Vec::new();

    // Remove well-known sensitive vars.
    for var in SENSITIVE_ENV_VARS {
        if std::env::var(var).is_ok() {
            std::env::remove_var(var);
            removed.push(var.to_string());
        }
    }

    // Remove socket/pipe env vars (SSH agent, GPG agent, D-Bus).
    for var in SOCKET_ENV_VARS {
        if std::env::var(var).is_ok() {
            std::env::remove_var(var);
            if !removed.contains(&var.to_string()) {
                removed.push(var.to_string());
            }
        }
    }

    // Remove Cloudflare account tokens (dynamically numbered 1..=30).
    for i in 1..=30 {
        let token_key = format!("CF_ACCOUNT_{}_TOKEN", i);
        if std::env::var(&token_key).is_ok() {
            std::env::remove_var(&token_key);
            removed.push(token_key);
        }
        // Also scrub the account IDs — they're not secret per se, but
        // they reduce the information available to untrusted code.
        let id_key = format!("CF_ACCOUNT_{}_ID", i);
        if std::env::var(&id_key).is_ok() {
            std::env::remove_var(&id_key);
            removed.push(id_key);
        }
    }

    removed
}

/// Check if any sensitive environment variables are still present.
/// Returns a list of variable names that should have been scrubbed.
pub fn audit_env_exposure() -> Vec<String> {
    let mut exposed = Vec::new();

    for var in SENSITIVE_ENV_VARS {
        if std::env::var(var).is_ok() {
            exposed.push(var.to_string());
        }
    }

    for var in SOCKET_ENV_VARS {
        if std::env::var(var).is_ok() {
            if !exposed.contains(&var.to_string()) {
                exposed.push(var.to_string());
            }
        }
    }

    for i in 1..=30 {
        let token_key = format!("CF_ACCOUNT_{}_TOKEN", i);
        if std::env::var(&token_key).is_ok() {
            exposed.push(token_key);
        }
    }

    exposed
}

// ─── Credential Boundary Audit ─────────────────────────────────────────────

/// Result of auditing the credential boundary before sandbox execution.
///
/// This captures what credential-bearing resources are still accessible
/// to the process. The sandbox isolates computation but NOT the environment
/// the process inherits — env vars, agent sockets, and mounted config
/// directories all cross the boundary by default.
#[derive(Clone, Debug, Serialize)]
pub struct CredentialBoundaryAudit {
    /// Sensitive env vars still present (should have been scrubbed).
    pub exposed_env_vars: Vec<String>,
    /// Socket paths reachable from the process (SSH agent, GPG agent).
    pub reachable_sockets: Vec<(String, String)>,
    /// Config directories that exist and are readable (may contain keys).
    pub accessible_config_dirs: Vec<String>,
    /// Whether the audit passed (no credential leaks detected).
    pub clean: bool,
}

impl CredentialBoundaryAudit {
    /// Perform a full credential boundary audit.
    ///
    /// Checks:
    /// 1. Sensitive env vars that should have been scrubbed
    /// 2. Agent sockets (SSH_AUTH_SOCK, GPG_AGENT_INFO) still in env
    /// 3. Config directories (~/.ssh, ~/.aws, ~/.config/gcloud) that are readable
    ///
    /// Call this BEFORE executing JIT-compiled code in the sandbox.
    pub fn run() -> Self {
        let exposed_env_vars = audit_env_exposure();

        // Check for reachable agent sockets.
        let mut reachable_sockets = Vec::new();
        for var in SOCKET_ENV_VARS {
            if let Ok(path) = std::env::var(var) {
                if !path.is_empty() {
                    // Check if the socket file actually exists and is accessible.
                    if std::path::Path::new(&path).exists() {
                        reachable_sockets.push((var.to_string(), path));
                    }
                }
            }
        }

        // Check for accessible credential config directories.
        let mut accessible_config_dirs = Vec::new();
        let config_dirs = sensitive_config_paths();
        for dir in config_dirs {
            if dir.exists() && dir.is_dir() {
                accessible_config_dirs.push(dir.display().to_string());
            }
        }

        let clean = exposed_env_vars.is_empty()
            && reachable_sockets.is_empty()
            && accessible_config_dirs.is_empty();

        Self {
            exposed_env_vars,
            reachable_sockets,
            accessible_config_dirs,
            clean,
        }
    }

    /// Format a human-readable warning if the boundary is not clean.
    pub fn warning_message(&self) -> Option<String> {
        if self.clean {
            return None;
        }

        let mut lines = Vec::new();
        lines.push("Credential boundary audit detected potential leaks:".to_string());

        if !self.exposed_env_vars.is_empty() {
            lines.push(format!(
                "  - {} sensitive env var(s) still present: {:?}",
                self.exposed_env_vars.len(),
                self.exposed_env_vars
            ));
        }

        if !self.reachable_sockets.is_empty() {
            lines.push(format!(
                "  - {} agent socket(s) reachable: {:?}",
                self.reachable_sockets.len(),
                self.reachable_sockets
            ));
        }

        if !self.accessible_config_dirs.is_empty() {
            lines.push(format!(
                "  - {} config dir(s) accessible: {:?}",
                self.accessible_config_dirs.len(),
                self.accessible_config_dirs
            ));
        }

        lines.push("  JIT code in the sandbox may be able to exfiltrate credentials.".to_string());
        Some(lines.join("\n"))
    }

    /// Return a severity level for the audit result.
    pub fn severity(&self) -> &'static str {
        if self.clean {
            "none"
        } else if !self.exposed_env_vars.is_empty() && !self.reachable_sockets.is_empty() {
            "critical" // env vars + sockets = active exfiltration path
        } else if !self.exposed_env_vars.is_empty() {
            "high" // env vars present but no sockets
        } else if !self.reachable_sockets.is_empty() {
            "high" // sockets reachable
        } else {
            "medium" // only config dirs accessible
        }
    }

    /// Return a compact summary of the audit.
    pub fn summary(&self) -> CredentialAuditSummary {
        CredentialAuditSummary {
            clean: self.clean,
            severity: self.severity().to_string(),
            exposed_env_count: self.exposed_env_vars.len(),
            reachable_socket_count: self.reachable_sockets.len(),
            accessible_config_dir_count: self.accessible_config_dirs.len(),
            total_issues: self.exposed_env_vars.len()
                + self.reachable_sockets.len()
                + self.accessible_config_dirs.len(),
        }
    }
}

/// Compact summary of a credential boundary audit.
#[derive(Debug, Clone, Serialize)]
pub struct CredentialAuditSummary {
    pub clean: bool,
    pub severity: String,
    pub exposed_env_count: usize,
    pub reachable_socket_count: usize,
    pub accessible_config_dir_count: usize,
    pub total_issues: usize,
}

/// Return paths to sensitive config directories that may contain credentials.
/// These directories, if readable, give sandbox code access to keys/certs.
fn sensitive_config_paths() -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();

    if let Some(home) = home_dir() {
        // SSH keys and config
        paths.push(home.join(".ssh"));
        // AWS credentials (~/.aws/credentials)
        paths.push(home.join(".aws"));
        // GCP service account keys
        paths.push(home.join(".config").join("gcloud"));
        // Azure credentials
        paths.push(home.join(".azure"));
        // Docker config (registry auth)
        paths.push(home.join(".docker"));
        // Kubernetes config (cluster credentials)
        paths.push(home.join(".kube"));
        // Terraform credentials
        paths.push(home.join(".terraform.d"));
        // npm/pip auth tokens
        paths.push(home.join(".npmrc"));
        paths.push(home.join(".pypirc"));
        // Git credentials
        paths.push(home.join(".git-credentials"));
    }

    paths
}

/// Get the user's home directory.
fn home_dir() -> Option<std::path::PathBuf> {
    // Check HOME env var first (works on Unix and some Windows setups).
    if let Ok(home) = std::env::var("HOME") {
        return Some(std::path::PathBuf::from(home));
    }
    // Windows fallback.
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        return Some(std::path::PathBuf::from(userprofile));
    }
    None
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env vars are process-global state. Tests that modify them must be
    // serialized to avoid races when running in parallel.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn secret_string_zeroizes_on_drop() {
        let mut s = SecretString::new("super_secret_key_12345".to_string());
        assert_eq!(s.as_str(), "super_secret_key_12345");

        // Zeroize manually to check.
        s.zeroize();
        // After zeroize, all bytes should be 0.
        assert!(s.as_bytes().iter().all(|&b| b == 0));
    }

    #[test]
    fn secret_string_debug_is_redacted() {
        let s = SecretString::new("my_api_key".to_string());
        let debug = format!("{:?}", s);
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains("my_api_key"));
    }

    #[test]
    fn secret_string_masked_short_key() {
        let s = SecretString::new("short".to_string());
        assert_eq!(s.masked(), "****");
    }

    #[test]
    fn secret_string_masked_long_key() {
        let s = SecretString::new("vr_standard_abc123xyz".to_string());
        let masked = s.masked();
        // "vr_standard_abc123xyz" is 21 chars: first 4 = "vr_s", last 4 = "3xyz"
        assert!(masked.starts_with("vr_s"), "expected start 'vr_s', got: {}", masked);
        assert!(masked.ends_with("3xyz"), "expected end '3xyz', got: {}", masked);
        assert!(masked.contains("..."));
    }

    #[test]
    fn secret_string_from_env() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("TEST_SECRET_KEY_XYZ", "test_value_123");
        let s = SecretString::from_env("TEST_SECRET_KEY_XYZ");
        assert!(s.is_some());
        assert_eq!(s.unwrap().as_str(), "test_value_123");
        std::env::remove_var("TEST_SECRET_KEY_XYZ");

        // Missing var returns None.
        assert!(SecretString::from_env("NONEXISTENT_VAR_XYZ").is_none());
    }

    #[test]
    fn credential_scope_scrub_clears_all() {
        let mut scope = CredentialScope::new();
        scope.load_value("secret1".to_string());
        scope.load_value("secret2".to_string());
        assert_eq!(scope.len(), 2);

        scope.scrub();
        assert!(scope.is_empty());
    }

    #[test]
    fn credential_scope_zeroizes_on_drop() {
        let mut scope = CredentialScope::new();
        scope.load_value("will_be_zeroized".to_string());
        // Drop will call scrub() automatically.
        drop(scope);
        // If we got here without panic, the drop worked correctly.
    }

    #[test]
    fn scrub_env_vars_removes_known_secrets() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("VELOCITY_API_KEY", "vr_test_scrub");
        std::env::set_var("OPENAI_API_KEY", "sk-test-scrub");
        assert!(std::env::var("VELOCITY_API_KEY").is_ok());

        let removed = scrub_sensitive_env_vars();
        assert!(removed.contains(&"VELOCITY_API_KEY".to_string()));
        assert!(removed.contains(&"OPENAI_API_KEY".to_string()));

        // Vars should be gone now.
        assert!(std::env::var("VELOCITY_API_KEY").is_err());
        assert!(std::env::var("OPENAI_API_KEY").is_err());
    }

    #[test]
    fn scrub_env_vars_handles_cf_accounts() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("CF_ACCOUNT_1_ID", "cf_id_1");
        std::env::set_var("CF_ACCOUNT_1_TOKEN", "cf_token_1");
        std::env::set_var("CF_ACCOUNT_2_ID", "cf_id_2");
        std::env::set_var("CF_ACCOUNT_2_TOKEN", "cf_token_2");

        let removed = scrub_sensitive_env_vars();
        assert!(removed.contains(&"CF_ACCOUNT_1_TOKEN".to_string()));
        assert!(removed.contains(&"CF_ACCOUNT_1_ID".to_string()));
        assert!(removed.contains(&"CF_ACCOUNT_2_TOKEN".to_string()));

        assert!(std::env::var("CF_ACCOUNT_1_TOKEN").is_err());
        assert!(std::env::var("CF_ACCOUNT_2_TOKEN").is_err());
    }

    #[test]
    fn audit_env_exposure_detects_unscrubbed() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("VELOCITY_API_KEY", "vr_audit_test");
        let exposed = audit_env_exposure();
        assert!(exposed.contains(&"VELOCITY_API_KEY".to_string()));
        std::env::remove_var("VELOCITY_API_KEY");
    }

    #[test]
    fn audit_env_exposure_clean_after_scrub() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("VELOCITY_API_KEY", "vr_will_be_scrubbed");
        scrub_sensitive_env_vars();
        let exposed = audit_env_exposure();
        assert!(!exposed.contains(&"VELOCITY_API_KEY".to_string()));
    }

    #[test]
    fn scrub_removes_ssh_auth_sock() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("SSH_AUTH_SOCK", "/tmp/ssh-agent-sock");
        let removed = scrub_sensitive_env_vars();
        assert!(removed.contains(&"SSH_AUTH_SOCK".to_string()));
        assert!(std::env::var("SSH_AUTH_SOCK").is_err());
    }

    #[test]
    fn scrub_removes_aws_credentials() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("AWS_ACCESS_KEY_ID", "AKIAIOSF000000000000");
        std::env::set_var("AWS_SECRET_ACCESS_KEY", "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY");
        let removed = scrub_sensitive_env_vars();
        assert!(removed.contains(&"AWS_ACCESS_KEY_ID".to_string()));
        assert!(removed.contains(&"AWS_SECRET_ACCESS_KEY".to_string()));
        assert!(std::env::var("AWS_ACCESS_KEY_ID").is_err());
        assert!(std::env::var("AWS_SECRET_ACCESS_KEY").is_err());
    }

    #[test]
    fn boundary_audit_detects_exposed_env() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("VELOCITY_API_KEY", "vr_leaked");
        let audit = CredentialBoundaryAudit::run();
        assert!(!audit.clean);
        assert!(audit.exposed_env_vars.contains(&"VELOCITY_API_KEY".to_string()));
        let warning = audit.warning_message();
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("Credential boundary audit detected"));
        std::env::remove_var("VELOCITY_API_KEY");
    }

    #[test]
    fn boundary_audit_clean_after_scrub() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("VELOCITY_API_KEY", "vr_will_scrub");
        scrub_sensitive_env_vars();
        let audit = CredentialBoundaryAudit::run();
        // After scrubbing, the env vars should be gone.
        assert!(!audit.exposed_env_vars.contains(&"VELOCITY_API_KEY".to_string()));
    }

    #[test]
    fn sensitive_config_paths_returns_home_relative() {
        let paths = sensitive_config_paths();
        // Should include .ssh, .aws, .docker at minimum (if home exists).
        if home_dir().is_some() {
            assert!(paths.iter().any(|p| p.ends_with(".ssh")));
            assert!(paths.iter().any(|p| p.ends_with(".aws")));
            assert!(paths.iter().any(|p| p.ends_with(".docker")));
        }
    }

    #[test]
    fn credential_scope_labels() {
        let mut scope = CredentialScope::new();
        scope.load_value("secret_a".to_string());
        scope.load_labeled("api_key", "secret_b".to_string());
        assert_eq!(scope.label(0), Some("secret_0"));
        assert_eq!(scope.label(1), Some("api_key"));
        assert_eq!(scope.label(99), None);
    }

    #[test]
    fn credential_scope_get_by_index() {
        let mut scope = CredentialScope::new();
        scope.load_value("my_secret".to_string());
        assert!(scope.get(0).is_some());
        assert_eq!(scope.get(0).unwrap().as_str(), "my_secret");
        assert!(scope.get(1).is_none());
    }

    #[test]
    fn credential_scope_timestamps() {
        let scope = CredentialScope::new();
        assert!(scope.created_at() > 0);
        assert_eq!(scope.scrubbed_at(), 0);
    }

    #[test]
    fn credential_scope_scrub_updates_timestamp() {
        let mut scope = CredentialScope::new();
        scope.load_value("temp".to_string());
        assert_eq!(scope.scrubbed_at(), 0);
        scope.scrub();
        assert!(scope.scrubbed_at() > 0);
    }

    #[test]
    fn credential_scope_audit_report() {
        let mut scope = CredentialScope::new();
        scope.load_labeled("test_key", "value".to_string());
        scope.record_scrubbed_env("TEST_ENV".to_string());
        let report = scope.audit_report();
        assert_eq!(report.secret_count, 1);
        assert_eq!(report.labels, vec!["test_key"]);
        assert_eq!(report.env_vars_scrubbed, vec!["TEST_ENV"]);
        assert!(!report.is_scrubbed);
        assert!(report.created_at > 0);
    }

    #[test]
    fn credential_scope_report_after_scrub() {
        let mut scope = CredentialScope::new();
        scope.load_value("temp".to_string());
        scope.scrub();
        let report = scope.audit_report();
        assert_eq!(report.secret_count, 0);
        assert!(report.is_scrubbed);
    }

    #[test]
    fn credential_scope_report_serialize() {
        let report = CredentialScopeReport {
            secret_count: 2,
            labels: vec!["api_key".to_string(), "db_pass".to_string()],
            env_vars_scrubbed: vec!["VELOCITY_API_KEY".to_string()],
            created_at: 1700000000,
            scrubbed_at: 0,
            is_scrubbed: false,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"secret_count\":2"));
        assert!(json.contains("\"is_scrubbed\":false"));
        // Ensure no actual secret values in the report
        assert!(!json.contains("password"));
        assert!(!json.contains("key_value"));
    }

    #[test]
    fn boundary_audit_serialize() {
        let audit = CredentialBoundaryAudit {
            exposed_env_vars: vec!["VELOCITY_API_KEY".to_string()],
            reachable_sockets: vec![],
            accessible_config_dirs: vec![],
            clean: false,
        };
        let json = serde_json::to_string(&audit).unwrap();
        assert!(json.contains("\"clean\":false"));
        assert!(json.contains("VELOCITY_API_KEY"));
    }

    #[test]
    fn scrubbed_env_count() {
        let mut scope = CredentialScope::new();
        assert_eq!(scope.scrubbed_env_count(), 0);
        scope.record_scrubbed_env("VAR1".to_string());
        scope.record_scrubbed_env("VAR2".to_string());
        assert_eq!(scope.scrubbed_env_count(), 2);
    }

    // ─── New Tests ─────────────────────────────────────────────────────────

    #[test]
    fn secret_string_len_and_is_empty() {
        let s = SecretString::new("hello".to_string());
        assert_eq!(s.len(), 5);
        assert!(!s.is_empty());

        let empty = SecretString::new(String::new());
        assert_eq!(empty.len(), 0);
        assert!(empty.is_empty());
    }

    #[test]
    fn secret_string_eq_secret_matches() {
        let s = SecretString::new("my_api_key".to_string());
        assert!(s.eq_secret("my_api_key"));
        assert!(!s.eq_secret("wrong_key"));
        assert!(!s.eq_secret(""));
        assert!(!s.eq_secret("my_api_key_extra"));
    }

    #[test]
    fn secret_string_clone_is_independent() {
        let mut s1 = SecretString::new("original".to_string());
        let s2 = s1.clone();
        assert_eq!(s2.as_str(), "original");
        // Zeroizing s1 doesn't affect s2.
        s1.zeroize();
        assert_eq!(s2.as_str(), "original");
    }

    #[test]
    fn audit_severity_clean() {
        let audit = CredentialBoundaryAudit {
            exposed_env_vars: vec![],
            reachable_sockets: vec![],
            accessible_config_dirs: vec![],
            clean: true,
        };
        assert_eq!(audit.severity(), "none");
    }

    #[test]
    fn audit_severity_critical() {
        let audit = CredentialBoundaryAudit {
            exposed_env_vars: vec!["VELOCITY_API_KEY".into()],
            reachable_sockets: vec![("SSH_AUTH_SOCK".into(), "/tmp/sock".into())],
            accessible_config_dirs: vec![],
            clean: false,
        };
        assert_eq!(audit.severity(), "critical");
    }

    #[test]
    fn audit_severity_high_env_only() {
        let audit = CredentialBoundaryAudit {
            exposed_env_vars: vec!["OPENAI_API_KEY".into()],
            reachable_sockets: vec![],
            accessible_config_dirs: vec![],
            clean: false,
        };
        assert_eq!(audit.severity(), "high");
    }

    #[test]
    fn audit_severity_high_socket_only() {
        let audit = CredentialBoundaryAudit {
            exposed_env_vars: vec![],
            reachable_sockets: vec![("SSH_AUTH_SOCK".into(), "/tmp/sock".into())],
            accessible_config_dirs: vec![],
            clean: false,
        };
        assert_eq!(audit.severity(), "high");
    }

    #[test]
    fn audit_severity_medium_config_only() {
        let audit = CredentialBoundaryAudit {
            exposed_env_vars: vec![],
            reachable_sockets: vec![],
            accessible_config_dirs: vec!["/home/user/.ssh".into()],
            clean: false,
        };
        assert_eq!(audit.severity(), "medium");
    }

    #[test]
    fn audit_summary_counts() {
        let audit = CredentialBoundaryAudit {
            exposed_env_vars: vec!["A".into(), "B".into()],
            reachable_sockets: vec![("S".into(), "/p".into())],
            accessible_config_dirs: vec!["D1".into(), "D2".into(), "D3".into()],
            clean: false,
        };
        let summary = audit.summary();
        assert!(!summary.clean);
        assert_eq!(summary.severity, "critical");
        assert_eq!(summary.exposed_env_count, 2);
        assert_eq!(summary.reachable_socket_count, 1);
        assert_eq!(summary.accessible_config_dir_count, 3);
        assert_eq!(summary.total_issues, 6);
    }

    #[test]
    fn audit_summary_serializes() {
        let summary = CredentialAuditSummary {
            clean: false,
            severity: "high".into(),
            exposed_env_count: 1,
            reachable_socket_count: 0,
            accessible_config_dir_count: 0,
            total_issues: 1,
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"severity\":\"high\""));
        assert!(json.contains("\"total_issues\":1"));
    }

    #[test]
    fn warning_message_none_when_clean() {
        let audit = CredentialBoundaryAudit {
            exposed_env_vars: vec![],
            reachable_sockets: vec![],
            accessible_config_dirs: vec![],
            clean: true,
        };
        assert!(audit.warning_message().is_none());
    }

    #[test]
    fn warning_message_mentions_jit_when_dirty() {
        let audit = CredentialBoundaryAudit {
            exposed_env_vars: vec!["VELOCITY_API_KEY".into()],
            reachable_sockets: vec![],
            accessible_config_dirs: vec![],
            clean: false,
        };
        let msg = audit.warning_message().unwrap();
        assert!(msg.contains("JIT code"));
        assert!(msg.contains("exfiltrate"));
    }
}
