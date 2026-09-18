//! GUI command processing — executes commands received from external processes
//! (MCP server, AI agents) on the main UI thread.

use super::struct_def::VelocityApp;
use crate::editor::gui_control::{GuiCommand, GuiResponse, IdeState};
use eframe::egui;

impl VelocityApp {
    /// Process any pending GUI control commands from external processes.
    /// Called at the start of each egui frame.
    /// `ctx` is used for viewport-level actions (e.g. closing the window on
    /// a remote quit request).
    pub fn process_gui_commands(&mut self, ctx: &egui::Context) {
        // Take the receiver out temporarily to satisfy the borrow checker.
        let rx = match self.gui_cmd_rx.take() {
            Some(rx) => rx,
            None => return,
        };

        // Process all pending commands (non-blocking).
        while let Ok((cmd, resp_tx)) = rx.try_recv() {
            let response = self.execute_gui_command(cmd, ctx);
            let _ = resp_tx.send(response);
        }

        // Put the receiver back.
        self.gui_cmd_rx = Some(rx);
    }

    /// Execute a single GUI command and return the response.
    fn execute_gui_command(&mut self, cmd: GuiCommand, ctx: &egui::Context) -> GuiResponse {
        match cmd {
            GuiCommand::OpenFile { path } => self.cmd_open_file(path),
            GuiCommand::GetState {} => self.cmd_get_state(),
            GuiCommand::NavigatePanel { panel } => self.cmd_navigate_panel(panel),
            GuiCommand::TogglePanel { panel } => self.cmd_toggle_panel(panel),
            GuiCommand::Screenshot { path } => self.cmd_screenshot(path),
            GuiCommand::Quit {} => self.cmd_quit(ctx),
        }
    }

    /// Open a file in the editor.
    /// Validates the path is absolute, resolves symlinks, and ensures it's
    /// within the workspace root.
    fn cmd_open_file(&mut self, path: String) -> GuiResponse {
        // Validate path security: must be absolute, within workspace, no symlink escapes
        let validated_path =
            match crate::editor::gui_control::validate_open_path(&path, &self.workspace_root) {
                Ok(p) => p,
                Err(e) => {
                    return GuiResponse {
                        success: false,
                        data: None,
                        error: Some(e),
                    };
                }
            };

        self.open_editor(Some(validated_path.clone()));
        GuiResponse {
            success: true,
            data: Some(serde_json::json!({ "opened": validated_path.display().to_string() })),
            error: None,
        }
    }

    /// Get current IDE state.
    fn cmd_get_state(&mut self) -> GuiResponse {
        let open_files: Vec<String> = self
            .tabs
            .iter()
            .filter_map(|tab| {
                if let crate::editor::app::types::TabKind::Editor {
                    path: Some(ref p), ..
                } = tab.kind
                {
                    Some(p.display().to_string())
                } else {
                    None
                }
            })
            .collect();

        let active_file = self
            .active_tab
            .as_ref()
            .and_then(|id| self.tabs.iter().find(|t| &t.id == id))
            .and_then(|tab| {
                if let crate::editor::app::types::TabKind::Editor {
                    path: Some(ref p), ..
                } = tab.kind
                {
                    Some(p.display().to_string())
                } else {
                    None
                }
            });

        let panel_names = [
            "files",
            "search",
            "git",
            "chat",
            "build",
            "agents",
            "knowledge",
            "workspace",
        ];
        let active_panel = panel_names
            .get(self.activity_bar_selection)
            .unwrap_or(&"unknown")
            .to_string();

        // What the central area is actually showing. Read the same way the frame
        // reads it, so a driver can tell "the tab exists and is drawing" apart
        // from "the tab exists but the welcome screen owns the panel".
        let focused_tab =
            crate::editor::app::types::focused_tab(&self.tabs, self.active_tab.as_ref())
                .map(|tab| tab.title());
        let central_area = if self.central_shows_dock() {
            "dock"
        } else {
            "welcome"
        }
        .to_string();

        let state = IdeState {
            open_files,
            active_file,
            active_panel,
            workspace_root: self.workspace_root.display().to_string(),
            sidebar_visible: self.left_sidebar_visible,
            chat_message_count: self.chat.messages.len(),
            git_branch: if self.git_state.branch.is_empty() {
                None
            } else {
                Some(self.git_state.branch.clone())
            },
            focused_tab,
            central_area,
        };

        GuiResponse {
            success: true,
            data: Some(serde_json::to_value(&state).unwrap_or_default()),
            error: None,
        }
    }

    /// Open (or, if already focused, close) a central panel tab. Goes through
    /// `toggle_panel` -- the identical entry point the activity-bar gear, the
    /// menu item, Ctrl+, and the status-bar provider chip use -- so a driver
    /// reaching Settings by this route exercises the same code a click does.
    fn cmd_toggle_panel(&mut self, panel: String) -> GuiResponse {
        let kind = match crate::editor::app::types::panel_kind_from_name(&panel) {
            Some(kind) => kind,
            None => {
                return GuiResponse {
                    success: false,
                    data: None,
                    error: Some(format!(
                        "Unknown panel '{panel}'. Valid: {:?}",
                        crate::editor::app::types::bridge_panel_names()
                    )),
                };
            }
        };

        self.toggle_panel(kind);
        // Report what the central area ended up showing: opening can equally
        // have toggled the tab closed, and the caller should not have to guess.
        GuiResponse {
            success: true,
            data: Some(serde_json::json!({
                "panel": panel,
                "focused_tab": crate::editor::app::types::focused_tab(
                    &self.tabs,
                    self.active_tab.as_ref()
                )
                .map(|tab| tab.title()),
                "central_area": if self.central_shows_dock() {
                    "dock"
                } else {
                    "welcome"
                },
            })),
            error: None,
        }
    }

    /// Navigate to a specific activity bar panel.
    fn cmd_navigate_panel(&mut self, panel: String) -> GuiResponse {
        let panel_names = [
            "files",
            "search",
            "git",
            "chat",
            "build",
            "agents",
            "knowledge",
            "workspace",
        ];

        let idx = panel_names.iter().position(|&p| p == panel);
        match idx {
            Some(i) => {
                self.activity_bar_selection = i;
                self.left_sidebar_visible = true;
                GuiResponse {
                    success: true,
                    data: Some(serde_json::json!({ "panel": panel })),
                    error: None,
                }
            }
            None => GuiResponse {
                success: false,
                data: None,
                error: Some(format!(
                    "Unknown panel '{}'. Valid: {:?}",
                    panel, panel_names
                )),
            },
        }
    }

    /// Capture a screenshot and save to disk.
    fn cmd_screenshot(&mut self, _path: String) -> GuiResponse {
        // egui doesn't have built-in screenshot capture from the app side.
        // This would require the egui Context to capture the next frame.
        // For now, return a not-implemented response.
        GuiResponse {
            success: false,
            data: None,
            error: Some("Screenshot capture not yet implemented".into()),
        }
    }

    /// Quit the IDE. Sends a viewport Close command through the egui
    /// context, which drives the normal window-close flow — eframe still
    /// calls `on_exit`, so workspace preferences are saved.
    fn cmd_quit(&mut self, ctx: &egui::Context) -> GuiResponse {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        GuiResponse {
            success: true,
            data: Some(serde_json::json!({ "quitting": true })),
            error: None,
        }
    }
}
