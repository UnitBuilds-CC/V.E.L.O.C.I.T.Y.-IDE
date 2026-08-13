//! E2E test: MCP server stdio JSON-RPC protocol.
//!
//! Spawns the actual `velocity_mcp` binary with `--mode stdio` and
//! exercises the JSON-RPC protocol over stdin/stdout.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

/// Spawn the MCP server in stdio mode and return (child, stdin, stdout_reader).
fn spawn_mcp_server() -> (
    std::process::Child,
    std::process::ChildStdin,
    BufReader<std::process::ChildStdout>,
) {
    let binary = velocity_e2e::workspace_binary("velocity_mcp");
    let mut child = Command::new(&binary)
        .args(["--mode", "stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn velocity_mcp at '{}': {}", binary, e));

    let stdin = child.stdin.take().expect("no stdin");
    let stdout = child.stdout.take().expect("no stdout");
    (child, stdin, BufReader::new(stdout))
}

fn send_request(
    stdin: &mut std::process::ChildStdin,
    reader: &mut BufReader<std::process::ChildStdout>,
    method: &str,
    params: serde_json::Value,
    id: u64,
) -> serde_json::Value {
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": id
    });
    let line = serde_json::to_string(&req).unwrap();
    writeln!(stdin, "{}", line).unwrap();
    stdin.flush().unwrap();

    // Skip any non-JSON startup lines (e.g. "Starting V.E.L.O.C.I.T.Y...")
    loop {
        let mut response_line = String::new();
        reader.read_line(&mut response_line).unwrap();
        let trimmed = response_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            return val;
        }
        // Not JSON — skip this startup banner line
    }
}

#[test]
fn mcp_initialize_handshake() {
    let (mut child, mut stdin, mut reader) = spawn_mcp_server();

    let resp = send_request(
        &mut stdin,
        &mut reader,
        "initialize",
        serde_json::json!({}),
        1,
    );

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);
    assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(
        resp["result"]["serverInfo"]["name"],
        "velocity-mcp-rust-server"
    );
    assert_eq!(resp["result"]["serverInfo"]["version"], "1.0.0");
    assert!(resp["result"]["capabilities"]["tools"].is_object());

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn mcp_tools_list_returns_tools() {
    let (mut child, mut stdin, mut reader) = spawn_mcp_server();

    // Must initialize first
    let _ = send_request(
        &mut stdin,
        &mut reader,
        "initialize",
        serde_json::json!({}),
        1,
    );

    // Now list tools
    let resp = send_request(
        &mut stdin,
        &mut reader,
        "tools/list",
        serde_json::json!({}),
        2,
    );

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 2);
    let tools = resp["result"]["tools"]
        .as_array()
        .expect("tools should be an array");
    assert!(
        !tools.is_empty(),
        "should have at least one tool registered"
    );

    // Verify each tool has name and description
    for tool in tools {
        assert!(tool["name"].is_string(), "tool should have a name");
        assert!(
            tool["description"].is_string(),
            "tool should have a description"
        );
    }

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn mcp_invalid_json_returns_parse_error() {
    let (mut child, mut stdin, mut reader) = spawn_mcp_server();

    // Send garbage
    writeln!(stdin, "{{not valid json").unwrap();
    stdin.flush().unwrap();

    // Skip startup banner lines, then read the parse error response
    let resp = loop {
        let mut response_line = String::new();
        reader.read_line(&mut response_line).unwrap();
        let trimmed = response_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            break val;
        }
    };

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["error"]["code"], -32700);
    assert!(resp["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Parse error"));

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn mcp_unknown_tool_returns_error_text() {
    let (mut child, mut stdin, mut reader) = spawn_mcp_server();

    let _ = send_request(
        &mut stdin,
        &mut reader,
        "initialize",
        serde_json::json!({}),
        1,
    );

    let resp = send_request(
        &mut stdin,
        &mut reader,
        "tools/call",
        serde_json::json!({"name": "nonexistent_tool_xyz", "arguments": {}}),
        2,
    );

    // The server should return a result (not crash), and the text should mention the error
    let text = resp["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(
        text.contains("nonexistent_tool_xyz")
            || text.contains("Unknown tool")
            || text.contains("Error"),
        "error message should mention the unknown tool, got: {}",
        text
    );

    let _ = child.kill();
    let _ = child.wait();
}
