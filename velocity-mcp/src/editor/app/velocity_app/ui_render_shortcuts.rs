//! Global keyboard shortcuts rendering for `VelocityApp`.
//!
//! Extracted verbatim from `ui_render.rs` (no logic changes).
use super::super::types::*;
use super::struct_def::VelocityApp;
use egui;

impl VelocityApp {
    /// Run any caret-needing editor action the command palette queued for this
    /// frame. Palette actions only set `queued_*` flags (they run without an
    /// `egui::Context`); this is the single consumption point, called from the
    /// shortcut handler each frame *and* from the GUI bridge's `RunCommand`
    /// right before it reports state, so a driver never sees a one-frame-stale
    /// status or `state_after`.
    pub fn flush_queued_editor_actions(&mut self, ctx: &egui::Context) {
        if let Some(op) = self.queued_line_op.take() {
            self.apply_line_op(ctx, op);
        }
        if std::mem::take(&mut self.queued_toggle_comment) {
            self.toggle_line_comment(ctx);
        }
        if let Some(indent) = self.queued_indent.take() {
            self.apply_indent(ctx, indent);
        }
        if let Some(expand) = self.queued_select.take() {
            self.select_surrounding_item(ctx, expand);
        }
        if let Some(forward) = self.queued_change_jump.take() {
            self.goto_change(ctx, forward);
        }
        if let Some(forward) = self.queued_problem_jump.take() {
            self.goto_problem(forward);
        }
    }

