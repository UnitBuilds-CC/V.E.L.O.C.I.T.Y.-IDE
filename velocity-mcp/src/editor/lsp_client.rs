#![allow(dead_code)]
//! LSP (Language Server Protocol) client implementation.
//!
//! Manages language server processes and provides go-to-definition, hover,
//! references, rename, and diagnostics via JSON-RPC over stdin/stdout.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read as IoRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Configuration for a language server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspServerConfig {
    pub language_id: String,
    pub command: String,
    pub args: Vec<String>,
    pub root_uri: Option<String>,
    pub extensions: Vec<String>,
}

impl LspServerConfig {
    pub fn rust_analyzer(workspace_root: &Path) -> Self {
        Self {
            language_id: "rust".to_string(),
            command: "rust-analyzer".to_string(),
            args: Vec::new(),
            root_uri: Some(format!("file:///{}", workspace_root.display()).replace('\\', "/")),
            extensions: vec!["rs".to_string()],
        }
    }

    pub fn typescript(workspace_root: &Path) -> Self {
        Self {
            language_id: "typescript".to_string(),
            command: "typescript-language-server".to_string(),
            args: vec!["--stdio".to_string()],
            root_uri: Some(format!("file:///{}", workspace_root.display()).replace('\\', "/")),
            extensions: vec!["ts".to_string(), "tsx".to_string(), "js".to_string(), "jsx".to_string()],
        }
    }
}

/// A running language server process.
pub struct LspServer {
    pub config: LspServerConfig,
    process: Option<Child>,
    request_id: i64,
    pending_requests: HashMap<i64, String>,
    pub initialized: bool,
    pub capabilities: ServerCapabilities,
    /// Shared inbox for responses/notifications coming from the language server's stdout.
    pub inbox: Arc<Mutex<LspInbox>>,
}

/// Accumulated responses and notifications from the LSP server stdout reader thread.
#[derive(Debug, Default)]
pub struct LspInbox {
    /// Responses keyed by request id.
    pub responses: HashMap<i64, Value>,
    /// Incoming notifications (method, params).
    pub notifications: Vec<(String, Value)>,
    /// Reader thread alive flag.
    pub reader_alive: bool,
}

/// Subset of server capabilities we care about.
#[derive(Debug, Clone, Default)]
pub struct ServerCapabilities {
    pub completion: bool,
    pub hover: bool,
    pub definition: bool,
    pub references: bool,
    pub rename: bool,
    pub diagnostics: bool,
    pub document_symbols: bool,
}

/// An LSP diagnostic (error/warning from the language server).
#[derive(Debug, Clone)]
pub struct LspDiagnostic {
    pub file: PathBuf,
    pub line: usize,
    pub col: usize,
    pub end_line: usize,
    pub end_col: usize,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub source: Option<String>,
    pub code: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

/// Location result from go-to-definition or references.
#[derive(Debug, Clone)]
pub struct LspLocation {
    pub file: PathBuf,
    pub line: usize,
    pub col: usize,
}

/// Hover result.
#[derive(Debug, Clone)]
pub struct HoverResult {
    pub contents: String,
    pub range_start: Option<(usize, usize)>,
}

impl LspServer {
    pub fn new(config: LspServerConfig) -> Self {
        Self {
            config,
            process: None,
            request_id: 0,
            pending_requests: HashMap::new(),
            initialized: false,
            capabilities: ServerCapabilities::default(),
            inbox: Arc::new(Mutex::new(LspInbox {
                responses: HashMap::new(),
                notifications: Vec::new(),
                reader_alive: false,
            })),
        }
    }

