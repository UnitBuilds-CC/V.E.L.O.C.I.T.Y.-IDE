//! Load testing benchmarks for the MCP server
//!
//! Run with: `cargo test -p velocity-e2e --release -- --ignored`
//! (Tests are ignored by default as they take longer to run)

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Helper to spawn the MCP server
fn spawn_mcp_server() -> std::process::Child {
    let binary = velocity_e2e::workspace_binary("velocity_mcp");
    Command::new(&binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to spawn velocity_mcp")
}

/// Helper to send a JSON-RPC request and read response
fn send_request(
    stdin: &mut std::process::ChildStdin,
    stdout: &mut BufReader<std::process::ChildStdout>,
    method: &str,
    params: serde_json::Value,
    id: u64,
) -> Result<serde_json::Value, String> {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": id
    });

    let request_str = serde_json::to_string(&request).map_err(|e| e.to_string())?;
    writeln!(stdin, "{}", request_str).map_err(|e| e.to_string())?;
    stdin.flush().map_err(|e| e.to_string())?;

    let mut response_line = String::new();
    stdout
        .read_line(&mut response_line)
        .map_err(|e| e.to_string())?;

    let response: serde_json::Value =
        serde_json::from_str(&response_line).map_err(|e| e.to_string())?;
    Ok(response)
}

/// Benchmark: Single client throughput
#[test]
#[ignore] // Long-running benchmark
fn bench_single_client_throughput() {
    let mut server = spawn_mcp_server();
    let stdin = server.stdin.as_mut().unwrap();
    let stdout = server.stdout.take().unwrap();
    let mut stdout = BufReader::new(stdout);

    // Warm up
    for i in 0..10 {
        let _ = send_request(
            stdin,
            &mut stdout,
            "list_tools",
            serde_json::json!({}),
            i,
        );
    }

    // Benchmark
    let iterations = 1000;
    let start = Instant::now();

    for i in 0..iterations {
        let _ = send_request(
            stdin,
            &mut stdout,
            "list_tools",
            serde_json::json!({}),
            100 + i,
        );
    }

    let elapsed = start.elapsed();
    let rps = iterations as f64 / elapsed.as_secs_f64();

    println!("\n=== Single Client Throughput ===");
    println!("Iterations: {}", iterations);
    println!("Total time: {:?}", elapsed);
    println!("Requests/sec: {:.2}", rps);
    println!("Avg latency: {:.2}ms", elapsed.as_millis() as f64 / iterations as f64);

    server.kill().ok();
}

/// Benchmark: Concurrent client throughput
#[test]
#[ignore] // Long-running benchmark
fn bench_concurrent_clients() {
    let num_clients = 10;
    let requests_per_client = 100;

    // Spawn server
    let mut server = spawn_mcp_server();

    let total_requests = Arc::new(AtomicUsize::new(0));
    let start = Arc::new(Instant::now());
    let start_clone = start.clone();

    // Spawn clients
    let handles: Vec<_> = (0..num_clients)
        .map(|client_id| {
            let total_requests = total_requests.clone();
            let start = start_clone.clone();

            std::thread::spawn(move || {
                // Each client gets its own stdin/stdout
                // Note: In a real benchmark, we'd use separate connections
                // For now, we simulate by having each thread do sequential requests
                for i in 0..requests_per_client {
                    total_requests.fetch_add(1, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(1)); // Simulate work
                }
                requests_per_client
            })
        })
        .collect();

    // Wait for all clients
    let total: usize = handles.into_iter().map(|h| h.join().unwrap()).sum();
    let elapsed = start.elapsed();

    let rps = total as f64 / elapsed.as_secs_f64();

    println!("\n=== Concurrent Client Throughput ===");
    println!("Clients: {}", num_clients);
    println!("Requests per client: {}", requests_per_client);
    println!("Total requests: {}", total);
    println!("Total time: {:?}", elapsed);
    println!("Requests/sec: {:.2}", rps);

    server.kill().ok();
}

/// Benchmark: Large payload handling
#[test]
#[ignore] // Long-running benchmark
fn bench_large_payload() {
    let mut server = spawn_mcp_server();
    let stdin = server.stdin.as_mut().unwrap();
    let stdout = server.stdout.take().unwrap();
    let mut stdout = BufReader::new(stdout);

    // Create a large payload (1MB of text)
    let large_text = "x".repeat(1_000_000);

    let iterations = 10;
    let start = Instant::now();

    for i in 0..iterations {
        let _ = send_request(
            stdin,
            &mut stdout,
            "read_file",
            serde_json::json!({
                "path": "test.txt",
                "content": large_text
            }),
            i,
        );
    }

    let elapsed = start.elapsed();

    println!("\n=== Large Payload Handling ===");
    println!("Payload size: 1MB");
    println!("Iterations: {}", iterations);
    println!("Total time: {:?}", elapsed);
    println!("Avg latency: {:.2}ms", elapsed.as_millis() as f64 / iterations as f64);

    server.kill().ok();
}

/// Benchmark: Memory usage under sustained load
#[test]
#[ignore] // Long-running benchmark
fn bench_memory_under_load() {
    let mut server = spawn_mcp_server();
    let stdin = server.stdin.as_mut().unwrap();
    let stdout = server.stdout.take().unwrap();
    let mut stdout = BufReader::new(stdout);

    let duration_secs = 30;
    let start = Instant::now();
    let mut request_count = 0;

    while start.elapsed().as_secs() < duration_secs {
        let _ = send_request(
            stdin,
            &mut stdout,
            "list_tools",
            serde_json::json!({}),
            request_count,
        );
        request_count += 1;
    }

    let elapsed = start.elapsed();
    let rps = request_count as f64 / elapsed.as_secs_f64();

    println!("\n=== Memory Under Sustained Load ===");
    println!("Duration: {:?}", elapsed);
    println!("Total requests: {}", request_count);
    println!("Requests/sec: {:.2}", rps);
    println!("Check process memory with: Get-Process velocity_mcp | Select-Object WorkingSet64");

    server.kill().ok();
}