    pub fn handle_global_shortcuts(&mut self, ctx: &egui::Context) {
        if self.command_palette.open
            || self.quick_open.open
            || self.goto_line_open
            || self.branch_switcher_open
            || self.goto_symbol_open
            || self.rename_open
            || self.code_actions_open
            || self.call_hierarchy_open
            || self.mru.open
        {
            return;
        }
        // Apply a whole-line edit queued by the command palette now that we have
        // an `egui::Context` to read/re-aim the caret through.
        self.flush_queued_editor_actions(ctx);
        // Whether the user pressed Tab while an inline suggestion is showing.
        // Detected inside the input closure (which borrows `ctx`) and acted on
        // afterwards so we can pass `ctx` to the accept routine.
        let mut accept_inline = false;
        // Whole-line edits (duplicate / delete / move) are `editorFocus`-gated
        // staples the default keymap advertises. Only fire them while the code
        // editor holds focus -- so a terminal or chat field keeps its own
        // Alt+Up/Down history -- and consume the chord afterwards so egui's
        // TextEdit does not also move the caret underneath the operation.
        let editor_focused = self
            .active_tab
            .as_ref()
            .map(crate::editor::code_editor::CodeEditor::textedit_id)
            .is_some_and(|eid| ctx.memory(|m| m.focused() == Some(eid)));
        let mut pending_line_op: Option<(
            crate::editor::line_ops::LineOp,
            egui::Modifiers,
            egui::Key,
        )> = None;
        // Ctrl+/ toggles a line comment; detected here (with the editor focus
        // gate) and acted on after the closure so its chord can be consumed.
        let mut pending_comment: Option<(egui::Modifiers, egui::Key)> = None;
        // Indent/dedent of a block (edit.indent / edit.dedent) — the last
        // advertised editor commands still lacking a handler.
        let mut pending_indent: Option<(bool, egui::Modifiers, egui::Key)> = None;
        // F3 / Shift+F3 cycle find matches (edit.find_next / edit.find_prev).
        // Detected here, applied after the closure so it can drive the editor.
        let mut pending_find: Option<bool> = None;
        // Shift+Alt+Right / Shift+Alt+Left grow or shrink the selection to the
        // next LSP semantic scope (editor.select_expand / editor.select_shrink).
        let mut pending_select: Option<bool> = None;
        // The completion popup owns Tab/Enter/Up/Down/Escape while it is open.
        // Encoded as: 0 = next, 1 = prev, 2 = commit (Tab), 3 = commit (Enter),
        // 4 = dismiss. Detected in the closure, applied after so the chord can
        // be consumed before egui's TextEdit sees it.
        let mut pending_completion: Option<u8> = None;
        // Whether the editor's live selection spans more than one line. Plain
        // Tab only indents a block in that case — so a lone caret still lets
        // egui insert the indent character (and Tab-accept an inline
        // suggestion) — while Shift+Tab always dedents.
        let block_selection_present = editor_focused
            && self.active_tab.as_ref().is_some_and(|id| {
                let eid = crate::editor::code_editor::CodeEditor::textedit_id(id);
                let content = self
                    .buffers
                    .get(id)
                    .map(|b| b.content().to_owned())
                    .unwrap_or_default();
                egui::widgets::text_edit::TextEditState::load(ctx, eid)
                    .and_then(|s| s.cursor.char_range())
                    .map(|r| {
                        let a: usize = r.primary.index.into();
                        let b: usize = r.secondary.index.into();
                        let (f, l) = crate::editor::line_ops::line_range_of(&content, a, b);
                        f != l
                    })
                    .unwrap_or(false)
            });
        ctx.input(|i| {
            // Custom keybinding intercept: honor a user's remaps and additions
            // from `.velocity/keybindings.json` ahead of the built-in chain, so
            // a rebound or newly-added chord wins. Stock chords are ignored here
            // (`custom_command_for` returns `None` for them) and fall through to
            // the handlers below, so a default install behaves exactly as before.
            if let Some(cmd) = self.matched_custom_command(i) {
                if self.dispatch_keybinding(ctx, &cmd) {
                    return;
                }
            }
            // The completion popup is modal while visible: Up/Down walk the
            // list, Tab/Enter commit, Escape dismisses. Checked before the
            // editor-focus chords so a shown popup always beats indent and
            // inline-suggestion Tab handling.
            if self.completion_state.active && !self.completion_state.filtered.is_empty() {
                if i.key_pressed(egui::Key::ArrowDown) {
                    pending_completion = Some(0);
                    return;
                } else if i.key_pressed(egui::Key::ArrowUp) {
                    pending_completion = Some(1);
                    return;
                } else if i.key_pressed(egui::Key::Tab) {
                    pending_completion = Some(2);
                    return;
                } else if i.key_pressed(egui::Key::Enter) {
                    pending_completion = Some(3);
                    return;
                } else if i.key_pressed(egui::Key::Escape) {
                    pending_completion = Some(4);
                    return;
                }
            }
            // Stock whole-line editing chords, ahead of the general chain so an
            // editor-focused Ctrl+Shift+K deletes the line (matching the keymap)
            // instead of the global disk-hygiene binding further down.
            if editor_focused {
                let m = i.modifiers;
                let chord = if m.command && m.shift && i.key_pressed(egui::Key::D) {
                    Some((
                        crate::editor::line_ops::LineOp::Duplicate,
                        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                        egui::Key::D,
                    ))
                } else if m.command && m.shift && i.key_pressed(egui::Key::K) {
                    Some((
                        crate::editor::line_ops::LineOp::Delete,
                        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                        egui::Key::K,
                    ))
                } else if m.alt && i.key_pressed(egui::Key::ArrowUp) {
                    Some((
                        crate::editor::line_ops::LineOp::MoveUp,
                        egui::Modifiers::ALT,
                        egui::Key::ArrowUp,
                    ))
                } else if m.alt && i.key_pressed(egui::Key::ArrowDown) {
                    Some((
                        crate::editor::line_ops::LineOp::MoveDown,
                        egui::Modifiers::ALT,
                        egui::Key::ArrowDown,
                    ))
                } else {
                    None
                };
                if let Some(op) = chord {
                    pending_line_op = Some(op);
                    return;
                }
                if m.command && i.key_pressed(egui::Key::Slash) {
                    pending_comment = Some((egui::Modifiers::COMMAND, egui::Key::Slash));
                    return;
                }
                // Smart selection: grow (Right) / shrink (Left) to the next LSP
                // scope. Checked inside the editor-focus gate and ahead of the
                // general Alt+arrows navigation so Shift+Alt doesn't fall through
                // to nav_back/nav_forward.
                if m.alt && m.shift && i.key_pressed(egui::Key::ArrowRight) {
                    pending_select = Some(true);
                    return;
                } else if m.alt && m.shift && i.key_pressed(egui::Key::ArrowLeft) {
                    pending_select = Some(false);
                    return;
                }
                if i.key_pressed(egui::Key::Tab) {
                    if m.shift {
                        pending_indent = Some((false, egui::Modifiers::SHIFT, egui::Key::Tab));
                        return;
                    } else if block_selection_present {
                        pending_indent = Some((true, egui::Modifiers::NONE, egui::Key::Tab));
                        return;
                    }
                }
            }
            let cmd = i.modifiers.command;
            let shift = i.modifiers.shift;
            let inline_active = self.inline_suggestions.state
                == crate::editor::inline_suggestions::SuggestionState::Showing;
            if inline_active && i.key_pressed(egui::Key::Tab) {
                accept_inline = true;
            } else if inline_active && i.key_pressed(egui::Key::Escape) {
                self.inline_suggestions.dismiss();
            } else if i.key_pressed(egui::Key::F1) {
                self.show_shortcuts = !self.show_shortcuts;
            } else if i.key_pressed(egui::Key::F3) {
                // Shift+F3 → previous match; plain F3 → next match.
                pending_find = Some(!shift);
            } else if cmd && shift && i.key_pressed(egui::Key::P) {
                self.open_command_palette();
            } else if cmd && i.key_pressed(egui::Key::P) {
                // Ctrl+P opens the command palette (alias for Ctrl+Shift+P).
                self.open_command_palette();
            } else if cmd && shift && i.key_pressed(egui::Key::T) {
                self.reopen_closed_tab();
            } else if cmd && i.key_pressed(egui::Key::G) {
                self.open_goto_line();
            } else if cmd && shift && i.key_pressed(egui::Key::O) {
                self.open_goto_symbol();
            } else if cmd && shift && i.key_pressed(egui::Key::W) {
                // Open workspace switcher (Ctrl+Shift+W).
                self.workspace_switcher_open = !self.workspace_switcher_open;
                self.workspace_switcher_selected = 0;
                self.workspace_switcher_just_opened = true;
            } else if i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft) {
                self.nav_back();
            } else if i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight) {
                self.nav_forward();
            } else if cmd && i.key_pressed(egui::Key::N) {
                self.open_editor(None);
            } else if cmd && i.key_pressed(egui::Key::O) {
                self.open_file_dialog();
            } else if cmd && shift && i.key_pressed(egui::Key::S) {
                self.save_all();
            } else if cmd && i.key_pressed(egui::Key::S) {
                self.save_active();
            } else if cmd && shift && i.key_pressed(egui::Key::B) {
                self.toggle_bookmark_current_line();
            } else if cmd && i.key_pressed(egui::Key::B) {
                self.build_active();
            } else if cmd && i.key_pressed(egui::Key::R) {
                self.run_active();
            } else if cmd && i.key_pressed(egui::Key::W) {
                self.close_active_tab();
            } else if cmd && i.key_pressed(egui::Key::Backslash) {
                // Split editor view (Ctrl+\).
                self.split_editor();
            } else if cmd && !i.modifiers.alt && i.key_pressed(egui::Key::J) {
                self.toggle_panel(TabKind::Chat);
            } else if cmd && i.key_pressed(egui::Key::Backtick) {
                // Toggle bottom panel (Terminal)
                self.bottom_panel_state.collapsed = !self.bottom_panel_state.collapsed;
                if !self.bottom_panel_state.collapsed {
                    self.bottom_panel_state.active_tab = crate::editor::bottom_panel::TAB_TERMINAL;
                }
            } else if cmd && i.key_pressed(egui::Key::E) {
                self.toggle_left_sidebar();
            } else if cmd && shift && i.key_pressed(egui::Key::E) {
                self.toggle_right_sidebar();
            } else if cmd && i.key_pressed(egui::Key::PageDown) {
                self.cycle_tabs(1);
            } else if cmd && i.key_pressed(egui::Key::PageUp) {
                self.cycle_tabs(-1);
            } else if cmd && i.key_pressed(egui::Key::Num1) {
                self.set_work_mode(crate::editor::theme::WorkspaceProfile::Coder);
            } else if cmd && i.key_pressed(egui::Key::Num2) {
                self.set_work_mode(crate::editor::theme::WorkspaceProfile::AutomationOperator);
            } else if cmd && i.key_pressed(egui::Key::Num3) {
                self.set_work_mode(crate::editor::theme::WorkspaceProfile::MissionControl);
            } else if cmd && i.key_pressed(egui::Key::Num4) {
                self.set_work_mode(crate::editor::theme::WorkspaceProfile::Accessibility);
            }
            // --- Panel toggle shortcuts ---
            else if cmd && shift && i.key_pressed(egui::Key::Y) {
                self.toggle_orchestrator();
            } else if cmd && shift && i.key_pressed(egui::Key::F) {
                self.toggle_search();
            } else if cmd && shift && i.key_pressed(egui::Key::K) {
                // Disk hygiene overlay (opens read-only; deletion needs the
                // overlay's own confirm click).
                self.open_disk_hygiene();
            } else if cmd && i.key_pressed(egui::Key::Comma) {
                self.toggle_settings();
            } else if cmd && shift && i.key_pressed(egui::Key::I) {
                self.request_inline_suggestion();
            } else if cmd && shift && i.key_pressed(egui::Key::X) {
                self.toggle_extensions();
            } else if cmd && shift && i.key_pressed(egui::Key::A) {
                self.toggle_activity();
            } else if cmd && shift && i.key_pressed(egui::Key::V) {
                self.toggle_voice();
            } else if cmd && shift && i.key_pressed(egui::Key::G) {
                self.open_branch_switcher();
            } else if cmd && i.modifiers.alt && i.key_pressed(egui::Key::R) {
                self.rollback_deploy();
            } else if cmd && i.modifiers.alt && i.key_pressed(egui::Key::J) {
                // Go to next/previous unsaved change: queued so the shortcut
                // pass (which owns `ctx`) re-aims the caret.
                self.queued_change_jump = Some(true);
            } else if cmd && i.modifiers.alt && i.key_pressed(egui::Key::K) {
                self.queued_change_jump = Some(false);
            }
            // --- IDE Editor Shortcuts ---
            else if i.modifiers.alt && shift && i.key_pressed(egui::Key::F) {
                // Format document (LSP textDocument/formatting)
                self.format_document_via_lsp();
            } else if i.key_pressed(egui::Key::F2) {
                // Rename symbol (LSP textDocument/rename) — opens the input overlay
                self.open_rename_overlay();
            } else if i.modifiers.alt && i.key_pressed(egui::Key::Enter) {
                // Code actions / quick fixes (LSP textDocument/codeAction)
                self.open_code_actions_overlay();
            } else if cmd && i.key_pressed(egui::Key::F) {
                // Find — contextual: when the terminal grid is the visible
                // bottom panel and no editor owns focus, the query targets
                // the terminal output; otherwise the active buffer's bar.
                let terminal_shows_find = !self.bottom_panel_state.collapsed
                    && self.bottom_panel_state.active_tab
                        == crate::editor::bottom_panel::TAB_TERMINAL
                    && !editor_focused;
                if terminal_shows_find {
                    self.terminal_state.open_find();
                } else {
                    self.open_find_active();
                }
            } else if cmd && i.key_pressed(egui::Key::H) {
                // Find & Replace
                self.open_find_replace_active();
            } else if cmd && i.key_pressed(egui::Key::Z) && shift {
                // Redo
                self.redo_active();
            } else if cmd && i.key_pressed(egui::Key::Z) {
                // Undo
                self.undo_active();
            } else if i.key_pressed(egui::Key::Escape) {
                // Close find/replace if open
                if let Some(id) = &self.active_tab {
                    if let Some(buf) = self.buffers.get_mut(id) {
                        if buf.find_replace.visible {
                            buf.find_replace.close();
                        }
                    }
                }
            } else if shift && i.key_pressed(egui::Key::F5) {
                // Shift+F5: stop the debug session (checked before plain F5).
                self.debug_stop();
            } else if i.key_pressed(egui::Key::F5) {
                // F5: start or continue debugging.
                self.debug_start_or_continue();
            } else if shift && i.key_pressed(egui::Key::F8) {
                // Go to previous problem: queued so the shortcut pass re-aims
                // the caret like the change jumps above.
                self.queued_problem_jump = Some(false);
            } else if i.key_pressed(egui::Key::F8) {
                self.queued_problem_jump = Some(true);
            } else if i.key_pressed(egui::Key::F9) {
                // Toggle breakpoint at current line
                self.toggle_breakpoint_current_line();
            } else if i.key_pressed(egui::Key::F10) {
                // Step over
                self.debug_step_over();
            } else if shift && i.key_pressed(egui::Key::F11) {
                // Shift+F11: step out (checked before plain F11).
                self.debug_step_out();
            } else if i.key_pressed(egui::Key::F11) {
                // Step into
                self.debug_step_into();
            } else if shift && i.key_pressed(egui::Key::F12) {
                // Find all references (LSP)
                self.find_references_at_cursor();
            } else if i.key_pressed(egui::Key::F12) {
                // Go to definition (LSP)
                self.goto_definition_at_cursor();
            } else if cmd && shift && i.key_pressed(egui::Key::Space) {
                // Ctrl+Shift+Space: manually (re)invoke parameter hints.
                self.request_signature_help(None);
            } else if cmd && i.key_pressed(egui::Key::Space) {
                // Trigger completion (applied next frame by completion_tick so
                // the popup sees an up-to-date caret/buffer, same as '.' auto).
                self.completion_queued_trigger = true;
            } else if editor_focused && i.key_pressed(egui::Key::Period) {
                // Competitor behavior: typing '.' (member access) pops the
                // completion list on the following frame, after TextEdit has
                // inserted the '.' — nothing is consumed, typing continues.
                self.completion_queued_trigger = true;
            } else if cmd && shift && i.key_pressed(egui::Key::M) {
                // Toggle minimap
                self.show_minimap = !self.show_minimap;
            } else if i.modifiers.alt && i.key_pressed(egui::Key::Z) {
                // Toggle word wrap
                self.word_wrap = !self.word_wrap;
            }
        });
        if let Some((op, mods, key)) = pending_line_op {
            // Drop the chord from this frame's events so the editor widget does
            // not additionally move the caret, then perform the line operation.
            ctx.input_mut(|i| {
                i.consume_key(mods, key);
            });
            self.apply_line_op(ctx, op);
        }
        if let Some((mods, key)) = pending_comment {
            ctx.input_mut(|i| {
                i.consume_key(mods, key);
            });
            self.toggle_line_comment(ctx);
        }
        if let Some((indent, mods, key)) = pending_indent {
            ctx.input_mut(|i| {
                i.consume_key(mods, key);
            });
            self.apply_indent(ctx, indent);
        }
        if let Some(forward) = pending_find {
            self.find_goto_match(ctx, forward);
        }
        if let Some(expand) = pending_select {
            ctx.input_mut(|i| {
                i.consume_key(
                    egui::Modifiers::ALT | egui::Modifiers::SHIFT,
                    if expand {
                        egui::Key::ArrowRight
                    } else {
                        egui::Key::ArrowLeft
                    },
                );
            });
            self.select_surrounding_item(ctx, expand);
        }
        if let Some(action) = pending_completion {
            ctx.input_mut(|i| {
                i.consume_key(
                    egui::Modifiers::NONE,
                    match action {
                        0 => egui::Key::ArrowDown,
                        1 => egui::Key::ArrowUp,
                        2 => egui::Key::Tab,
                        3 => egui::Key::Enter,
                        _ => egui::Key::Escape,
                    },
                );
            });
            match action {
                0 => self.completion_move(true),
                1 => self.completion_move(false),
                2 | 3 => self.commit_selected_completion(ctx),
                _ => self.completion_state.close(),
            }
        }
        if accept_inline {
            self.accept_inline_suggestion(ctx);
        }
    }

    /// Scan this frame's pressed keys for a chord the user has customized
    /// (added or rebound) in `keybindings.json`. Returns the command id, if
    /// any. Only global-scope chords (no `when` clause) are considered; a stock
    /// chord that still maps to its default command yields `None`.
    fn matched_custom_command(&self, i: &egui::InputState) -> Option<String> {
        if self.keybindings_config.bindings.is_empty() {
            return None;
        }
        // Fast path: no key was pressed this frame at all.
        if !egui::Key::ALL.iter().any(|k| i.key_pressed(*k)) {
            return None;
        }
        let mods = i.modifiers;
        for key in egui::Key::ALL.iter().copied() {
            if !i.key_pressed(key) {
                continue;
            }
            let chord = crate::editor::keybindings::KeyBinding::from_egui(key, &mods);
            if let Some(cmd) = self.keybindings_config.custom_command_for(&chord) {
                return Some(cmd);
            }
        }
        None
    }

    /// Run the editor action behind a keybinding command id. Returns `true`
    /// when the id maps to a handler here (so the caller suppresses the
    /// built-in chain) and `false` for ids it does not recognize, letting the
    /// stock key handling proceed untouched.
    fn dispatch_keybinding(&mut self, ctx: &egui::Context, cmd: &str) -> bool {
        match cmd {
            // File
            "file.new" => self.open_editor(None),
            "file.open" => self.open_file_dialog(),
            "file.save" => self.save_active(),
            "file.save_all" => self.save_all(),
            "file.close" => self.close_active_tab(),
            "file.quick_open" => self.open_quick_open(),
            // View / layout
            "view.command_palette" => self.open_command_palette(),
            "view.toggle_sidebar" => self.toggle_left_sidebar(),
            "view.toggle_search" => self.toggle_search(),
            "view.toggle_settings" => self.toggle_settings(),
            "view.toggle_chat" => self.toggle_panel(TabKind::Chat),
            "view.toggle_orchestrator" => self.toggle_orchestrator(),
            "view.toggle_extensions" => self.toggle_extensions(),
            "view.toggle_activity" => self.toggle_activity(),
            "view.toggle_voice" => self.toggle_voice(),
            "view.split_editor" => self.split_editor(),
            "view.toggle_minimap" => self.show_minimap = !self.show_minimap,
            "view.word_wrap" => self.word_wrap = !self.word_wrap,
            "view.toggle_auto_indent" => {
                self.auto_indent_enabled = !self.auto_indent_enabled;
                self.save_workspace_preferences();
                self.status_message = format!(
                    "Auto-indent {}",
                    if self.auto_indent_enabled {
                        "on"
                    } else {
                        "off"
                    }
                );
            }
            "view.toggle_auto_close_brackets" => {
                self.auto_close_brackets_enabled = !self.auto_close_brackets_enabled;
                self.save_workspace_preferences();
                self.status_message = format!(
                    "Auto-close brackets {}",
                    if self.auto_close_brackets_enabled {
                        "on"
                    } else {
                        "off"
                    }
                );
            }
            "view.toggle_auto_save" => self.toggle_auto_save(),
            "edit.toggle_format_on_save" => self.toggle_format_on_save(),
            "edit.toggle_trim_trailing_ws" => self.toggle_trim_trailing_ws(),
            "view.toggle_terminal" => {
                self.bottom_panel_state.collapsed = !self.bottom_panel_state.collapsed;
                if !self.bottom_panel_state.collapsed {
                    self.bottom_panel_state.active_tab = crate::editor::bottom_panel::TAB_TERMINAL;
                }
            }
            // Reveal the terminal (if hidden) and open its find bar.
            "terminal.find" => {
                self.bottom_panel_state.collapsed = false;
                self.bottom_panel_state.active_tab = crate::editor::bottom_panel::TAB_TERMINAL;
                self.terminal_state.open_find();
            }
            // Navigation
            "nav.goto_line" => self.open_goto_line(),
            "nav.goto_symbol" => self.open_goto_symbol(),
            "nav.back" => self.nav_back(),
            "nav.forward" => self.nav_forward(),
            "nav.next_tab" => self.cycle_tabs(1),
            "nav.prev_tab" => self.cycle_tabs(-1),
            "nav.goto_definition" => self.goto_definition_at_cursor(),
            "nav.goto_declaration" => self.goto_declaration_at_cursor(),
            "nav.goto_type_definition" => self.goto_type_definition_at_cursor(),
            "nav.goto_implementation" => self.goto_implementation_at_cursor(),
            "nav.find_references" => self.find_references_at_cursor(),
            "nav.incoming_calls" => self.show_call_hierarchy(true),
            "nav.outgoing_calls" => self.show_call_hierarchy(false),
            // LSP refactoring engine
            "editor.rename_symbol" => self.open_rename_overlay(),
            "editor.code_actions" => self.open_code_actions_overlay(),
            "editor.format_document" => self.format_document_via_lsp(),
            "editor.signature_help" => self.request_signature_help(None),
            "editor.toggle_bookmark" => self.toggle_bookmark_current_line(),
            // Whole-line editing (so a user can rebind these off their defaults)
            "edit.duplicate_line" => {
                self.apply_line_op(ctx, crate::editor::line_ops::LineOp::Duplicate)
            }
            "edit.delete_line" => self.apply_line_op(ctx, crate::editor::line_ops::LineOp::Delete),
            "edit.move_line_up" => self.apply_line_op(ctx, crate::editor::line_ops::LineOp::MoveUp),
            "edit.move_line_down" => {
                self.apply_line_op(ctx, crate::editor::line_ops::LineOp::MoveDown)
            }
            "edit.toggle_comment" => self.toggle_line_comment(ctx),
            "edit.indent" => self.apply_indent(ctx, true),
            "edit.dedent" => self.apply_indent(ctx, false),
            "edit.find_next" => self.find_goto_match(ctx, true),
            "edit.find_prev" => self.find_goto_match(ctx, false),
            // Go to next/previous unsaved change (rebindable form of the
            // Ctrl+Alt+J / Ctrl+Alt+K chords above).
            "edit.next_change" => self.goto_change(ctx, true),
            "edit.prev_change" => self.goto_change(ctx, false),
            // Go to next/previous problem (rebindable form of F8 / Shift+F8).
            "edit.next_problem" => self.goto_problem(true),
            "edit.prev_problem" => self.goto_problem(false),
            // Smart (scope) selection, so a user can rebind off the defaults.
            "editor.select_expand" => self.select_surrounding_item(ctx, true),
            "editor.select_shrink" => self.select_surrounding_item(ctx, false),
            // Find widget + history (so a user can rebind these off their defaults)
            "edit.find" => self.open_find_active(),
            "edit.replace" => self.open_find_replace_active(),
            "edit.undo" => self.undo_active(),
            "edit.redo" => self.redo_active(),
            // Build / run
            "build.build" => self.build_active(),
            "build.run" => self.run_active(),
            "build.rollback_deploy" => self.rollback_deploy(),
            "build.show_nodes" => self.open_nodes_panel(),
            // Agent
            "agent.request_inline_suggestion" => self.request_inline_suggestion(),
            "completion.trigger" => self.completion_queued_trigger = true,
            // Editor font zoom
            "editor.zoom_in" => self.adjust_code_scale(0.1),
            "editor.zoom_out" => self.adjust_code_scale(-0.1),
            "editor.zoom_reset" => self.reset_code_scale(),
            // Debug
            "debug.toggle_breakpoint" => self.toggle_breakpoint_current_line(),
            "debug.start" => self.debug_start_or_continue(),
            "debug.continue" => self.debug_start_or_continue(),
            "debug.stop" => self.debug_stop(),
            "debug.step_over" => self.debug_step_over(),
            "debug.step_into" => self.debug_step_into(),
            "debug.step_out" => self.debug_step_out(),
            // Git
            "git.switch_branch" => self.open_branch_switcher(),
            "mode.coder" => self.set_work_mode(crate::editor::theme::WorkspaceProfile::Coder),
            "mode.operator" => {
                self.set_work_mode(crate::editor::theme::WorkspaceProfile::AutomationOperator)
            }
            "mode.mission" => {
                self.set_work_mode(crate::editor::theme::WorkspaceProfile::MissionControl)
            }
            "mode.accessibility" => {
                self.set_work_mode(crate::editor::theme::WorkspaceProfile::Accessibility)
            }
            _ => return false,
        }
        true
    }
}

