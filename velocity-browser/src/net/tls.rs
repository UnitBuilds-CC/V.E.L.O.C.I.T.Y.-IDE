//! Real TLS client transport backed by rustls (RFC 8446 TLS 1.3 + TLS 1.2).
//!
//! `NativeTlsStream` performs a genuine handshake and validates the server
//! certificate chain against the Mozilla root program (`webpki-roots`),
//! including hostname (SAN) and expiry checks. It uses the `ring` crypto
//! provider, which is already locked into the workspace via `ureq` â€” so no
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

    /// Route connections through an HTTP proxy at `addr` (`host:port`) using
    /// the CONNECT method to establish a raw tunnel to the target.
    pub fn http(addr: impl Into<String>) -> Self {
        Self { proxy_type: ProxyType::Http(addr.into()) }
    }

    /// Route connections through a SOCKS5 proxy at `addr` (`host:port`).
    pub fn socks5(addr: impl Into<String>) -> Self {
        Self { proxy_type: ProxyType::Socks5(addr.into()) }
    }

    /// Establish a raw TCP stream to `target_host:target_port`, tunneling
    /// through the configured proxy when one is set. For [`ProxyType::Direct`]
    /// this is a plain `TcpStream::connect`.
    pub fn connect_tcp(
        &self,
        target_host: &str,
        target_port: u16,
    ) -> Result<TcpStream, std::io::Error> {
        match &self.proxy_type {
            ProxyType::Direct => {
                TcpStream::connect(format!("{target_host}:{target_port}"))
            }
            ProxyType::Http(proxy_addr) => {
                let mut stream = TcpStream::connect(proxy_addr)?;
                let request = build_http_connect_request(target_host, target_port);
                stream.write_all(request.as_bytes())?;
                stream.flush()?;
                let head = read_http_headers(&mut stream)?;
                parse_http_connect_status(&head)
                    .map_err(std::io::Error::other)?;
                Ok(stream)
            }
            ProxyType::Socks5(proxy_addr) => {
                let mut stream = TcpStream::connect(proxy_addr)?;
                // Greeting: offer only the "no authentication" method.
                stream.write_all(&socks5_greeting())?;
                stream.flush()?;
                let mut selection = [0u8; 2];
                stream.read_exact(&mut selection)?;
                validate_socks5_method_selection(&selection)
                    .map_err(std::io::Error::other)?;
                // CONNECT request to the target host/port.
                let request = build_socks5_connect_request(target_host, target_port)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
                stream.write_all(&request)?;
                stream.flush()?;
                let reply = read_socks5_reply(&mut stream)?;
                parse_socks5_reply(&reply)
                    .map_err(std::io::Error::other)?;
                Ok(stream)
            }
        }
    }
}

/// Build the HTTP `CONNECT` request line + headers for a proxy tunnel.
fn build_http_connect_request(host: &str, port: u16) -> String {
    format!(
        "CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\nProxy-Connection: keep-alive\r\n\r\n"
    )
}

/// Validate that an HTTP proxy accepted the CONNECT tunnel (2xx status).
fn parse_http_connect_status(head: &str) -> Result<(), String> {
    let status_line = head.lines().next().ok_or("empty CONNECT response")?;
    // Expected form: "HTTP/1.1 200 Connection established"
    let code = status_line
        .split_whitespace()
        .nth(1)
        .ok_or("malformed CONNECT status line")?;
    let code: u16 = code.parse().map_err(|_| "non-numeric CONNECT status")?;
    if (200..300).contains(&code) {
        Ok(())
    } else {
        Err(format!("proxy refused CONNECT: status {code}"))
    }
}

/// Read raw bytes from `stream` until the HTTP header terminator (`\r\n\r\n`).
fn read_http_headers(stream: &mut TcpStream) -> Result<String, std::io::Error> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = stream.read(&mut byte)?;
        if n == 0 {
            break;
        }
        buf.push(byte[0]);
        if buf.ends_with(b"\r\n\r\n") {
            break;
        }
        if buf.len() > 8192 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "CONNECT response headers too large",
            ));
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// SOCKS5 client greeting offering only the "no authentication" method (0x00).
fn socks5_greeting() -> Vec<u8> {
    vec![0x05, 0x01, 0x00]
}

/// Validate the server's SOCKS5 method-selection reply (`[0x05, 0x00]`).
fn validate_socks5_method_selection(reply: &[u8]) -> Result<(), String> {
    if reply.len() != 2 {
        return Err("short SOCKS5 method selection".to_string());
    }
    if reply[0] != 0x05 {
        return Err(format!("unexpected SOCKS version {:#x}", reply[0]));
    }
    match reply[1] {
        0x00 => Ok(()),
        0xFF => Err("SOCKS5 proxy rejected all offered auth methods".to_string()),
        other => Err(format!("SOCKS5 proxy selected unsupported method {other:#x}")),
    }
}

/// Build a SOCKS5 CONNECT request using a domain-name address (ATYP 0x03).
fn build_socks5_connect_request(host: &str, port: u16) -> Result<Vec<u8>, String> {
    let host_bytes = host.as_bytes();
    if host_bytes.len() > 255 {
        return Err("SOCKS5 hostname exceeds 255 bytes".to_string());
    }
    let mut req = Vec::with_capacity(host_bytes.len() + 7);
    req.push(0x05); // version
    req.push(0x01); // CMD = CONNECT
    req.push(0x00); // reserved
    req.push(0x03); // ATYP = domain name
    req.push(host_bytes.len() as u8);
    req.extend_from_slice(host_bytes);
    req.extend_from_slice(&port.to_be_bytes());
    Ok(req)
}

