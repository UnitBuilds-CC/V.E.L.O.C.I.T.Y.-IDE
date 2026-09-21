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
//! VelocityApp::process_gui_commands(ctx)
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
    /// Select one of the eight activity-bar rails by category name (`files`,
    /// `git`, ...). Not the same axis as `TogglePanel`: this moves the left rail,
    /// that opens a tab in the centre.
    NavigatePanel { panel: String },
    /// Open (or close, if already focused) a central panel tab: Settings, Wiki,
    /// Graph, ... Drives the same `toggle_panel` the gear and Ctrl+, call.
    TogglePanel { panel: String },
    /// Capture the IDE's own window and save it to disk. An empty `path` picks
    /// a timestamped name under `<workspace>/.velocity/screenshots/`; a given
    /// path has to stay inside the workspace.
    Screenshot {
        path: String,
        /// A second image inside the workspace to compare the capture against,
        /// so a caller can ask whether the screen actually changed instead of
        /// only where its bytes landed. Optional with a default so an older MCP
        /// binary that sends no such field still works against a newer GUI.
        #[serde(default)]
        against: Option<String>,
    },
    /// Close the IDE.
    Quit {},
    /// Enumerate the command palette: label, category, shortcut and risk tier.
    /// This is what makes "every button" a list rather than a guess.
    ListCommands {
        #[serde(default)]
        category: Option<String>,
    },
    /// Invoke a palette command by label. Navigation-tier commands always run;
    /// modify- and execute-tier ones are refused unless `allow_unsafe` is set,
    /// so a sweep cannot kick off a build or approve pending tools by accident.
    RunCommand {
        label: String,
        #[serde(default)]
        allow_unsafe: Option<bool>,
    },
    /// The whole navigable surface as nodes and edges, optionally with the
    /// shortest route between two of them.
    AppMap {
        #[serde(default)]
        from: Option<String>,
        #[serde(default)]
        to: Option<String>,
    },
    /// Get a panel, rail or sub-tab on screen, working out the route from the
    /// map. Takes a name (`settings`, `rail:agents`, `agents:orchestration`) or
    /// an exact node id.
    NavigateTo { target: String },
    /// List the tabs the central dock currently holds, in dock order, with the
    /// focused one marked.
    ListTabs {},
    /// Give a dock tab the focus, the way clicking its tab header would.
    /// `tab` matches a `ListTabs` `id`, a title, or a panel slug.
    SelectTab { tab: String },
    /// Pick a sub-tab within an activity-bar rail (`agents:orchestration`).
    SelectSubTab { rail: String, sub_tab: String },
    /// Close every transient overlay -- palettes, switchers, in-app dialogs and
    /// find/replace -- the way a run of Escape presses would. The route back to
    /// a known state once something has been raised, since Escape is only heard
    /// by the overlay that currently owns the frame.
    DismissOverlays {},
    /// Answer whichever path prompt is on screen -- the Open File or Save As
    /// dialog -- with the value a person would have typed into its box, and
    /// press its button. The completing half of `DismissOverlays`: without it a
    /// driver can raise a prompt and stand it down but never get anything done
    /// through one, which left the only tested path through those dialogs the
    /// one that discards. The value is held to the workspace exactly as
    /// `OpenFile`'s path is.
    SubmitDialog { value: String },
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
    /// Title of the tab holding the central area ("Settings", "Chat", ...).
    #[serde(default)]
    pub focused_tab: Option<String>,
    /// `"dock"` or `"welcome"`: which host is actually on screen in the central
    /// area. A panel tab only renders inside the dock, so this is what tells a
    /// driver whether its open-the-settings-panel request became visible.
    #[serde(default)]
    pub central_area: String,
    /// The selected sub-tab within [`IdeState::active_panel`], by label. The
    /// rails carry up to six sub-tabs each, and a driver that only sees
    /// `agents` cannot tell whether it is looking at Activity or at Metrics.
    #[serde(default)]
    pub active_section: Option<String>,
    /// The workspace profile the layout is following. Commands are gated on it,
    /// so a refusal needs it to be explainable.
    #[serde(default)]
    pub mode: String,
    /// Transient overlays currently on screen: command palette, quick open, the
    /// in-app file dialogs, find/replace, ... A driver raises these and needs to
    /// know both that one took and that it has since stood them back down --
    /// `DismissOverlays` reports what it closed, which is not the same as proof
    /// nothing is left.
    #[serde(default)]
    pub open_overlays: Vec<String>,
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

