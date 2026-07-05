//! Main editor UI: dockable panels, theme, command palette.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use egui::{Align, Color32, Frame, Id, Key, Layout, Stroke, TextEdit, TopBottomPanel, Ui, Vec2};
use egui::{CentralPanel, SidePanel};
use egui::{Align2, RichText, WidgetText};
use egui_dock::{DockArea, DockState, NodeIndex, SurfaceIndex, TabViewer};

use crate::agent::{AgentToUiMessage, UiToAgentMessage};

use super::buffer::EditorBuffer;
use super::code_editor::CodeEditor;
use super::theme::{self, IdePalette};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TabKind {
    Editor,
    Chat,
    Output,
    Diff,
    Search,
    Terminal,
    Settings,
}

#[derive(Clone, Debug)]
struct Tab {
    title: String,
    kind: TabKind,
    buf_id: Option<usize>,
}

impl Tab {
    fn editor(id: usize, path: Option<&Path>) -> Self {
        Self {
            title: path
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "untitled".to_string()),
            kind: TabKind::Editor,
            buf_id: Some(id),
        }
    }
}

pub struct VelocityApp {
    pub root_path: PathBuf,
    open_tabs: Vec<Tab>,
    active_tab: usize,
    buffers: BTreeMap<usize, EditorBuffer>,
    next_buf_id: usize,
    file_tree: Vec<(PathBuf, bool)>, // path, is_dir

    // Docking layout
    dock_state: DockState<Tab>,

    // Agent / compiler / tool state
    pub chat_input: String,
    pub chat_history: Vec<(String, String)>, // role, text
    pub command_output: String,
    pub status_message: String,
    pub is_building: bool,
    pub agent_tx: crossbeam_channel::Sender<UiToAgentMessage>,
    pub agent_rx: crossbeam_channel::Receiver<AgentToUiMessage>,

    // Toggles
    pub show_sidebar: bool,
    pub show_statusbar: bool,
    pub show_minimap: bool,
    pub show_command_palette: bool,

    // Search
    pub search_query: String,
    pub replace_query: String,
    pub case_sensitive: bool,
    pub regex_mode: bool,

    // Settings
    pub font_size: f32,

    // Command palette
    palette_query: String,
    palette_selected: usize,
}

impl VelocityApp {
    pub fn new(
        workspace_root: PathBuf,
        agent_tx: crossbeam_channel::Sender<UiToAgentMessage>,
        agent_rx: crossbeam_channel::Receiver<AgentToUiMessage>,
    ) -> Self {
        let open_tabs = vec![Tab::editor(0, None)];
        let mut buffers = BTreeMap::new();
        buffers.insert(0, EditorBuffer::new(None, String::new()));
        let mut dock_state = DockState::new(open_tabs.clone());
        let surface = SurfaceIndex::main();
        let node = NodeIndex::root();
        
        let [_, chat_node] = dock_state.split(
            (surface, node),
            egui_dock::Split::Right,
            0.75,
            egui_dock::Node::leaf(Tab {
                title: "Chat".to_string(),
                kind: TabKind::Chat,
                buf_id: None,
            }),
        );
        dock_state.split(
            (surface, chat_node),
            egui_dock::Split::Below,
            0.70,
            egui_dock::Node::leaf(Tab {
                title: "Output".to_string(),
                kind: TabKind::Output,
                buf_id: None,
            }),
        );

        let mut app = Self {
            root_path: workspace_root,
            open_tabs,
            active_tab: 0,
            buffers,
            next_buf_id: 1,
            file_tree: Vec::new(),
            dock_state,
            chat_input: String::new(),
            chat_history: vec![(
                "assistant".to_string(),
                "Welcome to V.E.L.O.C.I.T.Y. IDE. Ask me to explain, refactor, or generate code.".to_string(),
            )],
            command_output: "MCP server ready.".to_string(),
            status_message: "Ready".to_string(),
            is_building: false,
            agent_tx,
            agent_rx,
            show_sidebar: true,
            show_statusbar: true,
            show_minimap: false,
            show_command_palette: false,
            search_query: String::new(),
            replace_query: String::new(),
            case_sensitive: false,
            regex_mode: false,
            font_size: 14.0,
            palette_query: String::new(),
            palette_selected: 0,
        };
        app.refresh_file_tree();
        app
    }
}
impl VelocityApp {
    fn send_chat(&mut self) {
        let q = self.chat_input.trim().to_string();
        if q.is_empty() {
            return;
        }
        self.chat_history.push(("user".to_string(), q.clone()));
        self.chat_input.clear();
        if let Err(e) = self.agent_tx.send(crate::agent::UiToAgentMessage::UserPrompt(q)) {
            self.chat_history.push((
                "assistant".to_string(),
                format!("[Error sending query: {:?}]", e),
            ));
        }
    }

