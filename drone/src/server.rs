//! HTTP server for the Velocity Drone.
//!
//! Built on `std::net::TcpListener` — no external HTTP server dependency.
//! Implements all endpoints from DRONE_PROTOCOL.md.

use std::io::{BufRead, BufReader, Read as IoRead, Write as IoWrite};
use std::net::TcpListener;
use std::sync::Arc;

use crate::core::DroneCore;

/// A minimal parsed HTTP request.
struct HttpRequest {
    method: String,
    path: String,
    body: Vec<u8>,
    /// Value of the `Authorization` header, if present.
    auth_header: Option<String>,
}

/// Maximum request body size (16 MB). Prevents memory exhaustion from
/// malicious or misconfigured clients sending huge Content-Length values.
const MAX_BODY_SIZE: usize = 16 * 1024 * 1024;

/// Parse a raw HTTP request from a reader.
fn parse_request(reader: &mut BufReader<&mut dyn IoRead>) -> Option<HttpRequest> {
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).ok()? == 0 {
        return None;
    }

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }

    let method = parts[0].to_string();
    let path = parts[1].to_string();

    // Read headers.
    let mut content_length: usize = 0;
    let mut auth_header: Option<String> = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        let lower = trimmed.to_lowercase();
        if let Some(val) = lower.strip_prefix("content-length:") {
            content_length = val.trim().parse().unwrap_or(0);
            if content_length > MAX_BODY_SIZE {
                return None; // Reject oversized requests early.
            }
        } else if lower.starts_with("authorization:") {
            // Take the value from the original line, not `lower`: a bearer
            // token is case-significant, and the old code handed the
            // lower-cased remainder downstream, where the exact-prefix
            // strip matched nothing and every authenticated call 401'd.
            auth_header = Some(trimmed["authorization:".len()..].trim().to_string());
        }
    }

    // Read body.
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body).ok()?;
    }

    Some(HttpRequest {
        method,
        path,
        body,
        auth_header,
    })
}

/// Write an HTTP response.
fn write_response(stream: &mut dyn IoWrite, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Unknown",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).ok();
    stream.flush().ok();
}

/// Constant-time string comparison to prevent timing attacks on auth tokens.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    if a_bytes.len() != b_bytes.len() {
        // Still do a dummy comparison to avoid leaking length via timing.
        let _ = a_bytes
            .iter()
            .zip(b_bytes.iter())
            .fold(0u8, |acc, (&x, &y)| acc | (x ^ y));
        return false;
    }
    a_bytes
        .iter()
        .zip(b_bytes.iter())
        .fold(0u8, |acc, (&x, &y)| acc | (x ^ y))
        == 0
}