/// Generate a random hex-encoded token using the OS cryptographic RNG.
fn random_hex_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("cryptographic RNG unavailable");
    buf.iter().map(|b| format!("{:02x}", b)).collect()
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
    let canonical_root = workspace_root
        .canonicalize()
        .map_err(|e| format!("Cannot resolve workspace root: {}", e))?;

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

/// How long the listener keeps trying to bind before giving up.
///
/// Windows parks the `(address, port)` tuple of a killed process's connections
/// in TIME_WAIT for up to twice the MSL -- about two minutes -- and
/// [`std::net::TcpListener::bind`] deliberately does not set `SO_REUSEADDR` on
/// Windows. Restarting straight after a crash therefore finds the control port
/// briefly unbindable. Binding once and returning left an IDE that looked
/// perfectly healthy on screen while being permanently deaf to the bridge, with
/// nothing to show for it but an `eprintln!` no windowed launch displays.
const BIND_ATTEMPTS: usize = 12;

/// Spacing between attempts; 12 × 15 s clears the two-minute TIME_WAIT window.
const BIND_RETRY_GAP: std::time::Duration = std::time::Duration::from_millis(15_000);

/// How often a wait checks the shutdown flag, so quitting is never held up by
/// the retry backoff.
const SHUTDOWN_POLL: std::time::Duration = std::time::Duration::from_millis(100);

/// Bind the control port, retrying across the window in which a dying instance
/// can still be holding it. `None` means it never came free.
///
/// The port is a parameter rather than [`GUI_CONTROL_PORT`] so the retry path is
/// testable: a test that bound the real port would collide with whichever IDE
/// the developer currently has open.
fn bind_control_port(
    port: u16,
    attempts: usize,
    gap: std::time::Duration,
    shutdown: &AtomicBool,
) -> Option<std::net::TcpListener> {
    for attempt in 0..attempts.max(1) {
        match std::net::TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => {
                if attempt > 0 {
                    log::info!("gui_control: bound port {port} on attempt {}", attempt + 1);
                }
                return Some(listener);
            }
            Err(e) => {
                log::error!(
                    "gui_control: bind of port {port} failed (attempt {}/{}): {e}",
                    attempt + 1,
                    attempts.max(1)
                );
                // Either the last attempt has been used, or the wait was cut
                // short because the app is closing. Both mean stop trying.
                if attempt + 1 >= attempts || sleep_through(gap, shutdown) {
                    return None;
                }
            }
        }
    }
    None
}

/// Nap for `gap` in [`SHUTDOWN_POLL`]-sized pieces, reporting whether `shutdown`
/// was set while waiting.
fn sleep_through(gap: std::time::Duration, shutdown: &AtomicBool) -> bool {
    let mut waited = std::time::Duration::ZERO;
    while waited < gap {
        if shutdown.load(Ordering::Relaxed) {
            return true;
        }
        let nap = gap.min(SHUTDOWN_POLL);
        std::thread::sleep(nap);
        waited += nap;
    }
    shutdown.load(Ordering::Relaxed)
}

