//! At-rest encryption for `.velocity/*.nda` artifacts.
//!
//! Model: one 32-byte master key per workspace, generated once and sealed at
//! rest by the OS key store (Windows DPAPI via FFI, bound to the current user
//! account and machine). Per-artifact subkeys are derived from the master via
//! HKDF (`velocity_browser::nda::derive_nda_key`) so that, e.g., the
//! per-workspace sitemap and the core chat/transcript files never share a key.
//! Payloads are sealed with AES-256-GCM (hardware-accelerated AES-NI when
//! available) using the vetted `velocity-browser` `NDA1` envelope, which binds
//! a SHA-256 integrity tag and the header as AEAD additional-data.
//!
//! This unifies the tiering: the master key is *per workspace* (unique, stored
//! in the workspace) and *keyring-sealed* (recoverable only by the current OS
//! user), while HKDF gives cryptographic domain separation per artifact class.

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MASTER_KEY_LEN: usize = 32;
const KEY_FILE: &str = "nda.key";

/// Cache of decrypted per-workspace master keys, so we hit the OS keyring at
/// most once per workspace per process rather than on every artifact write.
static KEY_CACHE: Lazy<Mutex<HashMap<PathBuf, [u8; MASTER_KEY_LEN]>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// --- OS-backed primitives: random + keyring sealing --------------------------

#[cfg(windows)]
mod os {
    use core::ffi::c_void;

    #[repr(C)]
    struct DataBlob {
        cb_data: u32,
        pb_data: *mut u8,
    }

    // Ties every sealed blob to this application; the same entropy must be
    // supplied to unseal, preventing cross-app unprotection.
    const ENTROPY: &[u8] = b"velocity-nda-keyring-v1";
    const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;
    const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x0000_0002;

    #[link(name = "Crypt32")]
    extern "system" {
        fn CryptProtectData(
            p_data_in: *mut DataBlob,
            sz_data_descr: *const u16,
            p_optional_entropy: *mut DataBlob,
            pv_reserved: *mut c_void,
            p_prompt_struct: *mut c_void,
            dw_flags: u32,
            p_data_out: *mut DataBlob,
        ) -> i32;
        fn CryptUnprotectData(
            p_data_in: *mut DataBlob,
            pp_sz_data_descr: *mut *mut u16,
            p_optional_entropy: *mut DataBlob,
            pv_reserved: *mut c_void,
            p_prompt_struct: *mut c_void,
            dw_flags: u32,
            p_data_out: *mut DataBlob,
        ) -> i32;
    }

    #[link(name = "Bcrypt")]
    extern "system" {
        fn BCryptGenRandom(
            h_algorithm: *mut c_void,
            pb_buffer: *mut u8,
            cb_buffer: u32,
            dw_flags: u32,
        ) -> i32;
    }

    extern "system" {
        fn LocalFree(h_mem: *mut c_void) -> *mut c_void;
    }

    pub fn random(buf: &mut [u8]) -> bool {
        if buf.is_empty() {
            return true;
        }
        // SAFETY: BCryptGenRandom with null algorithm handle and BCRYPT_USE_SYSTEM_PREFERRED_RNG
        // uses the system's preferred RNG. buf.as_mut_ptr() is valid for buf.len() bytes.
        unsafe {
            BCryptGenRandom(
                core::ptr::null_mut(),
                buf.as_mut_ptr(),
                buf.len() as u32,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            ) == 0
        }
    }

    pub fn protect(plaintext: &[u8]) -> Option<Vec<u8>> {
        // SAFETY: CryptProtectData encrypts plaintext using DPAPI. We provide valid DataBlob
        // pointers and entropy. The output blob is allocated by the system and must be freed
        // with LocalFree. We check for null output and free the memory after copying.
        unsafe {
            let mut in_blob = DataBlob {
                cb_data: plaintext.len() as u32,
                pb_data: plaintext.as_ptr() as *mut u8,
            };
            let mut entropy = DataBlob {
                cb_data: ENTROPY.len() as u32,
                pb_data: ENTROPY.as_ptr() as *mut u8,
            };
            let mut out = DataBlob {
                cb_data: 0,
                pb_data: core::ptr::null_mut(),
            };
            let ok = CryptProtectData(
                &mut in_blob,
                core::ptr::null(),
                &mut entropy,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            );
            if ok == 0 || out.pb_data.is_null() {
                return None;
            }
            let sealed = core::slice::from_raw_parts(out.pb_data, out.cb_data as usize).to_vec();
            LocalFree(out.pb_data as *mut c_void);
            Some(sealed)
        }
    }

