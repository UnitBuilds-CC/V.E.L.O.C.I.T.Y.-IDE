use std::path::PathBuf;
use egui_dock::DockState;

use crate::agent::UiToAgentMessage;
use crate::editor::buffer::EditorBuffer;
use super::super::types::*;
use super::super::helpers::*;
use super::struct_def::VelocityApp;

impl VelocityApp {
    pub fn commands(&self) -> Vec<Command> {
        vec![
            Command {
                label: "Command Palette…",
                action: |a| a.open_command_palette(),
            },
            Command {
                label: "Refresh Models",
                action: |a| a.refresh_models(),
            },
            Command {
                label: "Approve All Pending Tools",
                action: |a| a.approve_all_pending_tools(),
            },
            Command {
                label: "Decline All Pending Tools",
                action: |a| a.reject_all_pending_tools(),
            },
            Command {
                label: "Focus Agent Chat",
                action: |a| a.toggle_panel(TabKind::Chat),
            },
            Command {
                label: "Focus Mission Control",
                action: |a| a.toggle_panel(TabKind::MissionControl),
            },
            Command {
                label: "Focus Orchestrator",
                action: |a| a.toggle_panel(TabKind::Orchestrator),
            },
            Command {
                label: "Plan Routed Sub-Agents",
                action: |a| a.plan_routed_subagents(),
            },
            Command {
                label: "New File",
                action: |a| a.open_editor(None),
            },
            Command {
                label: "Open File…",
                action: |a| a.open_file_dialog(),
            },
            Command {
                label: "Save",
                action: |a| a.save_active(),
            },
            Command {
                label: "Save As…",
                action: |a| a.save_active_as(),
            },
            Command {
                label: "Save All",
                action: |a| a.save_all(),
            },
            Command {
                label: "Close Tab",
                action: |a| a.close_active_tab(),
            },
            Command {
                label: "Build",
                action: |a| a.build_active(),
            },
            Command {
                label: "Run",
                action: |a| a.run_active(),
            },
            Command {
                label: "Toggle Output",
                action: |a| a.toggle_panel(TabKind::Output),
            },
            Command {
                label: "Toggle Chat",
                action: |a| a.toggle_panel(TabKind::Chat),
            },
            Command {
                label: "Toggle Orchestrator",
                action: |a| a.toggle_panel(TabKind::Orchestrator),
            },
            Command {
                label: "Toggle Mission Control",
                action: |a| a.toggle_panel(TabKind::MissionControl),
            },
            Command {
                label: "Toggle Usage",
                action: |a| a.toggle_panel(TabKind::Usage),
            },
            Command {
                label: "Toggle Search",
                action: |a| a.toggle_panel(TabKind::Search),
            },
            Command {
                label: "Toggle Merkle Graph",
                action: |a| a.toggle_panel(TabKind::Graph),
            },
        ]
    }

    pub fn command_list_filtered(&self) -> Vec<Command> {
        let query = self.command_palette.query.to_lowercase();
        self.commands()
            .into_iter()
            .filter(|c| c.label.to_lowercase().contains(&query))
            .collect()
    }

    pub fn open_command_palette(&mut self) {
        self.command_palette.open = true;
        self.command_palette.query.clear();
        self.command_palette.selected = 0;
    }

    pub fn close_active_tab(&mut self) {
        if let Some(id) = self.active_tab.take() {
            self.close_tab(&id);
        } else if let Some(first) = self.tabs.first().cloned() {
            self.close_tab(&first.id);
        }
    }

    pub fn close_tab(&mut self, id: &TabId) {
        self.tabs.retain(|t| t.id != *id);
        self.buffers.remove(id);
        if self.active_tab.as_ref() == Some(id) {
            self.active_tab = self.tabs.first().map(|t| t.id.clone());
        }
    }

