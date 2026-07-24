use crate::registry::call_tool_in_workspace;
use serde_json::json;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

fn read_http_request(stream: &mut TcpStream) -> String {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
    let mut data = Vec::new();
    let mut buf = [0u8; 1024];
    let mut expected_total = None;

    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(read) => {
                data.extend_from_slice(&buf[..read]);
                if expected_total.is_none() {
                    if let Some(header_end) =
                        data.windows(4).position(|window| window == b"\r\n\r\n")
                    {
                        let headers_end = header_end + 4;
                        let headers = String::from_utf8_lossy(&data[..headers_end]);
                        let content_length = headers
                            .lines()
                            .find_map(|line| {
                                let lower = line.to_ascii_lowercase();
                                lower
                                    .strip_prefix("content-length:")
                                    .and_then(|value| value.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        expected_total = Some(headers_end + content_length);
                    }
                }
                if let Some(total) = expected_total {
                    if data.len() >= total {
                        break;
                    }
                }
            }
            Err(_) => break,
        }
    }

    String::from_utf8_lossy(&data).to_string()
}

#[test]
fn browser_session_wait_protocol_round_trip() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{}/", port);
    let response_url = url.clone();

    std::thread::spawn(move || {
        let mut idx = 0;
        loop {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = read_http_request(&mut stream);
                let body = "<html><head><title>Dashboard</title></head><body><p>Ready</p></body></html>";
                let response = if idx == 0 {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\nX-Velocity-Protocol-Events: event_stream|open|{0}events|connected;websocket|frame|{0}ws|hello\r\nContent-Length: {1}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{2}",
                        response_url,
                        body.len(),
                        body
                    )
                };
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
                idx += 1;
            } else {
                break;
            }
        }
    });

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();

    call_tool_in_workspace(
        &root,
        "browser_create_session",
        &json!({"sessionId": "protocol-waiter"}),
    )
    .unwrap();
    call_tool_in_workspace(
        &root,
        "browser_session_navigate",
        &json!({"sessionId": "protocol-waiter", "url": url}),
    )
    .unwrap();

    let waited = call_tool_in_workspace(
        &root,
        "browser_session_wait",
        &json!({"sessionId": "protocol-waiter", "protocolKind": "event_stream", "protocolPhase": "open", "protocolTarget": "events", "protocolDetail": "connected", "timeoutMs": 1500, "intervalMs": 10}),
    )
    .unwrap();
    assert!(waited.contains("Session wait complete."));
    assert!(waited.contains("Protocol events: 2"));
    assert!(waited.contains("Diff: protocol+2"));
}

#[test]
fn browser_session_wait_storage_round_trip() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{}", port);

    std::thread::spawn(move || {
        let mut idx = 0;
        loop {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = read_http_request(&mut stream);
                let body = "<html><head><title>Dashboard</title></head><body><p>Ready</p></body></html>";
                let response = if idx == 0 {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\nX-Velocity-Session-Storage: csrf=token123\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                };
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
                idx += 1;
            } else {
                break;
            }
        }
    });

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();

    call_tool_in_workspace(
        &root,
        "browser_create_session",
        &json!({"sessionId": "storage-waiter"}),
    )
    .unwrap();
    call_tool_in_workspace(
        &root,
        "browser_session_navigate",
        &json!({"sessionId": "storage-waiter", "url": url}),
    )
    .unwrap();

    let waited = call_tool_in_workspace(
        &root,
        "browser_session_wait",
        &json!({"sessionId": "storage-waiter", "storageScope": "session", "storageKey": "csrf", "storageValue": "token", "timeoutMs": 1500, "intervalMs": 10}),
    )
    .unwrap();
    assert!(waited.contains("Session wait complete."));
    assert!(waited.contains("Session storage: 1"));
    assert!(waited.contains("Diff: storage+1"));
}

#[test]
fn browser_session_wait_stream_complete_round_trip() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{}/", port);
    let response_url = url.clone();

    std::thread::spawn(move || {
        let mut idx = 0;
        loop {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = read_http_request(&mut stream);
                let body = "<html><head><title>Dashboard</title></head><body><p>Ready</p></body></html>";
                let response = if idx == 0 {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\nX-Velocity-Protocol-Events: event_stream|open|{0}events|connected;event_stream|complete|{0}events|stream complete\r\nContent-Length: {1}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{2}",
                        response_url,
                        body.len(),
                        body
                    )
                };
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
                idx += 1;
            } else {
                break;
            }
        }
    });

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();

    call_tool_in_workspace(
        &root,
        "browser_create_session",
        &json!({"sessionId": "stream-waiter"}),
    )
    .unwrap();
    call_tool_in_workspace(
        &root,
        "browser_session_navigate",
        &json!({"sessionId": "stream-waiter", "url": url}),
    )
    .unwrap();

    let waited = call_tool_in_workspace(
        &root,
        "browser_session_wait",
        &json!({"sessionId": "stream-waiter", "streamComplete": true, "timeoutMs": 1500, "intervalMs": 10}),
    )
    .unwrap();
    assert!(waited.contains("Protocol events: 2"));
    assert!(waited.contains("Diff: protocol+2"));
}

