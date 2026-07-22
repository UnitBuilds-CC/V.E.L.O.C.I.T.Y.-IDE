use std::path::PathBuf;
use eframe::egui;

use crate::editor::agent_ui_render::{render_agent_metrics, render_pending_approvals, render_thinking_panel, RenderSnapshot};
use crate::editor::task_timeline::render_task_timeline;
use crate::editor::theme::IdePalette;

use super::super::helpers::*;
use super::super::render::TabViewerImpl;
use super::super::types::*;
use super::struct_def::VelocityApp;

impl VelocityApp {
    pub fn search_panel(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.heading("🔍 Search Workspace");
                    ui.horizontal(|ui| {
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.search_query)
                                .hint_text("Search query...")
                                .desired_width(ui.available_width() - 80.0),
                        );
                        if response.changed() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            self.search_hits = crate::editor::search::project_search(
                                &self.workspace_root,
                                &self.search_query,
                                100,
                            );
                        }
                        if ui.button("Search").clicked() {
                            self.search_hits = crate::editor::search::project_search(
                                &self.workspace_root,
                                &self.search_query,
                                100,
                            );
                        }
                    });
                    ui.separator();

                    let hits = self.search_hits.clone();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        if hits.is_empty() {
                            if self.search_query.is_empty() {
                                ui.label("Type a query to search files.");
                            } else {
                                ui.label("No results found.");
                            }
                        } else {
                            for hit in &hits {
                                let icon = crate::editor::search::icon_for_path(&hit.path);
                                let title =
                                    format!("{} {} : line {}", icon, hit.path.display(), hit.line);
                                ui.group(|ui| {
                                    ui.horizontal(|ui| {
                                        if ui.link(title).clicked() {
                                            let abs_path = self.workspace_root.join(&hit.path);
                                            self.open_editor(Some(abs_path));
                                            self.pending_cursor_line = Some(hit.line);
                                        }
                                    });
                                    ui.label(egui::RichText::new(&hit.text).monospace().size(12.0));
                                });
                            }
                        }
                    });
                });
            });
    }

    pub fn handle_global_shortcuts(&mut self, ctx: &egui::Context) {
        if self.command_palette.open {
            return;
        }
        ctx.input(|i| {
            let cmd = i.modifiers.command;
            let shift = i.modifiers.shift;
            if cmd && shift && i.key_pressed(egui::Key::P) {
                self.open_command_palette();
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
            }
        });
    }

    pub fn command_palette_ui(&mut self, ctx: &egui::Context) {
        if !self.command_palette.open {
            return;
        }

        let area = egui::Area::new(egui::Id::new("command_palette_area"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_TOP, egui::Vec2::new(0.0, 80.0));

        let commands = self.command_list_filtered();
        let mut open = self.command_palette.open;

        self.command_palette.selected = self
            .command_palette
            .selected
            .min(commands.len().saturating_sub(1));

        area.show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(ui.visuals().code_bg_color)
                .stroke(ui.visuals().window_stroke)
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.set_width(480.0);
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.command_palette.query)
                            .hint_text("Type a command…")
                            .desired_width(480.0),
                    );
                    if response.changed() {
                        self.command_palette.selected = 0;
                    }
                    ui.separator();

                    egui::ScrollArea::vertical()
                        .max_height(300.0)
                        .show(ui, |ui| {
                            for (idx, cmd) in commands.iter().enumerate() {
                                let selected = idx == self.command_palette.selected;
                                let text = egui::RichText::new(cmd.label).color(if selected {
                                    ui.visuals().selection.stroke.color
                                } else {
                                    ui.visuals().text_color()
                                });
                                if ui.selectable_label(selected, text).clicked() {
                                    (cmd.action)(self);
                                    self.command_palette.open = false;
                                }
                            }
                        });

                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        if let Some(cmd) = commands.get(self.command_palette.selected) {
                            let action = cmd.action;
                            action(self);
                        }
                        self.command_palette.open = false;
                    } else if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                        if !commands.is_empty() {
                            self.command_palette.selected =
                                (self.command_palette.selected + 1) % commands.len();
                        }
                    } else if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                        if !commands.is_empty() {
                            self.command_palette.selected = self
                                .command_palette
                                .selected
                                .checked_sub(1)
                                .unwrap_or(commands.len() - 1);
                        }
                    } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        open = false;
                    }
                });
        });

        self.command_palette.open = open;
    }

    pub fn file_dialog_ui(&mut self, ctx: &egui::Context) {
        let mut open = self.pending_open_path.is_some();
        if !open {
            return;
        }
        let mut path_string = self
            .pending_open_path
            .as_ref()
            .and_then(|p| p.to_str())
            .map(String::from)
            .unwrap_or_default();

        egui::Window::new("Open File")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("File path (relative to workspace):");
                ui.text_edit_singleline(&mut path_string);
                ui.horizontal(|ui| {
                    if ui.button("Open").clicked() {
                        let p = self.workspace_root.join(&path_string);
                        if p.exists() && p.is_file() {
                            self.open_editor(Some(p));
                            self.pending_open_path = None;
                        } else {
                            self.status_message = format!("File not found: {}", p.display());
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.pending_open_path = None;
                    }
                });
            });
        if !open {
            self.pending_open_path = None;
        }
    }

    pub fn save_as_dialog_ui(&mut self, ctx: &egui::Context) {
        let mut open = self.pending_save_as_path.is_some();
        if !open {
            return;
        }
        let mut path_string = self
            .pending_save_as_path
            .as_ref()
            .and_then(|p| p.to_str())
            .map(String::from)
            .unwrap_or_default();

        egui::Window::new("Save As")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("File path (relative to workspace):");
                ui.text_edit_singleline(&mut path_string);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        if let Some(id) = self.active_tab.clone() {
                            let p = self.workspace_root.join(&path_string);
                            self.save_buffer_to(&id, &p);
                            if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
                                if let TabKind::Editor { ref mut path, .. } = tab.kind {
                                    *path = Some(p);
                                }
                            }
                            self.pending_save_as_path = None;
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.pending_save_as_path = None;
                    }
                });
            });
        if !open {
            self.pending_save_as_path = None;
        }
    }

    pub fn full_diff_ui(&mut self, ctx: &egui::Context) {
        if !self.show_full_diff {
            return;
        }
        let mut open = self.show_full_diff;
        let active_change_preview = self.active_change_preview();
        egui::Window::new("Full Diff")
            .open(&mut open)
            .resizable(true)
            .default_size(egui::vec2(720.0, 520.0))
            .show(ctx, |ui| {
                if let Some(change_preview) = &active_change_preview {
                    ui.label(
                        egui::RichText::new(format!(
                            "{}  (+{} / -{})",
                            change_preview.file_label,
                            change_preview.added_lines,
                            change_preview.removed_lines
                        ))
                        .strong()
                        .color(IdePalette::dark().warning),
                    );
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(change_preview.full_diff.as_str())
                                .monospace()
                                .size(10.0)
                                .color(IdePalette::dark().text),
                        );
                    });
                } else {
                    ui.label("No active unsaved changes.");
                }
            });
        self.show_full_diff = open;
    }
}

