/// TLS JA3/JA3S fingerprint profile for browser impersonation.
#[derive(Debug, Clone)]
pub struct TlsJa3Profile {
    pub ja3_hash: String,
    pub cipher_suites: Vec<u16>,
    pub tls_version: u16,
    /// Named groups (supported_groups extension).
    pub named_groups: Vec<u16>,
    /// Signature algorithms.
    pub sig_algs: Vec<u16>,
    /// EC point formats.
    pub ec_point_formats: Vec<u8>,
    /// Browser name this profile impersonates.
    pub browser_name: String,
}

/// TLS fingerprint rotator with multiple browser profiles.
pub struct TlsFingerprintRotator {
    pub active_profile: TlsJa3Profile,
    pub profiles: Vec<TlsJa3Profile>,
    pub current_index: usize,
}

impl TlsFingerprintRotator {
    /// Create with V.E.L.O.C.I.T.Y. native profile.
    pub fn velocity_native() -> Self {
        let native = Self::chrome_windows_profile();
        let profiles = Self::all_profiles();
        let idx = profiles
            .iter()
            .position(|p| p.browser_name == native.browser_name)
            .unwrap_or(0);
        Self {
            active_profile: native,
            profiles,
            current_index: idx,
        }
    }

    /// Rotate to the next profile in the library.
    pub fn rotate_profile(&mut self) -> &TlsJa3Profile {
        self.current_index = (self.current_index + 1) % self.profiles.len();
        self.active_profile = self.profiles[self.current_index].clone();
        &self.active_profile
    }

