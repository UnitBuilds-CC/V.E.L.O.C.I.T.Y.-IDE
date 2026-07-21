#[derive(Debug, Clone)]
pub struct TlsJa3Profile {
    pub ja3_hash: String,
    pub cipher_suites: Vec<u16>,
    pub tls_version: u16,
}

pub struct TlsFingerprintRotator {
    pub active_profile: TlsJa3Profile,
}

impl TlsFingerprintRotator {
    pub fn chrome_desktop() -> Self {
        Self {
            active_profile: TlsJa3Profile {
                ja3_hash: "771,4865-4866-4867-49195-49199,0-23-65281-10-11,29-23-24,0".to_string(),
                cipher_suites: vec![0x1301, 0x1302, 0x1303, 0xc02b, 0xc02f],
                tls_version: 0x0304, // TLS 1.3
            },
        }
    }

    pub fn rotate_profile(&mut self) -> &TlsJa3Profile {
        self.active_profile.ja3_hash = format!("rotated_ja3_{}", self.active_profile.cipher_suites.len());
        &self.active_profile
    }
}