/// Every command id [`VelocityApp::dispatch_keybinding`] recognizes. Kept
/// directly beneath that match so a new handler and this list are updated
/// together; the regression test asserts each advertised default keybinding
/// appears here, so a shortcut can never again ship wired to nothing (the
/// class of drift that `view.fold*` fell into).
#[cfg(test)]
pub(crate) const DISPATCH_HANDLED_COMMANDS: &[&str] = &[
    // File
    "file.new",
    "file.open",
    "file.save",
    "file.save_all",
    "file.close",
    "file.quick_open",
    // View / layout
    "view.command_palette",
    "view.toggle_sidebar",
    "view.toggle_search",
    "view.toggle_settings",
    "view.toggle_chat",
    "view.toggle_orchestrator",
    "view.toggle_extensions",
    "view.toggle_activity",
    "view.toggle_voice",
    "view.split_editor",
    "view.toggle_minimap",
    "view.word_wrap",
    "view.toggle_auto_indent",
    "view.toggle_auto_close_brackets",
    "view.toggle_auto_save",
    "edit.toggle_format_on_save",
    "edit.toggle_trim_trailing_ws",
    "view.toggle_terminal",
    "terminal.find",
    // Navigation
    "nav.goto_line",
    "nav.goto_symbol",
    "nav.back",
    "nav.forward",
    "nav.next_tab",
    "nav.prev_tab",
    "nav.goto_definition",
    "nav.goto_declaration",
    "nav.goto_type_definition",
    "nav.goto_implementation",
    "nav.find_references",
    "nav.incoming_calls",
    "nav.outgoing_calls",
    // LSP refactoring engine
    "editor.rename_symbol",
    "editor.code_actions",
    "editor.format_document",
    "editor.signature_help",
    "editor.toggle_bookmark",
    // Whole-line editing + find widget + history
    "edit.duplicate_line",
    "edit.delete_line",
    "edit.move_line_up",
    "edit.move_line_down",
    "edit.toggle_comment",
    "edit.indent",
    "edit.dedent",
    "edit.find_next",
    "edit.find_prev",
    "edit.next_change",
    "edit.prev_change",
    "edit.next_problem",
    "edit.prev_problem",
    "edit.find",
    "edit.replace",
    "edit.undo",
    "edit.redo",
    // Smart (scope) selection
    "editor.select_expand",
    "editor.select_shrink",
    // Build / run
    "build.build",
    "build.run",
    "build.rollback_deploy",
    "build.show_nodes",
    // Agent
    "agent.request_inline_suggestion",
    "completion.trigger",
    // Editor font zoom
    "editor.zoom_in",
    "editor.zoom_out",
    "editor.zoom_reset",
    // Debug
    "debug.toggle_breakpoint",
    "debug.start",
    "debug.continue",
    "debug.stop",
    "debug.step_over",
    "debug.step_into",
    "debug.step_out",
    // Git
    "git.switch_branch",
    // Workspace modes
    "mode.coder",
    "mode.operator",
    "mode.mission",
    "mode.accessibility",
];

