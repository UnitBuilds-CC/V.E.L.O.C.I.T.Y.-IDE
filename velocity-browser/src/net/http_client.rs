//! Native HTTP/1.1 client over raw TCP, with TLS 1.3 for `https://` origins.
//!
//! Hardened for real-world responses: follows redirects, decodes
//! `Transfer-Encoding: chunked`, honors `Content-Length`, and inflates
//! `Content-Encoding: gzip`/`deflate` bodies via the from-scratch
//! [`crate::net::inflate`] module. No third-party HTTP/compression crates.
//!
//! `https://` origins are tunneled through [`crate::net::tls::NativeTlsStream`]
//! (rustls, validated against the Mozilla root program).

use std::collections::HashMap;
use std::io::{Read, Write};

use crate::net::inflate;
use crate::net::tls::{NativeTlsStream, ProxyResolver};
use crate::session_cookie_store::{CookieRecord, CookieStore, SameSitePolicy};

const MAX_REDIRECTS: usize = 5;

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status_code: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
    /// All Set-Cookie headers parsed from the response.
    pub set_cookies: Vec<ParsedSetCookie>,
}

/// A fully parsed Set-Cookie header with all attributes.
#[derive(Debug, Clone)]
pub struct ParsedSetCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub max_age: Option<i64>,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: SameSitePolicy,
}

pub struct HttpClient {
    pub cookie_jar: HashMap<String, String>,
    pub cookie_store: CookieStore,
    /// Connection router: direct by default, or through an HTTP/SOCKS5 proxy.
    pub proxy: ProxyResolver,
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClient {
    pub fn new() -> Self {
        Self {
            cookie_jar: HashMap::new(),
            cookie_store: CookieStore::new(),
            proxy: ProxyResolver::direct(),
        }
    }

    /// Route all connections through `resolver` (HTTP CONNECT or SOCKS5).
    pub fn with_proxy(mut self, resolver: ProxyResolver) -> Self {
        self.proxy = resolver;
        self
    }

    /// Perform a GET, following redirects up to [`MAX_REDIRECTS`].
    pub fn get(&mut self, url: &str) -> Result<HttpResponse, Box<dyn std::error::Error + Send + Sync>> {
        let mut current = url.to_string();
        for _ in 0..=MAX_REDIRECTS {
            let (scheme, host, port, path) = parse_url(&current)?;
            let raw = self.request_once(&scheme, &host, port, &path)?;
            let response = self.build_response(&raw, &scheme, &host)?;

            if is_redirect(response.status_code) {
                if let Some(location) = response.headers.get("location") {
                    current = resolve_redirect(&scheme, &host, port, &path, location);
                    continue;
                }
            }
            return Ok(response);
        }
        Err("too many redirects".into())
    }

    /// Build the raw HTTP/1.1 GET request line + headers for `host`/`path`.
    fn build_request(&self, scheme: &str, host: &str, path: &str) -> String {
        let mut req = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: VelocityAgent/1.0\r\nAccept-Encoding: gzip, deflate\r\nConnection: close\r\n",
            path, host
        );
        let cookie_header = self.build_cookie_header(scheme, host, path);
        if !cookie_header.is_empty() {
            req.push_str(&format!("Cookie: {}\r\n", cookie_header));
        }
        req.push_str("\r\n");
        req
    }

    /// Build the Cookie header value from both the legacy jar and the full cookie store.
    fn build_cookie_header(&self, scheme: &str, host: &str, path: &str) -> String {
        let is_secure = scheme == "https";
        let store_cookies = self.cookie_store.get_cookies_for_url(host, path, is_secure);
        let mut pairs: Vec<String> = store_cookies
            .iter()
            .map(|c| format!("{}={}", c.name, c.value))
            .collect();
        // Also include legacy jar entries not already present
        for (k, v) in &self.cookie_jar {
            if !pairs.iter().any(|p| p.starts_with(&format!("{}=", k))) {
                pairs.push(format!("{}={}", k, v));
            }
        }
        pairs.join("; ")
    }