    /// Select a specific browser profile by name.
    pub fn select_profile(&mut self, browser_name: &str) -> Result<&TlsJa3Profile, &'static str> {
        if let Some(idx) = self
            .profiles
            .iter()
            .position(|p| p.browser_name == browser_name)
        {
            self.current_index = idx;
            self.active_profile = self.profiles[idx].clone();
            Ok(&self.active_profile)
        } else {
            Err("Profile not found")
        }
    }

    /// Get all available profile names.
    pub fn available_profiles(&self) -> Vec<&str> {
        self.profiles
            .iter()
            .map(|p| p.browser_name.as_str())
            .collect()
    }

    /// Chrome 120 on Windows 10 — most common desktop fingerprint.
    pub fn chrome_windows_profile() -> TlsJa3Profile {
        TlsJa3Profile {
            ja3_hash: "771,4865-4866-4867-49195-49199-49196-49200-52393-52392-49171-49172-156-157-47-53,0-23-65281-10-10-11-13-35-16-5-5-18-23-43-27-17513-21,29-23-24,0".to_string(),
            cipher_suites: vec![0x1301, 0x1302, 0x1303, 0xc02b, 0xc02f, 0xc02c, 0xc030, 0xcca9, 0xcca8, 0xc013, 0xc014, 0x009c, 0x009d, 0x002f, 0x0035],
            tls_version: 0x0304,
            named_groups: vec![0x001d, 0x0017, 0x0018],
            sig_algs: vec![0x0403, 0x0503, 0x0603, 0x0804, 0x0805, 0x0806],
            ec_point_formats: vec![0],
            browser_name: "chrome_120_win10".to_string(),
        }
    }

    /// Firefox 121 on Windows 10.
    pub fn firefox_windows_profile() -> TlsJa3Profile {
        TlsJa3Profile {
            ja3_hash: "771,4865-4867-4866-49195-49199-49196-49200-52393-52392-49171-49172-156-157-47-53,0-23-65281-10-10-11-13-35-16-5-5-18-23-43-27-17513-21,29-23-24,0".to_string(),
            cipher_suites: vec![0x1301, 0x1303, 0x1302, 0xc02b, 0xc02f, 0xc02c, 0xc030, 0xcca9, 0xcca8, 0xc013, 0xc014, 0x009c, 0x009d, 0x002f, 0x0035],
            tls_version: 0x0304,
            named_groups: vec![0x001d, 0x0017, 0x0018],
            sig_algs: vec![0x0403, 0x0503, 0x0603, 0x0804, 0x0805, 0x0806],
            ec_point_formats: vec![0],
            browser_name: "firefox_121_win10".to_string(),
        }
    }

    /// Safari 17 on macOS.
    pub fn safari_macos_profile() -> TlsJa3Profile {
        TlsJa3Profile {
            ja3_hash: "771,4865-4866-4867-49196-49195-49200-49199-52393-52392-49172-49171-157-156-53-47-49162-49161-49192-49191-47-53-10,0-23-65281-10-10-11-13-35-16-5-5-18-23-43-27-21,29-23-24-25,0".to_string(),
            cipher_suites: vec![0x1301, 0x1302, 0x1303, 0xc02c, 0xc02b, 0xc030, 0xc02f, 0xcca9, 0xcca8, 0xc014, 0xc013, 0x009d, 0x009c, 0x0035, 0x002f],
            tls_version: 0x0304,
            named_groups: vec![0x001d, 0x0017, 0x0018, 0x0019],
            sig_algs: vec![0x0403, 0x0503, 0x0603],
            ec_point_formats: vec![0],
            browser_name: "safari_17_macos".to_string(),
        }
    }

    /// Chrome 120 on Android.
    pub fn chrome_android_profile() -> TlsJa3Profile {
        TlsJa3Profile {
            ja3_hash: "771,4865-4866-4867-49195-49199-49196-49200-52393-52392-49171-49172-156-157-47-53,0-23-65281-10-10-11-13-35-16-5-18-23-27-43-5-17513-21,29-23-24,0".to_string(),
            cipher_suites: vec![0x1301, 0x1302, 0x1303, 0xc02b, 0xc02f, 0xc02c, 0xc030, 0xcca9, 0xcca8, 0xc013, 0xc014, 0x009c, 0x009d, 0x002f, 0x0035],
            tls_version: 0x0304,
            named_groups: vec![0x001d, 0x0017, 0x0018],
            sig_algs: vec![0x0403, 0x0503, 0x0603, 0x0804, 0x0805, 0x0806],
            ec_point_formats: vec![0],
            browser_name: "chrome_120_android".to_string(),
        }
    }

    /// Build the full profile library.
    fn all_profiles() -> Vec<TlsJa3Profile> {
        vec![
            Self::chrome_windows_profile(),
            Self::firefox_windows_profile(),
            Self::safari_macos_profile(),
            Self::chrome_android_profile(),
            Self::edge_windows_profile(),
            Self::brave_windows_profile(),
        ]
    }

    /// Microsoft Edge 120 on Windows 11 — shares Chromium fingerprint base
    /// with different extension ordering.
    pub fn edge_windows_profile() -> TlsJa3Profile {
        TlsJa3Profile {
            ja3_hash: "771,4865-4866-4867-49195-49199-49196-49200-52393-52392-49171-49172-156-157-47-53,0-23-65281-10-11-13-35-16-5-18-23-27-43-17513-21,29-23-24,0".to_string(),
            cipher_suites: vec![0x1301, 0x1302, 0x1303, 0xc02b, 0xc02f, 0xc02c, 0xc030, 0xcca9, 0xcca8, 0xc013, 0xc014, 0x009c, 0x009d, 0x002f, 0x0035],
            tls_version: 0x0304,
            named_groups: vec![0x001d, 0x0017, 0x0018],
            sig_algs: vec![0x0403, 0x0503, 0x0603, 0x0804, 0x0805, 0x0806],
            ec_point_formats: vec![0],
            browser_name: "edge_120_win11".to_string(),
        }
    }

    /// Brave 1.62 on Windows — Chromium-based with randomized extension order.
    pub fn brave_windows_profile() -> TlsJa3Profile {
        TlsJa3Profile {
            ja3_hash: "771,4865-4867-4866-49195-49199-49196-49200-52393-52392-49171-49172-156-157-47-53,0-23-65281-10-11-13-35-16-5-18-23-27-43-17513-21,29-23-24,0".to_string(),
            cipher_suites: vec![0x1301, 0x1303, 0x1302, 0xc02b, 0xc02f, 0xc02c, 0xc030, 0xcca9, 0xcca8, 0xc013, 0xc014, 0x009c, 0x009d, 0x002f, 0x0035],
            tls_version: 0x0304,
            named_groups: vec![0x001d, 0x0017, 0x0018],
            sig_algs: vec![0x0403, 0x0503, 0x0603, 0x0804, 0x0805],
            ec_point_formats: vec![0],
            browser_name: "brave_162_win11".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_native_profile() {
        let rotator = TlsFingerprintRotator::velocity_native();
        assert!(!rotator.active_profile.ja3_hash.is_empty());
        assert!(!rotator.active_profile.cipher_suites.is_empty());
        assert_eq!(rotator.active_profile.tls_version, 0x0304);
    }

    #[test]
    fn test_rotate() {
        let mut rotator = TlsFingerprintRotator::velocity_native();
        let first = rotator.active_profile.browser_name.clone();
        rotator.rotate_profile();
        assert_ne!(rotator.active_profile.browser_name, first);
    }

    #[test]
    fn test_select_profile() {
        let mut rotator = TlsFingerprintRotator::velocity_native();
        rotator.select_profile("firefox_121_win10").unwrap();
        assert_eq!(rotator.active_profile.browser_name, "firefox_121_win10");
    }

    #[test]
    fn test_select_nonexistent() {
        let mut rotator = TlsFingerprintRotator::velocity_native();
        assert!(rotator.select_profile("nonexistent").is_err());
    }

    #[test]
    fn test_available_profiles() {
        let rotator = TlsFingerprintRotator::velocity_native();
        let profiles = rotator.available_profiles();
        assert!(profiles.len() >= 6);
        assert!(profiles.contains(&"chrome_120_win10"));
        assert!(profiles.contains(&"safari_17_macos"));
        assert!(profiles.contains(&"edge_120_win11"));
        assert!(profiles.contains(&"brave_162_win11"));
    }

    #[test]
    fn test_rotate_wraps() {
        let mut rotator = TlsFingerprintRotator::velocity_native();
        let count = rotator.profiles.len();
        let original = rotator.active_profile.browser_name.clone();
        for _ in 0..count {
            rotator.rotate_profile();
        }
        assert_eq!(rotator.active_profile.browser_name, original);
    }
}
