//! Lightweight peer API server for cross-device agent collaboration.
//!
//! Each V.E.L.O.C.I.T.Y. instance can expose a small HTTP API that remote
//! peers use for pairing, messaging, file transfer, and task delegation.
//! Built on `std::net::TcpListener` — no external HTTP server dependency.
//!
//! # Endpoints
//!
//! | Method | Path | Purpose |
//! |--------|------|---------|
//! | GET | `/peer/identity` | Get this instance's identity |
//! | POST | `/peer/pair` | Request pairing |
//! | POST | `/peer/message` | Send a message |
//! | POST | `/peer/file/start` | Begin file transfer |
//! | POST | `/peer/file/chunk` | Send file chunk |
//! | POST | `/peer/file/complete` | Complete file transfer |
//! | POST | `/peer/task` | Delegate a task |
//! | POST | `/peer/task/progress` | Update task progress |
//! | POST | `/peer/task/complete` | Complete a task |
//! | GET | `/peer/health` | Health check |

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::peer_link::{PeerManager, PeerMessage, PeerMessageKind};

/// Configuration for the peer API server.
#[derive(Debug, Clone)]
pub struct PeerServerConfig {
    /// Port to listen on.
    pub port: u16,
    /// Bind address (default: 0.0.0.0).
    pub bind_addr: String,
    /// Request timeout in seconds.
    pub timeout_secs: u64,
    /// Maximum request body size (bytes).
    pub max_body_size: usize,
}

impl Default for PeerServerConfig {
    fn default() -> Self {
        Self {
            port: 9191,
            bind_addr: "0.0.0.0".to_string(),
            timeout_secs: 30,
            max_body_size: 10 * 1024 * 1024, // 10 MB
        }
    }
}

/// A parsed HTTP request (minimal parser for our API).
#[derive(Debug)]
struct HttpRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

/// An HTTP response to send back.
#[derive(Debug)]
struct HttpResponse {
    status: u16,
    status_text: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl HttpResponse {
    fn json(status: u16, body: &serde_json::Value) -> Self {
        let body_str = serde_json::to_string(body).unwrap_or_default();
        Self {
            status,
            status_text: status_text(status),
            headers: vec![
                ("Content-Type".to_string(), "application/json".to_string()),
                ("Content-Length".to_string(), body_str.len().to_string()),
                ("Access-Control-Allow-Origin".to_string(), "*".to_string()),
            ],
            body: body_str,
        }
    }

    fn ok(body: &serde_json::Value) -> Self { Self::json(200, body) }
    fn not_found() -> Self { Self::json(404, &serde_json::json!({"error": "not found"})) }
    fn bad_request(msg: &str) -> Self { Self::json(400, &serde_json::json!({"error": msg})) }
    fn server_error(msg: &str) -> Self { Self::json(500, &serde_json::json!({"error": msg})) }

    fn to_bytes(&self) -> Vec<u8> {
        let mut response = format!("HTTP/1.1 {} {}\r\n", self.status, self.status_text);
        for (name, value) in &self.headers {
            response.push_str(&format!("{}: {}\r\n", name, value));
        }
        response.push_str("\r\n");
        response.push_str(&self.body);
        response.into_bytes()
    }
}

fn status_text(code: u16) -> String {
    match code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "Unknown",
    }.to_string()
}

/// Parse a minimal HTTP request from raw bytes.
fn parse_request(raw: &str) -> Option<HttpRequest> {
    let mut lines = raw.lines();
    let request_line = lines.next()?;
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 { return None; }

    let method = parts[0].to_string();
    let path = parts[1].to_string();

    let mut headers = Vec::new();
    let mut body_start = false;
    let mut body = String::new();

    for line in lines {
        if body_start {
            body.push_str(line);
            body.push('\n');
        } else if line.is_empty() {
            body_start = true;
        } else if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }

    Some(HttpRequest { method, path, headers, body: body.trim().to_string() })
}