    /// Start the language server process and spawn the stdout reader thread.
    pub fn start(&mut self) -> Result<(), String> {
        let mut child = Command::new(&self.config.command)
            .args(&self.config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to start {}: {}", self.config.command, e))?;

        // Take stdout and spawn reader thread
        if let Some(stdout) = child.stdout.take() {
            let inbox = self.inbox.clone();
            inbox.lock().unwrap().reader_alive = true;
            thread::spawn(move || {
                lsp_stdout_reader(stdout, inbox);
            });
        }

        self.process = Some(child);
        Ok(())
    }

    /// Take a response for a given request ID, if available.
    pub fn take_response(&self, id: i64) -> Option<Value> {
        self.inbox.lock().ok()?.responses.remove(&id)
    }

    /// Drain all pending notifications from the inbox.
    pub fn drain_notifications(&self) -> Vec<(String, Value)> {
        match self.inbox.lock() {
            Ok(mut inbox) => std::mem::take(&mut inbox.notifications),
            Err(_) => Vec::new(),
        }
    }

    /// Send the initialize request.
    pub fn initialize(&mut self, workspace_root: &Path) -> Result<(), String> {
        let root_uri = format!("file:///{}", workspace_root.display()).replace('\\', "/");
        let params = serde_json::json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "completion": { "completionItem": { "snippetSupport": true } },
                    "hover": {},
                    "definition": {},
                    "references": {},
                    "rename": { "prepareSupport": true },
                    "publishDiagnostics": { "relatedInformation": true }
                }
            }
        });
        self.send_request("initialize", params)?;
        self.initialized = true;
        Ok(())
    }

    /// Send a JSON-RPC request.
    fn send_request(&mut self, method: &str, params: Value) -> Result<i64, String> {
        self.request_id += 1;
        let id = self.request_id;
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let body = serde_json::to_string(&request).map_err(|e| e.to_string())?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());

        if let Some(ref mut child) = self.process {
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(header.as_bytes()).map_err(|e| e.to_string())?;
                stdin.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
                stdin.flush().map_err(|e| e.to_string())?;
            }
        }
        self.pending_requests.insert(id, method.to_string());
        Ok(id)
    }

    /// Send a notification (no response expected).
    fn send_notification(&mut self, method: &str, params: Value) -> Result<(), String> {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        let body = serde_json::to_string(&notification).map_err(|e| e.to_string())?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());

        if let Some(ref mut child) = self.process {
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(header.as_bytes()).map_err(|e| e.to_string())?;
                stdin.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
                stdin.flush().map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    /// Notify the server about a file open.
    pub fn did_open(&mut self, path: &Path, content: &str, language_id: &str) -> Result<(), String> {
        let uri = path_to_uri(path);
        self.send_notification("textDocument/didOpen", serde_json::json!({
            "textDocument": {
                "uri": uri,
                "languageId": language_id,
                "version": 1,
                "text": content,
            }
        }))
    }

    /// Notify the server about a file change.
    pub fn did_change(&mut self, path: &Path, content: &str, version: i32) -> Result<(), String> {
        let uri = path_to_uri(path);
        self.send_notification("textDocument/didChange", serde_json::json!({
            "textDocument": { "uri": uri, "version": version },
            "contentChanges": [{ "text": content }]
        }))
    }

    /// Request go-to-definition.
    pub fn goto_definition(&mut self, path: &Path, line: usize, col: usize) -> Result<i64, String> {
        let uri = path_to_uri(path);
        self.send_request("textDocument/definition", serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": col }
        }))
    }

    /// Request hover information.
    pub fn hover(&mut self, path: &Path, line: usize, col: usize) -> Result<i64, String> {
        let uri = path_to_uri(path);
        self.send_request("textDocument/hover", serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": col }
        }))
    }

    /// Request references.
    pub fn references(&mut self, path: &Path, line: usize, col: usize) -> Result<i64, String> {
        let uri = path_to_uri(path);
        self.send_request("textDocument/references", serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": col },
            "context": { "includeDeclaration": true }
        }))
    }

    /// Request completion at position.
    pub fn completion(&mut self, path: &Path, line: usize, col: usize) -> Result<i64, String> {
        let uri = path_to_uri(path);
        self.send_request("textDocument/completion", serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": col }
        }))
    }

    /// T3c: Request document symbols (outline of all functions, structs, etc.)
    /// Used by the test generator to discover testable functions via LSP.
    pub fn document_symbol(&mut self, path: &Path) -> Result<i64, String> {
        let uri = path_to_uri(path);
        self.send_request("textDocument/documentSymbol", serde_json::json!({
            "textDocument": { "uri": uri }
        }))
    }

    /// Shutdown the server gracefully.
    pub fn shutdown(&mut self) -> Result<(), String> {
        let _ = self.send_request("shutdown", Value::Null);
        let _ = self.send_notification("exit", Value::Null);
        if let Some(ref mut child) = self.process {
            let _ = child.kill();
        }
        Ok(())
    }

    /// Check if process is still running.
    pub fn is_alive(&mut self) -> bool {
        if let Some(ref mut child) = self.process {
            matches!(child.try_wait(), Ok(None))
        } else {
            false
        }
    }
}