/// Read a SOCKS5 CONNECT reply, including the variable-length bound address.
fn read_socks5_reply(stream: &mut TcpStream) -> Result<Vec<u8>, std::io::Error> {
    let mut header = [0u8; 4];
    stream.read_exact(&mut header)?;
    let atyp = header[3];
    let addr_len = match atyp {
        0x01 => 4,  // IPv4
        0x04 => 16, // IPv6
        0x03 => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len)?;
            len[0] as usize
        }
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unknown SOCKS5 address type in reply",
            ))
        }
    };
    // Consume the bound address + 2-byte port so the stream is left clean.
    let mut rest = vec![0u8; addr_len + 2];
    stream.read_exact(&mut rest)?;
    let mut reply = header.to_vec();
    if atyp == 0x03 {
        reply.push(addr_len as u8);
    }
    reply.extend_from_slice(&rest);
    Ok(reply)
}

/// Validate that a SOCKS5 CONNECT reply reports success (REP == 0x00).
fn parse_socks5_reply(reply: &[u8]) -> Result<(), String> {
    if reply.len() < 2 {
        return Err("short SOCKS5 reply".to_string());
    }
    if reply[0] != 0x05 {
        return Err(format!("unexpected SOCKS version {:#x}", reply[0]));
    }
    match reply[1] {
        0x00 => Ok(()),
        0x01 => Err("SOCKS5: general server failure".to_string()),
        0x02 => Err("SOCKS5: connection not allowed by ruleset".to_string()),
        0x03 => Err("SOCKS5: network unreachable".to_string()),
        0x04 => Err("SOCKS5: host unreachable".to_string()),
        0x05 => Err("SOCKS5: connection refused".to_string()),
        0x06 => Err("SOCKS5: TTL expired".to_string()),
        0x07 => Err("SOCKS5: command not supported".to_string()),
        0x08 => Err("SOCKS5: address type not supported".to_string()),
        other => Err(format!("SOCKS5: unknown reply code {other:#x}")),
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

    /// Connect to `hostname:port` through `resolver` (direct or proxied) and
    /// perform a validated TLS handshake for `hostname`. When a proxy is set,
    /// the TLS session runs end-to-end inside the proxy tunnel.
    pub fn connect_via(
        resolver: &ProxyResolver,
        hostname: &str,
        port: u16,
    ) -> Result<Self, std::io::Error> {
        let server_name = ServerName::try_from(hostname.to_string()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid DNS hostname")
        })?;
        let socket = resolver.connect_tcp(hostname, port)?;
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
    fn http_connect_request_is_well_formed() {
        let req = build_http_connect_request("example.com", 443);
        assert!(req.starts_with("CONNECT example.com:443 HTTP/1.1\r\n"));
        assert!(req.contains("Host: example.com:443\r\n"));
        assert!(req.ends_with("\r\n\r\n"));
    }

    #[test]
    fn http_connect_status_accepts_2xx_rejects_others() {
        assert!(parse_http_connect_status("HTTP/1.1 200 Connection established\r\n").is_ok());
        assert!(parse_http_connect_status("HTTP/1.1 407 Proxy Authentication Required\r\n").is_err());
        assert!(parse_http_connect_status("HTTP/1.1 502 Bad Gateway\r\n").is_err());
        assert!(parse_http_connect_status("garbage").is_err());
    }

    #[test]
    fn socks5_greeting_offers_no_auth_only() {
        assert_eq!(socks5_greeting(), vec![0x05, 0x01, 0x00]);
    }

    #[test]
    fn socks5_method_selection_validates_no_auth() {
        assert!(validate_socks5_method_selection(&[0x05, 0x00]).is_ok());
        assert!(validate_socks5_method_selection(&[0x05, 0xFF]).is_err());
        assert!(validate_socks5_method_selection(&[0x05, 0x02]).is_err());
        assert!(validate_socks5_method_selection(&[0x04, 0x00]).is_err());
        assert!(validate_socks5_method_selection(&[0x05]).is_err());
    }

    #[test]
    fn socks5_connect_request_encodes_domain_and_port() {
        let req = build_socks5_connect_request("example.com", 443).unwrap();
        // version, cmd, rsv, atyp(domain), len
        assert_eq!(&req[..5], &[0x05, 0x01, 0x00, 0x03, 11]);
        assert_eq!(&req[5..16], b"example.com");
        // port 443 = 0x01BB, big-endian
        assert_eq!(&req[16..], &[0x01, 0xBB]);
    }

    #[test]
    fn socks5_connect_request_rejects_overlong_host() {
        let long = "a".repeat(256);
        assert!(build_socks5_connect_request(&long, 80).is_err());
    }

    #[test]
    fn socks5_reply_maps_reply_codes() {
        assert!(parse_socks5_reply(&[0x05, 0x00, 0x00, 0x01]).is_ok());
        assert!(parse_socks5_reply(&[0x05, 0x01, 0x00, 0x01]).is_err()); // general failure
        assert!(parse_socks5_reply(&[0x05, 0x05, 0x00, 0x01]).is_err()); // refused
        assert!(parse_socks5_reply(&[0x04, 0x00]).is_err()); // wrong version
        assert!(parse_socks5_reply(&[0x05]).is_err()); // short
    }

    #[test]
    fn direct_resolver_connects_without_proxy() {
        // A Direct resolver should attempt a plain TCP connect; connecting to a
        // closed local port fails fast without any proxy handshake.
        let resolver = ProxyResolver::direct();
        let result = resolver.connect_tcp("127.0.0.1", 1);
        assert!(result.is_err(), "connecting to a dead port should fail");
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