    /// Perform a POST with a URL-encoded body, following redirects (307/308 preserve method).
    pub fn post(
        &mut self,
        url: &str,
        body: &str,
        content_type: &str,
    ) -> Result<HttpResponse, Box<dyn std::error::Error + Send + Sync>> {
        let mut current_url = url.to_string();
        let mut current_body = body.to_string();
        let current_ct = content_type.to_string();
        let mut method_is_post = true;

        for _ in 0..=MAX_REDIRECTS {
            let (scheme, host, port, path) = parse_url(&current_url)?;
            let buffer = if method_is_post {
                let req = self.build_post_request(&scheme, &host, &path, &current_body, &current_ct);
                self.send_raw_request(&scheme, &host, port, &req)?
            } else {
                self.request_once(&scheme, &host, port, &path)?
            };
            let response = self.build_response(&buffer, &scheme, &host)?;

            if is_redirect(response.status_code) {
                if let Some(location) = response.headers.get("location") {
                    current_url = resolve_redirect(&scheme, &host, port, &path, location);
                    // 307/308 preserve the original method; 301/302/303 switch to GET
                    if response.status_code == 307 || response.status_code == 308 {
                        // Keep POST with same body
                    } else {
                        method_is_post = false;
                        current_body.clear();
                    }
                    continue;
                }
            }
            return Ok(response);
        }
        Err("too many redirects".into())
    }

    fn build_post_request(&self, scheme: &str, host: &str, path: &str, body: &str, content_type: &str) -> String {
        let mut req = format!(
            "POST {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: VelocityAgent/1.0\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccept-Encoding: gzip, deflate\r\nConnection: close\r\n",
            path, host, content_type, body.len()
        );
        let cookie_header = self.build_cookie_header(scheme, host, path);
        if !cookie_header.is_empty() {
            req.push_str(&format!("Cookie: {}\r\n", cookie_header));
        }
        req.push_str("\r\n");
        req.push_str(body);
        req
    }

    /// Send a raw request string over the appropriate transport.
    fn send_raw_request(
        &self,
        scheme: &str,
        host: &str,
        port: u16,
        request: &str,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let mut buffer = Vec::new();
        if scheme == "https" {
            let mut tls = NativeTlsStream::connect_via(&self.proxy, host, port)?;
            tls.write_all(request.as_bytes())?;
            tls.flush()?;
            tls.read_to_end(&mut buffer)?;
        } else {
            let mut stream = self.proxy.connect_tcp(host, port)?;
            stream.write_all(request.as_bytes())?;
            stream.flush()?;
            stream.read_to_end(&mut buffer)?;
        }
        Ok(buffer)
    }

    fn request_once(
        &self,
        scheme: &str,
        host: &str,
        port: u16,
        path: &str,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let req = self.build_request(scheme, host, path);
        self.send_raw_request(scheme, host, port, &req)
    }

    /// Parse headers, capture cookies, then dechunk and decompress the body.
    fn build_response(
        &mut self,
        raw: &[u8],
        scheme: &str,
        host: &str,
    ) -> Result<HttpResponse, Box<dyn std::error::Error + Send + Sync>> {
        let (head, body) = split_head_body(raw).ok_or("malformed HTTP response")?;
        let (status_code, headers, raw_set_cookies) = parse_status_and_headers(head);

        // Parse and store cookies with full attributes
        let mut parsed_cookies = Vec::new();
        let is_secure_origin = scheme == "https";
        for raw_cookie in &raw_set_cookies {
            let parsed = parse_set_cookie_full(raw_cookie, host);
            // Skip secure cookies received over plain HTTP
            if parsed.secure && !is_secure_origin {
                continue;
            }
            // Store in legacy jar for backward compat
            self.cookie_jar.insert(parsed.name.clone(), parsed.value.clone());
            // Store in full cookie store
            self.cookie_store.set_cookie(CookieRecord {
                name: parsed.name.clone(),
                value: parsed.value.clone(),
                domain: parsed.domain.clone(),
                path: parsed.path.clone(),
                expires_timestamp: parsed.max_age.map(|a| a as f64).unwrap_or(0.0),
                samesite: parsed.same_site.clone(),
                secure: parsed.secure,
                http_only: parsed.http_only,
            });
            parsed_cookies.push(parsed);
        }

        // Transfer-Encoding: chunked takes precedence over Content-Length.
        let decoded_body = if headers
            .get("transfer-encoding")
            .map(|v| v.to_ascii_lowercase().contains("chunked"))
            .unwrap_or(false)
        {
            dechunk(body)?
        } else if let Some(len) = headers.get("content-length").and_then(|v| v.trim().parse::<usize>().ok()) {
            body.iter().take(len).copied().collect()
        } else {
            body.to_vec()
        };

        let encoding = headers.get("content-encoding").map(|s| s.as_str()).unwrap_or("");
        let final_bytes = inflate::decode_content_encoding(encoding, &decoded_body)
            .unwrap_or_else(|_| decoded_body.clone());

        Ok(HttpResponse {
            status_code,
            headers,
            body: String::from_utf8_lossy(&final_bytes).to_string(),
            set_cookies: parsed_cookies,
        })
    }
}