    fn handle_agent_messages(&mut self) {
        while let Ok(msg) = self.agent_rx.try_recv() {
            match msg {
                crate::agent::AgentToUiMessage::ThoughtToken(_token) => {}
                crate::agent::AgentToUiMessage::OutputToken(token) => {
                    if let Some((role, text)) = self.chat_history.last_mut() {
                        if role == "assistant" {
                            text.push_str(&token);
                        } else {
                            self.chat_history.push(("assistant".to_string(), token));
                        }
                    } else {
                        self.chat_history.push(("assistant".to_string(), token));
                    }
                }
                crate::agent::AgentToUiMessage::StatusUpdate(status) => {
                    self.status_message = status;
                }
                crate::agent::AgentToUiMessage::RequestToolApproval { id, tool_name, arguments } => {
                    self.agent_tx.send(crate::agent::UiToAgentMessage::ApproveTool { id, tool_name, arguments }).ok();
                }
                crate::agent::AgentToUiMessage::ToolExecutionStarted { tool_name } => {
                    self.command_output.push_str(&format!("[Running {}...]\n", tool_name));
                }
                crate::agent::AgentToUiMessage::ToolExecutionFinished { tool_name, result } => {
                    self.command_output.push_str(&format!("[Finished {}: {}]\n", tool_name, result));
                }
                crate::agent::AgentToUiMessage::UpdateFileBuffer { path, content } => {
                    if let Some(buf) = self.buffers.values_mut().find(|b| b.path.as_ref() == Some(&path)) {
                        buf.update_content(content);
                    }
                }
                crate::agent::AgentToUiMessage::AgentFinished => {
                    self.status_message = "Agent execution complete".to_string();
                }
            }
        }
    }

    pub fn refresh_file_tree(&mut self) {
        self.file_tree.clear();
        fn visit(entries: &mut Vec<(PathBuf, bool)>, base: &Path) {
            let _ = std::fs::read_dir(base).map(|entries_iter| {
                let mut collected: Vec<_> = entries_iter.filter_map(|e| e.ok()).collect();
                collected.sort_by(|a, b| {
                    let a_dir = a.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                    let b_dir = b.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                    b_dir.cmp(&a_dir).then_with(|| a.file_name().cmp(&b.file_name()))
                });
                for entry in collected {
                    let path = entry.path();
                    let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                    entries.push((path.clone(), is_dir));
                    if is_dir {
                        visit(entries, &path);
                    }
                }
            });
        }
        let root = self.root_path.clone();
        visit(&mut self.file_tree, &root);
    }

    fn open_file(&mut self, path: &Path) {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let id = self.next_buf_id;
        self.next_buf_id += 1;
        let buf = EditorBuffer::new(Some(path.to_path_buf()), text);
        self.buffers.insert(id, buf);
        let tab = Tab::editor(id, Some(path));
        self.open_tabs.push(tab.clone());
        self.active_tab = self.open_tabs.len() - 1;
        self.dock_state.push_to_focused_leaf(tab);
        self.status_message = format!("Opened {}", path.display());
    }