impl Drop for LspServer {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

/// Manager for multiple LSP servers (one per language).
#[derive(Default)]
pub struct LspManager {
    servers: HashMap<String, LspServer>,
    pub diagnostics: Vec<LspDiagnostic>,
}

impl LspManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register and start a language server.
    pub fn register(&mut self, config: LspServerConfig, workspace_root: &Path) {
        let lang = config.language_id.clone();
        let mut server = LspServer::new(config);
        if server.start().is_ok() {
            let _ = server.initialize(workspace_root);
        }
        self.servers.insert(lang, server);
    }

    /// Get the server for a given file extension.
    pub fn server_for_extension(&mut self, ext: &str) -> Option<&mut LspServer> {
        self.servers.values_mut().find(|s| s.config.extensions.iter().any(|e| e == ext))
    }

    /// Get server by language ID.
    pub fn server_for_language(&mut self, lang: &str) -> Option<&mut LspServer> {
        self.servers.get_mut(lang)
    }

    /// Detect and start appropriate servers for a workspace.
    pub fn auto_detect(workspace_root: &Path) -> Self {
        let mut mgr = Self::new();

        // Check for Rust project
        if workspace_root.join("Cargo.toml").exists() {
            mgr.register(LspServerConfig::rust_analyzer(workspace_root), workspace_root);
        }
        // Check for TypeScript/JS project
        if workspace_root.join("package.json").exists() || workspace_root.join("tsconfig.json").exists() {
            mgr.register(LspServerConfig::typescript(workspace_root), workspace_root);
        }

        mgr
    }

    /// Poll all servers for incoming notifications and update diagnostics.
    pub fn poll_notifications(&mut self) {
        let mut new_diagnostics = Vec::new();
        for server in self.servers.values_mut() {
            let notifications = server.drain_notifications();
            for (method, params) in notifications {
                if method == "textDocument/publishDiagnostics" {
                    if let Some(diags) = parse_publish_diagnostics(&params) {
                        // Remove old diagnostics for this file, add new ones
                        let file = &diags[0].file;
                        self.diagnostics.retain(|d| &d.file != file);
                        new_diagnostics.extend(diags);
                    }
                }
                // Handle other notifications (e.g. progress) in the future
            }
        }
        self.diagnostics.extend(new_diagnostics);
    }

    /// Shutdown all servers.
    pub fn shutdown_all(&mut self) {
        for server in self.servers.values_mut() {
            let _ = server.shutdown();
        }
    }
}

/// Convert a filesystem path to a file:// URI.
pub fn path_to_uri(path: &Path) -> String {
    let s = path.display().to_string().replace('\\', "/");
    if s.starts_with('/') {
        format!("file://{}", s)
    } else {
        format!("file:///{}", s)
    }
}

/// Convert a file:// URI back to a filesystem path.
pub fn uri_to_path(uri: &str) -> PathBuf {
    let stripped = uri.strip_prefix("file:///").or_else(|| uri.strip_prefix("file://")).unwrap_or(uri);
    PathBuf::from(stripped.replace('/', std::path::MAIN_SEPARATOR_STR))
}

// ═══════════════════════════════════════════════════════════════════════════
// LSP Stdout Reader Thread
// ═══════════════════════════════════════════════════════════════════════════

/// Background thread that reads JSON-RPC messages from a language server's stdout.
/// Parses Content-Length headers, reads the JSON body, and deposits messages into
/// the shared `LspInbox`.
fn lsp_stdout_reader(stdout: impl IoRead + Send + 'static, inbox: Arc<Mutex<LspInbox>>) {
    let mut reader = BufReader::new(stdout);
    loop {
        // Read headers until empty line
        let mut content_length: Option<usize> = None;
        loop {
            let mut header_line = String::new();
            match reader.read_line(&mut header_line) {
                Ok(0) => {
                    // EOF — server process exited
                    if let Ok(mut inbox) = inbox.lock() {
                        inbox.reader_alive = false;
                    }
                    return;
                }
                Ok(_) => {
                    let trimmed = header_line.trim();
                    if trimmed.is_empty() {
                        break; // End of headers
                    }
                    if let Some(len_str) = trimmed.strip_prefix("Content-Length:") {
                        if let Ok(len) = len_str.trim().parse::<usize>() {
                            content_length = Some(len);
                        }
                    }
                }
                Err(_) => {
                    if let Ok(mut inbox) = inbox.lock() {
                        inbox.reader_alive = false;
                    }
                    return;
                }
            }
        }

        let Some(len) = content_length else {
            continue; // No Content-Length — skip malformed frame
        };

        // Read the JSON body
        let mut body = vec![0u8; len];
        if reader.read_exact(&mut body).is_err() {
            if let Ok(mut inbox) = inbox.lock() {
                inbox.reader_alive = false;
            }
            return;
        }

        let Ok(json) = serde_json::from_slice::<Value>(&body) else {
            continue; // Unparseable JSON — skip
        };

        // Classify: response (has "id" + "result"/"error") vs notification (has "method")
        if let Some(id) = json.get("id").and_then(|v| v.as_i64()) {
            // It's a response to a request
            if let Ok(mut inbox) = inbox.lock() {
                inbox.responses.insert(id, json);
            }
        } else if let Some(method) = json.get("method").and_then(|v| v.as_str()) {
            // It's a notification from the server
            let params = json.get("params").cloned().unwrap_or(Value::Null);
            if let Ok(mut inbox) = inbox.lock() {
                inbox.notifications.push((method.to_string(), params));
            }
        }
    }
}