    pub fn open_editor(&mut self, path: Option<PathBuf>) {
        if let Some(ref p) = path {
            for tab in &self.tabs {
                if let TabKind::Editor {
                    path: Some(ref tab_path),
                    ..
                } = tab.kind
                {
                    if tab_path == p {
                        self.active_tab = Some(tab.id.clone());
                        return;
                    }
                }
            }
        }

        let id = TabId::next(&mut self.tab_counter);
        let tab = Tab {
            id: id.clone(),
            kind: TabKind::Editor {
                path: path.clone(),
                buffer_id: id.clone(),
            },
        };
        let mut buf = EditorBuffer::default();
        if let Some(ref p) = path {
            if let Ok(content) = std::fs::read_to_string(p) {
                buf.load_text(&content);
            } else {
                self.status_message = format!("Failed to read file: {:?}", p);
            }
        }
        self.buffers.insert(id.clone(), buf);
        self.tabs.push(tab.clone());
        if let Some(dock) = self.dock_state.as_mut() {
            dock.push_to_focused_leaf(tab);
        }
        self.active_tab = Some(id);
    }

    pub fn open_editor_stub(&mut self) {
        self.open_editor(None);
    }

    pub fn open_file_dialog(&mut self) {
        self.pending_open_path = Some(PathBuf::new());
    }

    pub fn save_active(&mut self) {
        let active = self.active_tab.clone();
        if let Some(id) = active {
            if let Some(path) = self.tab_path(&id).cloned() {
                self.save_buffer_to(&id, &path);
            } else {
                self.save_active_as();
            }
        } else {
            self.save_all();
        }
    }

    pub fn save_active_as(&mut self) {
        if self.active_tab.is_some() {
            self.pending_save_as_path = Some(PathBuf::new());
        } else {
            self.status_message = "No active editor to save".into();
        }
    }

    pub fn save_buffer_to(&mut self, id: &TabId, path: &PathBuf) -> bool {
        self.save_buffer_to_with_feedback(id, path, true)
    }

    pub fn save_buffer_to_with_feedback(
        &mut self,
        id: &TabId,
        path: &PathBuf,
        success_feedback: bool,
    ) -> bool {
        if let Some(buf) = self.buffers.get(id) {
            match std::fs::write(path, buf.content()) {
                Ok(_) => {
                    if success_feedback {
                        self.status_message = format!("Saved {}", path.display());
                        let filename = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                        self.toasts
                            .push(crate::editor::toast::Toast::success(format!(
                                "Saved {filename}"
                            )));
                    }
                    true
                }
                Err(e) => {
                    self.status_message = format!("Error saving {}: {}", path.display(), e);
                    self.toasts.push(crate::editor::toast::Toast::error(format!(
                        "Failed to save: {e}"
                    )));
                    false
                }
            }
        } else {
            self.status_message = format!("No buffer found for {}", path.display());
            self.toasts.push(crate::editor::toast::Toast::error(
                "Failed to save: missing buffer",
            ));
            false
        }
    }

    pub fn tab_path(&self, id: &TabId) -> Option<&PathBuf> {
        self.tabs.iter().find(|t| t.id == *id)?.editor_path()
    }

    pub fn active_change_preview(&self) -> Option<ActiveChangePreview> {
        let active_id = self.active_tab.as_ref()?;
        let path = self.tab_path(active_id)?;
        let buf = self.buffers.get(active_id)?;
        let disk_content = std::fs::read_to_string(path).ok()?;
        if disk_content == buf.content() {
            return None;
        }

        let (added_lines, removed_lines, preview) =
            diff_preview(&disk_content, buf.content(), 10);
        let (_, _, full_diff) = diff_preview(&disk_content, buf.content(), usize::MAX);
        Some(ActiveChangePreview {
            file_label: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            added_lines,
            removed_lines,
            preview,
            full_diff,
        })
    }

