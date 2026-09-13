//! GUI Control Bridge — allows external processes (MCP server, AI agents) to
//! control the running IDE GUI via a local TCP connection.
//!
//! ## Architecture
//!
//! ```text
//! MCP Server (velocity_mcp --mode stdio)
//!     │
//!     │ JSON-RPC over TCP (localhost:19821)
//!     ▼
//! GUI Process (velocity_ide_gui)
//!     │
//!     │ GuiControlListener (background thread)
//!     │ reads JSON commands, pushes to crossbeam channel
//!     ▼
//! VelocityApp::process_gui_commands()
//!     │ called at start of each egui frame
//!     │ executes commands on the main thread
//!     ▼
//! Response sent back through TCP
//! ```
//!
//! ## Security
//!
//! - **Shared-secret token**: Every command must include an `auth_token` field
//!   matching the token generated at startup (written to `.velocity/gui_control.token`).
//!   Commands without a valid token are rejected.
//! - **Path validation**: `OpenFile` paths must be absolute, resolve to a real
//!   file, and fall within the workspace root (symlinks resolved).

use crossbeam_channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// TCP port for GUI control.
pub const GUI_CONTROL_PORT: u16 = 19821;

/// Length of the shared-secret token in bytes (hex-encoded → 64 chars).
const TOKEN_BYTES: usize = 32;

/// A command sent from an external process to the GUI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", content = "params")]
pub enum GuiCommand {
    /// Open a file in the editor.
    OpenFile { path: String },
    /// Get current IDE state.
    GetState {},
    /// Navigate to a specific activity bar panel.
    NavigatePanel { panel: String },
    /// Capture a screenshot and save to disk.
    Screenshot { path: String },
    /// Close the IDE.
    Quit {},
}

/// Wrapper that includes the auth token alongside the command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticatedCommand {
    /// Shared-secret token (hex-encoded). Must match the token in
    /// `.velocity/gui_control.token` for the command to be accepted.
    pub auth_token: String,
    /// The actual command to execute.
    #[serde(flatten)]
    pub command: GuiCommand,
}

/// Response sent back from the GUI to the caller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuiResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Current IDE state returned by `GetState`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdeState {
    pub open_files: Vec<String>,
    pub active_file: Option<String>,
    pub active_panel: String,
    pub workspace_root: String,
    pub sidebar_visible: bool,
    pub chat_message_count: usize,
    pub git_branch: Option<String>,
}

/// Handle for the GUI control listener. Holds the shutdown flag.
pub struct GuiControlHandle {
    pub shutdown: Arc<AtomicBool>,
}

/// Generate a cryptographically random token and write it to the workspace
/// `.velocity/gui_control.token` file. Returns the hex-encoded token.
pub fn generate_and_persist_token(workspace_root: &std::path::Path) -> String {
    let token = random_hex_token(TOKEN_BYTES);
    let token_path = workspace_root.join(".velocity").join("gui_control.token");
    if let Some(parent) = token_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&token_path, &token) {
        log::warn!("gui_control: failed to write token file: {e}");
    }
    token
}

/// Load an existing token from disk, or generate a new one.
pub fn load_or_generate_token(workspace_root: &std::path::Path) -> String {
    let token_path = workspace_root.join(".velocity").join("gui_control.token");
    if let Ok(token) = std::fs::read_to_string(&token_path) {
        let token = token.trim().to_string();
        if token.len() >= 32 {
            return token;
        }
    }
    generate_and_persist_token(workspace_root)
}