#[test]
fn browser_session_wait_title_and_stable_round_trip() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{}/", port);

    std::thread::spawn(move || {
        let mut idx = 0;
        while let Ok((mut stream, _)) = listener.accept() {
            let _ = read_http_request(&mut stream);
            let body = match idx {
                0 => "<html><head><title>Loading</title></head><body><p>Preparing</p></body></html>",
                1 => "<html><head><title>Dashboard Ready</title></head><body><p>Preparing</p></body></html>",
                _ => "<html><head><title>Dashboard Ready</title></head><body><p>Stable</p></body></html>",
            };
            let response = if idx >= 1 {
                format!(
                    "HTTP/1.1 200 OK\r\nX-Velocity-Mutations: route:dashboard;hydration:complete\r\nX-Velocity-Settle: response:complete;navigation:settled;network:settled\r\nX-Velocity-Runtime-State: router:name=dashboard;store:panel=ready\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
            } else {
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
            };
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
            idx += 1;
        }
    });

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();

    call_tool_in_workspace(
        &root,
        "browser_create_session",
        &json!({"sessionId": "steady"}),
    )
    .unwrap();
    call_tool_in_workspace(
        &root,
        "browser_session_navigate",
        &json!({"sessionId": "steady", "url": url}),
    )
    .unwrap();

    let title_wait = call_tool_in_workspace(
        &root,
        "browser_session_wait",
        &json!({"sessionId": "steady", "title": "Dashboard", "timeoutMs": 1500, "intervalMs": 10}),
    )
    .unwrap();
    assert!(title_wait.contains("Title: Dashboard Ready"));

    let mutation_wait = call_tool_in_workspace(
        &root,
        "browser_session_wait",
        &json!({"sessionId": "steady", "mutation": "hydration", "timeoutMs": 1500, "intervalMs": 10}),
    )
    .unwrap();
    assert!(mutation_wait.contains("Diff: no_semantic_change"));

    let runtime_wait = call_tool_in_workspace(
        &root,
        "browser_session_wait",
        &json!({"sessionId": "steady", "runtimeScope": "router", "runtimeKey": "name", "runtimeValue": "dashboard", "timeoutMs": 1500, "intervalMs": 10}),
    )
    .unwrap();
    assert!(runtime_wait.contains("Runtime state: 2"));

    let settle_wait = call_tool_in_workspace(
        &root,
        "browser_session_wait",
        &json!({"sessionId": "steady", "settleScope": "network", "settleState": "settled", "timeoutMs": 1500, "intervalMs": 10}),
    )
    .unwrap();
    assert!(settle_wait.contains("Settle signals: 3"));

    let stable_wait = call_tool_in_workspace(
        &root,
        "browser_session_wait",
        &json!({"sessionId": "steady", "stablePolls": 2, "timeoutMs": 1500, "intervalMs": 10}),
    )
    .unwrap();
    assert!(stable_wait.contains("Title: Dashboard Ready"));

    let network_idle_wait = call_tool_in_workspace(
        &root,
        "browser_session_wait",
        &json!({"sessionId": "steady", "networkIdle": true, "timeoutMs": 1500, "intervalMs": 10}),
    )
    .unwrap();
    assert!(network_idle_wait.contains("Settle signals: 3"));

    let app_ready_wait = call_tool_in_workspace(
        &root,
        "browser_session_wait",
        &json!({"sessionId": "steady", "appReady": true, "timeoutMs": 1500, "intervalMs": 10}),
    )
    .unwrap();
    assert!(app_ready_wait.contains("Runtime state: 2"));

    let mutation_settled_wait = call_tool_in_workspace(
        &root,
        "browser_session_wait",
        &json!({"sessionId": "steady", "mutationSettled": true, "timeoutMs": 1500, "intervalMs": 10}),
    )
    .unwrap();
    assert!(mutation_settled_wait.contains("Settle signals: 3"));
}

#[test]
fn test_web_navigate_native_parser() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{}/", port);

    std::thread::spawn(move || {
        loop {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);

                let body = "<html><head><title>Egui Test</title></head><body><a href=\"/button\">Click Me</a></body></html>";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            } else {
                break;
            }
        }
    });

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().to_path_buf();
    let sitemap_path = root.join(".velocity").join("site_map");

    let res = crate::editor::browser::crawl_and_sync_sitemap(&url, &sitemap_path).unwrap();
    assert!(res.contains("Egui Test"));
    assert!(res.contains("Interactive Elements: 1"));

    let compact =
        call_tool_in_workspace(&root, "web_navigate", &json!({"url": url, "compact": true}))
            .unwrap();
    assert!(compact.contains("\"snapshot\":"));
    assert!(compact.contains("\"title\": \"Egui Test\""));
    assert!(compact.contains("\"element_count\": 1"));

    let snapshots =
        call_tool_in_workspace(&root, "browser_list_snapshots", &json!({})).unwrap();
    assert!(snapshots.contains(&url));
    assert!(snapshots.contains("Egui Test"));
}
