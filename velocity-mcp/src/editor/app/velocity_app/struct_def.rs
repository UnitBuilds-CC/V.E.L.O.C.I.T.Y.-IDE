use crossbeam_channel::{Receiver, Sender};
use eframe::egui;
use egui_dock::DockState;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use velocity_ide::site_map::SiteMap;

use crate::agent::{AgentToUiMessage, ModelInfo, UiToAgentMessage};
use crate::editor::agent_ui_state::AgentUiState;
use crate::editor::bottom_panel::BottomPanelState;
use crate::editor::buffer::EditorBuffer;
use crate::editor::chat_panel::ChatPanelState;
use crate::editor::mission_control::MissionControlState;
use crate::editor::orchestrator_panel::OrchestratorPanel;
use crate::editor::smart_sidebar::SmartSidebarState;
use crate::editor::task_timeline::{persist_mission_activity_nda, TaskTimelineState as TTState};
use crate::usage::{
    load_workspace_provider_settings, save_workspace_provider_settings, AccountUsageView,
    WorkspaceProviderSettings,
};

use super::super::types::*;
use crate::agent::AiProvider;
use crate::editor::theme::{apply_theme, AppearanceSettings, IdePalette, WorkspaceProfile};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ModeLayout {
    pub left_visible: bool,
    pub left_width: f32,
    pub right_visible: bool,
    pub right_width: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspacePreferences {
    pub appearance: AppearanceSettings,
    pub auto_approve: bool,
    #[serde(default)]
    pub show_thoughts: bool,
    pub selected_model: String,
    pub provider: String,
    pub thinking_enabled: bool,
    pub left_sidebar_visible: bool,
    pub left_sidebar_width: f32,
    pub right_sidebar_visible: bool,
    pub right_sidebar_width: f32,
    /// Per-mode panel arrangement the user has customized, so switching modes
    /// restores their own layout instead of resetting to defaults every time.
    #[serde(default)]
    pub mode_layouts: HashMap<WorkspaceProfile, ModeLayout>,
    /// Editor file paths that were open last session, restored on launch.
    #[serde(default)]
    pub open_tabs: Vec<String>,
    /// Path of the editor tab that was active last session.
    #[serde(default)]
    pub active_tab: Option<String>,
}

impl WorkspacePreferences {
    pub fn capture(app: &VelocityApp) -> Self {
        // Refresh the active mode's entry so an un-switched session still
        // persists the layout the user is currently looking at.
        let mut mode_layouts = app.mode_layouts.clone();
        mode_layouts.insert(
            app.appearance.profile,
            ModeLayout {
                left_visible: app.left_sidebar_visible,
                left_width: app.left_sidebar_width,
                right_visible: app.right_sidebar_visible,
                right_width: app.right_sidebar_width,
            },
        );
        Self {
            appearance: app.appearance,
            auto_approve: app.auto_approve,
            show_thoughts: app.chat.show_thoughts,
            selected_model: app.selected_model.clone(),
            provider: app.provider.label().to_string(),
            thinking_enabled: app.thinking_enabled,
            left_sidebar_visible: app.left_sidebar_visible,
            left_sidebar_width: app.left_sidebar_width,
            right_sidebar_visible: app.right_sidebar_visible,
            right_sidebar_width: app.right_sidebar_width,
            mode_layouts,
            open_tabs: app
                .tabs
                .iter()
                .filter_map(|t| t.editor_path())
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
            active_tab: app
                .active_tab
                .as_ref()
                .and_then(|id| app.tabs.iter().find(|t| &t.id == id))
                .and_then(|t| t.editor_path())
                .map(|p| p.to_string_lossy().to_string()),
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
    /// When true, the keybinding cheat-sheet overlay is shown (toggled with F1).
    pub show_shortcuts: bool,
    pub quick_open: QuickOpen,
    pub mru: MruSwitcher,
    /// Stack of recently closed editor file paths for Ctrl+Shift+T reopen.
    pub closed_editor_paths: Vec<PathBuf>,
    /// Ctrl+G go-to-line dialog state.
    pub goto_line_open: bool,
    pub goto_line_input: String,
    pub goto_line_just_opened: bool,
    /// Ctrl+Shift+O go-to-symbol switcher (sitemap-backed).
    pub goto_symbol_open: bool,
    pub goto_symbol_query: String,
    pub goto_symbol_selected: usize,
    pub goto_symbol_just_opened: bool,
    pub goto_symbol_entries: Vec<crate::editor::search::SymbolEntry>,
    /// Cached workspace symbol index (sitemap-backed), shared by go-to-symbol
    /// and the clickable callers/dependencies in the symbol context panel.
    pub workspace_symbols: Vec<crate::editor::search::SymbolEntry>,
    /// Query the cached go-to-symbol `goto_symbol_filtered` indices were computed for.
    pub goto_symbol_last_query: String,
    /// Cached indices into `goto_symbol_entries` (avoids per-frame cloning).
    pub goto_symbol_filtered: Vec<usize>,
    /// One-shot: force the go-to-symbol scroll view to the selected row.
    pub goto_symbol_scroll_to_selected: bool,
    /// Back/forward navigation history (Alt+← / Alt+→).
    pub nav_back: Vec<NavLocation>,
    pub nav_forward: Vec<NavLocation>,
    /// Cached workspace site map (avoids re-reading index.json every frame).
    pub cached_site_map: Option<Arc<SiteMap>>,
    /// When `cached_site_map` was fetched (TTL refresh).
    pub cached_site_map_at: Option<Instant>,
    /// Symbol the cached callers/deps below belong to.
    pub cached_relation_symbol: Option<String>,
    /// Cached caller names for `cached_relation_symbol`.
    pub cached_callers: Vec<String>,
    /// Cached dependency names for `cached_relation_symbol`.
    pub cached_deps: Vec<String>,
    /// When diagnostics were last polled (throttles per-frame disk reads).
    pub last_diagnostics_poll: Option<Instant>,
    /// Throttle for scanning open buffers for on-disk changes.
    pub last_external_check: Option<Instant>,
    pub status_message: String,
    pub appearance: AppearanceSettings,
    pub provider_settings: WorkspaceProviderSettings,
    pub left_sidebar_visible: bool,
    pub left_sidebar_width: f32,
    pub left_sidebar_tab: usize,
    pub right_sidebar_visible: bool,
    pub right_sidebar_width: f32,

    /// Layout the user arranged for each work mode, restored on switch-back.
    pub mode_layouts: HashMap<WorkspaceProfile, ModeLayout>,

    pub tab_counter: u64,

    pub expert_teams: Vec<crate::editor::expert_team::ExpertTeam>,
    pub active_team_index: usize,
    pub selected_member_id: Option<String>,

    /// Which team card is currently expanded in the gallery (None = all collapsed).
    pub team_gallery_expanded: Option<usize>,
    /// Chat state for the team builder sub-chat.
    pub team_builder_chat: crate::editor::team_builder_chat::TeamBuilderChat,

    pub agent_ui_state: AgentUiState,
    pub task_timeline: TTState,
    pub smart_sidebar: SmartSidebarState,
    /// Whether the "Active changes" section in the right sidebar is collapsed.
    pub right_changes_collapsed: bool,
    /// Whether the "Symbol context" section in the right sidebar is collapsed.
    pub right_symbol_collapsed: bool,
    pub bottom_panel_state: BottomPanelState,

    /// Pinned favorite files (Accessibility mode).
    pub favorite_files: Vec<PathBuf>,
    /// In-file bookmarks (Accessibility mode).
    pub bookmarks: Vec<crate::editor::sidebar_tabs::BookmarkEntry>,
    /// Whether the agent is currently recording actions (Operator mode).
    pub recording_active: bool,
    /// Saved recording names (Operator mode).
    pub recordings: Vec<String>,

    pub projects: Vec<PathBuf>,
    pub show_add_project_ui: bool,
    pub new_project_path_input: String,
    /// Ctrl+Shift+W workspace switcher popup state.
    pub workspace_switcher_open: bool,
    pub workspace_switcher_selected: usize,
    pub workspace_switcher_just_opened: bool,
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
    /// Tab awaiting an unsaved-changes confirmation before it can close.
    pub pending_close_tab: Option<TabId>,
    pub show_full_diff: bool,
    pub build_errors_count: usize,
    pub gpu_name: String,
    pub search_query: String,
    pub search_hits: Vec<crate::editor::search::SearchHit>,
    /// Replacement text for the workspace find-and-replace panel.
    pub replace_query: String,
    /// Debounce timer: when the search query last changed (runs after a pause).
    pub search_pending_since: Option<Instant>,
    pub pending_cursor_line: Option<usize>,
    /// Current cursor line in the active editor (updated during rendering).
    pub current_cursor_line: usize,
    /// Current cursor column in the active editor (updated during rendering).
    pub current_cursor_col: usize,
    /// LSP find-references results popup state (I1).
    pub references_open: bool,
    /// References as (file path, 1-based line) for the results popup.
    pub references_results: Vec<(PathBuf, usize)>,
    /// Selected index in the references results popup.
    pub references_selected: usize,
    pub file_tree: Option<FileNode>,
    pub last_tree_update: std::time::Instant,
    /// Last observed mtime of the workspace root (skips tree rebuilds when unchanged).
    pub last_tree_mtime: Option<std::time::SystemTime>,
    pub toasts: crate::editor::toast::ToastQueue,
    pub orchestrator: OrchestratorPanel,
    pub mission_control: MissionControlState,
    pub next_intervention_id: u64,

    pub mediator: std::sync::Arc<crate::automation::mediator::MediatorArena>,
    pub graph_view: crate::editor::graph_view::MerkleGraphView,
    pub wiki_view: crate::editor::wiki_view::WikiView,
    /// Per-tab NDA document editor state, keyed by tab id.
    pub nda_docs: std::collections::HashMap<TabId, crate::editor::nda_document::NdaDocumentView>,
    pub terminal_rx: Option<std::sync::mpsc::Receiver<String>>,
    pub terminal_input: String,
    pub current_agent_task_id: u32,

    pub chat_history: String,

    // ─── IDE Feature Integration State ────────────────────────────────────────
    /// Code completion popup state.
    pub completion_state: crate::editor::completion::CompletionState,
    /// LSP client manager.
    pub lsp_manager: Option<crate::editor::lsp_client::LspManager>,
    /// Aggregated diagnostics from LSP.
    pub diagnostics: crate::editor::diagnostics::DiagnosticsState,
    /// Interactive terminal emulator state.
    pub terminal_state: crate::editor::terminal::TerminalState,
    /// Whether the terminal shell process has been spawned.
    pub terminal_spawned: bool,
    /// Debugger (DAP) session state.
    pub dap_client: Option<crate::editor::debugger::DapClient>,
    /// Configurable keybinding config.
    pub keybindings_config: crate::editor::keybindings::KeybindingsConfig,
    /// Git integration state.
    pub git_state: crate::editor::git_ui::GitState,
    /// Extension registry.
    pub extension_registry: crate::editor::extensions::ExtensionRegistry,
    /// Minimap configuration.
    pub minimap_config: crate::editor::minimap::MinimapConfig,
    /// Snippet collection.
    pub snippet_collection: crate::editor::snippets::SnippetCollection,
    /// Whether to show minimap in editor.
    pub show_minimap: bool,
    /// Whether to show breadcrumbs above editor.
    pub show_breadcrumbs: bool,
    /// Whether word wrap is enabled.
    pub word_wrap: bool,
    /// Browse panel state (web research sidebar).
    pub browse_state: crate::editor::browse_panel::BrowseState,
    /// Workspace checkpoint manager (git-stash rollback).
    pub checkpoint_manager: crate::editor::checkpoint::CheckpointManager,
    /// Persistent per-member agent knowledge store.
    pub agent_memory: crate::editor::agent_memory::AgentMemoryManager,
    /// Live multi-agent orchestration activity feed and progress.
    pub live_orchestration: crate::editor::live_orchestration::LiveOrchestrationState,
    /// Speculative pre-computation cache for agent workers.
    pub precomp_cache: crate::editor::speculative_precomp::PrecomputationCache,
    /// Semantic code search index (TF-IDF).
    pub semantic_index: Option<crate::editor::semantic_search::SemanticIndex>,
    /// Whether semantic search mode is active (vs. literal grep).
    pub semantic_search_active: bool,
    /// Inline ghost-text suggestion engine.
    pub inline_suggestions: crate::editor::inline_suggestions::InlineSuggestionEngine,
    /// Auto-generated test coverage analyzer.
    pub test_generator: crate::editor::test_generator::TestGenerator,
    /// Build/test/deploy pipeline manager.
    pub deploy_pipeline: Option<crate::editor::deploy_pipeline::PipelineManager>,
    /// Voice-to-task input state.
    pub voice_input: crate::editor::voice_commands::VoiceInputState,
    /// Unified knowledge / RAG store queried by agents and the Knowledge panel.
    pub knowledge_base: crate::editor::knowledge_base::KnowledgeBase,
    /// Draft query text in the Knowledge panel search box.
    pub knowledge_query: String,
    /// Draft path text in the Knowledge panel ingest box.
    pub knowledge_ingest_input: String,
    /// Last ranked results rendered in the Knowledge panel.
    pub knowledge_results: Vec<crate::editor::knowledge_base::KnowledgeHit>,
    /// Unattended-execution trigger registry shown in the Triggers panel.
    pub triggers: crate::editor::triggers::TriggerRegistry,
    /// Draft trigger name in the Triggers panel add box.
    pub trigger_name_input: String,
    /// Draft schedule spec (e.g. "5m", "daily@09:00") in the Triggers panel.
    pub trigger_interval_input: String,
    /// Draft agent prompt for a new trigger in the Triggers panel.
    pub trigger_prompt_input: String,
    /// Workflow composer registry shown in the Workflows panel.
    pub workflows: crate::editor::workflow::WorkflowRegistry,
    /// Draft workflow name in the Workflows panel create box.
    pub workflow_name_input: String,
    /// Id of the workflow currently open in the step editor.
    pub workflow_selected: Option<String>,
    /// Draft step: tool name in the Workflows panel add-step row.
    pub workflow_step_tool_input: String,
    /// Draft step: tool JSON args in the Workflows panel add-step row.
    pub workflow_step_args_input: String,
    /// Draft step: agent prompt in the Workflows panel add-step row.
    pub workflow_step_prompt_input: String,
    /// Last workflow run result rendered in the Workflows panel run log.
    pub workflow_last_run: Option<crate::editor::workflow::WorkflowRun>,
    /// Visual canvas instances keyed by workflow id.
    pub workflow_canvases:
        std::collections::HashMap<String, crate::editor::workflow_canvas::WorkflowCanvas>,
    /// Id of the workflow currently open in the visual canvas editor.
    pub workflow_canvas_selected: Option<String>,
    /// Whether the visual canvas editor is active (vs list composer).
    pub workflow_visual_mode: bool,
    /// AI generation prompt input for natural language workflow creation.
    pub workflow_ai_prompt: String,
    /// Version history registry for workflows.
    pub workflow_versions: crate::editor::workflow_version::VersionRegistry,
    /// Governance policy engine edited in the Governance panel.
    pub policy: crate::editor::governance::PolicyEngine,
    /// Approval queue shown in the Governance panel.
    pub approvals: crate::editor::governance::ApprovalQueue,
    /// Secret store (handles only, masked) shown in the Governance panel.
    pub secrets: crate::security::secrets::SecretStore,
    /// Connector registry shown/edited in the Governance panel.
    pub connectors: crate::connectors::ConnectorRegistry,
    /// Draft rule tool name in the Governance policy editor.
    pub gov_rule_tool_input: String,
    /// Draft rule path prefix in the Governance policy editor.
    pub gov_rule_path_input: String,
    /// Draft new secret name in the Governance secrets section.
    pub gov_secret_name_input: String,
    /// Draft new secret value in the Governance secrets section.
    pub gov_secret_value_input: String,
    /// Draft connector id in the Governance connectors section.
    pub gov_connector_id_input: String,
    /// Draft connector base URL in the Governance connectors section.
    pub gov_connector_url_input: String,
    /// Draft connector secret handle in the Governance connectors section.
    pub gov_connector_secret_input: String,
    /// Transient status line shown at the top of the Governance panel.
    pub gov_status: String,

    // ─── Cross-device Peer Collaboration ────────────────────────────────────
    /// Peer manager for cross-device agent collaboration.
    pub peer_manager: crate::agent::peer_link::PeerManager,
    /// Whether the peer API server is currently running.
    pub peer_server_running: bool,
    /// Port configured for the peer API server.
    pub peer_port: u16,
    /// Draft peer host for adding a new peer connection.
    pub peer_add_host: String,
    /// Draft peer port for adding a new peer connection.
    pub peer_add_port: String,
    /// Draft peer name for adding a new peer connection.
    pub peer_add_name: String,
    /// Draft chat message for peer-to-peer messaging.
    pub peer_chat_message: String,
    /// Selected peer ID for the chat panel.
    pub peer_chat_selected: Option<String>,
    /// Transient status line for the peer panel.
    pub peer_status: String,

    // ─── Remaining Module State ──────────────────────────────────────────────
    /// Multimodal attachments for chat.
    pub multimodal_attachments: Vec<crate::editor::multimodal::Attachment>,
    /// Continuation ledger for cross-model context handoff.
    pub continuation_ledger: Option<crate::editor::continuation_ledger::ContinuationLedger>,
    /// Plugin registry (distinct from extension registry).
    pub plugin_registry: crate::editor::plugin_registry::PluginRegistry,
    /// Agent skill file definitions.
    pub skill_files: Vec<crate::editor::skill_file::SkillFile>,
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
        self.mode_layouts = preferences.mode_layouts;
        self.chat.auto_approve = self.auto_approve;
        self.chat.show_thoughts = preferences.show_thoughts;
        self.chat.selected_model = self.selected_model.clone();
        self.chat.thinking_enabled = self.thinking_enabled;

        // Reopen last session's editor tabs (open_editor dedupes by path).
        for tab_path in &preferences.open_tabs {
            let p = PathBuf::from(tab_path);
            if p.is_file() {
                self.open_editor(Some(p));
            }
        }
        if let Some(active) = &preferences.active_tab {
            let ap = PathBuf::from(active);
            if let Some(id) = self
                .tabs
                .iter()
                .find(|t| t.editor_path() == Some(&ap))
                .map(|t| t.id.clone())
            {
                self.active_tab = Some(id);
            }
        }
        self.rebuild_dock();

        self.status_message = format!("Restored {} workspace", self.appearance.profile.label());
    }

    pub fn persist_mission_activity(&self) {
        persist_mission_activity_nda(&self.workspace_root, &self.task_timeline);
    }

    pub fn reload_workspace_provider_settings(&mut self) {
        self.provider_settings = load_workspace_provider_settings(&self.workspace_root);
    }

    pub fn save_provider_settings(&mut self) {
        match save_workspace_provider_settings(&self.workspace_root, &self.provider_settings) {
            Ok(()) => {
                self.status_message = "Saved workspace provider settings".into();
                let _ = self.agent_tx.send(UiToAgentMessage::ReloadProviderConfig);
                let _ = self.agent_tx.send(UiToAgentMessage::ApplySessionState {
                    provider: self.provider,
                    model: self.selected_model.clone(),
                    thinking: self.thinking_enabled,
                });
            }
            Err(err) => {
                self.status_message = err;
            }
        }
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
            WorkspaceProfile::Coder => vec![TabKind::Chat, TabKind::Output],
            WorkspaceProfile::AutomationOperator => {
                vec![TabKind::Orchestrator, TabKind::Chat, TabKind::Output]
            }
            WorkspaceProfile::MissionControl => {
                vec![TabKind::MissionControl, TabKind::Chat, TabKind::Output]
            }
            WorkspaceProfile::Accessibility => vec![TabKind::Chat, TabKind::Output],
        };

        for tab in Self::collect_panel_tabs(&self.tabs, &primary_kinds) {
            if !root_tabs.iter().any(|existing| existing.id == tab.id) {
                root_tabs.push(tab);
            }
        }

        DockState::new(if root_tabs.is_empty() {
            self.tabs.clone()
        } else {
            root_tabs
        })
    }

    pub fn apply_workspace_profile(&mut self, profile: WorkspaceProfile) {
        self.appearance.apply_profile(profile);

        // Tailor the panel arrangement to the work mode so switching feels
        // like night and day, not just a recolor.
        let (left_visible, right_visible) = match profile {
            // Coding: file tree + symbol inspector both in view.
            WorkspaceProfile::Coder => (true, true),
            // Automation: lean on the orchestrator/browser, hide the inspector.
            WorkspaceProfile::AutomationOperator => (true, false),
            // Mission control: hide the file tree, keep the monitoring rail.
            WorkspaceProfile::MissionControl => (false, true),
            // Accessibility: everything visible for maximum context.
            WorkspaceProfile::Accessibility => (true, true),
        };
        self.left_sidebar_visible = left_visible;
        self.right_sidebar_visible = right_visible;

        let mut tabs: Vec<Tab> = self
            .tabs
            .iter()
            .filter(|tab| matches!(tab.kind, TabKind::Editor { .. }))
            .cloned()
            .collect();

        let push_unique = |kind: TabKind, tabs: &mut Vec<Tab>, counter: &mut u64| {
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
                push_unique(TabKind::Chat, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Output, &mut tabs, &mut self.tab_counter);
            }
            WorkspaceProfile::AutomationOperator => {
                push_unique(TabKind::Orchestrator, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Chat, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Output, &mut tabs, &mut self.tab_counter);
            }
            WorkspaceProfile::MissionControl => {
                push_unique(TabKind::MissionControl, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Chat, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Output, &mut tabs, &mut self.tab_counter);
            }
            WorkspaceProfile::Accessibility => {
                push_unique(TabKind::Chat, &mut tabs, &mut self.tab_counter);
                push_unique(TabKind::Output, &mut tabs, &mut self.tab_counter);
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

    /// Quick work-mode switch used by the toolbar, shortcuts and command palette:
    /// re-themes, re-panels, and persists the choice. No-ops if already active.
    /// Remembers the layout of the mode being left and restores the user's own
    /// arrangement for the mode being entered (falling back to its defaults).
    pub fn set_work_mode(&mut self, profile: WorkspaceProfile) {
        if self.appearance.profile == profile {
            return;
        }
        self.snapshot_mode_layout(self.appearance.profile);
        self.apply_workspace_profile(profile);
        self.restore_mode_layout(profile);
        // Apply mode-specific sidebar filter
        self.smart_sidebar.filter_for_mode(profile);
        // Reset bottom panel tab selection for the new mode
        self.bottom_panel_state.active_tab = 0;
        self.save_workspace_preferences();
        self.status_message = format!("Switched to {} mode", profile.label());
        // Central, self-dismissing confirmation — useful when the switch came
        // from a keyboard shortcut and the eye isn't on the toolbar pills.
        self.toasts.push(crate::editor::toast::Toast::info(format!(
            "{} {} mode",
            profile.glyph(),
            profile.label()
        )));
    }

    /// Forget the user's custom arrangement for the active mode and restore its
    /// night-and-day defaults. Exposed via the command palette so a customized
    /// mode can always be returned to its original layout.
    pub fn reset_current_mode_layout(&mut self) {
        let profile = self.appearance.profile;
        self.mode_layouts.remove(&profile);
        self.apply_workspace_profile(profile);
        self.save_workspace_preferences();
        self.status_message = format!("Reset {} layout to default", profile.label());
        self.toasts.push(crate::editor::toast::Toast::info(format!(
            "{} {} layout reset",
            profile.glyph(),
            profile.short_label()
        )));
    }

    /// Record the current sidebar arrangement under the given mode.
    fn snapshot_mode_layout(&mut self, profile: WorkspaceProfile) {
        self.mode_layouts.insert(
            profile,
            ModeLayout {
                left_visible: self.left_sidebar_visible,
                left_width: self.left_sidebar_width,
                right_visible: self.right_sidebar_visible,
                right_width: self.right_sidebar_width,
            },
        );
    }

    /// Restore a previously customized layout for the mode, if one exists.
    /// When none is stored, the mode's night-and-day defaults (already applied
    /// by `apply_workspace_profile`) stand.
    fn restore_mode_layout(&mut self, profile: WorkspaceProfile) {
        if let Some(layout) = self.mode_layouts.get(&profile).copied() {
            self.left_sidebar_visible = layout.left_visible;
            self.left_sidebar_width = layout.left_width.max(180.0);
            self.right_sidebar_visible = layout.right_visible;
            self.right_sidebar_width = layout.right_width.max(220.0);
        }
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
        let chat = Tab {
            id: TabId::next(&mut tab_counter),
            kind: TabKind::Chat,
        };
        let output = Tab {
            id: TabId::next(&mut tab_counter),
            kind: TabKind::Output,
        };
        let tabs = vec![chat.clone(), output.clone()];

        let mut projects = vec![workspace_root.clone()];
        if let Some(parent) = workspace_root.parent() {
            for sub in &["velocity-mcp", "velocity-ide", "ide", "agent"] {
                let path = parent.join(sub);
                if path.exists() && path.is_dir() && !projects.contains(&path) {
                    projects.push(path);
                }
            }
        }

        let provider_settings = load_workspace_provider_settings(&workspace_root);
        let expert_teams = crate::editor::expert_team::load_expert_teams(&workspace_root);
        let mut app = Self {
            agent_tx,
            agent_rx,
            workspace_root: workspace_root.clone(),
            tabs: tabs.clone(),
            active_tab: Some(chat.id.clone()),
            buffers: HashMap::new(),
            dock_state: Some(DockState::new(tabs)),
            chat_history: String::new(),
            command_output: String::from("V.E.L.O.C.I.T.Y. IDE initialized.\n"),
            command_palette: CommandPalette {
                open: false,
                query: String::new(),
                selected: 0,
                just_opened: false,
            },
            show_shortcuts: false,
            quick_open: QuickOpen {
                open: false,
                query: String::new(),
                selected: 0,
                just_opened: false,
                files: Vec::new(),
                last_query: String::new(),
                last_file_count: 0,
                filtered: Vec::new(),
                scroll_to_selected: false,
            },
            mru: MruSwitcher {
                open: false,
                selected: 0,
                order: Vec::new(),
            },
            closed_editor_paths: Vec::new(),
            goto_line_open: false,
            goto_line_input: String::new(),
            goto_line_just_opened: false,
            goto_symbol_open: false,
            goto_symbol_query: String::new(),
            goto_symbol_selected: 0,
            goto_symbol_just_opened: false,
            goto_symbol_entries: Vec::new(),
            workspace_symbols: Vec::new(),
            goto_symbol_last_query: String::new(),
            goto_symbol_filtered: Vec::new(),
            goto_symbol_scroll_to_selected: false,
            nav_back: Vec::new(),
            nav_forward: Vec::new(),
            cached_site_map: None,
            cached_site_map_at: None,
            cached_relation_symbol: None,
            cached_callers: Vec::new(),
            cached_deps: Vec::new(),
            last_diagnostics_poll: None,
            last_external_check: None,
            status_message: String::from("Ready"),
            appearance,
            provider_settings,
            left_sidebar_visible: true,
            left_sidebar_width: 240.0,
            left_sidebar_tab: 0,
            right_sidebar_visible: false,
            right_sidebar_width: 280.0,
            mode_layouts: HashMap::new(),
            tab_counter,
            expert_teams,
            active_team_index: 0,
            selected_member_id: None,
            team_gallery_expanded: None,
            team_builder_chat: crate::editor::team_builder_chat::TeamBuilderChat::default(),
            agent_ui_state: AgentUiState::default(),
            task_timeline: TTState::default(),
            smart_sidebar: SmartSidebarState::default(),
            right_changes_collapsed: false,
            right_symbol_collapsed: false,
            bottom_panel_state: BottomPanelState::default(),
            favorite_files: Vec::new(),
            bookmarks: Vec::new(),
            recording_active: false,
            recordings: Vec::new(),
            projects,
            show_add_project_ui: false,
            new_project_path_input: String::new(),
            workspace_switcher_open: false,
            workspace_switcher_selected: 0,
            workspace_switcher_just_opened: false,
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
            pending_close_tab: None,
            show_full_diff: false,
            build_errors_count: 0,
            account_usage: Vec::new(),
            usage_date: String::new(),
            gpu_name,
            search_query: String::new(),
            search_hits: Vec::new(),
            replace_query: String::new(),
            search_pending_since: None,
            pending_cursor_line: None,
            current_cursor_line: 0,
            current_cursor_col: 0,
            references_open: false,
            references_results: Vec::new(),
            references_selected: 0,
            file_tree: None,
            last_tree_update: std::time::Instant::now(),
            last_tree_mtime: None,
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
                attachments: Vec::new(),
                attach_input: String::new(),
            },
            mediator,
            graph_view: crate::editor::graph_view::MerkleGraphView::new(),
            wiki_view: crate::editor::wiki_view::WikiView::new(),
            nda_docs: std::collections::HashMap::new(),
            terminal_rx: None,
            terminal_input: String::new(),
            current_agent_task_id: 0,
            // IDE Feature Integration
            completion_state: crate::editor::completion::CompletionState::default(),
            lsp_manager: None,
            diagnostics: crate::editor::diagnostics::DiagnosticsState::default(),
            terminal_state: crate::editor::terminal::TerminalState::new(80, 24),
            terminal_spawned: false,
            dap_client: None,
            keybindings_config: crate::editor::keybindings::KeybindingsConfig::default(),
            git_state: crate::editor::git_ui::GitState::default(),
            extension_registry: crate::editor::extensions::ExtensionRegistry::default(),
            minimap_config: crate::editor::minimap::MinimapConfig::default(),
            snippet_collection: crate::editor::snippets::SnippetCollection::default(),
            show_minimap: true,
            show_breadcrumbs: true,
            word_wrap: false,
            browse_state: crate::editor::browse_panel::BrowseState::default(),
            checkpoint_manager: crate::editor::checkpoint::CheckpointManager::new(&workspace_root),
            agent_memory: {
                let mut mgr = crate::editor::agent_memory::AgentMemoryManager::new(&workspace_root);
                mgr.load_all();
                mgr
            },
            live_orchestration: crate::editor::live_orchestration::LiveOrchestrationState::new(),
            precomp_cache: crate::editor::speculative_precomp::PrecomputationCache::new(),
            semantic_index: None,
            semantic_search_active: false,
            inline_suggestions: crate::editor::inline_suggestions::InlineSuggestionEngine::default(
            ),
            test_generator: crate::editor::test_generator::TestGenerator::default(),
            deploy_pipeline: None,
            voice_input: crate::editor::voice_commands::VoiceInputState::new(),
            knowledge_base: crate::editor::knowledge_base::KnowledgeBase::load(&workspace_root),
            knowledge_query: String::new(),
            knowledge_ingest_input: String::new(),
            knowledge_results: Vec::new(),
            triggers: crate::editor::triggers::TriggerRegistry::load(&workspace_root),
            trigger_name_input: String::new(),
            trigger_interval_input: String::new(),
            trigger_prompt_input: String::new(),
            workflows: crate::editor::workflow::WorkflowRegistry::load(&workspace_root),
            workflow_name_input: String::new(),
            workflow_selected: None,
            workflow_step_tool_input: String::new(),
            workflow_step_args_input: String::new(),
            workflow_step_prompt_input: String::new(),
            workflow_last_run: None,
            workflow_canvases: std::collections::HashMap::new(),
            workflow_canvas_selected: None,
            workflow_visual_mode: false,
            workflow_ai_prompt: String::new(),
            workflow_versions: crate::editor::workflow_version::VersionRegistry::load(
                &workspace_root,
            ),
            policy: crate::editor::governance::PolicyEngine::load(&workspace_root),
            approvals: crate::editor::governance::ApprovalQueue::load(&workspace_root),
            secrets: crate::security::secrets::SecretStore::load(&workspace_root),
            connectors: crate::connectors::ConnectorRegistry::load(&workspace_root),
            gov_rule_tool_input: String::new(),
            gov_rule_path_input: String::new(),
            gov_secret_name_input: String::new(),
            gov_secret_value_input: String::new(),
            gov_connector_id_input: String::new(),
            gov_connector_url_input: String::new(),
            gov_connector_secret_input: String::new(),
            gov_status: String::new(),
            // Cross-device peer collaboration
            peer_manager: {
                let mut mgr = crate::agent::peer_link::PeerManager::new();
                let hostname = std::env::var("COMPUTERNAME")
                    .or_else(|_| std::env::var("HOSTNAME"))
                    .unwrap_or_else(|_| "velocity-instance".to_string());
                mgr.init(&workspace_root, &hostname);
                mgr
            },
            peer_server_running: false,
            peer_port: 9191,
            peer_add_host: String::new(),
            peer_add_port: String::new(),
            peer_add_name: String::new(),
            peer_chat_message: String::new(),
            peer_chat_selected: None,
            peer_status: String::new(),
            // Remaining Module State
            multimodal_attachments: Vec::new(),
            continuation_ledger: None,
            plugin_registry: crate::editor::plugin_registry::PluginRegistry::new(&workspace_root),
            skill_files: Vec::new(),
        };
        app.open_editor(None);
        app.apply_workspace_profile(app.appearance.profile);
        app.restore_workspace_preferences();
        app.apply_appearance(&cc.egui_ctx);
        app.task_timeline
            .session_marker("IDE session ready", "agentic workspace initialized");
        app.persist_mission_activity();
        let _ = app.agent_tx.send(UiToAgentMessage::ApplySessionState {
            provider: app.provider,
            model: app.selected_model.clone(),
            thinking: app.thinking_enabled,
        });
        app.save_workspace_preferences();
        // Initialize LSP manager (auto-detect language servers)
        app.lsp_manager = Some(crate::editor::lsp_client::LspManager::auto_detect(
            &app.workspace_root,
        ));
        // Initialize git state
        app.git_state.refresh(&app.workspace_root);
        // Load keybindings from workspace config
        app.keybindings_config =
            crate::editor::keybindings::KeybindingsConfig::load(&app.workspace_root);
        // Load snippets
        let snippets_path = app.workspace_root.join(".velocity").join("snippets.json");
        app.snippet_collection =
            crate::editor::snippets::SnippetCollection::load_from_file(&snippets_path);
        app
    }
}
