use std::collections::HashMap;
use std::path::{Path, PathBuf};
use crossbeam_channel::{Receiver, Sender};
use eframe::egui;
use egui_dock::{DockState, NodeIndex};
use serde::{Deserialize, Serialize};

use crate::agent::{AgentToUiMessage, ModelInfo, UiToAgentMessage};
use crate::editor::agent_ui_state::AgentUiState;
use crate::editor::buffer::EditorBuffer;
use crate::editor::chat_panel::ChatPanelState;
use crate::editor::mission_control::MissionControlState;
use crate::editor::orchestrator_panel::OrchestratorPanel;
use crate::editor::smart_sidebar::SmartSidebarState;
use crate::editor::task_timeline::{persist_mission_activity_nda, TaskTimelineState as TTState};
use crate::usage::AccountUsageView;

use super::super::types::*;
use crate::agent::AiProvider;
use crate::editor::theme::{apply_theme, AppearanceSettings, IdePalette, WorkspaceProfile};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspacePreferences {
    pub appearance: AppearanceSettings,
    pub auto_approve: bool,
    pub selected_model: String,
    pub provider: String,
    pub thinking_enabled: bool,
    pub left_sidebar_visible: bool,
    pub left_sidebar_width: f32,
    pub right_sidebar_visible: bool,
    pub right_sidebar_width: f32,
}

