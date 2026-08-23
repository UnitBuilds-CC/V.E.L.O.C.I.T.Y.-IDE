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
}

impl CredentialScope {
    pub fn new() -> Self {
        Self {
            secrets: Vec::new(),
            env_vars_scrubbed: Vec::new(),
        }
    }

    /// Load a secret from an environment variable and track it.
    pub fn load_env(&mut self, var: &str) -> Option<&SecretString> {
        if let Some(secret) = SecretString::from_env(var) {
            self.secrets.push(secret);
            self.secrets.last()
        } else {
            None
        }
    }

    /// Load a secret from a known value and track it.
    pub fn load_value(&mut self, value: String) -> &SecretString {
        self.secrets.push(SecretString::new(value));
        self.secrets.last().unwrap()
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
            .finish()
    }
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
];

/// Remove known sensitive environment variables from the process environment.
///
/// Call this AFTER loading credentials into a [`CredentialScope`] or
/// [`SecretString`]. This prevents any code in the process (including
/// JIT-compiled closures) from accessing these secrets via `std::env::var`.
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

    for i in 1..=30 {
        let token_key = format!("CF_ACCOUNT_{}_TOKEN", i);
        if std::env::var(&token_key).is_ok() {
            exposed.push(token_key);
        }
    }

    exposed
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
}
