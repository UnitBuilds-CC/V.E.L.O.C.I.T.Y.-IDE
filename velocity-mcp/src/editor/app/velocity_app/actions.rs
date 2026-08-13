use std::path::PathBuf;

use super::super::helpers::*;
use super::super::types::*;
use super::struct_def::VelocityApp;
use crate::editor::buffer::EditorBuffer;
use crate::editor::theme::WorkspaceProfile;

/// Loose subsequence match: every character of `needle` must appear in
/// `haystack` in order (not necessarily contiguously). Lets "tsb" find
/// "Toggle Sidebar" the way a modern command palette is expected to.
pub(crate) fn fuzzy_subsequence(haystack: &str, needle: &str) -> bool {
    let mut chars = haystack.chars();
    needle.chars().all(|nc| chars.any(|hc| hc == nc))
}

/// Return the char positions in `haystack` that match `needle` as a
/// case-insensitive subsequence, or `None` if it doesn't match. Positions are
/// char indices into `haystack` (not byte offsets), suitable for highlighting.
pub(crate) fn fuzzy_match_indices(haystack: &str, needle: &str) -> Option<Vec<usize>> {
    if needle.is_empty() {
        return Some(Vec::new());
    }
    let hay: Vec<char> = haystack.chars().collect();
    let mut needle_chars = needle.chars().map(|c| c.to_ascii_lowercase()).peekable();
    let mut matches = Vec::new();
    let mut want = needle_chars.next();
    for (idx, hc) in hay.iter().enumerate() {
        if let Some(w) = want {
            if hc.to_ascii_lowercase() == w {
                matches.push(idx);
                want = needle_chars.next();
            }
        } else {
            break;
        }
    }
    if want.is_none() {
        Some(matches)
    } else {
        None
    }
}

