use crate::agent::{AgentToUiMessage, ModelInfo, UiToAgentMessage, AiProvider};
use crate::automation::{read_latest_diagnostics, AgentTaskKind, WorkspaceCoordinator};
use crate::editor::buffer::EditorBuffer;
use crate::editor::chat_panel::{ChatPanelState, render_chat_panel};
use crate::editor::code_editor::CodeEditor;
use crate::editor::orchestrator_panel::OrchestratorPanel;
use crate::editor::theme::IdePalette;
use crate::editor::usage_panel::{render_usage_compact, render_usage_panel};
use crate::editor::agent_ui_state::AgentUiState;
use crate::editor::agent_ui_render::{RenderSnapshot, render_thinking_panel, render_pending_approvals, render_agent_metrics};
use crate::editor::task_timeline::{TaskTimelineState as TTState, TaskTimelineSnapshot, render_task_timeline};
use crate::editor::smart_sidebar::{SmartSidebarSnapshot, SmartSidebarState, render_smart_sidebar};
use crate::usage::AccountUsageView;
use crossbeam_channel::{Receiver, Sender};
use eframe::egui;
use egui_dock::{DockArea, DockState, Style as DockStyle, TabViewer};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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
            TabKind::Search => "Search".into(),
            TabKind::Graph => "Merkle Graph".into(),
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
    Search,
    Graph,
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

struct ActiveChangePreview {
    file_label: String,
    added_lines: usize,
    removed_lines: usize,
    preview: String,
    full_diff: String,
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

    // Agentic UI State (Phase 1 - Zero-allocation)
    agent_ui_state: AgentUiState,
    // Task Timeline (Phase 2)
    task_timeline: TTState,
    // Smart Sidebar (Phase 2)
    smart_sidebar: SmartSidebarState,

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
    pub show_full_diff: bool,
    pub build_errors_count: usize,
    pub gpu_name: String,
    pub search_query: String,
    pub search_hits: Vec<crate::editor::search::SearchHit>,
    pub pending_cursor_line: Option<usize>,
    pub file_tree: Option<FileNode>,
    pub last_tree_update: std::time::Instant,
    pub toasts: crate::editor::toast::ToastQueue,
    pub orchestrator: crate::editor::orchestrator_panel::OrchestratorPanel,