impl eframe::App for VelocityApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_agent_messages();
        self.handle_global_shortcuts(&ctx);
        self.update_diagnostics();

        let now = std::time::Instant::now();
        if self.file_tree.is_none()
            || now.duration_since(self.last_tree_update) > std::time::Duration::from_secs(3)
        {
            self.file_tree = Some(build_file_tree(&self.workspace_root));
            self.last_tree_update = now;
        }

        let mut cursor_pos = None;
        if let Some(active_id) = &self.active_tab {
            if let Some(buf) = self.buffers.get(active_id) {
                let editor_id = egui::Id::new("code_editor");
                if let Some(state) = egui::widgets::text_edit::TextEditState::load(&ctx, editor_id) {
                    if let Some(cursor_range) = state.cursor.char_range() {
                        cursor_pos = Some(get_cursor_pos(
                            buf.content(),
                            cursor_range.primary.index.into(),
                        ));
                    }
                }
            }
        }
        let dirty_buffer_count = self.dirty_buffer_count();
        let active_change_preview = self.active_change_preview();

        egui::Panel::top("toolbar").show(ui, |ui: &mut egui::Ui| {
            ui.horizontal(|ui: &mut egui::Ui| {
                ui.spacing_mut().item_spacing.x = 10.0;

                let buttons: [(&str, fn(&mut VelocityApp)); 8] = [
                    ("➕ New", VelocityApp::open_editor_stub),
                    ("📂 Open", VelocityApp::open_file_dialog),
                    ("💾 Save", VelocityApp::save_active),
                    ("💾 Save As…", VelocityApp::save_active_as),
                    ("💾 Save All", VelocityApp::save_all),
                    ("🔄 Models", VelocityApp::refresh_models),
                    ("✅ Approve All", VelocityApp::approve_all_pending_tools),
                    ("🛑 Decline All", VelocityApp::reject_all_pending_tools),
                ];
                for (label, action) in buttons {
                    if ui.button(label).clicked() {
                        action(self);
                    }
                }

                if ui.button("⚙️ Build").clicked() {
                    self.build_active();
                }
                if ui.button("▶ Run").clicked() {
                    self.run_active();
                }
                if ui.button("💬 Chat").clicked() {
                    self.focus_panel(TabKind::Chat);
                }
                if ui.button("🧭 Route").clicked() {
                    self.plan_routed_subagents();
                }
                if ui.button("🎛 Mission").clicked() {
                    self.focus_panel(TabKind::MissionControl);
                }
                if ui.button("🧠 Orchestrate").clicked() {
                    self.focus_panel(TabKind::Orchestrator);
                }
                if ui.button("🔍 Search").clicked() {
                    self.focus_panel(TabKind::Search);
                }
                if ui.button("📊 Graph").clicked() {
                    self.focus_panel(TabKind::Graph);
                }
                if ui.button("📺 Terminal").clicked() {
                    self.focus_panel(TabKind::Output);
                }
                if dirty_buffer_count > 0 {
                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!("Δ {} dirty", dirty_buffer_count))
                            .strong()
                            .color(IdePalette::dark().warning),
                    );
                }
            });
        });

        egui::Panel::left("left_sidebar")
            .resizable(true)
            .default_size(240.0)
            .show(ui, |ui: &mut egui::Ui| {
                ui.add_space(4.0);
                ui.vertical(|ui: &mut egui::Ui| {
                    ui.horizontal(|ui: &mut egui::Ui| {
                        ui.label(
                            egui::RichText::new("📁 PROJECTS")
                                .size(12.0)
                                .strong()
                                .color(IdePalette::dark().accent),
                        );
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if ui
                            .button("➕ Register")
                            .on_hover_text("Register Project Directory")
                            .clicked()
                        {
                            self.show_add_project_ui = !self.show_add_project_ui;
                        }
                    });

                    if self.show_add_project_ui {
                        ui.horizontal(|ui: &mut egui::Ui| {
                            ui.text_edit_singleline(&mut self.new_project_path_input);
                            if ui.button("Add").clicked() {
                                let path = PathBuf::from(&self.new_project_path_input);
                                if path.exists() && path.is_dir() {
                                    if !self.projects.contains(&path) {
                                        self.projects.push(path.clone());
                                    }
                                    self.new_project_path_input.clear();
                                    self.show_add_project_ui = false;
                                } else {
                                    self.status_message =
                                        "Path does not exist or is not a directory".into();
                                }
                            }
                        });
                    }

                    let active_name = self
                        .workspace_root
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    egui::ComboBox::from_id_salt("project_combo")
                        .selected_text(active_name)
                        .show_ui(ui, |ui: &mut egui::Ui| {
                            let mut selected_idx =
                                self.projects.iter().position(|p| p == &self.workspace_root);
                            for (idx, proj) in self.projects.iter().enumerate() {
                                let name = proj
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string();
                                if ui
                                    .selectable_value(&mut selected_idx, Some(idx), name)
                                    .clicked()
                                {
                                    let new_path = proj.clone();
                                    if new_path.is_dir() {
                                        self.workspace_root = new_path.clone();
                                        let _ = self
                                            .agent_tx
                                            .send(crate::agent::UiToAgentMessage::SetWorkspace(new_path.clone()));
                                        self.status_message = format!(
                                            "Switched to {:?}",
                                            proj.file_name().unwrap_or_default()
                                        );
                                    } else {
                                        self.status_message =
                                            format!("Failed to switch to {:?}", new_path);
                                    }
                                }
                            }
                        });

                    ui.separator();

                    let timeline_snapshot = crate::editor::task_timeline::TaskTimelineSnapshot::new(&self.task_timeline);
                    render_task_timeline(ui, &timeline_snapshot);

                    ui.separator();
                    ui.label(
                        egui::RichText::new("🌲 FILE EXPLORER")
                            .size(12.0)
                            .strong()
                            .color(IdePalette::dark().accent),
                    );
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        if let Some(tree) = self.file_tree.take() {
                            let tree_copied = tree.clone();
                            let root = tree_copied.clone();
                            
                            fn render_tree_node(ui: &mut egui::Ui, node: &FileNode, app: &mut VelocityApp) {
                                if let Some(children) = &node.children {
                                    for child in children {
                                        if child.is_dir {
                                            ui.collapsing(format!("📁 {}", child.name), |ui| {
                                                render_tree_node(ui, child, app);
                                            });
                                        } else {
                                            ui.horizontal(|ui| {
                                                ui.label("📄");
                                                if ui.selectable_label(false, &child.name).clicked() {
                                                    app.open_editor(Some(child.path.clone()));
                                                }
                                            });
                                        }
                                    }
                                }
                            }
                            render_tree_node(ui, &root, self);
                            self.file_tree = Some(tree);
                        }
                    });
                });
            });

        let mut active_symbol = None;
        if let Some(active_id) = &self.active_tab {
            if let Some(buf) = self.buffers.get(active_id) {
                if let Some((line, _col)) = cursor_pos {
                    active_symbol = get_active_symbol(buf.content(), line);
                }
            }
        }

        egui::Panel::right("right_sidebar")
            .resizable(true)
            .default_size(280.0)
            .show(ui, |ui: &mut egui::Ui| {
                ui.add_space(4.0);
                ui.vertical(|ui: &mut egui::Ui| {
                    ui.label(egui::RichText::new("🧠 SEMANTIC HISTORY").size(12.0).strong().color(IdePalette::dark().accent));
                    ui.separator();

                    self.smart_sidebar.clear();
                    if self.build_errors_count > 0 {
                        self.smart_sidebar.add_diagnostic(0, true, "workspace", 0, 0, "Build errors require attention");
                    }
                    if !self.search_query.is_empty() {
                        self.smart_sidebar.add_quick_action(0, "Review search results", &self.search_query, 1);
                    }

                    if let Some(change_preview) = &active_change_preview {
                        self.smart_sidebar.add_quick_action(0, "Review current changes", &change_preview.file_label, 0);
                        ui.group(|ui| {
                            ui.label(
                                egui::RichText::new(format!("Δ Active changes: {}", change_preview.file_label))
                                    .strong()
                                    .color(IdePalette::dark().warning),
                            );
                            ui.label(
                                egui::RichText::new(format!("+{} / -{} lines", change_preview.added_lines, change_preview.removed_lines))
                                    .size(10.0)
                                    .color(IdePalette::dark().text_muted),
                            );
                            ui.horizontal(|ui| {
                                if ui.small_button("Save").clicked() {
                                    self.save_active();
                                }
                                if ui.small_button("Revert").clicked() {
                                    self.revert_active_from_disk();
                                }
                                if ui.small_button("Stage").clicked() {
                                    self.stage_active_file();
                                }
                                if ui.small_button("Ask agent").clicked() {
                                    self.ask_agent_about_active_diff();
                                }
                                if ui.small_button("Full diff").clicked() {
                                    self.show_full_diff = true;
                                }
                            });
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(change_preview.preview.as_str())
                                    .monospace()
                                    .size(10.0)
                                    .color(IdePalette::dark().text),
                            );
                        });
                        ui.separator();
                    }

                    if let Some(symbol) = &active_symbol {
                        self.smart_sidebar.add_symbol(0, symbol, "active-buffer", cursor_pos.map(|(line, _)| line as u32).unwrap_or(0), 0);
                        self.smart_sidebar.add_quick_action(0, "Inspect semantic history", symbol, 2);
                        ui.label(egui::RichText::new(format!("Symbol: {}()", symbol)).strong().color(IdePalette::dark().accent));
                        
                        let symbol_hash = hash_str(symbol);
                        ui.label(egui::RichText::new(format!("Hash: {:016x}", symbol_hash)).size(10.0).weak());

                        if let Ok(sm) = crate::automation::open_workspace_site_map(&self.workspace_root) {
                            let callers = sm.get_callers(symbol_hash);
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new("📞 CALLERS").size(11.0).strong().color(IdePalette::dark().accent));
                            if callers.is_empty() {
                                ui.label("No active callers found in graph.");
                            } else {
                                for caller in &callers {
                                    ui.label(format!("• 0x{:016x}", caller));
                                }
                            }

                            let deps = sm.get_dependencies(symbol_hash);
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new("⚙️ DEPENDENCIES").size(11.0).strong().color(IdePalette::dark().accent));
                            if deps.is_empty() {
                                ui.label("No dependencies found.");
                            } else {
                                for dep in &deps {
                                    ui.label(format!("• 0x{:016x}", dep));
                                }
                            }

                            let intent_triples = sm.find_triples(Some(symbol_hash), Some(3), None);
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new("💬 AI INTENT & TRANSCRIPTS").size(11.0).strong().color(IdePalette::dark().accent));
                            if intent_triples.is_empty() {
                                ui.label("No agent sessions linked to this symbol.");
                            } else {
                                for triple in &intent_triples {
                                    ui.horizontal(|ui| {
                                        ui.label(format!("• Session: {:016x}", triple.object_hash));
                                    });
                                }
                            }
                        } else {
                            ui.label("SiteMap index offline or empty.");
                        }
                    } else {
                        self.smart_sidebar.add_quick_action(0, "Select a symbol", "Move the cursor to a declaration", 3);
                        ui.vertical_centered(|ui| {
                            ui.add_space(40.0);
                            ui.label("Place cursor on a class or function declaration to view its Semantic Blame history.");
                        });
                    }

                    ui.separator();
                    let sidebar_snapshot = crate::editor::smart_sidebar::SmartSidebarSnapshot::new(&self.smart_sidebar);
                    crate::editor::smart_sidebar::render_smart_sidebar(ui, &sidebar_snapshot);
                });
            });

        let branch = get_git_branch(&self.workspace_root);
        let build_ok = self.build_errors_count == 0;
        let latency_us = crate::ipc::telemetry_share::TELEMETRY_LATENCY_US
            .load(std::sync::atomic::Ordering::Relaxed);
        let status_info = if latency_us > 0 {
            format!(
                "{} | 🟢 GPU: {} | ⚡ ShMem: {}µs",
                self.status_message, self.gpu_name, latency_us
            )
        } else {
            format!(
                "{} | 🟢 GPU: {} | ⚡ ShMem: active",
                self.status_message, self.gpu_name
            )
        };
        crate::editor::status_bar::StatusBar::show(
            ui,
            branch.as_deref(),
            cursor_pos,
            build_ok,
            &status_info,
        );

        egui::CentralPanel::default().show(ui, |ui| {
            let mut dock_state = self.dock_state.take().expect("dock state");
            let mut viewer = TabViewerImpl { app: self };
            egui_dock::DockArea::new(&mut dock_state)
                .style(egui_dock::Style::from_egui(ui.style().as_ref()))
                .show_inside(ui, &mut viewer);
            self.dock_state = Some(dock_state);
        });

        egui::Panel::bottom("agentic_ui_panel")
            .default_size(120.0)
            .resizable(true)
            .show(ui, |ui: &mut egui::Ui| {
                ui.add_space(4.0);

                {
                    let snapshot = RenderSnapshot::new(&self.agent_ui_state);
                    ui.vertical(|ui| {
                        render_agent_metrics(ui, &snapshot);
                        ui.separator();
                        render_thinking_panel(ui, &snapshot, (226, 227, 243));
                        render_pending_approvals(ui, &snapshot);
                    });
                }

                if !self.pending_approvals.is_empty() {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("Direct approval actions")
                                .size(10.0)
                                .color(IdePalette::dark().text_muted),
                        );
                        if ui.button("Approve all").clicked() {
                            self.approve_all_pending_tools();
                        }
                        if ui.button("Decline all").clicked() {
                            self.reject_all_pending_tools();
                        }
                    });

                    let approval_count = self.pending_approvals.len().min(3);
                    for idx in 0..approval_count {
                        let tool_name = self.pending_approvals[idx].1.clone();
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(tool_name.as_str())
                                    .size(10.0)
                                    .color(IdePalette::dark().text),
                            );
                            if ui.small_button("Approve").clicked() {
                                self.approve_pending_tool_at(idx);
                            }
                            if ui.small_button("Decline").clicked() {
                                self.reject_pending_tool_at(idx);
                            }
                            if ui.small_button("Chat").clicked() {
                                self.toggle_chat();
                            }
                        });
                    }
                }
            });

        self.command_palette_ui(&ctx);
        self.file_dialog_ui(&ctx);
        self.save_as_dialog_ui(&ctx);
        self.full_diff_ui(&ctx);
        self.toasts.ui(&ctx);
    }
}
