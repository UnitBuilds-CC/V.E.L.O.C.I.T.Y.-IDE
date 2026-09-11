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

use crossbeam_channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// TCP port for GUI control.
pub const GUI_CONTROL_PORT: u16 = 19821;

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

/// Start the GUI control listener on a background thread.
/// Returns the command receiver (for VelocityApp) and a shutdown flag.
/// The `egui_ctx` is used to request repaints when commands arrive.
pub fn start_listener(
    egui_ctx: egui::Context,
) -> (
    Receiver<(GuiCommand, Sender<GuiResponse>)>,
    Arc<AtomicBool>,
) {
    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<(GuiCommand, Sender<GuiResponse>)>();
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    std::thread::Builder::new()
        .name("gui-control-listener".into())
        .spawn(move || {
            listener_loop(cmd_tx, shutdown_clone, egui_ctx);
        })
        .expect("failed to spawn gui-control-listener thread");

    (cmd_rx, shutdown)
}

/// Main listener loop — accepts TCP connections and processes commands.
fn listener_loop(
    cmd_tx: Sender<(GuiCommand, Sender<GuiResponse>)>,
    shutdown: Arc<AtomicBool>,
    egui_ctx: egui::Context,
) {
    use std::io::{BufRead, BufReader, Write};

    let listener = match std::net::TcpListener::bind(("127.0.0.1", GUI_CONTROL_PORT)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[gui_control] Failed to bind TCP listener on port {}: {}", GUI_CONTROL_PORT, e);
            return;
        }
    };

    // Set a timeout so we can check the shutdown flag periodically.
    let _ = listener.set_nonblocking(false);
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

        let reader = BufReader::new(stream.try_clone().unwrap_or_else(|_| {
            panic!("failed to clone stream")
        }));

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

            let cmd: GuiCommand = match serde_json::from_str(&line) {
                Ok(c) => c,
                Err(e) => {
                    let resp = GuiResponse {
                        success: false,
                        data: None,
                        error: Some(format!("Parse error: {}", e)),
                    };
                    let json = serde_json::to_string(&resp).unwrap_or_default();
                    let _ = writeln!(stream, "{}", json);
                    continue;
                }
            };

            let (resp_tx, resp_rx) = crossbeam_channel::bounded::<GuiResponse>(1);

            if cmd_tx.send((cmd, resp_tx)).is_err() {
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

/// Send a command to the GUI from an external process (MCP tool).
/// Connects to the TCP listener, sends a JSON command, reads the response.
pub fn send_command(cmd: &GuiCommand) -> Result<GuiResponse, String> {
    use std::io::{BufRead, BufReader, Write};

    let mut stream = std::net::TcpStream::connect(("127.0.0.1", GUI_CONTROL_PORT))
        .map_err(|_| "GUI is not running (cannot connect to control port 19821)".to_string())?;

    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;

    let json = serde_json::to_string(cmd).map_err(|e| e.to_string())?;
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