    pub mediator: std::sync::Arc<crate::automation::mediator::MediatorArena>,
    pub graph_view: crate::editor::graph_view::MerkleGraphView,
    pub terminal_rx: Option<std::sync::mpsc::Receiver<String>>,
    pub terminal_input: String,
    pub current_agent_task_id: u32,
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
        gpu_name: String,
        mediator: std::sync::Arc<crate::automation::mediator::MediatorArena>,
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
        let graph = Tab {
            id: TabId::next(&mut tab_counter),
            kind: TabKind::Graph,
        };
        let tabs = vec![output.clone(), chat.clone(), orchestrator.clone(), graph.clone()];

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
                    dock_state: Some(DockState::new(vec![output, chat, orchestrator, graph])),
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
            agent_ui_state: AgentUiState::default(),
            task_timeline: TTState::default(),
                    smart_sidebar: SmartSidebarState::default(),
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
            show_full_diff: false,
            build_errors_count: 0,
            account_usage: Vec::new(),
            usage_date: String::new(),
            gpu_name,
            search_query: String::new(),
            search_hits: Vec::new(),
            pending_cursor_line: None,
            file_tree: None,
            last_tree_update: std::time::Instant::now(),
            toasts: crate::editor::toast::ToastQueue::default(),
            orchestrator: crate::editor::orchestrator_panel::OrchestratorPanel::new(),
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
            mediator,
            graph_view: crate::editor::graph_view::MerkleGraphView::new(),
            terminal_rx: None,
            terminal_input: String::new(),
            current_agent_task_id: 0,
        };
        app.open_editor(None);
        app.task_timeline.session_marker("IDE session ready", "agentic workspace initialized");
        let _ = app.agent_tx.send(UiToAgentMessage::RefreshModels);
        app
    }

    fn commands(&self) -> Vec<Command> {
        vec![
            Command { label: "Command Palette…", action: |a| a.open_command_palette() },
            Command { label: "Refresh Models", action: |a| a.refresh_models() },
            Command { label: "Approve All Pending Tools", action: |a| a.approve_all_pending_tools() },
            Command { label: "Decline All Pending Tools", action: |a| a.reject_all_pending_tools() },
            Command { label: "Focus Agent Chat", action: |a| a.toggle_panel(TabKind::Chat) },
            Command { label: "Focus Orchestrator", action: |a| a.toggle_panel(TabKind::Orchestrator) },
            Command { label: "Plan Routed Sub-Agents", action: |a| a.plan_routed_subagents() },
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
            Command { label: "Toggle Usage", action: |a| a.toggle_panel(TabKind::Usage) },
            Command { label: "Toggle Search", action: |a| a.toggle_panel(TabKind::Search) },
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

    fn save_buffer_to(&mut self, id: &TabId, path: &PathBuf) -> bool {
        self.save_buffer_to_with_feedback(id, path, true)
    }

    fn save_buffer_to_with_feedback(&mut self, id: &TabId, path: &PathBuf, success_feedback: bool) -> bool {
        if let Some(buf) = self.buffers.get(id) {
            match std::fs::write(path, buf.content()) {
                Ok(_) => {
                    if success_feedback {
                        self.status_message = format!("Saved {}", path.display());
                        let filename = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                        self.toasts.push(crate::editor::toast::Toast::success(format!("Saved {filename}")));
                    }
                    true
                }
                Err(e) => {
                    self.status_message = format!("Error saving {}: {}", path.display(), e);
                    self.toasts.push(crate::editor::toast::Toast::error(format!("Failed to save: {e}")));
                    false
                }
            }
        } else {
            self.status_message = format!("No buffer found for {}", path.display());
            self.toasts.push(crate::editor::toast::Toast::error("Failed to save: missing buffer"));
            false
        }
    }

    fn tab_path(&self, id: &TabId) -> Option<&PathBuf> {
        self.tabs.iter().find(|t| t.id == *id)?.editor_path()
    }

    fn diff_preview(old: &str, new: &str, max_lines: usize) -> (usize, usize, String) {
        let old_lines: Vec<&str> = old.lines().collect();
        let new_lines: Vec<&str> = new.lines().collect();
        let mut out = String::new();
        let mut added = 0usize;
        let mut removed = 0usize;
        let mut shown = 0usize;
        let mut o = 0usize;
        let mut n = 0usize;

        while (o < old_lines.len() || n < new_lines.len()) && shown < max_lines {
            if o < old_lines.len() && n < new_lines.len() && old_lines[o] == new_lines[n] {
                o += 1;
                n += 1;
            } else if n < new_lines.len() && (o >= old_lines.len() || !old_lines[o..].contains(&new_lines[n])) {
                added += 1;
                out.push_str("+ ");
                out.push_str(new_lines[n]);
                out.push('\n');
                n += 1;
                shown += 1;
            } else if o < old_lines.len() {
                removed += 1;
                out.push_str("- ");
                out.push_str(old_lines[o]);
                out.push('\n');
                o += 1;
                shown += 1;
            } else {
                break;
            }
        }

        if out.is_empty() {
            out.push_str("(no line-level changes)");
        }

        (added, removed, out)
    }

    fn active_change_preview(&self) -> Option<ActiveChangePreview> {
        let active_id = self.active_tab.as_ref()?;
        let path = self.tab_path(active_id)?;
        let buf = self.buffers.get(active_id)?;
        let disk_content = std::fs::read_to_string(path).ok()?;
        if disk_content == buf.content() {
            return None;
        }

        let (added_lines, removed_lines, preview) = Self::diff_preview(&disk_content, buf.content(), 10);
        let (_, _, full_diff) = Self::diff_preview(&disk_content, buf.content(), usize::MAX);
        Some(ActiveChangePreview {
            file_label: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            added_lines,
            removed_lines,
            preview,
            full_diff,
        })
    }

    fn revert_active_from_disk(&mut self) {
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
                }
                let filename = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                self.status_message = format!("Reverted {} from disk", path.display());
                self.toasts.push(crate::editor::toast::Toast::warn(format!("Reverted {filename}")));
            }
            Err(e) => {
                self.status_message = format!("Failed to revert {}: {}", path.display(), e);
                self.toasts.push(crate::editor::toast::Toast::error(format!("Revert failed: {e}")));
            }
        }
    }

    fn ask_agent_about_active_diff(&mut self) {
        let Some(change_preview) = self.active_change_preview() else {
            self.status_message = "No active diff to send".into();
            return;
        };
        let prompt = format!(
            "Review the following active diff and suggest the best next coding steps.\n\nFile: {}\nAdded lines: {}\nRemoved lines: {}\n\nDiff:\n{}",
            change_preview.file_label,
            change_preview.added_lines,
            change_preview.removed_lines,
            change_preview.full_diff,
        );
        self.chat.push_user(prompt.clone());
        self.chat_history.push_str("\nYou: ");
        self.chat_history.push_str(&prompt);
        self.agent_active = true;
        self.chat.agent_active = true;
        self.status_message = "Sent active diff to agent".into();
        self.toggle_chat();
        let _ = self.agent_tx.send(UiToAgentMessage::UserPrompt(prompt));
    }

    fn stage_active_file(&mut self) {
        let Some(active_id) = self.active_tab.clone() else {
            self.status_message = "No active editor to stage".into();
            return;
        };
        let Some(path) = self.tab_path(&active_id).cloned() else {
            self.status_message = "Active buffer has no file path".into();
            return;
        };

        let filename = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        if !self.save_buffer_to_with_feedback(&active_id, &path, false) {
            self.status_message = format!("Failed to save {} before staging", path.display());
            self.toasts.push(crate::editor::toast::Toast::error(format!("Save failed before staging {filename}")));
            return;
        }

        let relative = path.strip_prefix(&self.workspace_root).unwrap_or(&path).to_path_buf();
        match std::process::Command::new("git")
            .current_dir(&self.workspace_root)
            .arg("add")
            .arg(&relative)
            .output()
        {
            Ok(output) if output.status.success() => {
                self.status_message = format!("Saved and staged {}", relative.display());
                self.toasts.push(crate::editor::toast::Toast::success(format!("Saved and staged {filename}")));
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                self.status_message = format!("Saved but failed to stage {}", relative.display());
                self.toasts.push(crate::editor::toast::Toast::error(format!("git add failed after save: {}", stderr.trim())));
            }
            Err(e) => {
                self.status_message = format!("Saved but failed to run git add: {e}");
                self.toasts.push(crate::editor::toast::Toast::error(format!("git add error after save: {e}")));
            }
        }
    }

    fn dirty_buffer_count(&self) -> usize {
        self.tabs
            .iter()
            .filter_map(|tab| {
                let path = tab.editor_path()?;
                let buffer = self.buffers.get(&tab.id)?;
                let disk_content = std::fs::read_to_string(path).ok()?;
                (disk_content != buffer.content()).then_some(())
            })
            .count()
    }

    fn save_all(&mut self) {
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

    fn toggle_panel(&mut self, kind: TabKind) {
        let is_active = self.active_tab.as_ref().map(|active_id| {
            self.tabs.iter().any(|t| t.id == *active_id && std::mem::discriminant(&t.kind) == std::mem::discriminant(&kind))
        }).unwrap_or(false);

        if is_active {
            self.tabs.retain(|t| std::mem::discriminant(&t.kind) != std::mem::discriminant(&kind));
            self.rebuild_dock();
            self.active_tab = self.tabs.first().map(|t| t.id.clone());
        } else {
            let maybe_existing = self.tabs.iter().find(|t| std::mem::discriminant(&t.kind) == std::mem::discriminant(&kind)).cloned();
            if let Some(existing) = maybe_existing {
                self.active_tab = Some(existing.id);
            } else {
                let id = TabId::next(&mut self.tab_counter);
                let tab = Tab { id: id.clone(), kind };
                self.tabs.push(tab.clone());
                if let Some(dock) = self.dock_state.as_mut() {
                    dock.push_to_focused_leaf(tab);
                }
                self.active_tab = Some(id);
            }
        }
    }

    fn rebuild_dock(&mut self) {
        self.dock_state = Some(DockState::new(self.tabs.clone()));
    }

    fn refresh_models(&mut self) {
        self.models_loading = true;
        self.chat.models_loading = true;
        self.status_message = "Refreshing model catalog...".into();
        let _ = self.agent_tx.send(UiToAgentMessage::RefreshModels);
    }

    fn sync_approval_state_from_pending(&mut self) {
        self.agent_ui_state.approvals = AgentUiState::default().approvals;
        for (id, tool_name, _) in &self.pending_approvals {
            let tool_id = id.parse::<u32>().unwrap_or(0);
            let _ = self.agent_ui_state.approvals.add_approval(tool_id, tool_name, false);
        }
    }

    fn approve_pending_tool_at(&mut self, idx: usize) {
        if idx >= self.pending_approvals.len() {
            self.status_message = "No pending tool approval at that index".into();
            return;
        }

        let (id, tool_name, arguments) = self.pending_approvals[idx].clone();
        let _ = self.agent_tx.send(UiToAgentMessage::ApproveTool {
            id: id.clone(),
            tool_name: tool_name.clone(),
            arguments,
        });
        self.pending_approvals.remove(idx);
        self.chat.pending_approvals.retain(|(pending_id, _, _)| pending_id != &id);
        self.sync_approval_state_from_pending();
        self.status_message = format!("Approved tool: {}", tool_name);
        self.toasts.push(crate::editor::toast::Toast::success(format!("Approved {}", tool_name)));
    }

    fn reject_pending_tool_at(&mut self, idx: usize) {
        if idx >= self.pending_approvals.len() {
            self.status_message = "No pending tool approval at that index".into();
            return;
        }

        let (id, tool_name, _) = self.pending_approvals[idx].clone();
        let _ = self.agent_tx.send(UiToAgentMessage::RejectTool {
            id: id.clone(),
            tool_name: tool_name.clone(),
        });
        self.pending_approvals.remove(idx);
        self.chat.pending_approvals.retain(|(pending_id, _, _)| pending_id != &id);
        self.sync_approval_state_from_pending();
        self.status_message = format!("Declined tool: {}", tool_name);
        self.toasts.push(crate::editor::toast::Toast::warn(format!("Declined {}", tool_name)));
    }

    fn approve_all_pending_tools(&mut self) {
        let pending_len = self.pending_approvals.len();
        if pending_len == 0 {
            self.status_message = "No pending tool approvals".into();
            return;
        }

        while !self.pending_approvals.is_empty() {
            self.approve_pending_tool_at(0);
        }
        self.status_message = format!("Approved {} pending tool(s)", pending_len);
        self.toasts.push(crate::editor::toast::Toast::success(format!("Approved {} tool request(s)", pending_len)));
    }

    fn reject_all_pending_tools(&mut self) {
        let pending_len = self.pending_approvals.len();
        if pending_len == 0 {
            self.status_message = "No pending tool approvals".into();
            return;
        }

        while !self.pending_approvals.is_empty() {
            self.reject_pending_tool_at(0);
        }
        self.status_message = format!("Declined {} pending tool(s)", pending_len);
        self.toasts.push(crate::editor::toast::Toast::warn(format!("Declined {} tool request(s)", pending_len)));
    }

    fn build_active(&mut self) {
        self.command_output.clear();
        self.status_message = "Running local build...".into();
        self.agent_active = true;
        let _ = self.agent_tx.send(UiToAgentMessage::RunLocalBuild);
    }

    fn plan_routed_subagents(&mut self) {
        let Some(goal) = self.current_routing_goal() else {
            self.status_message = "Enter a chat prompt or keep a recent user goal to route".into();
            self.toasts.push(crate::editor::toast::Toast::warn("No goal available for routed planning"));
            return;
        };
        let task_kind = infer_task_kind_from_goal(&goal);
        let scope_files = self.collect_routing_scope_files(&goal);
        if scope_files.is_empty() {
            self.status_message = "No scoped files available for routed planning".into();
            self.toasts.push(crate::editor::toast::Toast::warn("No files found for routed planning"));
            return;
        }

        let sitemap_dir = self.workspace_root.join(".velocity").join("site_map");
        let site_map = match velocity_ide::site_map::SiteMap::open(&sitemap_dir, 0xDEAD) {
            Ok(site_map) => site_map,
            Err(err) => {
                self.status_message = format!("SiteMap unavailable: {err}");
                self.toasts.push(crate::editor::toast::Toast::error(format!("SiteMap unavailable: {err}")));
                return;
            }
        };

        let model_catalogs = vec![(self.provider, self.available_models.clone())];
        let coordinator = WorkspaceCoordinator::new(self.mediator.clone());
        let routed_tasks = coordinator.plan_routed_tasks(
            &self.workspace_root,
            &goal,
            task_kind,
            &scope_files,
            &site_map,
            &model_catalogs,
        );

        if routed_tasks.is_empty() {
            self.status_message = "Routing produced no sub-agent tasks".into();
            self.toasts.push(crate::editor::toast::Toast::warn("Routing produced no sub-agent tasks"));
            return;
        }

        let routed_count = routed_tasks.len();
        let scope_count = scope_files.len();
        self.orchestrator
            .set_routed_tasks(goal.clone(), task_kind, scope_count, routed_tasks.clone());
        self.task_timeline
            .session_marker("Sub-agent route planned", &format!("{} tasks for {}", routed_count, task_kind.as_str()));
        self.command_output.push_str(&format!(
            "[routed-plan] goal={goal}\n[routed-plan] kind={} scope_files={} tasks={}\n",
            task_kind.as_str(),
            scope_count,
            routed_count,
        ));
        for task in &routed_tasks {
            self.command_output.push_str(&format!(
                "  - {} :: {} / {} :: {} file(s)\n",
                task.task_id,
                task.provider.label(),
                task.model_label,
                task.files.len(),
            ));
        }
        self.status_message = format!("Planned {} routed sub-agent task(s)", routed_count);
        self.toasts.push(crate::editor::toast::Toast::success(format!(
            "Planned {} routed sub-agent task(s)",
            routed_count,
        )));
        self.focus_orchestrator_tab();
    }

    fn current_routing_goal(&self) -> Option<String> {
        let draft = self.chat.input.trim();
        if !draft.is_empty() {
            return Some(draft.to_string());
        }
        self.chat
            .messages
            .iter()
            .rev()
            .find(|message| message.role == crate::editor::chat_panel::ChatRole::User)
            .map(|message| message.content.trim().to_string())
            .filter(|message| !message.is_empty())
    }

    fn collect_routing_scope_files(&self, goal: &str) -> Vec<PathBuf> {
        let use_workspace = wants_workspace_scope(goal);
        let mut files = if use_workspace {
            collect_workspace_routing_files(&self.workspace_root, 96)
        } else {
            self.collect_open_editor_paths()
        };
        if files.is_empty() {
            files = collect_workspace_routing_files(&self.workspace_root, 96);
        }
        files
    }

    fn collect_open_editor_paths(&self) -> Vec<PathBuf> {
        let mut seen = HashSet::new();
        let mut files = Vec::new();
        for tab in &self.tabs {
            if let Some(path) = tab.editor_path() {
                if seen.insert(path.clone()) {
                    files.push(path.clone());
                }
            }
        }
        files
    }

    fn focus_orchestrator_tab(&mut self) {
        if let Some(tab) = self
            .tabs
            .iter()
            .find(|tab| matches!(tab.kind, TabKind::Orchestrator))
            .cloned()
        {
            self.active_tab = Some(tab.id);
        } else {
            self.toggle_orchestrator();
        }
    }

    fn run_active(&mut self) {
        self.command_output.clear();
        self.status_message = "Running local execute...".into();
        self.agent_active = true;
        let _ = self.agent_tx.send(UiToAgentMessage::RunLocalRun);
    }

    fn update_diagnostics(&mut self) {
        let diag = read_latest_diagnostics(&self.workspace_root);
        let count = if diag.success { 0 } else { diag.errors.len() };
        if count != self.build_errors_count {
            if count == 0 {
                self.toasts.push(crate::editor::toast::Toast::success("Build succeeded!"));
            } else {
                self.toasts.push(crate::editor::toast::Toast::error(format!("Build failed with {} errors", count)));
            }
            self.build_errors_count = count;
        }
    }
}

