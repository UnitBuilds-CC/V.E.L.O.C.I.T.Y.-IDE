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
                label: "Close All Tabs",
                category: "File",
                shortcut: None,
                action: |a| a.close_all_tabs(),
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
                label: "Go to Declaration",
                category: "File",
                shortcut: None,
                action: |a| a.goto_declaration_at_cursor(),
                modes: &[],
            },
            Command {
                label: "Go to Type Definition",
                category: "File",
                shortcut: None,
                action: |a| a.goto_type_definition_at_cursor(),
                modes: &[],
            },
            Command {
                label: "Go to Implementation",
                category: "File",
                shortcut: None,
                action: |a| a.goto_implementation_at_cursor(),
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
                label: "Show Incoming Calls",
                category: "File",
                shortcut: None,
                action: |a| a.show_call_hierarchy(true),
                modes: &[],
            },
            Command {
                label: "Show Outgoing Calls",
                category: "File",
                shortcut: None,
                action: |a| a.show_call_hierarchy(false),
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
                label: "Format Document",
                category: "File",
                shortcut: Some("Shift+Alt+F"),
                action: |a| a.format_document_via_lsp(),
                modes: &[],
            },
            Command {
                label: "Show Signature Help (Parameter Hints)",
                category: "File",
                shortcut: Some("Ctrl+Shift+Space"),
                action: |a| a.request_signature_help(None),
                modes: &[],
            },
            Command {
                label: "Toggle Bookmark on Line",
                category: "File",
                shortcut: Some("Ctrl+Shift+B"),
                action: |a| a.toggle_bookmark_current_line(),
                modes: &[],
            },
            Command {
                label: "Rename Symbol (LSP)",
                category: "File",
                shortcut: Some("F2"),
                action: |a| a.open_rename_overlay(),
                modes: &[],
            },
            Command {
                label: "Refactor / Quick Fix (Code Actions)",
                category: "File",
                shortcut: Some("Alt+Enter"),
                action: |a| a.open_code_actions_overlay(),
                modes: &[],
            },
            Command {
                label: "Duplicate Line",
                category: "Edit",
                shortcut: Some("Ctrl+Shift+D"),
                action: |a| a.queued_line_op = Some(crate::editor::line_ops::LineOp::Duplicate),
                modes: &[],
            },
            Command {
                label: "Delete Line",
                category: "Edit",
                shortcut: Some("Ctrl+Shift+K"),
                action: |a| a.queued_line_op = Some(crate::editor::line_ops::LineOp::Delete),
                modes: &[],
            },
            Command {
                label: "Move Line Up",
                category: "Edit",
                shortcut: Some("Alt+Up"),
                action: |a| a.queued_line_op = Some(crate::editor::line_ops::LineOp::MoveUp),
                modes: &[],
            },
            Command {
                label: "Move Line Down",
                category: "Edit",
                shortcut: Some("Alt+Down"),
                action: |a| a.queued_line_op = Some(crate::editor::line_ops::LineOp::MoveDown),
                modes: &[],
            },
            Command {
                label: "Toggle Line Comment",
                category: "Edit",
                shortcut: Some("Ctrl+/"),
                action: |a| a.queued_toggle_comment = true,
                modes: &[],
            },
            Command {
                label: "Indent Lines",
                category: "Edit",
                shortcut: Some("Tab"),
                action: |a| a.queued_indent = Some(true),
                modes: &[],
            },
            Command {
                label: "Dedent Lines",
                category: "Edit",
                shortcut: Some("Shift+Tab"),
                action: |a| a.queued_indent = Some(false),
                modes: &[],
            },
            Command {
                label: "Jump to Next Change",
                category: "Edit",
                shortcut: Some("Ctrl+Alt+J"),
                action: |a| a.queued_change_jump = Some(true),
                modes: &[],
            },
            Command {
                label: "Jump to Previous Change",
                category: "Edit",
                shortcut: Some("Ctrl+Alt+K"),
                action: |a| a.queued_change_jump = Some(false),
                modes: &[],
            },
            Command {
                label: "Go to Next Problem",
                category: "File",
                shortcut: Some("F8"),
                action: |a| a.queued_problem_jump = Some(true),
                modes: &[],
            },
            Command {
                label: "Go to Previous Problem",
                category: "File",
                shortcut: Some("Shift+F8"),
                action: |a| a.queued_problem_jump = Some(false),
                modes: &[],
            },
            Command {
                label: "Expand Selection",
                category: "Edit",
                shortcut: Some("Shift+Alt+Right"),
                action: |a| a.queued_select = Some(true),
                modes: &[],
            },
            Command {
                label: "Shrink Selection",
                category: "Edit",
                shortcut: Some("Shift+Alt+Left"),
                action: |a| a.queued_select = Some(false),
                modes: &[],
            },
            Command {
                label: "Undo",
                category: "Edit",
                shortcut: Some("Ctrl+Z"),
                action: |a| a.undo_active(),
                modes: &[],
            },
            Command {
                label: "Redo",
                category: "Edit",
                shortcut: Some("Ctrl+Shift+Z"),
                action: |a| a.redo_active(),
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
            Command {
                label: "Nodes: Remote Build Nodes",
                category: "Build",
                shortcut: Some("Ctrl+Alt+B"),
                action: |a| a.open_nodes_panel(),
                modes: &[],
            },
            // Debugging (DAP)
            Command {
                label: "Start / Continue Debugging",
                category: "Debug",
                shortcut: Some("F5"),
                action: |a| a.debug_start_or_continue(),
                modes: &[WorkspaceProfile::Coder],
            },
            Command {
                label: "Stop Debugging",
                category: "Debug",
                shortcut: Some("Shift+F5"),
                action: |a| a.debug_stop(),
                modes: &[WorkspaceProfile::Coder],
            },
            Command {
                label: "Step Over",
                category: "Debug",
                shortcut: Some("F10"),
                action: |a| a.debug_step_over(),
                modes: &[WorkspaceProfile::Coder],
            },
            Command {
                label: "Step Into",
                category: "Debug",
                shortcut: Some("F11"),
                action: |a| a.debug_step_into(),
                modes: &[WorkspaceProfile::Coder],
            },
            Command {
                label: "Step Out",
                category: "Debug",
                shortcut: Some("Shift+F11"),
                action: |a| a.debug_step_out(),
                modes: &[WorkspaceProfile::Coder],
            },
            Command {
                label: "Toggle Breakpoint",
                category: "Debug",
                shortcut: Some("F9"),
                action: |a| a.toggle_breakpoint_current_line(),
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
                label: "Research Browser",
                category: "Workspace",
                shortcut: None,
                action: |a| a.open_browse_workspace(),
                modes: &[],
            },
            Command {
                label: "Clean Build Artifacts\u{2026}",
                category: "Workspace",
                shortcut: Some("Ctrl+Shift+K"),
                action: |a| a.open_disk_hygiene(),
                modes: &[],
            },
            Command {
                label: "Review Changes",
                category: "Panels",
                shortcut: None,
                action: |a| a.focus_panel(TabKind::Changes),
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
                label: "Code Graph",
                category: "Knowledge",
                shortcut: None,
                action: |a| a.toggle_panel(TabKind::Graph),
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
                label: "Knowledge Base",
                category: "Knowledge",
                shortcut: None,
                action: |a| a.toggle_knowledge(),
                modes: &[],
            },
            Command {
                label: "Bookmarks",
                category: "Knowledge",
                shortcut: None,
                action: |a| a.toggle_panel(TabKind::Bookmarks),
                modes: &[],
            },
            Command {
                label: "Agent Memory",
                category: "Knowledge",
                shortcut: None,
                action: |a| a.toggle_panel(TabKind::AgentMemory),
                modes: &[],
            },
            Command {
                label: "Shared Memory",
                category: "Knowledge",
                shortcut: None,
                action: |a| a.toggle_panel(TabKind::SharedMemory),
                modes: &[],
            },
            Command {
                label: "Triggers",
                category: "Automation",
                shortcut: None,
                action: |a| a.toggle_triggers(),
                modes: &[],
            },
            Command {
                label: "Workflows",
                category: "Automation",
                shortcut: None,
                action: |a| a.toggle_workflows(),
                modes: &[],
            },
            Command {
                label: "Governance",
                category: "Automation",
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
                label: "Find in Terminal",
                category: "Edit",
                shortcut: Some("Ctrl+F"),
                action: |a| {
                    a.bottom_panel_state.collapsed = false;
                    a.bottom_panel_state.active_tab = crate::editor::bottom_panel::TAB_TERMINAL;
                    a.terminal_state.open_find();
                },
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
                label: "Toggle Auto Save",
                category: "View",
                shortcut: None,
                action: |a| a.toggle_auto_save(),
                modes: &[],
            },
            Command {
                label: "Toggle Format On Save",
                category: "Edit",
                shortcut: None,
                action: |a| a.toggle_format_on_save(),
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
            Command {
                label: "Git: Switch Branch",
                category: "Git",
                shortcut: None,
                action: |a| a.open_branch_switcher(),
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
            self.push_closed_editor_path(path.clone());
            // Notify LSP server that the document is closed.
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if let Some(lsp) = self.lsp_state.lsp_manager.as_mut() {
                    lsp.close_document(ext, &path);
                }
            }
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
                self.push_closed_editor_path(path.clone());
                // Notify LSP server that the document is closed.
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if let Some(lsp) = self.lsp_state.lsp_manager.as_mut() {
                        lsp.close_document(ext, &path);
                    }
                }
            }
            self.buffers.remove(id);
        }
        self.tabs.retain(|t| t.id == keep);
        self.active_tab = Some(keep);
        self.rebuild_dock();
        self.status_message = "Closed other tabs".into();
    }

    /// Close every editor tab (keeps non-editor panels like Chat/Output).
    pub fn close_all_tabs(&mut self) {
        let removed: Vec<TabId> = self
            .tabs
            .iter()
            .filter(|t| matches!(t.kind, TabKind::Editor { .. }))
            .map(|t| t.id.clone())
            .collect();
        for id in &removed {
            if let Some(path) = self.tab_path(id).cloned() {
                self.push_closed_editor_path(path.clone());
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if let Some(lsp) = self.lsp_state.lsp_manager.as_mut() {
                        lsp.close_document(ext, &path);
                    }
                }
            }
            self.buffers.remove(id);
        }
        self.tabs.retain(|t| !removed.contains(&t.id));
        self.active_tab = self.tabs.first().map(|t| t.id.clone());
        self.rebuild_dock();
        self.status_message = "Closed all editor tabs".into();
    }

    /// Remember a closed editor file so it can be reopened with Ctrl+Shift+T.
    fn push_closed_editor_path(&mut self, path: PathBuf) {
        self.closed_editor_paths
            .retain(|p| !crate::editor::file_ops::same_editor_path(p, &path));
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
        // Fresh session: no LSP request in flight, no debounce clock.
        self.goto_symbol_lsp_pending = None;
        self.goto_symbol_lsp_dispatched.clear();
        self.goto_symbol_lsp_typing = None;
        self.goto_symbol_lsp_retries = 0;
        self.goto_symbol_lsp_note = None;
    }

    /// Language-server feed for the go-to-symbol switcher: once the typed
    /// query settles briefly, dispatch a non-blocking `workspace/symbol`
    /// request (gated on the server advertising the provider), poll the
    /// answer on subsequent frames, and merge it into the entry list. No
    /// capable server — or no answer within the budget — simply leaves the
    /// sitemap-backed symbols in place; keystrokes never wait on the server.
    pub fn update_goto_symbol_lsp(&mut self, query: &str, ctx: &egui::Context) {
        // Poll the in-flight request first: it must land even after the
        // query stops changing.
        if let Some((pext, id, sent_at)) = self.goto_symbol_lsp_pending.clone() {
            if sent_at.elapsed() > std::time::Duration::from_secs(3) {
                // A hung answer must not mute the query forever: re-arm so
                // the same text gets another chance after a backoff pause.
                self.goto_symbol_lsp_pending = None;
                self.goto_symbol_lsp_dispatched.clear();
                self.goto_symbol_lsp_typing = Some(std::time::Instant::now());
                self.goto_symbol_lsp_retries = self.goto_symbol_lsp_retries.saturating_add(1);
                ctx.request_repaint_after(crate::editor::search::symbol_query_retry_delay(
                    self.goto_symbol_lsp_retries,
                ));
            } else if let Some(results) = self
                .lsp_state
                .lsp_manager
                .as_mut()
                .and_then(|lsp| lsp.poll_workspace_symbols(&pext, id))
            {
                self.goto_symbol_lsp_pending = None;
                if results.is_empty() {
                    // A still-loading server (rust-analyzer before its crate
                    // graph is ready) answers `[]` right away. That's "ask
                    // again later", not "nothing exists" — re-arm with the
                    // backoff below until real symbols arrive.
                    self.goto_symbol_lsp_dispatched.clear();
                    self.goto_symbol_lsp_typing = Some(std::time::Instant::now());
                    self.goto_symbol_lsp_retries = self.goto_symbol_lsp_retries.saturating_add(1);
                    // The retry needs a future frame that no keystroke will
                    // ever provide (the user has stopped typing, the server
                    // is still loading) — schedule the wake-up explicitly.
                    ctx.request_repaint_after(crate::editor::search::symbol_query_retry_delay(
                        self.goto_symbol_lsp_retries,
                    ));
                    self.goto_symbol_lsp_note =
                        Some("Language server is still loading symbols\u{2026}".into());
                } else {
                    self.goto_symbol_lsp_retries = 0;
                    self.goto_symbol_lsp_note = None;
                    self.goto_symbol_entries = crate::editor::search::merge_workspace_symbols(
                        &self.workspace_symbols,
                        &results,
                        &self.workspace_root,
                    );
                    // Sentinel forces the overlay to re-filter the merged
                    // list next frame (no real query can compare equal).
                    self.goto_symbol_last_query = "\u{0}".into();
                    self.goto_symbol_selected = 0;
                }
            } else {
                // Answer still in transit: keep the frames coming.
                ctx.request_repaint();
                return;
            }
        }
        // Restart the debounce clock whenever the query changes.
        if self.goto_symbol_last_query != query {
            self.goto_symbol_lsp_typing = Some(std::time::Instant::now());
            // Fresh text deserves a fresh (quick) chance.
            self.goto_symbol_lsp_retries = 0;
        }
        let Some(paused) = self.goto_symbol_lsp_typing else {
            return;
        };
        if query.is_empty() || query == self.goto_symbol_lsp_dispatched {
            return;
        }
        let delay = crate::editor::search::symbol_query_retry_delay(self.goto_symbol_lsp_retries);
        if paused.elapsed() < delay {
            // Guarantee the debounce deadline gets a frame even if the user
            // stops typing and no other event wakes the app.
            ctx.request_repaint_after(delay - paused.elapsed());
            return;
        }
        // The server is picked through the active editor's language; with no
        // editor open there is nothing to ask, and the local list stands.
        let Some((_, ext, _)) = self.active_lsp_target() else {
            self.goto_symbol_lsp_note =
                Some("Open a code file to search its language server".into());
            return;
        };
        let dispatched = self
            .lsp_state
            .lsp_manager
            .as_mut()
            .and_then(|lsp| lsp.request_workspace_symbols(&ext, query));
        // Mark the attempt even when it fails: a dead or incapable server
        // must not be re-poked on every frame for the same query.
        self.goto_symbol_lsp_dispatched = query.to_string();
        if dispatched.is_none() {
            // Say so honestly instead of blaming the user's indexer.
            self.goto_symbol_lsp_note = Some(format!(
                "Language server has no workspace-symbol provider for .{ext}"
            ));
        }
        if let Some(id) = dispatched {
            self.goto_symbol_lsp_pending = Some((ext, id, std::time::Instant::now()));
            ctx.request_repaint();
        }
    }

    /// Open the file defining `entry` and jump to the symbol's definition line.
    pub fn jump_to_symbol(&mut self, entry: &crate::editor::search::SymbolEntry) {
        self.push_nav_location();
        let abs = self.workspace_root.join(&entry.file);
        // Language-server entries carry the exact line; sitemap entries fall
        // back to a text scan of the definition file.
        let line = entry.line.or_else(|| {
            std::fs::read_to_string(&abs).ok().and_then(|content| {
                crate::editor::search::find_definition_line(&content, &entry.name)
            })
        });
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

    /// Move the caret to a 1-based `line` in the already-active editor and
    /// record the jump on the back stack. Used by the Outline rows: the file is
    /// open, so this is a pure in-buffer scroll (no `open_editor` round-trip).
    pub fn jump_to_line_in_active(&mut self, line: usize) {
        self.push_nav_location();
        self.pending_cursor_line = Some(line);
    }

    /// Load and display a file's diff in the Changes panel. Untracked files
    /// have no `git diff` body, so they are shown as an all-additions view of
    /// their on-disk content; everything else uses the real unified diff.
    /// Accepts either spelling — panel rows carry the porcelain-relative path,
    /// external callers an absolute one — by resolving to the absolute form for
    /// filesystem reads and the repo-relative form for the git pathspec.
    pub fn load_scm_diff(&mut self, path: &std::path::Path) {
        use crate::editor::diff_view;
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace_root.join(path)
        };
        let rel = abs
            .strip_prefix(&self.workspace_root)
            .unwrap_or(&abs)
            .to_path_buf();
        let lines = if self.is_untracked(&rel) {
            let text = std::fs::read_to_string(&abs).unwrap_or_default();
            diff_view::diff_from_sides(Some(&text), None)
        } else {
            self.git_state.diff_file(&self.workspace_root, &rel);
            diff_view::parse_unified_diff(&self.git_state.diff_output.clone())
        };
        self.scm_diff_lines = lines;
        self.scm_diff_path = Some(abs);
    }

    /// Header label for the open diff, or `None` when no file is selected.
    pub fn scm_diff_label(&self) -> Option<String> {
        self.scm_diff_path
            .as_ref()
            .map(|p| crate::editor::diff_view::diff_header_label(&self.workspace_root, p))
    }

    fn is_untracked(&self, path: &std::path::Path) -> bool {
        self.git_state
            .entries
            .iter()
            .any(|e| e.path == path && e.status == crate::editor::git_ui::GitFileStatus::Untracked)
    }

    /// Snapshot the active editor's file/line onto the back stack. Called before
    /// a jump so it can be unwound with Alt+â†. Clears the forward stack.
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

    /// Navigate to the previous location (Alt+â†).
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

    /// Navigate forward again after going back (Alt+â†’).
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
                } if crate::editor::file_ops::same_editor_path(tab_path, p) => Some(tab.id.clone()),
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
        // The buffer carries its path from birth: `active_lsp_target()` reads
        // it, and an async open that left it None silently muted every LSP
        // feature — go-to-definition, hover, workspace/symbol — for any file
        // opened through this path, not just the symbol switcher.
        let mut buf = EditorBuffer::default();
        buf.path = path.clone();
        if let Some(ref p) = path {
            // Spawn a background thread to read the file so the UI stays responsive.
            let tab_id = id.clone();
            let file_path = p.clone();
            let tx = self.file_io_tx.clone();
            self.pending_file_loads.insert(id.clone());
            std::thread::spawn(move || match std::fs::read_to_string(&file_path) {
                Ok(content) => {
                    let mtime = std::fs::metadata(&file_path)
                        .and_then(|m| m.modified())
                        .ok();
                    let _ = tx.send(super::super::types::FileIoResult::FileLoaded {
                        tab_id,
                        path: file_path,
                        content,
                        mtime,
                    });
                }
                Err(e) => {
                    let _ = tx.send(super::super::types::FileIoResult::FileLoadFailed {
                        tab_id,
                        path: file_path,
                        error: e.to_string(),
                    });
                }
            });
        }
        self.buffers.insert(id.clone(), buf);
        self.tabs.push(tab.clone());
        if let Some(dock) = self.dock_state.as_mut() {
            dock.push_to_focused_leaf(tab);
        }
        self.active_tab = Some(id.clone());
        self.touch_mru(&id);
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

        // Share the same buffer content by copying from the existing buffer.
        let active_buf_id = self
            .active_tab
            .as_ref()
            .and_then(|id| self.tabs.iter().find(|t| &t.id == id).map(|t| t.id.clone()));
        let buf = if let Some(src_id) = active_buf_id {
            if let Some(src) = self.buffers.get(&src_id) {
                let mut b = EditorBuffer::default();
                b.path = src.path.clone();
                b.load_text(src.content());
                b.disk_mtime = src.disk_mtime;
                b
            } else {
                EditorBuffer::default()
            }
        } else {
            EditorBuffer::default()
        };
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

    // ─── Hot exit: unsaved buffers survive a restart ────────────────────────

    /// Capture every dirty editor buffer to `.velocity/hot-exit.json` so
    /// closing the window never silently discards work. With nothing dirty
    /// the file is removed instead — there is nothing left to rescue.
    pub fn write_hot_exit_session(&self) {
        use crate::editor::hot_exit;
        let path = hot_exit::session_path(&self.workspace_root);
        let mut files = Vec::new();
        let mut active_index = None;
        for tab in &self.tabs {
            let TabKind::Editor { buffer_id, .. } = &tab.kind else {
                continue;
            };
            let Some(buf) = self.buffers.get(buffer_id) else {
                continue;
            };
            if !buf.is_dirty() {
                continue;
            }
            if self.active_tab.as_ref() == Some(&tab.id) {
                active_index = Some(files.len());
            }
            files.push(hot_exit::HotExitFile {
                path: buf.path.clone(),
                content: buf.content.clone(),
            });
        }
        if files.is_empty() {
            let _ = hot_exit::clear_session(&path);
            return;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let session = hot_exit::build_session(files, active_index, now);
        let _ = hot_exit::write_session(&path, &session);
    }

    /// Reopen the previous session's unsaved tabs — marked dirty against the
    /// bytes on disk, exactly as the user left them — on the first frame,
    /// once. The session file is consumed either way so a restore never
    /// replays, and expired sessions are dropped rather than restored.
    pub fn restore_hot_exit(&mut self) {
        use crate::editor::hot_exit;
        if self.hot_exit_restored {
            return;
        }
        self.hot_exit_restored = true;
        let path = hot_exit::session_path(&self.workspace_root);
        let Some(session) = hot_exit::read_session(&path) else {
            return;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if hot_exit::is_expired(&session, now, hot_exit::RETENTION_DAYS) {
            let _ = hot_exit::clear_session(&path);
            return;
        }
        let mut restored = 0usize;
        let mut active: Option<TabId> = None;
        let mut last: Option<TabId> = None;
        for (i, file) in session.files.iter().enumerate() {
            // On a restart, workspace preferences reopen the last session's
            // tabs (with an async disk read in flight) before this runs. The
            // rescued buffer wins over that stale disk version: overlay it on
            // the already-open tab and cancel its pending load. Skipping the
            // file instead would silently discard unsaved work on every
            // restart after the first.
            let open = file.path.as_ref().and_then(|p| {
                self.tabs.iter().find_map(|t| match &t.kind {
                    TabKind::Editor {
                        path: Some(tp),
                        buffer_id,
                        ..
                    } if crate::editor::file_ops::same_editor_path(tp, p) => {
                        Some((t.id.clone(), buffer_id.clone()))
                    }
                    _ => None,
                })
            });
            if let Some((tab_id, buffer_id)) = open {
                // Edits made since launch are newer than the rescue file;
                // never clobber them. (A reopened tab is always clean at
                // startup, so this only guards future refactors.)
                if self.buffers.get(&buffer_id).is_some_and(|b| b.is_dirty()) {
                    continue;
                }
                let path = file.path.as_ref().expect("open tab path");
                self.pending_file_loads.remove(&buffer_id);
                let (disk, mtime) = (
                    std::fs::read_to_string(path).unwrap_or_default(),
                    std::fs::metadata(path).and_then(|m| m.modified()).ok(),
                );
                if let Some(buf) = self.buffers.get_mut(&buffer_id) {
                    *buf = EditorBuffer::new(Some(path.clone()), disk);
                    buf.disk_mtime = mtime;
                    buf.update_content(file.content.clone());
                }
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if let Some(lsp) = self.lsp_state.lsp_manager.as_mut() {
                        lsp.sync_document(ext, path, &file.content);
                    }
                }
                if session.active_index == Some(i) {
                    active = Some(tab_id.clone());
                }
                last = Some(tab_id);
                restored += 1;
                continue;
            }
            let (disk, mtime) = match &file.path {
                Some(p) => (
                    std::fs::read_to_string(p).unwrap_or_default(),
                    std::fs::metadata(p).and_then(|m| m.modified()).ok(),
                ),
                None => (String::new(), None),
            };
            let mut buf = EditorBuffer::new(file.path.clone(), disk);
            buf.disk_mtime = mtime;
            // Layering the unsaved content over the disk baseline marks the
            // buffer dirty against disk — the tab comes back exactly as left,
            // and Save writes the right bytes.
            buf.update_content(file.content.clone());
            let id = TabId::next(&mut self.tab_counter);
            let tab = Tab {
                id: id.clone(),
                kind: TabKind::Editor {
                    path: file.path.clone(),
                    buffer_id: id.clone(),
                },
            };
            if let Some(p) = &file.path {
                if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                    if let Some(lsp) = self.lsp_state.lsp_manager.as_mut() {
                        lsp.sync_document(ext, p, &file.content);
                    }
                }
            }
            self.buffers.insert(id.clone(), buf);
            self.tabs.push(tab.clone());
            if let Some(dock) = self.dock_state.as_mut() {
                dock.push_to_focused_leaf(tab);
            }
            self.touch_mru(&id);
            restored += 1;
            if session.active_index == Some(i) {
                active = Some(id.clone());
            }
            last = Some(id);
        }
        if restored > 0 {
            self.active_tab = active.or(last);
            if let Some(id) = self.active_tab.clone() {
                self.touch_mru(&id);
            }
            self.status_message =
                format!("Restored {restored} unsaved file(s) from the previous session");
        }
        let _ = hot_exit::clear_session(&path);
    }

    /// Switch workspaces in place (the project switcher). Order matters:
    /// the departing workspace's dirty buffers are captured to *its own*
    /// hot-exit file before the root moves — otherwise the next exit would
    /// journal them under the new root, or drop them on the floor. Then the
    /// old workspace's editor tabs close and the new one's preferences
    /// reopen, along with any rescued buffers it holds, so the UI lands
    /// exactly where that workspace's last session left it.
    pub fn switch_workspace_to(&mut self, path: PathBuf) {
        if crate::editor::file_ops::same_editor_path(&path, &self.workspace_root) {
            return;
        }
        self.write_hot_exit_session();
        self.save_workspace_preferences();
        let stale: Vec<TabId> = self
            .tabs
            .iter()
            .filter(|t| matches!(t.kind, TabKind::Editor { .. }))
            .map(|t| t.id.clone())
            .collect();
        for id in stale {
            self.close_tab(&id);
        }
        self.rebuild_dock();
        self.workspace_root = path;
        self.status_message = format!(
            "Switched to {}",
            self.workspace_root
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| self.workspace_root.display().to_string())
        );
        self.restore_workspace_preferences();
        // The new root may hold unsaved work from when it was last open;
        // a restore overwrites the status with "Restored N ..." so the
        // rescue stays visible.
        self.hot_exit_restored = false;
        self.restore_hot_exit();
    }

    /// Ctrl+O: raise the file picker.
    ///
    /// There is no native dialog and nothing to fall back to --
    /// `open_file_dialog` shows the app's own browser, which is the only picker
    /// there is. The comment here claimed otherwise for long enough that the
    /// bridge gate believed it and refused two commands a driver can actually
    /// complete.
    pub fn prompt_open_file(&mut self) {
        self.open_file_dialog();
    }

    /// Open the in-app file browser. An `egui::Window` inside this app rather
    /// than a system modal: the frame keeps drawing, Cancel closes it, and
    /// `dismiss_transient_ui` stands it down from outside the frame loop.
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

    /// Ctrl+Shift+S: raise the in-app save-as prompt for the focused editor.
    /// With no editor open there is nothing to name, so it says so rather than
    /// showing a dialog that could not save anything.
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
        // Format-on-save runs before the write so both the disk and the
        // editor end up holding the formatted text; the success path below
        // re-baselines the dirty flag via mark_saved as usual.
        self.format_buffer_for_save(id, path);
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
                    // Notify LSP server of the saved content (ensures server has latest).
                    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                        if let Some(buf) = self.buffers.get(id) {
                            if let Some(lsp) = self.lsp_state.lsp_manager.as_mut() {
                                lsp.sync_document(ext, path, buf.content());
                            }
                        }
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

    /// Files dropped onto the window from the OS shell arrive as paths; open
    /// each real file as an editor tab. `open_editor` already dedupes by path
    /// and routes `.nda` documents, so drops behave exactly like opening from
    /// the explorer. Non-files (folders, vanished entries) are ignored.
    pub fn handle_dropped_paths(&mut self, paths: Vec<PathBuf>) {
        let mut opened = 0usize;
        for path in paths {
            if path.is_file() {
                self.open_editor(Some(path));
                opened += 1;
            }
        }
        if opened > 0 {
            self.rebuild_dock();
            self.status_message = format!("Opened {opened} dropped file(s)");
        }
    }

    /// Flip auto-save and persist the choice alongside the other workspace
    /// preferences so a restart keeps the user's setting.
    pub fn toggle_auto_save(&mut self) {
        self.auto_save = !self.auto_save;
        self.save_workspace_preferences();
        self.status_message = format!(
            "Auto-save {}",
            if self.auto_save {
                "enabled"
            } else {
                "disabled"
            }
        );
    }

    /// Tabs auto-save should write: dirty buffers on editor tabs that have a
    /// real path on disk. Untitled buffers are skipped -- autosave must not
    /// invent filenames or prompt mid-edit.
    pub fn auto_save_candidates(&self) -> Vec<(TabId, PathBuf)> {
        self.tabs
            .iter()
            .filter(|t| self.tab_is_dirty(&t.id))
            .filter_map(|t| t.editor_path().cloned().map(|p| (t.id.clone(), p)))
            .collect()
    }

    /// Per-frame auto-save sweep, throttled to one pass every two seconds.
    /// Uses the same silent path as Ctrl+S (`success_feedback = false`) so
    /// git status and LSP sync stay consistent without toast spam.
    pub fn auto_save_tick(&mut self) {
        if !self.auto_save {
            return;
        }
        let due = self
            .last_auto_save
            .map(|t| t.elapsed() >= std::time::Duration::from_secs(2))
            .unwrap_or(true);
        if !due {
            return;
        }
        self.last_auto_save = Some(std::time::Instant::now());
        for (id, path) in self.auto_save_candidates() {
            self.save_buffer_to_with_feedback(&id, &path, false);
        }
    }

    /// Flip format-on-save and persist the choice with the other workspace
    /// preferences.
    pub fn toggle_format_on_save(&mut self) {
        self.format_on_save = !self.format_on_save;
        self.save_workspace_preferences();
        self.status_message = format!(
            "Format on save {}",
            if self.format_on_save {
                "enabled"
            } else {
                "disabled"
            }
        );
    }

    /// Format-on-save hook run by every save path (Ctrl+S, Save All,
    /// auto-save) before bytes hit the disk: when enabled and the language
    /// server answers with formatted text, the buffer is rewritten (through
    /// the dirty/undo path, then re-baselined by the save) and the server is
    /// fed the new text. Silent by design: no formatter, no change, no
    /// message -- the save proceeds with whatever content is there.
    pub fn format_buffer_for_save(&mut self, id: &TabId, path: &PathBuf) {
        if !self.format_on_save {
            return;
        }
        let Some(ext) = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_string)
        else {
            return;
        };
        let Some(content) = self.buffers.get(id).map(|b| b.content().to_string()) else {
            return;
        };
        let (tab_size, insert_spaces) = match self
            .buffers
            .get(id)
            .map(|b| b.indent_style)
            .unwrap_or_default()
        {
            crate::editor::auto_indent::IndentStyle::Tabs => (4, false),
            crate::editor::auto_indent::IndentStyle::Spaces(w) => ((w as u64).max(1), true),
        };
        let formatted = match self.lsp_state.lsp_manager.as_mut() {
            Some(lsp) => lsp.format_document(&ext, path, &content, tab_size, insert_spaces),
            None => None,
        };
        if let Some(text) = formatted {
            if text != content {
                if let Some(buf) = self.buffers.get_mut(id) {
                    // content_mut() marks the buffer mutated, so Ctrl+Z can
                    // undo the formatter's rewrite; mark_saved() in the save
                    // path then clears the flag against the new baseline.
                    *buf.content_mut() = text.clone();
                }
                if let Some(lsp) = self.lsp_state.lsp_manager.as_mut() {
                    lsp.sync_document(&ext, path, &text);
                }
            }
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

    /// Drain all pending background file I/O results and apply them.
    /// Called once per frame from the render loop.
    pub fn poll_file_io_results(&mut self) {
        while let Ok(result) = self.file_io_rx.try_recv() {
            self.apply_file_io_result(result);
        }
    }

    fn apply_file_io_result(&mut self, result: FileIoResult) {
        match result {
            FileIoResult::FileLoaded {
                tab_id,
                path,
                content,
                mtime,
            } => {
                // No pending marker means the load was cancelled — hot-exit
                // restore overlaid rescued work onto this tab. Applying the
                // disk version now would silently clobber the unsaved buffer.
                if !self.pending_file_loads.remove(&tab_id) {
                    return;
                }
                if let Some(buf) = self.buffers.get_mut(&tab_id) {
                    // Re-assert the path even if the buffer was created by an
                    // older code path — content and path must agree on which
                    // file this buffer is.
                    buf.path = Some(path.clone());
                    buf.load_text(&content);
                    buf.disk_mtime = mtime;
                }
                // Announce the file to the LSP server now that content is loaded.
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if let Some(lsp) = self.lsp_state.lsp_manager.as_mut() {
                        lsp.sync_document(ext, &path, &content);
                    }
                }
            }
            FileIoResult::FileLoadFailed {
                tab_id,
                path,
                error,
            } => {
                if !self.pending_file_loads.remove(&tab_id) {
                    return; // cancelled load: nothing to report
                }
                // Surface the underlying cause: "permission denied", "file not
                // found" and "file locked" all need a different user response,
                // so the reason has to reach the status line and the toast
                // (matching FileSaveFailed below).
                self.status_message = format!("Error reading {}: {}", path.display(), error);
                self.toasts.push(crate::editor::toast::Toast::error(format!(
                    "Failed to open {}: {error}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                )));
            }
            FileIoResult::FileSaved { path } => {
                self.toasts
                    .push(crate::editor::toast::Toast::success(format!(
                        "Saved {}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    )));
            }
            FileIoResult::FileSaveFailed { path, error } => {
                self.status_message = format!("Error saving {}: {}", path.display(), error);
                self.toasts.push(crate::editor::toast::Toast::error(format!(
                    "Failed to save: {error}"
                )));
            }
        }
    }

    // ─── Disk hygiene (Clean Build Artifacts…, Ctrl+Shift+K) ────────────────

    /// Open the hygiene overlay and kick off a background scan. Opening is
    /// non-destructive: nothing leaves the disk until the overlay's confirm
    /// click, so the palette entry (and the bridge) can reach it freely.
    pub fn open_disk_hygiene(&mut self) {
        self.hygiene.open = true;
        self.hygiene.confirming = false;
        self.start_hygiene_scan();
    }

    /// Scan off the UI thread: a workspace can hold millions of artifact
    /// files and the frame loop must never wait on that walk.
    pub fn start_hygiene_scan(&mut self) {
        if self.hygiene.scanning {
            return;
        }
        self.hygiene.scanning = true;
        let root = self.workspace_root.clone();
        let tx = self.hygiene.tx.clone();
        std::thread::spawn(move || {
            let report = crate::disk_hygiene::scan_and_record(&root);
            let _ = tx.send(super::substructs::HygieneEvent::ScanDone(report));
        });
    }

    /// Drain completed background work into the overlay state. Called each
    /// frame the overlay is open; results also land in the status line so the
    /// outcome is visible even if the overlay was dismissed mid-clean.
    pub fn handle_hygiene_events(&mut self) {
        use super::substructs::HygieneEvent;
        use crate::disk_hygiene::format_bytes;
        while let Ok(event) = self.hygiene.rx.try_recv() {
            match event {
                HygieneEvent::ScanDone(report) => {
                    self.hygiene.scanning = false;
                    // Fresh scan: re-check every safe tree, drop stale picks.
                    self.hygiene.checked = report
                        .entries
                        .iter()
                        .filter(|e| e.safety == crate::disk_hygiene::Safety::Safe)
                        .map(|e| e.relative_path.clone())
                        .collect();
                    self.hygiene.report = Some(report);
                }
                HygieneEvent::CleanDone(result) => {
                    self.hygiene.cleaning = false;
                    self.hygiene.confirming = false;
                    let verb = if result.dry_run {
                        "Would reclaim"
                    } else {
                        "Reclaimed"
                    };
                    let mut line = format!(
                        "{verb} {} ({} files)",
                        format_bytes(result.freed_bytes),
                        crate::disk_hygiene::format_count(result.file_count)
                    );
                    if !result.rejected.is_empty() {
                        line.push_str(&format!(" — refused: {}", result.rejected.join("; ")));
                    }
                    if !result.dry_run && result.freed_bytes > 0 {
                        self.status_message = line.clone();
                        self.toasts
                            .push(crate::editor::toast::Toast::success(line.clone()));
                    }
                    self.hygiene.last_result = Some(line);
                    // Refresh the table so removed trees disappear from it.
                    self.start_hygiene_scan();
                }
            }
        }
    }

    /// Reclaim (or dry-run) the checked trees on a background thread.
    pub fn start_hygiene_clean(&mut self, dry_run: bool) {
        if self.hygiene.cleaning || self.hygiene.scanning {
            return;
        }
        let selected: Vec<String> = self.hygiene.checked.clone();
        if selected.is_empty() {
            self.hygiene.last_result = Some("Nothing selected to reclaim.".into());
            return;
        }
        self.hygiene.cleaning = true;
        let root = self.workspace_root.clone();
        let tx = self.hygiene.tx.clone();
        std::thread::spawn(move || {
            let result = crate::disk_hygiene::clean(&root, Some(&selected), dry_run);
            let _ = tx.send(super::substructs::HygieneEvent::CleanDone(result));
        });
    }

    // ── Build nodes (Nodes panel + remote routing) ────────────────────────────

    /// Reveal the Build rail's Nodes section (single writer for the rail
    /// selection state, via `select_rail_section`).
    pub fn open_nodes_panel(&mut self) {
        self.select_rail_section("build", "nodes");
    }

    /// Drain completed background node operations into app state. Runs once per
    /// frame rather than only while the panel is visible: a routed build must
    /// report its outcome even if the user navigated away mid-run.
    pub fn handle_nodes_events(&mut self, ctx: &egui::Context) {
        use super::substructs::NodesEvent;
        let mut woke = false;
        while let Ok(event) = self.nodes.rx.try_recv() {
            woke = true;
            match event {
                NodesEvent::Pinged { id, summary } => {
                    self.nodes.pinging.retain(|p| p != &id);
                    self.nodes.status_line = summary;
                }
                NodesEvent::ExecDone {
                    id: _,
                    text,
                    ok,
                    routed,
                } => {
                    self.nodes.exec_busy = false;
                    if routed {
                        self.agent_active = false;
                        self.status_message = if ok {
                            "Remote build finished".to_string()
                        } else {
                            "Remote build failed".to_string()
                        };
                        // Mirror the local build indicator from what the drone
                        // actually reported.
                        let errs = if ok {
                            0
                        } else {
                            text.lines()
                                .filter(|l| l.starts_with("error"))
                                .count()
                                .max(1)
                        };
                        self.build_errors_count = errs;
                        if ok {
                            self.toasts.push(crate::editor::toast::Toast::success(
                                "Remote build finished",
                            ));
                        } else {
                            self.toasts
                                .push(crate::editor::toast::Toast::error("Remote build failed"));
                        }
                    } else {
                        self.nodes.status_line = if ok {
                            "Command finished on the node".into()
                        } else {
                            "Command failed on the node".into()
                        };
                    }
                    self.command_output.push_str(&text);
                    if !self.command_output.ends_with('\n') {
                        self.command_output.push('\n');
                    }
                }
            }
        }
        if woke || !self.nodes.pinging.is_empty() || self.nodes.exec_busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    /// The routing target as a live record: it exists in the registry and is
    /// eligible right now. `None` means "build here" - including when the id
    /// was forgotten or the node went stale, which is safer than promising a
    /// remote run that would fail.
    pub fn routed_node(&self) -> Option<crate::agent::instances::InstanceRecord> {
        let id = self.nodes.target_id.as_deref()?;
        let registry = crate::agent::instances::InstanceRegistry::load(
            &crate::agent::instance_tools::instances_path(&self.workspace_root),
        );
        let rec = registry.get(id)?;
        if rec.eligible(crate::agent::instance_tools::now_secs()) {
            Some(rec.clone())
        } else {
            None
        }
    }

    /// Health-ping a node on a background thread through the same tool handler
    /// the agent uses (`instance_ping`), so sightings persist and platforms get
    /// adopted no matter who asked.
    pub fn ping_node(&mut self, id: String) {
        if self.nodes.pinging.iter().any(|p| p == &id) {
            return;
        }
        self.nodes.pinging.push(id.clone());
        let root = self.workspace_root.clone();
        let tx = self.nodes.tx.clone();
        std::thread::spawn(move || {
            let args = serde_json::json!({ "id": &id });
            let summary = match crate::agent::instance_tools::handle_instance_tool(
                &root,
                "instance_ping",
                &args,
            ) {
                Ok(Some(out)) => {
                    let v: serde_json::Value = serde_json::from_str(&out).unwrap_or_default();
                    let name = v["instance"]["name"].as_str().unwrap_or(id.as_str());
                    if v["success"].as_bool().unwrap_or(false) {
                        let status = v["status"].as_str().unwrap_or("online");
                        let env = v["drone"]["environment"].as_str().unwrap_or("");
                        format!("{name}: {status} ({env})")
                    } else {
                        let err = v["error"].as_str().unwrap_or("unreachable");
                        format!("{name}: offline - {err}")
                    }
                }
                Ok(None) => format!("{id}: ping was not handled"),
                Err(e) => format!("{id}: {e}"),
            };
            let _ = tx.send(super::substructs::NodesEvent::Pinged { id, summary });
        });
    }

    /// Register the add-form node through `instance_add`, then ping it: a fresh
    /// node has no sighting, and the panel should show its real state rather
    /// than "unknown".
    pub fn add_node(&mut self) {
        let name = self.nodes.add_name.trim().to_string();
        let addr = self.nodes.add_addr.trim().to_string();
        let dir = self.nodes.add_dir.trim().to_string();
        if name.is_empty() || addr.is_empty() {
            self.nodes.status_line = "Name and drone address are required.".into();
            return;
        }
        let args = serde_json::json!({
            "name": name,
            "drone_addr": addr,
            "role": "buildbox",
            "work_dir": dir,
        });
        match crate::agent::instance_tools::handle_instance_tool(
            &self.workspace_root,
            "instance_add",
            &args,
        ) {
            Ok(Some(out)) => {
                let v: serde_json::Value = serde_json::from_str(&out).unwrap_or_default();
                let id = v["instance"]["id"].as_str().unwrap_or("").to_string();
                self.nodes.add_name.clear();
                self.nodes.add_addr.clear();
                self.nodes.add_dir.clear();
                self.nodes.show_add = false;
                self.nodes.status_line = format!("Added {name}; pinging...");
                if !id.is_empty() {
                    self.ping_node(id);
                }
            }
            Ok(None) => self.nodes.status_line = "Add was not handled.".into(),
            Err(e) => self.nodes.status_line = format!("Add failed: {e}"),
        }
    }

    /// Drop a node from the registry. If it was the routing target, routing
    /// returns to local so Ctrl+B never keeps pointing at a dead id.
    pub fn forget_node(&mut self, id: &str) {
        let args = serde_json::json!({ "id": id });
        match crate::agent::instance_tools::handle_instance_tool(
            &self.workspace_root,
            "instance_remove",
            &args,
        ) {
            Ok(Some(_)) => {
                if self.nodes.target_id.as_deref() == Some(id) {
                    self.nodes.target_id = None;
                }
                self.nodes.status_line = format!("Forgot {id}.");
            }
            Ok(None) => self.nodes.status_line = "Remove was not handled.".into(),
            Err(e) => self.nodes.status_line = format!("Remove failed: {e}"),
        }
    }

    /// Run `command` on the node in the background: claim, submit, poll to a
    /// terminal state, release. `routed` marks a build/run redirected from the
    /// Build buttons (whose completion clears `agent_active`); direct panel
    /// commands pass `false`.
    pub fn run_on_node(&mut self, id: String, command: String, routed: bool) {
        if self.nodes.exec_busy {
            self.nodes.status_line = "A remote command is already running.".into();
            return;
        }
        self.nodes.exec_busy = true;
        self.nodes.status_line = format!("Running on {id}...");
        let root = self.workspace_root.clone();
        let tx = self.nodes.tx.clone();
        std::thread::spawn(move || {
            let (ok, text) = node_exec_blocking(&root, &id, &command);
            let _ = tx.send(super::substructs::NodesEvent::ExecDone {
                id,
                text,
                ok,
                routed,
            });
        });
    }
}

/// The blocking half of [`VelocityApp::run_on_node`]. Runs on a background
/// thread and owns its registry touches outright - load, claim, execute,
/// release - reloading the file at each step because an agent may be mutating
/// the same registry while we wait on the node.
fn node_exec_blocking(root: &std::path::Path, id: &str, command: &str) -> (bool, String) {
    use crate::agent::drone_bridge::DroneClient;
    use crate::agent::instance_tools::{drone_url_for, instances_path};
    use crate::agent::instances::InstanceRegistry;

    let path = instances_path(root);
    let rec = match InstanceRegistry::load(&path).get(id) {
        Some(r) => r.clone(),
        None => return (false, format!("{id}: no longer registered.")),
    };
    let mut reg = InstanceRegistry::load(&path);
    reg.claim(id);
    let _ = reg.save_in_place();

    let client = DroneClient::new(&drone_url_for(&rec.drone_addr), rec.auth_token.as_deref());
    let header = format!("---- {} ({}) - {command}", rec.name, rec.drone_addr);
    let mut ok = false;
    let mut body = String::new();
    match client.submit_task(command) {
        Err(e) => body.push_str(&format!("submit failed: {e}\n")),
        Ok(sub) => {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(900);
            loop {
                match client.task_status(&sub.task_id) {
                    Ok(st) => {
                        if st.status != "pending" && st.status != "running" {
                            let exit = st.effective_exit_code().unwrap_or(-1);
                            ok = exit == 0;
                            body.push_str(&format!("exit {exit}\n"));
                            if let Some(so) = st.effective_stdout() {
                                body.push_str(so);
                            }
                            if let Some(se) = st.effective_stderr() {
                                if !se.trim().is_empty() {
                                    body.push_str("\n[stderr]\n");
                                    body.push_str(se);
                                }
                            }
                            break;
                        }
                    }
                    Err(e) => {
                        body.push_str(&format!("status poll failed: {e}\n"));
                        break;
                    }
                }
                if std::time::Instant::now() >= deadline {
                    body.push_str("timed out after 900s; the task may still be running there.\n");
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
    }
    let mut reg = InstanceRegistry::load(&path);
    reg.release(id);
    let _ = reg.save_in_place();
    (ok, format!("{header}\n{body}\n"))
}
