//! Debug Adapter Protocol (DAP) client for IDE debugger integration.
//!
//! Communicates with debug adapters (e.g., codelldb for Rust, node-debug for JS)
//! via stdin/stdout JSON messages with Content-Length headers.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A breakpoint set in the editor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Breakpoint {
    pub file: PathBuf,
    pub line: usize,
    pub condition: Option<String>,
    pub hit_count: Option<String>,
    pub enabled: bool,
    /// Server-assigned ID after verification.
    pub verified_id: Option<i64>,
}

/// A stack frame from a paused debug session.
#[derive(Debug, Clone)]
pub struct StackFrame {
    pub id: i64,
    pub name: String,
    pub file: Option<PathBuf>,
    pub line: usize,
    pub col: usize,
    pub module: Option<String>,
}

/// A variable in a scope.
#[derive(Debug, Clone)]
pub struct Variable {
    pub name: String,
    pub value: String,
    pub type_name: Option<String>,
    pub variables_reference: i64,
    /// Whether this variable can be expanded (has children).
    pub has_children: bool,
}

/// Watch expression.
#[derive(Debug, Clone)]
pub struct WatchExpression {
    pub expression: String,
    pub result: Option<String>,
}

/// Debug session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugState {
    Inactive,
    Starting,
    Running,
    Paused,
    Stopped,
}

/// Configuration for launching a debug session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchConfig {
    pub adapter_command: String,
    pub adapter_args: Vec<String>,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: HashMap<String, String>,
    pub stop_on_entry: bool,
}

impl LaunchConfig {
    /// Default config for debugging a Rust binary.
    pub fn rust_debug(binary_path: &Path, workspace_root: &Path) -> Self {
        Self {
            adapter_command: "codelldb".to_string(),
            adapter_args: vec!["--port".to_string(), "0".to_string()],
            program: binary_path.display().to_string(),
            args: Vec::new(),
            cwd: Some(workspace_root.display().to_string()),
            env: HashMap::new(),
            stop_on_entry: false,
        }
    }
}

/// DAP client managing a debug adapter process.
pub struct DapClient {
    process: Option<Child>,
    request_seq: i64,
    pub state: DebugState,
    pub breakpoints: Vec<Breakpoint>,
    pub stack_frames: Vec<StackFrame>,
    pub variables: Vec<Variable>,
    pub watches: Vec<WatchExpression>,
    pub output: Vec<String>,
    pub thread_id: Option<i64>,
}

impl Default for DapClient {
    fn default() -> Self {
        Self {
            process: None,
            request_seq: 0,
            state: DebugState::Inactive,
            breakpoints: Vec::new(),
            stack_frames: Vec::new(),
            variables: Vec::new(),
            watches: Vec::new(),
            output: Vec::new(),
            thread_id: None,
        }
    }
}

impl DapClient {
    pub fn new() -> Self {
        Self::default()
    }