    fn save_active(&mut self) {
        if let Some(tab) = self.open_tabs.get(self.active_tab) {
            if let Some(id) = tab.buf_id {
                if let Some(buf) = self.buffers.get_mut(&id) {
                    if let Err(e) = buf.save() {
                        self.command_output.push_str(&format!("Error saving buffer: {}\n", e));
                    } else {
                        self.command_output.push_str("Saved active buffer.\n");
                        self.status_message = "Saved".to_string();
                    }
                }
            }
        }
    }

    fn build_active(&mut self) {
        self.is_building = true;
        self.command_output.push_str("Starting build...\n");
        self.status_message = "Building...".to_string();
        
        let output = std::process::Command::new("cargo")
            .arg("build")
            .output();
            
        match output {
            Ok(out) => {
                self.command_output.push_str(&String::from_utf8_lossy(&out.stdout));
                self.command_output.push_str(&String::from_utf8_lossy(&out.stderr));
            }
            Err(e) => {
                self.command_output.push_str(&format!("Failed to execute cargo build: {:?}\n", e));
            }
        }
        self.is_building = false;
    }

    fn run_active(&mut self) {
        self.command_output.push_str("Running...\n");
        self.status_message = "Running...".to_string();
        
        let output = std::process::Command::new("cargo")
            .arg("run")
            .output();
            
        match output {
            Ok(out) => {
                self.command_output.push_str(&String::from_utf8_lossy(&out.stdout));
                self.command_output.push_str(&String::from_utf8_lossy(&out.stderr));
            }
            Err(e) => {
                self.command_output.push_str(&format!("Failed to execute cargo run: {:?}\n", e));
            }
        }
        self.status_message = "Run finished".to_string();
    }

    pub fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_agent_messages();
        self.handle_global_keys(ctx);

        TopBottomPanel::top("toolbar_panel").show(ctx, |ui| {
            self.render_top_toolbar(ui);
        });

        if self.show_sidebar {
            SidePanel::left("sidebar")
                .resizable(true)
                .default_width(220.0)
                .width_range(160.0..=400.0)
                .show(ctx, |ui| {
                    self.render_sidebar(ui);
                });
        }

        if self.show_statusbar {
            TopBottomPanel::bottom("statusbar")
                .resizable(false)
                .default_height(26.0)
                .show(ctx, |ui| {
                    self.render_statusbar(ui);
                });
        }

        CentralPanel::default().show(ctx, |ui| {
            let mut dock_state = std::mem::replace(&mut self.dock_state, egui_dock::DockState::new(vec![]));
            DockArea::new(&mut dock_state)
                .style(egui_dock::Style::from_egui(ui.style().as_ref()))
                .show_add_buttons(true)
                .show_inside(ui, &mut TabViewerImpl { app: self });
            self.dock_state = dock_state;
        });