impl WorkspacePreferences {
    pub fn capture(app: &VelocityApp) -> Self {
        Self {
            appearance: app.appearance,
            auto_approve: app.auto_approve,
            selected_model: app.selected_model.clone(),
            provider: app.provider.label().to_string(),
            thinking_enabled: app.thinking_enabled,
            left_sidebar_visible: app.left_sidebar_visible,
            left_sidebar_width: app.left_sidebar_width,
            right_sidebar_visible: app.right_sidebar_visible,
            right_sidebar_width: app.right_sidebar_width,
        }
    }
}

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
    pub appearance: AppearanceSettings,
    pub left_sidebar_visible: bool,
    pub left_sidebar_width: f32,
    pub right_sidebar_visible: bool,
    pub right_sidebar_width: f32,

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
    fn workspace_state_dir(workspace_root: &Path) -> PathBuf {
        workspace_root.join(".velocity")
    }

    fn workspace_preferences_path(workspace_root: &Path) -> PathBuf {
        Self::workspace_state_dir(workspace_root).join("workspace-preferences.json")
    }

    fn parse_provider_label(label: &str) -> Option<AiProvider> {
        match label {
            "Cloudflare Workers AI" => Some(AiProvider::CloudflareWorkersAi),
            "OpenRouter" => Some(AiProvider::OpenRouter),
            "Azure OpenAI" => Some(AiProvider::AzureOpenAi),
            "Local Ollama" => Some(AiProvider::LocalOllama),
            _ => None,
        }
    }

    fn load_workspace_preferences(workspace_root: &Path) -> Option<WorkspacePreferences> {
        let path = Self::workspace_preferences_path(workspace_root);
        let raw = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&raw).ok()
    }

    pub fn save_workspace_preferences(&mut self) {
        let path = Self::workspace_preferences_path(&self.workspace_root);
        if let Some(parent) = path.parent() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                self.status_message = format!("Failed to create workspace state folder: {err}");
                return;
            }
        }

        match serde_json::to_string_pretty(&WorkspacePreferences::capture(self)) {
            Ok(json) => {
                if let Err(err) = std::fs::write(&path, json) {
                    self.status_message = format!("Failed to save workspace preferences: {err}");
                }
            }
            Err(err) => {
                self.status_message = format!("Failed to serialize workspace preferences: {err}");
            }
        }
    }

    pub fn restore_workspace_preferences(&mut self) {
        let Some(preferences) = Self::load_workspace_preferences(&self.workspace_root) else {
            return;
        };

        self.apply_workspace_profile(preferences.appearance.profile);
        self.appearance = preferences.appearance;
        self.auto_approve = preferences.auto_approve;
        self.selected_model = preferences.selected_model;
        if let Some(provider) = Self::parse_provider_label(&preferences.provider) {
            self.provider = provider;
            self.chat.provider = provider;
        }
        self.thinking_enabled = preferences.thinking_enabled;
        self.left_sidebar_visible = preferences.left_sidebar_visible;
        self.left_sidebar_width = preferences.left_sidebar_width.max(180.0);
        self.right_sidebar_visible = preferences.right_sidebar_visible;
        self.right_sidebar_width = preferences.right_sidebar_width.max(220.0);
        self.chat.auto_approve = self.auto_approve;
        self.chat.selected_model = self.selected_model.clone();
        self.chat.thinking_enabled = self.thinking_enabled;
        self.status_message = format!("Restored {} workspace", self.appearance.profile.label());
    }

    pub fn persist_mission_activity(&self) {
        persist_mission_activity_nda(&self.workspace_root, &self.task_timeline);
    }

    pub fn palette(&self) -> IdePalette {
        self.appearance.palette()
    }

    pub fn apply_appearance(&self, ctx: &egui::Context) {
        apply_theme(ctx, self.appearance);
    }

    fn find_tab_by_kind(tabs: &[Tab], kind: &TabKind) -> Option<Tab> {
        tabs.iter()
            .find(|tab| std::mem::discriminant(&tab.kind) == std::mem::discriminant(kind))
            .cloned()
    }

    fn collect_panel_tabs(tabs: &[Tab], kinds: &[TabKind]) -> Vec<Tab> {
        let mut collected = Vec::new();
        for kind in kinds {
            if let Some(tab) = Self::find_tab_by_kind(tabs, kind) {
                if !collected.iter().any(|existing: &Tab| existing.id == tab.id) {
                    collected.push(tab);
                }
            }
        }
        collected
    }

    pub(crate) fn build_workspace_dock(&self, profile: WorkspaceProfile) -> DockState<Tab> {
        let mut root_tabs: Vec<Tab> = self
            .tabs
            .iter()
            .filter(|tab| matches!(tab.kind, TabKind::Editor { .. }))
            .cloned()
            .collect();

        let primary_kinds: Vec<TabKind> = match profile {
            WorkspaceProfile::Coder => vec![TabKind::Chat, TabKind::Search],
            WorkspaceProfile::AutomationOperator => vec![
                TabKind::MissionControl,
                TabKind::Orchestrator,
                TabKind::Output,
            ],
            WorkspaceProfile::MissionControl => vec![TabKind::MissionControl, TabKind::Chat],
            WorkspaceProfile::Accessibility => {
                vec![TabKind::MissionControl, TabKind::Chat, TabKind::Settings]
            }
        };

        for tab in Self::collect_panel_tabs(&self.tabs, &primary_kinds) {
            if !root_tabs.iter().any(|existing| existing.id == tab.id) {
                root_tabs.push(tab);
            }
        }

        let mut dock = DockState::new(if root_tabs.is_empty() {
            self.tabs.clone()
        } else {
            root_tabs
        });
        let surface = dock.main_surface_mut();

        match profile {
            WorkspaceProfile::Coder => {
                let right_tabs = Self::collect_panel_tabs(
                    &self.tabs,
                    &[TabKind::MissionControl, TabKind::Orchestrator, TabKind::Settings],
                );
                let bottom_tabs = Self::collect_panel_tabs(&self.tabs, &[TabKind::Output]);
                let center = if right_tabs.is_empty() {
                    None
                } else {
                    Some(surface.split_right(NodeIndex::root(), 0.72, right_tabs)[0])
                };
                if !bottom_tabs.is_empty() {
                    surface.split_below(center.unwrap_or(NodeIndex::root()), 0.7, bottom_tabs);
                }
            }
            WorkspaceProfile::AutomationOperator => {
                let right_tabs =
                    Self::collect_panel_tabs(&self.tabs, &[TabKind::Graph, TabKind::Settings]);
                let bottom_tabs = Self::collect_panel_tabs(&self.tabs, &[TabKind::Chat]);
                let center = if right_tabs.is_empty() {
                    None
                } else {
                    Some(surface.split_right(NodeIndex::root(), 0.7, right_tabs)[0])
                };
                if !bottom_tabs.is_empty() {
                    surface.split_below(center.unwrap_or(NodeIndex::root()), 0.64, bottom_tabs);
                }
            }
            WorkspaceProfile::MissionControl => {
                let right_tabs = Self::collect_panel_tabs(
                    &self.tabs,
                    &[TabKind::Orchestrator, TabKind::Usage, TabKind::Settings],
                );
                let bottom_tabs = Self::collect_panel_tabs(&self.tabs, &[TabKind::Output]);
                let center = if right_tabs.is_empty() {
                    None
                } else {
                    Some(surface.split_right(NodeIndex::root(), 0.68, right_tabs)[0])
                };
                if !bottom_tabs.is_empty() {
                    surface.split_below(center.unwrap_or(NodeIndex::root()), 0.6, bottom_tabs);
                }
            }
            WorkspaceProfile::Accessibility => {
                let bottom_tabs =
                    Self::collect_panel_tabs(&self.tabs, &[TabKind::Output, TabKind::Search]);
                if !bottom_tabs.is_empty() {
                    surface.split_below(NodeIndex::root(), 0.7, bottom_tabs);
                }
            }
        }

        dock
    }

    pub fn apply_workspace_profile(&mut self, profile: WorkspaceProfile) {
        self.appearance.apply_profile(profile);

        let mut tabs: Vec<Tab> = self
            .tabs
            .iter()
            .filter(|tab| matches!(tab.kind, TabKind::Editor { .. }))
            .cloned()
            .collect();

        let mut push_unique = |kind: TabKind, tabs: &mut Vec<Tab>, counter: &mut u64| {
            if tabs
                .iter()
                .any(|tab| std::mem::discriminant(&tab.kind) == std::mem::discriminant(&kind))
            {
                return;
            }
            tabs.push(Tab {
                id: TabId::next(counter),
                kind,
            });
        };

        match profile {
            WorkspaceProfile::Coder => {
                push_unique(TabKind::MissionControl, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Chat, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Output, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Search, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Orchestrator, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Settings, &mut tabs, &mut self.tab_counter);
            }
            WorkspaceProfile::AutomationOperator => {
                push_unique(TabKind::MissionControl, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Orchestrator, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Output, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Chat, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Graph, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Settings, &mut tabs, &mut self.tab_counter);
            }
            WorkspaceProfile::MissionControl => {
                push_unique(TabKind::MissionControl, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Orchestrator, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Chat, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Usage, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Output, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Settings, &mut tabs, &mut self.tab_counter);
            }
            WorkspaceProfile::Accessibility => {
                push_unique(TabKind::MissionControl, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Chat, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Search, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Output, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Settings, &mut tabs, &mut self.tab_counter);
            }
        }

        self.tabs = tabs;
        self.rebuild_dock();

        let focus_kind = match profile {
            WorkspaceProfile::Coder => TabKind::Chat,
            WorkspaceProfile::AutomationOperator => TabKind::Orchestrator,
            WorkspaceProfile::MissionControl => TabKind::MissionControl,
            WorkspaceProfile::Accessibility => TabKind::Settings,
        };
        self.focus_panel(focus_kind);
        self.status_message = format!("Applied {} workspace preset", profile.label());
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
        let appearance = AppearanceSettings::default();
        apply_theme(&cc.egui_ctx, appearance);

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
            appearance,
            left_sidebar_visible: true,
            left_sidebar_width: 240.0,
            right_sidebar_visible: true,
            right_sidebar_width: 280.0,
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
        app.apply_workspace_profile(app.appearance.profile);
        app.restore_workspace_preferences();
        app.apply_appearance(&cc.egui_ctx);
        app.task_timeline
            .session_marker("IDE session ready", "agentic workspace initialized");
        app.persist_mission_activity();
        let _ = app.agent_tx.send(UiToAgentMessage::RefreshModels);
        app.save_workspace_preferences();
        app
    }
}
