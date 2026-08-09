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
}

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
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(val) = trimmed.to_lowercase().strip_prefix("content-length:") {
            content_length = val.trim().parse().unwrap_or(0);
        }
    }

    // Read body.
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body).ok()?;
    }

    Some(HttpRequest { method, path, body })
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

/// Route a request to the appropriate handler.
fn route_request(core: &DroneCore, req: &HttpRequest) -> (u16, String) {
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

        // ── Fallback ──
        _ => (404, r#"{"error":"Not found"}"#.into()),
    }
}

/// The drone HTTP server.
pub struct DroneServer {
    core: Arc<DroneCore>,
    addr: String,
}

impl DroneServer {
    pub fn new(core: DroneCore, host: &str, port: u16) -> Self {
        Self {
            core: Arc::new(core),
            addr: format!("{host}:{port}"),
        }
    }

    /// Start serving (blocking).
    pub fn serve(&self) -> Result<(), String> {
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
        println!("Press Ctrl+C to stop.");

        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    let core = Arc::clone(&self.core);
                    std::thread::Builder::new()
                        .name("drone-req".into())
                        .spawn(move || {
                            let mut reader = BufReader::new(&mut stream as &mut dyn IoRead);
                            if let Some(req) = parse_request(&mut reader) {
                                let (status, body) = route_request(&core, &req);
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
        };
        let (status, body) = route_request(&core, &req);
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
        };
        let (status, body) = route_request(&core, &req);
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
        };
        let (status, body) = route_request(&core, &req);
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
        };
        let (status, _) = route_request(&core, &req);
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
        };
        let (status, body) = route_request(&core, &req);
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
}