/// Main listener loop — accepts TCP connections and processes commands.
fn listener_loop(
    cmd_tx: Sender<(GuiCommand, Sender<GuiResponse>)>,
    shutdown: Arc<AtomicBool>,
    egui_ctx: egui::Context,
    auth_token: String,
) {
    use std::io::{BufRead, BufReader, Write};

    let listener =
        match bind_control_port(GUI_CONTROL_PORT, BIND_ATTEMPTS, BIND_RETRY_GAP, &shutdown) {
            Some(listener) => listener,
            None => {
                // Outlasted the whole retry window. Loudly, in both channels: with
                // no listener the app is invisible to the MCP tools and to any agent
                // driving them, and the alternative is a person concluding the IDE
                // simply ignores them.
                let reason = format!(
                    "[gui_control] Port {GUI_CONTROL_PORT} is still in use after {}s of retrying. \
                 The IDE is running but cannot be controlled over the bridge; close the other \
                 Velocity instance or wait for the port to be released and restart this one.",
                    (BIND_RETRY_GAP * BIND_ATTEMPTS as u32).as_secs()
                );
                log::error!("{reason}");
                eprintln!("{reason}");
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
                        error: Some(format!(
                            "Parse error: {}. Expected AuthenticatedCommand with auth_token.",
                            e
                        )),
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

            // How long the bridge waits for the UI thread to run this command.
            // It has to be longer than the slowest legitimate frame, not longer
            // than an ideal one: on a machine doing real work (a compile, an
            // antivirus scan of a fresh binary) the UI thread can take seconds to
            // reach the head of the queue, and a short window answers "Timeout
            // waiting for GUI response" to a healthy IDE -- which every driver
            // reads as a crash, and which is unrecoverable-looking from the
            // outside. Note this is a reply deadline, not a cancellation: a
            // command that answers late has still been dispatched and still runs.
            match resp_rx.recv_timeout(std::time::Duration::from_secs(REPLY_WINDOW_SECS)) {
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

/// Seconds the control bridge holds a command's socket open waiting for the UI
/// thread to answer. It is the slowest legitimate frame, not the ideal one, that
/// has to fit inside this window.
///
/// Anything talking to the bridge has to wait *longer* than it or the caller sees
/// a dropped connection where the IDE never sent one: [`send_command`] uses
/// [`REPLY_WINDOW_SECS`] plus slack, and `sweep_gui.ps1` defaults its own
/// `-TimeoutMs` above that again. Keeping the three in one place is the point --
/// they used to be 5, 10 and 20 seconds, so the IDE gave up answering before its
/// own client did and reported a timeout as if it were a crash.
const REPLY_WINDOW_SECS: u64 = 20;

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
        .set_read_timeout(Some(std::time::Duration::from_secs(REPLY_WINDOW_SECS + 10)))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The command crosses the wire as `{"command": ..., "params": {...}}`
    /// flattened beside `auth_token`. A new variant that does not survive that
    /// shape is silently dropped by the listener, so pin the bytes.
    #[test]
    fn toggle_panel_uses_the_tagged_wire_shape() {
        let cmd = AuthenticatedCommand {
            auth_token: "tok".into(),
            command: GuiCommand::TogglePanel {
                panel: "settings".into(),
            },
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let parsed: AuthenticatedCommand = serde_json::from_str(&json).unwrap();
        assert!(json.contains("\"command\":\"TogglePanel\""), "{json}");
        assert!(json.contains("\"panel\":\"settings\""), "{json}");
        match parsed.command {
            GuiCommand::TogglePanel { panel } => assert_eq!(panel, "settings"),
            other => panic!("round-trip changed the command: {other:?}"),
        }
    }

    /// `velocity_mcp.exe` and the GUI binary are swapped independently, so a new
    /// MCP against an older GUI must still parse the state it gets back rather
    /// than failing the whole call over the fields added since.
    #[test]
    fn ide_state_reads_pre_reconciliation_payloads() {
        let older = r#"{
            "open_files": [],
            "active_file": null,
            "active_panel": "files",
            "workspace_root": "C:/ws",
            "sidebar_visible": true,
            "chat_message_count": 0,
            "git_branch": null
        }"#;
        let state: IdeState = serde_json::from_str(older).unwrap();
        assert_eq!(state.focused_tab, None);
        assert_eq!(state.central_area, "");
        // The rail's sub-tab may legitimately be absent (an empty rail), but a
        // mode never is -- a binary too old to report one reads as "", which a
        // driver can tell apart from a real profile name.
        assert_eq!(state.active_section, None);
        assert_eq!(state.mode, "");
        // An old GUI says nothing about overlays; "nothing reported" must not
        // parse as "something is stuck open".
        assert!(state.open_overlays.is_empty());
    }

    /// Every command the bridge accepts has to survive the adjacently-tagged
    /// envelope. A variant that does not is dropped by the listener with a parse
    /// error, which reads to a driver as "the IDE ignored me".
    #[test]
    fn every_command_survives_the_tagged_envelope() {
        let cases = vec![
            GuiCommand::ListCommands { category: None },
            GuiCommand::ListCommands {
                category: Some("Panels".into()),
            },
            GuiCommand::RunCommand {
                label: "Toggle Sidebar".into(),
                allow_unsafe: None,
            },
            GuiCommand::RunCommand {
                label: "Build".into(),
                allow_unsafe: Some(true),
            },
            GuiCommand::AppMap {
                from: None,
                to: Some("panel:settings".into()),
            },
            GuiCommand::NavigateTo {
                target: "rail:git/changes".into(),
            },
            GuiCommand::ListTabs {},
            GuiCommand::SelectTab { tab: "3".into() },
            GuiCommand::SelectSubTab {
                rail: "agents".into(),
                sub_tab: "orchestration".into(),
            },
            GuiCommand::DismissOverlays {},
            GuiCommand::Screenshot {
                path: "shots/now.png".into(),
                against: None,
            },
            GuiCommand::Screenshot {
                path: "shots/now.png".into(),
                against: Some("shots/before.png".into()),
            },
            GuiCommand::SubmitDialog {
                value: "notes/todo.md".into(),
            },
        ];
        for command in cases {
            let envelope = AuthenticatedCommand {
                auth_token: "tok".into(),
                command: command.clone(),
            };
            let json = serde_json::to_string(&envelope).unwrap();
            assert!(
                json.contains("\"params\""),
                "missing params wrapper: {json}"
            );
            let parsed: AuthenticatedCommand = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{json} did not parse back: {e}"));
            match (command, parsed.command) {
                (
                    GuiCommand::RunCommand {
                        label,
                        allow_unsafe,
                    },
                    GuiCommand::RunCommand {
                        label: got,
                        allow_unsafe: got_flag,
                    },
                ) => {
                    // The whole point of `#[serde(default)]` here: an omitted
                    // flag has to stay `None` so the handler can default it to
                    // "do not run anything that writes".
                    assert_eq!(label, got);
                    assert_eq!(allow_unsafe, got_flag, "{json}");
                }
                (a, b) => assert_eq!(format!("{a:?}"), format!("{b:?}"), "{json} changed"),
            }
        }
    }

    #[test]
    fn a_run_command_may_omit_the_unsafe_flag() {
        let parsed: AuthenticatedCommand = serde_json::from_str(
            r#"{"auth_token":"t","command":"RunCommand","params":{"label":"Build"}}"#,
        )
        .unwrap();
        match parsed.command {
            GuiCommand::RunCommand {
                label,
                allow_unsafe,
            } => {
                assert_eq!(label, "Build");
                assert_eq!(allow_unsafe, None);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    // ─── Recovering the control port after a crash ─────────────────────────

    /// A port we know is taken, plus the listener taking it. Never
    /// [`GUI_CONTROL_PORT`]: binding the real one would collide with whatever
    /// IDE the person running the suite already has open.
    fn occupied_port() -> (u16, std::net::TcpListener) {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        (port, listener)
    }

    #[test]
    fn a_port_still_held_by_a_dying_instance_is_retried_until_it_frees_up() {
        let (port, held) = occupied_port();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(80));
            drop(held);
        });
        let shutdown = AtomicBool::new(false);
        let bound = bind_control_port(port, 50, std::time::Duration::from_millis(20), &shutdown);
        assert!(
            bound.is_some(),
            "the retry loop gave up on a port that came free mid-wait"
        );
    }

    #[test]
    fn retrying_stops_after_the_allotted_attempts() {
        // Without a ceiling the listener thread spins against a port someone
        // else has legitimately taken -- another IDE session -- forever.
        let (port, _held) = occupied_port();
        let shutdown = AtomicBool::new(false);
        let started = std::time::Instant::now();
        assert!(
            bind_control_port(port, 3, std::time::Duration::from_millis(5), &shutdown).is_none()
        );
        // Two naps, not twelve: the last attempt must not sleep before returning.
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "gave up after {:?}, so it slept past its attempts",
            started.elapsed()
        );
    }

    #[test]
    fn the_retry_wait_is_abandoned_when_the_app_closes() {
        // Quitting must not queue behind a 15 s backoff. This is the whole
        // reason the sleep is sliced rather than one long `thread::sleep`.
        let (port, _held) = occupied_port();
        let shutdown = AtomicBool::new(true);
        let started = std::time::Instant::now();
        assert!(bind_control_port(
            port,
            BIND_ATTEMPTS,
            std::time::Duration::from_secs(30),
            &shutdown
        )
        .is_none());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "waited {:?} on a shutdown that had already been asked for",
            started.elapsed()
        );
    }
}
