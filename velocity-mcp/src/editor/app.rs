use crate::agent::{AgentToUiMessage, ModelInfo, UiToAgentMessage, AiProvider};
use crate::automation::read_latest_diagnostics;
use crate::editor::buffer::EditorBuffer;
use crate::editor::chat_panel::{ChatPanelState, render_chat_panel};
use crate::editor::code_editor::CodeEditor;
use crate::editor::orchestrator_panel::OrchestratorPanel;
use crate::editor::theme::IdePalette;
use crate::editor::usage_panel::{render_usage_compact, render_usage_panel};
use crate::usage::AccountUsageView;
use crossbeam_channel::{Receiver, Sender};
use eframe::egui;
use egui_dock::{DockArea, DockState, Style as DockStyle, TabViewer};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TabId(pub u64);

impl TabId {
    pub fn next(counter: &mut u64) -> Self {
        *counter += 1;
        TabId(*counter)
    }
}

#[derive(Clone, Debug)]
pub struct Tab {
    pub id: TabId,
    pub kind: TabKind,
}

impl Tab {
    pub fn title(&self) -> String {
        match &self.kind {
            TabKind::Editor { path, .. } => path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "untitled".into()),
            TabKind::Chat => "Agent Chat".into(),
            TabKind::Output => "Output".into(),
            TabKind::Orchestrator => "Orchestrator".into(),
            TabKind::Usage => "Usage".into(),
        }
    }

    pub fn editor_path(&self) -> Option<&PathBuf> {
        match &self.kind {
            TabKind::Editor { path, .. } => path.as_ref(),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum TabKind {
    Editor {
        path: Option<PathBuf>,
        buffer_id: TabId,
    },
    Chat,
    Output,
    Orchestrator,
    Usage,
}

struct Command {
    label: &'static str,
    action: fn(&mut VelocityApp),
}

struct CommandPalette {
    open: bool,
    query: String,
    selected: usize,
}

pub struct VelocityApp {
    agent_tx: Sender<UiToAgentMessage>,
    agent_rx: Receiver<AgentToUiMessage>,

    workspace_root: PathBuf,

    tabs: Vec<Tab>,
    active_tab: Option<TabId>,
    buffers: HashMap<TabId, EditorBuffer>,

    dock_state: Option<DockState<Tab>>,

    chat: ChatPanelState,
    command_output: String,

    account_usage: Vec<AccountUsageView>,
    usage_date: String,

    command_palette: CommandPalette,
    status_message: String,

    tab_counter: u64,

    // Project Management
    pub projects: Vec<PathBuf>,
    pub show_add_project_ui: bool,
    pub new_project_path_input: String,
    pub agent_active: bool,
    pub pending_approvals: Vec<(String, String, serde_json::Value)>,
    pub auto_approve: bool,
    pub available_models: Vec<ModelInfo>,
    pub selected_model: String,
    pub thinking_enabled: bool,
    pub thinking_supported: bool,
    pub tools_supported: bool,
    pub models_loading: bool,
    pub provider: AiProvider,

    // File dialog state
    pub pending_open_path: Option<PathBuf>,
    pub pending_save_as_path: Option<PathBuf>,
    pub build_errors_count: usize,

    // Legacy inline chat state (used by chat_panel fn)
    pub chat_input: String,
    pub chat_history: String,
}

impl VelocityApp {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        workspace_root: PathBuf,
        agent_tx: Sender<UiToAgentMessage>,
        agent_rx: Receiver<AgentToUiMessage>,
    ) -> Self {
        let mut tab_counter = 0u64;
        let output = Tab {
            id: TabId::next(&mut tab_counter),
            kind: TabKind::Output,
        };
        let chat = Tab {
            id: TabId::next(&mut tab_counter),
            kind: TabKind::Chat,
        };
        let orchestrator = Tab {
            id: TabId::next(&mut tab_counter),
            kind: TabKind::Orchestrator,
        };
        let tabs = vec![output.clone(), chat.clone(), orchestrator.clone()];

        // Register default projects from nearby directories
        let mut projects = vec![workspace_root.clone()];
        if let Some(parent) = workspace_root.parent() {
            for sub in &["velocity-mcp", "velocity-ide", "ide", "agent"] {
                let path = parent.join(sub);
                if path.exists() && path.is_dir() && !projects.contains(&path) {
                    projects.push(path);
                }
            }
        }

        let mut app = Self {
            agent_tx,
            agent_rx,
            workspace_root,
            tabs,
            active_tab: Some(output.id.clone()),
            buffers: HashMap::new(),
            dock_state: Some(DockState::new(vec![output, chat, orchestrator])),
            chat_input: String::new(),
            command_output: String::from("V.E.L.O.C.I.T.Y. IDE initialized.\n"),
            chat_history: String::new(),
            command_palette: CommandPalette {
                open: false,
                query: String::new(),
                selected: 0,
            },
            status_message: String::from("Ready"),
            tab_counter,
            projects,
            show_add_project_ui: false,
            new_project_path_input: String::new(),
            agent_active: false,
            pending_approvals: Vec::new(),
            auto_approve: false,
            available_models: vec![ModelInfo {
                id: "@cf/moonshotai/kimi-k2.7-code".into(),
                label: "kimi-k2.7-code".into(),
                api_style: crate::agent::ApiStyle::OpenAiTools,
                supports_tools: true,
                supports_thinking: true,
            }],
            selected_model: "@cf/moonshotai/kimi-k2.7-code".into(),
            thinking_enabled: false,
            thinking_supported: true,
            tools_supported: true,
            models_loading: false,
            provider: AiProvider::CloudflareWorkersAi,
            pending_open_path: None,
            pending_save_as_path: None,
            build_errors_count: 0,
            account_usage: Vec::new(),
            usage_date: String::new(),
            chat: crate::editor::chat_panel::ChatPanelState {
                messages: Vec::new(),
                input: String::new(),
                agent_active: false,
                pending_approvals: Vec::new(),
                auto_approve: false,
                available_models: Vec::new(),
                selected_model: "@cf/moonshotai/kimi-k2.7-code".into(),
                thinking_enabled: false,
                thinking_supported: true,
                tools_supported: true,
                models_loading: false,
                show_thoughts: false,
            },
        };
        app.open_editor(None);
        let _ = app.agent_tx.send(UiToAgentMessage::RefreshModels);
        app
    }

    fn commands(&self) -> Vec<Command> {
        vec![
            Command { label: "Command Palette…", action: |a| a.open_command_palette() },
            Command { label: "New File", action: |a| a.open_editor(None) },
            Command { label: "Open File…", action: |a| a.open_file_dialog() },
            Command { label: "Save", action: |a| a.save_active() },
            Command { label: "Save As…", action: |a| a.save_active_as() },
            Command { label: "Save All", action: |a| a.save_all() },
            Command { label: "Close Tab", action: |a| a.close_active_tab() },
            Command { label: "Build", action: |a| a.build_active() },
            Command { label: "Run", action: |a| a.run_active() },
            Command { label: "Toggle Output", action: |a| a.toggle_panel(TabKind::Output) },
            Command { label: "Toggle Chat", action: |a| a.toggle_panel(TabKind::Chat) },
            Command { label: "Toggle Orchestrator", action: |a| a.toggle_panel(TabKind::Orchestrator) },
        ]
    }

    fn command_list_filtered(&self) -> Vec<Command> {
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

    fn close_active_tab(&mut self) {
        if let Some(id) = self.active_tab.take() {
            self.close_tab(&id);
        } else if let Some(first) = self.tabs.first().cloned() {
            self.close_tab(&first.id);
        }
    }

    fn close_tab(&mut self, id: &TabId) {
        self.tabs.retain(|t| t.id != *id);
        self.buffers.remove(id);
        if self.active_tab.as_ref() == Some(id) {
            self.active_tab = self.tabs.first().map(|t| t.id.clone());
        }
    }

    fn open_editor(&mut self, path: Option<PathBuf>) {
        if let Some(ref p) = path {
            // Check if we already have an open tab for this file
            for tab in &self.tabs {
                if let TabKind::Editor { path: Some(ref tab_path), .. } = tab.kind {
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

    fn open_file_dialog(&mut self) {
        // Use rfd if present; otherwise fall back to a simple text dialog.
        // For now we use a pending path input dialog.
        self.pending_open_path = Some(PathBuf::new());
    }

    fn save_active(&mut self) {
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

    fn save_active_as(&mut self) {
        if self.active_tab.is_some() {
            self.pending_save_as_path = Some(PathBuf::new());
        } else {
            self.status_message = "No active editor to save".into();
        }
    }

    fn save_buffer_to(&mut self, id: &TabId, path: &PathBuf) {
        if let Some(buf) = self.buffers.get(id) {
            match std::fs::write(path, buf.content()) {
                Ok(_) => self.status_message = format!("Saved {}", path.display()),
                Err(e) => self.status_message = format!("Error saving {}: {}", path.display(), e),
            }
        }
    }

    fn tab_path(&self, id: &TabId) -> Option<&PathBuf> {
        self.tabs.iter().find(|t| t.id == *id)?.editor_path()
    }

    fn save_all(&mut self) {
        let mut saved = 0usize;
        let ids: Vec<TabId> = self.tabs.iter().map(|t| t.id.clone()).collect();
        for id in ids {
            if let Some(path) = self.tab_path(&id).cloned() {
                self.save_buffer_to(&id, &path);
                saved += 1;
            }
        }
        self.status_message = format!("Saved {} buffers", saved);
    }

    fn toggle_panel(&mut self, kind: TabKind) {
        if self.tabs.iter().any(|t| std::mem::discriminant(&t.kind) == std::mem::discriminant(&kind)) {
            self.tabs.retain(|t| std::mem::discriminant(&t.kind) != std::mem::discriminant(&kind));
            self.rebuild_dock();
            self.active_tab = self.tabs.first().map(|t| t.id.clone());
        } else {
            let id = TabId::next(&mut self.tab_counter);
            let tab = Tab { id, kind };
            self.tabs.push(tab.clone());
            if let Some(dock) = self.dock_state.as_mut() {
                dock.push_to_focused_leaf(tab);
            }
        }
    }

    fn rebuild_dock(&mut self) {
        self.dock_state = Some(DockState::new(self.tabs.clone()));
    }

    fn build_active(&mut self) {
        self.command_output.push_str("$ cargo check\n");
        self.status_message = "Requested build via agent".into();
        self.agent_active = true;
        let _ = self
            .agent_tx
            .send(UiToAgentMessage::UserPrompt("Please run `cargo check` and report any errors.".into()));
    }

    fn run_active(&mut self) {
        self.command_output.push_str("$ cargo run\n");
        self.status_message = "Requested run via agent".into();
        self.agent_active = true;
        let _ = self
            .agent_tx
            .send(UiToAgentMessage::UserPrompt("Please run `cargo run` and report the result.".into()));
    }

    fn update_diagnostics(&mut self) {
        let diag = read_latest_diagnostics(&self.workspace_root);
        self.build_errors_count = if diag.success { 0 } else { diag.errors.len() };
    }
}

impl eframe::App for VelocityApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.apply_theme(&ctx);
        self.handle_agent_messages();
        self.handle_global_shortcuts(&ctx);
        self.update_diagnostics();

        // 1. Top Panel Toolbar with System Status & GPU Telemetry
        egui::Panel::top("toolbar").show(ui, |ui: &mut egui::Ui| {
            ui.horizontal(|ui: &mut egui::Ui| {
                ui.spacing_mut().item_spacing.x = 10.0;

                ui.label(egui::RichText::new("⚡ VELOCITY").size(15.0).strong().color(egui::Color32::from_rgb(168, 85, 247)));
                ui.label(egui::RichText::new("COGNITIVE IDE").size(11.0).color(egui::Color32::from_rgb(34, 211, 238)));
                ui.separator();

                let buttons: [(&str, fn(&mut VelocityApp)); 9] = [
                    ("➕ New", VelocityApp::open_editor_stub),
                    ("📂 Open", VelocityApp::open_file_dialog),
                    ("💾 Save", VelocityApp::save_active),
                    ("💾 Save As…", VelocityApp::save_active_as),
                    ("💾 Save All", VelocityApp::save_all),
                    ("⚙️ Build", VelocityApp::build_active),
                    ("▶ Run", VelocityApp::run_active),
                    ("💬 Chat", VelocityApp::toggle_chat),
                    ("🧠 Orchestrate", VelocityApp::toggle_orchestrator),
                ];
                for (label, action) in buttons {
                    if ui.button(label).clicked() {
                        action(self);
                    }
                }

                if ui.button("📺 Terminal").clicked() {
                    self.toggle_panel(TabKind::Output);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui: &mut egui::Ui| {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("🟢 GPU ACTIVE: MX250").size(11.0).color(egui::Color32::from_rgb(34, 211, 238)).strong());
                    ui.separator();
                    ui.label(egui::RichText::new(format!("🤖 Agent: {}", self.status_message)).size(11.0).color(egui::Color32::from_rgb(168, 85, 247)).strong());
                    
                    if self.build_errors_count > 0 {
                        ui.separator();
                        ui.label(
                            egui::RichText::new(format!("⚠️ Build Errors: {}", self.build_errors_count))
                                .size(11.0)
                                .color(egui::Color32::from_rgb(239, 68, 68))
                                .strong(),
                        );
                    } else {
                        ui.separator();
                        ui.label(
                            egui::RichText::new("✨ Build: OK")
                                .size(11.0)
                                .color(egui::Color32::from_rgb(34, 197, 94))
                                .strong(),
                        );
                    }
                });
            });
        });

        // 2. Left Side Panel: Interactive Project & File Tree Explorer
        egui::Panel::left("left_sidebar")
            .resizable(true)
            .default_size(240.0)
            .show(ui, |ui: &mut egui::Ui| {
                ui.add_space(4.0);
                ui.vertical(|ui: &mut egui::Ui| {
                    // --- Projects section ---
                    ui.horizontal(|ui: &mut egui::Ui| {
                        ui.label(egui::RichText::new("📁 PROJECTS").size(12.0).strong().color(egui::Color32::from_rgb(168, 85, 247)));
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if ui.button("➕ Register").on_hover_text("Register Project Directory").clicked() {
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
                                    self.status_message = "Path does not exist or is not a directory".into();
                                }
                            }
                        });
                    }

                    let active_name = self.workspace_root.file_name().unwrap_or_default().to_string_lossy().to_string();
                    egui::ComboBox::from_id_salt("project_combo")
                        .selected_text(active_name)
                        .show_ui(ui, |ui: &mut egui::Ui| {
                            let mut selected_idx = self.projects.iter().position(|p| p == &self.workspace_root);
                            for (idx, proj) in self.projects.iter().enumerate() {
                                let name = proj.file_name().unwrap_or_default().to_string_lossy().to_string();
                                if ui.selectable_value(&mut selected_idx, Some(idx), name).clicked() {
                                    let new_path = proj.clone();
                                    if new_path.is_dir() {
                                        self.workspace_root = new_path.clone();
                                        let _ = self.agent_tx.send(UiToAgentMessage::SetWorkspace(new_path.clone()));
                                        self.status_message = format!("Switched to {:?}", proj.file_name().unwrap_or_default());
                                    } else {
                                        self.status_message = format!("Failed to switch to {:?}", new_path);
                                    }
                                }
                            }
                        });

                    ui.separator();

                    ui.label(egui::RichText::new("🌲 FILE EXPLORER").size(12.0).strong().color(egui::Color32::from_rgb(168, 85, 247)));
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        render_file_tree(ui, &self.workspace_root.clone(), self);
                    });
                });
            });

        // 3. Central Docking Panels
        egui::CentralPanel::default().show(ui, |ui| {
            let mut dock_state = self.dock_state.take().expect("dock state");
            let mut viewer = TabViewerImpl { app: self };
            DockArea::new(&mut dock_state)
                .style(DockStyle::from_egui(ui.style().as_ref()))
                .show_inside(ui, &mut viewer);
            self.dock_state = Some(dock_state);
        });

        self.command_palette_ui(&ctx);
        self.file_dialog_ui(&ctx);
        self.save_as_dialog_ui(&ctx);
    }
}