    pub fn unprotect(sealed: &[u8]) -> Option<Vec<u8>> {
        // SAFETY: CryptUnprotectData decrypts DPAPI-sealed data. Same safety reasoning as protect():
        // valid input blobs, system-allocated output, null-checked, freed after copy.
        unsafe {
            let mut in_blob = DataBlob {
                cb_data: sealed.len() as u32,
                pb_data: sealed.as_ptr() as *mut u8,
            };
            let mut entropy = DataBlob {
                cb_data: ENTROPY.len() as u32,
                pb_data: ENTROPY.as_ptr() as *mut u8,
            };
            let mut out = DataBlob {
                cb_data: 0,
                pb_data: core::ptr::null_mut(),
            };
            let ok = CryptUnprotectData(
                &mut in_blob,
                core::ptr::null_mut(),
                &mut entropy,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            );
            if ok == 0 || out.pb_data.is_null() {
                return None;
            }
            let plain = core::slice::from_raw_parts(out.pb_data, out.cb_data as usize).to_vec();
            LocalFree(out.pb_data as *mut c_void);
            Some(plain)
        }
    }
}

#[cfg(all(unix, not(windows)))]
mod os {
    // Unix fallback: derive a machine-specific key from hostname + uid and use
    // the NDA seal/open primitives to protect the master key at rest. This is
    // not account-bound like DPAPI, but provides real encryption on disk.
    pub fn random(buf: &mut [u8]) -> bool {
        use std::io::Read;
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(buf))
            .is_ok()
    }

    /// Derive a machine-specific protection key from hostname + uid.
    fn machine_key() -> [u8; 32] {
        let hostname = std::fs::read_to_string("/etc/hostname")
            .unwrap_or_else(|_| "localhost".to_string());
        let uid = unsafe { libc::getuid() };
        let mut seed = hostname.into_bytes();
        seed.extend_from_slice(&uid.to_le_bytes());
        velocity_browser::nda::derive_nda_key(&seed, b"unix_machine_protect")
    }

    pub fn protect(plaintext: &[u8]) -> Option<Vec<u8>> {
        let key = machine_key();
        let mut nonce = [0u8; 12];
        if !random(&mut nonce) {
            return None;
        }
        Some(velocity_browser::nda::seal_bytes(&key, &nonce, plaintext))
    }

    pub fn unprotect(sealed: &[u8]) -> Option<Vec<u8>> {
        // Check if it's an NDA envelope
        if sealed.len() >= 4 && sealed[0..4] == velocity_browser::nda::NDA_MAGIC {
            let key = machine_key();
            velocity_browser::nda::open_bytes(&key, sealed).ok()
        } else {
            // Legacy plaintext format — pass through
            Some(sealed.to_vec())
        }
    }
}

#[cfg(not(any(windows, unix)))]
mod os {
    pub fn random(_buf: &mut [u8]) -> bool {
        false
    }
    pub fn protect(plaintext: &[u8]) -> Option<Vec<u8>> {
        Some(plaintext.to_vec())
    }
    pub fn unprotect(sealed: &[u8]) -> Option<Vec<u8>> {
        Some(sealed.to_vec())
    }
}

// --- Key management ----------------------------------------------------------

/// Load (or, on first use, generate) the workspace master key. The key is held
/// in memory only after being unsealed by the OS keyring; on disk it lives at
/// `.velocity/nda.key` in DPAPI-sealed form.
pub fn workspace_master_key(workspace_root: &Path) -> Option<[u8; MASTER_KEY_LEN]> {
    let key_dir = workspace_root.join(".velocity");
    let key_path = key_dir.join(KEY_FILE);

    if let Ok(cache) = KEY_CACHE.lock() {
        if let Some(key) = cache.get(&key_path) {
            return Some(*key);
        }
    }

    // Try to load and unseal an existing key.
    if let Ok(sealed) = std::fs::read(&key_path) {
        if let Some(raw) = os::unprotect(&sealed) {
            if raw.len() == MASTER_KEY_LEN {
                let mut key = [0u8; MASTER_KEY_LEN];
                key.copy_from_slice(&raw);
                if let Ok(mut cache) = KEY_CACHE.lock() {
                    cache.insert(key_path.clone(), key);
                }
                return Some(key);
            }
        }
    }

    // Generate a fresh key, seal it, and persist.
    let mut key = [0u8; MASTER_KEY_LEN];
    if !os::random(&mut key) {
        return None;
    }
    let sealed = os::protect(&key)?;
    let _ = std::fs::create_dir_all(&key_dir);
    if std::fs::write(&key_path, &sealed).is_err() {
        return None;
    }
    if let Ok(mut cache) = KEY_CACHE.lock() {
        cache.insert(key_path, key);
    }
    Some(key)
}