    pub fn revert_active_from_disk(&mut self) {
        let Some(active_id) = self.active_tab.clone() else {
            self.status_message = "No active editor to revert".into();
            return;
        };
        let Some(path) = self.tab_path(&active_id).cloned() else {
            self.status_message = "Active buffer has no file path".into();
            return;
        };
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                if let Some(buf) = self.buffers.get_mut(&active_id) {
                    buf.load_text(&content);
                }
                let filename = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                self.status_message = format!("Reverted {} from disk", path.display());
                self.toasts.push(crate::editor::toast::Toast::warn(format!(
                    "Reverted {filename}"
                )));
            }
            Err(e) => {
                self.status_message = format!("Failed to revert {}: {}", path.display(), e);
                self.toasts.push(crate::editor::toast::Toast::error(format!(
                    "Revert failed: {e}"
                )));
            }
        }
    }

    pub fn stage_active_file(&mut self) {
        let Some(active_id) = self.active_tab.clone() else {
            self.status_message = "No active editor to stage".into();
            return;
        };
        let Some(path) = self.tab_path(&active_id).cloned() else {
            self.status_message = "Active buffer has no file path".into();
            return;
        };

        let filename = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if !self.save_buffer_to_with_feedback(&active_id, &path, false) {
            self.status_message = format!("Failed to save {} before staging", path.display());
            self.toasts.push(crate::editor::toast::Toast::error(format!(
                "Save failed before staging {filename}"
            )));
            return;
        }

        let relative = path
            .strip_prefix(&self.workspace_root)
            .unwrap_or(&path)
            .to_path_buf();
        match std::process::Command::new("git")
            .current_dir(&self.workspace_root)
            .arg("add")
            .arg(&relative)
            .output()
        {
            Ok(output) if output.status.success() => {
                self.status_message = format!("Saved and staged {}", relative.display());
                self.toasts
                    .push(crate::editor::toast::Toast::success(format!(
                        "Saved and staged {filename}"
                    )));
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                self.status_message = format!("Saved but failed to stage {}", relative.display());
                self.toasts.push(crate::editor::toast::Toast::error(format!(
                    "git add failed after save: {}",
                    stderr.trim()
                )));
            }
            Err(e) => {
                self.status_message = format!("Saved but failed to run git add: {e}");
                self.toasts.push(crate::editor::toast::Toast::error(format!(
                    "git add error after save: {e}"
                )));
            }
        }
    }

    pub fn dirty_buffer_count(&self) -> usize {
        self.tabs
            .iter()
            .filter_map(|tab| {
                let path = tab.editor_path()?;
                let buffer = self.buffers.get(&tab.id)?;
                let disk_content = std::fs::read_to_string(path).ok()?;
                (disk_content != buffer.content()).then_some(())
            })
            .count()
    }

    pub fn save_all(&mut self) {
        let mut saved = 0usize;
        let ids: Vec<TabId> = self.tabs.iter().map(|t| t.id.clone()).collect();
        for id in ids {
            if let Some(path) = self.tab_path(&id).cloned() {
                if self.save_buffer_to(&id, &path) {
                    saved += 1;
                }
            }
        }
        self.status_message = format!("Saved {} buffers", saved);
    }

    pub fn focus_panel(&mut self, kind: TabKind) {
        let maybe_existing = self
            .tabs
            .iter()
            .find(|t| std::mem::discriminant(&t.kind) == std::mem::discriminant(&kind))
            .cloned();

        let tab_to_focus = if let Some(existing) = maybe_existing {
            existing
        } else {
            let id = TabId::next(&mut self.tab_counter);
            let tab = Tab {
                id: id.clone(),
                kind,
            };
            self.tabs.push(tab.clone());
            if let Some(dock) = self.dock_state.as_mut() {
                dock.push_to_focused_leaf(tab.clone());
            }
            tab
        };

        self.active_tab = Some(tab_to_focus.id.clone());
        if let Some(dock) = self.dock_state.as_mut() {
            if let Some(tab_path) = dock.find_tab(&tab_to_focus) {
                let _ = dock.set_active_tab(tab_path);
            }
        }
    }

    pub fn toggle_panel(&mut self, kind: TabKind) {
        self.focus_panel(kind);
    }

    pub fn rebuild_dock(&mut self) {
        self.dock_state = Some(DockState::new(self.tabs.clone()));
    }

    pub fn build_active(&mut self) {
        self.command_output.clear();
        self.status_message = "Running local build...".into();
        self.agent_active = true;
        let _ = self.agent_tx.send(UiToAgentMessage::RunLocalBuild);
    }

    pub fn run_active(&mut self) {
        self.command_output.clear();
        self.status_message = "Running local execute...".into();
        self.agent_active = true;
        let _ = self.agent_tx.send(UiToAgentMessage::RunLocalRun);
    }

    pub fn toggle_chat(&mut self) {
        self.toggle_panel(TabKind::Chat);
    }

    pub fn toggle_orchestrator(&mut self) {
        self.toggle_panel(TabKind::Orchestrator);
    }

    pub fn toggle_mission_control(&mut self) {
        self.toggle_panel(TabKind::MissionControl);
    }

    pub fn toggle_search(&mut self) {
        self.toggle_panel(TabKind::Search);
    }
}
