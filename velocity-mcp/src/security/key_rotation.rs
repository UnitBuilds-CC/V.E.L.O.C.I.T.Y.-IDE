//! Cryptographic key rotation and secure default configuration.
//!
//! Provides automatic key rotation for workspace master keys, ensuring
//! that encryption keys are periodically refreshed without requiring
//! manual intervention. Old keys are retained for decryption but new
//! data is always encrypted with the current key.

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

/// Default key rotation interval (90 days).
const DEFAULT_ROTATION_INTERVAL: Duration = Duration::from_secs(90 * 24 * 3600);

/// Maximum number of old keys to retain for decryption.
const MAX_RETAINED_KEYS: usize = 8;

/// A cryptographic key version with metadata.
#[derive(Debug, Clone)]
pub struct KeyVersion {
    /// Version identifier (monotonically increasing).
    pub version: u32,
    /// The raw key material (32 bytes for AES-256).
    pub key: Vec<u8>,
    /// When this key was created.
    pub created_at: SystemTime,
    /// Whether this is the current active encryption key.
    pub active: bool,
}

/// Key rotation manager for workspace encryption keys.
///
/// Maintains a current active key and a set of retired keys for
/// backward-compatible decryption. Automatically generates new keys
/// when the rotation interval elapses.
pub struct KeyRotationManager {
    /// Current active key for encryption.
    active_key: KeyVersion,
    /// Retired keys for decryption (version → key).
    retired_keys: HashMap<u32, KeyVersion>,
    /// How often to rotate keys.
    rotation_interval: Duration,
    /// Maximum retired keys to keep.
    max_retained: usize,
    /// Next version number.
    next_version: u32,
    /// When the active key was created (for rotation check).
    last_rotation: Instant,
}

/// Result of a key rotation operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RotationResult {
    /// Key was rotated successfully (old version → new version).
    Rotated { old_version: u32, new_version: u32 },
    /// Key rotation was not yet needed.
    NotNeeded,
    /// Rotation failed with an error message.
    Failed(String),
}

/// Secure default configuration for the IDE's cryptographic subsystem.
#[derive(Debug, Clone)]
pub struct SecureDefaults {
    /// AES key size in bits (256 for AES-256).
    pub aes_key_bits: u32,
    /// GCM nonce size in bytes (12 for AES-GCM).
    pub gcm_nonce_bytes: usize,
    /// GCM tag size in bytes (16 for AES-GCM).
    pub gcm_tag_bytes: usize,
    /// Maximum plaintext size before warning (100 MB).
    pub max_plaintext_bytes: usize,
    /// Whether to use hardware AES-NI when available.
    pub prefer_hardware_aes: bool,
    /// Key derivation iterations (for password-based keys).
    pub kdf_iterations: u32,
    /// Salt size in bytes (32 for PBKDF2).
    pub salt_bytes: usize,
}

impl SecureDefaults {
    /// Returns the recommended secure defaults for the IDE.
    pub fn ide_defaults() -> Self {
        Self {
            aes_key_bits: 256,
            gcm_nonce_bytes: 12,
            gcm_tag_bytes: 16,
            max_plaintext_bytes: 100 * 1024 * 1024,
            prefer_hardware_aes: true,
            kdf_iterations: 600_000,
            salt_bytes: 32,
        }
    }

    /// Validate that a configuration meets minimum security requirements.
    pub fn validate(&self) -> Result<(), String> {
        if self.aes_key_bits < 128 {
            return Err(format!(
                "AES key size {} bits is below minimum 128 bits",
                self.aes_key_bits
            ));
        }
        if self.gcm_nonce_bytes < 12 {
            return Err(format!(
                "GCM nonce {} bytes is below minimum 12 bytes",
                self.gcm_nonce_bytes
            ));
        }
        if self.kdf_iterations < 100_000 {
            return Err(format!(
                "KDF iterations {} is below minimum 100,000",
                self.kdf_iterations
            ));
        }
        if self.salt_bytes < 16 {
            return Err(format!(
                "Salt size {} bytes is below minimum 16 bytes",
                self.salt_bytes
            ));
        }
        Ok(())
    }
}

impl KeyRotationManager {
    /// Create a new key rotation manager with an initial key.
    pub fn new(initial_key: Vec<u8>) -> Self {
        let now = SystemTime::now();
        Self {
            active_key: KeyVersion {
                version: 1,
                key: initial_key,
                created_at: now,
                active: true,
            },
            retired_keys: HashMap::new(),
            rotation_interval: DEFAULT_ROTATION_INTERVAL,
            max_retained: MAX_RETAINED_KEYS,
            next_version: 2,
            last_rotation: Instant::now(),
        }
    }

    /// Create with custom rotation parameters.
    pub fn with_config(
        initial_key: Vec<u8>,
        rotation_interval: Duration,
        max_retained: usize,
    ) -> Self {
        let mut mgr = Self::new(initial_key);
        mgr.rotation_interval = rotation_interval;
        mgr.max_retained = max_retained;
        mgr
    }

    /// Get the current active key version.
    pub fn active_version(&self) -> u32 {
        self.active_key.version
    }

    /// Get the current active key material.
    pub fn active_key(&self) -> &[u8] {
        &self.active_key.key
    }

    /// Check if key rotation is needed based on elapsed time.
    pub fn needs_rotation(&self) -> bool {
        self.last_rotation.elapsed() >= self.rotation_interval
    }