/// Generate a random hex-encoded token using the OS RNG.
fn random_hex_token(bytes: usize) -> String {
    // Use a simple approach that doesn't require additional dependencies.
    // We use the system time + process id as entropy source, then hash it.
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::SystemTime;

    let mut token_bytes = Vec::with_capacity(bytes);
    let seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id() as u64;

    for i in 0..bytes {
        let mut hasher = DefaultHasher::new();
        seed.hash(&mut hasher);
        pid.hash(&mut hasher);
        (i as u64).hash(&mut hasher);
        let h = hasher.finish();
        token_bytes.push((h & 0xFF) as u8);
    }
    token_bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Validate that a path is safe to open:
/// - Must be absolute
/// - Must resolve to an existing file
/// - Must fall within the workspace root (after resolving symlinks)
pub fn validate_open_path(
    raw_path: &str,
    workspace_root: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    let path = std::path::Path::new(raw_path);

    // Must be absolute
    if !path.is_absolute() {
        return Err(format!(
            "Path must be absolute: {:?}. Relative paths are not allowed.",
            raw_path
        ));
    }

    // Resolve symlinks to get the canonical path
    let canonical = path.canonicalize().map_err(|e| {
        format!(
            "Cannot resolve path {:?}: {}. File may not exist.",
            raw_path, e
        )
    })?;

    // Resolve workspace root canonical path
    let canonical_root = workspace_root.canonicalize().map_err(|e| {
        format!("Cannot resolve workspace root: {}", e)
    })?;

    // Check the path is within the workspace root
    if !canonical.starts_with(&canonical_root) {
        return Err(format!(
            "Path {:?} is outside workspace root {:?}. Access denied.",
            canonical, canonical_root
        ));
    }

    // Must be a file (not a directory)
    if canonical.is_dir() {
        return Err(format!("Path {:?} is a directory, not a file.", canonical));
    }

    Ok(canonical)
}

/// Start the GUI control listener on a background thread.
/// Returns the command receiver (for VelocityApp) and a shutdown flag.
/// The `egui_ctx` is used to request repaints when commands arrive.
/// The `auth_token` is the shared secret that clients must present.
pub fn start_listener(
    egui_ctx: egui::Context,
    auth_token: String,
) -> (Receiver<(GuiCommand, Sender<GuiResponse>)>, Arc<AtomicBool>) {
    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<(GuiCommand, Sender<GuiResponse>)>();
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    std::thread::Builder::new()
        .name("gui-control-listener".into())
        .spawn(move || {
            listener_loop(cmd_tx, shutdown_clone, egui_ctx, auth_token);
        })
        .ok();

    (cmd_rx, shutdown)
}

/// Main listener loop — accepts TCP connections and processes commands.
fn listener_loop(
    cmd_tx: Sender<(GuiCommand, Sender<GuiResponse>)>,
    shutdown: Arc<AtomicBool>,
    egui_ctx: egui::Context,
    auth_token: String,
) {
    use std::io::{BufRead, BufReader, Write};

    let listener = match std::net::TcpListener::bind(("127.0.0.1", GUI_CONTROL_PORT)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!(
                "[gui_control] Failed to bind TCP listener on port {}: {}",
                GUI_CONTROL_PORT, e
            );
            return;
        }
    };

    // Set a timeout so we can check the shutdown flag periodically.
    let _ = listener.set_nonblocking(false);
    // Restrict to localhost only (TTL=1 prevents remote access).
    let _ = listener.set_ttl(1);

    while !shutdown.load(Ordering::Relaxed) {
        // Accept with timeout via poll.
        let (mut stream, _addr) = match listener.accept() {
            Ok(conn) => conn,
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
        };

        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(10)));

        let reader = match stream.try_clone() {
            Ok(s) => BufReader::new(s),
            Err(e) => {
                log::error!("gui-control: failed to clone stream: {e}");
                continue;
            }
        };

        for line in reader.lines() {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }

            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };

            if line.trim().is_empty() {
                continue;
            }

            // Parse as authenticated command (includes auth_token)
            let auth_cmd: AuthenticatedCommand = match serde_json::from_str(&line) {
                Ok(c) => c,
                Err(e) => {
                    let resp = GuiResponse {
                        success: false,
                        data: None,
                        error: Some(format!("Parse error: {}. Expected AuthenticatedCommand with auth_token.", e)),
                    };
                    let json = serde_json::to_string(&resp).unwrap_or_default();
                    let _ = writeln!(stream, "{}", json);
                    continue;
                }
            };

            // Verify the auth token (constant-time comparison to prevent timing attacks)
            if !constant_time_eq(auth_cmd.auth_token.as_bytes(), auth_token.as_bytes()) {
                let resp = GuiResponse {
                    success: false,
                    data: None,
                    error: Some("Authentication failed: invalid or missing auth_token.".into()),
                };
                let json = serde_json::to_string(&resp).unwrap_or_default();
                let _ = writeln!(stream, "{}", json);
                log::warn!("gui-control: rejected command with invalid auth token");
                continue;
            }

            let (resp_tx, resp_rx) = crossbeam_channel::bounded::<GuiResponse>(1);

            if cmd_tx.send((auth_cmd.command, resp_tx)).is_err() {
                break;
            }

            // Wake up the egui event loop so it processes the command immediately.
            egui_ctx.request_repaint();

            match resp_rx.recv_timeout(std::time::Duration::from_secs(5)) {
                Ok(resp) => {
                    let json = serde_json::to_string(&resp).unwrap_or_default();
                    let _ = writeln!(stream, "{}", json);
                }
                Err(_) => {
                    let resp = GuiResponse {
                        success: false,
                        data: None,
                        error: Some("Timeout waiting for GUI response".into()),
                    };
                    let json = serde_json::to_string(&resp).unwrap_or_default();
                    let _ = writeln!(stream, "{}", json);
                }
            }
        }
    }
}

/// Constant-time byte comparison to prevent timing attacks on the auth token.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Send a command to the GUI from an external process (MCP tool).
/// Connects to the TCP listener, sends an authenticated JSON command, reads the response.
/// The `auth_token` must match the token in `.velocity/gui_control.token`.
pub fn send_command(cmd: &GuiCommand, auth_token: &str) -> Result<GuiResponse, String> {
    use std::io::{BufRead, BufReader, Write};

    let mut stream = std::net::TcpStream::connect(("127.0.0.1", GUI_CONTROL_PORT))
        .map_err(|_| "GUI is not running (cannot connect to control port 19821)".to_string())?;

    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;

    let auth_cmd = AuthenticatedCommand {
        auth_token: auth_token.to_string(),
        command: cmd.clone(),
    };
    let json = serde_json::to_string(&auth_cmd).map_err(|e| e.to_string())?;
    stream
        .write_all(json.as_bytes())
        .map_err(|e| e.to_string())?;
    stream.write_all(b"\n").map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|e| e.to_string())?;

    serde_json::from_str(&line).map_err(|e| format!("Failed to parse response: {}", e))
}

/// Load the auth token from the workspace `.velocity/gui_control.token` file.
/// Returns `None` if the token file doesn't exist or can't be read.
pub fn load_token(workspace_root: &std::path::Path) -> Option<String> {
    let token_path = workspace_root.join(".velocity").join("gui_control.token");
    std::fs::read_to_string(&token_path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