/// Parse a `textDocument/publishDiagnostics` notification into our diagnostic structs.
fn parse_publish_diagnostics(params: &Value) -> Option<Vec<LspDiagnostic>> {
    let uri = params.get("uri")?.as_str()?;
    let file = uri_to_path(uri);
    let diagnostics_arr = params.get("diagnostics")?.as_array()?;
    if diagnostics_arr.is_empty() {
        // Empty diagnostics = file is clean. Return a sentinel to trigger cleanup.
        return Some(vec![LspDiagnostic {
            file,
            line: 0,
            col: 0,
            end_line: 0,
            end_col: 0,
            severity: DiagnosticSeverity::Info,
            message: String::new(),
            source: None,
            code: None,
        }]);
    }

    let mut results = Vec::with_capacity(diagnostics_arr.len());
    for diag in diagnostics_arr {
        let range = diag.get("range")?;
        let start = range.get("start")?;
        let end = range.get("end")?;
        let severity_num = diag.get("severity").and_then(|v| v.as_u64()).unwrap_or(1);
        let severity = match severity_num {
            1 => DiagnosticSeverity::Error,
            2 => DiagnosticSeverity::Warning,
            3 => DiagnosticSeverity::Info,
            _ => DiagnosticSeverity::Hint,
        };
        results.push(LspDiagnostic {
            file: file.clone(),
            line: start.get("line")?.as_u64()? as usize,
            col: start.get("character")?.as_u64()? as usize,
            end_line: end.get("line")?.as_u64()? as usize,
            end_col: end.get("character")?.as_u64()? as usize,
            severity,
            message: diag.get("message")?.as_str()?.to_string(),
            source: diag.get("source").and_then(|v| v.as_str()).map(String::from),
            code: diag.get("code").and_then(|v| {
                v.as_str().map(String::from).or_else(|| v.as_u64().map(|n| n.to_string()))
            }),
        });
    }
    if results.is_empty() { None } else { Some(results) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_uri_roundtrip_unix() {
        let path = Path::new("/home/user/project/src/main.rs");
        let uri = path_to_uri(path);
        assert!(uri.starts_with("file://"));
        assert!(uri.contains("main.rs"));
    }

    #[test]
    fn server_config_rust() {
        let cfg = LspServerConfig::rust_analyzer(Path::new("/tmp/project"));
        assert_eq!(cfg.language_id, "rust");
        assert!(cfg.extensions.contains(&"rs".to_string()));
    }

    #[test]
    fn parse_publish_diagnostics_works() {
        let params = serde_json::json!({
            "uri": "file:///home/user/project/src/main.rs",
            "diagnostics": [
                {
                    "range": {
                        "start": { "line": 5, "character": 10 },
                        "end": { "line": 5, "character": 15 }
                    },
                    "severity": 1,
                    "message": "cannot find value `x`",
                    "source": "rust-analyzer"
                }
            ]
        });
        let result = parse_publish_diagnostics(&params).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].line, 5);
        assert_eq!(result[0].severity, DiagnosticSeverity::Error);
        assert_eq!(result[0].message, "cannot find value `x`");
    }

    #[test]
    fn parse_publish_diagnostics_empty_clears_file() {
        let params = serde_json::json!({
            "uri": "file:///home/user/project/src/main.rs",
            "diagnostics": []
        });
        let result = parse_publish_diagnostics(&params).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].message.is_empty());
    }

    #[test]
    fn inbox_default_is_empty() {
        let inbox = LspInbox::default();
        assert!(inbox.responses.is_empty());
        assert!(inbox.notifications.is_empty());
        assert!(!inbox.reader_alive);
    }
}