    /// Rotate the key with new key material.
    ///
    /// The current active key is retired and the new key becomes active.
    /// If too many retired keys exist, the oldest is evicted.
    pub fn rotate(&mut self, new_key: Vec<u8>) -> RotationResult {
        if new_key.is_empty() {
            return RotationResult::Failed("New key material is empty".into());
        }

        let old_version = self.active_key.version;

        // Retire the current key
        let mut retired = std::mem::replace(
            &mut self.active_key,
            KeyVersion {
                version: self.next_version,
                key: new_key,
                created_at: SystemTime::now(),
                active: true,
            },
        );
        retired.active = false;

        // Store retired key
        self.retired_keys.insert(old_version, retired);

        // Evict oldest if over limit
        if self.retired_keys.len() > self.max_retained {
            if let Some(oldest_version) = self.retired_keys.keys().min().copied() {
                self.retired_keys.remove(&oldest_version);
            }
        }

        self.next_version += 1;
        self.last_rotation = Instant::now();

        RotationResult::Rotated {
            old_version,
            new_version: self.active_key.version,
        }
    }

    /// Look up a key by version (active or retired).
    pub fn get_key(&self, version: u32) -> Option<&[u8]> {
        if version == self.active_key.version {
            Some(&self.active_key.key)
        } else {
            self.retired_keys.get(&version).map(|k| k.key.as_slice())
        }
    }

    /// Get the number of retained old keys.
    pub fn retained_count(&self) -> usize {
        self.retired_keys.len()
    }

    /// Get all active and retired key versions.
    pub fn all_versions(&self) -> Vec<u32> {
        let mut versions: Vec<u32> = self.retired_keys.keys().copied().collect();
        versions.push(self.active_key.version);
        versions.sort();
        versions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> Vec<u8> {
        vec![0xAB; 32]
    }

    fn test_key_v2() -> Vec<u8> {
        vec![0xCD; 32]
    }

    #[test]
    fn test_new_manager_has_version_1() {
        let mgr = KeyRotationManager::new(test_key());
        assert_eq!(mgr.active_version(), 1);
        assert_eq!(mgr.active_key(), &test_key());
        assert_eq!(mgr.retained_count(), 0);
    }

    #[test]
    fn test_rotate_increments_version() {
        let mut mgr = KeyRotationManager::new(test_key());
        let result = mgr.rotate(test_key_v2());
        assert_eq!(
            result,
            RotationResult::Rotated {
                old_version: 1,
                new_version: 2
            }
        );
        assert_eq!(mgr.active_version(), 2);
        assert_eq!(mgr.active_key(), &test_key_v2());
    }

    #[test]
    fn test_retired_key_still_accessible() {
        let mut mgr = KeyRotationManager::new(test_key());
        mgr.rotate(test_key_v2());
        assert_eq!(mgr.get_key(1), Some(test_key().as_slice()));
        assert_eq!(mgr.get_key(2), Some(test_key_v2().as_slice()));
    }

    #[test]
    fn test_rotate_rejects_empty_key() {
        let mut mgr = KeyRotationManager::new(test_key());
        let result = mgr.rotate(vec![]);
        assert!(matches!(result, RotationResult::Failed(_)));
        assert_eq!(mgr.active_version(), 1); // unchanged
    }

    #[test]
    fn test_eviction_when_over_max_retained() {
        let mut mgr = KeyRotationManager::with_config(test_key(), Duration::from_secs(0), 3);
        for i in 0..5 {
            let key = vec![i as u8; 32];
            mgr.rotate(key);
        }
        // Should have at most 3 retained keys
        assert!(mgr.retained_count() <= 3);
    }

    #[test]
    fn test_all_versions_sorted() {
        let mut mgr = KeyRotationManager::new(test_key());
        mgr.rotate(vec![0x02; 32]);
        mgr.rotate(vec![0x03; 32]);
        mgr.rotate(vec![0x04; 32]);
        let versions = mgr.all_versions();
        assert!(versions.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn test_needs_rotation_initially_false() {
        let mgr = KeyRotationManager::new(test_key());
        assert!(!mgr.needs_rotation());
    }

    #[test]
    fn test_needs_rotation_with_zero_interval() {
        let mgr = KeyRotationManager::with_config(test_key(), Duration::from_secs(0), 8);
        // With zero interval, rotation is immediately needed
        // (elapsed time is always >= 0)
        assert!(mgr.needs_rotation());
    }

    #[test]
    fn test_get_key_unknown_version() {
        let mgr = KeyRotationManager::new(test_key());
        assert_eq!(mgr.get_key(999), None);
    }

    #[test]
    fn test_secure_defaults_valid() {
        let defaults = SecureDefaults::ide_defaults();
        assert!(defaults.validate().is_ok());
        assert_eq!(defaults.aes_key_bits, 256);
        assert_eq!(defaults.gcm_nonce_bytes, 12);
        assert_eq!(defaults.kdf_iterations, 600_000);
    }

    #[test]
    fn test_secure_defaults_reject_weak() {
        let mut weak = SecureDefaults::ide_defaults();
        weak.aes_key_bits = 64;
        assert!(weak.validate().is_err());

        weak = SecureDefaults::ide_defaults();
        weak.kdf_iterations = 1000;
        assert!(weak.validate().is_err());

        weak = SecureDefaults::ide_defaults();
        weak.salt_bytes = 4;
        assert!(weak.validate().is_err());
    }

    #[test]
    fn test_multiple_rotations() {
        let mut mgr = KeyRotationManager::new(vec![1; 32]);
        for i in 2..=10u8 {
            let result = mgr.rotate(vec![i; 32]);
            assert!(matches!(result, RotationResult::Rotated { .. }));
        }
        assert_eq!(mgr.active_version(), 10);
        assert_eq!(mgr.active_key(), &vec![10u8; 32]);
        // Old keys should be accessible (up to max_retained)
        assert!(mgr.get_key(9).is_some());
    }
}
