//! Real TLS client transport backed by rustls (RFC 8446 TLS 1.3 + TLS 1.2).
//!
//! `NativeTlsStream` performs a genuine handshake and validates the server
//! certificate chain against the Mozilla root program (`webpki-roots`),
//! including hostname (SAN) and expiry checks. It uses the `ring` crypto
//! provider, which is already locked into the workspace via `ureq` — so no
//! `aws-lc-rs`/NASM toolchain requirement on Windows.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, OnceLock};

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

#[derive(Debug, Clone)]
pub enum ProxyType {
    Direct,
    Http(String),
    Socks5(String),
}

pub struct ProxyResolver {
    pub proxy_type: ProxyType,
}

impl ProxyResolver {
    pub fn direct() -> Self {
        Self { proxy_type: ProxyType::Direct }
    }
}

pub enum TlsState {
    Uninitialized,
    HandshakeInProgress,
    Connected,
    Failed(String),
}

/// Process-wide client config (root store + ring provider), built once.
fn client_config() -> Arc<ClientConfig> {
    static CONFIG: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut roots = RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let config = ClientConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .expect("ring provider supports TLS 1.2/1.3")
            .with_root_certificates(roots)
            .with_no_client_auth();
            Arc::new(config)
        })
        .clone()
}

pub struct NativeTlsStream {
    stream: Option<StreamOwned<ClientConnection, TcpStream>>,
    pub state: TlsState,
    pub hostname: String,
}

impl NativeTlsStream {
    /// Connect to `addr` (`host:port`) and perform a validated TLS handshake
    /// for `hostname`. The handshake completes lazily on first read/write.
    pub fn connect(addr: &str, hostname: &str) -> Result<Self, std::io::Error> {
        // Validate the SNI hostname before spending a socket on it.
        let server_name = ServerName::try_from(hostname.to_string()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid DNS hostname")
        })?;
        let socket = TcpStream::connect(addr)?;
        let conn = ClientConnection::new(client_config(), server_name)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(Self {
            stream: Some(StreamOwned::new(conn, socket)),
            state: TlsState::Connected,
            hostname: hostname.to_string(),
        })
    }

    fn stream_mut(
        &mut self,
    ) -> Result<&mut StreamOwned<ClientConnection, TcpStream>, std::io::Error> {
        self.stream.as_mut().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotConnected, "TLS stream not connected")
        })
    }

    pub fn write_all(&mut self, buf: &[u8]) -> Result<(), std::io::Error> {
        self.stream_mut()?.write_all(buf)
    }

    pub fn flush(&mut self) -> Result<(), std::io::Error> {
        self.stream_mut()?.flush()
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, std::io::Error> {
        self.stream_mut()?.read(buf)
    }

    /// Read until the peer closes the connection. Tolerates a TCP close that
    /// omits TLS `close_notify` (common for HTTP/1.1 `Connection: close`),
    /// treating the bytes already received as the complete response.
    pub fn read_to_end(&mut self, buf: &mut Vec<u8>) -> Result<(), std::io::Error> {
        let stream = self.stream_mut()?;
        let mut chunk = [0u8; 16 * 1024];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_config_builds_and_is_cached() {
        // Exercises the rustls builder + ring provider + webpki-roots load.
        // A panic here would mean the trust store failed to initialize.
        let a = client_config();
        let b = client_config();
        assert!(Arc::ptr_eq(&a, &b), "config should be built once and cached");
    }

    #[test]
    fn rejects_invalid_hostname_without_socket() {
        // Hostname validation happens before the TCP connect, so a malformed
        // SNI name fails fast with InvalidInput and never touches the network.
        let result = NativeTlsStream::connect("127.0.0.1:1", "bad host name");
        match result {
            Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput),
            Ok(_) => panic!("malformed hostname must be rejected"),
        }
    }

    /// End-to-end handshake against a real host. Requires network egress, so it
    /// is ignored by default; run with `cargo test -- --ignored tls_handshake`.
    #[test]
    #[ignore]
    fn tls_handshake_against_real_host() {
        let mut tls = NativeTlsStream::connect("example.com:443", "example.com")
            .expect("TCP connect + client setup");
        tls.write_all(b"GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n")
            .expect("write request over validated TLS channel");
        tls.flush().expect("flush");
        let mut buf = Vec::new();
        tls.read_to_end(&mut buf).expect("read response");
        let head = String::from_utf8_lossy(&buf);
        assert!(head.starts_with("HTTP/1.1 200"), "unexpected status line: {}", &head[..head.len().min(64)]);
    }
}
