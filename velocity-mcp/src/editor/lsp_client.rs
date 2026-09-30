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

    pub fn python(workspace_root: &Path) -> Self {
        Self {
            language_id: "python".to_string(),
            command: "pyright-langserver".to_string(),
            args: vec!["--stdio".to_string()],
            root_uri: Some(format!("file:///{}", workspace_root.display()).replace('\\', "/")),
            extensions: vec!["py".to_string(), "pyi".to_string()],
        }
    }

    pub fn go(workspace_root: &Path) -> Self {
        Self {
            language_id: "go".to_string(),
            command: "gopls".to_string(),
            args: Vec::new(),
            root_uri: Some(format!("file:///{}", workspace_root.display()).replace('\\', "/")),
            extensions: vec!["go".to_string()],
        }
    }

    pub fn clangd(workspace_root: &Path) -> Self {
        Self {
            language_id: "cpp".to_string(),
            command: "clangd".to_string(),
            args: Vec::new(),
            root_uri: Some(format!("file:///{}", workspace_root.display()).replace('\\', "/")),
            extensions: vec![
                "c".to_string(),
                "cpp".to_string(),
                "h".to_string(),
                "hpp".to_string(),
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
    /// Incoming server→client *requests* (id, method, params). These carry both
    /// an `id` and a `method` (e.g. `workspace/applyEdit`) and must be answered,
    /// so they are kept apart from the plain notifications and from the
    /// responses to *our* requests.
    pub requests: Vec<(i64, String, Value)>,
    /// Reader thread alive flag.
    pub reader_alive: bool,
}

/// How an inbound JSON-RPC frame is classified. A server→client request is
/// distinguished from a response by having a `method` *and* an `id`; a response
/// has an `id` and a `result`/`error` but no `method`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageKind {
    /// Reply to one of our requests (has `id`, no `method`).
    Response,
    /// A request from the server that expects a response (has `id` + `method`).
    Request,
    /// A one-way notification (has `method`, no `id`).
    Notification,
    /// Neither — ignored.
    Unknown,
}

/// Classify an inbound JSON-RPC message. The order matters: `id` alone is a
/// response, `id` + `method` is a server-initiated request, and `method` alone
/// is a notification.
pub fn classify_message(json: &Value) -> MessageKind {
    let has_id = json.get("id").is_some_and(|v| !v.is_null());
    let has_method = json.get("method").is_some_and(|v| v.is_string());
    match (has_id, has_method) {
        (true, true) => MessageKind::Request,
        (true, false) => MessageKind::Response,
        (false, true) => MessageKind::Notification,
        (false, false) => MessageKind::Unknown,
    }
}

/// Encode a JSON-RPC 2.0 response frame (headers included) for a server→client
/// request. Factored out of [`LspServer::respond`] so the wire format is a pure,
/// unit-testable function.
pub fn encode_jsonrpc_response(id: i64, result: Value) -> String {
    let body = serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string();
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body)
}

/// Choose a benign `result` for a server→client request we do not specially
/// handle, so the server never blocks waiting for a reply. Pure (and thus
/// unit-testable); `workspace/applyEdit` is normally resolved synchronously by
/// [`LspManager::execute_code_action`], and a stray one arriving while idle is
/// declined rather than dropped.
pub fn server_request_result(method: &str, params: &Value) -> Value {
    match method {
        // Decline dynamic registration; statically-advertised capabilities
        // still function, and this keeps the server from hanging.
        "client/registerCapability" | "client/unregisterCapability" => Value::Null,
        // Reply with one null per requested item: "no client-side settings".
        "workspace/configuration" => {
            let n = params
                .get("items")
                .and_then(|i| i.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            Value::Array(vec![Value::Null; n])
        }
        "window/workDoneProgress/create" => Value::Null,
        "workspace/applyEdit" => serde_json::json!({ "applied": false }),
        _ => Value::Null,
    }
}

/// Subset of server capabilities we care about. Populated from the real
/// `initialize` result ([`LspServer::apply_server_capabilities`]), so
/// requests that a server has not advertised are never sent.
#[derive(Debug, Clone, Default)]
pub struct ServerCapabilities {
    pub completion: bool,
    pub hover: bool,
    pub definition: bool,
    /// `declarationProvider` — jump to the *declaration* (C/C++ header) of a
    /// symbol whose definition lives in an implementation file.
    pub declaration: bool,
    /// `typeDefinitionProvider` — jump to the type of the symbol under the caret.
    pub type_definition: bool,
    /// `implementationProvider` — jump to concrete implementations of an
    /// interface / abstract member.
    pub implementation: bool,
    pub references: bool,
    pub rename: bool,
    pub diagnostics: bool,
    pub document_symbols: bool,
    /// `workspaceSymbolProvider` is advertised — the server answers
    /// `workspace/symbol`, powering go-to-symbol across the whole project
    /// (not just the open file).
    pub workspace_symbol: bool,
    /// `semanticTokensProvider` is advertised and can be requested.
    pub semantic_tokens: bool,
    /// Legend token type names, in the server's declared order — the index
    /// space of every semantic-token response's quintuple.
    pub semantic_token_types: Vec<String>,
    /// Legend token modifier names, in declared order (bitfield position).
    pub semantic_token_modifiers: Vec<String>,
    /// `documentFormattingProvider` is advertised.
    pub formatting: bool,
    /// `codeActionProvider` is advertised (quick fixes / refactorings).
    pub code_action: bool,
    /// `codeActionProvider.resolveProvider` is true: actions may come back
    /// without `edit`/`command` and must be completed via `codeAction/resolve`.
    pub code_action_resolve: bool,
    /// `executeCommandProvider` is advertised — the server accepts
    /// `workspace/executeCommand`, so command-only code actions can run.
    pub execute_command: bool,
    /// `signatureHelpProvider` is advertised — the server answers
    /// `textDocument/signatureHelp` (parameter hints inside a call).
    pub signature_help: bool,
    /// Characters the server wants as signature-help triggers (usually `(` and
    /// `,`). The editor auto-requests parameter hints right after typing one.
    pub signature_trigger_chars: Vec<String>,
    /// `callHierarchyProvider` is advertised — the server answers
    /// `textDocument/prepareCallHierarchy` and the follow-on
    /// `callHierarchy/{incoming,outgoing}Calls` (who-calls / what-calls).
    pub call_hierarchy: bool,
    /// `selectionRangeProvider` is advertised — the server answers
    /// `textDocument/selectionRange`, powering expand/shrink smart selection.
    pub selection_range: bool,
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

/// A `window/showMessage` notification from a language server. These are
/// user-facing status lines (e.g. rust-analyzer's "Failed to load
/// workspaces.") that carry no file or location. They were previously
/// discarded outright, hiding real server-side problems from the user; the
/// GUI now surfaces them as toasts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspMessage {
    pub severity: DiagnosticSeverity,
    pub message: String,
}

/// Location result from go-to-definition or references.
#[derive(Debug, Clone)]
pub struct LspLocation {
    pub file: PathBuf,
    pub line: usize,
    pub col: usize,
}

/// One node in a call hierarchy: a function/method that calls the symbol under
/// the caret (an *incoming* call) or is called by it (an *outgoing* call).
/// Normalized from an LSP `CallHierarchyItem` for display + click-to-navigate.
#[derive(Debug, Clone)]
pub struct LspCallHierarchyItem {
    pub name: String,
    /// LSP `SymbolKind` number (1..=26); carried so the UI can label the node.
    pub kind: u8,
    pub file: PathBuf,
    pub line: usize,
    pub col: usize,
}

/// A semantic selection range (LSP `SelectionRange`), in 0-based
/// line/character coordinates. `textDocument/selectionRange` returns these
/// nested (each node is contained by its parent); the parser flattens a chain
/// innermost-first so expand/shrink can walk outward/inward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LspSelectionRange {
    pub start_line: usize,
    pub start_char: usize,
    pub end_line: usize,
    pub end_char: usize,
}

/// Hover result.
#[derive(Debug, Clone)]
pub struct HoverResult {
    pub contents: String,
    pub range_start: Option<(usize, usize)>,
}

/// One overload's parameter, normalized to a display `label`. LSP sends the
/// label either as a string or as a `[start, end]` utf16 range into the
/// signature label; the range form is sliced out into `label` by the parser.
#[derive(Debug, Clone)]
pub struct LspParameterInformation {
    pub label: String,
    pub documentation: String,
}

/// A single callable signature (one overload).
#[derive(Debug, Clone)]
pub struct LspSignature {
    pub label: String,
    pub documentation: String,
    pub parameters: Vec<LspParameterInformation>,
}

/// Result of `textDocument/signatureHelp` — parameter hints for the call the
/// caret currently sits inside. `active_*` index into `signatures` /
/// the active signature's `parameters` (both default to 0).
#[derive(Debug, Clone)]
pub struct LspSignatureHelp {
    pub signatures: Vec<LspSignature>,
    pub active_signature: usize,
    pub active_parameter: usize,
}

impl LspSignatureHelp {
    /// The signature the server marked active, clamped into range.
    pub fn active(&self) -> Option<&LspSignature> {
        self.signatures.get(self.active_signature)
    }

    /// The display label of the parameter the server marks active (the one the
    /// caret is currently typing), clamped into range. The GUI bolds every
    /// occurrence of this text inside the signature label.
    pub fn active_param_label(&self) -> Option<&str> {
        self.active()?
            .parameters
            .get(self.active_parameter)
            .map(|p| p.label.as_str())
    }
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

/// One decoded semantic token at absolute document coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspSemanticToken {
    /// 0-based line.
    pub line: usize,
    /// 0-based start column (in the encoding the server declared).
    pub start_char: usize,
    pub length: usize,
    /// Legend-resolved type name (e.g. "function", "variable").
    pub token_type: String,
    /// Active modifier names (e.g. "declaration", "deprecated").
    pub modifiers: Vec<String>,
}

/// A text edit from `textDocument/formatting` (or `rangeFormatting`).
#[derive(Debug, Clone)]
pub struct LspTextEdit {
    pub start_line: usize,
    pub start_char: usize,
    pub end_line: usize,
    pub end_char: usize,
    pub new_text: String,
}

/// A per-file group of edits inside a `WorkspaceEdit` (rename / code action).
#[derive(Debug, Clone)]
pub struct LspFileEdits {
    pub file: PathBuf,
    pub edits: Vec<LspTextEdit>,
}

/// A parsed LSP `WorkspaceEdit`: the set of file changes a rename or code
/// action applies. A semantic rename frequently touches many files, so this
/// normalizes both wire shapes (`changes` keyed by URI and `documentChanges`
/// as an array) into one list of per-file edits. Ordering *within* a file is
/// handled by [`apply_text_edits`].
#[derive(Debug, Clone, Default)]
pub struct LspWorkspaceEdit {
    pub changes: Vec<LspFileEdits>,
}

impl LspWorkspaceEdit {
    /// True when no file carries any edit.
    pub fn is_empty(&self) -> bool {
        self.changes.iter().all(|c| c.edits.is_empty())
    }
    /// Total edits summed across every touched file.
    pub fn total_edits(&self) -> usize {
        self.changes.iter().map(|c| c.edits.len()).sum()
    }
}

/// A server-side command reference from a CodeAction's `command` field. When a
/// code action carries no `workspaceEdit`, applying it requires the client to
/// send `workspace/executeCommand` with this `command` (and optional
/// `arguments`); the server then usually pushes the edits back via a
/// `workspace/applyEdit` request.
#[derive(Debug, Clone)]
pub struct LspCommand {
    pub command: String,
    pub arguments: Option<Value>,
}

/// A code action (quick fix or refactoring) from `textDocument/codeAction`.
#[derive(Debug, Clone)]
pub struct LspCodeAction {
    pub title: String,
    /// Kind string, e.g. `quickfix` or `refactor.extract`.
    pub kind: String,
    /// Direct edits, when the action carries a `workspaceEdit`. Actions that
    /// only name a server-side `command` yield `None` here (the client cannot
    /// apply them without a follow-up request). Servers with
    /// `resolveProvider` may leave this `None` until the action is resolved.
    pub edit: Option<LspWorkspaceEdit>,
    /// Whether the server marked this the preferred fix for the diagnostic.
    pub is_preferred: bool,
    /// The command to run when the action has no inline `edit` (or as a
    /// follow-up even when it does). Populated from the CodeAction's `command`.
    pub command: Option<LspCommand>,
    /// The action's original JSON object, kept verbatim so an unresolved
    /// action can be echoed back to the server via `codeAction/resolve`
    /// (the spec requires the full object, including its opaque `data`).
    pub raw: Option<Value>,
}

/// Standard LSP semantic token types (client-declared support set; the
/// authoritative order always comes from the server's legend).
const STANDARD_TOKEN_TYPES: &[&str] = &[
    "namespace",
    "type",
    "class",
    "enum",
    "interface",
    "struct",
    "typeParameter",
    "parameter",
    "variable",
    "property",
    "enumMember",
    "event",
    "function",
    "method",
    "macro",
    "keyword",
    "modifier",
    "comment",
    "string",
    "number",
    "regexp",
    "operator",
    "decorator",
];