fn render_file_tree(ui: &mut egui::Ui, path: &std::path::Path, app: &mut VelocityApp) {
    if let Ok(entries) = std::fs::read_dir(path) {
        let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
        entries.sort_by_key(|e| (e.file_type().map(|t| !t.is_dir()).unwrap_or(true), e.file_name()));

        for entry in entries {
            let entry_path = entry.path();
            let file_name = entry.file_name().to_string_lossy().to_string();

            if file_name.starts_with('.') || file_name == "target" || file_name == "node_modules" {
                continue;
            }

            if entry_path.is_dir() {
                ui.collapsing(format!("📁 {}", file_name), |ui| {
                    render_file_tree(ui, &entry_path, app);
                });
            } else {
                ui.horizontal(|ui| {
                    ui.label("📄");
                    if ui.selectable_label(false, &file_name).clicked() {
                        app.open_editor(Some(entry_path));
                    }
                });
            }
        }
    }
}

impl VelocityApp {
    fn open_editor_stub(&mut self) {
        self.open_editor(None);
    }

    fn toggle_chat(&mut self) {
        self.toggle_panel(TabKind::Chat);
    }

    fn toggle_orchestrator(&mut self) {
        self.toggle_panel(TabKind::Orchestrator);
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        let palette = IdePalette::dark();
        let mut visuals = egui::Visuals::dark();
        visuals.dark_mode = true;
        visuals.override_text_color = Some(palette.text);
        visuals.panel_fill = palette.bg_secondary;
        visuals.window_fill = palette.bg_primary;
        visuals.selection.bg_fill = palette.accent.gamma_multiply(0.25);
        visuals.selection.stroke.color = palette.text;
        visuals.window_stroke.color = palette.border;
        visuals.hyperlink_color = palette.accent;
        visuals.faint_bg_color = palette.bg_secondary;

        let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
        style.visuals = visuals;
        style.spacing.item_spacing = egui::Vec2::splat(6.0);
        style.spacing.button_padding = egui::Vec2::new(8.0, 4.0);
        ctx.set_global_style(style);
    }

