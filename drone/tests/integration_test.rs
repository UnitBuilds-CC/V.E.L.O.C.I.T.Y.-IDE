//! Integration tests: full IDE ↔ Drone workflow scenarios.
//!
//! Tests the complete collaboration pipeline: discovery, pairing,
//! file transfer, task delegation, and multi-drone coordination.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Read as IoRead, Write as IoWrite};
use std::net::TcpStream;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use velocity_drone::core::{DroneCore, DroneIdentity};
use velocity_drone::server::DroneServer;

/// Start a drone server on a free port, return (core_arc, port).
fn start_drone(name: &str) -> (Arc<DroneCore>, u16) {
    let ws = std::env::temp_dir().join(format!(
        "drone_integ_{}_{name}",
        velocity_drone::core::now_secs()
    ));
    std::fs::create_dir_all(&ws).unwrap();
    let identity = DroneIdentity::new(name, 0);
    let core = DroneCore::new(identity, ws);

    // Find a free port.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let server = DroneServer::new(core, "127.0.0.1", port);
    let core_ref = server.core().clone();

    thread::Builder::new()
        .name(format!("drone-{name}"))
        .spawn(move || {
            server.serve().ok();
        })
        .unwrap();

    thread::sleep(Duration::from_millis(100));
    (core_ref, port)
}

fn http_get(port: u16, path: &str) -> (u16, serde_json::Value) {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).unwrap();
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

    let body = response.split("\r\n\r\n").nth(1).unwrap_or("");
    let json: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    (status, json)
}

fn http_post(port: u16, path: &str, data: &serde_json::Value) -> (u16, serde_json::Value) {
    let body = data.to_string();
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).unwrap();
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

    let resp_body = response.split("\r\n\r\n").nth(1).unwrap_or("");
    let json: serde_json::Value = serde_json::from_str(resp_body).unwrap_or_default();
    (status, json)
}

#[test]
fn e2e_full_collaboration_workflow() {
    // Simulates: IDE discovers two drones, pairs, builds on drone 1,
    // deploys artifact to drone 2, runs tests on drone 2.
    let (build_core, build_port) = start_drone("BuildMachine");
    let (test_core, test_port) = start_drone("TestMachine");

    // Step 1: Discover drones.
    let (_, health1) = http_get(build_port, "/peer/health");
    let (_, health2) = http_get(test_port, "/peer/health");
    assert_eq!(health1["status"], "ok");
    assert_eq!(health2["status"], "ok");
    assert_eq!(health1["name"], "BuildMachine");
    assert_eq!(health2["name"], "TestMachine");

    // Step 2: Pair with both.
    let (_, pair1) = http_post(
        build_port,
        "/peer/pair",
        &serde_json::json!({"peer_id": "ide_dev_1", "name": "Developer IDE"}),
    );
    let (_, pair2) = http_post(
        test_port,
        "/peer/pair",
        &serde_json::json!({"peer_id": "ide_dev_1", "name": "Developer IDE"}),
    );
    assert!(pair1["accepted"].as_bool().unwrap());
    assert!(pair2["accepted"].as_bool().unwrap());

    // Step 3: Build task on drone 1.
    let (_, build_task) = http_post(
        build_port,
        "/peer/task",
        &serde_json::json!({
            "task_id": "build_001",
            "prompt": "Build the project",
            "instructions": "echo BUILD_SUCCESS",
        }),
    );
    assert!(build_task["accepted"].as_bool().unwrap());

    // Wait for build.
    let mut build_status = serde_json::json!({});
    for _ in 0..20 {
        thread::sleep(Duration::from_millis(200));
        let (_, s) = http_get(build_port, "/peer/task/build_001/status");
        build_status = s;
        if build_status["status"] == "completed" || build_status["status"] == "failed" {
            break;
        }
    }
    assert_eq!(build_status["status"], "completed");
    assert!(build_status["result"]["stdout"]
        .as_str()
        .unwrap()
        .contains("BUILD_SUCCESS"));

    // Step 4: Transfer "artifact" to drone 2.
    let artifact = b"ELF_BINARY_MOCK_" .iter().copied().chain(std::iter::repeat(0).take(100)).collect::<Vec<u8>>();
    let sha256 = format!("{:x}", Sha256::new().chain_update(&artifact).finalize());
    let b64 = B64.encode(&artifact);

    let (_, start) = http_post(
        test_port,
        "/peer/file/start",
        &serde_json::json!({
            "transfer_id": "deploy_001",
            "filename": "app_binary",
            "total_size": artifact.len(),
            "sha256": sha256,
            "total_chunks": 1,
            "instructions": "notify Deployed to test machine",
        }),
    );
    assert!(start["accepted"].as_bool().unwrap());

    http_post(
        test_port,
        "/peer/file/chunk",
        &serde_json::json!({
            "transfer_id": "deploy_001",
            "index": 0,
            "data": b64,
        }),
    );

    let (_, deploy) = http_post(
        test_port,
        "/peer/file/complete",
        &serde_json::json!({"transfer_id": "deploy_001"}),
    );
    assert!(deploy["complete"].as_bool().unwrap());
    assert!(deploy["verified"].as_bool().unwrap());
    assert!(deploy["deploy_result"]["deployed"].as_bool().unwrap());

    // Step 5: Run tests on drone 2.
    let (_, test_task) = http_post(
        test_port,
        "/peer/task",
        &serde_json::json!({
            "task_id": "test_001",
            "prompt": "Run integration tests",
            "instructions": "echo 42 passed, 0 failed",
        }),
    );
    assert!(test_task["accepted"].as_bool().unwrap());

    let mut test_status = serde_json::json!({});
    for _ in 0..20 {
        thread::sleep(Duration::from_millis(200));
        let (_, s) = http_get(test_port, "/peer/task/test_001/status");
        test_status = s;
        if test_status["status"] == "completed" || test_status["status"] == "failed" {
            break;
        }
    }
    assert_eq!(test_status["status"], "completed");
    assert!(test_status["result"]["stdout"]
        .as_str()
        .unwrap()
        .contains("42 passed, 0 failed"));

    // Step 6: Chat messages.
    http_post(
        build_port,
        "/peer/message",
        &serde_json::json!({
            "id": "chat_001",
            "from": "ide_dev_1",
            "kind": "Chat",
            "payload": {"text": "Build complete"},
        }),
    );
    http_post(
        test_port,
        "/peer/message",
        &serde_json::json!({
            "id": "chat_002",
            "from": "ide_dev_1",
            "kind": "Chat",
            "payload": {"text": "Tests passed!"},
        }),
    );

    assert_eq!(build_core.messages.lock().unwrap().len(), 1);
    assert_eq!(test_core.messages.lock().unwrap().len(), 1);
}