impl VelocityApp {
    pub fn commands(&self) -> Vec<Command> {
        vec![
            // File
            Command {
                label: "New File",
                category: "File",
                shortcut: Some("Ctrl+N"),
                action: |a| a.open_editor(None),
                modes: &[],
            },
            Command {
                label: "Open File\u{2026}",
                category: "File",
                shortcut: Some("Ctrl+O"),
                action: |a| a.open_file_dialog(),
                modes: &[],
            },
            Command {
                label: "Go to File\u{2026}",
                category: "File",
                shortcut: Some("Ctrl+P"),
                action: |a| a.open_quick_open(),
                modes: &[],
            },
            Command {
                label: "Next Tab",
                category: "File",
                shortcut: Some("Ctrl+PageDown"),
                action: |a| a.cycle_tabs(1),
                modes: &[],
            },
            Command {
                label: "Previous Tab",
                category: "File",
                shortcut: Some("Ctrl+PageUp"),
                action: |a| a.cycle_tabs(-1),
                modes: &[],
            },
            Command {
                label: "Close Other Tabs",
                category: "File",
                shortcut: None,
                action: |a| a.close_other_tabs(),
                modes: &[],
            },
            Command {
                label: "Reopen Closed Tab",
                category: "File",
                shortcut: Some("Ctrl+Shift+T"),
                action: |a| a.reopen_closed_tab(),
                modes: &[],
            },
            Command {
                label: "Go to Line\u{2026}",
                category: "File",
                shortcut: Some("Ctrl+G"),
                action: |a| a.open_goto_line(),
                modes: &[],
            },
            Command {
                label: "Go to Symbol\u{2026}",
                category: "File",
                shortcut: Some("Ctrl+Shift+O"),
                action: |a| a.open_goto_symbol(),
                modes: &[],
            },
            Command {
                label: "Go Back",
                category: "File",
                shortcut: Some("Alt+Left"),
                action: |a| a.nav_back(),
                modes: &[],
            },
            Command {
                label: "Go Forward",
                category: "File",
                shortcut: Some("Alt+Right"),
                action: |a| a.nav_forward(),
                modes: &[],
            },
            Command {
                label: "Go to Definition",
                category: "File",
                shortcut: Some("F12"),
                action: |a| a.goto_definition_at_cursor(),
                modes: &[],
            },
            Command {
                label: "Find All References",
                category: "File",
                shortcut: Some("Shift+F12"),
                action: |a| a.find_references_at_cursor(),
                modes: &[],
            },
            Command {
                label: "Show Hover Info",
                category: "File",
                shortcut: None,
                action: |a| a.show_hover_at_cursor(),
                modes: &[],
            },
            Command {
                label: "Save",
                category: "File",
                shortcut: Some("Ctrl+S"),
                action: |a| a.save_active(),
                modes: &[],
            },
            Command {
                label: "Save As\u{2026}",
                category: "File",
                shortcut: None,
                action: |a| a.save_active_as(),
                modes: &[],
            },
            Command {
                label: "Save All",
                category: "File",
                shortcut: Some("Ctrl+Shift+S"),
                action: |a| a.save_all(),
                modes: &[],
            },
            Command {
                label: "Close Tab",
                category: "File",
                shortcut: Some("Ctrl+W"),
                action: |a| a.close_active_tab(),
                modes: &[],
            },
            // Build
            Command {
                label: "Build",
                category: "Build",
                shortcut: Some("Ctrl+B"),
                action: |a| a.build_active(),
                modes: &[WorkspaceProfile::Coder],
            },
            Command {
                label: "Run",
                category: "Build",
                shortcut: Some("Ctrl+R"),
                action: |a| a.run_active(),
                modes: &[WorkspaceProfile::Coder],
            },
            // Automation
            Command {
                label: "Run Selected Flow",
                category: "Automation",
                shortcut: Some("Ctrl+Enter"),
                action: |a| a.run_active(),
                modes: &[WorkspaceProfile::AutomationOperator],
            },
            // Panels
            Command {
                label: "Chat",
                category: "Panels",
                shortcut: Some("Ctrl+J"),
                action: |a| a.toggle_panel(TabKind::Chat),
                modes: &[],
            },
            Command {
                label: "Output",
                category: "Panels",
                shortcut: Some("Ctrl+`"),
                action: |a| a.toggle_panel(TabKind::Output),
                modes: &[],
            },
            Command {
                label: "Orchestrator",
                category: "Panels",
                shortcut: Some("Ctrl+Shift+Y"),
                action: |a| a.focus_orchestrator_tab(),
                modes: &[WorkspaceProfile::AutomationOperator],
            },
            Command {
                label: "Mission Control",
                category: "Panels",
                shortcut: None,
                action: |a| a.toggle_panel(TabKind::MissionControl),
                modes: &[WorkspaceProfile::MissionControl],
            },
            Command {
                label: "Search",
                category: "Panels",
                shortcut: Some("Ctrl+Shift+F"),
                action: |a| a.toggle_search(),
                modes: &[],
            },
            Command {
                label: "Usage",
                category: "Panels",
                shortcut: None,
                action: |a| a.toggle_panel(TabKind::Usage),
                modes: &[],
            },
            Command {
                label: "Graph",
                category: "Panels",
                shortcut: None,
                action: |a| a.toggle_panel(TabKind::Graph),
                modes: &[],
            },
            Command {
                label: "Wiki",
                category: "Panels",
                shortcut: None,
                action: |a| a.toggle_panel(TabKind::Wiki),
                modes: &[],
            },
            Command {
                label: "Settings",
                category: "Panels",
                shortcut: Some("Ctrl+,"),
                action: |a| a.toggle_settings(),
                modes: &[],
            },
            Command {
                label: "Extensions",
                category: "Panels",
                shortcut: None,
                action: |a| a.toggle_extensions(),
                modes: &[],
            },
            Command {
                label: "Live Activity",
                category: "Panels",
                shortcut: None,
                action: |a| a.toggle_activity(),
                modes: &[WorkspaceProfile::MissionControl],
            },
            Command {
                label: "Test Coverage",
                category: "Panels",
                shortcut: None,
                action: |a| a.toggle_coverage(),
                modes: &[WorkspaceProfile::Coder],
            },
            Command {
                label: "Deploy Pipeline",
                category: "Build",
                shortcut: None,
                action: |a| a.toggle_pipeline(),
                modes: &[WorkspaceProfile::Coder],
            },
            Command {
                label: "Voice Commands",
                category: "Panels",
                shortcut: None,
                action: |a| a.toggle_voice(),
                modes: &[],
            },
            Command {
                label: "Knowledge",
                category: "Panels",
                shortcut: None,
                action: |a| a.toggle_knowledge(),
                modes: &[],
            },
            Command {
                label: "Triggers",
                category: "Panels",
                shortcut: None,
                action: |a| a.toggle_triggers(),
                modes: &[],
            },
            Command {
                label: "Workflows",
                category: "Panels",
                shortcut: None,
                action: |a| a.toggle_workflows(),
                modes: &[],
            },
            Command {
                label: "Governance",
                category: "Panels",
                shortcut: None,
                action: |a| a.toggle_governance(),
                modes: &[],
            },
            Command {
                label: "Find / Replace",
                category: "Edit",
                shortcut: Some("Ctrl+H"),
                action: |a| a.open_find_replace_active(),
                modes: &[],
            },
            Command {
                label: "Find",
                category: "Edit",
                shortcut: Some("Ctrl+F"),
                action: |a| a.open_find_active(),
                modes: &[],
            },
            Command {
                label: "Request Inline Suggestion",
                category: "Agent",
                shortcut: Some("Ctrl+Shift+I"),
                action: |a| a.request_inline_suggestion(),
                modes: &[],
            },
            Command {
                label: "Rollback Deploy",
                category: "Build",
                shortcut: Some("Ctrl+Alt+R"),
                action: |a| a.rollback_deploy(),
                modes: &[],
            },
            // Agent
            Command {
                label: "Approve All Tools",
                category: "Agent",
                shortcut: None,
                action: |a| a.approve_all_pending_tools(),
                modes: &[],
            },
            Command {
                label: "Decline All Tools",
                category: "Agent",
                shortcut: None,
                action: |a| a.reject_all_pending_tools(),
                modes: &[],
            },
            Command {
                label: "Plan Sub-Agents",
                category: "Agent",
                shortcut: None,
                action: |a| a.plan_routed_subagents(),
                modes: &[],
            },
            Command {
                label: "Refresh Models",
                category: "Agent",
                shortcut: None,
                action: |a| a.refresh_models(),
                modes: &[],
            },
            // Workspace modes
            Command {
                label: "Mode: Coder",
                category: "Workspace",
                shortcut: Some("Ctrl+1"),
                action: |a| a.set_work_mode(WorkspaceProfile::Coder),
                modes: &[],
            },
            Command {
                label: "Mode: Automation Operator",
                category: "Workspace",
                shortcut: Some("Ctrl+2"),
                action: |a| a.set_work_mode(WorkspaceProfile::AutomationOperator),
                modes: &[],
            },
            Command {
                label: "Mode: Mission Control",
                category: "Workspace",
                shortcut: Some("Ctrl+3"),
                action: |a| a.set_work_mode(WorkspaceProfile::MissionControl),
                modes: &[],
            },
            Command {
                label: "Mode: Accessibility",
                category: "Workspace",
                shortcut: Some("Ctrl+4"),
                action: |a| a.set_work_mode(WorkspaceProfile::Accessibility),
                modes: &[],
            },
            Command {
                label: "Mode: Reset Layout to Default",
                category: "Workspace",
                shortcut: None,
                action: |a| a.reset_current_mode_layout(),
                modes: &[],
            },
            Command {
                label: "Wiki: Export to Markdown",
                category: "Workspace",
                shortcut: None,
                action: |a| a.export_wiki_markdown(),
                modes: &[],
            },
            Command {
                label: "NDA: New Document",
                category: "Workspace",
                shortcut: None,
                action: |a| a.new_nda_document(),
                modes: &[],
            },
            Command {
                label: "NDA: Import Active File",
                category: "Workspace",
                shortcut: None,
                action: |a| a.import_file_to_nda(),
                modes: &[],
            },
            Command {
                label: "NDA: Open Browser Viewer",
                category: "Workspace",
                shortcut: None,
                action: |a| a.open_nda_viewer(),
                modes: &[],
            },
            // View
            Command {
                label: "Toggle Sidebar",
                category: "View",
                shortcut: Some("Ctrl+E"),
                action: |a| a.toggle_left_sidebar(),
                modes: &[],
            },
            Command {
                label: "Toggle History",
                category: "View",
                shortcut: None,
                action: |a| a.toggle_right_sidebar(),
                modes: &[],
            },
            Command {
                label: "Reset Layout",
                category: "View",
                shortcut: None,
                action: |a| a.reset_workspace_layout(),
                modes: &[],
            },
        ]
    }