impl eframe::App for VelocityApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        crate::editor::theme::apply_theme(&ctx, IdePalette::dark());
        self.handle_agent_messages();
        self.handle_global_shortcuts(&ctx);
        self.update_diagnostics();

        let now = std::time::Instant::now();
        if self.file_tree.is_none() || now.duration_since(self.last_tree_update) > std::time::Duration::from_secs(3) {
            self.file_tree = Some(build_file_tree(&self.workspace_root));
            self.last_tree_update = now;
        }

        let mut cursor_pos = None;
        if let Some(active_id) = &self.active_tab {
            if let Some(buf) = self.buffers.get(active_id) {
                let editor_id = egui::Id::new("code_editor");
                if let Some(state) = egui::widgets::text_edit::TextEditState::load(&ctx, editor_id) {
                    if let Some(cursor_range) = state.cursor.char_range() {
                        cursor_pos = Some(get_cursor_pos(buf.content(), cursor_range.primary.index.into()));
                    }
                }
            }
        }
        let dirty_buffer_count = self.dirty_buffer_count();
        let active_change_preview = self.active_change_preview();

        // 1. Top Panel Toolbar with System Status & GPU Telemetry
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
                    self.toggle_chat();
                }
                if ui.button("🧭 Route").clicked() {
                    self.plan_routed_subagents();
                }
                if ui.button("🧠 Orchestrate").clicked() {
                    self.toggle_panel(TabKind::Orchestrator);
                }
                if ui.button("🔍 Search").clicked() {
                    self.toggle_panel(TabKind::Search);
                }
                if ui.button("📊 Graph").clicked() {
                    self.toggle_panel(TabKind::Graph);
                }
                if ui.button("📺 Terminal").clicked() {
                    self.toggle_panel(TabKind::Output);
                }
                if dirty_buffer_count > 0 {
                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!("Δ {} dirty", dirty_buffer_count))
                            .strong()
                            .color(egui::Color32::from_rgb(250, 204, 21)),
                    );
                }
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

                    let timeline_snapshot = TaskTimelineSnapshot::new(&self.task_timeline);
                    render_task_timeline(ui, &timeline_snapshot);

                    ui.separator();
                    ui.label(egui::RichText::new("🌲 FILE EXPLORER").size(12.0).strong().color(egui::Color32::from_rgb(168, 85, 247)));
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        if let Some(tree) = self.file_tree.take() {
                            render_file_tree(ui, &tree, self);
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

        // 2c. Right Side Panel: Semantic History & Discourse Blame
        egui::Panel::right("right_sidebar")
            .resizable(true)
            .default_size(280.0)
            .show(ui, |ui: &mut egui::Ui| {
                ui.add_space(4.0);
                ui.vertical(|ui: &mut egui::Ui| {
                    ui.label(egui::RichText::new("🧠 SEMANTIC HISTORY").size(12.0).strong().color(egui::Color32::from_rgb(34, 211, 238)));
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
                                    .color(egui::Color32::from_rgb(250, 204, 21)),
                            );
                            ui.label(
                                egui::RichText::new(format!("+{} / -{} lines", change_preview.added_lines, change_preview.removed_lines))
                                    .size(10.0)
                                    .color(egui::Color32::from_rgb(125, 131, 166)),
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
                                    .color(egui::Color32::from_rgb(226, 227, 243)),
                            );
                        });
                        ui.separator();
                    }

                    if let Some(symbol) = &active_symbol {
                        self.smart_sidebar.add_symbol(0, symbol, "active-buffer", cursor_pos.map(|(line, _)| line as u32).unwrap_or(0), 0);
                        self.smart_sidebar.add_quick_action(0, "Inspect semantic history", symbol, 2);
                        ui.label(egui::RichText::new(format!("Symbol: {}()", symbol)).strong().color(egui::Color32::from_rgb(168, 85, 247)));
                        
                        let symbol_hash = hash_str(symbol);
                        ui.label(egui::RichText::new(format!("Hash: {:016x}", symbol_hash)).size(10.0).weak());

                        // Query SiteMap triples
                        let sitemap_dir = self.workspace_root.join(".velocity").join("site_map");
                        let weight_root = 0xDEAD; // Dummy or active root
                        if let Ok(sm) = velocity_ide::site_map::SiteMap::open(&sitemap_dir, weight_root) {
                            // Find callers
                            let callers = sm.get_callers(symbol_hash);
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new("📞 CALLERS").size(11.0).strong().color(egui::Color32::from_rgb(34, 211, 238)));
                            if callers.is_empty() {
                                ui.label("No active callers found in graph.");
                            } else {
                                for caller in &callers {
                                    ui.label(format!("• 0x{:016x}", caller));
                                }
                            }

                            // Find dependencies
                            let deps = sm.get_dependencies(symbol_hash);
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new("⚙️ DEPENDENCIES").size(11.0).strong().color(egui::Color32::from_rgb(34, 211, 238)));
                            if deps.is_empty() {
                                ui.label("No dependencies found.");
                            } else {
                                for dep in &deps {
                                    ui.label(format!("• 0x{:016x}", dep));
                                }
                            }

                            // Find intent conversations link
                            let intent_triples = sm.find_triples(Some(symbol_hash), Some(3), None);
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new("💬 AI INTENT & TRANSCRIPTS").size(11.0).strong().color(egui::Color32::from_rgb(168, 85, 247)));
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
                    let sidebar_snapshot = SmartSidebarSnapshot::new(&self.smart_sidebar);
                    render_smart_sidebar(ui, &sidebar_snapshot);
                });
            });

        // 2b. Bottom Panel: Status Bar
        let branch = get_git_branch(&self.workspace_root);
        let build_ok = self.build_errors_count == 0;
        let latency_us = crate::ipc::telemetry_share::TELEMETRY_LATENCY_US.load(std::sync::atomic::Ordering::Relaxed);
        let status_info = if latency_us > 0 {
            format!("{} | 🟢 GPU: {} | ⚡ ShMem: {}µs", self.status_message, self.gpu_name, latency_us)
        } else {
            format!("{} | 🟢 GPU: {} | ⚡ ShMem: active", self.status_message, self.gpu_name)
        };
        crate::editor::status_bar::StatusBar::show(
            ui,
            branch.as_deref(),
            cursor_pos,
            build_ok,
            &status_info,
        );

        // 3. Central Docking Panels
        egui::CentralPanel::default().show(ui, |ui| {
            let mut dock_state = self.dock_state.take().expect("dock state");
            let mut viewer = TabViewerImpl { app: self };
            DockArea::new(&mut dock_state)
                .style(DockStyle::from_egui(ui.style().as_ref()))
                .show_inside(ui, &mut viewer);
            self.dock_state = Some(dock_state);
        });

        // Bottom panel: Agentic UI (thinking, approvals, metrics)
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
                            ui.label(egui::RichText::new("Direct approval actions").size(10.0).color(egui::Color32::from_rgb(125, 131, 166)));
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
                                ui.label(egui::RichText::new(tool_name.as_str()).size(10.0).color(egui::Color32::from_rgb(226, 227, 243)));
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
    }}

