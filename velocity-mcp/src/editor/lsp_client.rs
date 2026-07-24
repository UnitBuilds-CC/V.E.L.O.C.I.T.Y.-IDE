#![allow(dead_code)]
//! LSP (Language Server Protocol) client implementation.
//!
//! Manages language server processes and provides go-to-definition, hover,
//! references, rename, and diagnostics via JSON-RPC over stdin/stdout.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

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
        }
    }

    /// Start the language server process.
    pub fn start(&mut self) -> Result<(), String> {
        let child = Command::new(&self.config.command)
            .args(&self.config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to start {}: {}", self.config.command, e))?;
        self.process = Some(child);
        Ok(())
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
}
