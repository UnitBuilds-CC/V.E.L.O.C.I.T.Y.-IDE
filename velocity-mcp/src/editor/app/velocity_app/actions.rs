use std::path::PathBuf;

use crate::agent::UiToAgentMessage;
use crate::editor::buffer::EditorBuffer;
use crate::editor::theme::WorkspaceProfile;
use super::super::types::*;
use super::super::helpers::*;
use super::struct_def::VelocityApp;

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
            Command { label: "New File", category: "File", shortcut: Some("Ctrl+N"), action: |a| a.open_editor(None), modes: &[] },
            Command { label: "Open File…", category: "File", shortcut: Some("Ctrl+O"), action: |a| a.open_file_dialog(), modes: &[] },
            Command { label: "Go to File…", category: "File", shortcut: Some("Ctrl+P"), action: |a| a.open_quick_open(), modes: &[] },
            Command { label: "Next Tab", category: "File", shortcut: Some("Ctrl+PageDown"), action: |a| a.cycle_tabs(1), modes: &[] },
            Command { label: "Previous Tab", category: "File", shortcut: Some("Ctrl+PageUp"), action: |a| a.cycle_tabs(-1), modes: &[] },
            Command { label: "Close Other Tabs", category: "File", shortcut: None, action: |a| a.close_other_tabs(), modes: &[] },
            Command { label: "Reopen Closed Tab", category: "File", shortcut: Some("Ctrl+Shift+T"), action: |a| a.reopen_closed_tab(), modes: &[] },
            Command { label: "Go to Line…", category: "File", shortcut: Some("Ctrl+G"), action: |a| a.open_goto_line(), modes: &[] },
            Command { label: "Go to Symbol…", category: "File", shortcut: Some("Ctrl+Shift+O"), action: |a| a.open_goto_symbol(), modes: &[] },
            Command { label: "Go Back", category: "File", shortcut: Some("Alt+Left"), action: |a| a.nav_back(), modes: &[] },
            Command { label: "Go Forward", category: "File", shortcut: Some("Alt+Right"), action: |a| a.nav_forward(), modes: &[] },
            Command { label: "Save", category: "File", shortcut: Some("Ctrl+S"), action: |a| a.save_active(), modes: &[] },
            Command { label: "Save As…", category: "File", shortcut: None, action: |a| a.save_active_as(), modes: &[] },
            Command { label: "Save All", category: "File", shortcut: Some("Ctrl+Shift+S"), action: |a| a.save_all(), modes: &[] },
            Command { label: "Close Tab", category: "File", shortcut: Some("Ctrl+W"), action: |a| a.close_active_tab(), modes: &[] },
            // Build
            Command { label: "Build", category: "Build", shortcut: Some("Ctrl+B"), action: |a| a.build_active(), modes: &[WorkspaceProfile::Coder] },
            Command { label: "Run", category: "Build", shortcut: Some("Ctrl+R"), action: |a| a.run_active(), modes: &[WorkspaceProfile::Coder] },
            // Automation
            Command { label: "Run Selected Flow", category: "Automation", shortcut: Some("Ctrl+Enter"), action: |a| a.run_active(), modes: &[WorkspaceProfile::AutomationOperator] },
            // Panels
            Command { label: "Chat", category: "Panels", shortcut: Some("Ctrl+J"), action: |a| a.toggle_panel(TabKind::Chat), modes: &[] },
            Command { label: "Output", category: "Panels", shortcut: Some("Ctrl+`"), action: |a| a.toggle_panel(TabKind::Output), modes: &[] },
            Command { label: "Orchestrator", category: "Panels", shortcut: Some("Ctrl+Shift+Y"), action: |a| a.toggle_orchestrator(), modes: &[WorkspaceProfile::AutomationOperator] },
            Command { label: "Mission Control", category: "Panels", shortcut: None, action: |a| a.toggle_panel(TabKind::MissionControl), modes: &[WorkspaceProfile::MissionControl] },
            Command { label: "Search", category: "Panels", shortcut: Some("Ctrl+Shift+F"), action: |a| a.toggle_search(), modes: &[] },
            Command { label: "Usage", category: "Panels", shortcut: None, action: |a| a.toggle_panel(TabKind::Usage), modes: &[] },
            Command { label: "Graph", category: "Panels", shortcut: None, action: |a| a.toggle_panel(TabKind::Graph), modes: &[] },
            Command { label: "Wiki", category: "Panels", shortcut: None, action: |a| a.toggle_panel(TabKind::Wiki), modes: &[] },
            Command { label: "Settings", category: "Panels", shortcut: Some("Ctrl+,"), action: |a| a.toggle_settings(), modes: &[] },
            Command { label: "Extensions", category: "Panels", shortcut: None, action: |a| a.toggle_extensions(), modes: &[] },
            Command { label: "Live Activity", category: "Panels", shortcut: None, action: |a| a.toggle_activity(), modes: &[WorkspaceProfile::MissionControl] },
            Command { label: "Test Coverage", category: "Panels", shortcut: None, action: |a| a.toggle_coverage(), modes: &[WorkspaceProfile::Coder] },
            Command { label: "Deploy Pipeline", category: "Build", shortcut: None, action: |a| a.toggle_pipeline(), modes: &[WorkspaceProfile::Coder] },
            Command { label: "Voice Commands", category: "Panels", shortcut: None, action: |a| a.toggle_voice(), modes: &[] },
            Command { label: "Knowledge", category: "Panels", shortcut: None, action: |a| a.toggle_knowledge(), modes: &[] },
            Command { label: "Triggers", category: "Panels", shortcut: None, action: |a| a.toggle_triggers(), modes: &[] },
            Command { label: "Workflows", category: "Panels", shortcut: None, action: |a| a.toggle_workflows(), modes: &[] },
            Command { label: "Governance", category: "Panels", shortcut: None, action: |a| a.toggle_governance(), modes: &[] },
            Command { label: "Find / Replace", category: "Edit", shortcut: Some("Ctrl+H"), action: |a| a.open_find_replace_active(), modes: &[] },
            Command { label: "Find", category: "Edit", shortcut: Some("Ctrl+F"), action: |a| a.open_find_active(), modes: &[] },
            Command { label: "Request Inline Suggestion", category: "Agent", shortcut: Some("Ctrl+Shift+I"), action: |a| a.request_inline_suggestion(), modes: &[] },
            Command { label: "Rollback Deploy", category: "Build", shortcut: Some("Ctrl+Alt+R"), action: |a| a.rollback_deploy(), modes: &[] },
            // Agent
            Command { label: "Approve All Tools", category: "Agent", shortcut: None, action: |a| a.approve_all_pending_tools(), modes: &[] },
            Command { label: "Decline All Tools", category: "Agent", shortcut: None, action: |a| a.reject_all_pending_tools(), modes: &[] },
            Command { label: "Plan Sub-Agents", category: "Agent", shortcut: None, action: |a| a.plan_routed_subagents(), modes: &[] },
            Command { label: "Refresh Models", category: "Agent", shortcut: None, action: |a| a.refresh_models(), modes: &[] },
            // Workspace modes
            Command { label: "Mode: Coder", category: "Workspace", shortcut: Some("Ctrl+1"), action: |a| a.set_work_mode(WorkspaceProfile::Coder), modes: &[] },
            Command { label: "Mode: Automation Operator", category: "Workspace", shortcut: Some("Ctrl+2"), action: |a| a.set_work_mode(WorkspaceProfile::AutomationOperator), modes: &[] },
            Command { label: "Mode: Mission Control", category: "Workspace", shortcut: Some("Ctrl+3"), action: |a| a.set_work_mode(WorkspaceProfile::MissionControl), modes: &[] },
            Command { label: "Mode: Accessibility", category: "Workspace", shortcut: Some("Ctrl+4"), action: |a| a.set_work_mode(WorkspaceProfile::Accessibility), modes: &[] },
            Command { label: "Mode: Reset Layout to Default", category: "Workspace", shortcut: None, action: |a| a.reset_current_mode_layout(), modes: &[] },
            Command { label: "Wiki: Export to Markdown", category: "Workspace", shortcut: None, action: |a| a.export_wiki_markdown(), modes: &[] },
            // View
            Command { label: "Toggle Sidebar", category: "View", shortcut: Some("Ctrl+E"), action: |a| a.toggle_left_sidebar(), modes: &[] },
            Command { label: "Toggle History", category: "View", shortcut: None, action: |a| a.toggle_right_sidebar(), modes: &[] },
            Command { label: "Reset Layout", category: "View", shortcut: None, action: |a| a.reset_workspace_layout(), modes: &[] },
        ]
    }

    pub fn command_list_filtered(&self) -> Vec<Command> {
        let query = self.command_palette.query.to_lowercase();
        let current_mode = self.appearance.profile;
        let mode_cfg = crate::editor::mode_config::mode_config_for(current_mode);
        let priority_cats = mode_cfg.priority_categories();
        let hidden_cats = mode_cfg.hidden_categories();

        let mut commands: Vec<Command> = self.commands()
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
                    "{filename} changed on disk — kept your unsaved edits"
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
        self.status_message = format!("{} → {}", entry.name, entry.file);
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
            self.status_message = format!("No definition found for “{}”", name);
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
    pub fn cached_site_map(&mut self, ttl: std::time::Duration) -> Option<std::sync::Arc<velocity_ide::site_map::SiteMap>> {
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
        self.quick_open.files = crate::editor::search::list_workspace_files(&self.workspace_root, 5000);
    }

    pub fn open_editor(&mut self, path: Option<PathBuf>) {
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

    pub fn focus_panel(&mut self, kind: TabKind) {
        if let Some(dock) = self.dock_state.as_mut() {
            let found_tab = dock
                .iter_all_tabs()
                .find(|(_, tab)| std::mem::discriminant(&tab.kind) == std::mem::discriminant(&kind))
                .map(|(_, tab)| tab.clone());

            if let Some(tab) = found_tab {
                if let Some(tab_path) = dock.find_tab(&tab) {
                    let _ = dock.set_active_tab(tab_path);
                    self.active_tab = Some(tab.id);
                    return;
                }
            }

            let id = TabId::next(&mut self.tab_counter);
            let tab = Tab {
                id: id.clone(),
                kind,
            };
            if !self.tabs.iter().any(|t| t.id == id) {
                self.tabs.push(tab.clone());
            }
            dock.push_to_focused_leaf(tab.clone());
            if let Some(tab_path) = dock.find_tab(&tab) {
                let _ = dock.set_active_tab(tab_path);
            }
            self.active_tab = Some(id);
        }
    }

    pub fn toggle_panel(&mut self, kind: TabKind) {
        self.focus_panel(kind);
    }

    pub fn rebuild_dock(&mut self) {
        self.dock_state = Some(self.build_workspace_dock(self.appearance.profile));
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

    pub fn toggle_orchestrator(&mut self) {
        self.toggle_panel(TabKind::Orchestrator);
    }

    pub fn toggle_mission_control(&mut self) {
        self.toggle_panel(TabKind::MissionControl);
    }

    /// Export the sitemap-generated wiki to `.wiki/` as interlinked Markdown.
    pub fn export_wiki_markdown(&mut self) {
        let workspace_root = self.workspace_root.clone();
        self.wiki_view.export(&workspace_root, &mut self.toasts);
    }

    pub fn toggle_search(&mut self) {
        self.toggle_panel(TabKind::Search);
    }

    pub fn toggle_settings(&mut self) {
        self.toggle_panel(TabKind::Settings);
    }

    /// Rescan extensions from disk and open the Extensions manager panel.
    pub fn toggle_extensions(&mut self) {
        let ws = self.workspace_root.clone();
        self.extension_registry.scan(&ws);
        self.toggle_panel(TabKind::Extensions);
    }

    /// Open the live orchestration Activity panel.
    pub fn toggle_activity(&mut self) {
        self.toggle_panel(TabKind::Activity);
    }

    /// Analyze coverage on first open, then show the Coverage panel.
    pub fn toggle_coverage(&mut self) {
        if self.test_generator.analysis.total_functions == 0 {
            self.run_coverage_analysis();
        }
        self.toggle_panel(TabKind::Coverage);
    }

    /// Initialize the deploy pipeline and open the Pipeline panel.
    pub fn toggle_pipeline(&mut self) {
        self.init_deploy_pipeline();
        self.toggle_panel(TabKind::Pipeline);
    }

    /// Open the Voice command panel.
    pub fn toggle_voice(&mut self) {
        self.toggle_panel(TabKind::Voice);
    }

    /// Open the Knowledge / RAG panel.
    pub fn toggle_knowledge(&mut self) {
        self.toggle_panel(TabKind::Knowledge);
    }

    /// Open the unattended-execution Triggers panel.
    pub fn toggle_triggers(&mut self) {
        self.toggle_panel(TabKind::Triggers);
    }

    /// Open the Workflow composer panel.
    pub fn toggle_workflows(&mut self) {
        self.toggle_panel(TabKind::Workflows);
    }

    /// Open the Governance panel (policy, approvals, secrets, connectors).
    pub fn toggle_governance(&mut self) {
        self.toggle_panel(TabKind::Governance);
    }

    pub fn toggle_left_sidebar(&mut self) {
        self.left_sidebar_visible = !self.left_sidebar_visible;
        self.save_workspace_preferences();
    }

    pub fn toggle_right_sidebar(&mut self) {
        self.right_sidebar_visible = !self.right_sidebar_visible;
        self.save_workspace_preferences();
    }

    pub fn reset_workspace_layout(&mut self) {
        let profile = self.appearance.profile;
        self.apply_workspace_profile(profile);
        self.left_sidebar_visible = true;
        self.left_sidebar_width = 240.0;
        self.right_sidebar_visible = true;
        self.right_sidebar_width = 280.0;
        self.save_workspace_preferences();
    }

    // ─── IDE Feature Helpers ───────────────────────────────────────────────

    /// Toggle breakpoint on the current cursor line.
    pub fn toggle_breakpoint_current_line(&mut self) {
        if let Some(id) = &self.active_tab {
            if let Some(buf) = self.buffers.get_mut(id) {
                // Use tracked cursor line (updated during rendering)
                let line = self.current_cursor_line;
                if let Some(pos) = buf.breakpoints.iter().position(|&l| l == line) {
                    buf.breakpoints.remove(pos);
                } else {
                    buf.breakpoints.push(line);
                }
            }
        }
    }

    /// Trigger code completion at cursor position.
    pub fn trigger_completion(&mut self) {
        let active_id = self.active_tab.clone();
        if let Some(id) = active_id {
            if let Some(buf) = self.buffers.get(&id) {
                // Get the word prefix before cursor (simple heuristic: last word chars)
                let content = &buf.content;
                let prefix = content
                    .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or("");

                // Build completion items from sitemap symbols
                let items = crate::editor::completion::CompletionState::compute_items(
                    prefix,
                    &self.workspace_symbols,
                );
                self.completion_state.show(items);
            }
        }
    }

    /// Open the in-file Find overlay on the active editor.
    pub fn open_find_active(&mut self) {
        if let Some(id) = self.active_tab.clone() {
            if let Some(buf) = self.buffers.get_mut(&id) {
                buf.find_replace.open_find();
            }
        }
    }

    /// Open the in-file Find+Replace overlay on the active editor.
    pub fn open_find_replace_active(&mut self) {
        if let Some(id) = self.active_tab.clone() {
            if let Some(buf) = self.buffers.get_mut(&id) {
                buf.find_replace.open_find_replace();
            }
        }
    }

    /// Get git status for the workspace.
    pub fn refresh_git_status(&mut self) {
        self.git_state = crate::editor::git_ui::GitState::from_workspace(&self.workspace_root);
    }

    /// Render the debug panel (call stack, variables, watches, toolbar).
    pub fn render_debug_panel(&mut self, ui: &mut eframe::egui::Ui, palette: crate::editor::theme::IdePalette) {
        use eframe::egui;
        use crate::editor::debugger::DebugState;

        let state = self.dap_client.as_ref().map(|d| d.state).unwrap_or(DebugState::Inactive);

        // Debug toolbar
        ui.horizontal(|ui| {
            let state_label = match state {
                DebugState::Inactive => "Inactive",
                DebugState::Starting => "Starting",
                DebugState::Running => "Running",
                DebugState::Paused => "Paused",
                DebugState::Stopped => "Stopped",
            };
            ui.label(egui::RichText::new(format!("\u{1F41E} {}", state_label)).size(10.0).color(match state {
                DebugState::Running => palette.success,
                DebugState::Paused => palette.warning,
                DebugState::Stopped => palette.error,
                _ => palette.text_muted,
            }));

            ui.add_space(8.0);
            let can_continue = state == DebugState::Paused;
            let can_step = state == DebugState::Paused;
            let can_stop = state == DebugState::Running || state == DebugState::Paused;

            if ui.add_enabled(can_continue, egui::Button::new("\u{25B6} Continue")).clicked() {
                if let Some(dap) = &mut self.dap_client {
                    let _ = dap.continue_execution();
                }
            }
            if ui.add_enabled(can_step, egui::Button::new("\u{23ED} Step Over")).clicked() {
                if let Some(dap) = &mut self.dap_client {
                    let _ = dap.step_over();
                }
            }
            if ui.add_enabled(can_step, egui::Button::new("\u{2B07} Step Into")).clicked() {
                if let Some(dap) = &mut self.dap_client {
                    let _ = dap.step_into();
                }
            }
            if ui.add_enabled(can_step, egui::Button::new("\u{2B06} Step Out")).clicked() {
                if let Some(dap) = &mut self.dap_client {
                    let _ = dap.step_out();
                }
            }
            if ui.add_enabled(can_stop, egui::Button::new("\u{23F9} Stop").fill(palette.error)).clicked() {
                if let Some(dap) = &mut self.dap_client {
                    let _ = dap.stop();
                }
            }
        });
        ui.separator();

        if state == DebugState::Inactive {
            ui.label(egui::RichText::new("No active debug session. Press F5 to start debugging.").size(9.0).color(palette.text_muted));
            return;
        }

        // Split: Call Stack | Variables | Watches
        ui.columns(3, |cols| {
            // Call Stack
            cols[0].label(egui::RichText::new("Call Stack").size(9.0).strong().color(palette.accent));
            if let Some(dap) = &self.dap_client {
                for frame in &dap.stack_frames {
                    let file = frame.file.as_ref()
                        .map(|f| f.file_name().unwrap_or_default().to_string_lossy().to_string())
                        .unwrap_or_default();
                    cols[0].label(egui::RichText::new(format!("  {} ({}:{})", frame.name, file, frame.line))
                        .monospace().size(9.0).color(palette.text));
                }
                if dap.stack_frames.is_empty() {
                    cols[0].label(egui::RichText::new("  (no frames)").size(9.0).color(palette.text_muted));
                }
            }

            // Variables
            cols[1].label(egui::RichText::new("Variables").size(9.0).strong().color(palette.accent));
            if let Some(dap) = &self.dap_client {
                for var in &dap.variables {
                    let type_hint = var.type_name.as_deref().unwrap_or("");
                    cols[1].label(egui::RichText::new(format!("  {} = {} {}", var.name, var.value, type_hint))
                        .monospace().size(9.0).color(palette.text));
                }
                if dap.variables.is_empty() {
                    cols[1].label(egui::RichText::new("  (no variables)").size(9.0).color(palette.text_muted));
                }
            }

            // Watches
            cols[2].label(egui::RichText::new("Watches").size(9.0).strong().color(palette.accent));
            if let Some(dap) = &self.dap_client {
                for watch in &dap.watches {
                    let result = watch.result.as_deref().unwrap_or("<not evaluated>");
                    cols[2].label(egui::RichText::new(format!("  {} = {}", watch.expression, result))
                        .monospace().size(9.0).color(palette.text));
                }
                if dap.watches.is_empty() {
                    cols[2].label(egui::RichText::new("  (no watches)").size(9.0).color(palette.text_muted));
                }
            }
        });
    }

    /// Launch a debug session. Auto-detects the debug adapter based on project type.
    pub fn launch_debug_session(&mut self) {
        use crate::editor::debugger::{DapClient, LaunchConfig};

        // Determine the binary to debug based on workspace type
        let cargo_toml = self.workspace_root.join("Cargo.toml");
        if cargo_toml.exists() {
            // Rust project — look for the target binary
            let target_dir = self.workspace_root.join("target").join("debug");
            let project_name = self.workspace_root
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .replace('-', "_");

            let binary = if cfg!(target_os = "windows") {
                target_dir.join(format!("{}.exe", project_name))
            } else {
                target_dir.join(&project_name)
            };

            if !binary.exists() {
                self.status_message = format!("Debug: binary not found at {}. Run 'cargo build' first.", binary.display());
                self.toasts.push(crate::editor::toast::Toast::error(
                    "Build project before debugging (cargo build)",
                ));
                return;
            }

            let config = LaunchConfig::rust_debug(&binary, &self.workspace_root);
            let mut dap = DapClient::new();
            match dap.launch(&config) {
                Ok(()) => {
                    self.dap_client = Some(dap);
                    self.status_message = "Debug: session started".to_string();
                    self.toasts.push(crate::editor::toast::Toast::success("Debug session launched"));
                    // Open debug tab in bottom panel
                    self.bottom_panel_state.collapsed = false;
                    self.bottom_panel_state.active_tab = 2; // Debug tab
                }
                Err(e) => {
                    self.status_message = format!("Debug: failed to launch — {}", e);
                    self.toasts.push(crate::editor::toast::Toast::error(
                        format!("Debug launch failed: {}", e),
                    ));
                }
            }
        } else {
            self.status_message = "Debug: no supported project found (Cargo.toml)".to_string();
            self.toasts.push(crate::editor::toast::Toast::info(
                "No debuggable project detected. Only Rust (codelldb) is supported currently.",
            ));
        }
    }

    // ─── Semantic Search Integration ─────────────────────────────────────────

    /// Run a semantic (TF-IDF similarity) search and produce SearchHit results.
    pub fn run_semantic_search(&mut self) {
        if self.search_query.is_empty() {
            self.search_hits.clear();
            return;
        }
        // Ensure the index is built.
        if self.semantic_index.is_none() {
            self.semantic_index = Some(
                crate::editor::semantic_search::SemanticIndex::build(&self.workspace_root),
            );
        }
        if let Some(ref index) = self.semantic_index {
            let hits = index.search(&self.search_query, 50);
            self.search_hits = hits
                .into_iter()
                .map(|h| crate::editor::search::SearchHit {
                    path: h.path,
                    line: 1,
                    text: format!("[{:.0}%] {}", h.score * 100.0, h.preview),
                })
                .collect();
        }
    }

    // ─── Inline Suggestions LLM Wiring ───────────────────────────────────────

    /// Request an inline ghost-text suggestion from the configured LLM provider.
    /// Called on cursor pause after debounce timer (see code_editor integration).
    pub fn request_inline_suggestion(&mut self) {
        use crate::editor::inline_suggestions::SuggestionRequest;

        let (file_path, prefix, suffix, language) = match self.active_tab.as_ref()
            .and_then(|id| {
                let path = self.tab_path(id)?.clone();
                let buf = self.buffers.get(id)?;
                let content = buf.content().to_string();
                // Split at roughly the middle or the end (no cursor byte available)
                // Use the last 500 chars as prefix context
                let split = content.len().min(2000);
                let prefix = content[..split].to_string();
                let suffix = content[split..].to_string();
                let ext = path.extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("txt");
                let language = match ext {
                    "rs" => "rust",
                    "py" => "python",
                    "js" | "jsx" => "javascript",
                    "ts" | "tsx" => "typescript",
                    "go" => "go",
                    "java" => "java",
                    _ => "plaintext",
                };
                Some((path, prefix, suffix, language.to_string()))
            }) {
            Some(tuple) => tuple,
            None => return,
        };

        let request = SuggestionRequest {
            file_path,
            prefix,
            suffix,
            language,
        };

        // Submit to the suggestion engine for async resolution.
        self.inline_suggestions.submit_request(
            request,
            self.provider,
            &self.selected_model,
            self.workspace_root.clone(),
        );
    }

    // ─── Deploy Pipeline UI Integration ──────────────────────────────────────

    /// Initialize the deploy pipeline from workspace configuration.
    pub fn init_deploy_pipeline(&mut self) {
        if self.deploy_pipeline.is_none() {
            self.deploy_pipeline = Some(
                crate::editor::deploy_pipeline::PipelineManager::from_workspace(&self.workspace_root),
            );
        }
    }

    /// Trigger a full deploy run (build → test → package → deploy).
    pub fn trigger_deploy(&mut self) {
        self.init_deploy_pipeline();
        if let Some(ref mut pipeline) = self.deploy_pipeline {
            pipeline.trigger_run();
            self.status_message = "Deploy pipeline started.".into();
            self.toasts.push(crate::editor::toast::Toast::info("▲ Deploy pipeline running"));
        }
    }

    /// Rollback to the previous successful deployment.
    pub fn rollback_deploy(&mut self) {
        if let Some(ref mut pipeline) = self.deploy_pipeline {
            match pipeline.rollback() {
                Ok(()) => {
                    self.status_message = "Rolled back to previous deployment.".into();
                    self.toasts.push(crate::editor::toast::Toast::success("Rollback successful"));
                }
                Err(e) => {
                    self.toasts.push(crate::editor::toast::Toast::error(format!("Rollback failed: {}", e)));
                }
            }
        }
    }
}