    fn handle_global_shortcuts(&mut self, ctx: &egui::Context) {
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

    fn handle_agent_messages(&mut self) {
        while let Ok(msg) = self.agent_rx.try_recv() {
            match msg {
                AgentToUiMessage::OutputToken(token) => {
                    if self.agent_active {
                        self.chat_history.push_str("\nAgent: ");
                        self.agent_active = false;
                    }
                    self.chat_history.push_str(&token);
                    self.status_message = token.chars().take(80).collect();
                }
                AgentToUiMessage::ThoughtToken(_) => {}
                AgentToUiMessage::RequestToolApproval { id, tool_name, arguments } => {
                    self.command_output.push_str(&format!("[tool-approval-request] {}: {:?}\n", tool_name, arguments));
                    if self.auto_approve {
                        let _ = self.agent_tx.send(UiToAgentMessage::ApproveTool {
                            id,
                            tool_name,
                            arguments,
                        });
                    } else {
                        self.pending_approvals.push((id, tool_name, arguments));
                    }
                }
                AgentToUiMessage::ToolExecutionStarted { tool_name } => {
                    self.command_output.push_str(&format!("[tool-start] {}\n", tool_name));
                }
                AgentToUiMessage::ToolExecutionFinished { tool_name, result } => {
                    self.command_output
                        .push_str(&format!("[tool-finish] {}: {}\n", tool_name, result));
                }
                AgentToUiMessage::StatusUpdate(message) => {
                    if message.to_lowercase().contains("model catalog") {
                        self.models_loading = false;
                    }
                    self.status_message = message;
                }
                AgentToUiMessage::AgentFinished => {
                    self.status_message = "Agent finished".into();
                }
                AgentToUiMessage::UpdateFileBuffer { path, content } => {
                    self.open_editor(Some(path.clone()));
                    if let Some(id) = &self.active_tab {
                        if let Some(buf) = self.buffers.get_mut(id) {
                            buf.load_text(&content);
                        }
                    }
                }
                AgentToUiMessage::ModelCatalog { models, selected, thinking } => {
                    if let Some(model) = models.iter().find(|model| model.id == selected) {
                        self.thinking_supported = model.supports_thinking;
                        self.tools_supported = model.supports_tools;
                    }
                    self.available_models = models;
                    self.selected_model = selected;
                    self.thinking_enabled = thinking;
                    self.models_loading = false;
                }
                AgentToUiMessage::ProviderChanged(new_provider) => {
                    self.provider = new_provider;
                }
                AgentToUiMessage::AccountUsage { accounts, date } => {
                    self.account_usage = accounts;
                    self.usage_date = date;
                }
                AgentToUiMessage::ChatHistoryRestored(history) => {
                    for (role, content) in history {
                        if content.trim().is_empty() { continue; }
                        let prefix = if role == "user" { "You: " } else { "Agent: " };
                        self.chat_history.push_str(&format!("\n{}{}\n", prefix, content));
                    }
                }
            }
        }
        self.cap_logs();
    }

    fn cap_logs(&mut self) {
        const MAX: usize = 32_000;
        if self.command_output.len() > MAX {
            let cut = self.command_output.len() - MAX;
            self.command_output = self.command_output.split_off(cut);
        }
        if self.chat_history.len() > MAX {
            let cut = self.chat_history.len() - MAX;
            self.chat_history = self.chat_history.split_off(cut);
        }
    }

    fn command_palette_ui(&mut self, ctx: &egui::Context) {
        if !self.command_palette.open {
            return;
        }

        let area = egui::Area::new(egui::Id::new("command_palette_area"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_TOP, egui::Vec2::new(0.0, 80.0));

        let commands = self.command_list_filtered();
        let mut open = self.command_palette.open;

        // Clamp selected to the filtered list whenever the UI is shown.
        self.command_palette.selected =
            self.command_palette.selected.min(commands.len().saturating_sub(1));

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

                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for (idx, cmd) in commands.iter().enumerate() {
                            let selected = idx == self.command_palette.selected;
                            let text = egui::RichText::new(cmd.label)
                                .color(if selected {
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
                            self.command_palette.selected =
                                self.command_palette.selected.checked_sub(1).unwrap_or(commands.len() - 1);
                        }
                    } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        open = false;
                    }
                });
        });

        self.command_palette.open = open;
    }

    fn file_dialog_ui(&mut self, ctx: &egui::Context) {
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

    fn save_as_dialog_ui(&mut self, ctx: &egui::Context) {
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
                            // Update tab path so subsequent saves go to the same file.
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
}

struct TabViewerImpl<'a> {
    app: &'a mut VelocityApp,
}

