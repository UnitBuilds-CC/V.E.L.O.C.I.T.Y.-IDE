//! Global keyboard shortcuts rendering for `VelocityApp`.
//!
//! Extracted verbatim from `ui_render.rs` (no logic changes).
use super::super::types::*;
use super::struct_def::VelocityApp;
use eframe::egui;

impl VelocityApp {
    pub fn handle_global_shortcuts(&mut self, ctx: &egui::Context) {
        if self.command_palette.open
            || self.quick_open.open
            || self.goto_line_open
            || self.goto_symbol_open
            || self.mru.open
        {
            return;
        }
        // Whether the user pressed Tab while an inline suggestion is showing.
        // Detected inside the input closure (which borrows `ctx`) and acted on
        // afterwards so we can pass `ctx` to the accept routine.
        let mut accept_inline = false;
        ctx.input(|i| {
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
            } else if cmd && shift && i.key_pressed(egui::Key::P) {
                self.open_command_palette();
            } else if cmd && i.key_pressed(egui::Key::P) {
                self.open_quick_open();
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
            } else if cmd && i.key_pressed(egui::Key::B) {
                self.build_active();
            } else if cmd && i.key_pressed(egui::Key::R) {
                self.run_active();
            } else if cmd && i.key_pressed(egui::Key::W) {
                self.close_active_tab();
            } else if cmd && i.key_pressed(egui::Key::Backslash) {
                // Split editor view (Ctrl+\).
                self.split_editor();
            } else if cmd && i.key_pressed(egui::Key::J) {
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
            } else if cmd && i.modifiers.alt && i.key_pressed(egui::Key::R) {
                self.rollback_deploy();
            }
            // --- IDE Editor Shortcuts ---
            else if cmd && i.key_pressed(egui::Key::F) {
                // Find in current buffer
                if let Some(id) = &self.active_tab {
                    if let Some(buf) = self.buffers.get_mut(id) {
                        buf.find_replace.open_find();
                    }
                }
            } else if cmd && i.key_pressed(egui::Key::H) {
                // Find & Replace
                if let Some(id) = &self.active_tab {
                    if let Some(buf) = self.buffers.get_mut(id) {
                        buf.find_replace.open_find_replace();
                    }
                }
            } else if cmd && i.key_pressed(egui::Key::Z) && shift {
                // Redo
                if let Some(id) = &self.active_tab {
                    if let Some(buf) = self.buffers.get_mut(id) {
                        buf.redo();
                    }
                }
            } else if cmd && i.key_pressed(egui::Key::Z) {
                // Undo
                if let Some(id) = &self.active_tab {
                    if let Some(buf) = self.buffers.get_mut(id) {
                        buf.undo();
                    }
                }
            } else if i.key_pressed(egui::Key::Escape) {
                // Close find/replace if open
                if let Some(id) = &self.active_tab {
                    if let Some(buf) = self.buffers.get_mut(id) {
                        if buf.find_replace.visible {
                            buf.find_replace.close();
                        }
                    }
                }
            } else if i.key_pressed(egui::Key::F5) {
                // Start/Continue debugging
                if let Some(dap) = &mut self.dap_client {
                    let _ = dap.continue_execution();
                } else {
                    // Launch a new debug session
                    self.launch_debug_session();
                }
            } else if i.key_pressed(egui::Key::F9) {
                // Toggle breakpoint at current line
                self.toggle_breakpoint_current_line();
            } else if i.key_pressed(egui::Key::F10) {
                // Step over
                if let Some(dap) = &mut self.dap_client {
                    let _ = dap.step_over();
                }
            } else if i.key_pressed(egui::Key::F11) {
                // Step into
                if let Some(dap) = &mut self.dap_client {
                    let _ = dap.step_into();
                }
            } else if shift && i.key_pressed(egui::Key::F12) {
                // Find all references (LSP)
                self.find_references_at_cursor();
            } else if i.key_pressed(egui::Key::F12) {
                // Go to definition (LSP)
                self.goto_definition_at_cursor();
            } else if cmd && i.key_pressed(egui::Key::Space) {
                // Trigger completion
                self.trigger_completion();
            } else if cmd && shift && i.key_pressed(egui::Key::M) {
                // Toggle minimap
                self.show_minimap = !self.show_minimap;
            } else if i.modifiers.alt && i.key_pressed(egui::Key::Z) {
                // Toggle word wrap
                self.word_wrap = !self.word_wrap;
            }
        });
        if accept_inline {
            self.accept_inline_suggestion(ctx);
        }
    }
}