#[cfg(test)]
mod keybinding_invariant_tests {
    use super::DISPATCH_HANDLED_COMMANDS;

    /// No advertised default keybinding may point at a command the editor does
    /// not actually dispatch. If someone adds a `entry("x.y", ..)` to
    /// `KeybindingsConfig::defaults()` without wiring a `dispatch_keybinding`
    /// arm (and listing it), this test fails instead of shipping a dead chord.
    #[test]
    fn every_default_keybinding_is_dispatchable() {
        let cfg = crate::editor::keybindings::KeybindingsConfig::defaults();
        for entry in &cfg.bindings {
            assert!(
                DISPATCH_HANDLED_COMMANDS.contains(&entry.command.as_str()),
                "default keybinding {:?} (chord {:?}) has no dispatch handler — \
                 wire it in dispatch_keybinding or stop advertising it",
                entry.command,
                entry.binding.display()
            );
        }
    }

    /// The registry itself must be free of duplicate ids, so the list stays a
    /// faithful mirror of the dispatch match.
    #[test]
    fn dispatch_registry_has_no_duplicates() {
        let mut sorted = DISPATCH_HANDLED_COMMANDS.to_vec();
        sorted.sort_unstable();
        let n = sorted.len();
        sorted.dedup();
        assert_eq!(sorted.len(), n, "DISPATCH_HANDLED_COMMANDS has a duplicate");
    }
}