        if self.show_command_palette {
            self.render_command_palette(ctx);
        }
    }

    // ------------------------------------------------------------------
    // Panels
    // ------------------------------------------------------------------

    fn render_top_toolbar(&mut self, ui: &mut Ui) {
        let p = IdePalette::default();
        ui.horizontal(|ui| {
            ui.style_mut().spacing.item_spacing = Vec2::new(8.0, 0.0);
            ui.label(
                RichText::new("V.E.L.O.C.I.T.Y.")
                    .color(p.text_muted)
                    .font(theme::ui_font_id(16.0)),
            );
            ui.separator();

            if ui.button("📂 Open").clicked() {
                self.status_message = "Open file dialog triggered".to_string();
            }
            if ui.button("💾 Save (Ctrl+S)").clicked() {
                self.save_active();
            }
            if ui.button("🔨 Build (Ctrl+B)").clicked() {
                self.build_active();
            }
            if ui.button("▶ Run (Ctrl+R)").clicked() {
                self.run_active();
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.toggle_value(&mut self.show_command_palette, "⌘K Command Palette");
                ui.toggle_value(&mut self.show_sidebar, " Sidebar");
                ui.toggle_value(&mut self.show_minimap, " Minimap");
            });
        });
        ui.painter().hline(
            ui.min_rect().x_range(),
            ui.min_rect().max.y,
            Stroke::new(1.0, p.border),
        );
    }

    fn render_sidebar(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("EXPLORER")
                        .color(IdePalette::default().text_muted)
                        .font(theme::ui_font_id(11.0)),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("⟳").clicked() {
                        self.refresh_file_tree();
                    }
                });
            });
            ui.add_space(4.0);

            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut file_to_open = None;
                for (path, is_dir) in &self.file_tree {
                    let depth = path
                        .ancestors()
                        .filter(|p| p != &self.root_path && p.starts_with(&self.root_path))
                        .count();
                    let indent = ui.spacing().indent * depth as f32;
                    ui.horizontal(|ui| {
                        ui.add_space(indent);
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        let icon = if *is_dir { "📁" } else { "📄" };
                        let label = ui.selectable_label(false, format!("{} {}", icon, name));
                        if label.double_clicked() && !is_dir {
                            file_to_open = Some(path.clone());
                        }
                    });
                }
                if let Some(path) = file_to_open {
                    self.open_file(&path);
                }
            });

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("SEARCH")
                        .color(IdePalette::default().text_muted)
                        .font(theme::ui_font_id(11.0)),
                );
            });
            ui.add_space(4.0);
            ui.add(TextEdit::singleline(&mut self.search_query).hint_text("Find..."));
            ui.add(TextEdit::singleline(&mut self.replace_query).hint_text("Replace..."));
            ui.checkbox(&mut self.case_sensitive, "Match case");
            ui.checkbox(&mut self.regex_mode, "Regex");
        });
    }

    fn render_statusbar(&mut self, ui: &mut Ui) {
        let p = IdePalette::default();
        ui.horizontal(|ui| {
            ui.style_mut().spacing.item_spacing = Vec2::new(12.0, 0.0);
            ui.label(
                RichText::new(&self.status_message)
                    .color(p.text)
                    .font(theme::ui_font_id(12.0)),
            );

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.is_building {
                    ui.spinner();
                }
                ui.label(
                    RichText::new("UTF-8")
                        .color(p.text_muted)
                        .font(theme::ui_font_id(11.0)),
                );
                ui.label(
                    RichText::new("Rust")
                        .color(p.text_muted)
                        .font(theme::ui_font_id(11.0)),
                );
                ui.label(
                    RichText::new("Ln 0, Col 0")
                        .color(p.text_muted)
                        .font(theme::ui_font_id(11.0)),
                );
            });
        });
        ui.painter().hline(
            ui.min_rect().x_range(),
            ui.min_rect().min.y,
            Stroke::new(1.0, p.border),
        );
    }

    fn render_command_palette(&mut self, ctx: &egui::Context) {
        let commands = self.command_list();
        let filtered: Vec<_> = commands
            .into_iter()
            .enumerate()
            .filter(|(_, c)| {
                c.0.to_lowercase()
                    .contains(&self.palette_query.to_lowercase())
            })
            .collect();

        egui::Area::new(Id::new("command_palette"))
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 80.0))
            .show(ctx, |ui| {
                let p = IdePalette::default();
                let width = 560.0;
                Frame::popup(ui.style())
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0, p.border))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.add(
                            TextEdit::singleline(&mut self.palette_query)
                                .hint_text("Type a command... (Esc to close)")
                                .desired_width(width - 16.0),
                        );
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                            for (idx, (name, _label, _)) in &filtered {
                                let selected = *idx == self.palette_selected;
                                let response =
                                    ui.selectable_label(selected, format!("{}\n  {}", name, _label));
                                if response.clicked() {
                                    self.run_command_by_index(*idx);
                                    self.show_command_palette = false;
                                }
                            }
                        });
                    });
            });

        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.show_command_palette = false;
        }
        if ctx.input(|i| i.key_pressed(Key::ArrowDown)) {
            self.palette_selected = (self.palette_selected + 1).min(filtered.len().saturating_sub(1));
        }
        if ctx.input(|i| i.key_pressed(Key::ArrowUp)) {
            self.palette_selected = self.palette_selected.saturating_sub(1);
        }
        if ctx.input(|i| i.key_pressed(Key::Enter)) {
            if let Some((idx, _)) = filtered.get(self.palette_selected) {
                self.run_command_by_index(*idx);
                self.show_command_palette = false;
            }
        }
        if ctx.input(|i| i.key_pressed(Key::Tab)) {
            if let Some((_, (name, _, _))) = filtered.get(self.palette_selected) {
                self.palette_query = name.to_string();
            }
        }
    }

    fn command_list(&self) -> Vec<(&'static str, &'static str, Box<dyn Fn(&mut Self)>)> {
        vec![
            ("Open File", "Open an existing file from disk", Box::new(|app| app.status_message = "Open file dialog triggered".to_string())),
            ("Save", "Save the active editor buffer", Box::new(|app| app.save_active())),
            ("Build", "Compile the current project", Box::new(|app| app.build_active())),
            ("Run", "Run the compiled result", Box::new(|app| app.run_active())),
            ("Toggle Sidebar", "Show or hide the left sidebar", Box::new(|app| app.show_sidebar = !app.show_sidebar)),
            ("Toggle Statusbar", "Show or hide the status bar", Box::new(|app| app.show_statusbar = !app.show_statusbar)),
            ("Toggle Minimap", "Show or hide the minimap", Box::new(|app| app.show_minimap = !app.show_minimap)),
            ("Refresh File Tree", "Rescan the workspace explorer", Box::new(|app| app.refresh_file_tree())),
        ]
    }

    fn run_command_by_index(&mut self, index: usize) {
        let commands = self.command_list();
        if let Some((_, _, f)) = commands.into_iter().nth(index) {
            f(self);
        }
    }

    fn handle_global_keys(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            if i.modifiers.command && i.key_pressed(Key::P) {
                self.show_command_palette = !self.show_command_palette;
            }
            if i.modifiers.command && i.key_pressed(Key::O) {
                self.status_message = "Open file dialog triggered".to_string();
            }
            if i.modifiers.command && i.key_pressed(Key::S) {
                self.save_active();
            }
            if i.modifiers.command && i.key_pressed(Key::B) {
                self.build_active();
            }
            if i.modifiers.command && i.key_pressed(Key::R) {
                self.run_active();
            }
            if i.modifiers.command && i.key_pressed(Key::K) {
                self.show_command_palette = !self.show_command_palette;
            }
            if i.modifiers.ctrl && i.key_pressed(Key::W) {
                // close active tab
                if !self.open_tabs.is_empty() {
                    self.open_tabs.remove(self.active_tab.min(self.open_tabs.len() - 1));
                    self.active_tab = self.active_tab.min(self.open_tabs.len().saturating_sub(1));
                }
            }
        });
    }

    fn render_editor_tab(&mut self, ui: &mut Ui, tab: &Tab) {
        let id = tab.buf_id.unwrap_or(0);
        if let Some(buf) = self.buffers.get_mut(&id) {
            let path = buf.path.clone();
            let mut editor = CodeEditor::new(id, path.as_deref());
            let _ = editor.show(ui, &mut buf.content);
        }
    }

    fn render_chat_tab(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            ui.add_space(4.0);
            ui.label(
                RichText::new("AI Agent")
                    .color(IdePalette::default().text)
                    .font(theme::ui_font_id(14.0)),
            );
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (role, text) in &self.chat_history {
                    let color = if role == "user" {
                        IdePalette::default().accent
                    } else {
                        IdePalette::default().text
                    };
                    ui.label(
                        RichText::new(format!("{}: {}", role, text))
                            .color(color)
                            .font(theme::ui_font_id(13.0)),
                    );
                    ui.add_space(6.0);
                }
            });
            ui.horizontal(|ui| {
                let response = ui.add(
                    TextEdit::singleline(&mut self.chat_input)
                        .hint_text("Ask the agent...")
                        .desired_width(ui.available_width() - 60.0),
                );
                if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    self.send_chat();
                }
                if ui.button("Send").clicked() {
                    self.send_chat();
                }
            });
        });
    }

    fn render_output_tab(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            ui.add_space(4.0);
            ui.label(
                RichText::new("Compiler Output")
                    .color(IdePalette::default().text)
                    .font(theme::ui_font_id(14.0)),
            );
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add(
                    TextEdit::multiline(&mut self.command_output)
                        .font(theme::code_font_id(13.0))
                        .code_editor()
                        .desired_width(f32::INFINITY),
                );
            });
        });
    }

    #[allow(dead_code)]
    fn render_diff_tab(&mut self, ui: &mut Ui) {
        ui.centered_and_justified(|ui| {
            ui.label(
                RichText::new("Diff view placeholder")
                    .color(IdePalette::default().text_muted)
                    .font(theme::ui_font_id(14.0)),
            );
        });
    }

    #[allow(dead_code)]
    fn render_search_tab(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            ui.add_space(4.0);
            ui.label(
                RichText::new("Global Search")
                    .color(IdePalette::default().text)
                    .font(theme::ui_font_id(14.0)),
            );
            ui.separator();
            ui.label("Search across workspace files is not yet implemented.");
        });
    }

    #[allow(dead_code)]
    fn render_terminal_tab(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            ui.add_space(4.0);
            ui.label(
                RichText::new("Terminal")
                    .color(IdePalette::default().text)
                    .font(theme::ui_font_id(14.0)),
            );
            ui.separator();
            ui.label("Integrated terminal is not yet implemented.");
        });
    }

    fn render_settings_tab(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            ui.add_space(4.0);
            ui.label(
                RichText::new("Settings")
                    .color(IdePalette::default().text)
                    .font(theme::ui_font_id(14.0)),
            );
            ui.separator();
            ui.add(egui::Slider::new(&mut self.font_size, 8.0..=32.0).text("Editor font size"));
            if ui.button("Apply Theme").clicked() {
                 theme::setup_custom_style(ui.ctx());
            }
        });
    }
}