    pub fn command_list_filtered(&self) -> Vec<Command> {
        let query = self.command_palette.query.to_lowercase();
        let current_mode = self.appearance.profile;
        let mode_cfg = crate::editor::mode_config::mode_config_for(current_mode);
        let priority_cats = mode_cfg.priority_categories();
        let hidden_cats = mode_cfg.hidden_categories();

        let mut commands: Vec<Command> = self
            .commands()
            .into_iter()
            // Filter by mode availability
            .filter(|c| c.modes.is_empty() || c.modes.contains(&current_mode))
            // Hide categories that are irrelevant to this mode
            .filter(|c| !hidden_cats.contains(&c.category))
            // Fuzzy filter by query
            .filter(|c| query.is_empty() || fuzzy_subsequence(&c.label.to_lowercase(), &query))
            .collect();

        // Sort: priority categories first, then alphabetical
        commands.sort_by(|a, b| {
            let a_priority = priority_cats.contains(&a.category);
            let b_priority = priority_cats.contains(&b.category);
            match (a_priority, b_priority) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.label.cmp(b.label),
            }
        });

        commands
    }

    pub fn open_command_palette(&mut self) {
        self.command_palette.open = true;
        self.command_palette.query.clear();
        self.command_palette.selected = 0;
        self.command_palette.just_opened = true;
    }

    pub fn close_active_tab(&mut self) {
        let id = self
            .active_tab
            .clone()
            .or_else(|| self.tabs.first().map(|t| t.id.clone()));
        if let Some(id) = id {
            if self.tab_is_dirty(&id) {
                // Defer to the confirm-on-close dialog instead of discarding edits.
                self.pending_close_tab = Some(id);
            } else {
                self.close_tab(&id);
                self.rebuild_dock();
            }
        }
    }

    /// True when an editor tab has unsaved in-memory edits.
    pub fn tab_is_dirty(&self, id: &TabId) -> bool {
        self.buffers.get(id).map(|b| b.is_dirty()).unwrap_or(false)
    }

    /// Modification time of a file on disk, if available.
    pub(crate) fn file_mtime(path: &std::path::Path) -> Option<std::time::SystemTime> {
        std::fs::metadata(path).and_then(|m| m.modified()).ok()
    }

    /// Detect files changed on disk by another process. Clean buffers are
    /// reloaded silently; dirty buffers keep their edits but warn once.
    /// Throttled by the caller to avoid per-frame `stat` syscalls.
    pub fn check_external_file_changes(&mut self) {
        let ids: Vec<TabId> = self.buffers.keys().cloned().collect();
        for id in ids {
            let Some(path) = self.buffers.get(&id).and_then(|b| b.path.clone()) else {
                continue;
            };
            let Some(disk_mtime) = Self::file_mtime(&path) else {
                continue;
            };
            let known = match self.buffers.get(&id) {
                Some(b) => b.disk_mtime,
                None => continue,
            };
            // Only react when we have a baseline and the file is strictly newer.
            let changed = match known {
                Some(prev) => disk_mtime > prev,
                None => false,
            };
            if !changed {
                continue;
            }
            let dirty = self.tab_is_dirty(&id);
            let filename = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            if dirty {
                // Preserve unsaved edits; warn once and adopt the new baseline
                // so we don't repeat the warning for the same external change.
                if let Some(buf) = self.buffers.get_mut(&id) {
                    buf.disk_mtime = Some(disk_mtime);
                }
                self.toasts.push(crate::editor::toast::Toast::warn(format!(
                    "{filename} changed on disk \u{2014} kept your unsaved edits"
                )));
            } else if let Ok(content) = std::fs::read_to_string(&path) {
                if let Some(buf) = self.buffers.get_mut(&id) {
                    buf.load_text(&content);
                    buf.disk_mtime = Some(disk_mtime);
                }
                self.toasts.push(crate::editor::toast::Toast::info(format!(
                    "Reloaded {filename} (changed on disk)"
                )));
            }
        }
    }

    /// Reload a buffer from disk if the given path matches an open editor tab.
    /// Called by the file watcher when it detects external changes.
    pub fn reload_buffer_if_open(&mut self, path: &std::path::Path) {
        // Find the buffer id for this path.
        let buf_id = self.buffers.iter().find_map(|(id, b)| {
            if b.path.as_deref() == Some(path) {
                Some(id.clone())
            } else {
                None
            }
        });
        let Some(id) = buf_id else { return };

        let dirty = self.tab_is_dirty(&id);
        let filename = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        if dirty {
            // Keep unsaved edits but update mtime so we don't re-warn.
            if let Some(buf) = self.buffers.get_mut(&id) {
                buf.disk_mtime = Self::file_mtime(path);
            }
            self.toasts.push(crate::editor::toast::Toast::warn(format!(
                "{filename} changed on disk \u{2014} kept your unsaved edits"
            )));
        } else if let Ok(content) = std::fs::read_to_string(path) {
            if let Some(buf) = self.buffers.get_mut(&id) {
                buf.load_text(&content);
                buf.disk_mtime = Self::file_mtime(path);
            }
            self.toasts.push(crate::editor::toast::Toast::info(format!(
                "Reloaded {filename} (changed on disk)"
            )));
        }
    }

    pub fn close_tab(&mut self, id: &TabId) {
        if let Some(path) = self.tab_path(id).cloned() {
            self.push_closed_editor_path(path);
        }
        self.tabs.retain(|t| t.id != *id);
        self.buffers.remove(id);
        if self.active_tab.as_ref() == Some(id) {
            self.active_tab = self.tabs.first().map(|t| t.id.clone());
        }
    }

    /// Close every tab except the active one (or the first tab if none is active).
    pub fn close_other_tabs(&mut self) {
        let keep = self
            .active_tab
            .clone()
            .or_else(|| self.tabs.first().map(|t| t.id.clone()));
        let Some(keep) = keep else {
            return;
        };
        let removed: Vec<TabId> = self
            .tabs
            .iter()
            .filter(|t| t.id != keep)
            .map(|t| t.id.clone())
            .collect();
        for id in &removed {
            if let Some(path) = self.tab_path(id).cloned() {
                self.push_closed_editor_path(path);
            }
            self.buffers.remove(id);
        }
        self.tabs.retain(|t| t.id == keep);
        self.active_tab = Some(keep);
        self.rebuild_dock();
        self.status_message = "Closed other tabs".into();
    }

    /// Remember a closed editor file so it can be reopened with Ctrl+Shift+T.
    fn push_closed_editor_path(&mut self, path: PathBuf) {
        self.closed_editor_paths.retain(|p| p != &path);
        self.closed_editor_paths.push(path);
        if self.closed_editor_paths.len() > 20 {
            let excess = self.closed_editor_paths.len() - 20;
            self.closed_editor_paths.drain(0..excess);
        }
    }

    /// Reopen the most recently closed editor file (Ctrl+Shift+T).
    pub fn reopen_closed_tab(&mut self) {
        if let Some(path) = self.closed_editor_paths.pop() {
            self.open_editor(Some(path));
        } else {
            self.status_message = "No recently closed tabs".into();
        }
    }

    /// Open the Ctrl+G go-to-line dialog for the active editor.
    pub fn open_goto_line(&mut self) {
        self.goto_line_open = true;
        self.goto_line_input.clear();
        self.goto_line_just_opened = true;
    }

    /// Open the Ctrl+Shift+O go-to-symbol switcher, gathering sitemap symbols.
    pub fn open_goto_symbol(&mut self) {
        self.goto_symbol_open = true;
        self.goto_symbol_query.clear();
        self.goto_symbol_selected = 0;
        self.goto_symbol_just_opened = true;
        self.workspace_symbols =
            crate::editor::search::collect_workspace_symbols(&self.workspace_root);
        self.goto_symbol_entries = self.workspace_symbols.clone();
    }

    /// Open the file defining `entry` and jump to the symbol's definition line.
    pub fn jump_to_symbol(&mut self, entry: &crate::editor::search::SymbolEntry) {
        self.push_nav_location();
        let abs = self.workspace_root.join(&entry.file);
        let line = std::fs::read_to_string(&abs)
            .ok()
            .and_then(|content| crate::editor::search::find_definition_line(&content, &entry.name));
        self.open_editor(Some(abs));
        if let Some(line) = line {
            self.pending_cursor_line = Some(line);
        }
        self.goto_symbol_open = false;
        self.status_message = format!("{} \u{2192} {}", entry.name, entry.file);
    }

    /// Resolve a symbol name against the cached workspace index and jump to its
    /// definition. Refreshes the (sitemap-backed) cache lazily on first use.
    pub fn jump_to_symbol_name(&mut self, name: &str) {
        if self.workspace_symbols.is_empty() {
            self.workspace_symbols =
                crate::editor::search::collect_workspace_symbols(&self.workspace_root);
        }
        if let Some(entry) = self
            .workspace_symbols
            .iter()
            .find(|e| e.name == name)
            .cloned()
        {
            self.jump_to_symbol(&entry);
        } else {
            self.status_message = format!("No definition found for \u{201c}{}\u{201d}", name);
        }
    }

    /// Snapshot the active editor's file/line onto the back stack. Called before
    /// a jump so it can be unwound with Alt+←. Clears the forward stack.
    pub fn push_nav_location(&mut self) {
        let Some(id) = self.active_tab.clone() else {
            return;
        };
        let Some(path) = self.tab_path(&id).cloned() else {
            return;
        };
        let line = self.pending_cursor_line;
        self.nav_back.push(NavLocation { path, line });
        if self.nav_back.len() > 100 {
            self.nav_back.remove(0);
        }
        self.nav_forward.clear();
    }

    /// Restore a saved location without recording a new history entry.
    fn restore_nav_location(&mut self, loc: NavLocation) {
        self.open_editor(Some(loc.path));
        if let Some(line) = loc.line {
            self.pending_cursor_line = Some(line);
        }
    }

    /// Navigate to the previous location (Alt+←).
    pub fn nav_back(&mut self) {
        let Some(loc) = self.nav_back.pop() else {
            self.status_message = "Nothing to go back to".into();
            return;
        };
        if let Some(id) = self.active_tab.clone() {
            if let Some(path) = self.tab_path(&id).cloned() {
                self.nav_forward.push(NavLocation {
                    path,
                    line: self.pending_cursor_line,
                });
            }
        }
        self.restore_nav_location(loc);
    }

    /// Navigate forward again after going back (Alt+→).
    pub fn nav_forward(&mut self) {
        let Some(loc) = self.nav_forward.pop() else {
            self.status_message = "Nothing to go forward to".into();
            return;
        };
        if let Some(id) = self.active_tab.clone() {
            if let Some(path) = self.tab_path(&id).cloned() {
                self.nav_back.push(NavLocation {
                    path,
                    line: self.pending_cursor_line,
                });
            }
        }
        self.restore_nav_location(loc);
    }

    /// Return a cached copy of the workspace site map, refreshing it from disk
    /// at most every `ttl` (and when the index entry count changes). This avoids
    /// re-reading and re-parsing `index.json` on every rendered frame.
    pub fn cached_site_map(
        &mut self,
        ttl: std::time::Duration,
    ) -> Option<std::sync::Arc<velocity_ide::site_map::SiteMap>> {
        let stale = match self.cached_site_map_at {
            Some(at) => at.elapsed() >= ttl,
            None => true,
        };
        if stale {
            if let Ok(sm) = crate::automation::open_workspace_site_map(&self.workspace_root) {
                self.cached_site_map = Some(std::sync::Arc::new(sm));
                self.cached_site_map_at = Some(std::time::Instant::now());
            }
        }
        self.cached_site_map.clone()
    }

    /// Activate the dock tab `direction` steps from the active one (wrapping).
    pub fn cycle_tabs(&mut self, direction: i32) {
        let Some(dock) = self.dock_state.as_mut() else {
            return;
        };
        let ordered: Vec<Tab> = dock.iter_all_tabs().map(|(_, tab)| tab.clone()).collect();
        if ordered.len() < 2 {
            return;
        }
        let current = self
            .active_tab
            .as_ref()
            .and_then(|id| ordered.iter().position(|t| &t.id == id))
            .unwrap_or(0);
        let len = ordered.len();
        let next = if direction > 0 {
            (current + 1) % len
        } else {
            current.checked_sub(1).unwrap_or(len - 1)
        };
        let target_id = ordered[next].id.clone();
        self.activate_tab_by_id(&target_id);
    }

    /// Focus the dock tab matching `id` and mark it active.
    pub fn activate_tab_by_id(&mut self, id: &TabId) {
        let Some(dock) = self.dock_state.as_mut() else {
            return;
        };
        let found = dock
            .iter_all_tabs()
            .find(|(_, tab)| &tab.id == id)
            .map(|(_, tab)| tab.clone());
        if let Some(tab) = found {
            if let Some(tab_path) = dock.find_tab(&tab) {
                let _ = dock.set_active_tab(tab_path);
                let id = tab.id.clone();
                self.active_tab = Some(id.clone());
                self.touch_mru(&id);
            }
        }
    }

    /// Record `id` as the most-recently-used tab for the Ctrl+Tab switcher.
    pub fn touch_mru(&mut self, id: &TabId) {
        self.mru.order.retain(|t| t != id);
        self.mru.order.insert(0, id.clone());
    }

    /// Open the Ctrl+P quick-open switcher, gathering the workspace file list.
    pub fn open_quick_open(&mut self) {
        self.quick_open.open = true;
        self.quick_open.query.clear();
        self.quick_open.selected = 0;
        self.quick_open.just_opened = true;
        self.quick_open.files =
            crate::editor::search::list_workspace_files(&self.workspace_root, 5000);
    }

    pub fn open_editor(&mut self, path: Option<PathBuf>) {
        // Route portable/sealed NDA documents to the dedicated NDA editor tab
        // (but never the `.velocity/` / `memory/` at-rest state envelopes).
        if let Some(ref p) = path {
            if p.extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("nda"))
                .unwrap_or(false)
                && !Self::is_internal_nda_path(p)
            {
                self.open_nda_document(Some(p.clone()));
                return;
            }
        }
        if let Some(ref p) = path {
            let existing = self.tabs.iter().find_map(|tab| match &tab.kind {
                TabKind::Editor {
                    path: Some(tab_path),
                    ..
                } if tab_path == p => Some(tab.id.clone()),
                _ => None,
            });
            if let Some(id) = existing {
                self.active_tab = Some(id.clone());
                self.touch_mru(&id);
                return;
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
                buf.disk_mtime = Self::file_mtime(p);
            } else {
                self.status_message = format!("Failed to read file: {:?}", p);
            }
        }
        self.buffers.insert(id.clone(), buf);
        self.tabs.push(tab.clone());
        if let Some(dock) = self.dock_state.as_mut() {
            dock.push_to_focused_leaf(tab);
        }
        self.active_tab = Some(id.clone());
        self.touch_mru(&id);

        // Announce the file to the LSP server so it starts providing diagnostics.
        if let Some(ref p) = path {
            if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                if let Some(content) = self.buffers.get(&id).map(|b| b.content().to_string()) {
                    if let Some(lsp) = self.lsp_manager.as_mut() {
                        lsp.sync_document(ext, p, &content);
                    }
                }
            }
        }
    }

    /// Split the current editor view: open the same file in a new tab side-by-side.
    /// This allows viewing different parts of the same file simultaneously.
    pub fn split_editor(&mut self) {
        // Get the path of the currently active editor tab.
        let active_path = self.active_tab.as_ref().and_then(|id| {
            self.tabs
                .iter()
                .find(|t| &t.id == id)
                .and_then(|t| t.editor_path().cloned())
        });

        let Some(path) = active_path else {
            self.status_message = "No active editor to split".to_string();
            return;
        };

        // Create a new editor tab for the same file (bypass deduplication).
        let id = TabId::next(&mut self.tab_counter);
        let tab = Tab {
            id: id.clone(),
            kind: TabKind::Editor {
                path: Some(path.clone()),
                buffer_id: id.clone(),
            },
        };

        // Share the same buffer content by copying it.
        let mut buf = EditorBuffer::default();
        if let Ok(content) = std::fs::read_to_string(&path) {
            buf.load_text(&content);
            buf.disk_mtime = Self::file_mtime(&path);
        }
        self.buffers.insert(id.clone(), buf);
        self.tabs.push(tab.clone());

        // Push to dock state to create a split view.
        if let Some(dock) = self.dock_state.as_mut() {
            dock.push_to_focused_leaf(tab);
        }
        self.active_tab = Some(id.clone());
        self.touch_mru(&id);
        self.status_message = "Split editor view".to_string();
    }

    pub fn prompt_open_file(&mut self) {
        // Trigger the native file dialog to let the user pick a file to open.
        // Falls back to opening an empty editor if no dialog system is available.
        self.open_file_dialog();
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
                    if let Some(buf) = self.buffers.get_mut(id) {
                        buf.mark_saved();
                        buf.disk_mtime = Self::file_mtime(path);
                    }
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
                    // Refresh git status after save
                    self.refresh_git_status();
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

        let (added_lines, removed_lines, preview) = diff_preview(&disk_content, buf.content(), 10);
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
                    buf.disk_mtime = Self::file_mtime(&path);
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
}