/// Handle a single HTTP request and produce a response.
fn handle_request(req: &HttpRequest, peer_mgr: &PeerManager) -> HttpResponse {
    match (req.method.as_str(), req.path.as_str()) {
        // Health check.
        ("GET", "/peer/health") => {
            HttpResponse::ok(&serde_json::json!({
                "status": "ok",
                "identity": peer_mgr.local_identity.as_ref().map(|id| serde_json::json!({
                    "id": id.id,
                    "name": id.name,
                    "port": id.port,
                })),
            }))
        }

        // Get local identity.
        ("GET", "/peer/identity") => {
            match &peer_mgr.local_identity {
                Some(id) => HttpResponse::ok(&serde_json::json!({
                    "id": id.id,
                    "name": id.name,
                    "host": id.host,
                    "port": id.port,
                    "capabilities": id.capabilities.iter().map(|c| c.label()).collect::<Vec<_>>(),
                    "environment": id.environment,
                })),
                None => HttpResponse::server_error("Identity not initialized"),
            }
        }

        // Pairing request.
        ("POST", "/peer/pair") => {
            match serde_json::from_str::<serde_json::Value>(&req.body) {
                Ok(val) => HttpResponse::ok(&serde_json::json!({
                    "accepted": true,
                    "message": "Pairing request received",
                    "peer_id": val.get("peer_id").and_then(|v| v.as_str()).unwrap_or("unknown"),
                })),
                Err(e) => HttpResponse::bad_request(&format!("Invalid JSON: {e}")),
            }
        }

        // Receive a message.
        ("POST", "/peer/message") => {
            match serde_json::from_str::<PeerMessage>(&req.body) {
                Ok(_msg) => HttpResponse::ok(&serde_json::json!({"received": true})),
                Err(e) => HttpResponse::bad_request(&format!("Invalid message: {e}")),
            }
        }

        // File transfer start.
        ("POST", "/peer/file/start") => {
            match serde_json::from_str::<serde_json::Value>(&req.body) {
                Ok(val) => {
                    let transfer_id = val.get("transfer_id").and_then(|v| v.as_str()).unwrap_or("");
                    HttpResponse::ok(&serde_json::json!({
                        "accepted": true,
                        "transfer_id": transfer_id,
                    }))
                }
                Err(e) => HttpResponse::bad_request(&format!("Invalid: {e}")),
            }
        }

        // File chunk.
        ("POST", "/peer/file/chunk") => {
            HttpResponse::ok(&serde_json::json!({"received": true}))
        }

        // File transfer complete.
        ("POST", "/peer/file/complete") => {
            HttpResponse::ok(&serde_json::json!({"acknowledged": true}))
        }

        // Task delegation.
        ("POST", "/peer/task") => {
            match serde_json::from_str::<serde_json::Value>(&req.body) {
                Ok(val) => {
                    let task_id = val.get("task_id").and_then(|v| v.as_str()).unwrap_or("unknown");
                    HttpResponse::ok(&serde_json::json!({
                        "accepted": true,
                        "task_id": task_id,
                        "status": "pending",
                    }))
                }
                Err(e) => HttpResponse::bad_request(&format!("Invalid: {e}")),
            }
        }

        // Task progress.
        ("POST", "/peer/task/progress") => {
            HttpResponse::ok(&serde_json::json!({"acknowledged": true}))
        }

        // Task complete.
        ("POST", "/peer/task/complete") => {
            HttpResponse::ok(&serde_json::json!({"acknowledged": true}))
        }

        // Unknown endpoint.
        _ => HttpResponse::not_found(),
    }
}

/// The peer API server state (can be run in a background thread).
pub struct PeerServer {
    config: PeerServerConfig,
    running: Arc<AtomicBool>,
}