fn infer_task_kind_from_goal(goal: &str) -> AgentTaskKind {
    let lower = goal.to_lowercase();
    if lower.contains("refactor") {
        AgentTaskKind::Refactor
    } else if lower.contains("fix") || lower.contains("bug") || lower.contains("error") {
        AgentTaskKind::BugFix
    } else if lower.contains("test") || lower.contains("validate") {
        AgentTaskKind::Test
    } else if lower.contains("doc") || lower.contains("readme") {
        AgentTaskKind::Documentation
    } else if lower.contains("merge") || lower.contains("reconcile") {
        AgentTaskKind::Merge
    } else if lower.contains("analy") || lower.contains("investig") {
        AgentTaskKind::Analysis
    } else {
        AgentTaskKind::Planning
    }
}

fn wants_workspace_scope(goal: &str) -> bool {
    let lower = goal.to_lowercase();
    lower.contains("codebase")
        || lower.contains("workspace")
        || lower.contains("project")
        || lower.contains("repository")
        || lower.contains("repo")
}

fn collect_workspace_routing_files(root: &Path, limit: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_workspace_routing_files_recursive(root, &mut files, limit);
    files
}

fn collect_workspace_routing_files_recursive(root: &Path, files: &mut Vec<PathBuf>, limit: usize) {
    if files.len() >= limit {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if files.len() >= limit {
            break;
        }
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if matches!(name.as_ref(), ".git" | ".velocity" | "target" | "archive" | "node_modules") {
                continue;
            }
            collect_workspace_routing_files_recursive(&path, files, limit);
            continue;
        }
        let include = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| matches!(ext, "rs" | "go" | "toml" | "md" | "json" | "yml" | "yaml"))
            .unwrap_or(false)
            || matches!(name.as_ref(), "Cargo.lock" | "Cargo.toml" | "go.mod" | "go.sum");
        if include {
            files.push(path);
        }
    }
}