#[test]
fn e2e_multi_chunk_large_file() {
    let (_, port) = start_drone("FileDrone");

    // 10KB file split into 4 chunks.
    let file_data: Vec<u8> = (0..10240).map(|i| (i % 256) as u8).collect();
    let sha256 = format!("{:x}", Sha256::new().chain_update(&file_data).finalize());

    let chunk_size = file_data.len() / 4;
    let chunks: Vec<&[u8]> = (0..4)
        .map(|i| &file_data[i * chunk_size..(i + 1) * chunk_size])
        .collect();

    http_post(
        port,
        "/peer/file/start",
        &serde_json::json!({
            "transfer_id": "large_xfer",
            "filename": "large_artifact.bin",
            "total_size": file_data.len(),
            "sha256": sha256,
            "total_chunks": 4,
        }),
    );

    for (i, chunk) in chunks.iter().enumerate() {
        let resp = http_post(
            port,
            "/peer/file/chunk",
            &serde_json::json!({
                "transfer_id": "large_xfer",
                "index": i,
                "data": B64.encode(chunk),
            }),
        );
        assert!(resp.1["received"].as_bool().unwrap());
    }

    let (_, result) = http_post(
        port,
        "/peer/file/complete",
        &serde_json::json!({"transfer_id": "large_xfer"}),
    );
    assert!(result["complete"].as_bool().unwrap());
    assert!(result["verified"].as_bool().unwrap());
}

#[test]
fn e2e_concurrent_tasks_multiple_drones() {
    let (_, port1) = start_drone("Drone1");
    let (_, port2) = start_drone("Drone2");

    // Send tasks to both drones.
    http_post(
        port1,
        "/peer/task",
        &serde_json::json!({
            "task_id": "concurrent_1",
            "prompt": "Task on drone 1",
            "instructions": "echo build_done",
        }),
    );
    http_post(
        port2,
        "/peer/task",
        &serde_json::json!({
            "task_id": "concurrent_2",
            "prompt": "Task on drone 2",
            "instructions": "echo test_done",
        }),
    );

    // Wait for both.
    let mut results = [false; 2];
    for _ in 0..30 {
        thread::sleep(Duration::from_millis(200));
        let (_, s1) = http_get(port1, "/peer/task/concurrent_1/status");
        let (_, s2) = http_get(port2, "/peer/task/concurrent_2/status");
        if s1["status"] == "completed" {
            results[0] = true;
        }
        if s2["status"] == "completed" {
            results[1] = true;
        }
        if results[0] && results[1] {
            break;
        }
    }

    assert!(results[0], "Drone 1 task should complete");
    assert!(results[1], "Drone 2 task should complete");
}
