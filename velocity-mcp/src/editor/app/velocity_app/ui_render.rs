use std::path::PathBuf;
use eframe::egui;

use crate::editor::agent_ui_render::{render_agent_metrics, RenderSnapshot};
use crate::editor::task_timeline::render_task_timeline;

use super::super::helpers::*;
use super::super::render::TabViewerImpl;
use super::super::types::*;
use super::struct_def::VelocityApp;

impl VelocityApp {
    pub fn search_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let suggested_queries: &[&str] = match self.appearance.profile {
            crate::editor::theme::WorkspaceProfile::Coder => &["TODO", "fn ", "struct "],
            crate::editor::theme::WorkspaceProfile::AutomationOperator => {
                &["desktop", "browser", "automation"]
            }
            crate::editor::theme::WorkspaceProfile::MissionControl => {
                &["worker", "task", "approval"]
            }
            crate::editor::theme::WorkspaceProfile::Accessibility => {
                &["theme", "contrast", "scale"]
            }
        };

        egui::Frame::new()
            .inner_margin(egui::Margin::same(10))
            .fill(palette.bg_primary)
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.heading("Search");
                    ui.horizontal(|ui| {
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.search_query)
                                .hint_text("Search…")
                                .desired_width(ui.available_width() - 10.0),
                        );
                        if response.changed() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
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
                                ui.add_space(8.0);
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(
                                        egui::RichText::new("Try:")
                                            .small()
                                            .color(palette.text_muted),
                                    );
                                    for query in suggested_queries {
                                        if ui.small_button(*query).clicked() {
                                            self.search_query = (*query).to_string();
                                            self.search_hits = crate::editor::search::project_search(
                                                &self.workspace_root,
                                                &self.search_query,
                                                100,
                                            );
                                        }
                                    }
                                });
                            } else {
                                ui.add_space(8.0);
                                ui.label(
                                    egui::RichText::new("No results found")
                                        .color(palette.text_muted),
                                );
                            }
                        } else {
                            ui.label(
                                egui::RichText::new(format!("{} results", hits.len()))
                                    .small()
                                    .color(palette.text_muted),
                            );
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
            } else if cmd && i.key_pressed(egui::Key::J) {
                self.toggle_panel(TabKind::Chat);
            } else if cmd && i.key_pressed(egui::Key::Backtick) {
                self.toggle_panel(TabKind::Output);
            } else if cmd && i.key_pressed(egui::Key::E) {
                self.toggle_left_sidebar();
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
                            let mut last_category = "";
                            for (idx, cmd) in commands.iter().enumerate() {
                                if cmd.category != last_category {
                                    last_category = cmd.category;
                                    ui.add_space(4.0);
                                    ui.label(
                                        egui::RichText::new(cmd.category.to_uppercase())
                                            .small()
                                            .strong(),
                                    );
                                }
                                let selected = idx == self.command_palette.selected;
                                ui.horizontal(|ui| {
                                    let text = egui::RichText::new(cmd.label).color(if selected {
                                        ui.visuals().selection.stroke.color
                                    } else {
                                        ui.visuals().text_color()
                                    });
                                    let resp = ui.selectable_label(selected, text);
                                    if let Some(shortcut) = cmd.shortcut {
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(
                                                egui::RichText::new(shortcut)
                                                    .small()
                                                    .monospace()
                                                    .color(ui.visuals().text_color().gamma_multiply(0.5)),
                                            );
                                        });
                                    }
                                    if resp.clicked() {
                                        (cmd.action)(self);
                                        self.command_palette.open = false;
                                    }
                                });
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
                if ui.text_edit_singleline(&mut path_string).changed() {
                    self.pending_open_path = Some(PathBuf::from(&path_string));
                }
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
                if ui.text_edit_singleline(&mut path_string).changed() {
                    self.pending_save_as_path = Some(PathBuf::from(&path_string));
                }
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
        let palette = self.palette();
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
                        .color(palette.warning),
                    );
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(change_preview.full_diff.as_str())
                                .monospace()
                                .size(10.0)
                                .color(palette.text),
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
    fn on_exit(&mut self) {
        self.save_workspace_preferences();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.apply_appearance(&ctx);
        let palette = self.palette();
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
        let active_change_preview = self.active_change_preview();

        egui::Panel::top("toolbar").show(ui, |ui: &mut egui::Ui| {
            ui.horizontal(|ui: &mut egui::Ui| {
                ui.spacing_mut().item_spacing.x = 6.0;

                ui.menu_button("File", |ui| {
                    if ui.button("New File").clicked() {
                        self.open_editor_stub();
                        ui.close();
                    }
                    if ui.button("Open File…").clicked() {
                        self.open_file_dialog();
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Save").clicked() {
                        self.save_active();
                        ui.close();
                    }
                    if ui.button("Save As…").clicked() {
                        self.save_active_as();
                        ui.close();
                    }
                    if ui.button("Save All").clicked() {
                        self.save_all();
                        ui.close();
                    }
                });

                ui.separator();

                if ui.button("Build").clicked() {
                    self.build_active();
                }
                if ui.button("Run").clicked() {
                    self.run_active();
                }
                if ui.button("Plan").on_hover_text("Plan routed sub-agents").clicked() {
                    self.plan_routed_subagents();
                }

                if !self.pending_approvals.is_empty() {
                    ui.separator();
                    if ui.button(format!("Approve All ({})", self.pending_approvals.len())).clicked() {
                        self.approve_all_pending_tools();
                    }
                    if ui.button("Decline All").clicked() {
                        self.reject_all_pending_tools();
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button(if self.right_sidebar_visible { "◧" } else { "◨" })
                        .on_hover_text("Toggle history panel")
                        .clicked()
                    {
                        self.toggle_right_sidebar();
                    }
                    if ui
                        .small_button(if self.left_sidebar_visible { "◨" } else { "◧" })
                        .on_hover_text("Toggle sidebar")
                        .clicked()
                    {
                        self.toggle_left_sidebar();
                    }

                    let model_name = if self.selected_model.is_empty() {
                        "default"
                    } else {
                        &self.selected_model
                    };
                    ui.label(
                        egui::RichText::new(format!("{} / {}", self.provider.label(), model_name))
                            .small()
                            .color(palette.accent),
                    );
                });
            });
        });

        if self.left_sidebar_visible {
            let panel_response = egui::Panel::left("left_sidebar")
                .resizable(true)
                .default_size(self.left_sidebar_width)
                .show(ui, |ui: &mut egui::Ui| {
                    self.left_sidebar_width = ui.available_width().max(180.0);
                    ui.add_space(6.0);

                    // Project selector (compact dropdown)
                    let current_name = self.workspace_root
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let projects = self.projects.clone();
                    egui::ComboBox::from_id_salt("project_selector")
                        .selected_text(current_name)
                        .width(160.0)
                        .show_ui(ui, |ui| {
                            for proj in &projects {
                                let name = proj.file_name().unwrap_or_default().to_string_lossy().to_string();
                                let is_current = proj == &self.workspace_root;
                                if ui.selectable_label(is_current, &name).clicked() && !is_current {
                                    let new_path = proj.clone();
                                    if new_path.is_dir() {
                                        self.workspace_root = new_path.clone();
                                        self.reload_workspace_provider_settings();
                                        self.restore_workspace_preferences();
                                        self.apply_appearance(&ctx);
                                        let _ = self.agent_tx.send(crate::agent::UiToAgentMessage::SetWorkspace(new_path.clone()));
                                        let _ = self.agent_tx.send(crate::agent::UiToAgentMessage::ApplySessionState {
                                            provider: self.provider,
                                            model: self.selected_model.clone(),
                                            thinking: self.thinking_enabled,
                                        });
                                        self.status_message = format!("Switched to {:?}", proj.file_name().unwrap_or_default());
                                    }
                                }
                            }
                        });

                    ui.separator();

                    // Tab selector (single instance)
                    ui.horizontal(|ui| {
                        if ui.selectable_label(self.left_sidebar_tab == 0, "Files").clicked() {
                            self.left_sidebar_tab = 0;
                        }
                        if ui.selectable_label(self.left_sidebar_tab == 1, "Activity").clicked() {
                            self.left_sidebar_tab = 1;
                        }
                    });
                    ui.separator();

                    if self.left_sidebar_tab == 1 {
                        let timeline_snapshot = crate::editor::task_timeline::TaskTimelineSnapshot::new(&self.task_timeline);
                        render_task_timeline(ui, &timeline_snapshot);
                    } else {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            if let Some(tree) = self.file_tree.take() {
                                let root = tree.clone();
                                fn render_tree_node(ui: &mut egui::Ui, node: &FileNode, app: &mut VelocityApp) {
                                    if let Some(children) = &node.children {
                                        for child in children {
                                            if child.is_dir {
                                                ui.collapsing(&child.name, |ui| {
                                                    render_tree_node(ui, child, app);
                                                });
                                            } else {
                                                if ui.selectable_label(false, &child.name).clicked() {
                                                    app.open_editor(Some(child.path.clone()));
                                                }
                                            }
                                        }
                                    }
                                }
                                render_tree_node(ui, &root, self);
                                self.file_tree = Some(tree);
                            }
                        });
                    }
                });
            self.left_sidebar_width = panel_response.response.rect.width().max(180.0);
        }

        let mut active_symbol = None;
        if let Some(active_id) = &self.active_tab {
            if let Some(buf) = self.buffers.get(active_id) {
                if let Some((line, _col)) = cursor_pos {
                    active_symbol = get_active_symbol(buf.content(), line);
                }
            }
        }

        if self.right_sidebar_visible {
            let panel_response = egui::Panel::right("right_sidebar")
                .resizable(true)
                .default_size(self.right_sidebar_width)
                .show(ui, |ui: &mut egui::Ui| {
                    self.right_sidebar_width = ui.available_width().max(220.0);
                    ui.add_space(4.0);

                    self.smart_sidebar.clear();
                    if self.build_errors_count > 0 {
                        self.smart_sidebar.add_diagnostic(0, true, "workspace", 0, 0, "Build errors require attention");
                    }

                    // Active changes section
                    if let Some(change_preview) = &active_change_preview {
                        self.smart_sidebar.add_quick_action(0, "Review current changes", &change_preview.file_label, 0);
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(&change_preview.file_label).strong().color(palette.warning));
                                ui.label(egui::RichText::new(format!("+{} -{}", change_preview.added_lines, change_preview.removed_lines)).small().color(palette.text_muted));
                            });
                            ui.horizontal(|ui| {
                                if ui.small_button("Save").clicked() { self.save_active(); }
                                if ui.small_button("Revert").clicked() { self.revert_active_from_disk(); }
                                if ui.small_button("Stage").clicked() { self.stage_active_file(); }
                                if ui.small_button("Diff").clicked() { self.show_full_diff = true; }
                            });
                        });
                        ui.separator();
                    }

                    // Symbol context section
                    if let Some(symbol) = &active_symbol {
                        self.smart_sidebar.add_symbol(0, symbol, "active-buffer", cursor_pos.map(|(line, _)| line as u32).unwrap_or(0), 0);
                        ui.label(egui::RichText::new(format!("{}()", symbol)).strong().color(palette.accent));

                        if let Ok(sm) = crate::automation::open_workspace_site_map(&self.workspace_root) {
                            let symbol_hash = hash_str(symbol);
                            let callers = sm.get_callers(symbol_hash);
                            let deps = sm.get_dependencies(symbol_hash);

                            if !callers.is_empty() {
                                ui.add_space(4.0);
                                ui.label(egui::RichText::new("Callers").small().strong());
                                for caller in &callers {
                                    ui.label(egui::RichText::new(format!("  {:016x}", caller)).small().color(palette.text_muted));
                                }
                            }
                            if !deps.is_empty() {
                                ui.add_space(4.0);
                                ui.label(egui::RichText::new("Dependencies").small().strong());
                                for dep in &deps {
                                    ui.label(egui::RichText::new(format!("  {:016x}", dep)).small().color(palette.text_muted));
                                }
                            }
                        }
                    } else {
                        ui.vertical_centered(|ui| {
                            ui.add_space(20.0);
                            ui.label(egui::RichText::new("Place cursor on a symbol to inspect").small().color(palette.text_muted));
                        });
                    }

                    ui.separator();
                    let sidebar_snapshot = crate::editor::smart_sidebar::SmartSidebarSnapshot::new(&self.smart_sidebar);
                    crate::editor::smart_sidebar::render_smart_sidebar(ui, &sidebar_snapshot, palette);
                });
            self.right_sidebar_width = panel_response.response.rect.width().max(220.0);
        }

        let branch = get_git_branch(&self.workspace_root);
        let build_ok = self.build_errors_count == 0;
        crate::editor::status_bar::StatusBar::show(
            ui,
            palette,
            branch.as_deref(),
            cursor_pos,
            build_ok,
            &self.status_message,
        );

        egui::CentralPanel::default().show(ui, |ui| {
            let mut dock_state = self.dock_state.take().expect("dock state");
            let mut viewer = TabViewerImpl { app: self };
            egui_dock::DockArea::new(&mut dock_state)
                .style(egui_dock::Style::from_egui(ui.style().as_ref()))
                .show_inside(ui, &mut viewer);
            self.dock_state = Some(dock_state);
        });

        // Only show bottom panel when there's agent activity or pending approvals
        let has_agent_activity = self.agent_active || !self.pending_approvals.is_empty();
        if has_agent_activity {
            egui::Panel::bottom("agentic_ui_panel")
                .default_size(100.0)
                .resizable(true)
                .show(ui, |ui: &mut egui::Ui| {
                    let snapshot = RenderSnapshot::new(&self.agent_ui_state);
                    render_agent_metrics(ui, &snapshot);

                    if !self.pending_approvals.is_empty() {
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(format!("{} pending approvals", self.pending_approvals.len())).small().color(palette.warning));
                            if ui.small_button("Approve all").clicked() {
                                self.approve_all_pending_tools();
                            }
                            if ui.small_button("Decline all").clicked() {
                                self.reject_all_pending_tools();
                            }
                        });
                    }
                });
        }

        self.command_palette_ui(&ctx);
        self.file_dialog_ui(&ctx);
        self.save_as_dialog_ui(&ctx);
        self.full_diff_ui(&ctx);
        self.toasts.ui(&ctx, palette);
    }
}