/// Split `scheme://host:port/path` into components. Missing pieces default to
/// `http`, port 80 (or 443 for https), and `/`.
fn parse_url(url: &str) -> Result<(String, String, u16, String), &'static str> {
    let s = url.trim();
    let (scheme, rest) = if let Some(r) = s.strip_prefix("http://") {
        ("http", r)
    } else if let Some(r) = s.strip_prefix("https://") {
        ("https", r)
    } else {
        ("http", s)
    };

    let (host_port, path) = match rest.split_once('/') {
        Some((hp, p)) => (hp, format!("/{}", p)),
        None => (rest, "/".to_string()),
    };

    let (host, port) = match host_port.split_once(':') {
        Some((h, p)) => (h.to_string(), p.parse::<u16>().unwrap_or(80)),
        None => (
            host_port.to_string(),
            if scheme == "https" { 443 } else { 80 },
        ),
    };

    Ok((scheme.to_string(), host, port, path))
}

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

/// Find the `\r\n\r\n` boundary and split headers from the body.
fn split_head_body(raw: &[u8]) -> Option<(&[u8], &[u8])> {
    let sep = b"\r\n\r\n";
    raw.windows(4)
        .position(|w| w == sep)
        .map(|i| (&raw[..i], &raw[i + 4..]))
}

/// Parse the status line and headers. Returns (status, headers, raw_set_cookie_values).
/// Header names are lowercased; the last value wins for duplicates except
/// `Set-Cookie`, which is collected separately as full raw values.
fn parse_status_and_headers(head: &[u8]) -> (u16, HashMap<String, String>, Vec<String>) {
    let text = String::from_utf8_lossy(head);
    let mut lines = text.lines();
    let status_line = lines.next().unwrap_or("");
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);

    let mut headers = HashMap::new();
    let mut cookies = Vec::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim().to_ascii_lowercase();
            let val = v.trim().to_string();
            if key == "set-cookie" {
                cookies.push(val.clone());
            }
            headers.insert(key, val);
        }
    }
    (status_code, headers, cookies)
}

/// Parse a full Set-Cookie header value into a structured record.
/// Example: "sid=abc123; Domain=.example.com; Path=/; Secure; HttpOnly; SameSite=Lax"
fn parse_set_cookie_full(raw: &str, request_host: &str) -> ParsedSetCookie {
    let parts: Vec<&str> = raw.split(';').collect();
    let (name, value) = if let Some((n, v)) = parts[0].split_once('=') {
        (n.trim().to_string(), v.trim().to_string())
    } else {
        (parts[0].trim().to_string(), String::new())
    };

    let mut domain = request_host.to_string();
    let mut path = "/".to_string();
    let mut max_age = None;
    let mut secure = false;
    let mut http_only = false;
    let mut same_site = SameSitePolicy::Lax;

    for part in parts.iter().skip(1) {
        let attr = part.trim();
        let attr_lower = attr.to_ascii_lowercase();
        if let Some(d) = attr_lower.strip_prefix("domain=") {
            domain = d.trim_start_matches('.').to_string();
        } else if let Some(p) = attr_lower.strip_prefix("path=") {
            path = p.to_string();
        } else if let Some(ma) = attr_lower.strip_prefix("max-age=") {
            max_age = ma.parse::<i64>().ok();
        } else if attr_lower == "secure" {
            secure = true;
        } else if attr_lower == "httponly" {
            http_only = true;
        } else if let Some(ss) = attr_lower.strip_prefix("samesite=") {
            same_site = match ss.trim() {
                "strict" => SameSitePolicy::Strict,
                "none" => SameSitePolicy::None,
                _ => SameSitePolicy::Lax,
            };
        }
    }

    ParsedSetCookie {
        name,
        value,
        domain,
        path,
        max_age,
        secure,
        http_only,
        same_site,
    }
}

