#![allow(dead_code)]
//! LSP (Language Server Protocol) client implementation.
//!
//! Manages language server processes and provides go-to-definition, hover,
//! references, rename, and diagnostics via JSON-RPC over stdin/stdout.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read as IoRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::editor::completion::{CompletionItem, CompletionKind};
use crate::safety::SafeMutex;

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
            extensions: vec![
                "ts".to_string(),
                "tsx".to_string(),
                "js".to_string(),
                "jsx".to_string(),
            ],
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

/// A document symbol from `textDocument/documentSymbol` (outline entry).
///
/// Normalizes both LSP response shapes — hierarchical `DocumentSymbol`
/// (`range` + `children`) and flat `SymbolInformation` (`location`) — into a
/// single recursive structure. `line` is 0-based.
#[derive(Debug, Clone)]
pub struct LspSymbol {
    pub name: String,
    /// LSP SymbolKind number (e.g. 12 = Function, 6 = Method, 9 = Constructor).
    pub kind: u64,
    /// Signature/detail string reported by the server, if any.
    pub detail: String,
    /// 0-based line of the symbol's declaration.
    pub line: usize,
    pub children: Vec<LspSymbol>,
}

impl LspSymbol {
    /// Flatten the symbol tree depth-first into a list of functions/methods
    /// (SymbolKind Function = 12, Method = 6, Constructor = 9).
    pub fn flatten_functions(&self, out: &mut Vec<LspSymbol>) {
        if matches!(self.kind, 6 | 9 | 12) {
            out.push(self.clone());
        }
        for child in &self.children {
            child.flatten_functions(out);
        }
    }
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
            inbox.lock_safe().reader_alive = true;
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
                stdin
                    .write_all(header.as_bytes())
                    .map_err(|e| e.to_string())?;
                stdin
                    .write_all(body.as_bytes())
                    .map_err(|e| e.to_string())?;
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
                stdin
                    .write_all(header.as_bytes())
                    .map_err(|e| e.to_string())?;
                stdin
                    .write_all(body.as_bytes())
                    .map_err(|e| e.to_string())?;
                stdin.flush().map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    /// Notify the server about a file open.
    pub fn did_open(
        &mut self,
        path: &Path,
        content: &str,
        language_id: &str,
    ) -> Result<(), String> {
        let uri = path_to_uri(path);
        self.send_notification(
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": language_id,
                    "version": 1,
                    "text": content,
                }
            }),
        )
    }

    /// Notify the server about a file change.
    pub fn did_change(&mut self, path: &Path, content: &str, version: i32) -> Result<(), String> {
        let uri = path_to_uri(path);
        self.send_notification(
            "textDocument/didChange",
            serde_json::json!({
                "textDocument": { "uri": uri, "version": version },
                "contentChanges": [{ "text": content }]
            }),
        )
    }

    /// Request go-to-definition.
    pub fn goto_definition(&mut self, path: &Path, line: usize, col: usize) -> Result<i64, String> {
        let uri = path_to_uri(path);
        self.send_request(
            "textDocument/definition",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": col }
            }),
        )
    }

    /// Request hover information.
    pub fn hover(&mut self, path: &Path, line: usize, col: usize) -> Result<i64, String> {
        let uri = path_to_uri(path);
        self.send_request(
            "textDocument/hover",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": col }
            }),
        )
    }

    /// Request references.
    pub fn references(&mut self, path: &Path, line: usize, col: usize) -> Result<i64, String> {
        let uri = path_to_uri(path);
        self.send_request(
            "textDocument/references",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": col },
                "context": { "includeDeclaration": true }
            }),
        )
    }

    /// Request completion at position.
    pub fn completion(&mut self, path: &Path, line: usize, col: usize) -> Result<i64, String> {
        let uri = path_to_uri(path);
        self.send_request(
            "textDocument/completion",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": col }
            }),
        )
    }

    /// T3c: Request document symbols (outline of all functions, structs, etc.)
    /// Used by the test generator to discover testable functions via LSP.
    pub fn document_symbol(&mut self, path: &Path) -> Result<i64, String> {
        let uri = path_to_uri(path);
        self.send_request(
            "textDocument/documentSymbol",
            serde_json::json!({
                "textDocument": { "uri": uri }
            }),
        )
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

/// Immutable snapshot of a single language server's status for the UI panel.
#[derive(Debug, Clone)]
pub struct LspServerStatus {
    pub language: String,
    pub command: String,
    pub alive: bool,
    pub initialized: bool,
    pub extensions: Vec<String>,
}

/// Manager for multiple LSP servers (one per language).
#[derive(Default)]
pub struct LspManager {
    servers: HashMap<String, LspServer>,
    pub diagnostics: Vec<LspDiagnostic>,
    /// Documents we have announced to a server via `textDocument/didOpen`.
    open_docs: HashSet<PathBuf>,
    /// Per-document version counter for `textDocument/didChange`.
    doc_versions: HashMap<PathBuf, i32>,
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
        self.servers
            .values_mut()
            .find(|s| s.config.extensions.iter().any(|e| e == ext))
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
            mgr.register(
                LspServerConfig::rust_analyzer(workspace_root),
                workspace_root,
            );
        }
        // Check for TypeScript/JS project
        if workspace_root.join("package.json").exists()
            || workspace_root.join("tsconfig.json").exists()
        {
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

    /// Snapshot of one registered server's status (language, alive, initialized).
    pub fn server_snapshot(&mut self) -> Vec<LspServerStatus> {
        self.servers
            .iter_mut()
            .map(|(lang, srv)| {
                let alive = srv.is_alive();
                LspServerStatus {
                    language: lang.clone(),
                    command: srv.config.command.clone(),
                    alive,
                    initialized: srv.initialized,
                    extensions: srv.config.extensions.clone(),
                }
            })
            .collect()
    }

    /// Number of registered servers.
    pub fn server_count(&self) -> usize {
        self.servers.len()
    }

    /// Number of diagnostics currently held.
    pub fn diagnostics_count(&self) -> usize {
        self.diagnostics.len()
    }

    /// Announce/refresh a document with the matching language server so that
    /// subsequent requests see the current buffer content. Sends `didOpen` the
    /// first time a path is seen and `didChange` afterwards. Never panics when
    /// no server matches the extension.
    pub fn sync_document(&mut self, ext: &str, path: &Path, content: &str) {
        let already = self.open_docs.contains(path);
        let next_version = self.doc_versions.get(path).copied().unwrap_or(1) + 1;
        // Resolve the language id with a short-lived immutable borrow.
        let lang = self
            .server_for_extension(ext)
            .map(|s| s.config.language_id.clone());
        let Some(lang) = lang else { return };
        if let Some(server) = self.server_for_extension(ext) {
            if already {
                let _ = server.did_change(path, content, next_version);
            } else {
                let _ = server.did_open(path, content, &lang);
            }
        }
        if !already {
            self.open_docs.insert(path.to_path_buf());
        }
        self.doc_versions.insert(path.to_path_buf(), next_version);
    }

    /// Block (bounded) until the response for `id` arrives, or time out.
    /// Drains nothing else; responses are keyed by request id in the inbox.
    fn await_response(&mut self, ext: &str, id: i64) -> Option<Value> {
        let deadline = Instant::now() + Duration::from_millis(2000);
        loop {
            if let Some(server) = self.server_for_extension(ext) {
                if let Some(resp) = server.take_response(id) {
                    return Some(resp);
                }
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Go-to-definition at a position. Returns an empty vec when no server is
    /// available, the request fails, or the server times out.
    pub fn definition(
        &mut self,
        ext: &str,
        path: &Path,
        line: usize,
        col: usize,
        content: &str,
    ) -> Vec<LspLocation> {
        self.sync_document(ext, path, content);
        let id = match self.server_for_extension(ext) {
            Some(s) => s.goto_definition(path, line, col),
            None => return Vec::new(),
        };
        let Ok(id) = id else { return Vec::new() };
        match self.await_response(ext, id) {
            Some(resp) => parse_definition(&resp),
            None => Vec::new(),
        }
    }

    /// Find references at a position. Degrades to an empty vec like [`Self::definition`].
    pub fn references(
        &mut self,
        ext: &str,
        path: &Path,
        line: usize,
        col: usize,
        content: &str,
    ) -> Vec<LspLocation> {
        self.sync_document(ext, path, content);
        let id = match self.server_for_extension(ext) {
            Some(s) => s.references(path, line, col),
            None => return Vec::new(),
        };
        let Ok(id) = id else { return Vec::new() };
        match self.await_response(ext, id) {
            Some(resp) => parse_definition(&resp),
            None => Vec::new(),
        }
    }

    /// Hover information at a position. Returns `None` when unavailable.
    pub fn hover(
        &mut self,
        ext: &str,
        path: &Path,
        line: usize,
        col: usize,
        content: &str,
    ) -> Option<HoverResult> {
        self.sync_document(ext, path, content);
        let s = self.server_for_extension(ext)?;
        let id = s.hover(path, line, col);
        let Ok(id) = id else { return None };
        self.await_response(ext, id)
            .and_then(|resp| parse_hover(&resp))
    }

    /// Completion items at a position. Degrades to an empty vec when unavailable.
    pub fn completion(
        &mut self,
        ext: &str,
        path: &Path,
        line: usize,
        col: usize,
        content: &str,
    ) -> Vec<CompletionItem> {
        self.sync_document(ext, path, content);
        let id = match self.server_for_extension(ext) {
            Some(s) => s.completion(path, line, col),
            None => return Vec::new(),
        };
        let Ok(id) = id else { return Vec::new() };
        match self.await_response(ext, id) {
            Some(resp) => parse_completion(&resp),
            None => Vec::new(),
        }
    }

    /// Document symbol outline for a file (functions, structs, etc.). Degrades
    /// to an empty vec when no server is available or the request times out.
    /// Used by the test-coverage generator to discover testable functions.
    pub fn document_symbols(&mut self, ext: &str, path: &Path, content: &str) -> Vec<LspSymbol> {
        self.sync_document(ext, path, content);
        let id = match self.server_for_extension(ext) {
            Some(s) => s.document_symbol(path),
            None => return Vec::new(),
        };
        let Ok(id) = id else { return Vec::new() };
        match self.await_response(ext, id) {
            Some(resp) => parse_document_symbols(&resp),
            None => Vec::new(),
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
    let stripped = uri
        .strip_prefix("file:///")
        .or_else(|| uri.strip_prefix("file://"))
        .unwrap_or(uri);
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
            source: diag
                .get("source")
                .and_then(|v| v.as_str())
                .map(String::from),
            code: diag.get("code").and_then(|v| {
                v.as_str()
                    .map(String::from)
                    .or_else(|| v.as_u64().map(|n| n.to_string()))
            }),
        });
    }
    if results.is_empty() {
        None
    } else {
        Some(results)
    }
}

/// Parse a single LSP `Location` (`{uri, range}`) or `LocationLink`
/// (`{targetUri, targetRange}`) object into an [`LspLocation`].
fn parse_location_obj(obj: &Value) -> Option<LspLocation> {
    // Standard Location: { uri, range: { start: { line, character } } }
    if let (Some(uri), Some(range)) = (obj.get("uri").and_then(|v| v.as_str()), obj.get("range")) {
        let start = range.get("start")?;
        let line = start.get("line")?.as_u64()? as usize;
        let col = start.get("character")?.as_u64()? as usize;
        return Some(LspLocation {
            file: uri_to_path(uri),
            line,
            col,
        });
    }
    // LocationLink: { targetUri, targetRange: { start: { line, character } } }
    if let (Some(uri), Some(range)) = (
        obj.get("targetUri").and_then(|v| v.as_str()),
        obj.get("targetRange"),
    ) {
        let start = range.get("start")?;
        let line = start.get("line")?.as_u64()? as usize;
        let col = start.get("character")?.as_u64()? as usize;
        return Some(LspLocation {
            file: uri_to_path(uri),
            line,
            col,
        });
    }
    None
}

/// Parse a `textDocument/definition` (or `references`) response. The `result`
/// may be a single `Location`, an array of `Location`, or an array of
/// `LocationLink`. Returns an empty vec for null/absent results.
pub fn parse_definition(v: &Value) -> Vec<LspLocation> {
    let result = match v.get("result") {
        Some(r) if !r.is_null() => r,
        _ => return Vec::new(),
    };
    if let Some(arr) = result.as_array() {
        arr.iter().filter_map(parse_location_obj).collect()
    } else {
        parse_location_obj(result).into_iter().collect()
    }
}

/// Recursively extract plain text from an LSP hover `contents` value, which may
/// be a string, a `MarkupContent`/`MarkedString` object (`{value}`), or an array
/// of those.
fn extract_markup(v: &Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        return Some(s.to_string());
    }
    if let Some(value) = v.get("value").and_then(|x| x.as_str()) {
        return Some(value.to_string());
    }
    if let Some(arr) = v.as_array() {
        let parts: Vec<String> = arr.iter().filter_map(extract_markup).collect();
        if parts.is_empty() {
            return None;
        }
        return Some(parts.join("\n"));
    }
    None
}

/// Parse a `textDocument/hover` response into a [`HoverResult`]. Returns `None`
/// when the result is null or has no textual content.
pub fn parse_hover(v: &Value) -> Option<HoverResult> {
    let result = v.get("result")?;
    if result.is_null() {
        return None;
    }
    let contents = extract_markup(result.get("contents")?)?;
    if contents.trim().is_empty() {
        return None;
    }
    let range_start = result
        .get("range")
        .and_then(|r| r.get("start"))
        .and_then(|s| {
            let l = s.get("line")?.as_u64()? as usize;
            let c = s.get("character")?.as_u64()? as usize;
            Some((l, c))
        });
    Some(HoverResult {
        contents,
        range_start,
    })
}

/// Map an LSP `CompletionItemKind` number to our [`CompletionKind`].
fn map_completion_kind(kind: u64) -> CompletionKind {
    match kind {
        2 | 3 => CompletionKind::Function,       // Method, Function
        7 | 8 | 13 | 22 => CompletionKind::Type, // Class, Interface, Enum, Struct
        9 => CompletionKind::Module,             // Module
        5 | 10 => CompletionKind::Field,         // Field, Property
        6 | 12 | 21 => CompletionKind::Variable, // Variable, Value, Constant
        14 => CompletionKind::Keyword,           // Keyword
        15 => CompletionKind::Snippet,           // Snippet
        17 => CompletionKind::File,              // File
        _ => CompletionKind::Variable,
    }
}

/// Parse a `textDocument/completion` response. The `result` may be a bare array
/// of `CompletionItem` or a `CompletionList { items }`. Returns an empty vec for
/// null/absent results.
pub fn parse_completion(v: &Value) -> Vec<CompletionItem> {
    let result = match v.get("result") {
        Some(r) if !r.is_null() => r,
        _ => return Vec::new(),
    };
    let items: &[Value] = if let Some(arr) = result.as_array() {
        arr.as_slice()
    } else if let Some(arr) = result.get("items").and_then(|i| i.as_array()) {
        arr.as_slice()
    } else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|it| {
            let label = it.get("label")?.as_str()?.to_string();
            let kind_num = it.get("kind").and_then(|k| k.as_u64()).unwrap_or(1);
            let kind = map_completion_kind(kind_num);
            let insert_text = it
                .get("insertText")
                .and_then(|x| x.as_str())
                .map(String::from)
                .unwrap_or_else(|| label.clone());
            let detail = it.get("detail").and_then(|x| x.as_str()).map(String::from);
            let sort_key = it
                .get("sortText")
                .and_then(|x| x.as_str())
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(20);
            Some(CompletionItem {
                label,
                kind,
                detail,
                insert_text,
                sort_key,
            })
        })
        .collect()
}

/// Parse a `textDocument/documentSymbol` response into a hierarchical symbol
/// list. Accepts the JSON-RPC envelope (`{ "result": ... }`) or a bare result
/// value, and both LSP shapes: hierarchical `DocumentSymbol[]` (with `range`
/// and `children`) and flat `SymbolInformation[]` (with `location`).
pub fn parse_document_symbols(v: &Value) -> Vec<LspSymbol> {
    let result = match v.get("result") {
        Some(r) if !r.is_null() => r,
        Some(_) => return Vec::new(),
        None => v, // caller passed the bare result value
    };
    let arr = match result.as_array() {
        Some(a) => a,
        None => return Vec::new(),
    };
    arr.iter().map(symbol_from_value).collect()
}

/// Build a single [`LspSymbol`] from a `DocumentSymbol` or `SymbolInformation`
/// JSON value, recursing into `children` when present.
fn symbol_from_value(sym: &Value) -> LspSymbol {
    let name = sym
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    let kind = sym.get("kind").and_then(|k| k.as_u64()).unwrap_or(0);
    let detail = sym
        .get("detail")
        .and_then(|d| d.as_str())
        .unwrap_or("")
        .to_string();
    // Hierarchical DocumentSymbol uses `range`; flat SymbolInformation uses `location.range`.
    let line = sym
        .get("range")
        .or_else(|| sym.get("location").and_then(|l| l.get("range")))
        .and_then(|r| r.get("start"))
        .and_then(|s| s.get("line"))
        .and_then(|l| l.as_u64())
        .unwrap_or(0) as usize;
    let children = sym
        .get("children")
        .and_then(|c| c.as_array())
        .map(|arr| arr.iter().map(symbol_from_value).collect())
        .unwrap_or_default();
    LspSymbol {
        name,
        kind,
        detail,
        line,
        children,
    }
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

    // ─── I1: interactive intelligence parsers ──────────────────────────────

    #[test]
    fn parse_definition_single_location() {
        let resp = serde_json::json!({
            "jsonrpc": "2.0", "id": 1,
            "result": {
                "uri": "file:///proj/src/lib.rs",
                "range": { "start": { "line": 10, "character": 4 }, "end": { "line": 10, "character": 9 } }
            }
        });
        let locs = parse_definition(&resp);
        assert_eq!(locs.len(), 1);
        assert_eq!(locs[0].line, 10);
        assert_eq!(locs[0].col, 4);
        assert!(locs[0].file.ends_with("lib.rs"));
    }

    #[test]
    fn parse_definition_array_of_locations() {
        let resp = serde_json::json!({
            "id": 2,
            "result": [
                { "uri": "file:///a.rs", "range": { "start": { "line": 1, "character": 0 } } },
                { "uri": "file:///b.rs", "range": { "start": { "line": 2, "character": 3 } } }
            ]
        });
        let locs = parse_definition(&resp);
        assert_eq!(locs.len(), 2);
        assert_eq!(locs[1].line, 2);
        assert_eq!(locs[1].col, 3);
    }

    #[test]
    fn parse_definition_location_links() {
        let resp = serde_json::json!({
            "id": 3,
            "result": [
                {
                    "targetUri": "file:///target.rs",
                    "targetRange": { "start": { "line": 7, "character": 2 } },
                    "targetSelectionRange": { "start": { "line": 7, "character": 2 } }
                }
            ]
        });
        let locs = parse_definition(&resp);
        assert_eq!(locs.len(), 1);
        assert_eq!(locs[0].line, 7);
        assert!(locs[0].file.ends_with("target.rs"));
    }

    #[test]
    fn parse_definition_null_result_is_empty() {
        let resp = serde_json::json!({ "id": 4, "result": null });
        assert!(parse_definition(&resp).is_empty());
        let no_result = serde_json::json!({ "id": 5 });
        assert!(parse_definition(&no_result).is_empty());
    }

    #[test]
    fn parse_hover_markup_content() {
        let resp = serde_json::json!({
            "id": 1,
            "result": {
                "contents": { "kind": "markdown", "value": "fn foo() -> i32" },
                "range": { "start": { "line": 3, "character": 5 } }
            }
        });
        let hover = parse_hover(&resp).expect("hover parsed");
        assert_eq!(hover.contents, "fn foo() -> i32");
        assert_eq!(hover.range_start, Some((3, 5)));
    }

    #[test]
    fn parse_hover_plain_string() {
        let resp = serde_json::json!({ "id": 2, "result": { "contents": "plain docs" } });
        let hover = parse_hover(&resp).expect("hover parsed");
        assert_eq!(hover.contents, "plain docs");
        assert_eq!(hover.range_start, None);
    }

    #[test]
    fn parse_hover_array_of_marked_strings() {
        let resp = serde_json::json!({
            "id": 3,
            "result": { "contents": [ { "language": "rust", "value": "sig" }, "extra" ] }
        });
        let hover = parse_hover(&resp).expect("hover parsed");
        assert_eq!(hover.contents, "sig\nextra");
    }

    #[test]
    fn parse_hover_null_is_none() {
        assert!(parse_hover(&serde_json::json!({ "id": 4, "result": null })).is_none());
        assert!(
            parse_hover(&serde_json::json!({ "id": 5, "result": { "contents": "" } })).is_none()
        );
    }

    #[test]
    fn parse_completion_bare_array() {
        let resp = serde_json::json!({
            "id": 1,
            "result": [
                { "label": "println!", "kind": 3, "detail": "macro", "insertText": "println!($0)" },
                { "label": "foo" }
            ]
        });
        let items = parse_completion(&resp);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].label, "println!");
        assert_eq!(items[0].kind, CompletionKind::Function);
        assert_eq!(items[0].insert_text, "println!($0)");
        assert_eq!(items[0].detail.as_deref(), Some("macro"));
        // No insertText -> falls back to label; no kind -> Variable default.
        assert_eq!(items[1].insert_text, "foo");
        assert_eq!(items[1].kind, CompletionKind::Variable);
    }

    #[test]
    fn parse_completion_list_and_kind_mapping() {
        let resp = serde_json::json!({
            "id": 2,
            "result": { "isIncomplete": false, "items": [
                { "label": "m", "kind": 2 },
                { "label": "f", "kind": 3 },
                { "label": "C", "kind": 7 },
                { "label": "I", "kind": 8 },
                { "label": "E", "kind": 13 },
                { "label": "S", "kind": 22 },
                { "label": "mod", "kind": 9 },
                { "label": "field", "kind": 5 },
                { "label": "prop", "kind": 10 },
                { "label": "var", "kind": 6 },
                { "label": "kw", "kind": 14 },
                { "label": "snip", "kind": 15 },
                { "label": "file", "kind": 17 },
                { "label": "unk", "kind": 99 }
            ] }
        });
        let items = parse_completion(&resp);
        assert_eq!(items.len(), 14);
        assert_eq!(items[0].kind, CompletionKind::Function);
        assert_eq!(items[1].kind, CompletionKind::Function);
        assert_eq!(items[2].kind, CompletionKind::Type);
        assert_eq!(items[3].kind, CompletionKind::Type);
        assert_eq!(items[4].kind, CompletionKind::Type);
        assert_eq!(items[5].kind, CompletionKind::Type);
        assert_eq!(items[6].kind, CompletionKind::Module);
        assert_eq!(items[7].kind, CompletionKind::Field);
        assert_eq!(items[8].kind, CompletionKind::Field);
        assert_eq!(items[9].kind, CompletionKind::Variable);
        assert_eq!(items[10].kind, CompletionKind::Keyword);
        assert_eq!(items[11].kind, CompletionKind::Snippet);
        assert_eq!(items[12].kind, CompletionKind::File);
        assert_eq!(items[13].kind, CompletionKind::Variable);
    }

    #[test]
    fn parse_completion_null_is_empty() {
        assert!(parse_completion(&serde_json::json!({ "id": 3, "result": null })).is_empty());
    }

    #[test]
    fn manager_helpers_degrade_without_server() {
        // No servers registered: every interactive helper must return an empty
        // result (or None) immediately and never panic.
        let mut mgr = LspManager::new();
        let path = Path::new("/tmp/does_not_matter.rs");
        assert!(mgr.definition("rs", path, 0, 0, "fn main() {}").is_empty());
        assert!(mgr.references("rs", path, 0, 0, "fn main() {}").is_empty());
        assert!(mgr.hover("rs", path, 0, 0, "fn main() {}").is_none());
        assert!(mgr.completion("rs", path, 0, 0, "fn main() {}").is_empty());
    }

    #[test]
    fn parse_document_symbols_hierarchical() {
        // Hierarchical DocumentSymbol[]: a struct (kind 23) nesting a method
        // (kind 6) plus a top-level function (kind 12).
        let resp = serde_json::json!({ "id": 7, "result": [
            {
                "name": "Widget", "kind": 23, "detail": "struct Widget",
                "range": { "start": { "line": 3, "character": 0 }, "end": { "line": 9, "character": 1 } },
                "children": [
                    { "name": "render", "kind": 6, "detail": "fn render(&self)",
                      "range": { "start": { "line": 5, "character": 4 }, "end": { "line": 7, "character": 5 } } }
                ]
            },
            { "name": "main", "kind": 12, "detail": "fn main()",
              "range": { "start": { "line": 11, "character": 0 }, "end": { "line": 13, "character": 1 } } }
        ]});
        let symbols = parse_document_symbols(&resp);
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "Widget");
        assert_eq!(symbols[0].children.len(), 1);
        // Flattening picks out only functions/methods (method + function).
        let mut fns = Vec::new();
        for s in &symbols {
            s.flatten_functions(&mut fns);
        }
        let names: Vec<&str> = fns.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["render", "main"]);
        assert_eq!(fns[0].line, 5);
        assert_eq!(fns[1].line, 11);
    }

    #[test]
    fn parse_document_symbols_flat_symbol_information() {
        // Flat SymbolInformation[] uses `location.range` instead of `range`.
        let resp = serde_json::json!([
            { "name": "helper", "kind": 12,
              "location": { "uri": "file:///x.rs", "range": { "start": { "line": 2, "character": 0 }, "end": { "line": 4, "character": 1 } } } }
        ]);
        let symbols = parse_document_symbols(&resp);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "helper");
        assert_eq!(symbols[0].kind, 12);
        assert_eq!(symbols[0].line, 2);
    }

    #[test]
    fn parse_document_symbols_null_is_empty() {
        assert!(parse_document_symbols(&serde_json::json!({ "id": 8, "result": null })).is_empty());
    }

    #[test]
    fn manager_document_symbols_degrade_without_server() {
        let mut mgr = LspManager::new();
        let path = Path::new("/tmp/does_not_matter.rs");
        assert!(mgr.document_symbols("rs", path, "fn main() {}").is_empty());
    }
}