    /// Launch a debug adapter and initialize the session.
    pub fn launch(&mut self, config: &LaunchConfig) -> Result<(), String> {
        let child = Command::new(&config.adapter_command)
            .args(&config.adapter_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to start debug adapter: {}", e))?;

        self.process = Some(child);
        self.state = DebugState::Starting;

        // Send initialize request
        self.send_request(
            "initialize",
            serde_json::json!({
                "clientID": "velocity-ide",
                "clientName": "Velocity IDE",
                "adapterID": "codelldb",
                "pathFormat": "path",
                "linesStartAt1": true,
                "columnsStartAt1": true,
                "supportsVariableType": true,
                "supportsVariablePaging": false,
                "supportsRunInTerminalRequest": false,
            }),
        )?;

        // Send launch request
        self.send_request(
            "launch",
            serde_json::json!({
                "program": config.program,
                "args": config.args,
                "cwd": config.cwd,
                "env": config.env,
                "stopOnEntry": config.stop_on_entry,
            }),
        )?;

        self.state = DebugState::Running;
        Ok(())
    }

    /// Set breakpoints for a file.
    pub fn set_breakpoints(&mut self, file: &Path, lines: &[usize]) -> Result<(), String> {
        let source_bps: Vec<Value> = lines
            .iter()
            .map(|&line| serde_json::json!({ "line": line }))
            .collect();

        self.send_request(
            "setBreakpoints",
            serde_json::json!({
                "source": { "path": file.display().to_string() },
                "breakpoints": source_bps,
            }),
        )
        .map(|_| ())
    }

    /// Continue execution after pause.
    pub fn continue_execution(&mut self) -> Result<(), String> {
        let thread_id = self.thread_id.unwrap_or(1);
        self.send_request("continue", serde_json::json!({ "threadId": thread_id }))?;
        self.state = DebugState::Running;
        Ok(())
    }

    /// Step over (next line).
    pub fn step_over(&mut self) -> Result<(), String> {
        let thread_id = self.thread_id.unwrap_or(1);
        self.send_request("next", serde_json::json!({ "threadId": thread_id }))?;
        Ok(())
    }

    /// Step into.
    pub fn step_into(&mut self) -> Result<(), String> {
        let thread_id = self.thread_id.unwrap_or(1);
        self.send_request("stepIn", serde_json::json!({ "threadId": thread_id }))?;
        Ok(())
    }

    /// Step out.
    pub fn step_out(&mut self) -> Result<(), String> {
        let thread_id = self.thread_id.unwrap_or(1);
        self.send_request("stepOut", serde_json::json!({ "threadId": thread_id }))?;
        Ok(())
    }

    /// Pause execution.
    pub fn pause(&mut self) -> Result<(), String> {
        let thread_id = self.thread_id.unwrap_or(1);
        self.send_request("pause", serde_json::json!({ "threadId": thread_id }))?;
        Ok(())
    }

    /// Stop (terminate) the debug session.
    pub fn stop(&mut self) -> Result<(), String> {
        let _ = self.send_request(
            "disconnect",
            serde_json::json!({ "terminateDebuggee": true }),
        );
        self.state = DebugState::Stopped;
        if let Some(ref mut child) = self.process {
            let _ = child.kill();
        }
        Ok(())
    }

    /// Evaluate an expression in the current context.
    pub fn evaluate(&mut self, expression: &str, frame_id: Option<i64>) -> Result<(), String> {
        let mut params = serde_json::json!({
            "expression": expression,
            "context": "watch",
        });
        if let Some(fid) = frame_id {
            params["frameId"] = Value::from(fid);
        }
        self.send_request("evaluate", params)?;
        Ok(())
    }

    /// Add a breakpoint.
    pub fn add_breakpoint(&mut self, file: PathBuf, line: usize) {
        self.breakpoints.push(Breakpoint {
            file,
            line,
            condition: None,
            hit_count: None,
            enabled: true,
            verified_id: None,
        });
    }

    /// Remove a breakpoint.
    pub fn remove_breakpoint(&mut self, file: &Path, line: usize) {
        self.breakpoints
            .retain(|bp| !(bp.file == file && bp.line == line));
    }

    /// Toggle a breakpoint at file:line.
    pub fn toggle_breakpoint(&mut self, file: PathBuf, line: usize) {
        if self
            .breakpoints
            .iter()
            .any(|bp| bp.file == file && bp.line == line)
        {
            self.remove_breakpoint(&file, line);
        } else {
            self.add_breakpoint(file, line);
        }
    }

    /// Check if a breakpoint exists at file:line.
    pub fn has_breakpoint(&self, file: &Path, line: usize) -> bool {
        self.breakpoints
            .iter()
            .any(|bp| bp.file == file && bp.line == line && bp.enabled)
    }

    /// Get breakpoints for a specific file.
    pub fn file_breakpoints(&self, file: &Path) -> Vec<&Breakpoint> {
        self.breakpoints
            .iter()
            .filter(|bp| bp.file == file)
            .collect()
    }

    /// Add a watch expression.
    pub fn add_watch(&mut self, expression: String) {
        self.watches.push(WatchExpression {
            expression,
            result: None,
        });
    }

    fn send_request(&mut self, command: &str, arguments: Value) -> Result<i64, String> {
        self.request_seq += 1;
        let seq = self.request_seq;
        let request = serde_json::json!({
            "seq": seq,
            "type": "request",
            "command": command,
            "arguments": arguments,
        });
        let body = serde_json::to_string(&request).map_err(|e| e.to_string())?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());

        if let Some(ref mut child) = self.process {
            if let Some(ref mut stdin) = child.stdin {
                stdin
                    .write_all(header.as_bytes())
                    .map_err(|e| e.to_string())?;
                stdin
                    .write_all(body.as_bytes())
                    .map_err(|e| e.to_string())?;
                stdin.flush().map_err(|e| e.to_string())?;
            }
        }
        Ok(seq)
    }
}

impl Drop for DapClient {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakpoint_toggle() {
        let mut dap = DapClient::new();
        let file = PathBuf::from("main.rs");
        dap.toggle_breakpoint(file.clone(), 10);
        assert!(dap.has_breakpoint(&file, 10));
        dap.toggle_breakpoint(file.clone(), 10);
        assert!(!dap.has_breakpoint(&file, 10));
    }

    #[test]
    fn file_breakpoints() {
        let mut dap = DapClient::new();
        dap.add_breakpoint(PathBuf::from("a.rs"), 5);
        dap.add_breakpoint(PathBuf::from("a.rs"), 10);
        dap.add_breakpoint(PathBuf::from("b.rs"), 3);
        assert_eq!(dap.file_breakpoints(Path::new("a.rs")).len(), 2);
    }

    #[test]
    fn watch_expressions() {
        let mut dap = DapClient::new();
        dap.add_watch("x + y".to_string());
        assert_eq!(dap.watches.len(), 1);
        assert_eq!(dap.watches[0].expression, "x + y");
    }

    #[test]
    fn initial_state() {
        let dap = DapClient::new();
        assert_eq!(dap.state, DebugState::Inactive);
    }
}