/// Decode a `Transfer-Encoding: chunked` body.
fn dechunk(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        let line_end = find_subslice(&data[i..], b"\r\n")
            .map(|p| p + i)
            .ok_or("chunk size line not terminated")?;
        let size_line = String::from_utf8_lossy(&data[i..line_end]);
        let size_hex = size_line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16).map_err(|_| "invalid chunk size")?;
        i = line_end + 2;
        if size == 0 {
            break;
        }
        if i + size > data.len() {
            return Err("chunk exceeds available data".to_string());
        }
        out.extend_from_slice(&data[i..i + size]);
        i += size;
        if data[i..].starts_with(b"\r\n") {
            i += 2;
        }
    }
    Ok(out)
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Resolve a redirect `Location` against the current request.
fn resolve_redirect(scheme: &str, host: &str, port: u16, base_path: &str, location: &str) -> String {
    let loc = location.trim();
    if loc.starts_with("http://") || loc.starts_with("https://") {
        return loc.to_string();
    }
    let authority = if (scheme == "http" && port == 80) || (scheme == "https" && port == 443) {
        host.to_string()
    } else {
        format!("{}:{}", host, port)
    };
    if loc.starts_with('/') {
        format!("{}://{}{}", scheme, authority, loc)
    } else {
        // Relative to the current path's directory.
        let dir = match base_path.rfind('/') {
            Some(idx) => &base_path[..=idx],
            None => "/",
        };
        format!("{}://{}{}{}", scheme, authority, dir, loc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_url_components() {
        let (scheme, host, port, path) = parse_url("http://example.com/a/b?x=1").unwrap();
        assert_eq!((scheme.as_str(), host.as_str(), port, path.as_str()), ("http", "example.com", 80, "/a/b?x=1"));
        let (s2, h2, p2, _) = parse_url("https://site.test:8443/").unwrap();
        assert_eq!((s2.as_str(), h2.as_str(), p2), ("https", "site.test", 8443));
    }

    #[test]
    fn splits_and_parses_headers_with_cookies() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nSet-Cookie: sid=abc; Path=/\r\n\r\nbody-here";
        let (head, body) = split_head_body(raw).unwrap();
        assert_eq!(body, b"body-here");
        let (status, headers, cookies) = parse_status_and_headers(head);
        assert_eq!(status, 200);
        assert_eq!(headers.get("content-type").map(|s| s.as_str()), Some("text/html"));
        assert_eq!(cookies.len(), 1);
        assert!(cookies[0].contains("sid=abc"));
    }

    #[test]
    fn dechunks_body() {
        // "Wiki" + "pedia" in two chunks, then a zero terminator.
        let data = b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n";
        assert_eq!(dechunk(data).unwrap(), b"Wikipedia");
    }

    #[test]
    fn resolves_redirect_targets() {
        assert_eq!(
            resolve_redirect("http", "a.com", 80, "/x", "https://b.com/y"),
            "https://b.com/y"
        );
        assert_eq!(
            resolve_redirect("http", "a.com", 80, "/x/y", "/z"),
            "http://a.com/z"
        );
        assert_eq!(
            resolve_redirect("http", "a.com", 8080, "/dir/page", "next"),
            "http://a.com:8080/dir/next"
        );
    }

    #[test]
    fn build_response_decodes_chunked() {
        let mut client = HttpClient::new();
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n0\r\n\r\n";
        let resp = client.build_response(raw, "http", "localhost").unwrap();
        assert_eq!(resp.status_code, 200);
        assert_eq!(resp.body, "Wiki");
    }

    #[test]
    fn parse_set_cookie_full_attributes() {
        let parsed = parse_set_cookie_full(
            "session=xyz123; Domain=.example.com; Path=/app; Secure; HttpOnly; SameSite=Strict; Max-Age=3600",
            "example.com"
        );
        assert_eq!(parsed.name, "session");
        assert_eq!(parsed.value, "xyz123");
        assert_eq!(parsed.domain, "example.com");
        assert_eq!(parsed.path, "/app");
        assert!(parsed.secure);
        assert!(parsed.http_only);
        assert_eq!(parsed.same_site, SameSitePolicy::Strict);
        assert_eq!(parsed.max_age, Some(3600));
    }

    #[test]
    fn cookie_store_integration() {
        let mut client = HttpClient::new();
        let raw = b"HTTP/1.1 200 OK\r\nSet-Cookie: token=abc; Domain=example.com; Path=/; Secure\r\n\r\nOK";
        let resp = client.build_response(raw, "https", "example.com").unwrap();
        assert_eq!(resp.set_cookies.len(), 1);
        assert_eq!(resp.set_cookies[0].name, "token");
        // Cookie should now be in the store
        let matching = client.cookie_store.get_cookies_for_url("example.com", "/", true);
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].value, "abc");
    }

    /// Full HTTPS fetch (TLS handshake + cert validation + response decode)
    /// against a real origin. Needs network egress, so it is ignored by
    /// default; run with `cargo test -- --ignored https_get`.
    #[test]
    #[ignore]
    fn https_get_end_to_end() {
        let mut client = HttpClient::new();
        let resp = client.get("https://example.com/").expect("https GET should succeed");
        assert_eq!(resp.status_code, 200);
        assert!(resp.body.to_ascii_lowercase().contains("<!doctype html") || !resp.body.is_empty());
    }

    #[test]
    fn parse_url_defaults() {
        // No scheme → http
        let (s, h, p, pa) = parse_url("example.com/path").unwrap();
        assert_eq!(s, "http");
        assert_eq!(h, "example.com");
        assert_eq!(p, 80);
        assert_eq!(pa, "/path");

        // HTTPS with no path
        let (s2, _, p2, pa2) = parse_url("https://secure.com").unwrap();
        assert_eq!(s2, "https");
        assert_eq!(p2, 443);
        assert_eq!(pa2, "/");
    }

    #[test]
    fn dechunk_empty_chunks() {
        let data = b"0\r\n\r\n";
        let result = dechunk(data).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_set_cookie_minimal() {
        let parsed = parse_set_cookie_full("simple=value", "example.com");
        assert_eq!(parsed.name, "simple");
        assert_eq!(parsed.value, "value");
        assert_eq!(parsed.domain, "example.com");
        assert_eq!(parsed.path, "/");
        assert!(!parsed.secure);
        assert!(!parsed.http_only);
        assert_eq!(parsed.same_site, SameSitePolicy::Lax);
        assert!(parsed.max_age.is_none());
    }

    #[test]
    fn secure_cookie_rejected_over_http() {
        let mut client = HttpClient::new();
        let raw = b"HTTP/1.1 200 OK\r\nSet-Cookie: token=abc; Secure\r\n\r\nOK";
        let resp = client.build_response(raw, "http", "example.com").unwrap();
        // Secure cookie should be skipped over plain HTTP
        assert!(resp.set_cookies.is_empty());
        assert!(!client.cookie_jar.contains_key("token"));
    }

    #[test]
    fn parse_url_custom_port() {
        let (s, h, p, pa) = parse_url("http://localhost:3000/api/v1").unwrap();
        assert_eq!(s, "http");
        assert_eq!(h, "localhost");
        assert_eq!(p, 3000);
        assert_eq!(pa, "/api/v1");
    }

    #[test]
    fn is_redirect_variants() {
        assert!(is_redirect(301));
        assert!(is_redirect(302));
        assert!(is_redirect(303));
        assert!(is_redirect(307));
        assert!(is_redirect(308));
        assert!(!is_redirect(200));
        assert!(!is_redirect(404));
        assert!(!is_redirect(500));
    }

    #[test]
    fn dechunk_with_extension() {
        // Chunk with extension parameter (should be ignored per spec)
        let data = b"5;ext=val\r\nHello\r\n0\r\n\r\n";
        assert_eq!(dechunk(data).unwrap(), b"Hello");
    }

    #[test]
    fn parse_set_cookie_samesite_none() {
        let parsed = parse_set_cookie_full("id=x; SameSite=None", "example.com");
        assert_eq!(parsed.same_site, SameSitePolicy::None);
    }

    #[test]
    fn parse_set_cookie_samesite_lax() {
        let parsed = parse_set_cookie_full("id=x; SameSite=Lax", "example.com");
        assert_eq!(parsed.same_site, SameSitePolicy::Lax);
    }

    #[test]
    fn split_head_body_no_separator() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: text/html";
        assert!(split_head_body(raw).is_none());
    }

    #[test]
    fn parse_status_and_headers_malformed_status() {
        let head = b"GARBAGE LINE\r\nContent-Type: text/html";
        let (status, headers, _) = parse_status_and_headers(head);
        assert_eq!(status, 0); // unparseable status
        assert_eq!(headers.get("content-type").map(|s| s.as_str()), Some("text/html"));
    }

    #[test]
    fn resolve_redirect_relative_same_dir() {
        let url = resolve_redirect("http", "a.com", 80, "/dir/page.html", "other.html");
        assert_eq!(url, "http://a.com/dir/other.html");
    }

    #[test]
    fn http_client_default() {
        let client = HttpClient::default();
        assert!(client.cookie_jar.is_empty());
        assert!(matches!(client.proxy.proxy_type, crate::net::tls::ProxyType::Direct));
    }

    #[test]
    fn build_response_content_length_truncation() {
        let mut client = HttpClient::new();
        // Content-Length says 5 bytes, but body is longer
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nHelloExtraData";
        let resp = client.build_response(raw, "http", "localhost").unwrap();
        assert_eq!(resp.body, "Hello");
    }
}