/// Standard LSP semantic token modifiers.
const STANDARD_TOKEN_MODIFIERS: &[&str] = &[
    "declaration",
    "definition",
    "readonly",
    "static",
    "deprecated",
    "abstract",
    "async",
    "modification",
    "documentation",
    "defaultLibrary",
];

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
                requests: Vec::new(),
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

    /// Drain server→client *requests* that have arrived (e.g.
    /// `workspace/applyEdit`). The caller must answer each with [`Self::respond`].
    pub fn take_requests(&self) -> Vec<(i64, String, Value)> {
        match self.inbox.lock() {
            Ok(mut inbox) => std::mem::take(&mut inbox.requests),
            Err(_) => Vec::new(),
        }
    }

    /// Answer a server→client request with a `result`. Encoded via
    /// [`encode_jsonrpc_response`] so the wire format stays unit-testable.
    pub fn respond(&mut self, id: i64, result: Value) -> Result<(), String> {
        let frame = encode_jsonrpc_response(id, result);
        if let Some(ref mut child) = self.process {
            if let Some(ref mut stdin) = child.stdin {
                stdin
                    .write_all(frame.as_bytes())
                    .map_err(|e| e.to_string())?;
                stdin.flush().map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    /// Send the initialize request, then *await* the result (bounded) so the
    /// server's advertised capabilities — including the semantic-token legend
    /// needed to decode token streams — are recorded before any document
    /// request is made. Finishes with the required `initialized` notification.
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
                    "declaration": {},
                    "references": {},
                    "rename": { "prepareSupport": true },
                    "publishDiagnostics": { "relatedInformation": true },
                    "codeAction": {
                        "codeActionLiteralSupport": {
                            "codeActionKind": {
                                "valueSet": [
                                    "quickfix",
                                    "refactor",
                                    "refactor.extract",
                                    "refactor.inline",
                                    "refactor.rewrite",
                                    "source",
                                    "source.organizeImports"
                                ]
                            }
                        },
                        "isPreferredSupport": true,
                        // We can echo an action back via `codeAction/resolve`;
                        // no extra properties are required to resolve.
                        "resolveSupport": { "properties": ["edit", "command"] }
                    },
                    "semanticTokens": {
                        "requests": { "full": true, "range": false },
                        "tokenTypes": STANDARD_TOKEN_TYPES,
                        "tokenModifiers": STANDARD_TOKEN_MODIFIERS,
                        "formats": ["relative"],
                        "overlappingTokenSupport": false,
                        "multilineTokenSupport": true
                    },
                    "formatting": {},
                    "rangeFormatting": {}
                },
                "workspace": {
                    "executeCommand": {},
                    "symbol": {},
                    "workspaceEdit": { "documentChanges": true }
                }
            }
        });
        let id = self.send_request("initialize", params)?;
        // Bounded await: a slow or missing server just leaves capabilities at
        // their defaults instead of stalling editor startup.
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if let Some(resp) = self.take_response(id) {
                self.apply_server_capabilities(&resp);
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let _ = self.send_notification("initialized", serde_json::json!({}));
        self.initialized = true;
        Ok(())
    }

    /// Record the capability flags from an `InitializeResult` response.
    fn apply_server_capabilities(&mut self, resp: &Value) {
        fn provider_present(v: Option<&Value>) -> bool {
            matches!(v, Some(Value::Bool(true)) | Some(Value::Object(_)))
        }
        fn string_list(v: Option<&Value>) -> Vec<String> {
            v.and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        }
        // The inbox stores the full JSON-RPC message; unwrap `result`.
        let caps = &resp["result"]["capabilities"];
        self.capabilities.completion = provider_present(caps.get("completionProvider"));
        self.capabilities.hover = provider_present(caps.get("hoverProvider"));
        self.capabilities.definition = provider_present(caps.get("definitionProvider"));
        self.capabilities.declaration = provider_present(caps.get("declarationProvider"));
        self.capabilities.type_definition = provider_present(caps.get("typeDefinitionProvider"));
        self.capabilities.implementation = provider_present(caps.get("implementationProvider"));
        self.capabilities.references = provider_present(caps.get("referencesProvider"));
        self.capabilities.rename = provider_present(caps.get("renameProvider"));
        self.capabilities.document_symbols = provider_present(caps.get("documentSymbolProvider"));
        self.capabilities.workspace_symbol = provider_present(caps.get("workspaceSymbolProvider"));
        self.capabilities.formatting = provider_present(caps.get("documentFormattingProvider"));
        self.capabilities.code_action = provider_present(caps.get("codeActionProvider"));
        self.capabilities.code_action_resolve = caps
            .get("codeActionProvider")
            .and_then(|p| p.get("resolveProvider"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        self.capabilities.execute_command = caps.get("executeCommandProvider").is_some_and(|p| {
            p.get("commands")
                .and_then(|c| c.as_array())
                .is_some_and(|a| !a.is_empty())
        });
        if let Some(sh) = caps.get("signatureHelpProvider") {
            self.capabilities.signature_help = provider_present(Some(sh));
            self.capabilities.signature_trigger_chars = string_list(sh.get("triggerCharacters"));
        }
        self.capabilities.call_hierarchy = provider_present(caps.get("callHierarchyProvider"));
        self.capabilities.selection_range = provider_present(caps.get("selectionRangeProvider"));
        if let Some(st) = caps.get("semanticTokensProvider") {
            self.capabilities.semantic_tokens = provider_present(Some(st));
            let legend = st.get("legend");
            self.capabilities.semantic_token_types =
                string_list(legend.and_then(|l| l.get("tokenTypes")));
            self.capabilities.semantic_token_modifiers =
                string_list(legend.and_then(|l| l.get("tokenModifiers")));
        }
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

    /// Notify the server that a document was closed.
    pub fn did_close(&mut self, path: &Path) -> Result<(), String> {
        let uri = path_to_uri(path);
        self.send_notification(
            "textDocument/didClose",
            serde_json::json!({
                "textDocument": { "uri": uri }
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

    /// Request go-to-declaration (the header declaration of a symbol, for
    /// languages where declaration and definition differ).
    pub fn goto_declaration(
        &mut self,
        path: &Path,
        line: usize,
        col: usize,
    ) -> Result<i64, String> {
        let uri = path_to_uri(path);
        self.send_request(
            "textDocument/declaration",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": col }
            }),
        )
    }

    /// Request go-to-type-definition (the type of the symbol at a position).
    pub fn goto_type_definition(
        &mut self,
        path: &Path,
        line: usize,
        col: usize,
    ) -> Result<i64, String> {
        let uri = path_to_uri(path);
        self.send_request(
            "textDocument/typeDefinition",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": col }
            }),
        )
    }

    /// Request go-to-implementation (concrete impls of an interface member).
    pub fn goto_implementation(
        &mut self,
        path: &Path,
        line: usize,
        col: usize,
    ) -> Result<i64, String> {
        let uri = path_to_uri(path);
        self.send_request(
            "textDocument/implementation",
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

    /// `textDocument/prepareCallHierarchy` — resolve the callable symbol at a
    /// position into one or more `CallHierarchyItem`s. The first returned item
    /// is echoed back into an incoming/outgoing-calls request.
    pub fn prepare_call_hierarchy(
        &mut self,
        path: &Path,
        line: usize,
        col: usize,
    ) -> Result<i64, String> {
        let uri = path_to_uri(path);
        self.send_request(
            "textDocument/prepareCallHierarchy",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": col }
            }),
        )
    }

    /// `callHierarchy/incomingCalls` — the callers of `item` (who calls it).
    pub fn incoming_calls(&mut self, item: &Value) -> Result<i64, String> {
        self.send_request(
            "callHierarchy/incomingCalls",
            serde_json::json!({ "item": item }),
        )
    }

    /// `callHierarchy/outgoingCalls` — the callees of `item` (what it calls).
    pub fn outgoing_calls(&mut self, item: &Value) -> Result<i64, String> {
        self.send_request(
            "callHierarchy/outgoingCalls",
            serde_json::json!({ "item": item }),
        )
    }

    /// `textDocument/selectionRange` — the semantic ranges enclosing each of
    /// `positions` (line, character). We request a single caret position; the
    /// server replies with a nested chain from the tightest node outward.
    pub fn selection_range(
        &mut self,
        path: &Path,
        positions: &[(usize, usize)],
    ) -> Result<i64, String> {
        let uri = path_to_uri(path);
        let pos: Vec<Value> = positions
            .iter()
            .map(|(l, c)| serde_json::json!({ "line": l, "character": c }))
            .collect();
        self.send_request(
            "textDocument/selectionRange",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "positions": pos
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

    /// Request signature help (parameter hints) at a position. When
    /// `trigger_char` is `Some` the request is sent as a TriggerCharacter
    /// (`triggerKind` 2); otherwise it is an Invoked request (`triggerKind` 1).
    pub fn signature_help(
        &mut self,
        path: &Path,
        line: usize,
        col: usize,
        trigger_char: Option<&str>,
    ) -> Result<i64, String> {
        let uri = path_to_uri(path);
        let trigger_kind = if trigger_char.is_some() { 2 } else { 1 };
        let mut context = serde_json::json!({ "triggerKind": trigger_kind });
        if let Some(c) = trigger_char {
            context["triggerCharacter"] = Value::String(c.to_string());
        }
        self.send_request(
            "textDocument/signatureHelp",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": col },
                "context": context
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

    /// Request a workspace-wide symbol search (`workspace/symbol`). Unlike the
    /// `textDocument/*` requests this is not scoped to a document — the server
    /// fuzzy-matches `query` against every symbol it has indexed. Fire it with
    /// [`LspManager::request_workspace_symbols`] and collect the answer with
    /// [`LspManager::poll_workspace_symbols`]; never blocks the caller.
    pub fn workspace_symbol(&mut self, query: &str) -> Result<i64, String> {
        self.send_request("workspace/symbol", serde_json::json!({ "query": query }))
    }

    /// Request the full semantic token stream for a document.
    pub fn semantic_tokens(&mut self, path: &Path) -> Result<i64, String> {
        let uri = path_to_uri(path);
        self.send_request(
            "textDocument/semanticTokens/full",
            serde_json::json!({
                "textDocument": { "uri": uri }
            }),
        )
    }

    /// Request formatting edits for a whole document.
    pub fn formatting(
        &mut self,
        path: &Path,
        tab_size: u64,
        insert_spaces: bool,
    ) -> Result<i64, String> {
        let uri = path_to_uri(path);
        self.send_request(
            "textDocument/formatting",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "options": { "tabSize": tab_size, "insertSpaces": insert_spaces }
            }),
        )
    }

    /// Request a workspace-wide semantic rename of the symbol at `position`.
    /// The server replies with a `WorkspaceEdit` describing every occurrence
    /// to change (across files), which the client applies — the safe way to
    /// rename, since the server resolves shadowing, imports, and references.
    pub fn rename(
        &mut self,
        path: &Path,
        line: usize,
        col: usize,
        new_name: &str,
    ) -> Result<i64, String> {
        let uri = path_to_uri(path);
        self.send_request(
            "textDocument/rename",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": col },
                "newName": new_name
            }),
        )
    }

    /// Request the code actions (quick fixes / refactorings) offered for a
    /// range. `diagnostics` are the LSP diagnostic objects covering the range
    /// (empty is valid — servers then offer kind-based refactorings only).
    #[allow(clippy::too_many_arguments)]
    pub fn code_action(
        &mut self,
        path: &Path,
        start_line: usize,
        start_char: usize,
        end_line: usize,
        end_char: usize,
        diagnostics: Vec<Value>,
    ) -> Result<i64, String> {
        let uri = path_to_uri(path);
        self.send_request(
            "textDocument/codeAction",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": start_line, "character": start_char },
                    "end": { "line": end_line, "character": end_char }
                },
                "context": { "diagnostics": diagnostics }
            }),
        )
    }

    /// Resolve a lazily-computed code action via `codeAction/resolve`. The
    /// params are the action's original JSON object verbatim (spec: the
    /// server may rely on its opaque `data` to identify the action). Returns
    /// the request id; the caller awaits and re-parses the result.
    pub fn resolve_code_action(&mut self, action: &Value) -> Result<i64, String> {
        self.send_request("codeAction/resolve", action.clone())
    }

    /// Run a server-side command via `workspace/executeCommand`. Returns the
    /// request id; the caller awaits the response while also draining any
    /// `workspace/applyEdit` requests the server pushes back.
    pub fn execute_command(
        &mut self,
        command: &str,
        arguments: Option<Value>,
    ) -> Result<i64, String> {
        self.send_request(
            "workspace/executeCommand",
            serde_json::json!({
                "command": command,
                "arguments": arguments.unwrap_or_else(|| serde_json::json!([]))
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
    /// Accumulated `window/showMessage` notifications awaiting surfacing. The
    /// GUI drains these into toasts each poll cycle.
    pub messages: Vec<LspMessage>,
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
        // Check for Python project
        if workspace_root.join("pyproject.toml").exists()
            || workspace_root.join("setup.py").exists()
            || workspace_root.join("requirements.txt").exists()
        {
            mgr.register(LspServerConfig::python(workspace_root), workspace_root);
        }
        // Check for Go project
        if workspace_root.join("go.mod").exists() {
            mgr.register(LspServerConfig::go(workspace_root), workspace_root);
        }
        // Check for C/C++ project
        if workspace_root.join("compile_commands.json").exists()
            || workspace_root.join("CMakeLists.txt").exists()
            || workspace_root.join(".clangd").exists()
        {
            mgr.register(LspServerConfig::clangd(workspace_root), workspace_root);
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
                } else if method == "window/showMessage" {
                    // Server status line (no file/location). Accumulate so the
                    // GUI can surface it as a toast instead of discarding it.
                    if let Some(msg) = parse_show_message(&params) {
                        self.messages.push(msg);
                    }
                }
                // Handle other notifications (e.g. progress) in the future
            }
            // Answer any server→client requests that landed while idle, so a
            // server that queries configuration or attempts (un)registration
            // is never left blocked on a reply that would never come.
            for (rid, method, params) in server.take_requests() {
                let result = server_request_result(&method, &params);
                let _ = server.respond(rid, result);
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
                if let Err(e) = server.did_change(path, content, next_version) {
                    log::warn!("LSP did_change failed: {}", e);
                }
            } else if let Err(e) = server.did_open(path, content, &lang) {
                log::warn!("LSP did_open failed: {}", e);
            }
        }
        if !already {
            self.open_docs.insert(path.to_path_buf());
        }
        self.doc_versions.insert(path.to_path_buf(), next_version);
    }

    /// Notify the matching language server that a document was closed.
    /// Removes the path from the open-docs tracking set.
    pub fn close_document(&mut self, ext: &str, path: &Path) {
        if !self.open_docs.contains(path) {
            return;
        }
        if let Some(server) = self.server_for_extension(ext) {
            if let Err(e) = server.did_close(path) {
                log::warn!("LSP did_close failed: {}", e);
            }
        }
        self.open_docs.remove(path);
        self.doc_versions.remove(path);
    }

    /// Dispatch a non-blocking workspace-wide symbol search against the server
    /// matching `ext`, gated on its `workspaceSymbolProvider` capability so
    /// unadvertised servers are never asked. Returns the request id to poll
    /// with [`Self::poll_workspace_symbols`], or `None` when no capable server
    /// exists for the language.
    pub fn request_workspace_symbols(&mut self, ext: &str, query: &str) -> Option<i64> {
        let server = self.server_for_extension(ext)?;
        if !server.capabilities.workspace_symbol {
            return None;
        }
        server.workspace_symbol(query).ok()
    }

    /// Non-blocking collection of a [`Self::request_workspace_symbols`] answer:
    /// `Some(results)` once the response lands, `None` while still pending.
    /// A late result for a request the caller forgot simply stays in the inbox
    /// keyed by id, the same contract the bounded `await_response` relies on.
    pub fn poll_workspace_symbols(
        &mut self,
        ext: &str,
        id: i64,
    ) -> Option<Vec<LspWorkspaceSymbol>> {
        let resp = self.server_for_extension(ext)?.take_response(id)?;
        Some(parse_workspace_symbols(&resp))
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

    /// Go-to-declaration: the location(s) of the symbol's *declaration* (e.g.
    /// the C/C++ header prototype behind a definition). Gated on
    /// `declarationProvider` so servers without it are never asked; same
    /// degrade-to-empty contract as [`Self::definition`].
    pub fn declaration(
        &mut self,
        ext: &str,
        path: &Path,
        line: usize,
        col: usize,
        content: &str,
    ) -> Vec<LspLocation> {
        self.sync_document(ext, path, content);
        let id = match self.server_for_extension(ext) {
            Some(s) if s.capabilities.declaration => s.goto_declaration(path, line, col),
            _ => return Vec::new(),
        };
        let Ok(id) = id else { return Vec::new() };
        match self.await_response(ext, id) {
            Some(resp) => parse_definition(&resp),
            None => Vec::new(),
        }
    }

    /// Go-to-type-definition: the location(s) of the *type* of the symbol at a
    /// position. Same degrade-to-empty contract as [`Self::definition`].
    pub fn type_definition(
        &mut self,
        ext: &str,
        path: &Path,
        line: usize,
        col: usize,
        content: &str,
    ) -> Vec<LspLocation> {
        self.sync_document(ext, path, content);
        let id = match self.server_for_extension(ext) {
            Some(s) => s.goto_type_definition(path, line, col),
            None => return Vec::new(),
        };
        let Ok(id) = id else { return Vec::new() };
        match self.await_response(ext, id) {
            Some(resp) => parse_definition(&resp),
            None => Vec::new(),
        }
    }

    /// Go-to-implementation: concrete implementation(s) of the interface /
    /// abstract member at a position. Same contract as [`Self::definition`].
    pub fn implementation(
        &mut self,
        ext: &str,
        path: &Path,
        line: usize,
        col: usize,
        content: &str,
    ) -> Vec<LspLocation> {
        self.sync_document(ext, path, content);
        let id = match self.server_for_extension(ext) {
            Some(s) => s.goto_implementation(path, line, col),
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

    /// Whether a server is registered for `ext` *and* advertises
    /// `callHierarchyProvider`. Cheap gate so the editor never issues
    /// call-hierarchy requests for languages without a provider.
    pub fn supports_call_hierarchy(&mut self, ext: &str) -> bool {
        self.server_for_extension(ext)
            .map(|s| s.capabilities.call_hierarchy)
            .unwrap_or(false)
    }

    /// Resolve the callable at a position to its raw `CallHierarchyItem` (the
    /// first one the server returns). Returns `None` when there is no provider
    /// or the position is not on a callable symbol. The item is echoed back
    /// into an incoming/outgoing-calls request, so it is kept as raw JSON.
    fn prepare_call_item(
        &mut self,
        ext: &str,
        path: &Path,
        line: usize,
        col: usize,
        content: &str,
    ) -> Option<Value> {
        self.sync_document(ext, path, content);
        if !self.supports_call_hierarchy(ext) {
            return None;
        }
        let id = self
            .server_for_extension(ext)?
            .prepare_call_hierarchy(path, line, col)
            .ok()?;
        let resp = self.await_response(ext, id)?;
        let first = resp.get("result")?.as_array()?.first()?;
        if first.is_object() {
            Some(first.clone())
        } else {
            None
        }
    }

    /// Incoming calls: the functions that call the symbol at a position.
    /// Degrades to an empty vec when unsupported or not on a callable.
    pub fn incoming_calls(
        &mut self,
        ext: &str,
        path: &Path,
        line: usize,
        col: usize,
        content: &str,
    ) -> Vec<LspCallHierarchyItem> {
        let Some(item) = self.prepare_call_item(ext, path, line, col, content) else {
            return Vec::new();
        };
        let id = match self.server_for_extension(ext) {
            Some(s) => s.incoming_calls(&item),
            None => return Vec::new(),
        };
        let Ok(id) = id else { return Vec::new() };
        match self.await_response(ext, id) {
            Some(resp) => parse_incoming_calls(&resp),
            None => Vec::new(),
        }
    }

    /// Outgoing calls: the functions called by the symbol at a position.
    pub fn outgoing_calls(
        &mut self,
        ext: &str,
        path: &Path,
        line: usize,
        col: usize,
        content: &str,
    ) -> Vec<LspCallHierarchyItem> {
        let Some(item) = self.prepare_call_item(ext, path, line, col, content) else {
            return Vec::new();
        };
        let id = match self.server_for_extension(ext) {
            Some(s) => s.outgoing_calls(&item),
            None => return Vec::new(),
        };
        let Ok(id) = id else { return Vec::new() };
        match self.await_response(ext, id) {
            Some(resp) => parse_outgoing_calls(&resp),
            None => Vec::new(),
        }
    }

    /// Whether a server is registered for `ext` *and* advertises
    /// `selectionRangeProvider`. Cheap gate so the editor never issues
    /// smart-selection requests for languages without a provider.
    pub fn supports_selection_range(&mut self, ext: &str) -> bool {
        self.server_for_extension(ext)
            .map(|s| s.capabilities.selection_range)
            .unwrap_or(false)
    }

    /// Semantic selection ranges enclosing a position, flattened innermost
    /// (tightest) to outermost (whole file). Empty when unsupported or the
    /// request fails — the caller then leaves the selection untouched.
    pub fn selection_ranges(
        &mut self,
        ext: &str,
        path: &Path,
        line: usize,
        col: usize,
        content: &str,
    ) -> Vec<LspSelectionRange> {
        self.sync_document(ext, path, content);
        if !self.supports_selection_range(ext) {
            return Vec::new();
        }
        let id = match self.server_for_extension(ext) {
            Some(s) => s.selection_range(path, &[(line, col)]),
            None => return Vec::new(),
        };
        let Ok(id) = id else { return Vec::new() };
        match self.await_response(ext, id) {
            Some(resp) => parse_selection_ranges(&resp),
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

    /// Signature help (parameter hints) at a position. Returns `None` when no
    /// server is registered for `ext`, the server has not advertised a
    /// `signatureHelpProvider`, the request fails, or it times out.
    pub fn signature_help(
        &mut self,
        ext: &str,
        path: &Path,
        line: usize,
        col: usize,
        content: &str,
        trigger_char: Option<&str>,
    ) -> Option<LspSignatureHelp> {
        self.sync_document(ext, path, content);
        if !self.supports_signature_help(ext) {
            return None;
        }
        let s = self.server_for_extension(ext)?;
        let id = s.signature_help(path, line, col, trigger_char);
        let Ok(id) = id else { return None };
        self.await_response(ext, id)
            .and_then(|resp| parse_signature_help(&resp))
    }

    /// Whether a server is registered for `ext` *and* advertises signature
    /// help. Cheap gate so the editor never issues parameter-hint requests for
    /// languages without a provider.
    pub fn supports_signature_help(&mut self, ext: &str) -> bool {
        self.server_for_extension(ext)
            .map(|s| s.capabilities.signature_help)
            .unwrap_or(false)
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

    /// Semantic tokens for a document, decoded to absolute positions with
    /// legend-resolved type names. Empty when the server has not advertised a
    /// `semanticTokensProvider`, is not running, or times out — scope-based
    /// highlighting remains the fallback. This is what lets the editor match
    /// the semantic coloring depth external editors get from LSP.
    pub fn semantic_tokens(
        &mut self,
        ext: &str,
        path: &Path,
        content: &str,
    ) -> Vec<LspSemanticToken> {
        self.sync_document(ext, path, content);
        let id = match self.server_for_extension(ext) {
            Some(s) => {
                if !s.capabilities.semantic_tokens {
                    return Vec::new();
                }
                s.semantic_tokens(path)
            }
            None => return Vec::new(),
        };
        let Ok(id) = id else { return Vec::new() };
        let legend = self
            .server_for_extension(ext)
            .map(|s| {
                (
                    s.capabilities.semantic_token_types.clone(),
                    s.capabilities.semantic_token_modifiers.clone(),
                )
            })
            .unwrap_or_default();
        match self.await_response(ext, id) {
            Some(resp) => parse_semantic_tokens(&resp, &legend.0, &legend.1),
            None => Vec::new(),
        }
    }

    /// Whether a server is registered for `ext` *and* has advertised a
    /// semantic-tokens provider. Cheap gate so the render loop never issues
    /// token requests for languages without one.
    pub fn supports_semantic_tokens(&mut self, ext: &str) -> bool {
        self.server_for_extension(ext)
            .map(|s| s.capabilities.semantic_tokens)
            .unwrap_or(false)
    }

    /// Non-blocking half of the highlight pipeline: sync the document and
    /// issue a `semanticTokens/full` request, returning its id (or `None` if
    /// there is no provider). Poll the result later with
    /// [`Self::take_semantic_tokens`] so the UI thread never blocks on the
    /// language server.
    pub fn request_semantic_tokens(
        &mut self,
        ext: &str,
        path: &Path,
        content: &str,
    ) -> Option<i64> {
        self.sync_document(ext, path, content);
        let s = self.server_for_extension(ext)?;
        if !s.capabilities.semantic_tokens {
            return None;
        }
        s.semantic_tokens(path).ok()
    }

    /// Non-blocking counterpart to [`Self::request_semantic_tokens`]: if the
    /// reply for `id` has arrived, decode it through the server legend and
    /// return `Some(tokens)` (possibly empty). Returns `None` while still
    /// pending so the caller can keep waiting.
    pub fn take_semantic_tokens(&mut self, ext: &str, id: i64) -> Option<Vec<LspSemanticToken>> {
        let resp = self.server_for_extension(ext)?.take_response(id)?;
        let legend = self
            .server_for_extension(ext)
            .map(|s| {
                (
                    s.capabilities.semantic_token_types.clone(),
                    s.capabilities.semantic_token_modifiers.clone(),
                )
            })
            .unwrap_or_default();
        Some(parse_semantic_tokens(&resp, &legend.0, &legend.1))
    }

    /// Run the server's whole-document formatter and return the formatted
    /// text. `None` when the server has no formatting provider, the request
    /// fails or times out, or the formatter proposed no edits (the document
    /// is already formatted). Callers replace the buffer and push the result
    /// back through [`Self::sync_document`] as a user edit.
    pub fn format_document(
        &mut self,
        ext: &str,
        path: &Path,
        content: &str,
        tab_size: u64,
        insert_spaces: bool,
    ) -> Option<String> {
        self.sync_document(ext, path, content);
        let s = self.server_for_extension(ext)?;
        if !s.capabilities.formatting {
            return None;
        }
        let id = s.formatting(path, tab_size, insert_spaces).ok()?;
        let resp = self.await_response(ext, id)?;
        let edits = parse_formatting(&resp);
        if edits.is_empty() {
            return None;
        }
        Some(apply_text_edits(content, &edits))
    }

    /// Semantic rename at a position through the language server
    /// (`textDocument/rename`). Returns the parsed `WorkspaceEdit` (files →
    /// edits) spanning every affected file, or `None` when the server lacks a
    /// rename provider, the request fails or times out, or the server proposes
    /// no changes. Callers apply it with [`apply_workspace_edit`], reconciling
    /// open buffers and on-disk files by their current content.
    pub fn rename_symbol(
        &mut self,
        ext: &str,
        path: &Path,
        line: usize,
        col: usize,
        new_name: &str,
        content: &str,
    ) -> Option<LspWorkspaceEdit> {
        self.sync_document(ext, path, content);
        let s = self.server_for_extension(ext)?;
        if !s.capabilities.rename {
            return None;
        }
        let id = s.rename(path, line, col, new_name).ok()?;
        let resp = self.await_response(ext, id)?;
        let edit = parse_workspace_edit(&resp);
        if edit.is_empty() {
            None
        } else {
            Some(edit)
        }
    }

    /// Code actions (quick fixes / refactorings) offered for a range, given as
    /// `((start_line, start_char), (end_line, end_char))` (0-based). Degrades
    /// to an empty vec when the server has no `codeActionProvider` or times
    /// out. Diagnostics are passed as empty so servers reply with the
    /// range-independent refactorings they can offer without a live diagnostic.
    pub fn code_actions(
        &mut self,
        ext: &str,
        path: &Path,
        range: ((usize, usize), (usize, usize)),
        content: &str,
    ) -> Vec<LspCodeAction> {
        self.sync_document(ext, path, content);
        let id = match self.server_for_extension(ext) {
            Some(s) if s.capabilities.code_action => s.code_action(
                path,
                range.0 .0,
                range.0 .1,
                range.1 .0,
                range.1 .1,
                Vec::new(),
            ),
            _ => return Vec::new(),
        };
        let Ok(id) = id else { return Vec::new() };
        match self.await_response(ext, id) {
            Some(resp) => parse_code_actions(&resp),
            None => Vec::new(),
        }
    }

    /// Complete a code action the server returned unresolved (a
    /// `resolveProvider` server sends lightweight actions and fills in the
    /// `edit`/`command` on demand — rust-analyzer's expensive refactorings
    /// work this way). Returns `None` when the server can't resolve, has no
    /// raw object to echo back, or the follow-up request fails; the caller
    /// then keeps the original action so its title stays visible in the UI.
    pub fn resolve_code_action(
        &mut self,
        ext: &str,
        action: &LspCodeAction,
    ) -> Option<LspCodeAction> {
        let raw = action.raw.clone()?;
        let id = match self.server_for_extension(ext) {
            Some(s) if s.capabilities.code_action_resolve => s.resolve_code_action(&raw),
            _ => return None,
        };
        let Ok(id) = id else { return None };
        let resp = self.await_response(ext, id)?;
        let resolved = resp.get("result").filter(|v| !v.is_null())?;
        let mut out = parse_one_code_action(resolved)?;
        // A resolve that mysteriously drops fields must not lose information
        // the original carried: keep the more actionable variant.
        if out.edit.is_none() && out.command.is_none() {
            return Some(action.clone());
        }
        out.is_preferred = action.is_preferred;
        Some(out)
    }

    /// Run a code action that names a server-side `command` (no inline edit).
    /// Sends `workspace/executeCommand`, then awaits the reply while answering
    /// any `workspace/applyEdit` request the server pushes back — accumulating
    /// those edits so the caller applies them exactly like an inline-edit
    /// action. Returns `None` when the server is unavailable, does not advertise
    /// `executeCommandProvider`, or produces no edits.
    pub fn execute_code_action(
        &mut self,
        ext: &str,
        path: &Path,
        command: &str,
        arguments: Option<Value>,
        content: &str,
    ) -> Option<LspWorkspaceEdit> {
        self.sync_document(ext, path, content);
        let s = self.server_for_extension(ext)?;
        if !s.capabilities.execute_command {
            return None;
        }
        let id = s.execute_command(command, arguments).ok()?;
        let mut accumulated = LspWorkspaceEdit::default();
        let mut got_result = false;
        let deadline = Instant::now() + Duration::from_millis(3000);
        loop {
            let server = self.server_for_extension(ext)?;
            // Answer every server→client request that has landed.
            for (rid, method, params) in server.take_requests() {
                if method == "workspace/applyEdit" {
                    let edit = params.get("edit").cloned().unwrap_or(Value::Null);
                    let parsed = parse_workspace_edit_value(&edit);
                    accumulated.changes.extend(parsed.changes);
                    let _ = server.respond(rid, serde_json::json!({ "applied": true }));
                } else {
                    let _ = server.respond(rid, Value::Null);
                }
            }
            if server.take_response(id).is_some() {
                got_result = true;
            }
            if got_result || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        accumulated.changes.sort_by(|a, b| a.file.cmp(&b.file));
        if accumulated.is_empty() {
            None
        } else {
            Some(accumulated)
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

// ---------------------------------------------------------------------------
// LSP Stdout Reader Thread
// ---------------------------------------------------------------------------

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

        // Classify: response (reply to our request) vs server→client request
        // (has an `id` *and* a `method`, e.g. workspace/applyEdit) vs a plain
        // notification. Requests must be kept apart so the caller can answer
        // them; mis-filing one as a response would silently drop it.
        match classify_message(&json) {
            MessageKind::Response => {
                if let Some(id) = json.get("id").and_then(|v| v.as_i64()) {
                    if let Ok(mut inbox) = inbox.lock() {
                        inbox.responses.insert(id, json);
                    }
                }
            }
            MessageKind::Request => {
                let id = json.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
                let method = json["method"].as_str().unwrap_or("").to_string();
                let params = json.get("params").cloned().unwrap_or(Value::Null);
                if let Ok(mut inbox) = inbox.lock() {
                    inbox.requests.push((id, method, params));
                }
            }
            MessageKind::Notification => {
                let method = json["method"].as_str().unwrap_or("").to_string();
                let params = json.get("params").cloned().unwrap_or(Value::Null);
                if let Ok(mut inbox) = inbox.lock() {
                    inbox.notifications.push((method, params));
                }
            }
            MessageKind::Unknown => {}
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

/// Parse a `window/showMessage` notification (`{type, message}`) into an
/// [`LspMessage`]. MessageType is 1=Error, 2=Warning, 3=Info, 4=Log; an absent
/// or unrecognized type falls back to Info. A missing or empty `message` yields
/// `None` so blank status lines never occupy the toast area.
pub fn parse_show_message(params: &Value) -> Option<LspMessage> {
    let message = params.get("message")?.as_str()?;
    if message.is_empty() {
        return None;
    }
    let severity = match params.get("type").and_then(|v| v.as_u64()).unwrap_or(3) {
        1 => DiagnosticSeverity::Error,
        2 => DiagnosticSeverity::Warning,
        3 => DiagnosticSeverity::Info,
        4 => DiagnosticSeverity::Hint,
        _ => DiagnosticSeverity::Info,
    };
    Some(LspMessage {
        severity,
        message: message.to_string(),
    })
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

/// Build a [`LspCallHierarchyItem`] from an LSP `CallHierarchyItem` object
/// (`{ name, kind, location: { uri, range: { start } } }`).
fn call_item_from(obj: &Value) -> Option<LspCallHierarchyItem> {
    let name = obj.get("name")?.as_str()?.to_string();
    let kind = obj.get("kind").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
    let loc = obj.get("location")?;
    let uri = loc.get("uri")?.as_str()?;
    let start = loc.get("range")?.get("start")?;
    let line = start.get("line")?.as_u64()? as usize;
    let col = start.get("character")?.as_u64()? as usize;
    Some(LspCallHierarchyItem {
        name,
        kind,
        file: uri_to_path(uri),
        line,
        col,
    })
}

/// Parse a `prepareCallHierarchy` result (`CallHierarchyItem[]` or one item).
pub fn parse_call_hierarchy_items(v: &Value) -> Vec<LspCallHierarchyItem> {
    let result = match v.get("result") {
        Some(r) if !r.is_null() => r,
        _ => return Vec::new(),
    };
    if let Some(arr) = result.as_array() {
        arr.iter().filter_map(call_item_from).collect()
    } else {
        call_item_from(result).into_iter().collect()
    }
}

/// Shared edge parser: incoming calls carry the node under `from`, outgoing
/// calls under `to`; both are `CallHierarchyEdge { <key>: item, .. }`.
fn parse_call_edges(v: &Value, key: &str) -> Vec<LspCallHierarchyItem> {
    let result = match v.get("result") {
        Some(r) if !r.is_null() => r,
        _ => return Vec::new(),
    };
    let Some(arr) = result.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|edge| call_item_from(edge.get(key)?))
        .collect()
}

/// Parse `callHierarchy/incomingCalls` (`{ from, fromRanges }[]`).
pub fn parse_incoming_calls(v: &Value) -> Vec<LspCallHierarchyItem> {
    parse_call_edges(v, "from")
}

/// Parse `callHierarchy/outgoingCalls` (`{ to, ranges }[]`).
pub fn parse_outgoing_calls(v: &Value) -> Vec<LspCallHierarchyItem> {
    parse_call_edges(v, "to")
}

/// Build an [`LspSelectionRange`] from a `SelectionRange` node's `range` field.
fn sel_range_from(node: &Value) -> Option<LspSelectionRange> {
    let r = node.get("range")?;
    let s = r.get("start")?;
    let e = r.get("end")?;
    Some(LspSelectionRange {
        start_line: s.get("line")?.as_u64()? as usize,
        start_char: s.get("character")?.as_u64()? as usize,
        end_line: e.get("line")?.as_u64()? as usize,
        end_char: e.get("character")?.as_u64()? as usize,
    })
}

/// Walk a `SelectionRange` node chain (each node's `parent` is strictly wider),
/// pushing innermost → outermost into `out`.
fn flatten_selection_chain(node: &Value, out: &mut Vec<LspSelectionRange>) {
    if let Some(sr) = sel_range_from(node) {
        out.push(sr);
    }
    if let Some(parent) = node.get("parent") {
        if !parent.is_null() {
            flatten_selection_chain(parent, out);
        }
    }
}

/// Parse a `textDocument/selectionRange` response (`SelectionRange[]`, one
/// nested chain per requested position) into a flat innermost-first list.
pub fn parse_selection_ranges(v: &Value) -> Vec<LspSelectionRange> {
    let result = match v.get("result") {
        Some(r) if !r.is_null() => r,
        _ => return Vec::new(),
    };
    let mut out = Vec::new();
    if let Some(arr) = result.as_array() {
        for node in arr {
            flatten_selection_chain(node, &mut out);
        }
    } else {
        flatten_selection_chain(result, &mut out);
    }
    out.dedup();
    out
}

/// "Expand selection": the narrowest span that fully contains the current
/// `(cur_s, cur_e)` selection yet is strictly wider. `spans` are `(start, end)`
/// char offsets. Returns `None` when already at the outermost range.
pub fn wider_selection(
    spans: &[(usize, usize)],
    cur_s: usize,
    cur_e: usize,
) -> Option<(usize, usize)> {
    let cur_w = cur_e.saturating_sub(cur_s);
    let mut best: Option<(usize, usize)> = None;
    let mut best_w = usize::MAX;
    for &(s, e) in spans {
        if s <= cur_s && e >= cur_e {
            let w = e.saturating_sub(s);
            if w > cur_w && w < best_w {
                best = Some((s, e));
                best_w = w;
            }
        }
    }
    best
}

/// "Shrink selection": the widest span strictly contained in the current
/// `(cur_s, cur_e)` selection yet still narrower than it. Returns `None` when
/// already back at the innermost range.
pub fn narrower_selection(
    spans: &[(usize, usize)],
    cur_s: usize,
    cur_e: usize,
) -> Option<(usize, usize)> {
    let cur_w = cur_e.saturating_sub(cur_s);
    let mut best: Option<(usize, usize)> = None;
    let mut best_w = 0;
    for &(s, e) in spans {
        if s >= cur_s && e <= cur_e {
            let w = e.saturating_sub(s);
            if w < cur_w && w > best_w {
                best = Some((s, e));
                best_w = w;
            }
        }
    }
    best
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

/// Slice `label` by a LSP `[start, end]` utf16 range. Signature labels are
/// overwhelmingly ASCII, so treating the offsets as char indices is a safe
/// approximation for display; out-of-range values are clamped.
fn slice_label_range(label: &str, range: &[Value]) -> String {
    let chars: Vec<char> = label.chars().collect();
    let get = |v: &Value| -> usize {
        v.as_u64()
            .map(|x| (x as usize).min(chars.len()))
            .unwrap_or(0)
    };
    let start = range.first().map(get).unwrap_or(0);
    let end = range.get(1).map(get).unwrap_or(chars.len());
    if start >= end {
        return String::new();
    }
    chars[start..end].iter().collect()
}

/// Parse a `textDocument/signatureHelp` response. Returns `None` when the
/// result is null or carries no signatures. Normalizes both parameter-label
/// shapes LSP allows (a plain string, or a `[start, end]` range into the
/// signature label) and clamps the `active*` indices into range.
pub fn parse_signature_help(v: &Value) -> Option<LspSignatureHelp> {
    let result = v.get("result")?;
    if result.is_null() {
        return None;
    }
    let sigs_val = result.get("signatures")?.as_array()?;
    let mut signatures = Vec::new();
    for s in sigs_val {
        let label = s.get("label")?.as_str()?.to_string();
        let documentation = s
            .get("documentation")
            .and_then(extract_markup)
            .unwrap_or_default();
        let mut parameters = Vec::new();
        if let Some(ps) = s.get("parameters").and_then(|p| p.as_array()) {
            for p in ps {
                let plabel = match p.get("label") {
                    Some(Value::String(strs)) => strs.clone(),
                    Some(Value::Array(rng)) => slice_label_range(&label, rng),
                    _ => String::new(),
                };
                let pdoc = p
                    .get("documentation")
                    .and_then(extract_markup)
                    .unwrap_or_default();
                parameters.push(LspParameterInformation {
                    label: plabel,
                    documentation: pdoc,
                });
            }
        }
        signatures.push(LspSignature {
            label,
            documentation,
            parameters,
        });
    }
    if signatures.is_empty() {
        return None;
    }
    let active_signature = result
        .get("activeSignature")
        .and_then(|x| x.as_u64())
        .map(|x| (x as usize).min(signatures.len() - 1))
        .unwrap_or(0);
    let param_count = signatures[active_signature].parameters.len();
    let active_parameter = result
        .get("activeParameter")
        .and_then(|x| x.as_u64())
        .map(|x| (x as usize).min(param_count.saturating_sub(1)))
        .unwrap_or(0);
    Some(LspSignatureHelp {
        signatures,
        active_signature,
        active_parameter,
    })
}

/// Whether the caret sits inside an unclosed call parenthesis, given the text
/// before it. Drives signature-help dismissal: the popup stays up while the
/// innermost `(` is unmatched and drops as soon as the matching `)` is typed.
/// A plain depth scan (no string/comment elision) is the standard heuristic and
/// errs only by lingering a frame, never by crashing or mis-firing an edit.
pub fn caret_inside_parens(before: &str) -> bool {
    let mut depth = 0i32;
    for c in before.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = (depth - 1).max(0),
            _ => {}
        }
    }
    depth > 0
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
            let documentation = completion_documentation(it);
            let sort_key = it
                .get("sortText")
                .and_then(|x| x.as_str())
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(20);
            Some(CompletionItem {
                label,
                kind,
                detail,
                documentation,
                insert_text,
                sort_key,
            })
        })
        .collect()
}

/// Extract human-readable documentation from an LSP `CompletionItem`'s
/// `documentation` field, which per spec may be a plain string, a
/// `MarkupContent` object (`{ kind, value }`), or a legacy `MarkedString[]`
/// array (mixing bare strings and `{ language, value }` objects). Fragments are
/// joined with a blank line; an absent, null, or entirely-empty field yields
/// `None`. Kept as a pure function so the three shapes are unit-testable.
pub fn completion_documentation(item: &Value) -> Option<String> {
    let node = item.get("documentation")?;
    let parts: Vec<String> = match node {
        Value::String(s) => vec![s.clone()],
        Value::Object(_) => markup_value(node).into_iter().collect(),
        Value::Array(arr) => arr.iter().filter_map(markup_value).collect(),
        _ => Vec::new(),
    };
    let joined = parts
        .into_iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

/// Read the text out of a `MarkupContent` / `MarkedString`: either a bare
/// string or an object carrying a `value` field.
fn markup_value(node: &Value) -> Option<String> {
    match node {
        Value::String(s) => Some(s.clone()),
        Value::Object(_) => node.get("value").and_then(|v| v.as_str()).map(String::from),
        _ => None,
    }
}

/// One result of a `workspace/symbol` search: a `WorkspaceSymbol` (LSP 3.17)
/// or legacy `SymbolInformation` — always `name` + `location`.
#[derive(Debug, Clone)]
pub struct LspWorkspaceSymbol {
    pub name: String,
    /// Absolute path decoded from the location URI.
    pub path: PathBuf,
    /// 0-based definition line; `None` when the server omitted the range.
    pub line: Option<usize>,
}

/// Parse a `workspace/symbol` response into a flat result list. Accepts the
/// JSON-RPC envelope or a bare result value; entries without a name or
/// location (protocol violations) are dropped.
pub fn parse_workspace_symbols(v: &Value) -> Vec<LspWorkspaceSymbol> {
    let arr = match v.get("result").and_then(|r| r.as_array()) {
        Some(a) => a,
        None => return Vec::new(),
    };
    let mut out = Vec::with_capacity(arr.len());
    for s in arr {
        let name = s.get("name").and_then(|n| n.as_str()).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let Some(loc) = s.get("location") else {
            continue;
        };
        let Some(uri) = loc.get("uri").and_then(|u| u.as_str()) else {
            continue;
        };
        let line = loc
            .get("range")
            .and_then(|r| r.get("start"))
            .and_then(|st| st.get("line"))
            .and_then(|l| l.as_u64())
            .map(|l| l as usize);
        out.push(LspWorkspaceSymbol {
            name: name.to_string(),
            path: uri_to_path(uri),
            line,
        });
    }
    out
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

/// Decode a `textDocument/semanticTokens/full` result (relative encoding,
/// which is what our client capabilities request): the `data` array is a
/// flat stream of quintuples `(deltaLine, startChar, length, typeIndex,
/// modifierBits)` where `deltaLine` is relative to the previous token's line
/// and `startChar` is absolute within its line. Type indices and modifier
/// bits resolve through the server legend captured at initialize time.
pub fn parse_semantic_tokens(
    resp: &Value,
    token_types: &[String],
    token_modifiers: &[String],
) -> Vec<LspSemanticToken> {
    let data: Vec<u64> = resp["result"]
        .get("data")
        .or_else(|| resp.get("data"))
        .and_then(|d| d.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect())
        .unwrap_or_default();
    let mut out = Vec::new();
    let mut line = 0usize;
    for q in data.chunks(5) {
        if q.len() < 5 {
            break;
        }
        line += q[0] as usize;
        let bits = q[4];
        out.push(LspSemanticToken {
            line,
            start_char: q[1] as usize,
            length: q[2] as usize,
            token_type: token_types
                .get(q[3] as usize)
                .cloned()
                .unwrap_or_else(|| format!("#{}", q[3])),
            modifiers: token_modifiers
                .iter()
                .enumerate()
                .filter_map(|(i, m)| {
                    if i < 64 && bits & (1u64 << i) != 0 {
                        Some(m.clone())
                    } else {
                        None
                    }
                })
                .collect(),
        });
    }
    out
}

/// Parse one `{range, newText}` text-edit object. Shared by the formatter,
/// rename and code-action parsers, which all carry the same edit shape.
fn parse_text_edit(e: &Value) -> Option<LspTextEdit> {
    let range = &e["range"];
    Some(LspTextEdit {
        start_line: range["start"]["line"].as_u64()? as usize,
        start_char: range["start"]["character"].as_u64()? as usize,
        end_line: range["end"]["line"].as_u64()? as usize,
        end_char: range["end"]["character"].as_u64()? as usize,
        new_text: e["newText"].as_str()?.to_string(),
    })
}

/// Parse a `textDocument/formatting` (or `rangeFormatting`) result: an array
/// of `{range, newText}` text edits. Null/absent results yield no edits.
pub fn parse_formatting(resp: &Value) -> Vec<LspTextEdit> {
    let arr = resp
        .get("result")
        .and_then(|r| r.as_array())
        .or_else(|| resp.as_array());
    let Some(arr) = arr else { return Vec::new() };
    arr.iter().filter_map(parse_text_edit).collect()
}

/// Parse a bare `WorkspaceEdit` value (NOT wrapped in a JSON-RPC envelope).
/// Handles both representations the spec allows:
/// * `changes`: a map of `uri` → array of text edits.
/// * `documentChanges`: an array of `{ textDocument: {uri}, edits: [...] }`.
///
/// Create/rename-file operations (`documentChanges` entries without `edits`)
/// are skipped — this engine applies text edits only.
fn parse_workspace_edit_value(we: &Value) -> LspWorkspaceEdit {
    let mut out = LspWorkspaceEdit::default();
    if let Some(changes) = we.get("changes").and_then(|c| c.as_object()) {
        for (uri, edits) in changes {
            let parsed: Vec<LspTextEdit> = edits
                .as_array()
                .map(|a| a.iter().filter_map(parse_text_edit).collect())
                .unwrap_or_default();
            if !parsed.is_empty() {
                out.changes.push(LspFileEdits {
                    file: uri_to_path(uri),
                    edits: parsed,
                });
            }
        }
    }
    if let Some(dcs) = we.get("documentChanges").and_then(|d| d.as_array()) {
        for dc in dcs {
            let Some(edits_arr) = dc.get("edits").and_then(|e| e.as_array()) else {
                continue;
            };
            let uri = match dc["textDocument"]["uri"].as_str() {
                Some(u) => u,
                None => continue,
            };
            let parsed: Vec<LspTextEdit> = edits_arr.iter().filter_map(parse_text_edit).collect();
            if !parsed.is_empty() {
                out.changes.push(LspFileEdits {
                    file: uri_to_path(uri),
                    edits: parsed,
                });
            }
        }
    }
    // Deterministic file order (a JSON object map has none) so callers and
    // tests see a stable sequence.
    out.changes.sort_by(|a, b| a.file.cmp(&b.file));
    out
}

/// Parse a `textDocument/rename` response envelope into a [`LspWorkspaceEdit`].
/// Accepts both `{"result": {...}}` and a bare edit object.
pub fn parse_workspace_edit(resp: &Value) -> LspWorkspaceEdit {
    let result = resp.get("result").unwrap_or(resp);
    parse_workspace_edit_value(result)
}

/// Parse a single CodeAction object (as returned by both
/// `textDocument/codeAction` elements and `codeAction/resolve` results).
/// Bare Commands (title + command, no kind/edit) parse with an empty kind so
/// callers can still show and run them.
pub fn parse_one_code_action(a: &Value) -> Option<LspCodeAction> {
    let title = a["title"].as_str()?.to_string();
    let edit = a
        .get("edit")
        .filter(|e| !e.is_null())
        .map(parse_workspace_edit_value);
    let command = a.get("command").filter(|c| !c.is_null()).and_then(|c| {
        let name = c["command"].as_str()?.to_string();
        Some(LspCommand {
            command: name,
            arguments: c.get("arguments").cloned().filter(|v| !v.is_null()),
        })
    });
    Some(LspCodeAction {
        title,
        kind: a["kind"].as_str().unwrap_or("").to_string(),
        edit,
        is_preferred: a["isPreferred"].as_bool().unwrap_or(false),
        command,
        // Keep the original object so an unresolved action can be echoed to
        // `codeAction/resolve` (carries the server's opaque `data`).
        raw: Some(a.clone()),
    })
}

/// Parse a `textDocument/codeAction` result: an array of CodeAction (or
/// Command) objects. Commands lack `kind`/`edit` and are surfaced with an
/// empty kind and no edits so the caller can still show their titles.
pub fn parse_code_actions(resp: &Value) -> Vec<LspCodeAction> {
    let arr = resp
        .get("result")
        .and_then(|r| r.as_array())
        .or_else(|| resp.as_array());
    let Some(arr) = arr else { return Vec::new() };
    arr.iter().filter_map(parse_one_code_action).collect()
}

/// Byte offset of a `(line, char)` position in `text`. Characters beyond a
/// line's end clamp to it; lines beyond the document clamp to EOF.
fn line_char_offset(text: &str, line: usize, ch: usize) -> usize {
    let mut offset = 0usize;
    for (idx, l) in text.split_inclusive('\n').enumerate() {
        if idx == line {
            let chars = l.chars().count();
            let clipped = ch.min(chars.saturating_sub(if l.ends_with('\n') { 1 } else { 0 }));
            return offset
                + l.char_indices()
                    .nth(clipped)
                    .map(|(b, _)| b)
                    .unwrap_or(l.len());
        }
        offset += l.len();
    }
    offset
}

/// Apply a set of non-overlapping text edits (back-to-front, so earlier
/// offsets stay valid while later ones are spliced). Used to turn formatter
/// responses into a replacement buffer.
pub fn apply_text_edits(text: &str, edits: &[LspTextEdit]) -> String {
    let mut ordered: Vec<&LspTextEdit> = edits.iter().collect();
    // Descending start position: splicing from the end never invalidates the
    // offsets of the edits that follow.
    ordered.sort_by_key(|e| std::cmp::Reverse((e.start_line, e.start_char)));
    let mut doc = text.to_string();
    for e in ordered {
        let start = line_char_offset(&doc, e.start_line, e.start_char);
        let end = line_char_offset(&doc, e.end_line, e.end_char);
        if start > end {
            continue;
        }
        doc.replace_range(start..end, &e.new_text);
    }
    doc
}

/// Apply a [`LspWorkspaceEdit`] across files, given the current contents of
/// each via `current`. Pure and total: for every touched file with known
/// content it produces the rewritten text; files with no known content are
/// skipped (the caller decides whether an unreadable file is an error). The
/// caller is responsible for persisting the results (open buffers vs. disk),
/// which is why this returns `(path, new_text)` pairs rather than writing.
pub fn apply_workspace_edit<F>(edit: &LspWorkspaceEdit, current: F) -> Vec<(PathBuf, String)>
where
    F: Fn(&Path) -> Option<String>,
{
    let mut out = Vec::new();
    for fe in &edit.changes {
        if let Some(text) = current(&fe.file) {
            out.push((fe.file.clone(), apply_text_edits(&text, &fe.edits)));
        }
    }
    out
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
    fn parse_show_message_maps_severity() {
        let err = parse_show_message(&serde_json::json!({"type": 1, "message": "boom"})).unwrap();
        assert_eq!(err.severity, DiagnosticSeverity::Error);
        assert_eq!(err.message, "boom");

        let warn = parse_show_message(&serde_json::json!({"type": 2, "message": "hmm"})).unwrap();
        assert_eq!(warn.severity, DiagnosticSeverity::Warning);

        // An absent type falls back to Info but the message still surfaces.
        let info = parse_show_message(&serde_json::json!({"message": "fyi"})).unwrap();
        assert_eq!(info.severity, DiagnosticSeverity::Info);

        // An unrecognized type also degrades to Info rather than being dropped.
        let odd = parse_show_message(&serde_json::json!({"type": 99, "message": "x"})).unwrap();
        assert_eq!(odd.severity, DiagnosticSeverity::Info);
    }

    #[test]
    fn parse_show_message_rejects_missing_or_empty() {
        // No message field at all.
        assert!(parse_show_message(&serde_json::json!({"type": 1})).is_none());
        // Empty message must not occupy the toast area.
        assert!(parse_show_message(&serde_json::json!({"type": 1, "message": ""})).is_none());
    }

    #[test]
    fn inbox_default_is_empty() {
        let inbox = LspInbox::default();
        assert!(inbox.responses.is_empty());
        assert!(inbox.notifications.is_empty());
        assert!(!inbox.reader_alive);
    }

    // --- I1: interactive intelligence parsers ------------------------------

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
    fn completion_documentation_handles_all_shapes() {
        // Plain string.
        let s = serde_json::json!({ "documentation": "A simple doc" });
        assert_eq!(
            completion_documentation(&s).as_deref(),
            Some("A simple doc")
        );
        // MarkupContent object.
        let m = serde_json::json!({
            "documentation": { "kind": "markdown", "value": "# Title\nbody" }
        });
        assert_eq!(
            completion_documentation(&m).as_deref(),
            Some("# Title\nbody")
        );
        // Legacy MarkedString[] mixing a bare string and a code object.
        let a = serde_json::json!({
            "documentation": [ "First para", { "language": "rust", "value": "fn x()" } ]
        });
        assert_eq!(
            completion_documentation(&a).as_deref(),
            Some("First para\n\nfn x()")
        );
        // Absent / null / empty-after-trim all yield None.
        assert_eq!(completion_documentation(&serde_json::json!({})), None);
        assert_eq!(
            completion_documentation(&serde_json::json!({ "documentation": null })),
            None
        );
        assert_eq!(
            completion_documentation(&serde_json::json!({ "documentation": "   " })),
            None
        );
    }

    #[test]
    fn parse_completion_captures_documentation() {
        let resp = serde_json::json!({
            "result": [
                { "label": "spawn", "documentation": { "kind": "plaintext", "value": "Spawns a thread" } },
                { "label": "no_doc" }
            ]
        });
        let items = parse_completion(&resp);
        assert_eq!(items[0].documentation.as_deref(), Some("Spawns a thread"));
        assert_eq!(items[1].documentation, None);
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

    #[test]
    fn parse_semantic_tokens_decodes_relative_quintuples() {
        // Legend: types ["variable","function"], modifiers ["declaration","deprecated"].
        let types: Vec<String> = ["variable", "function"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mods: Vec<String> = ["declaration", "deprecated"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let resp = serde_json::json!({ "id": 9, "result": { "data": [
            0, 4, 3, 1, 1,   // line 0, col 4, "foo" type=function, mod bit0=declaration
            2, 0, 5, 0, 3,   // line 2 later, col 0, "bar" type=variable, bits 3 = declaration+deprecated
            0, 9, 2, 7, 0    // unknown type index 7 -> fallback name
        ]}});
        let tokens = parse_semantic_tokens(&resp, &types, &mods);
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].line, 0);
        assert_eq!(tokens[0].token_type, "function");
        assert_eq!(tokens[0].modifiers, vec!["declaration".to_string()]);
        assert_eq!(tokens[1].line, 2, "relative line accumulates");
        assert_eq!(tokens[1].start_char, 0);
        assert_eq!(
            tokens[1].modifiers,
            vec!["declaration".to_string(), "deprecated".to_string()]
        );
        assert_eq!(
            tokens[2].token_type, "#7",
            "unknown legend index degrades to a label"
        );
        // Null result and short tails degrade cleanly.
        assert!(
            parse_semantic_tokens(&serde_json::json!({"result": null}), &types, &mods).is_empty()
        );
        let truncated = serde_json::json!({ "result": { "data": [0, 1, 2] } });
        assert!(parse_semantic_tokens(&truncated, &types, &mods).is_empty());
    }

    #[test]
    fn parse_formatting_reads_text_edits() {
        let resp = serde_json::json!({ "id": 10, "result": [
            { "range": { "start": { "line": 1, "character": 0 }, "end": { "line": 1, "character": 8 } },
              "newText": "    let x = 1;" }
        ]});
        let edits = parse_formatting(&resp);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].start_line, 1);
        assert_eq!(edits[0].end_char, 8);
        assert_eq!(edits[0].new_text, "    let x = 1;");
        assert!(parse_formatting(&serde_json::json!({"result": null})).is_empty());
    }

    #[test]
    fn apply_text_edits_splices_back_to_front() {
        let text = "fn main() {\nlet a=1;\nlet b=2;\n}\n";
        let edits = vec![
            LspTextEdit {
                start_line: 1,
                start_char: 0,
                end_line: 1,
                end_char: 8,
                new_text: "    let a = 1;".into(),
            },
            LspTextEdit {
                start_line: 2,
                start_char: 0,
                end_line: 2,
                end_char: 8,
                new_text: "    let b = 2;".into(),
            },
        ];
        let out = apply_text_edits(text, &edits);
        assert_eq!(out, "fn main() {\n    let a = 1;\n    let b = 2;\n}\n");
        // A whole-document replacement edit (multi-line range). The end
        // position (3, 1) is before the trailing newline of "}\n", so LSP
        // clamping keeps that newline: replacing up to it is correct.
        let whole = vec![LspTextEdit {
            start_line: 0,
            start_char: 0,
            end_line: 3,
            end_char: 1,
            new_text: "fn main() {}".into(),
        }];
        assert_eq!(apply_text_edits(text, &whole), "fn main() {}\n");
    }

    #[test]
    fn apply_server_capabilities_records_providers_and_legend() {
        let mut server = LspServer::new(LspServerConfig::rust_analyzer(Path::new("/tmp")));
        let resp = serde_json::json!({ "id": 1, "result": { "capabilities": {
            "hoverProvider": true,
            "completionProvider": { "triggerCharacters": ["."] },
            "documentFormattingProvider": true,
            "semanticTokensProvider": {
                "legend": { "tokenTypes": ["variable", "function"], "tokenModifiers": ["declaration"] },
                "full": true
            }
        }}});
        server.apply_server_capabilities(&resp);
        assert!(server.capabilities.hover);
        assert!(server.capabilities.completion);
        assert!(server.capabilities.formatting);
        assert!(server.capabilities.semantic_tokens);
        assert_eq!(
            server.capabilities.semantic_token_types,
            vec!["variable".to_string(), "function".to_string()]
        );
        assert_eq!(
            server.capabilities.semantic_token_modifiers,
            vec!["declaration".to_string()]
        );
        assert!(!server.capabilities.rename, "absent providers stay off");
    }

    #[test]
    fn semantic_tokens_and_formatting_degrade_without_server() {
        let mut mgr = LspManager::new();
        let path = Path::new("/tmp/does_not_matter.rs");
        assert!(mgr.semantic_tokens("rs", path, "fn main() {}").is_empty());
        assert_eq!(
            mgr.format_document("rs", path, "fn main() {}", 4, true),
            None
        );
        // Rename and code actions degrade the same way with no server.
        assert!(mgr
            .rename_symbol("rs", path, 0, 4, "new_name", "fn main() {}")
            .is_none());
        assert!(mgr
            .code_actions("rs", path, ((0, 0), (0, 1)), "fn main() {}")
            .is_empty());
    }

    #[test]
    fn apply_server_capabilities_detects_code_action_provider() {
        let mut server = LspServer::new(LspServerConfig::rust_analyzer(Path::new("/tmp")));
        assert!(!server.capabilities.code_action, "off by default");
        let resp = serde_json::json!({ "id": 1, "result": { "capabilities": {
            "renameProvider": true,
            "codeActionProvider": { "codeActionKinds": ["quickfix", "refactor"] }
        }}});
        server.apply_server_capabilities(&resp);
        assert!(server.capabilities.rename);
        assert!(server.capabilities.code_action, "object provider counts");
    }

    #[test]
    fn parse_workspace_edit_changes_shape() {
        // `changes`: uri -> [edits]; a rename touching two files.
        let resp = serde_json::json!({ "id": 7, "result": { "changes": {
            "file:///proj/src/lib.rs": [
                { "range": { "start": {"line":0,"character":4}, "end": {"line":0,"character":7} },
                  "newText": "renamed" }
            ],
            "file:///proj/src/main.rs": [
                { "range": { "start": {"line":3,"character":8}, "end": {"line":3,"character":11} },
                  "newText": "renamed" },
                { "range": { "start": {"line":9,"character":2}, "end": {"line":9,"character":5} },
                  "newText": "renamed" }
            ]
        }}});
        let we = parse_workspace_edit(&resp);
        assert_eq!(we.total_edits(), 3);
        assert!(!we.is_empty());
        // Sorted by path: lib.rs before main.rs.
        assert!(we.changes[0].file.display().to_string().contains("lib.rs"));
        assert_eq!(we.changes[1].edits.len(), 2);
    }

    #[test]
    fn parse_workspace_edit_document_changes_shape_and_skips_create() {
        // `documentChanges`: array form, plus a create-file op that must be ignored.
        let resp = serde_json::json!({ "result": { "documentChanges": [
            { "textDocument": { "uri": "file:///a.rs" }, "edits": [
                { "range": { "start": {"line":1,"character":0}, "end": {"line":1,"character":3} },
                  "newText": "abc" }
            ]},
            { "kind": "create", "uri": "file:///brand_new.rs" }
        ]}});
        let we = parse_workspace_edit(&resp);
        assert_eq!(we.changes.len(), 1, "create op carries no text edits");
        assert!(
            we.changes[0].file.display().to_string().ends_with("a.rs"),
            "uri decoded to a path: {:?}",
            we.changes[0].file
        );
        assert_eq!(we.changes[0].edits[0].new_text, "abc");
    }

    #[test]
    fn parse_workspace_edit_null_and_empty_are_empty() {
        assert!(parse_workspace_edit(&serde_json::json!({"result": null})).is_empty());
        assert!(parse_workspace_edit(&serde_json::json!({"result": {}})).is_empty());
    }

    #[test]
    fn apply_workspace_edit_rewrites_multiple_files() {
        // Build a two-file edit and apply against in-memory contents.
        let resp = serde_json::json!({ "result": { "changes": {
            "file:///foo.rs": [
                { "range": { "start": {"line":0,"character":0}, "end": {"line":0,"character":3} },
                  "newText": "BAR" }
            ],
            "file:///bar.rs": [
                { "range": { "start": {"line":1,"character":0}, "end": {"line":1,"character":3} },
                  "newText": "BAR" }
            ]
        }}});
        let we = parse_workspace_edit(&resp);
        let current = |p: &Path| -> Option<String> {
            match p.to_str().unwrap_or("") {
                s if s.ends_with("foo.rs") => Some("foo\nfoo2\n".to_string()),
                s if s.ends_with("bar.rs") => Some("x\nfoo\n".to_string()),
                _ => None,
            }
        };
        let applied = apply_workspace_edit(&we, current);
        assert_eq!(applied.len(), 2);
        let map: HashMap<PathBuf, String> = applied.into_iter().collect();
        let foo = map
            .keys()
            .find(|k| k.display().to_string().ends_with("foo.rs"))
            .unwrap();
        assert_eq!(map[foo], "BAR\nfoo2\n");
    }

    #[test]
    fn apply_workspace_edit_skips_files_with_no_content() {
        let resp = serde_json::json!({ "result": { "changes": {
            "file:///missing.rs": [ { "range": { "start": {"line":0,"character":0}, "end": {"line":0,"character":1} }, "newText": "x" } ]
        }}});
        let we = parse_workspace_edit(&resp);
        // `current` knows nothing -> nothing is applied (caller sees empty vec).
        assert!(apply_workspace_edit(&we, |_| None).is_empty());
    }

    #[test]
    fn parse_code_actions_reads_titles_kinds_and_edits() {
        let resp = serde_json::json!({ "result": [
            { "title": "Extract function", "kind": "refactor.extract",
              "edit": { "changes": { "file:///a.rs": [
                  { "range": { "start": {"line":0,"character":0}, "end": {"line":0,"character":1} }, "newText": "z" }
              ]}}},
            { "title": "Organize imports", "kind": "source.organizeImports", "isPreferred": true },
            { "title": "Bare command", "command": { "title": "x", "command": "rust-analyzer.fixAll", "arguments": [{ "textDocument": { "uri": "file:///a.rs" } }] } }
        ]});
        let actions = parse_code_actions(&resp);
        assert_eq!(actions.len(), 3);
        let extract = actions
            .iter()
            .find(|a| a.title == "Extract function")
            .unwrap();
        assert_eq!(extract.kind, "refactor.extract");
        assert!(extract.edit.as_ref().unwrap().total_edits() == 1);
        let org = actions
            .iter()
            .find(|a| a.title == "Organize imports")
            .unwrap();
        assert!(org.is_preferred);
        assert!(
            org.edit.is_none(),
            "command-less action has no direct edits"
        );
        // A pure Command still surfaces its title with no kind/edits, but its
        // `command` id + arguments are parsed so the client can run it.
        let bare = actions.iter().find(|a| a.title == "Bare command").unwrap();
        assert!(bare.kind.is_empty());
        assert!(bare.edit.is_none());
        assert_eq!(
            bare.command.as_ref().unwrap().command,
            "rust-analyzer.fixAll"
        );
        assert!(bare.command.as_ref().unwrap().arguments.is_some());
        assert!(parse_code_actions(&serde_json::json!({"result": null})).is_empty());
    }

    #[test]
    fn python_config_has_correct_extensions() {
        let cfg = LspServerConfig::python(Path::new("/tmp"));
        assert_eq!(cfg.language_id, "python");
        assert_eq!(cfg.command, "pyright-langserver");
        assert!(cfg.extensions.contains(&"py".to_string()));
        assert!(cfg.extensions.contains(&"pyi".to_string()));
    }

    #[test]
    fn go_config_has_correct_extensions() {
        let cfg = LspServerConfig::go(Path::new("/tmp"));
        assert_eq!(cfg.language_id, "go");
        assert_eq!(cfg.command, "gopls");
        assert!(cfg.extensions.contains(&"go".to_string()));
    }

    #[test]
    fn clangd_config_has_correct_extensions() {
        let cfg = LspServerConfig::clangd(Path::new("/tmp"));
        assert_eq!(cfg.language_id, "cpp");
        assert_eq!(cfg.command, "clangd");
        assert!(cfg.extensions.contains(&"c".to_string()));
        assert!(cfg.extensions.contains(&"cpp".to_string()));
        assert!(cfg.extensions.contains(&"h".to_string()));
    }

    #[test]
    fn auto_detect_rust_project() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("Cargo.toml"), "[package]").unwrap();
        let mgr = LspManager::auto_detect(tmp.path());
        assert!(mgr.server_count() >= 1);
    }

    #[test]
    fn auto_detect_python_project() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("pyproject.toml"), "[tool.pyright]").unwrap();
        let mgr = LspManager::auto_detect(tmp.path());
        assert!(mgr.server_count() >= 1);
    }

    #[test]
    fn auto_detect_go_project() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("go.mod"), "module example.com/m").unwrap();
        let mgr = LspManager::auto_detect(tmp.path());
        assert!(mgr.server_count() >= 1);
    }

    #[test]
    fn auto_detect_empty_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = LspManager::auto_detect(tmp.path());
        assert_eq!(mgr.server_count(), 0);
    }

    #[test]
    fn classify_message_distinguishes_response_request_notification() {
        // A reply to our request: id present, no method.
        let resp = serde_json::json!({ "jsonrpc": "2.0", "id": 3, "result": {} });
        assert_eq!(classify_message(&resp), MessageKind::Response);
        // A server→client request: id AND method (e.g. workspace/applyEdit).
        let req = serde_json::json!({ "jsonrpc": "2.0", "id": 7, "method": "workspace/applyEdit", "params": {} });
        assert_eq!(classify_message(&req), MessageKind::Request);
        // A one-way notification: method, no id.
        let note =
            serde_json::json!({ "jsonrpc": "2.0", "method": "window/logMessage", "params": {} });
        assert_eq!(classify_message(&note), MessageKind::Notification);
        // A null id is not an id → still a notification.
        let null_id = serde_json::json!({ "jsonrpc": "2.0", "id": null, "method": "x" });
        assert_eq!(classify_message(&null_id), MessageKind::Notification);
        // Neither id nor method → unknown.
        assert_eq!(
            classify_message(&serde_json::json!({})),
            MessageKind::Unknown
        );
    }

    #[test]
    fn encode_jsonrpc_response_has_matching_length_and_body() {
        let frame = encode_jsonrpc_response(12, serde_json::json!({ "applied": true }));
        let (header, body) = frame.split_once("\r\n\r\n").expect("header delimiter");
        let declared: usize = header
            .trim_start_matches("Content-Length:")
            .trim()
            .parse()
            .expect("numeric length");
        assert_eq!(declared, body.len(), "Content-Length must equal body bytes");
        let parsed: Value = serde_json::from_str(body).expect("valid json body");
        assert_eq!(parsed["id"], serde_json::json!(12));
        assert_eq!(parsed["result"]["applied"], serde_json::json!(true));
        assert_eq!(parsed["jsonrpc"], serde_json::json!("2.0"));
    }

    #[test]
    fn apply_server_capabilities_detects_execute_command() {
        let mut server = LspServer::new(LspServerConfig::rust_analyzer(Path::new("/tmp")));
        assert!(!server.capabilities.execute_command, "off by default");
        // An advertised command list turns the capability on.
        let with_cmds = serde_json::json!({ "id": 1, "result": { "capabilities": {
            "executeCommandProvider": { "commands": ["rust-analyzer.fixAll"] }
        }}});
        server.apply_server_capabilities(&with_cmds);
        assert!(server.capabilities.execute_command);
        // An empty command list is not a usable provider.
        let mut server2 = LspServer::new(LspServerConfig::rust_analyzer(Path::new("/tmp")));
        let empty = serde_json::json!({ "id": 1, "result": { "capabilities": {
            "executeCommandProvider": { "commands": [] }
        }}});
        server2.apply_server_capabilities(&empty);
        assert!(
            !server2.capabilities.execute_command,
            "no commands = no provider"
        );
    }

    #[test]
    fn apply_server_capabilities_detects_type_def_and_implementation() {
        let mut server = LspServer::new(LspServerConfig::rust_analyzer(Path::new("/tmp")));
        assert!(!server.capabilities.type_definition, "off by default");
        assert!(!server.capabilities.implementation, "off by default");
        server.apply_server_capabilities(&serde_json::json!({ "id": 1, "result": {
            "capabilities": {
                "typeDefinitionProvider": true,
                "implementationProvider": { "workDoneProgress": false }
            }
        }}));
        assert!(server.capabilities.type_definition, "boolean provider");
        assert!(
            server.capabilities.implementation,
            "object provider counts too"
        );
    }

    #[test]
    fn apply_server_capabilities_detects_declaration_provider() {
        let mut server = LspServer::new(LspServerConfig::typescript(Path::new("/tmp")));
        assert!(!server.capabilities.declaration, "off by default");
        server.apply_server_capabilities(&serde_json::json!({ "id": 1, "result": {
            "capabilities": { "declarationProvider": true }
        }}));
        assert!(server.capabilities.declaration);
        // Rust-analyzer-style servers that omit it stay false, and the gated
        // manager request then returns empty without sending anything.
        let mut plain = LspServer::new(LspServerConfig::rust_analyzer(Path::new("/tmp")));
        plain.apply_server_capabilities(&serde_json::json!({ "id": 1, "result": {
            "capabilities": { "definitionProvider": true }
        }}));
        assert!(!plain.capabilities.declaration, "definition ≠ declaration");
    }

    #[test]
    fn apply_server_capabilities_detects_signature_help() {
        let mut server = LspServer::new(LspServerConfig::rust_analyzer(Path::new("/tmp")));
        assert!(!server.capabilities.signature_help, "off by default");
        let resp = serde_json::json!({ "id": 1, "result": { "capabilities": {
            "signatureHelpProvider": { "triggerCharacters": ["(", ","] }
        }}});
        server.apply_server_capabilities(&resp);
        assert!(server.capabilities.signature_help);
        assert_eq!(
            server.capabilities.signature_trigger_chars,
            vec!["(".to_string(), ",".to_string()]
        );
        // A bare boolean `true` provider with no trigger chars still counts.
        let mut server2 = LspServer::new(LspServerConfig::rust_analyzer(Path::new("/tmp")));
        server2.apply_server_capabilities(&serde_json::json!({ "id": 1, "result": {
            "capabilities": { "signatureHelpProvider": true }
        }}));
        assert!(server2.capabilities.signature_help);
        assert!(server2.capabilities.signature_trigger_chars.is_empty());
    }

    #[test]
    fn parse_signature_help_string_labels_and_active_indices() {
        let resp = serde_json::json!({ "id": 7, "result": {
            "signatures": [
                { "label": "fn add(a: i32, b: i32) -> i32",
                  "documentation": { "kind": "markdown", "value": "Adds two ints." },
                  "parameters": [ { "label": "a: i32" }, { "label": "b: i32" } ] },
                { "label": "fn add(a: f32, b: f32) -> f32",
                  "parameters": [ { "label": "a: f32" }, { "label": "b: f32" } ] }
            ],
            "activeSignature": 1,
            "activeParameter": 1
        }});
        let help = parse_signature_help(&resp).expect("parsed");
        assert_eq!(help.signatures.len(), 2);
        assert_eq!(help.active_signature, 1);
        assert_eq!(help.active_parameter, 1);
        assert_eq!(
            help.active().unwrap().label,
            "fn add(a: f32, b: f32) -> f32"
        );
        assert_eq!(help.active_param_label(), Some("b: f32"));
        assert_eq!(help.signatures[0].documentation, "Adds two ints.");
        assert_eq!(
            help.signatures[1].documentation, "",
            "absent docs normalize empty"
        );
    }

    #[test]
    fn parse_signature_help_range_labels_are_sliced() {
        // rust-analyzer sends parameter labels as [start, end] utf16 offsets
        // into the signature label rather than duplicate strings.
        let resp = serde_json::json!({ "result": {
            "signatures": [ { "label": "move_to(x: i32, y: i32)",
                "parameters": [ { "label": [8, 14] }, { "label": [16, 22] } ] } ],
            "activeSignature": 0,
            "activeParameter": 0
        }});
        let help = parse_signature_help(&resp).expect("parsed");
        let sig = help.active().unwrap();
        assert_eq!(sig.parameters[0].label, "x: i32");
        assert_eq!(sig.parameters[1].label, "y: i32");
        assert_eq!(help.active_param_label(), Some("x: i32"));
    }

    #[test]
    fn parse_signature_help_clamps_out_of_range_indices() {
        let resp = serde_json::json!({ "result": {
            "signatures": [ { "label": "only()", "parameters": [] } ],
            "activeSignature": 99,
            "activeParameter": 42
        }});
        let help = parse_signature_help(&resp).expect("parsed");
        assert_eq!(help.active_signature, 0, "clamped to last signature");
        assert_eq!(help.active_parameter, 0, "clamped with zero params");
        assert!(help.active().is_some());
        assert_eq!(help.active_param_label(), None, "no parameters to point at");
    }

    #[test]
    fn parse_signature_help_null_and_empty_are_none() {
        assert!(parse_signature_help(&serde_json::json!({ "result": Value::Null })).is_none());
        assert!(
            parse_signature_help(&serde_json::json!({ "result": { "signatures": [] } })).is_none()
        );
        assert!(
            parse_signature_help(&serde_json::json!({ "id": 1 })).is_none(),
            "no result key"
        );
    }

    #[test]
    fn caret_inside_parens_tracks_call_depth() {
        assert!(caret_inside_parens("foo("), "open paren, nothing after");
        assert!(caret_inside_parens("foo(a, "));
        assert!(caret_inside_parens("outer(inner(x), "), "nested stays open");
        assert!(!caret_inside_parens("foo(a)"), "closed call");
        assert!(!caret_inside_parens("let x = 1;\n"), "no parens at all");
        assert!(
            !caret_inside_parens("))))"),
            "extra closers clamp to zero, never negative"
        );
    }

    /// Build a Content-Length-framed byte stream from JSON bodies, as the
    /// server would write to stdout.
    fn framed(bodies: &[Value]) -> Vec<u8> {
        let mut out = Vec::new();
        for b in bodies {
            let s = b.to_string();
            out.extend_from_slice(format!("Content-Length: {}\r\n\r\n", s.len()).as_bytes());
            out.extend_from_slice(s.as_bytes());
        }
        out
    }

    #[test]
    fn reader_keeps_applyedit_request_apart_from_responses() {
        // Regression: a server→client request carries BOTH an id and a method.
        // It must land in `requests` (answerable), never be mis-filed as a
        // response to one of our requests and silently dropped.
        let stream = framed(&[
            serde_json::json!({ "jsonrpc": "2.0", "id": 99, "method": "workspace/applyEdit",
                "params": { "edit": { "changes": { "file:///a.rs": [] } } } }),
            serde_json::json!({ "jsonrpc": "2.0", "id": 5, "result": { "ok": true } }),
            serde_json::json!({ "jsonrpc": "2.0", "method": "window/logMessage",
                "params": { "type": 3, "message": "hi" } }),
        ]);
        let inbox = Arc::new(Mutex::new(LspInbox::default()));
        lsp_stdout_reader(std::io::Cursor::new(stream), inbox.clone());
        let inbox = inbox.lock().unwrap();
        assert_eq!(inbox.requests.len(), 1, "applyEdit request captured");
        assert_eq!(inbox.requests[0].0, 99);
        assert_eq!(inbox.requests[0].1, "workspace/applyEdit");
        assert_eq!(
            inbox.responses.len(),
            1,
            "the id=5 reply is a separate response"
        );
        assert!(inbox.responses.contains_key(&5));
        assert_eq!(inbox.notifications.len(), 1);
        assert_eq!(inbox.notifications[0].0, "window/logMessage");
    }

    #[test]
    fn server_request_result_answers_without_stalling() {
        // configuration: one null per requested item, so the array matches.
        let params = serde_json::json!({ "items": [{ "section": "a" }, { "section": "b" }] });
        let cfg = server_request_result("workspace/configuration", &params);
        assert_eq!(cfg.as_array().map(|a| a.len()), Some(2));
        assert!(cfg.as_array().unwrap().iter().all(|v| v.is_null()));
        // An empty/absent items list yields an empty array, never a hang.
        assert_eq!(
            server_request_result("workspace/configuration", &serde_json::json!({}))
                .as_array()
                .map(|a| a.len()),
            Some(0)
        );
        // A stray applyEdit is declined, not dropped.
        let apply = server_request_result("workspace/applyEdit", &serde_json::json!({}));
        assert_eq!(apply["applied"], serde_json::json!(false));
        // Registration and unknown methods answer with null.
        assert!(
            server_request_result("client/registerCapability", &serde_json::json!({})).is_null()
        );
        assert!(server_request_result("some/weird/Method", &serde_json::json!({})).is_null());
    }

    #[test]
    fn parse_call_hierarchy_items_decodes_name_kind_and_location() {
        let resp = serde_json::json!({ "result": [
            { "name": "main", "kind": 13, "location": {
                "uri": "file:///src/main.rs",
                "range": { "start": { "line": 10, "character": 4 }, "end": { "line": 10, "character": 8 } }
            }}
        ]});
        let items = parse_call_hierarchy_items(&resp);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "main");
        assert_eq!(items[0].kind, 13);
        assert_eq!(items[0].line, 10);
        assert_eq!(items[0].col, 4);
        // A single (non-array) item is accepted too.
        let single = serde_json::json!({ "result":
            { "name": "f", "kind": 12, "location": {
                "uri": "file:///lib.rs", "range": { "start": { "line": 1, "character": 0 }, "end": {} } }}});
        assert_eq!(parse_call_hierarchy_items(&single).len(), 1);
        // null / missing result is empty.
        assert!(parse_call_hierarchy_items(&serde_json::json!({ "result": null })).is_empty());
    }

    #[test]
    fn parse_incoming_and_outgoing_calls_read_the_right_edge_field() {
        // Incoming calls carry the caller under `from`.
        let incoming = serde_json::json!({ "result": [
            { "from": { "name": "caller_a", "kind": 12, "location": {
                "uri": "file:///a.rs", "range": { "start": { "line": 3, "character": 0 }, "end": {} } }},
              "fromRanges": [] }
        ]});
        let a = parse_incoming_calls(&incoming);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].name, "caller_a");
        assert_eq!(a[0].line, 3);
        // Outgoing calls carry the callee under `to`, so the same doc must NOT
        // parse as incoming (proves the two edge keys are not conflated).
        let outgoing = serde_json::json!({ "result": [
            { "to": { "name": "callee_b", "kind": 6, "location": {
                "uri": "file:///b.rs", "range": { "start": { "line": 9, "character": 2 }, "end": {} } }},
              "ranges": [] }
        ]});
        let b = parse_outgoing_calls(&outgoing);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].name, "callee_b");
        assert_eq!(b[0].col, 2);
        assert!(
            parse_outgoing_calls(&incoming).is_empty(),
            "from-edge is not a to-edge"
        );
        assert!(
            parse_incoming_calls(&outgoing).is_empty(),
            "to-edge is not a from-edge"
        );
    }

    #[test]
    fn apply_server_capabilities_detects_call_hierarchy() {
        let mut server = LspServer::new(LspServerConfig::rust_analyzer(Path::new("/tmp")));
        assert!(!server.capabilities.call_hierarchy, "off by default");
        server.apply_server_capabilities(&serde_json::json!({ "id": 1, "result": {
            "capabilities": { "callHierarchyProvider": true }
        }}));
        assert!(server.capabilities.call_hierarchy, "boolean provider");
        // The object form (with workDoneProgress) also counts.
        let mut server2 = LspServer::new(LspServerConfig::rust_analyzer(Path::new("/tmp")));
        server2.apply_server_capabilities(&serde_json::json!({ "id": 1, "result": {
            "capabilities": { "callHierarchyProvider": { "workDoneProgress": false } }
        }}));
        assert!(
            server2.capabilities.call_hierarchy,
            "options-object provider"
        );
    }

    #[test]
    fn parse_selection_ranges_flattens_innermost_first() {
        // A leaf with a wider parent and a whole-file grandparent.
        let resp = serde_json::json!({ "result": [
            { "range": { "start": {"line":0,"character":8}, "end": {"line":0,"character":11} },
              "parent": {
                "range": { "start": {"line":0,"character":4}, "end": {"line":2,"character":1} },
                "parent": {
                  "range": { "start": {"line":0,"character":0}, "end": {"line":4,"character":0} }
                }
              } }
        ]});
        let ranges = parse_selection_ranges(&resp);
        assert_eq!(ranges.len(), 3, "leaf + parent + grandparent");
        assert_eq!(ranges[0].start_char, 8, "innermost first");
        assert_eq!(ranges[1].start_char, 4);
        assert_eq!(ranges[2].end_line, 4, "outermost last");
        // null / missing result is empty.
        assert!(parse_selection_ranges(&serde_json::json!({ "result": null })).is_empty());
    }

    #[test]
    fn wider_and_narrower_selection_walk_the_nested_chain() {
        // Spans (start,end) innermost -> outermost, in char offsets.
        let spans = [(8usize, 11usize), (4, 15), (0, 20)];
        // Expand from the innermost selection to the next-wider containing one.
        assert_eq!(wider_selection(&spans, 8, 11), Some((4, 15)));
        assert_eq!(wider_selection(&spans, 4, 15), Some((0, 20)));
        // At the outermost, nothing wider exists.
        assert_eq!(wider_selection(&spans, 0, 20), None);
        // Shrink back inward from a mid selection to the widest contained.
        assert_eq!(narrower_selection(&spans, 4, 15), Some((8, 11)));
        assert_eq!(narrower_selection(&spans, 0, 20), Some((4, 15)));
        // At the innermost, nothing narrower exists.
        assert_eq!(narrower_selection(&spans, 8, 11), None);
        // A bare caret (zero width) inside the leaf expands to the leaf itself.
        assert_eq!(wider_selection(&spans, 9, 9), Some((8, 11)));
    }

    #[test]
    fn apply_server_capabilities_detects_selection_range() {
        let mut server = LspServer::new(LspServerConfig::rust_analyzer(Path::new("/tmp")));
        assert!(!server.capabilities.selection_range, "off by default");
        server.apply_server_capabilities(&serde_json::json!({ "id": 1, "result": {
            "capabilities": { "selectionRangeProvider": true }
        }}));
        assert!(server.capabilities.selection_range);
    }

    #[test]
    fn apply_server_capabilities_detects_code_action_resolve() {
        let mut server = LspServer::new(LspServerConfig::rust_analyzer(Path::new("/tmp")));
        assert!(
            !server.capabilities.code_action_resolve,
            "off without the object form"
        );
        // A boolean provider cannot resolve: lazy actions need the object
        // form with resolveProvider explicitly true.
        server.apply_server_capabilities(&serde_json::json!({ "id": 1, "result": {
            "capabilities": { "codeActionProvider": true }
        }}));
        assert!(server.capabilities.code_action);
        assert!(!server.capabilities.code_action_resolve, "bare true");
        let mut server2 = LspServer::new(LspServerConfig::rust_analyzer(Path::new("/tmp")));
        server2.apply_server_capabilities(&serde_json::json!({ "id": 1, "result": {
            "capabilities": { "codeActionProvider": { "resolveProvider": true } }
        }}));
        assert!(server2.capabilities.code_action_resolve, "explicit resolve");
        let mut server3 = LspServer::new(LspServerConfig::rust_analyzer(Path::new("/tmp")));
        server3.apply_server_capabilities(&serde_json::json!({ "id": 1, "result": {
            "capabilities": { "codeActionProvider": { "resolveProvider": false } }
        }}));
        assert!(server3.capabilities.code_action);
        assert!(
            !server3.capabilities.code_action_resolve,
            "explicitly false"
        );
    }

    #[test]
    fn code_action_parsing_keeps_raw_for_resolve() {
        // A lightweight (unresolved) action: no edit, no command, but the
        // opaque `data` must survive so it can be echoed to resolve.
        let actions = parse_code_actions(&serde_json::json!({ "result": [
            { "title": "Add missing impl", "kind": "refactor",
              "data": { "refactorKind": "generateImpl" } }
        ]}));
        assert_eq!(actions.len(), 1);
        let a = &actions[0];
        assert!(
            a.edit.is_none() && a.command.is_none(),
            "unresolved payload"
        );
        let raw = a.raw.as_ref().expect("raw object kept");
        assert_eq!(raw["data"]["refactorKind"].as_str(), Some("generateImpl"));
    }

    #[test]
    fn parse_one_code_action_reads_resolved_edit_and_command() {
        // The result of `codeAction/resolve` is a single completed action:
        // re-parsing through the same path must surface its edit and command.
        let resolved = serde_json::json!({
            "title": "Pull method up", "kind": "refactor.pull.up",
            "edit": { "changes": { "file:///a.rs": [
                { "range": { "start": {"line":0,"character":0}, "end": {"line":0,"character":0} },
                  "newText": "fn x() {}" }
            ]}},
            "command": { "command": "rust-analyzer.pull.up", "arguments": [1] }
        });
        let a = parse_one_code_action(&resolved).expect("parses");
        assert_eq!(a.kind, "refactor.pull.up");
        assert!(
            a.edit.as_ref().is_some_and(|e| !e.is_empty()),
            "edit filled in"
        );
        assert_eq!(a.command.as_ref().unwrap().command, "rust-analyzer.pull.up");
        // A missing title is not a code action at all.
        assert!(parse_one_code_action(&serde_json::json!({ "kind": "quickfix" })).is_none());
    }

    #[test]
    fn apply_server_capabilities_detects_workspace_symbol() {
        let mut server = LspServer::new(LspServerConfig::rust_analyzer(Path::new("/tmp")));
        assert!(!server.capabilities.workspace_symbol, "off by default");
        server.apply_server_capabilities(&serde_json::json!({ "id": 1, "result": {
            "capabilities": { "workspaceSymbolProvider": { "resolveProvider": false } }
        }}));
        assert!(
            server.capabilities.workspace_symbol,
            "options-object provider counts"
        );
        let mut plain = LspServer::new(LspServerConfig::rust_analyzer(Path::new("/tmp")));
        plain.apply_server_capabilities(&serde_json::json!({ "id": 1, "result": {
            "capabilities": { "documentSymbolProvider": true }
        }}));
        assert!(
            !plain.capabilities.workspace_symbol,
            "document scope is not workspace scope"
        );
    }

    #[test]
    fn parse_workspace_symbols_reads_uri_and_optional_line() {
        let resp = serde_json::json!({ "result": [
            { "name": "compute", "kind": 3, "containerName": "util",
              "location": { "uri": "file:///proj/src/lib.rs",
                            "range": { "start": {"line":41,"character":4},
                                       "end": {"line":41,"character":11} } } },
            // Range is optional on LSP 3.17 WorkspaceSymbol.
            { "name": "no_range", "kind": 5,
              "location": { "uri": "file:///proj/src/other.rs" } },
            // Junk rows: missing name/location must not produce entries.
            { "location": { "uri": "file:///proj/x.rs" } },
            { "name": "orphan" }
        ]});
        let syms = parse_workspace_symbols(&resp);
        assert_eq!(syms.len(), 2, "two well-formed symbols");
        assert_eq!(syms[0].name, "compute");
        assert!(syms[0].path.to_string_lossy().ends_with("lib.rs"));
        assert_eq!(syms[0].line, Some(41));
        assert_eq!(
            syms[1].line, None,
            "absent range degrades to a name-only hit"
        );
        assert!(parse_workspace_symbols(&serde_json::json!({ "result": null })).is_empty());
    }

    /// Live end-to-end probe through the real rust-analyzer: spawn, initialize
    /// handshake, capability detection, `workspace/symbol` dispatch + poll —
    /// the exact path the go-to-symbol switcher rides on. Skipped by default
    /// (needs the binary + the `_probe_ws50` fixture); run with `--ignored`.
    #[test]
    #[ignore = "spawns the real rust-analyzer; run with --ignored for live verification"]
    fn live_rust_analyzer_workspace_symbol_roundtrip() {
        let ws = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .map(|root| root.join("_probe_ws50"));
        let ws = match ws {
            Some(f) if f.join("Cargo.toml").exists() => f,
            _ => {
                eprintln!("fixture _probe_ws50 missing; nothing to probe");
                return;
            }
        };
        let mut mgr = LspManager::auto_detect(&ws);
        let main_rs = ws.join("src").join("main.rs");
        let content = std::fs::read_to_string(&main_rs).unwrap();
        let advertised = {
            let srv = mgr
                .server_for_extension("rs")
                .expect("rust-analyzer registered");
            srv.did_open(&main_rs, &content, "rust").unwrap();
            srv.capabilities.workspace_symbol
        };
        assert!(
            advertised,
            "real rust-analyzer must advertise workspaceSymbolProvider after initialize"
        );
        // Retry the way the GUI now does: `[]` while the crate graph loads is
        // transient, not "nothing exists".
        let deadline = Instant::now() + Duration::from_secs(150);
        loop {
            let id = mgr
                .request_workspace_symbols("rs", "marker")
                .expect("workspace/symbol dispatch");
            let mut answer = None;
            while answer.is_none() && Instant::now() < deadline {
                answer = mgr.poll_workspace_symbols("rs", id);
                if answer.is_none() {
                    thread::sleep(Duration::from_millis(20));
                }
            }
            if let Some(syms) = answer.filter(|a| !a.is_empty()) {
                let hit = syms
                    .iter()
                    .find(|s| s.name == "zeta_marker")
                    .expect("zeta_marker in workspace symbols");
                assert!(hit.path.to_string_lossy().ends_with("main.rs"));
                assert_eq!(hit.line, Some(13), "0-based line of fn zeta_marker");
                return;
            }
            thread::sleep(Duration::from_millis(500));
            assert!(
                Instant::now() < deadline,
                "no symbols after 150s of retries"
            );
        }
    }
}
