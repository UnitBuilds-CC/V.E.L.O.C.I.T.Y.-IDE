//! E2E test support library.
//!
//! Provides helpers for spawning V.E.L.O.C.I.T.Y. binaries and
//! communicating with them over their native protocols.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// A running MCP server process connected over stdio JSON-RPC.
pub struct McpStdioClient {
    child: Child,
    stdin: std::process::ChildStdin,
    reader: BufReader<std::process::ChildStdout>,
}

impl McpStdioClient {
    /// Spawn the MCP server binary in stdio mode.
    pub fn spawn(binary_path: &str) -> Result<Self, String> {
        let mut child = Command::new(binary_path)
            .args(["--mode", "stdio"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("failed to spawn MCP server: {}", e))?;

        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let reader = BufReader::new(stdout);

        Ok(Self {
            child,
            stdin,
            reader,
        })
    }

    /// Send a JSON-RPC request and read the response line.
    pub fn request(
        &mut self,
        method: &str,
        params: serde_json::Value,
        id: u64,
    ) -> Result<serde_json::Value, String> {
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": id
        });
        let line = serde_json::to_string(&req).map_err(|e| e.to_string())?;
        writeln!(self.stdin, "{}", line).map_err(|e| format!("stdin write failed: {}", e))?;
        self.stdin
            .flush()
            .map_err(|e| format!("stdin flush failed: {}", e))?;

        let mut response_line = String::new();
        self.reader
            .read_line(&mut response_line)
            .map_err(|e| format!("stdout read failed: {}", e))?;

        serde_json::from_str(response_line.trim()).map_err(|e| {
            format!(
                "failed to parse response: {} (raw: {})",
                e,
                response_line.trim()
            )
        })
    }

    /// Send a JSON-RPC request with no params.
    pub fn request_simple(&mut self, method: &str, id: u64) -> Result<serde_json::Value, String> {
        self.request(method, serde_json::json!({}), id)
    }

    /// Read a line from stdout with a best-effort timeout.
    pub fn read_line_timeout(&mut self, timeout: Duration) -> Option<String> {
        // Use a non-blocking approach: try reading in a loop
        let start = std::time::Instant::now();
        let mut line = String::new();
        loop {
            match self.reader.read_line(&mut line) {
                Ok(0) => return None, // EOF
                Ok(_) => return Some(line),
                Err(_) => {
                    if start.elapsed() > timeout {
                        return None;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }
}

impl Drop for McpStdioClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Find the path to a workspace binary (built in debug mode).
pub fn workspace_binary(name: &str) -> String {
    // When running via `cargo test`, CARGO_BIN_EXE_<name> is set
    // for workspace binaries. Fall back to a relative path.
    let env_var = format!("CARGO_BIN_EXE_{}", name);
    if let Ok(path) = std::env::var(&env_var) {
        return path;
    }
    // Fallback: look relative to the workspace target dir
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let target_dir = std::path::Path::new(manifest_dir)
        .parent()
        .unwrap()
        .join("target")
        .join("debug");
    let exe = if cfg!(windows) {
        format!("{}.exe", name)
    } else {
        name.to_string()
    };
    target_dir.join(exe).to_string_lossy().to_string()
}
