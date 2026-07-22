use std::path::PathBuf;
use std::collections::HashMap;
use crossbeam_channel::{Receiver, Sender};
use eframe::egui;
use egui_dock::DockState;

use crate::agent::{AgentToUiMessage, AiProvider, ModelInfo, UiToAgentMessage};
use crate::editor::agent_ui_state::AgentUiState;
use crate::editor::buffer::EditorBuffer;
use crate::editor::chat_panel::ChatPanelState;
use crate::editor::mission_control::MissionControlState;
use crate::editor::orchestrator_panel::OrchestratorPanel;
use crate::editor::smart_sidebar::SmartSidebarState;
use crate::editor::task_timeline::{persist_mission_activity_nda, TaskTimelineState as TTState};
use crate::usage::AccountUsageView;

use super::super::types::*;
use crate::editor::theme::{apply_theme, IdePalette};

pub struct VelocityApp {
    pub agent_tx: Sender<UiToAgentMessage>,
    pub agent_rx: Receiver<AgentToUiMessage>,

    pub workspace_root: PathBuf,

    pub tabs: Vec<Tab>,
    pub active_tab: Option<TabId>,
    pub buffers: HashMap<TabId, EditorBuffer>,

    pub dock_state: Option<DockState<Tab>>,

    pub chat: ChatPanelState,
    pub command_output: String,

    pub account_usage: Vec<AccountUsageView>,
    pub usage_date: String,

    pub command_palette: CommandPalette,
    pub status_message: String,

    pub tab_counter: u64,

    pub agent_ui_state: AgentUiState,
    pub task_timeline: TTState,
    pub smart_sidebar: SmartSidebarState,

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
    pub orchestrator: OrchestratorPanel,
    pub mission_control: MissionControlState,
    pub next_intervention_id: u64,

    pub mediator: std::sync::Arc<crate::automation::mediator::MediatorArena>,
    pub graph_view: crate::editor::graph_view::MerkleGraphView,
    pub terminal_rx: Option<std::sync::mpsc::Receiver<String>>,
    pub terminal_input: String,
    pub current_agent_task_id: u32,

    pub chat_input: String,
    pub chat_history: String,
}

impl VelocityApp {
    pub fn persist_mission_activity(&self) {
        persist_mission_activity_nda(&self.workspace_root, &self.task_timeline);
    }

    pub fn new(
        cc: &eframe::CreationContext<'_>,
        workspace_root: PathBuf,
        agent_tx: Sender<UiToAgentMessage>,
        agent_rx: Receiver<AgentToUiMessage>,
        gpu_name: String,
        mediator: std::sync::Arc<crate::automation::mediator::MediatorArena>,
    ) -> Self {
        let mut fonts = egui::FontDefinitions::default();
        let _ = crate::editor::theme::setup_fonts(&mut fonts);
        cc.egui_ctx.set_fonts(fonts);
        apply_theme(&cc.egui_ctx, IdePalette::dark());

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
        let mission_control = Tab {
            id: TabId::next(&mut tab_counter),
            kind: TabKind::MissionControl,
        };
        let graph = Tab {
            id: TabId::next(&mut tab_counter),
            kind: TabKind::Graph,
        };
        let tabs = vec![
            mission_control.clone(),
            output.clone(),
            chat.clone(),
            orchestrator.clone(),
            graph.clone(),
        ];

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
            active_tab: Some(mission_control.id.clone()),
            buffers: HashMap::new(),
            dock_state: Some(DockState::new(vec![
                mission_control,
                output,
                chat,
                orchestrator,
                graph,
            ])),
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
            orchestrator: OrchestratorPanel::new(),
            mission_control: MissionControlState::new(),
            next_intervention_id: 1,
            chat: ChatPanelState {
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
                provider: crate::agent::AiProvider::CloudflareWorkersAi,
            },
            mediator,
            graph_view: crate::editor::graph_view::MerkleGraphView::new(),
            terminal_rx: None,
            terminal_input: String::new(),
            current_agent_task_id: 0,
        };
        app.open_editor(None);
        app.task_timeline
            .session_marker("IDE session ready", "agentic workspace initialized");
        app.persist_mission_activity();
        let _ = app.agent_tx.send(UiToAgentMessage::RefreshModels);
        app
    }
}