fn render_file_tree(ui: &mut egui::Ui, node: &FileNode, app: &mut VelocityApp) {
    if let Some(children) = &node.children {
        for child in children {
            if child.is_dir {
                ui.collapsing(format!("📁 {}", child.name), |ui| {
                    render_file_tree(ui, child, app);
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

    fn toggle_search(&mut self) {
        self.toggle_panel(TabKind::Search);
    }

    pub fn search_panel(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new().inner_margin(egui::Margin::same(10)).show(ui, |ui| {
            ui.vertical(|ui| {
                ui.heading("🔍 Search Workspace");
                ui.horizontal(|ui| {
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.search_query)
                            .hint_text("Search query...")
                            .desired_width(ui.available_width() - 80.0)
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
                            let title = format!("{} {} : line {}", icon, hit.path.display(), hit.line);
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

    fn handle_terminal_messages(&mut self) {
        if let Some(rx) = &self.terminal_rx {
            while let Ok(out) = rx.try_recv() {
                self.command_output.push_str(&out);
            }
        }
    }

    fn handle_agent_messages(&mut self) {
        self.handle_terminal_messages();
        while let Ok(msg) = self.agent_rx.try_recv() {
            match msg {
                AgentToUiMessage::OutputToken(token) => {
                    let mut last_you_idx = None;
                    let mut last_agent_idx = None;
                    for (idx, line) in self.chat_history.lines().enumerate() {
                        if line.starts_with("You: ") {
                            last_you_idx = Some(idx);
                        } else if line.starts_with("Agent: ") || line.starts_with("Antigravity: ") || line.starts_with("Kimi: ") {
                            last_agent_idx = Some(idx);
                        }
                    }

                    let needs_prefix = match (last_you_idx, last_agent_idx) {
                        (Some(y), Some(a)) => y > a,
                        (Some(_), None) => true,
                        _ => self.chat_history.is_empty() || self.agent_active,
                    };

                    if needs_prefix {
                        self.chat_history.push_str("\nAgent: ");
                        self.agent_active = false;
                    }
                    self.chat_history.push_str(&token);
                    self.status_message = token.chars().take(80).collect();
                    self.chat.append_agent_token(&token);
                }
                AgentToUiMessage::ThoughtToken(token) => {
                    if self.current_agent_task_id == 0 {
                        self.current_agent_task_id = self.task_timeline.task_started("Agent response", "reasoning", 0);
                        self.task_timeline.agent_marker("Agent session start", "reasoning stream opened", self.current_agent_task_id);
                    }
                    let _ = self.agent_ui_state.thinking.append_token(&token);
                    self.chat.append_thought_token(&token);
                }
                AgentToUiMessage::RequestToolApproval { id, tool_name, arguments } => {
                    let tool_id = id.parse::<u32>().unwrap_or(0);
                    let _ = self.agent_ui_state.approvals.add_approval(tool_id, &tool_name, false);
                    if self.current_agent_task_id == 0 {
                        self.current_agent_task_id = self.task_timeline.task_started("Tool approval", "awaiting approval", 0);
                    }
                    self.task_timeline.agent_marker("Approval requested", &tool_name, self.current_agent_task_id);
                    self.task_timeline.tool_call(self.current_agent_task_id, &tool_name, "approval required");

                    self.command_output.push_str(&format!("[tool-approval-request] {}: {:?}\n", tool_name, arguments));
                    let should_auto = self.chat.auto_approve || self.auto_approve;
                    if should_auto {
                        let _ = self.agent_tx.send(UiToAgentMessage::ApproveTool {
                            id,
                            tool_name,
                            arguments,
                        });
                    } else {
                        self.pending_approvals.push((id.clone(), tool_name.clone(), arguments.clone()));
                        self.chat.pending_approvals.push((id, tool_name.clone(), arguments));
                        self.sync_approval_state_from_pending();
                        self.toasts.push(crate::editor::toast::Toast::warn(format!("Approval needed: {}", tool_name)));
                    }
                }
                AgentToUiMessage::ToolExecutionStarted { tool_name } => {
                    self.agent_ui_state.metrics.state = crate::editor::agent_ui_state::AgentState::Running;
                    self.agent_ui_state.metrics.tool_call_count += 1;
                    if self.current_agent_task_id == 0 {
                        self.current_agent_task_id = self.task_timeline.task_started("Tool execution", "agent tool run", 0);
                    }
                    self.task_timeline.agent_marker("Tool phase", &tool_name, self.current_agent_task_id);
                    self.task_timeline.tool_call(self.current_agent_task_id, &tool_name, "started");

                    self.command_output.push_str(&format!("[tool-start] {}\n", tool_name));
                    self.status_message = format!("Running tool: {}", tool_name);
                    self.toasts.push(crate::editor::toast::Toast::info(format!("Running tool: {}", tool_name)));
                }
                AgentToUiMessage::ToolExecutionFinished { tool_name, result } => {
                    self.agent_ui_state.metrics.state = crate::editor::agent_ui_state::AgentState::Running;
                    if self.current_agent_task_id != 0 {
                        self.task_timeline.tool_result(self.current_agent_task_id, &tool_name, true, 0);
                    }

                    self.command_output
                        .push_str(&format!("[tool-finish] {}: {}\n", tool_name, result));
                    self.status_message = format!("Tool done: {}", tool_name);
                    self.toasts.push(crate::editor::toast::Toast::success(format!("Finished tool: {}", tool_name)));
                    self.chat.agent_active = true;
                }
                AgentToUiMessage::StatusUpdate(message) => {
                    if message.to_lowercase().contains("model catalog") {
                        self.models_loading = false;
                        self.chat.models_loading = false;
                        self.task_timeline.session_marker("Model catalog refreshed", &message);
                    } else {
                        self.task_timeline.agent_marker("Status", &message, self.current_agent_task_id);
                    }
                    if self.current_agent_task_id == 0 {
                        self.current_agent_task_id = self.task_timeline.task_started("Status update", &message, 0);
                    }
                    self.status_message = message;
                }
                AgentToUiMessage::AgentFinished => {
                    self.agent_ui_state.metrics.state = crate::editor::agent_ui_state::AgentState::Idle;
                    if self.current_agent_task_id != 0 {
                        self.task_timeline.agent_marker("Agent session end", "response completed", self.current_agent_task_id);
                        self.task_timeline.task_completed(self.current_agent_task_id, 0, 0, 0);
                        self.current_agent_task_id = 0;
                    }

                    self.status_message = "Agent finished".into();
                    self.agent_active = false;
                    self.chat.agent_active = false;
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
                    self.task_timeline.session_marker("Model selected", &selected);
                    // Update metrics (thinking feature state)
                    self.agent_ui_state.metrics.thinking_enabled = thinking;

                    self.available_models = models.clone();
                    self.selected_model = selected.clone();
                    self.thinking_enabled = thinking;
                    self.models_loading = false;
                    // Sync chat panel state
                    self.chat.available_models = models;
                    self.chat.selected_model = selected;
                    self.chat.thinking_enabled = thinking;
                    self.chat.thinking_supported = self.thinking_supported;
                    self.chat.tools_supported = self.tools_supported;
                    self.chat.models_loading = false;
                }
                AgentToUiMessage::ProviderChanged(new_provider) => {
                    let provider_name = new_provider.label();
                    self.task_timeline.session_marker("Provider changed", provider_name);
                    self.provider = new_provider;
                }
                AgentToUiMessage::AccountUsage { accounts, date } => {
                    self.account_usage = accounts;
                    self.usage_date = date;
                }
                AgentToUiMessage::ChatHistoryRestored(history) => {
                    for (role, content) in &history {
                        if content.trim().is_empty() { continue; }
                        let prefix = if role == "user" { "You: " } else { "Agent: " };
                        self.chat_history.push_str(&format!("\n{}{}\n", prefix, content));
                    }
                    self.chat.restore_history(history);
                }
            }
        }
        self.cap_logs();
    }

    fn cap_logs(&mut self) {
        const MAX: usize = 32_000;
        if self.command_output.len() > MAX {
            let mut cut = self.command_output.len() - MAX;
            while cut < self.command_output.len() && !self.command_output.is_char_boundary(cut) {
                cut += 1;
            }
            self.command_output = self.command_output.split_off(cut);
        }
        if self.chat_history.len() > MAX {
            let mut cut = self.chat_history.len() - MAX;
            while cut < self.chat_history.len() && !self.chat_history.is_char_boundary(cut) {
                cut += 1;
            }
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

    fn full_diff_ui(&mut self, ctx: &egui::Context) {
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
                        egui::RichText::new(format!("{}  (+{} / -{})", change_preview.file_label, change_preview.added_lines, change_preview.removed_lines))
                            .strong()
                            .color(egui::Color32::from_rgb(250, 204, 21)),
                    );
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(change_preview.full_diff.as_str())
                                .monospace()
                                .size(10.0)
                                .color(egui::Color32::from_rgb(226, 227, 243)),
                        );
                    });
                } else {
                    ui.label("No active unsaved changes.");
                }
            });
        self.show_full_diff = open;
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
            TabKind::Editor { path, buffer_id } => {
                if let Some(buf) = self.app.buffers.get_mut(buffer_id) {
                    egui::Frame::new().inner_margin(egui::Margin::same(4)).show(ui, |ui: &mut egui::Ui| {
                        let mut editor = CodeEditor::new("code_editor");
                        let locks = path.as_deref()
                            .map(|p| self.app.mediator.get_locks_for_file(p))
                            .unwrap_or_default();
                        editor.show(ui, buf.content_mut(), path.as_deref(), self.app.pending_cursor_line, &locks);
                        if self.app.pending_cursor_line.is_some() {
                            self.app.pending_cursor_line = None;
                        }
                    });
                }
            }
            TabKind::Chat => {
                render_chat_panel(ui, &mut self.app.chat, &self.app.agent_tx);
            }
            TabKind::Output => self.output_panel(ui),
            TabKind::Orchestrator => {
                self.app.orchestrator.ui(ui);
            }
            TabKind::Usage => {
                render_usage_panel(ui, &self.app.account_usage, &self.app.usage_date, || {
                    let _ = self.app.agent_tx.send(UiToAgentMessage::RefreshUsage);
                });
            }
            TabKind::Search => {
                self.app.search_panel(ui);
            }
            TabKind::Graph => {
                self.app.graph_view.ui(ui, &self.app.workspace_root, &self.app.mediator);
            }
        }
    }
}

impl<'a> TabViewerImpl<'a> {

    fn output_panel(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .inner_margin(egui::Margin::same(10))
            .fill(egui::Color32::from_rgb(7, 8, 12))
            .show(ui, |ui: &mut egui::Ui| {
                ui.vertical(|ui: &mut egui::Ui| {
                    let scroll_height = ui.available_height() - 75.0;
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

                    let mut run_command = false;
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("> ").monospace().color(egui::Color32::from_rgb(34, 211, 238)));
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut self.app.terminal_input)
                                .font(egui::FontId::monospace(13.0))
                                .desired_width(ui.available_width() - 120.0)
                                .text_color(egui::Color32::from_rgb(226, 227, 243))
                        );
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            run_command = true;
                        }
                    });

                    ui.add_space(4.0);

                    ui.horizontal(|ui: &mut egui::Ui| {
                        if ui.button("🗑 Clear Console").clicked() {
                            self.app.command_output.clear();
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui: &mut egui::Ui| {
                            ui.label(egui::RichText::new(format!("Buffer: {} bytes", self.app.command_output.len())).small().weak());
                        });
                    });

                    if run_command {
                        let cmd_str = self.app.terminal_input.trim().to_string();
                        if !cmd_str.is_empty() {
                            self.app.command_output.push_str(&format!("> {}\n", cmd_str));
                            self.app.terminal_input.clear();

                            let (tx, rx) = std::sync::mpsc::channel();
                            self.app.terminal_rx = Some(rx);

                            let workspace_root = self.app.workspace_root.clone();
                            std::thread::spawn(move || {
                                let mut cmd = if cfg!(target_os = "windows") {
                                    let mut c = std::process::Command::new("cmd");
                                    c.args(&["/C", &cmd_str]);
                                    c
                                } else {
                                    let mut c = std::process::Command::new("sh");
                                    c.args(&["-c", &cmd_str]);
                                    c
                                };
                                cmd.current_dir(&workspace_root);
                                if let Ok(output) = cmd.output() {
                                    let stdout = String::from_utf8_lossy(&output.stdout);
                                    let stderr = String::from_utf8_lossy(&output.stderr);
                                    let _ = tx.send(format!("{}{}", stdout, stderr));
                                } else {
                                    let _ = tx.send("Error: Command execution failed\n".to_string());
                                }
                            });
                        }
                    }
                });
            });
    }
}