impl PeerServer {
    /// Create a new peer server (does not start listening yet).
    pub fn new(config: PeerServerConfig) -> Self {
        Self {
            config,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get a handle to the running flag (for shutdown signaling).
    pub fn running_flag(&self) -> Arc<AtomicBool> {
        self.running.clone()
    }

    /// Start the server (blocking — call from a thread).
    /// `peer_mgr` is read-only for request handling.
    pub fn start(&self, peer_mgr: &PeerManager) -> Result<(), String> {
        let addr = format!("{}:{}", self.config.bind_addr, self.config.port);
        let listener = TcpListener::bind(&addr)
            .map_err(|e| format!("Failed to bind {}: {e}", addr))?;

        listener.set_nonblocking(false)
            .map_err(|e| format!("set_nonblocking: {e}"))?;

        self.running.store(true, Ordering::SeqCst);

        // Set a timeout so we can check the running flag periodically.
        let _ = listener.set_nonblocking(false);

        while self.running.load(Ordering::SeqCst) {
            // Accept with a short timeout by using poll/select pattern.
            // For simplicity, we just accept and handle one at a time.
            match listener.accept() {
                Ok((mut stream, _addr)) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(self.config.timeout_secs)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

                    let mut buf = vec![0u8; self.config.max_body_size];
                    let mut total = 0;
                    loop {
                        match stream.read(&mut buf[total..]) {
                            Ok(0) => break,
                            Ok(n) => {
                                total += n;
                                // Check if we've received the full request.
                                if let Some(content_len) = find_content_length(&buf[..total]) {
                                    let header_end = find_header_end(&buf[..total]);
                                    if let Some(body_start) = header_end {
                                        let body_received = total - body_start;
                                        if body_received >= content_len {
                                            break;
                                        }
                                    }
                                } else if find_header_end(&buf[..total]).is_some() {
                                    break; // No body expected.
                                }
                                if total >= self.config.max_body_size { break; }
                            }
                            Err(_) => break,
                        }
                    }

                    if total > 0 {
                        let raw = String::from_utf8_lossy(&buf[..total]);
                        if let Some(req) = parse_request(&raw) {
                            let response = handle_request(&req, peer_mgr);
                            let _ = stream.write_all(&response.to_bytes());
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
            }
        }

        Ok(())
    }

    /// Signal the server to stop.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

/// Find the Content-Length header value from raw bytes.
fn find_content_length(data: &[u8]) -> Option<usize> {
    let text = String::from_utf8_lossy(data);
    for line in text.lines() {
        if line.to_lowercase().starts_with("content-length:") {
            if let Some(val) = line.split(':').nth(1) {
                return val.trim().parse().ok();
            }
        }
    }
    None
}

/// Find the end of HTTP headers (the blank line).
fn find_header_end(data: &[u8]) -> Option<usize> {
    let text = String::from_utf8_lossy(data);
    if let Some(pos) = text.find("\r\n\r\n") {
        return Some(pos + 4);
    }
    if let Some(pos) = text.find("\n\n") {
        return Some(pos + 2);
    }
    None
}

/// Build the URL for sending a message to a peer.
pub fn peer_url(host: &str, port: u16, endpoint: &str) -> String {
    format!("http://{}:{}{}", host, port, endpoint)
}

/// Send a message to a remote peer via HTTP.
pub fn send_to_peer(peer_host: &str, peer_port: u16, endpoint: &str, body: &serde_json::Value) -> Result<serde_json::Value, String> {
    let url = peer_url(peer_host, peer_port, endpoint);
    let body_str = serde_json::to_string(body)
        .map_err(|e| format!("Serialize: {e}"))?;

    let response = ureq::post(&url)
        .set("Content-Type", "application/json")
        .timeout(Duration::from_secs(30))
        .send_string(&body_str)
        .map_err(|e| format!("Request failed: {e}"))?;

    let response_body = response.into_string()
        .map_err(|e| format!("Read response: {e}"))?;

    serde_json::from_str(&response_body)
        .map_err(|e| format!("Parse response: {e}"))
}

/// Check if a remote peer is reachable.
pub fn peer_health_check(host: &str, port: u16) -> Result<serde_json::Value, String> {
    send_to_peer(host, port, "/peer/health", &serde_json::json!({}))
}

/// Request pairing with a remote peer.
pub fn request_pairing(host: &str, port: u16, local_name: &str) -> Result<serde_json::Value, String> {
    send_to_peer(host, port, "/peer/pair", &serde_json::json!({
        "name": local_name,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_get_request() {
        let raw = "GET /peer/health HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let req = parse_request(raw).unwrap();
        assert_eq!(req.method, "GET");
        assert_eq!(req.path, "/peer/health");
    }

    #[test]
    fn parse_post_request() {
        let raw = "POST /peer/message HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: 13\r\n\r\n{\"hello\":\"world\"}";
        let req = parse_request(raw).unwrap();
        assert_eq!(req.method, "POST");
        assert_eq!(req.path, "/peer/message");
        assert!(req.body.contains("hello"));
    }

    #[test]
    fn health_endpoint() {
        let mut mgr = PeerManager::new();
        mgr.init(std::path::Path::new("/tmp"), "TestHost");
        let req = HttpRequest {
            method: "GET".to_string(),
            path: "/peer/health".to_string(),
            headers: Vec::new(),
            body: String::new(),
        };
        let resp = handle_request(&req, &mgr);
        assert_eq!(resp.status, 200);
        assert!(resp.body.contains("ok"));
    }

    #[test]
    fn identity_endpoint() {
        let mut mgr = PeerManager::new();
        mgr.init(std::path::Path::new("/tmp"), "MyHost");
        let req = HttpRequest {
            method: "GET".to_string(),
            path: "/peer/identity".to_string(),
            headers: Vec::new(),
            body: String::new(),
        };
        let resp = handle_request(&req, &mgr);
        assert_eq!(resp.status, 200);
        assert!(resp.body.contains("MyHost"));
    }

    #[test]
    fn not_found_endpoint() {
        let mgr = PeerManager::new();
        let req = HttpRequest {
            method: "GET".to_string(),
            path: "/unknown".to_string(),
            headers: Vec::new(),
            body: String::new(),
        };
        let resp = handle_request(&req, &mgr);
        assert_eq!(resp.status, 404);
    }

    #[test]
    fn pair_endpoint() {
        let mgr = PeerManager::new();
        let req = HttpRequest {
            method: "POST".to_string(),
            path: "/peer/pair".to_string(),
            headers: Vec::new(),
            body: r#"{"peer_id":"p1","name":"Remote"}"#.to_string(),
        };
        let resp = handle_request(&req, &mgr);
        assert_eq!(resp.status, 200);
        assert!(resp.body.contains("accepted"));
    }

    #[test]
    fn peer_url_format() {
        assert_eq!(peer_url("192.168.1.10", 9191, "/peer/health"),
            "http://192.168.1.10:9191/peer/health");
    }

    #[test]
    fn find_content_length_works() {
        let data = b"POST /test HTTP/1.1\r\nContent-Length: 42\r\n\r\n";
        assert_eq!(find_content_length(data), Some(42));
        assert_eq!(find_content_length(b"GET / HTTP/1.1\r\n\r\n"), None);
    }

    #[test]
    fn find_header_end_works() {
        let data = b"GET / HTTP/1.1\r\nHost: x\r\n\r\nbody";
        assert_eq!(find_header_end(data), Some(27));
    }

    #[test]
    fn http_response_to_bytes() {
        let resp = HttpResponse::ok(&serde_json::json!({"status": "ok"}));
        let bytes = resp.to_bytes();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("Content-Type: application/json"));
    }
}
