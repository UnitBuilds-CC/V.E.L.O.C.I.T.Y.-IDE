use std::path::{Path, PathBuf};
use crossbeam_channel::{Sender, Receiver};
use serde_json::Value;
use eframe::egui;
use eframe::egui::Color32;

use crate::editor::buffer::EditorBuffer;
use crate::agent::{UiToAgentMessage, AgentToUiMessage};

pub struct VelocityApp {
    workspace_root: PathBuf,
    open_tabs: Vec<EditorBuffer>,
    active_tab_idx: usize,
    files_list: Vec<PathBuf>,
    
    // Chat & Agent variables
    chat_messages: Vec<ChatMessageUi>,
    current_chat_input: String,
    is_thinking: bool,
    agent_status: String,
    pending_approval: Option<PendingApproval>,
    auto_approve: bool,
    
    agent_tx: Sender<UiToAgentMessage>,
    agent_rx: Receiver<AgentToUiMessage>,
    
    // Bottom panel terminal output
    terminal_output: String,
    
    // File search modal (Ctrl+P)
    show_search_modal: bool,
    search_query: String,
}

struct ChatMessageUi {
    role: String,
    content: String,
}

struct PendingApproval {
    id: String,
    tool_name: String,
    arguments: Value,
}

impl VelocityApp {
    pub fn new(
        workspace_root: PathBuf,
        agent_tx: Sender<UiToAgentMessage>,
        agent_rx: Receiver<AgentToUiMessage>,
    ) -> Self {
        let mut app = Self {
            workspace_root,
            open_tabs: Vec::new(),
            active_tab_idx: 0,
            files_list: Vec::new(),
            chat_messages: Vec::new(),
            current_chat_input: String::new(),
            is_thinking: false,
            agent_status: "Idling".to_string(),
            pending_approval: None,
            auto_approve: false,
            agent_tx,
            agent_rx,
            terminal_output: "V.E.L.O.C.I.T.Y. Execution Sandbox Terminal Initialized.\n".to_string(),
            show_search_modal: false,
            search_query: String::new(),
        };
        app.refresh_files();
        
        // Open README.md if it exists
        let readme_path = app.workspace_root.join("README.md");
        if readme_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&readme_path) {
                app.open_tabs.push(EditorBuffer::new(Some(readme_path), content));
            }
        }
        
        app
    }

    fn refresh_files(&mut self) {
        self.files_list.clear();
        scan_dir_recursive(&self.workspace_root, &mut self.files_list);
    }

    fn active_buffer_mut(&mut self) -> Option<&mut EditorBuffer> {
        self.open_tabs.get_mut(self.active_tab_idx)
    }

    fn handle_agent_messages(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.agent_rx.try_recv() {
            match msg {
                AgentToUiMessage::ThoughtToken(token) => {
                    self.terminal_output.push_str(&token);
                }
                AgentToUiMessage::OutputToken(token) => {
                    if self.chat_messages.is_empty() || self.chat_messages.last().unwrap().role != "assistant" {
                        self.chat_messages.push(ChatMessageUi {
                            role: "assistant".to_string(),
                            content: String::new(),
                        });
                    }
                    self.chat_messages.last_mut().unwrap().content.push_str(&token);
                }
                AgentToUiMessage::RequestToolApproval { id, tool_name, arguments } => {
                    if self.auto_approve {
                        self.agent_tx.send(UiToAgentMessage::ApproveTool {
                            id: id.clone(),
                            tool_name: tool_name.clone(),
                            arguments: arguments.clone(),
                        }).ok();
                    } else {
                        self.pending_approval = Some(PendingApproval { id, tool_name, arguments });
                    }
                }
                AgentToUiMessage::ToolExecutionStarted { tool_name } => {
                    self.terminal_output.push_str(&format!("\n[TOOL START] Executing tool: {}\n", tool_name));
                }
                AgentToUiMessage::ToolExecutionFinished { tool_name, result } => {
                    self.terminal_output.push_str(&format!("[TOOL FINISH] Tool: {} completed.\nResult Summary:\n{}\n", tool_name, result));
                }
                AgentToUiMessage::StatusUpdate(status) => {
                    self.agent_status = status;
                }
                AgentToUiMessage::AgentFinished => {
                    self.is_thinking = false;
                    self.agent_status = "Finished reasoning loop.".to_string();
                }
                AgentToUiMessage::UpdateFileBuffer { path, content } => {
                    let mut found_idx = None;
                    for (idx, tab) in self.open_tabs.iter().enumerate() {
                        if tab.path.as_ref() == Some(&path) {
                            found_idx = Some(idx);
                            break;
                        }
                    }
                    if let Some(idx) = found_idx {
                        self.open_tabs[idx].update_content(content);
                        self.active_tab_idx = idx;
                    } else {
                        self.open_tabs.push(EditorBuffer::new(Some(path), content));
                        self.active_tab_idx = self.open_tabs.len() - 1;
                    }
                }
            }
            ctx.request_repaint();
        }
    }
}

