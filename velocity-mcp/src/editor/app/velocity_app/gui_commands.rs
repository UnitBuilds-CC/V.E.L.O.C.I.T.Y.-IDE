//! GUI command processing — executes commands received from external processes
//! (MCP server, AI agents) on the main UI thread.

use super::struct_def::VelocityApp;
use crate::editor::gui_control::{GuiCommand, GuiResponse, IdeState};

impl VelocityApp {
    /// Process any pending GUI control commands from external processes.
    /// Called at the start of each egui frame.
    pub fn process_gui_commands(&mut self) {
        // Take the receiver out temporarily to satisfy the borrow checker.
        let rx = match self.gui_cmd_rx.take() {
            Some(rx) => rx,
            None => return,
        };

        // Process all pending commands (non-blocking).
        while let Ok((cmd, resp_tx)) = rx.try_recv() {
            let response = self.execute_gui_command(cmd);
            let _ = resp_tx.send(response);
        }

        // Put the receiver back.
        self.gui_cmd_rx = Some(rx);
    }

    /// Execute a single GUI command and return the response.
    fn execute_gui_command(&mut self, cmd: GuiCommand) -> GuiResponse {
        match cmd {
            GuiCommand::OpenFile { path } => self.cmd_open_file(path),
            GuiCommand::GetState {} => self.cmd_get_state(),
            GuiCommand::NavigatePanel { panel } => self.cmd_navigate_panel(panel),
            GuiCommand::Screenshot { path } => self.cmd_screenshot(path),
            GuiCommand::Quit {} => self.cmd_quit(),
        }
    }

    /// Open a file in the editor.
    fn cmd_open_file(&mut self, path: String) -> GuiResponse {
        let full_path = self.workspace_root.join(&path);
        if !full_path.exists() {
            return GuiResponse {
                success: false,
                data: None,
                error: Some(format!("File not found: {}", path)),
            };
        }

        self.open_editor(Some(full_path));
        GuiResponse {
            success: true,
            data: Some(serde_json::json!({ "opened": path })),
            error: None,
        }
    }

    /// Get current IDE state.
    fn cmd_get_state(&mut self) -> GuiResponse {
        let open_files: Vec<String> = self
            .tabs
            .iter()
            .filter_map(|tab| {
                if let crate::editor::app::types::TabKind::Editor { path: Some(ref p), .. } = tab.kind {
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
                if let crate::editor::app::types::TabKind::Editor { path: Some(ref p), .. } = tab.kind {
                    Some(p.display().to_string())
                } else {
                    None
                }
            });

        let panel_names = [
            "files", "search", "git", "chat", "build", "agents", "knowledge", "workspace",
        ];
        let active_panel = panel_names
            .get(self.activity_bar_selection)
            .unwrap_or(&"unknown")
            .to_string();

        let state = IdeState {
            open_files,
            active_file,
            active_panel,
            workspace_root: self.workspace_root.display().to_string(),
            sidebar_visible: self.left_sidebar_visible,
            chat_message_count: self.chat.messages.len(),
            git_branch: if self.git_state.branch.is_empty() { None } else { Some(self.git_state.branch.clone()) },
        };

        GuiResponse {
            success: true,
            data: Some(serde_json::to_value(&state).unwrap_or_default()),
            error: None,
        }
    }

    /// Navigate to a specific activity bar panel.
    fn cmd_navigate_panel(&mut self, panel: String) -> GuiResponse {
        let panel_names = [
            "files", "search", "git", "chat", "build", "agents", "knowledge", "workspace",
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

    /// Quit the IDE.
    fn cmd_quit(&mut self) -> GuiResponse {
        // Signal the egui event loop to stop.
        // The actual quit happens via the egui context.
        GuiResponse {
            success: true,
            data: Some(serde_json::json!({ "quitting": true })),
            error: None,
        }
    }
}
