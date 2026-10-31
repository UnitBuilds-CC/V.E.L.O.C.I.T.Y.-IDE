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

/// Read the next message off the protocol channel.
///
/// There is deliberately no "skip the startup banner" tolerance here: stdout *is*
/// the JSON-RPC channel, and a reader that quietly stepped over prose is what let
/// the server print its banner there in the first place.
fn read_message(reader: &mut BufReader<std::process::ChildStdout>) -> serde_json::Value {
    loop {
        let mut response_line = String::new();
        let read = reader.read_line(&mut response_line).unwrap();
        assert_ne!(read, 0, "the server closed stdout before answering");
        let trimmed = response_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        return serde_json::from_str(trimmed).unwrap_or_else(|e| {
            panic!("the first line on stdout must be the reply, not {trimmed:?} ({e})")
        });
    }
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

    read_message(reader)
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

    let resp = read_message(&mut reader);

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

#[test]
fn mcp_unknown_method_returns_method_not_found() {
    let (mut child, mut stdin, mut reader) = spawn_mcp_server();

    // An unrecognized method with a non-null id must yield JSON-RPC -32601.
    let resp = send_request(
        &mut stdin,
        &mut reader,
        "bogus/does_not_exist",
        serde_json::json!({}),
        7,
    );

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 7);
    assert_eq!(resp["error"]["code"], -32601);
    let msg = resp["error"]["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("not found") && msg.contains("bogus/does_not_exist"),
        "error message should name the unknown method, got: {}",
        msg
    );

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn mcp_request_ids_are_echoed_across_methods() {
    let (mut child, mut stdin, mut reader) = spawn_mcp_server();

    // Distinct ids across a result method, another result method, and an
    // error method must each be echoed back unchanged.
    let init = send_request(
        &mut stdin,
        &mut reader,
        "initialize",
        serde_json::json!({}),
        11,
    );
    assert_eq!(init["id"], 11);

    let list = send_request(
        &mut stdin,
        &mut reader,
        "tools/list",
        serde_json::json!({}),
        12,
    );
    assert_eq!(list["id"], 12);

    let unknown = send_request(
        &mut stdin,
        &mut reader,
        "no/such",
        serde_json::json!({}),
        13,
    );
    assert_eq!(unknown["id"], 13);

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn mcp_tools_call_unknown_tool_sets_is_error_flag() {
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
        serde_json::json!({"name": "definitely_not_a_real_tool", "arguments": {}}),
        2,
    );

    // tools/call always returns a result object (never a protocol-level error);
    // tool failures are surfaced via result.isError = true plus a text payload.
    assert_eq!(resp["id"], 2);
    assert!(
        resp["error"].is_null(),
        "tools/call should not return a JSON-RPC error object"
    );
    assert_eq!(resp["result"]["isError"], true);
    assert_eq!(resp["result"]["content"][0]["type"], "text");

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn mcp_notification_with_null_id_gets_no_response() {
    let (mut child, mut stdin, mut reader) = spawn_mcp_server();

    // A JSON-RPC notification carries no "id"; the server must not reply to it.
    let note = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    writeln!(stdin, "{}", serde_json::to_string(&note).unwrap()).unwrap();
    stdin.flush().unwrap();

    // The next request must produce the next response, proving the notification
    // neither consumed a reply nor emitted one of its own.
    let resp = send_request(
        &mut stdin,
        &mut reader,
        "initialize",
        serde_json::json!({}),
        5,
    );
    assert_eq!(resp["id"], 5);
    assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn mcp_tools_list_names_are_unique() {
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
        "tools/list",
        serde_json::json!({}),
        2,
    );

    let tools = resp["result"]["tools"]
        .as_array()
        .expect("tools should be an array");
    let mut names: Vec<String> = tools
        .iter()
        .filter_map(|t| t["name"].as_str().map(|s| s.to_string()))
        .collect();
    assert_eq!(
        names.len(),
        tools.len(),
        "every tool should expose a string name"
    );
    let total = names.len();
    names.sort();
    names.dedup();
    assert_eq!(
        names.len(),
        total,
        "tool names must be unique (no duplicates)"
    );

    let _ = child.kill();
    let _ = child.wait();
}

/// `ping` is how a client asks whether the server is alive without spending a tool
/// call. Until now it fell through to method-not-found, so a live server looked
/// unreachable to any client that probed it.
#[test]
fn mcp_ping_is_answered_with_an_empty_result() {
    let (mut child, mut stdin, mut reader) = spawn_mcp_server();

    let resp = send_request(&mut stdin, &mut reader, "ping", serde_json::json!({}), 21);

    assert_eq!(resp["id"], 21);
    assert!(
        resp["error"].is_null(),
        "ping must not be reported as an unknown method: {}",
        resp["error"]
    );
    assert_eq!(resp["result"], serde_json::json!({}));

    // Still alive afterwards, on the same stream.
    let again = send_request(&mut stdin, &mut reader, "ping", serde_json::json!({}), 22);
    assert_eq!(again["id"], 22);

    let _ = child.kill();
    let _ = child.wait();
}

/// A declined call and a broken call are different facts, and the client has to be
/// able to tell them apart without reading prose: it retries a fault and routes
/// around a refusal.
#[test]
fn mcp_separates_a_policy_refusal_from_a_missing_tool() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(workspace.path().join(".velocity")).unwrap();
    std::fs::write(
        workspace.path().join(".velocity").join("policy.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "default_decision": "Allow",
            "rules": [{ "tool": "read_file", "effect": "Deny" }],
        }))
        .unwrap(),
    )
    .unwrap();

    let binary = velocity_e2e::workspace_binary("velocity_mcp");
    let mut child = Command::new(&binary)
        .args([
            "--mode",
            "stdio",
            "--workspace",
            &workspace.path().display().to_string(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn velocity_mcp: {e}"));

    let mut stdin = child.stdin.take().expect("no stdin");
    let mut reader = BufReader::new(child.stdout.take().expect("no stdout"));

    let refused = send_request(
        &mut stdin,
        &mut reader,
        "tools/call",
        serde_json::json!({
            "name": "read_file",
            "arguments": { "relativeFilePath": "anything.txt" }
        }),
        31,
    );
    assert_eq!(refused["result"]["isError"], true);
    assert_eq!(refused["result"]["errorKind"], "permission_denied");
    let refusal_text = refused["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(
        refusal_text.contains("blocked by policy"),
        "the refusal keeps its own words: {refusal_text}"
    );
    assert!(
        !refusal_text.starts_with("Error running tool"),
        "a decision about the workspace is not a fault inside the tool: {refusal_text}"
    );

    let missing = send_request(
        &mut stdin,
        &mut reader,
        "tools/call",
        serde_json::json!({ "name": "no_such_tool_please", "arguments": {} }),
        32,
    );
    assert_eq!(missing["result"]["isError"], true);
    assert_eq!(
        missing["result"]["errorKind"], "tool_not_found",
        "the two kinds must not collapse into one label"
    );
    assert_ne!(
        refused["result"]["errorKind"],
        missing["result"]["errorKind"]
    );

    let _ = child.kill();
    let _ = child.wait();
}