impl eframe::App for VelocityApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Customize visuals
        let mut visuals = egui::Visuals::dark();
        visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(20, 20, 24);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(30, 30, 36);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(45, 45, 54);
        visuals.widgets.active.bg_fill = Color32::from_rgb(60, 60, 72);
        visuals.selection.bg_fill = Color32::from_rgb(79, 70, 229); // Indigo selection
        ctx.set_visuals(visuals);

        self.handle_agent_messages(ctx);

        if self.is_thinking {
            ctx.request_repaint();
        }

        // Global hotkeys
        if ctx.input(|i| i.key_pressed(egui::Key::P) && i.modifiers.command) {
            self.show_search_modal = true;
            self.search_query.clear();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::S) && i.modifiers.command) {
            let saved_path = if let Some(buf) = self.active_buffer_mut() {
                let _ = buf.save();
                buf.path.clone()
            } else {
                None
            };
            if let Some(path) = saved_path {
                self.terminal_output.push_str(&format!("Saved: {:?}\n", path));
            }
        }

        // Top Status Bar Panel
        egui::TopBottomPanel::top("top_panel")
            .frame(egui::Frame::none().fill(Color32::from_rgb(15, 15, 18)).inner_margin(8.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("⚡ V.E.L.O.C.I.T.Y. IDE");
                    ui.label("|");
                    ui.label(format!("Workspace: {}", self.workspace_root.to_string_lossy()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.is_thinking {
                            ui.spinner();
                        }
                        ui.label(format!("Agent Status: {}", self.agent_status));
                    });
                });
            });

        // Left Panel - File Explorer
        egui::SidePanel::left("explorer_panel")
            .width_range(180.0..=350.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label("📁 FILE EXPLORER");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("🔄").on_hover_text("Refresh Workspace").clicked() {
                            self.refresh_files();
                        }
                    });
                });
                ui.separator();
                
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut file_to_open = None;
                    
                    // Group files by top-level dir for a beautiful clean structure
                    let mut root_files = Vec::new();
                    let mut dirs = std::collections::BTreeMap::new();
                    
                    for f in &self.files_list {
                        if let Ok(rel) = f.strip_prefix(&self.workspace_root) {
                            let comps: Vec<_> = rel.components().collect();
                            if comps.len() == 1 {
                                root_files.push(f.clone());
                            } else if comps.len() > 1 {
                                let dir_name = comps[0].as_os_str().to_string_lossy().to_string();
                                dirs.entry(dir_name).or_insert_with(Vec::new).push(f.clone());
                            }
                        }
                    }

                    // Render Directories
                    for (dir, paths) in dirs {
                        egui::CollapsingHeader::new(format!("📁 {}", dir))
                            .default_open(false)
                            .show(ui, |ui| {
                                for p in paths {
                                    let filename = p.file_name().unwrap().to_string_lossy().to_string();
                                    let is_active = self.open_tabs.get(self.active_tab_idx)
                                        .and_then(|t| t.path.as_ref()) == Some(&p);
                                    
                                    let mut button_ui = ui.selectable_label(is_active, format!("📄 {}", filename));
                                    if button_ui.clicked() {
                                        file_to_open = Some(p.clone());
                                    }
                                }
                            });
                    }

                    // Render Root Files
                    for p in root_files {
                        let filename = p.file_name().unwrap().to_string_lossy().to_string();
                        let is_active = self.open_tabs.get(self.active_tab_idx)
                            .and_then(|t| t.path.as_ref()) == Some(&p);
                        
                        let button_ui = ui.selectable_label(is_active, format!("📄 {}", filename));
                        if button_ui.clicked() {
                            file_to_open = Some(p.clone());
                        }
                    }

                    if let Some(path) = file_to_open {
                        let mut found = false;
                        for (idx, tab) in self.open_tabs.iter().enumerate() {
                            if tab.path.as_ref() == Some(&path) {
                                self.active_tab_idx = idx;
                                found = true;
                                break;
                            }
                        }
                        if !found {
                            if let Ok(content) = std::fs::read_to_string(&path) {
                                self.open_tabs.push(EditorBuffer::new(Some(path), content));
                                self.active_tab_idx = self.open_tabs.len() - 1;
                            }
                        }
                    }
                });
            });

        // Right Panel - Agent Chat Console
        egui::SidePanel::right("chat_panel")
            .width_range(300.0..=500.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.label("🤖 ANTIGRAVITY CO-PILOT");
                ui.separator();

                // Pending Tool Approvals Panel (Rendered at top so it is always visible)
                if let Some(ref approval) = self.pending_approval {
                    let call_id = approval.id.clone();
                    let tool_name = approval.tool_name.clone();
                    let arguments = approval.arguments.clone();
                    
                    egui::Frame::none()
                        .fill(Color32::from_rgb(60, 48, 16))
                        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(217, 119, 6)))
                        .inner_margin(8.0)
                        .rounding(egui::Rounding::same(6.0))
                        .show(ui, |ui| {
                            ui.label("🛡️ TOOL APPROVAL REQUIRED:");
                            ui.label(format!("Tool: {}", tool_name));
                            ui.label(format!("Params: {}", serde_json::to_string(&arguments).unwrap_or_default()));
                            
                            ui.horizontal(|ui| {
                                if ui.button("🟢 Approve").clicked() {
                                    self.agent_tx.send(UiToAgentMessage::ApproveTool {
                                        id: call_id.clone(),
                                        tool_name: tool_name.clone(),
                                        arguments: arguments.clone(),
                                    }).ok();
                                    self.pending_approval = None;
                                }
                                if ui.button("🔴 Reject").clicked() {
                                    self.agent_tx.send(UiToAgentMessage::RejectTool {
                                        id: call_id.clone(),
                                        tool_name: tool_name.clone(),
                                    }).ok();
                                    self.pending_approval = None;
                                }
                            });
                        });
                    ui.add_space(8.0);
                }

                // Chat Messages Area
                let messages_area_height = ui.available_height() - 80.0;
                
                egui::ScrollArea::vertical()
                    .max_height(messages_area_height)
                    .show(ui, |ui| {
                        if self.chat_messages.is_empty() {
                            ui.vertical_centered(|ui| {
                                ui.add_space(20.0);
                                ui.label("No active discussion.");
                                ui.label("Type a query below to prompt the Agent.");
                            });
                        }
                        
                        for msg in &self.chat_messages {
                            let is_user = msg.role == "user";
                            let (bg_fill, border_color, text_color) = if is_user {
                                (Color32::from_rgb(79, 70, 229), Color32::from_rgb(99, 102, 241), Color32::WHITE)
                            } else {
                                (Color32::from_rgb(33, 33, 40), Color32::from_rgb(45, 45, 54), Color32::from_rgb(220, 220, 225))
                            };
                            
                            ui.horizontal(|ui| {
                                if is_user {
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                                        egui::Frame::none()
                                            .fill(bg_fill)
                                            .stroke(egui::Stroke::new(1.0, border_color))
                                            .inner_margin(8.0)
                                            .outer_margin(4.0)
                                            .rounding(egui::Rounding::same(8.0))
                                            .show(ui, |ui| {
                                                ui.set_max_width(280.0);
                                                ui.label(
                                                    egui::RichText::new(&msg.content)
                                                        .color(text_color)
                                                        .size(13.0)
                                                );
                                            });
                                    });
                                } else {
                                    ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
                                        egui::Frame::none()
                                            .fill(bg_fill)
                                            .stroke(egui::Stroke::new(1.0, border_color))
                                            .inner_margin(8.0)
                                            .outer_margin(4.0)
                                            .rounding(egui::Rounding::same(8.0))
                                            .show(ui, |ui| {
                                                ui.set_max_width(280.0);
                                                ui.label(
                                                    egui::RichText::new(&msg.content)
                                                        .color(text_color)
                                                        .size(13.0)
                                                );
                                            });
                                    });
                                }
                            });
                            ui.add_space(4.0);
                        }
                    });

                // Chat Input bar
                ui.separator();
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.auto_approve, "🛡️ Auto-Approve Tools");
                });
                ui.horizontal(|ui| {
                    let text_edit = ui.add(
                        egui::TextEdit::multiline(&mut self.current_chat_input)
                            .hint_text("Ask Antigravity... (Ctrl+Enter to send)")
                            .desired_rows(2)
                            .desired_width(ui.available_width() - 50.0)
                    );
                    
                    let enter_pressed = text_edit.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
                    
                    if (ui.button("Send").clicked() || enter_pressed) && !self.current_chat_input.trim().is_empty() {
                        let prompt = self.current_chat_input.trim().to_string();
                        self.chat_messages.push(ChatMessageUi {
                            role: "user".to_string(),
                            content: prompt.clone(),
                        });
                        self.current_chat_input.clear();
                        self.is_thinking = true;
                        self.agent_tx.send(UiToAgentMessage::UserPrompt(prompt)).ok();
                    }
                });
            });

        // Bottom Panel - Log & Terminal
        egui::TopBottomPanel::bottom("terminal_panel")
            .height_range(100.0..=300.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.label("🖥️ EXECUTION CONSOLE");
                ui.separator();
                egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.terminal_output)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .interactive(false)
                    );
                });
            });

        // Central Panel - Code Editor Workspace
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.open_tabs.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(60.0);
                    ui.heading("⚡ V.E.L.O.C.I.T.Y. IDE - Native Workspace Editor");
                    ui.add_space(10.0);
                    ui.label("Extremely high performance. Zero allocations. Direct GPU rendering.");
                    ui.add_space(20.0);
                    ui.label("Keyboard Shortcuts:");
                    ui.label("Ctrl + P  :  Search Files");
                    ui.label("Ctrl + S  :  Save Active Buffer");
                    ui.label("Ctrl + Enter  :  Send Chat Prompt");
                });
            } else {
                // Tab Bar
                ui.horizontal(|ui| {
                    let mut tab_to_close = None;
                    for (idx, tab) in self.open_tabs.iter().enumerate() {
                        let filename = tab.path.as_ref()
                            .and_then(|p| p.file_name())
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "Untitled".to_string());
                        
                        let name_formatted = format!("{}{}", filename, if tab.is_dirty { "*" } else { "" });
                        let is_active = idx == self.active_tab_idx;
                        
                        ui.horizontal(|ui| {
                            let select = ui.selectable_label(is_active, &name_formatted);
                            if select.clicked() {
                                self.active_tab_idx = idx;
                            }
                            if ui.button("x").clicked() {
                                tab_to_close = Some(idx);
                            }
                        });
                    }
                    
                    if let Some(close_idx) = tab_to_close {
                        self.open_tabs.remove(close_idx);
                        if self.open_tabs.is_empty() {
                            self.active_tab_idx = 0;
                        } else if self.active_tab_idx >= self.open_tabs.len() {
                            self.active_tab_idx = self.open_tabs.len() - 1;
                        }
                    }
                });
                
                ui.separator();
                
                // Code Editor TextArea
                if let Some(buf) = self.active_buffer_mut() {
                    let editor = egui::TextEdit::multiline(&mut buf.content)
                        .font(egui::TextStyle::Monospace)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(38)
                        .lock_focus(true);
                    
                    let response = ui.add(editor);
                    if response.changed() {
                        buf.is_dirty = true;
                        buf.rope = ropey::Rope::from_str(&buf.content);
                    }
                }
            }
        });

        // Search File Modal Dialog
        if self.show_search_modal {
            egui::Window::new("Quick Open File")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, -100.0])
                .show(ctx, |ui| {
                    let text_edit = ui.add(
                        egui::TextEdit::singleline(&mut self.search_query)
                            .hint_text("Type file name...")
                    );
                    text_edit.request_focus();
                    
                    ui.separator();
                    
                    let query_lower = self.search_query.to_lowercase();
                    let mut matches = Vec::new();
                    
                    for f in &self.files_list {
                        let filename = f.file_name().unwrap_or_default().to_string_lossy().to_string();
                        if filename.to_lowercase().contains(&query_lower) {
                            matches.push(f.clone());
                            if matches.len() >= 8 {
                                break;
                            }
                        }
                    }
                    
                    for m in matches {
                        let rel_path = m.strip_prefix(&self.workspace_root).unwrap_or(&m).to_string_lossy().to_string();
                        if ui.selectable_label(false, &rel_path).clicked() {
                            if let Ok(content) = std::fs::read_to_string(&m) {
                                let mut found = false;
                                for (idx, tab) in self.open_tabs.iter().enumerate() {
                                    if tab.path.as_ref() == Some(&m) {
                                        self.active_tab_idx = idx;
                                        found = true;
                                        break;
                                    }
                                }
                                if !found {
                                    self.open_tabs.push(EditorBuffer::new(Some(m), content));
                                    self.active_tab_idx = self.open_tabs.len() - 1;
                                }
                            }
                            self.show_search_modal = false;
                        }
                    }
                    
                    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.show_search_modal = false;
                    }
                });
        }
    }
}

fn scan_dir_recursive(dir: &Path, files: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
            if path.is_dir() {
                scan_dir_recursive(&path, files);
            } else {
                files.push(path);
            }
        }
    }
}