fn get_cursor_pos(text: &str, char_idx: usize) -> (usize, usize) {
    let mut line = 0;
    let mut col = 0;
    for (i, c) in text.chars().enumerate() {
        if i == char_idx {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

fn get_git_branch(workspace_root: &std::path::Path) -> Option<String> {
    let head_path = workspace_root.join(".git/HEAD");
    if let Ok(head_content) = std::fs::read_to_string(head_path) {
        let trimmed = head_content.trim();
        if trimmed.starts_with("ref: refs/heads/") {
            return Some(trimmed["ref: refs/heads/".len()..].to_string());
        } else if !trimmed.is_empty() {
            return Some(trimmed.chars().take(7).collect());
        }
    }
    None
}

#[derive(Clone, Debug)]
pub struct FileNode {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub children: Option<Vec<FileNode>>,
}

fn build_file_tree(dir: &std::path::Path) -> FileNode {
    let mut children = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
        entries.sort_by_key(|e| (e.file_type().map(|t| !t.is_dir()).unwrap_or(true), e.file_name()));
        for entry in entries {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            if path.is_dir() {
                children.push(build_file_tree(&path));
            } else {
                children.push(FileNode {
                    name,
                    path,
                    is_dir: false,
                    children: None,
                });
            }
        }
    }
    FileNode {
        name: dir.file_name().unwrap_or_default().to_string_lossy().to_string(),
        path: dir.to_path_buf(),
        is_dir: true,
        children: Some(children),
    }
}

fn hash_str(s: &str) -> u64 {
    use sha2::{Sha256, Digest};
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    let d = h.finalize();
    u64::from_le_bytes(d[..8].try_into().unwrap())
}

fn get_active_symbol(content: &str, cursor_line: usize) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    if cursor_line >= lines.len() {
        return None;
    }
    for idx in (0..=cursor_line).rev() {
        let line = lines[idx].trim();
        if line.contains("fn ") || line.contains("void ") || line.contains("def ") || line.contains("class ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            for (i, &word) in parts.iter().enumerate() {
                if word == "fn" || word == "def" || word == "class" || word == "void" {
                    if let Some(&name) = parts.get(i + 1) {
                        let name_cleaned = name.split('(').next().unwrap_or(name);
                        return Some(name_cleaned.to_string());
                    }
                }
            }
        }
    }
    None
}
