//! Encrypted-at-rest secret store.
//!
//! Named secrets (API tokens, webhook URLs, passwords) are held in memory as a
//! `name -> value` map and persisted to `.velocity/secrets.nda`, sealed under
//! the `secrets` artifact class via [`crate::agent::crypto`]. Values are never
//! written in the clear; if key material is unavailable, [`SecretStore::save`]
//! fails loudly rather than leaking plaintext.
//!
//! Consumers (connectors, providers) resolve credentials by *handle*, and
//! [`SecretStore::redact`] scrubs known secret values out of any text before it
//! is logged or shown in the UI.

use std::collections::BTreeMap;
use std::path::Path;

/// Artifact class label used for HKDF subkey derivation (domain separation).
const SECRETS_LABEL: &[u8] = b"secrets";
/// On-disk file name under `.velocity/`.
const SECRETS_FILE: &str = "secrets.nda";
/// Values shorter than this are not used for redaction (avoids scrubbing
/// trivial tokens like "1" out of unrelated text).
const MIN_REDACT_LEN: usize = 4;

/// In-memory, encrypted-at-rest map of named secrets.
#[derive(Debug, Clone, Default)]
pub struct SecretStore {
    entries: BTreeMap<String, String>,
}

impl SecretStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load and decrypt the store from `.velocity/secrets.nda`. A missing file
    /// yields an empty store. A file that fails to decrypt also yields an empty
    /// store (rather than surfacing ciphertext as garbage entries).
    pub fn load(workspace_root: &Path) -> Self {
        let path = workspace_root.join(".velocity").join(SECRETS_FILE);
        let Ok(bytes) = std::fs::read(&path) else {
            return Self::new();
        };
        let plain = crate::agent::crypto::open(workspace_root, SECRETS_LABEL, &bytes);
        if plain.is_empty() {
            return Self::new();
        }
        match serde_json::from_slice::<BTreeMap<String, String>>(&plain) {
            Ok(entries) => Self { entries },
            Err(_) => Self::new(),
        }
    }

    /// Seal and persist the store. Returns an error (without writing) if no key
    /// material or randomness is available, so secrets are never left in the
    /// clear on disk.
    pub fn save(&self, workspace_root: &Path) -> Result<(), String> {
        let json = serde_json::to_vec(&self.entries)
            .map_err(|e| format!("secret store serialize failed: {e}"))?;
        let sealed = crate::agent::crypto::seal(workspace_root, SECRETS_LABEL, &json)
            .ok_or_else(|| "no key material available to seal secrets".to_string())?;
        let dir = workspace_root.join(".velocity");
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create .velocity dir: {e}"))?;
        std::fs::write(dir.join(SECRETS_FILE), &sealed)
            .map_err(|e| format!("cannot write secrets file: {e}"))
    }

    /// Insert or overwrite a secret. Empty names are rejected.
    pub fn set(&mut self, name: impl Into<String>, value: impl Into<String>) -> bool {
        let name = name.into();
        if name.trim().is_empty() {
            return false;
        }
        self.entries.insert(name, value.into());
        true
    }

    /// Resolve a secret value by handle.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries.get(name).map(String::as_str)
    }

    /// Remove a secret, returning whether it existed.
    pub fn remove(&mut self, name: &str) -> bool {
        self.entries.remove(name).is_some()
    }

    /// Whether a handle exists.
    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    /// Number of stored secrets.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store holds no secrets.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Sorted list of handles (names only — never values), for UI listing.
    pub fn handles(&self) -> Vec<&str> {
        self.entries.keys().map(String::as_str).collect()
    }

    /// A masked preview for a handle, e.g. `sk-a…9f` → `sk-a••` (first 4 chars
    /// then bullets). Returns `None` if the handle is unknown.
    pub fn masked(&self, name: &str) -> Option<String> {
        self.entries.get(name).map(|v| mask_value(v))
    }

    /// Replace every known secret value occurring in `text` with a bullet mask,
    /// so credentials cannot leak into logs, transcripts, or the UI.
    pub fn redact(&self, text: &str) -> String {
        let mut out = text.to_string();
        // Redact longer values first so a value that is a substring of another
        // does not partially unmask it.
        let mut values: Vec<&String> = self
            .entries
            .values()
            .filter(|v| v.len() >= MIN_REDACT_LEN)
            .collect();
        values.sort_by_key(|v| std::cmp::Reverse(v.len()));
        for value in values {
            if out.contains(value.as_str()) {
                out = out.replace(
                    value.as_str(),
                    "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}",
                );
            }
        }
        out
    }
}

/// Mask a raw value for display: keep the first 4 characters, replace the rest
/// with bullets (bounded so very long tokens don't produce huge strings).
fn mask_value(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 4 {
        return "\u{2022}\u{2022}\u{2022}\u{2022}".to_string();
    }
    let visible: String = chars.iter().take(4).collect();
    let hidden = (chars.len() - 4).min(8);
    format!("{visible}{}", "\u{2022}".repeat(hidden))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_remove() {
        let mut s = SecretStore::new();
        assert!(s.set("github_token", "ghp_secret_value"));
        assert_eq!(s.get("github_token"), Some("ghp_secret_value"));
        assert!(s.contains("github_token"));
        assert_eq!(s.len(), 1);
        assert!(s.remove("github_token"));
        assert!(!s.contains("github_token"));
        assert!(s.is_empty());
    }

    #[test]
    fn empty_name_rejected() {
        let mut s = SecretStore::new();
        assert!(!s.set("   ", "value"));
        assert!(s.is_empty());
    }

    #[test]
    fn save_load_round_trip_encrypted() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = SecretStore::new();
        s.set("slack_webhook", "https://hooks.slack.com/services/XXX");
        s.set("api_key", "sk-1234567890abcdef");
        s.save(tmp.path()).expect("save");

        // On-disk bytes must be an NDA1 envelope, not the plaintext value.
        let on_disk = std::fs::read(tmp.path().join(".velocity").join(SECRETS_FILE)).unwrap();
        assert_eq!(&on_disk[0..4], b"NDA1");
        let as_text = String::from_utf8_lossy(&on_disk);
        assert!(!as_text.contains("sk-1234567890abcdef"));

        let loaded = SecretStore::load(tmp.path());
        assert_eq!(loaded.get("api_key"), Some("sk-1234567890abcdef"));
        assert_eq!(
            loaded.get("slack_webhook"),
            Some("https://hooks.slack.com/services/XXX")
        );
    }

    #[test]
    fn missing_file_yields_empty() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(SecretStore::load(tmp.path()).is_empty());
    }

    #[test]
    fn redact_scrubs_known_values() {
        let mut s = SecretStore::new();
        s.set("token", "supersecrettoken123");
        let log = "calling api with Authorization: Bearer supersecrettoken123 now";
        let scrubbed = s.redact(log);
        assert!(!scrubbed.contains("supersecrettoken123"));
        assert!(scrubbed.contains("\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}"));
    }

    #[test]
    fn short_values_not_used_for_redaction() {
        let mut s = SecretStore::new();
        s.set("n", "ab");
        assert_eq!(s.redact("ab cd ab"), "ab cd ab");
    }

    #[test]
    fn masked_hides_body() {
        let mut s = SecretStore::new();
        s.set("k", "abcdefghijkl");
        let m = s.masked("k").unwrap();
        assert!(m.starts_with("abcd"));
        assert!(m.contains('\u{2022}'));
        assert!(!m.contains("efgh"));
        assert!(s.masked("missing").is_none());
    }
}