// ------------------------------------------------------------------
// Tab viewer for egui_dock
// ------------------------------------------------------------------
struct TabViewerImpl<'a> {
    app: &'a mut VelocityApp,
}

impl<'a> TabViewer for TabViewerImpl<'a> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        RichText::new(&tab.title).font(theme::ui_font_id(13.0)).into()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match tab.kind {
            TabKind::Editor => self.app.render_editor_tab(ui, tab),
            TabKind::Chat => self.app.render_chat_tab(ui),
            TabKind::Output => self.app.render_output_tab(ui),
            TabKind::Diff => self.app.render_diff_tab(ui),
            TabKind::Search => self.app.render_search_tab(ui),
            TabKind::Terminal => self.app.render_terminal_tab(ui),
            TabKind::Settings => self.app.render_settings_tab(ui),
        }
    }

    fn closeable(&mut self, _tab: &mut Self::Tab) -> bool {
        _tab.kind != TabKind::Editor || self.app.open_tabs.len() > 1
    }

    fn on_close(&mut self, tab: &mut Self::Tab) -> bool {
        if let Some(pos) = self.app.open_tabs.iter().position(|t| t.kind == tab.kind && t.title == tab.title) {
            self.app.open_tabs.remove(pos);
        }
        true
    }
}

// ------------------------------------------------------------------
// eframe integration
// ------------------------------------------------------------------
impl eframe::App for VelocityApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.update(ctx, frame);
    }
}