/// Derive the AES-256 subkey for a given artifact class from the master key.
fn subkey(master: &[u8; MASTER_KEY_LEN], label: &[u8]) -> [u8; 32] {
    velocity_browser::nda::derive_nda_key(master, label)
}

// --- At-rest codec choke point -----------------------------------------------

/// Seal `plaintext` for the artifact class `label`, returning an AES-256-GCM
/// `NDA1` envelope. Returns `None` if no key material or randomness is
/// available (callers fall back to writing plaintext to avoid data loss).
pub fn seal(workspace_root: &Path, label: &[u8], plaintext: &[u8]) -> Option<Vec<u8>> {
    let master = workspace_master_key(workspace_root)?;
    let key = subkey(&master, label);
    let mut nonce = [0u8; 12];
    if !os::random(&mut nonce) {
        return None;
    }
    Some(velocity_browser::nda::seal_bytes(&key, &nonce, plaintext))
}

/// Open bytes read from disk. Backward-compatible: an `NDA1` envelope is
/// authenticated and decrypted with the artifact subkey; anything else (legacy
/// plaintext or an `NDAV` container) is returned unchanged for the caller's
/// existing parsers. An envelope that fails to open yields empty bytes rather
/// than surfacing ciphertext as text.
pub fn open(workspace_root: &Path, label: &[u8], bytes: &[u8]) -> Vec<u8> {
    if bytes.len() >= 4 && bytes[0..4] == velocity_browser::nda::NDA_MAGIC {
        if let Some(master) = workspace_master_key(workspace_root) {
            let key = subkey(&master, label);
            if let Ok(plain) = velocity_browser::nda::open_bytes(&key, bytes) {
                return plain;
            }
        }
        return Vec::new();
    }
    bytes.to_vec()
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn keyring_round_trip() {
        let sealed = os::protect(b"top secret master key").expect("DPAPI protect");
        assert_ne!(sealed.as_slice(), b"top secret master key");
        let opened = os::unprotect(&sealed).expect("DPAPI unprotect");
        assert_eq!(opened.as_slice(), b"top secret master key");
    }

    #[test]
    fn os_random_fills_distinct_bytes() {
        let mut a = [0u8; 32];
        let mut b = [0u8; 32];
        assert!(os::random(&mut a));
        assert!(os::random(&mut b));
        assert_ne!(a, b);
    }

    #[test]
    fn seal_open_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let plaintext = b"sitemap version 2\nentry_count 0\n";
        let env = seal(tmp.path(), b"sitemap", plaintext).expect("seal");
        assert_eq!(&env[0..4], b"NDA1");
        assert_ne!(env.as_slice(), plaintext); // actually encrypted
        assert_eq!(open(tmp.path(), b"sitemap", &env), plaintext);
    }

    #[test]
    fn wrong_label_cannot_open() {
        let tmp = tempfile::tempdir().unwrap();
        let env = seal(tmp.path(), b"chatlogs", b"private conversation").unwrap();
        // A different artifact class derives a different subkey -> auth fails.
        assert!(open(tmp.path(), b"sitemap", &env).is_empty());
    }

    #[test]
    fn legacy_plaintext_passes_through() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = b"changelog version 2\nentry_count 0\n";
        assert_eq!(open(tmp.path(), b"changelog", legacy), legacy);
    }

    #[test]
    fn master_key_is_stable_and_sealed_on_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let k1 = workspace_master_key(tmp.path()).unwrap();
        let k2 = workspace_master_key(tmp.path()).unwrap();
        assert_eq!(k1, k2);
        let on_disk = std::fs::read(tmp.path().join(".velocity").join("nda.key")).unwrap();
        assert_ne!(on_disk.as_slice(), &k1[..]); // never stored in the clear
    }
}