impl<'a> TabViewer for TabViewerImpl<'a> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        tab.title().into()
    }

    fn on_close(&mut self, tab: &mut Self::Tab) -> egui_dock::tab_viewer::OnCloseResponse {
        self.app.close_tab(&tab.id);
        egui_dock::tab_viewer::OnCloseResponse::Close
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match &mut tab.kind {
            TabKind::Editor { buffer_id, .. } => {
                if let Some(buf) = self.app.buffers.get_mut(buffer_id) {
                    egui::Frame::new().inner_margin(egui::Margin::same(4)).show(ui, |ui: &mut egui::Ui| {
                        let mut editor = CodeEditor::new("code_editor");
                        editor.show(ui, buf.content_mut());
                    });
                }
            }
            TabKind::Chat => self.chat_panel(ui),
            TabKind::Output => self.output_panel(ui),
            TabKind::Orchestrator => {
                let mut panel = OrchestratorPanel::new();
                panel.ui(ui);
            }
            TabKind::Usage => {
                render_usage_panel(ui, &self.app.account_usage, &self.app.usage_date, || {
                    let _ = self.app.agent_tx.send(UiToAgentMessage::RefreshUsage);
                });
            }
        }
    }
}

impl<'a> TabViewerImpl<'a> {
    fn chat_panel(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new().inner_margin(egui::Margin::same(6)).show(ui, |ui: &mut egui::Ui| {
            let mut messages = Vec::new();
            let mut current_is_user = false;
            let mut current_chunk = String::new();

            for line in self.app.chat_history.lines() {
                if line.starts_with("You: ") {
                    if !current_chunk.trim().is_empty() {
                        messages.push((current_is_user, current_chunk.clone()));
                        current_chunk.clear();
                    }
                    current_is_user = true;
                    current_chunk.push_str(&line["You: ".len()..]);
                    current_chunk.push('\n');
                } else if line.starts_with("Agent: ") || line.starts_with("Antigravity: ") || line.starts_with("Kimi: ") {
                    if !current_chunk.trim().is_empty() {
                        messages.push((current_is_user, current_chunk.clone()));
                        current_chunk.clear();
                    }
                    current_is_user = false;
                    current_chunk.push_str(line);
                    current_chunk.push('\n');
                } else {
                    current_chunk.push_str(line);
                    current_chunk.push('\n');
                }
            }
            if !current_chunk.trim().is_empty() {
                messages.push((current_is_user, current_chunk));
            }

            ui.vertical(|ui: &mut egui::Ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Agent Chat").strong().size(16.0));
                    let state = if self.app.agent_active { "Working" } else { "Ready" };
                    ui.label(egui::RichText::new(state).small().color(if self.app.agent_active {
                        egui::Color32::from_rgb(250, 204, 21)
                    } else {
                        egui::Color32::from_rgb(74, 222, 128)
                    }));
                });
                ui.horizontal_wrapped(|ui| {
                    // --- Provider selector ---
                    ui.label(egui::RichText::new("Provider").small().weak());
                    let providers = [AiProvider::CloudflareWorkersAi, AiProvider::OpenRouter];
                    let current_label = self.app.provider.label();
                    egui::ComboBox::from_id_salt("agent_provider")
                        .selected_text(current_label)
                        .width(180.0)
                        .show_ui(ui, |ui| {
                            for p in providers {
                                let selected = self.app.provider == p;
                                if ui.selectable_label(selected, p.label()).clicked() && !selected {
                                    let _ = self.app.agent_tx.send(UiToAgentMessage::SetProvider(p));
                                    self.app.models_loading = true;
                                    // Kick off model refresh for new provider
                                    let _ = self.app.agent_tx.send(UiToAgentMessage::RefreshModels);
                                }
                            }
                        });
                    ui.separator();
                    // --- Model selector ---
                    ui.label(egui::RichText::new("Model").small().weak());
                    let mut model_changed = false;
                    egui::ComboBox::from_id_salt("agent_model")
                        .selected_text(&self.app.selected_model)
                        .width(300.0)
                        .show_ui(ui, |ui| {
                            for model in self.app.available_models.clone() {
                                model_changed |= ui.selectable_value(&mut self.app.selected_model, model.id.clone(), model.label).changed();
                            }
                        });
                    if model_changed {
                        let _ = self.app.agent_tx.send(UiToAgentMessage::SetModel(self.app.selected_model.clone()));
                    }
                    if ui.button(if self.app.models_loading { "Loading…" } else { "↻ Models" }).clicked() && !self.app.models_loading {
                        self.app.models_loading = true;
                        let _ = self.app.agent_tx.send(UiToAgentMessage::RefreshModels);
                    }
                    let thinking_changed = ui
                        .add_enabled(self.app.thinking_supported, egui::Checkbox::new(&mut self.app.thinking_enabled, "Thinking"))
                        .changed();
                    if thinking_changed {
                        let _ = self.app.agent_tx.send(UiToAgentMessage::SetThinking(self.app.thinking_enabled));
                    }
                    if !self.app.thinking_supported {
                        ui.label(egui::RichText::new("unsupported by model").small().weak());
                    }
                });
                ui.separator();
                let scroll_height = ui.available_height() - 150.0;
                egui::ScrollArea::vertical()
                    .max_height(scroll_height)
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui: &mut egui::Ui| {
                        ui.spacing_mut().item_spacing.y = 10.0;
                        for (is_user, text) in messages {
                            let (bg_color, border_color, align) = if is_user {
                                (
                                    egui::Color32::from_rgb(45, 25, 78),
                                    egui::Color32::from_rgb(168, 85, 247),
                                    egui::Align::RIGHT,
                                )
                            } else {
                                (
                                    egui::Color32::from_rgb(20, 22, 34),
                                    egui::Color32::from_rgb(38, 41, 62),
                                    egui::Align::LEFT,
                                )
                            };

                            ui.with_layout(egui::Layout::top_down(align), |ui: &mut egui::Ui| {
                                egui::Frame::new()
                                    .fill(bg_color)
                                    .stroke(egui::Stroke::new(1.0, border_color))
                                    .corner_radius(egui::CornerRadius::same(8))
                                    .inner_margin(egui::Margin::symmetric(14, 10))
                                    .show(ui, |ui: &mut egui::Ui| {
                                        ui.set_max_width(ui.available_width() * 0.85);
                                        ui.label(egui::RichText::new(text.trim()).size(13.0));
                                    });
                            });
                        }
                    });

                ui.separator();

                ui.horizontal(|ui: &mut egui::Ui| {
                    // Auto-approve works for both structured and inline tool calls,
                    // so always enable the checkbox regardless of tools_supported.
                    ui.checkbox(&mut self.app.auto_approve, "Auto-approve tools");
                    if !self.app.tools_supported {
                        ui.label(egui::RichText::new("(inline tool calling)").small().weak());
                    }
                    ui.label(egui::RichText::new("Thinking is model-dependent").small().weak());
                });

                if !self.app.pending_approvals.is_empty() {
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(12, 10, 20))
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(234, 179, 8)))
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui: &mut egui::Ui| {
                            ui.vertical(|ui: &mut egui::Ui| {
                                ui.colored_label(egui::Color32::from_rgb(234, 179, 8), "⚠️ Pending Tool Approvals");
                                
                                let pending = self.app.pending_approvals.clone();
                                for (id, tool_name, arguments) in pending {
                                    ui.group(|ui: &mut egui::Ui| {
                                        ui.horizontal(|ui: &mut egui::Ui| {
                                            ui.label(egui::RichText::new(format!("Tool: {}", tool_name)).strong().color(egui::Color32::LIGHT_BLUE));
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui: &mut egui::Ui| {
                                                if ui.button("Decline ❌").clicked() {
                                                    let _ = self.app.agent_tx.send(UiToAgentMessage::RejectTool {
                                                        id: id.clone(),
                                                        tool_name: tool_name.clone(),
                                                    });
                                                    self.app.pending_approvals.retain(|(p_id, _, _)| p_id != &id);
                                                }
                                                if ui.button("Approve ✔️").clicked() {
                                                    let _ = self.app.agent_tx.send(UiToAgentMessage::ApproveTool {
                                                        id: id.clone(),
                                                        tool_name: tool_name.clone(),
                                                        arguments: arguments.clone(),
                                                    });
                                                    self.app.pending_approvals.retain(|(p_id, _, _)| p_id != &id);
                                                }
                                            });
                                        });
                                        ui.label(egui::RichText::new(format!("Arguments: {}", arguments)).size(11.0));
                                    });
                                }
                            });
                        });
                }

                ui.separator();

                ui.horizontal(|ui: &mut egui::Ui| {
                    let input_width = ui.available_width() - 85.0;
                    let response = ui.add(
                        egui::TextEdit::multiline(&mut self.app.chat_input)
                            .desired_width(input_width)
                            .desired_rows(2)
                            .hint_text("Type instructions for the agent…"),
                    );
                    
                    let enter_pressed = response.lost_focus() && ui.input(|i: &egui::InputState| i.key_pressed(egui::Key::Enter));
                    if ui.button("Send 🚀").clicked() || enter_pressed {
                        let text = self.app.chat_input.clone();
                        if !text.is_empty() {
                            self.app.chat_history.push_str(&format!("\nYou: {}\n", text));
                            self.app.chat_input.clear();
                            self.app.agent_active = true;
                            let _ = self
                                .app
                                .agent_tx
                                .send(UiToAgentMessage::UserPrompt(text));
                        }
                    }
                });
            });
        });
    }

    fn output_panel(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .inner_margin(egui::Margin::same(10))
            .fill(egui::Color32::from_rgb(7, 8, 12))
            .show(ui, |ui: &mut egui::Ui| {
                ui.vertical(|ui: &mut egui::Ui| {
                    let scroll_height = ui.available_height() - 35.0;
                    egui::ScrollArea::vertical()
                        .max_height(scroll_height)
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui: &mut egui::Ui| {
                            let mut text = self.app.command_output.clone();
                            ui.add(
                                egui::TextEdit::multiline(&mut text)
                                    .code_editor()
                                    .font(egui::FontId::monospace(13.0))
                                    .desired_width(f32::INFINITY)
                                    .text_color(egui::Color32::from_rgb(34, 211, 238)),
                            );
                        });

                    ui.separator();

                    ui.horizontal(|ui: &mut egui::Ui| {
                        if ui.button("🗑 Clear Console").clicked() {
                            self.app.command_output.clear();
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui: &mut egui::Ui| {
                            ui.label(egui::RichText::new(format!("Buffer: {} bytes", self.app.command_output.len())).small().weak());
                        });
                    });
                });
            });
    }
}