/// Route a request to the appropriate handler.
/// If `required_token` is `Some`, all endpoints except `/peer/health` require
/// a matching `Authorization: Bearer <token>` header.
fn route_request(
    core: &DroneCore,
    req: &HttpRequest,
    required_token: Option<&str>,
) -> (u16, String) {
    // ── Auth gate (health is always public) ──
    if let Some(token) = required_token {
        if req.path != "/peer/health" {
            let authorized = req
                .auth_header
                .as_deref()
                .map(|h| {
                    // RFC 7235: the scheme name is case-insensitive; the
                    // secret that follows it is not.
                    let (scheme, secret) = h.split_once(' ').unwrap_or((h, ""));
                    scheme.eq_ignore_ascii_case("bearer") && constant_time_eq(secret.trim(), token)
                })
                .unwrap_or(false);
            if !authorized {
                return (
                    401,
                    r#"{"error":"Unauthorized — missing or invalid Bearer token"}"#.into(),
                );
            }
        }
    }

    match (req.method.as_str(), req.path.as_str()) {
        // ── GET endpoints ──
        ("GET", "/peer/health") => {
            let uptime = crate::core::now_secs() - core.identity.start_time;
            (
                200,
                serde_json::json!({
                    "status": "ok",
                    "id": core.identity.id,
                    "name": core.identity.name,
                    "version": env!("CARGO_PKG_VERSION"),
                    "environment": core.identity.environment,
                    "uptime_secs": uptime,
                    "capabilities": core.identity.capabilities,
                })
                .to_string(),
            )
        }

        ("GET", "/peer/identity") => (200, core.identity.to_json().to_string()),

        ("GET", path) if path.starts_with("/peer/task/") && path.ends_with("/status") => {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 4 {
                let task_id = parts[3];
                let (status, json) = core.handle_task_status(task_id);
                (status, json.to_string())
            } else {
                (400, r#"{"error":"Invalid task path"}"#.into())
            }
        }

        // ── POST endpoints ──
        ("POST", "/peer/pair") => match serde_json::from_slice::<serde_json::Value>(&req.body) {
            Ok(data) => {
                let peer_id = data["peer_id"].as_str().unwrap_or("");
                let name = data["name"].as_str().unwrap_or("unknown");
                let result = core.handle_pair(peer_id, name);
                (200, result.to_string())
            }
            Err(e) => (400, format!(r#"{{"error":"Invalid JSON: {e}"}}"#)),
        },

        ("POST", "/peer/message") => match serde_json::from_slice::<serde_json::Value>(&req.body) {
            Ok(data) => {
                let result = core.handle_message(data);
                (200, result.to_string())
            }
            Err(e) => (400, format!(r#"{{"error":"Invalid JSON: {e}"}}"#)),
        },

        ("POST", "/peer/file/start") => {
            match serde_json::from_slice::<serde_json::Value>(&req.body) {
                Ok(data) => {
                    let result = core.handle_file_start(&data);
                    (200, result.to_string())
                }
                Err(e) => (400, format!(r#"{{"error":"Invalid JSON: {e}"}}"#)),
            }
        }

        ("POST", "/peer/file/chunk") => {
            match serde_json::from_slice::<serde_json::Value>(&req.body) {
                Ok(data) => {
                    let result = core.handle_file_chunk(&data);
                    (200, result.to_string())
                }
                Err(e) => (400, format!(r#"{{"error":"Invalid JSON: {e}"}}"#)),
            }
        }

        ("POST", "/peer/file/complete") => {
            match serde_json::from_slice::<serde_json::Value>(&req.body) {
                Ok(data) => {
                    let result = core.handle_file_complete(&data);
                    (200, result.to_string())
                }
                Err(e) => (400, format!(r#"{{"error":"Invalid JSON: {e}"}}"#)),
            }
        }

        ("POST", "/peer/task") => match serde_json::from_slice::<serde_json::Value>(&req.body) {
            Ok(data) => {
                let result = core.handle_task(&data);
                (200, result.to_string())
            }
            Err(e) => (400, format!(r#"{{"error":"Invalid JSON: {e}"}}"#)),
        },

        // Directory listing for the IDE's remote work-dir picker. POST (not
        // GET-with-query) so paths with spaces, `+`, `%` and non-ASCII need
        // no URL-encoding dance on either side of the wire.
        ("POST", "/peer/fs/list") => {
            let data: serde_json::Value =
                serde_json::from_slice(&req.body).unwrap_or(serde_json::Value::Null);
            let path = data["path"].as_str().unwrap_or("");
            let (status, body) = core.handle_fs_list(path);
            (status, body.to_string())
        }

        // ── Fallback ──
        _ => (404, r#"{"error":"Not found"}"#.into()),
    }
}

/// The drone HTTP server.
pub struct DroneServer {
    core: Arc<DroneCore>,
    host: String,
    addr: String,
    /// Shared-secret token for authenticating requests.  When `None`, auth is
    /// disabled (development mode).  Set via `DRONE_AUTH_TOKEN` env var.
    auth_token: Option<String>,
    /// Operator's explicit permission to run an unauthenticated drone on a
    /// reachably-off-box interface. Set via `DRONE_ALLOW_UNAUTHENTICATED_REMOTE=1`.
    allow_unauthenticated_remote: bool,
}

/// True when binding to `host` cannot be reached from another machine: the
/// loopback range (`127.0.0.0/8`, `::1`) or its name. Anything else — including
/// `0.0.0.0`, `::`, and a machine name that resolves outward — exposes the
/// endpoint beyond this host's own user.
fn is_loopback_only(host: &str) -> bool {
    let host = host.trim();
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => false,
    }
}

/// The reason a drone refuses to start, or `None` when it may serve.
///
/// `/peer/task` hands its command string straight to `cmd /C` (or `sh -c`), so an
/// unauthenticated drone reachable off-box is a remote shell for whoever can
/// open a socket to it. A token makes that safe; a loopback bind makes it
/// unreachable to anyone but this machine's own user; an explicit operator
/// override is the only other door.
fn startup_refusal(host: &str, token: Option<&str>, override_granted: bool) -> Option<String> {
    if token.is_some() || is_loopback_only(host) || override_granted {
        return None;
    }
    Some(format!(
        "refusing to bind {host} without DRONE_AUTH_TOKEN: /peer/task executes arbitrary \
         commands, so an off-loopback drone without a shared secret is a remote shell. Set \
         DRONE_AUTH_TOKEN, bind 127.0.0.1, or set DRONE_ALLOW_UNAUTHENTICATED_REMOTE=1 if this \
         machine's network is trusted."
    ))
}

impl DroneServer {
    pub fn new(core: DroneCore, host: &str, port: u16) -> Self {
        let auth_token = std::env::var("DRONE_AUTH_TOKEN")
            .ok()
            .filter(|t| !t.is_empty());
        Self {
            core: Arc::new(core),
            host: host.to_string(),
            addr: format!("{host}:{port}"),
            allow_unauthenticated_remote: std::env::var("DRONE_ALLOW_UNAUTHENTICATED_REMOTE")
                .ok()
                .filter(|v| v == "1")
                .is_some(),
            auth_token,
        }
    }

    /// Start serving (blocking).
    pub fn serve(&self) -> Result<(), String> {
        if let Some(reason) = startup_refusal(
            &self.host,
            self.auth_token.as_deref(),
            self.allow_unauthenticated_remote,
        ) {
            return Err(reason);
        }

        let listener = TcpListener::bind(&self.addr).map_err(|e| format!("Bind: {e}"))?;

        // Save identity.
        self.core.identity.save(&self.core.workspace).ok();

        println!(
            "Velocity Drone '{}' listening on {}",
            self.core.identity.name, self.addr
        );
        println!("  ID: {}", self.core.identity.id);
        println!("  Environment: {}", self.core.identity.environment);
        println!("  Workspace: {}", self.core.workspace.display());
        println!(
            "  Capabilities: {}",
            self.core.identity.capabilities.join(", ")
        );
        if self.auth_token.is_some() {
            println!("  Auth:         Shared-secret token enabled");
        } else if is_loopback_only(&self.host) {
            println!(
                "  Auth:         DISABLED — bound to loopback, so only this machine can reach it"
            );
        } else {
            println!(
                "  Auth:         DISABLED by operator override — every endpoint is a remote shell"
            );
        }
        println!("Press Ctrl+C to stop.");

        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    let core = Arc::clone(&self.core);
                    let auth_token = self.auth_token.clone();
                    std::thread::Builder::new()
                        .name("drone-req".into())
                        .spawn(move || {
                            let mut reader = BufReader::new(&mut stream as &mut dyn IoRead);
                            if let Some(req) = parse_request(&mut reader) {
                                let (status, body) =
                                    route_request(&core, &req, auth_token.as_deref());
                                write_response(&mut stream as &mut dyn IoWrite, status, &body);
                            }
                        })
                        .ok();
                }
                Err(e) => {
                    eprintln!("Accept error: {e}");
                }
            }
        }

        Ok(())
    }

    /// Get a reference to the core (for testing).
    pub fn core(&self) -> &Arc<DroneCore> {
        &self.core
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpStream;

    fn start_test_server() -> (DroneServer, u16) {
        let ws = std::env::temp_dir().join(format!("drone_srv_test_{}", crate::core::now_secs()));
        std::fs::create_dir_all(&ws).unwrap();
        let identity = crate::core::DroneIdentity::new("SrvTest", 0);
        let core = DroneCore::new(identity, ws);

        // Find a free port.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let server = DroneServer::new(core, "127.0.0.1", port);
        (server, port)
    }

    fn http_get(port: u16, path: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).unwrap();
        stream.flush().unwrap();

        let mut response = String::new();
        let mut reader = BufReader::new(&mut stream as &mut dyn IoRead);
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => response.push_str(&line),
                Err(_) => break,
            }
        }

        // Parse status code.
        let status = response
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0);

        // Extract body (after \r\n\r\n).
        let body = response.split("\r\n\r\n").nth(1).unwrap_or("").to_string();

        (status, body)
    }

    fn http_post(port: u16, path: &str, body: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).unwrap();
        stream.flush().unwrap();

        let mut response = String::new();
        let mut reader = BufReader::new(&mut stream as &mut dyn IoRead);
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => response.push_str(&line),
                Err(_) => break,
            }
        }

        let status = response
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0);

        let body = response.split("\r\n\r\n").nth(1).unwrap_or("").to_string();

        (status, body)
    }

    #[test]
    fn route_health() {
        let ws = std::env::temp_dir().join(format!("route_test_{}", crate::core::now_secs()));
        std::fs::create_dir_all(&ws).unwrap();
        let identity = crate::core::DroneIdentity::new("RouteTest", 9191);
        let core = DroneCore::new(identity, ws);

        let req = HttpRequest {
            method: "GET".into(),
            path: "/peer/health".into(),
            body: vec![],
            auth_header: None,
        };
        let (status, body) = route_request(&core, &req, None);
        assert_eq!(status, 200);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["status"], "ok");
        assert_eq!(json["name"], "RouteTest");
    }

    #[test]
    fn route_identity() {
        let ws = std::env::temp_dir().join(format!("route_test2_{}", crate::core::now_secs()));
        std::fs::create_dir_all(&ws).unwrap();
        let identity = crate::core::DroneIdentity::new("IdTest", 9191);
        let core = DroneCore::new(identity, ws);

        let req = HttpRequest {
            method: "GET".into(),
            path: "/peer/identity".into(),
            body: vec![],
            auth_header: None,
        };
        let (status, body) = route_request(&core, &req, None);
        assert_eq!(status, 200);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["name"], "IdTest");
        assert!(json["online"].as_bool().unwrap());
    }

    #[test]
    fn route_pair() {
        let ws = std::env::temp_dir().join(format!("route_test3_{}", crate::core::now_secs()));
        std::fs::create_dir_all(&ws).unwrap();
        let identity = crate::core::DroneIdentity::new("PairTest", 9191);
        let core = DroneCore::new(identity, ws);

        let req = HttpRequest {
            method: "POST".into(),
            path: "/peer/pair".into(),
            body: br#"{"peer_id":"p1","name":"Test"}"#.to_vec(),
            auth_header: None,
        };
        let (status, body) = route_request(&core, &req, None);
        assert_eq!(status, 200);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(json["accepted"].as_bool().unwrap());
    }

    #[test]
    fn route_not_found() {
        let ws = std::env::temp_dir().join(format!("route_test4_{}", crate::core::now_secs()));
        std::fs::create_dir_all(&ws).unwrap();
        let identity = crate::core::DroneIdentity::new("NFTest", 9191);
        let core = DroneCore::new(identity, ws);

        let req = HttpRequest {
            method: "GET".into(),
            path: "/nonexistent".into(),
            body: vec![],
            auth_header: None,
        };
        let (status, _) = route_request(&core, &req, None);
        assert_eq!(status, 404);
    }

    #[test]
    fn route_task_status_unknown() {
        let ws = std::env::temp_dir().join(format!("route_test5_{}", crate::core::now_secs()));
        std::fs::create_dir_all(&ws).unwrap();
        let identity = crate::core::DroneIdentity::new("TaskTest", 9191);
        let core = DroneCore::new(identity, ws);

        let req = HttpRequest {
            method: "GET".into(),
            path: "/peer/task/nonexistent/status".into(),
            body: vec![],
            auth_header: None,
        };
        let (status, body) = route_request(&core, &req, None);
        assert_eq!(status, 404);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(json["error"].as_str().unwrap().contains("Unknown"));
    }

    #[test]
    fn parse_request_get() {
        let raw = b"GET /peer/health HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let mut cursor = std::io::Cursor::new(raw.as_slice());
        let mut reader = BufReader::new(&mut cursor as &mut dyn IoRead);
        let req = parse_request(&mut reader).unwrap();
        assert_eq!(req.method, "GET");
        assert_eq!(req.path, "/peer/health");
        assert!(req.body.is_empty());
    }

    /// Regression: the parser used to hand the *lower-cased* Authorization
    /// value to the router, whose exact `"Bearer "` strip then matched
    /// nothing - every authenticated drone call answered 401, which is what
    /// a live buildbox deploy surfaced. Parsing preserves the value's case;
    /// the scheme match is case-insensitive, the secret is not.
    #[test]
    fn bearer_header_case_survives_parsing_and_authenticates() {
        let raw = b"GET /peer/identity HTTP/1.1\r\nHost: x\r\nAuthorization: BeArEr MiXeDcAsEsEcReT\r\n\r\n";
        let mut cursor = std::io::Cursor::new(raw.as_slice());
        let mut reader = BufReader::new(&mut cursor as &mut dyn IoRead);
        let req = parse_request(&mut reader).unwrap();
        assert_eq!(
            req.auth_header.as_deref(),
            Some("BeArEr MiXeDcAsEsEcReT"),
            "header value must keep its original case"
        );

        let ws = std::env::temp_dir().join(format!("auth_case_{}", crate::core::now_secs()));
        std::fs::create_dir_all(&ws).unwrap();
        let identity = crate::core::DroneIdentity::new("AuthCase", 9191);
        let core = DroneCore::new(identity, ws);
        let (status, _) = route_request(&core, &req, Some("MiXeDcAsEsEcReT"));
        assert_eq!(
            status, 200,
            "mixed-case scheme with the exact secret passes"
        );
        let (status, _) = route_request(&core, &req, Some("wrong-secret"));
        assert_eq!(status, 401, "a wrong secret is still refused");
    }

    #[test]
    fn parse_request_post_with_body() {
        let body = r#"{"key":"value"}"#;
        let raw = format!(
            "POST /peer/pair HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let mut cursor = std::io::Cursor::new(raw.into_bytes());
        let mut reader = BufReader::new(&mut cursor as &mut dyn IoRead);
        let req = parse_request(&mut reader).unwrap();
        assert_eq!(req.method, "POST");
        assert_eq!(req.path, "/peer/pair");
        assert_eq!(req.body, body.as_bytes());
    }

    // ── Integration tests using the HTTP helpers ──

    #[test]
    fn integration_health_endpoint() {
        let (server, port) = start_test_server();
        let _serve_handle = std::thread::spawn(move || {
            let _ = server.serve();
        });
        std::thread::sleep(std::time::Duration::from_millis(100));

        let (status, body) = http_get(port, "/peer/health");
        assert_eq!(status, 200);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["status"], "ok");
    }

    #[test]
    fn integration_identity_endpoint() {
        let (server, port) = start_test_server();
        let _serve_handle = std::thread::spawn(move || {
            let _ = server.serve();
        });
        std::thread::sleep(std::time::Duration::from_millis(100));

        let (status, body) = http_get(port, "/peer/identity");
        assert_eq!(status, 200);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(json["name"].as_str().is_some());
    }

    #[test]
    fn integration_pair_post_endpoint() {
        let (server, port) = start_test_server();
        let _serve_handle = std::thread::spawn(move || {
            let _ = server.serve();
        });
        std::thread::sleep(std::time::Duration::from_millis(100));

        let (status, body) = http_post(
            port,
            "/peer/pair",
            r#"{"peer_id":"int_p1","name":"IntTest"}"#,
        );
        assert_eq!(status, 200);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(json["accepted"].as_bool().unwrap());
    }

    #[test]
    fn integration_not_found_endpoint() {
        let (server, port) = start_test_server();
        let _serve_handle = std::thread::spawn(move || {
            let _ = server.serve();
        });
        std::thread::sleep(std::time::Duration::from_millis(100));

        let (status, _) = http_get(port, "/nonexistent/path");
        assert_eq!(status, 404);
    }

    /// Multiple threads hitting the server concurrently all get valid responses.
    #[test]
    fn integration_concurrent_requests() {
        let (server, port) = start_test_server();
        let _serve_handle = std::thread::spawn(move || {
            let _ = server.serve();
        });
        std::thread::sleep(std::time::Duration::from_millis(100));

        let mut handles = Vec::new();
        for _ in 0..8 {
            let h = std::thread::spawn(move || {
                let (status, body) = http_get(port, "/peer/health");
                assert_eq!(status, 200);
                let json: serde_json::Value = serde_json::from_str(&body).unwrap();
                assert_eq!(json["status"], "ok");
            });
            handles.push(h);
        }
        for h in handles {
            h.join().expect("concurrent request thread panicked");
        }
    }

    /// Rapid connect-disconnect cycles should not crash the server.
    #[test]
    fn integration_rapid_connect_disconnect() {
        let (server, port) = start_test_server();
        let _serve_handle = std::thread::spawn(move || {
            let _ = server.serve();
        });
        std::thread::sleep(std::time::Duration::from_millis(100));

        for _ in 0..20 {
            // Connect then immediately drop (close) the stream.
            let stream = TcpStream::connect(format!("127.0.0.1:{port}"));
            assert!(stream.is_ok());
            // Stream dropped here.
        }

        // Server should still respond after all the rapid disconnects.
        let (status, body) = http_get(port, "/peer/health");
        assert_eq!(status, 200);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["status"], "ok");
    }

    // ── Auth tests ──

    #[test]
    fn route_auth_required_without_token() {
        let ws = std::env::temp_dir().join(format!("route_auth1_{}", crate::core::now_secs()));
        std::fs::create_dir_all(&ws).unwrap();
        let identity = crate::core::DroneIdentity::new("AuthTest", 9191);
        let core = DroneCore::new(identity, ws);

        let req = HttpRequest {
            method: "GET".into(),
            path: "/peer/identity".into(),
            body: vec![],
            auth_header: None,
        };
        // With a required token, identity should be 401 without auth.
        let (status, body) = route_request(&core, &req, Some("secret123"));
        assert_eq!(status, 401);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(json["error"].as_str().unwrap().contains("Unauthorized"));
    }

    #[test]
    fn route_auth_health_always_public() {
        let ws = std::env::temp_dir().join(format!("route_auth2_{}", crate::core::now_secs()));
        std::fs::create_dir_all(&ws).unwrap();
        let identity = crate::core::DroneIdentity::new("AuthPub", 9191);
        let core = DroneCore::new(identity, ws);

        let req = HttpRequest {
            method: "GET".into(),
            path: "/peer/health".into(),
            body: vec![],
            auth_header: None,
        };
        // Health should work even with auth required and no token.
        let (status, _) = route_request(&core, &req, Some("secret123"));
        assert_eq!(status, 200);
    }

    #[test]
    fn route_auth_valid_token() {
        let ws = std::env::temp_dir().join(format!("route_auth3_{}", crate::core::now_secs()));
        std::fs::create_dir_all(&ws).unwrap();
        let identity = crate::core::DroneIdentity::new("AuthOk", 9191);
        let core = DroneCore::new(identity, ws);

        let req = HttpRequest {
            method: "GET".into(),
            path: "/peer/identity".into(),
            body: vec![],
            auth_header: Some("Bearer secret123".into()),
        };
        let (status, _) = route_request(&core, &req, Some("secret123"));
        assert_eq!(status, 200);
    }

    #[test]
    fn route_auth_wrong_token() {
        let ws = std::env::temp_dir().join(format!("route_auth4_{}", crate::core::now_secs()));
        std::fs::create_dir_all(&ws).unwrap();
        let identity = crate::core::DroneIdentity::new("AuthBad", 9191);
        let core = DroneCore::new(identity, ws);

        let req = HttpRequest {
            method: "GET".into(),
            path: "/peer/identity".into(),
            body: vec![],
            auth_header: Some("Bearer wrong_token".into()),
        };
        let (status, _) = route_request(&core, &req, Some("secret123"));
        assert_eq!(status, 401);
    }

    // ── Command allowlist tests ──

    fn test_ws() -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("drone_deploy_test_{}", crate::core::now_secs()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn deploy_blocked_command() {
        let ws = test_ws();
        let output = crate::core::execute_deploy_instructions("run rm -rf /", "/tmp/f.txt", &ws);
        assert!(output.contains("BLOCKED"));
    }

    #[test]
    fn deploy_allowed_echo() {
        let ws = test_ws();
        let output = crate::core::execute_deploy_instructions("run echo hello", "/tmp/f.txt", &ws);
        assert!(output.contains("[run] echo hello"));
        assert!(!output.contains("BLOCKED"));
    }

    // ── Bind-policy tests ──
    //
    // `/peer/task` spawns `cmd /C` (Windows) or `sh -c` (elsewhere) with whatever
    // command string the caller sent, so a drone that answers it without a shared
    // secret is a remote shell for anyone who can open a socket to it. These tests
    // pin the three doors: a token, a loopback bind, or an explicit override.

    #[test]
    fn only_loopback_spelling_counts_as_unreachable_off_box() {
        for host in ["127.0.0.1", "127.5.5.5", "localhost", "LOCALHOST", "::1"] {
            assert!(
                is_loopback_only(host),
                "{host} is reachable from this machine only"
            );
        }
        for host in [
            "0.0.0.0",
            "::",
            "192.168.1.10",
            "10.0.0.5",
            "buildbox.local",
            "",
        ] {
            assert!(
                !is_loopback_only(host),
                "{host} can be reached from another machine"
            );
        }
    }

    #[test]
    fn off_loopback_bind_without_a_token_is_refused() {
        let reason =
            startup_refusal("0.0.0.0", None, false).expect("0.0.0.0 with no token must refuse");
        assert!(
            reason.contains("refusing to bind 0.0.0.0 without DRONE_AUTH_TOKEN"),
            "the refusal names the interface and the missing variable: {reason}"
        );
        // ...and tells the operator all three ways out, so the message is actionable.
        assert!(reason.contains("DRONE_AUTH_TOKEN"));
        assert!(reason.contains("127.0.0.1"));
        assert!(reason.contains("DRONE_ALLOW_UNAUTHENTICATED_REMOTE"));
    }

    #[test]
    fn a_token_a_loopback_bind_or_an_override_allows_starting() {
        assert_eq!(startup_refusal("0.0.0.0", Some("s3cret"), false), None);
        assert_eq!(startup_refusal("0.0.0.0", None, true), None);
        assert_eq!(startup_refusal("127.0.0.1", None, false), None);
        assert_eq!(startup_refusal("::1", None, false), None);
    }

    /// Regression: the server used to bind and serve regardless of the bind
    /// address. The refusal has to happen *before* the listener opens, which is
    /// why this asks for an address this machine cannot own — if the guard were
    /// removed the bind would fail with a socket error, not the policy message.
    #[test]
    fn serve_refuses_an_unauthenticated_remote_bind_before_opening_a_socket() {
        let ws = test_ws();
        let core = DroneCore::new(crate::core::DroneIdentity::new("BindPolicy", 0), ws);
        let server = DroneServer {
            core: Arc::new(core),
            host: "192.0.2.1".to_string(),
            addr: "192.0.2.1:9191".to_string(),
            auth_token: None,
            allow_unauthenticated_remote: false,
        };

        let err = server.serve().expect_err("must not serve");
        assert!(
            err.starts_with("refusing to bind"),
            "expected the bind-policy refusal, got a socket error: {err}"
        );
    }

    #[test]
    fn serve_proceeds_when_a_remote_bind_carries_a_token() {
        let ws = test_ws();
        let core = DroneCore::new(crate::core::DroneIdentity::new("BindOk", 0), ws);
        let server = DroneServer {
            core: Arc::new(core),
            host: "192.0.2.1".to_string(),
            addr: "192.0.2.1:9191".to_string(),
            auth_token: Some("s3cret".to_string()),
            allow_unauthenticated_remote: false,
        };

        // Past the guard, so the only failure left is the impossible bind — which
        // proves the policy let a tokened drone through instead of refusing it.
        let err = server
            .serve()
            .expect_err("this address is not ours to bind");
        assert!(
            err.starts_with("Bind:"),
            "a tokened drone must reach the bind, got: {err}"
        );
    }
}
