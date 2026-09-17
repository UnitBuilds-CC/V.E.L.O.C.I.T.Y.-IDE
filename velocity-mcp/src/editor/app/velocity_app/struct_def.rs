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
use super::substructs::{GovernanceState, LspState, PeerCollabState, WorkflowAppState};
use crate::agent::AiProvider;
use crate::editor::theme::{apply_theme, AppearanceSettings, IdePalette, WorkspaceProfile};

/// Left sidebar width bounds, in logical pixels.
///
/// `MIN_W` keeps the file tree legible when the panel is dragged narrow. `MAX_W`
/// is an absolute ceiling; at render time the effective cap is further limited to
/// a fraction of the window (see `ui_render.rs`) so the sidebar can be pulled out
/// into the canvas for wide panels — e.g. Team Studio's two-column layout —
/// without ever swallowing the editor. The render loop and the preference/layout
/// loaders all clamp to these bounds, so a saved width survives a reload or a mode
/// switch instead of snapping back to the old 420px cap.
pub const LEFT_SIDEBAR_MIN_W: f32 = 180.0;
pub const LEFT_SIDEBAR_MAX_W: f32 = 1400.0;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ModeLayout {
    pub left_visible: bool,
    pub left_width: f32,
    pub right_visible: bool,
    pub right_width: f32,
}

/// Pre-formatted display strings for a search hit. Built once when hits change,
/// then reused every frame — avoids 4-5 `format!()` allocations per hit during render.
#[derive(Debug, Clone)]
pub struct SearchHitDisplay {
    /// File name for the clickable link (e.g., "main.rs").
    pub file_name: String,
    /// Full path for hover tooltip (e.g., "src/main.rs").
    pub path_display: String,
    /// "path : line N" label for the metadata row.
    pub path_line: String,
    /// Truncated code preview (≤80 chars + ellipsis).
    pub text_preview: String,
    /// Pre-computed icon glyph for this file type (e.g., "rs", "md", "{}").
    pub icon: &'static str,
    /// Pre-formatted link label: "{icon} {file_name}".
    pub link_label: String,
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
    /// Back/forward navigation history (Alt+â† / Alt+â†’).
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
    /// Throttle for syncing active buffer content to LSP server.
    pub last_lsp_sync: Option<Instant>,
    pub status_message: String,
    pub appearance: AppearanceSettings,
    /// Last-applied appearance snapshot; used to avoid re-applying theme/style every frame.
    pub last_applied_appearance: Option<AppearanceSettings>,
    /// When true the unified single-row header is used and the legacy toolbar
    /// top panel is suppressed to avoid rendering a double header.
    pub use_unified_header: bool,
    pub provider_settings: WorkspaceProviderSettings,
    pub left_sidebar_visible: bool,
    pub left_sidebar_width: f32,
    pub left_sidebar_tab: usize,
    /// Activity bar selection (0=Files, 1=Search, 2=Git, 3=Chat, 4=Build, 5=Agents, 6=Knowledge, 7=Workspace)
    pub activity_bar_selection: usize,
    /// Sub-panel selection within each activity bar category
    pub activity_sub_panel: [usize; 8],
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
    /// Draft fields for direct Team Studio creation flows.
    pub team_name_input: String,
    pub team_description_input: String,
    /// Draft fields for creating a reusable agent and assigning it to a team.
    pub team_agent_name_input: String,
    pub team_agent_role_input: String,
    pub team_agent_scope_input: String,
    pub team_agent_instructions_input: String,
    pub team_agent_target_index: Option<usize>,
    /// UI-facing manager that bridges Team Studio controls to the agent runtime.
    pub team_manager: crate::editor::app::team_manager::TeamManager,

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
    /// Pre-formatted display strings for search hits (avoids per-frame format! allocations).
    pub search_hit_cache: Vec<SearchHitDisplay>,
    /// Pre-formatted "N results" label (updated only when hits change).
    pub search_count_label: String,
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
    /// Channel for receiving completed background file-tree builds.
    pub file_tree_rx: crossbeam_channel::Receiver<(FileNode, Option<std::time::SystemTime>)>,
    /// Sending end kept so we can clone it for each background spawn.
    pub file_tree_tx: crossbeam_channel::Sender<(FileNode, Option<std::time::SystemTime>)>,
    /// True while a background tree build is running.
    pub tree_build_in_flight: bool,
    /// Channel for receiving completed background file I/O results.
    pub file_io_rx: crossbeam_channel::Receiver<FileIoResult>,
    /// Sending end cloned for each background file operation.
    pub file_io_tx: crossbeam_channel::Sender<FileIoResult>,
    /// Tab IDs with a pending background file load (prevents duplicate spawns).
    pub pending_file_loads: std::collections::HashSet<TabId>,
    /// Cached disk content for the active change preview (avoids per-frame reads).
    pub preview_disk_cache: Option<(std::path::PathBuf, std::time::SystemTime, String)>,
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
    /// Set when the user requests task cancellation (Interrupt/Stop). Checked in
    /// `handle_agent_messages` to emit a `Cancelled` timeline event instead of
    /// `Completed` when the agent finishes.
    pub cancel_requested: bool,

    pub chat_history: String,

    // â”€â”€â”€ IDE Feature Integration State â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    /// Code completion popup state.
    pub completion_state: crate::editor::completion::CompletionState,
    /// LSP client manager and diagnostics state.
    /// Groups lsp_manager and diagnostics into a focused sub-struct.
    pub lsp_state: LspState,
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
    /// OS-level file watcher for instant external change detection.
    pub file_watcher: Option<crate::editor::file_watcher::FileWatcher>,
    /// Extension registry.
    pub extension_registry: crate::editor::extensions::ExtensionRegistry,
    /// Minimap configuration.
    pub minimap_config: crate::editor::minimap::MinimapConfig,
    /// Snippet collection.
    pub snippet_collection: crate::editor::snippets::SnippetCollection,
    /// Draft filter text in the Snippets panel search box.
    pub snippet_search_query: String,
    /// Draft filter text in the file tree sub-panel.
    pub file_tree_filter: String,
    /// Draft filter text in the skills sub-panel.
    pub skill_filter: String,
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
    /// Whether agent_memory has been loaded from disk (lazy-loaded on first access).
    pub agent_memory_loaded: bool,
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
    /// Whether knowledge_base has been loaded from disk (lazy-loaded on first access).
    pub knowledge_base_loaded: bool,
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
    /// Workflow composer state (registry + UI draft fields + canvas + versions).
    /// Groups ~12 workflow-related fields into a focused sub-struct.
    pub workflow_state: WorkflowAppState,
    /// Governance policy engine, approval queue, secrets, connectors.
    /// Groups ~12 governance-related fields into a focused sub-struct.
    pub governance: GovernanceState,

    // â"€â"€â"€ Cross-device Peer Collaboration â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€â"€
    /// Cross-device peer collaboration state (manager + UI draft fields).
    /// Groups ~10 peer-related fields into a focused sub-struct.
    pub peer_state: PeerCollabState,

    // â”€â”€â”€ Remaining Module State â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    /// Multimodal attachments for chat.
    pub multimodal_attachments: Vec<crate::editor::multimodal::Attachment>,
    /// Continuation ledger for cross-model context handoff.
    pub continuation_ledger: Option<crate::editor::continuation_ledger::ContinuationLedger>,
    /// Plugin registry (distinct from extension registry).
    pub plugin_registry: crate::editor::plugin_registry::PluginRegistry,
    /// Agent skill file definitions.
    pub skill_files: Vec<crate::editor::skill_file::SkillFile>,
    /// Target URLs for the Targets dock panel.
    pub target_entries: Vec<crate::editor::sidebar_tabs::TargetEntry>,
    /// WCAG accessibility audit findings for the Audit dock panel.
    pub audit_findings: Vec<crate::editor::sidebar_tabs::AuditFinding>,

    // â”€â”€â”€ Agent Subsystem Panels â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    /// Self-improvement engine tracking failures and generating refinements.
    pub improvement_engine: crate::agent::self_improve::ImprovementEngine,
    /// Shared knowledge store for multi-agent collaboration.
    pub shared_memory: crate::agent::shared_memory::SharedMemoryStore,
    /// Background agent registry for autonomous background tasks.
    pub background_agents: crate::agent::background_agents::BackgroundAgentRegistry,
    /// Multi-agent conflict resolver for resource contention.
    pub conflict_resolver: crate::agent::conflict_resolution::ConflictResolver,
    /// Collaboration manager for shared sessions and presence.
    pub collaboration: crate::agent::collaboration::CollaborationManager,
    /// Persistent memory store (NDA-encrypted at rest).
    pub persistent_memory: crate::agent::memory_store::PersistentMemory,

    // ─── Performance Profiling ──────────────────────────────────────────────
    /// Last frame's duration in milliseconds (for display).
    pub last_frame_ms: f32,
    /// When the last frame started (for computing delta).
    pub last_frame_instant: Option<Instant>,
    /// Cached status-bar perf label (e.g., "Ready | 13ms"). Updated each frame
    /// but reuses the same String buffer — avoids 2 format!() allocations per frame.
    pub cached_status_perf: String,
    /// Cached profile label (e.g., "⚡ Coder").
    pub cached_profile_label: String,
    /// Cached right-sidebar "Changes" header (collapsed/expanded variants).
    pub cached_changes_header_right: String,
    pub cached_changes_header_down: String,
    /// Cached right-sidebar diff-stat label (e.g., "+12 -3").
    pub cached_diff_stat: String,
    /// Cached right-sidebar "Symbols" header (collapsed/expanded variants).
    pub cached_sym_header_right: String,
    pub cached_sym_header_down: String,

    // ─── GUI Control Bridge ────────────────────────────────────────────────
    /// Receiver for commands from external processes (MCP server, AI agents).
    /// The gui_control listener thread sends (command, response_sender) pairs.
    pub gui_cmd_rx: Option<
        crossbeam_channel::Receiver<(
            crate::editor::gui_control::GuiCommand,
            crossbeam_channel::Sender<crate::editor::gui_control::GuiResponse>,
        )>,
    >,
    /// Handle to the gui_control listener (holds shutdown flag).
    pub gui_control_handle: Option<crate::editor::gui_control::GuiControlHandle>,
}

impl VelocityApp {
    /// Surface a persistence failure as a toast notification.
    /// Usage: `Self::persist_err(&mut self.toasts, "knowledge_base", &err);`
    pub(crate) fn persist_err(
        toasts: &mut crate::editor::toast::ToastQueue,
        subsystem: &str,
        err: &str,
    ) {
        toasts.push(crate::editor::toast::Toast::error(format!(
            "Failed to save {subsystem}: {err}"
        )));
    }

    fn workspace_state_dir(workspace_root: &Path) -> PathBuf {
        workspace_root.join(".velocity")
    }

    fn workspace_preferences_path(workspace_root: &Path) -> PathBuf {
        Self::workspace_state_dir(workspace_root).join("workspace-preferences.json")
    }

    fn parse_provider_label(label: &str) -> Option<AiProvider> {
        // Full label round-trip (with slug fallback) — previously only 4 of
        // the 16 providers restored, silently reverting e.g. "Alibaba Qwen"
        // to the default on every restart.
        AiProvider::from_label(label)
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
        self.left_sidebar_width = preferences
            .left_sidebar_width
            .clamp(LEFT_SIDEBAR_MIN_W, LEFT_SIDEBAR_MAX_W);
        self.right_sidebar_visible = preferences.right_sidebar_visible;
        self.right_sidebar_width = preferences.right_sidebar_width.clamp(220.0, 600.0);
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

    pub fn persist_mission_activity(&self) -> Result<(), String> {
        persist_mission_activity_nda(&self.workspace_root, &self.task_timeline)
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

    pub fn apply_appearance(&mut self, ctx: &egui::Context) {
        // Avoid rebuilding and reapplying the full egui Style every frame. Only
        // re-apply when the appearance settings actually change.
        if self.last_applied_appearance != Some(self.appearance) {
            apply_theme(ctx, self.appearance);
            self.last_applied_appearance = Some(self.appearance);
        }
    }

    /// Assign new search hits and rebuild the pre-formatted display cache.
    /// Called whenever `search_hits` changes — avoids 4-5 `format!()` allocations
    /// per hit during every render frame.
    pub fn update_search_hits(&mut self, hits: Vec<crate::editor::search::SearchHit>) {
        self.search_hit_cache = hits
            .iter()
            .map(|hit| {
                let file_name = hit
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| hit.path.display().to_string());
                let path_display = hit.path.display().to_string();
                let path_line = format!("{} : line {}", hit.path.display(), hit.line);
                let text_preview = if hit.text.len() > 80 {
                    format!("{}\u{2026}", &hit.text[..80])
                } else {
                    hit.text.clone()
                };
                let icon = crate::editor::search::icon_for_path(&hit.path);
                let link_label = format!("{} {}", icon, file_name);
                SearchHitDisplay {
                    file_name,
                    path_display,
                    path_line,
                    text_preview,
                    icon,
                    link_label,
                }
            })
            .collect();
        self.search_count_label = format!("{} results", self.search_hit_cache.len());
        self.search_hits = hits;
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
        // like night and day, not just a recolor. Avoid unnecessary dock
        // rebuilds that cause visual jitter by only rebuilding when the set
        // of primary tabs or sidebar visibility actually change.
        let (new_left_visible, new_right_visible) = match profile {
            WorkspaceProfile::Coder => (true, true),
            WorkspaceProfile::AutomationOperator => (true, false),
            WorkspaceProfile::MissionControl => (false, true),
            WorkspaceProfile::Accessibility => (true, true),
        };

        // Collect current editor tabs (kinds) and prepare the desired set.
        let mut desired_tabs: Vec<Tab> = self
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
                push_unique(TabKind::Chat, &mut desired_tabs, &mut self.tab_counter);
                push_unique(TabKind::Output, &mut desired_tabs, &mut self.tab_counter);
            }
            WorkspaceProfile::AutomationOperator => {
                push_unique(
                    TabKind::Orchestrator,
                    &mut desired_tabs,
                    &mut self.tab_counter,
                );
                push_unique(TabKind::Chat, &mut desired_tabs, &mut self.tab_counter);
                push_unique(TabKind::Output, &mut desired_tabs, &mut self.tab_counter);
            }
            WorkspaceProfile::MissionControl => {
                push_unique(
                    TabKind::MissionControl,
                    &mut desired_tabs,
                    &mut self.tab_counter,
                );
                push_unique(TabKind::Chat, &mut desired_tabs, &mut self.tab_counter);
                push_unique(TabKind::Output, &mut desired_tabs, &mut self.tab_counter);
            }
            WorkspaceProfile::Accessibility => {
                push_unique(TabKind::Chat, &mut desired_tabs, &mut self.tab_counter);
                push_unique(TabKind::Output, &mut desired_tabs, &mut self.tab_counter);
            }
        }

        // Determine whether visible sidebars or tab kinds changed; only rebuild
        // dock when those change to reduce layout churn (fixes jitter on mode swap).
        let sidebars_changed = (self.left_sidebar_visible != new_left_visible)
            || (self.right_sidebar_visible != new_right_visible);

        let current_kinds: Vec<std::mem::Discriminant<TabKind>> = self
            .tabs
            .iter()
            .map(|t| std::mem::discriminant(&t.kind))
            .collect();
        let desired_kinds: Vec<std::mem::Discriminant<TabKind>> = desired_tabs
            .iter()
            .map(|t| std::mem::discriminant(&t.kind))
            .collect();

        let tabs_changed = current_kinds != desired_kinds;

        // Apply visibility and tabs; prefer restoring a previously customized
        // layout for this profile (avoids sudden size/visibility changes that
        // cause panels like Code/Automate to 'jitter' when the central area
        // resizes). If no saved layout exists, fall back to the default mapping.
        if let Some(layout) = self.mode_layouts.get(&profile) {
            self.left_sidebar_visible = layout.left_visible;
            // keep the stored width but constrain it to reasonable bounds
            // Clamp sidebar widths to the same bounds the render loop uses
            // so switching modes never causes a visible size jump.
            self.left_sidebar_width = layout
                .left_width
                .clamp(LEFT_SIDEBAR_MIN_W, LEFT_SIDEBAR_MAX_W);
            self.right_sidebar_width = layout.right_width.clamp(220.0, 600.0);
            self.right_sidebar_visible = layout.right_visible;
        } else {
            self.left_sidebar_visible = new_left_visible;
            self.right_sidebar_visible = new_right_visible;
            // Ensure sensible default widths so toggling doesn't aggressively
            // shrink the central content when panels appear/disappear.
            if self.left_sidebar_width < LEFT_SIDEBAR_MIN_W {
                self.left_sidebar_width = 220.0;
            }
            if self.right_sidebar_width < 220.0 {
                self.right_sidebar_width = 260.0;
            }
        }

        self.tabs = desired_tabs;
        if sidebars_changed || tabs_changed {
            self.rebuild_dock();
        }

        let focus_kind = match profile {
            WorkspaceProfile::Coder => TabKind::Chat,
            WorkspaceProfile::AutomationOperator => TabKind::Orchestrator,
            WorkspaceProfile::MissionControl => TabKind::MissionControl,
            WorkspaceProfile::Accessibility => TabKind::Settings,
        };

        // Only change focus if we rebuilt or the focused kind is missing.
        if sidebars_changed || tabs_changed || self.active_tab.is_none() {
            self.focus_panel(focus_kind);
        }

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
        self.bottom_panel_state.active_tab = crate::editor::bottom_panel::TAB_TERMINAL;
        self.save_workspace_preferences();
        self.status_message = format!("Switched to {} mode", profile.label());
        // Central, self-dismissing confirmation -- useful when the switch came
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
            self.left_sidebar_width = layout
                .left_width
                .clamp(LEFT_SIDEBAR_MIN_W, LEFT_SIDEBAR_MAX_W);
            self.right_sidebar_visible = layout.right_visible;
            self.right_sidebar_width = layout.right_width.clamp(220.0, 600.0);
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
        let (tree_tx, tree_rx) = crossbeam_channel::unbounded();
        let (file_io_tx, file_io_rx) = crossbeam_channel::unbounded();

        let mut app = Self {
            agent_tx: agent_tx.clone(),
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
            last_lsp_sync: None,
            status_message: String::from("Ready"),
            appearance,
            last_applied_appearance: Some(appearance),
            use_unified_header: true,
            provider_settings,
            left_sidebar_visible: true,
            left_sidebar_width: 240.0,
            left_sidebar_tab: 0,
            activity_bar_selection: 0,
            activity_sub_panel: [0; 8],
            right_sidebar_visible: false,
            right_sidebar_width: 280.0,
            mode_layouts: HashMap::new(),
            tab_counter,
            expert_teams,
            active_team_index: 0,
            selected_member_id: None,
            team_gallery_expanded: None,
            team_builder_chat: crate::editor::team_builder_chat::TeamBuilderChat::default(),
            team_name_input: String::new(),
            team_description_input: String::new(),
            team_agent_name_input: String::new(),
            team_agent_role_input: String::new(),
            team_agent_scope_input: String::new(),
            team_agent_instructions_input: String::new(),
            team_agent_target_index: None,
            // UI-facing manager that bridges Team Studio controls to the agent runtime.
            team_manager: crate::editor::app::team_manager::TeamManager::new(agent_tx.clone()),
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
            search_hit_cache: Vec::new(),
            search_count_label: String::new(),
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
            file_tree_rx: tree_rx,
            file_tree_tx: tree_tx,
            tree_build_in_flight: false,
            file_io_rx,
            file_io_tx,
            pending_file_loads: std::collections::HashSet::new(),
            preview_disk_cache: None,
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
                clear_timeline: false,
            },
            mediator,
            graph_view: crate::editor::graph_view::MerkleGraphView::new(),
            wiki_view: crate::editor::wiki_view::WikiView::new(),
            nda_docs: std::collections::HashMap::new(),
            terminal_rx: None,
            terminal_input: String::new(),
            current_agent_task_id: 0,
            cancel_requested: false,
            // IDE Feature Integration
            completion_state: crate::editor::completion::CompletionState::default(),
            lsp_state: LspState::default(),
            terminal_state: crate::editor::terminal::TerminalState::new(80, 24),
            terminal_spawned: false,
            dap_client: None,
            keybindings_config: crate::editor::keybindings::KeybindingsConfig::default(),
            git_state: crate::editor::git_ui::GitState::default(),
            file_watcher: None,
            extension_registry: crate::editor::extensions::ExtensionRegistry::default(),
            minimap_config: crate::editor::minimap::MinimapConfig::default(),
            snippet_collection: crate::editor::snippets::SnippetCollection::default(),
            snippet_search_query: String::new(),
            file_tree_filter: String::new(),
            skill_filter: String::new(),
            show_minimap: true,
            show_breadcrumbs: true,
            word_wrap: false,
            browse_state: crate::editor::browse_panel::BrowseState::default(),
            checkpoint_manager: crate::editor::checkpoint::CheckpointManager::new(&workspace_root),
            agent_memory: crate::editor::agent_memory::AgentMemoryManager::new(&workspace_root),
            agent_memory_loaded: false, // Lazy-loaded on first access
            live_orchestration: crate::editor::live_orchestration::LiveOrchestrationState::new(),
            precomp_cache: crate::editor::speculative_precomp::PrecomputationCache::new(),
            semantic_index: None,
            semantic_search_active: false,
            inline_suggestions: crate::editor::inline_suggestions::InlineSuggestionEngine::default(
            ),
            test_generator: crate::editor::test_generator::TestGenerator::default(),
            deploy_pipeline: None,
            voice_input: crate::editor::voice_commands::VoiceInputState::new(),
            knowledge_base: crate::editor::knowledge_base::KnowledgeBase::new(),
            knowledge_base_loaded: false, // Lazy-loaded on first access
            knowledge_query: String::new(),
            knowledge_ingest_input: String::new(),
            knowledge_results: Vec::new(),
            triggers: crate::editor::triggers::TriggerRegistry::load(&workspace_root),
            trigger_name_input: String::new(),
            trigger_interval_input: String::new(),
            trigger_prompt_input: String::new(),
            workflow_state: WorkflowAppState::new(&workspace_root),
            governance: GovernanceState::new(&workspace_root),
            // Cross-device peer collaboration
            peer_state: PeerCollabState::new(&workspace_root),
            // Remaining Module State
            multimodal_attachments: Vec::new(),
            continuation_ledger: None,
            plugin_registry: crate::editor::plugin_registry::PluginRegistry::new(&workspace_root),
            skill_files: Vec::new(),
            target_entries: Vec::new(),
            audit_findings: Vec::new(),
            // Agent Subsystem Panels
            persistent_memory: crate::agent::memory_store::PersistentMemory::open(&workspace_root),
            improvement_engine: {
                let mem = crate::agent::memory_store::PersistentMemory::open(&workspace_root);
                crate::agent::self_improve::ImprovementEngine::new(&mem)
            },
            shared_memory: crate::agent::shared_memory::SharedMemoryStore::new(),
            background_agents: crate::agent::background_agents::BackgroundAgentRegistry::new(),
            conflict_resolver: crate::agent::conflict_resolution::ConflictResolver::new(),
            collaboration: crate::agent::collaboration::CollaborationManager::new(),
            // Performance profiling
            last_frame_ms: 0.0,
            last_frame_instant: None,
            cached_status_perf: String::new(),
            cached_profile_label: String::new(),
            cached_changes_header_right: String::new(),
            cached_changes_header_down: String::new(),
            cached_diff_stat: String::new(),
            cached_sym_header_right: String::new(),
            cached_sym_header_down: String::new(),
            // GUI Control Bridge — start the named pipe listener
            gui_cmd_rx: None,
            gui_control_handle: None,
        };
        // Start the GUI control listener (TCP for external MCP/agent control)
        let auth_token = crate::editor::gui_control::load_or_generate_token(&workspace_root);
        let (cmd_rx, shutdown) = crate::editor::gui_control::start_listener(cc.egui_ctx.clone(), auth_token);
        app.gui_cmd_rx = Some(cmd_rx);
        app.gui_control_handle = Some(crate::editor::gui_control::GuiControlHandle { shutdown });
        // Don't create an untitled editor by default — show the welcome screen instead.
        // Users can open files or create new files via Ctrl+O / Ctrl+N.
        app.apply_workspace_profile(app.appearance.profile);
        app.restore_workspace_preferences();
        app.apply_appearance(&cc.egui_ctx);
        app.task_timeline.clear();
        app.task_timeline
            .session_marker("IDE session ready", "agentic workspace initialized");
        let _ = app.persist_mission_activity();
        let _ = app.agent_tx.send(UiToAgentMessage::ApplySessionState {
            provider: app.provider,
            model: app.selected_model.clone(),
            thinking: app.thinking_enabled,
        });
        app.save_workspace_preferences();
        // Initialize LSP manager (auto-detect language servers)
        app.lsp_state = LspState::new(&app.workspace_root);
        // Initialize git state
        app.git_state.refresh(&app.workspace_root);
        // Start OS-level file watcher for instant external change detection
        app.file_watcher = crate::editor::file_watcher::FileWatcher::new(&app.workspace_root);
        // Load keybindings from workspace config
        app.keybindings_config =
            crate::editor::keybindings::KeybindingsConfig::load(&app.workspace_root);
        // Load snippets
        let snippets_path = app.workspace_root.join(".velocity").join("snippets.json");
        app.snippet_collection =
            crate::editor::snippets::SnippetCollection::load_from_file(&snippets_path);
        app
    }

    /// Create a minimal VelocityApp instance for testing purposes.
    /// All fields are initialized with sensible defaults — no disk I/O, no network,
    /// no egui context required. Tests can override specific fields after construction.
    #[cfg(test)]
    pub fn test_stub() -> Self {
        let workspace_root = std::env::temp_dir().join("velocity_test_stub");
        let _ = std::fs::create_dir_all(&workspace_root);
        let (agent_tx, _) = crossbeam_channel::unbounded();
        let (_, agent_rx) = crossbeam_channel::unbounded();
        let (tree_tx, tree_rx) = crossbeam_channel::unbounded();
        let (file_io_tx, file_io_rx) = crossbeam_channel::unbounded();
        let mediator = std::sync::Arc::new(crate::automation::mediator::MediatorArena::new());

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

        Self {
            agent_tx: agent_tx.clone(),
            agent_rx,
            workspace_root: workspace_root.clone(),
            tabs: tabs.clone(),
            active_tab: Some(chat.id.clone()),
            buffers: HashMap::new(),
            dock_state: Some(DockState::new(tabs)),
            chat_history: String::new(),
            command_output: String::new(),
            command_palette: CommandPalette::default(),
            show_shortcuts: false,
            quick_open: QuickOpen::default(),
            mru: MruSwitcher::default(),
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
            last_lsp_sync: None,
            status_message: String::new(),
            appearance: AppearanceSettings::default(),
            last_applied_appearance: None,
            use_unified_header: true,
            provider_settings: WorkspaceProviderSettings::default(),
            left_sidebar_visible: true,
            left_sidebar_width: 240.0,
            left_sidebar_tab: 0,
            activity_bar_selection: 0,
            activity_sub_panel: [0; 8],
            right_sidebar_visible: false,
            right_sidebar_width: 280.0,
            mode_layouts: HashMap::new(),
            tab_counter,
            expert_teams: Vec::new(),
            active_team_index: 0,
            selected_member_id: None,
            team_gallery_expanded: None,
            team_builder_chat: Default::default(),
            team_name_input: String::new(),
            team_description_input: String::new(),
            team_agent_name_input: String::new(),
            team_agent_role_input: String::new(),
            team_agent_scope_input: String::new(),
            team_agent_instructions_input: String::new(),
            team_agent_target_index: None,
            team_manager: crate::editor::app::team_manager::TeamManager::new(agent_tx),
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
            projects: vec![workspace_root.clone()],
            show_add_project_ui: false,
            new_project_path_input: String::new(),
            workspace_switcher_open: false,
            workspace_switcher_selected: 0,
            workspace_switcher_just_opened: false,
            agent_active: false,
            pending_approvals: Vec::new(),
            auto_approve: false,
            available_models: Vec::new(),
            selected_model: String::new(),
            thinking_enabled: false,
            thinking_supported: false,
            tools_supported: false,
            models_loading: false,
            provider: AiProvider::CloudflareWorkersAi,
            pending_open_path: None,
            pending_save_as_path: None,
            pending_close_tab: None,
            show_full_diff: false,
            build_errors_count: 0,
            account_usage: Vec::new(),
            usage_date: String::new(),
            gpu_name: String::new(),
            search_query: String::new(),
            search_hits: Vec::new(),
            search_hit_cache: Vec::new(),
            search_count_label: String::new(),
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
            file_tree_rx: tree_rx,
            file_tree_tx: tree_tx,
            tree_build_in_flight: false,
            file_io_rx,
            file_io_tx,
            pending_file_loads: std::collections::HashSet::new(),
            preview_disk_cache: None,
            toasts: Default::default(),
            orchestrator: OrchestratorPanel::new(),
            mission_control: MissionControlState::new(),
            next_intervention_id: 1,
            chat: ChatPanelState::default(),
            mediator,
            graph_view: crate::editor::graph_view::MerkleGraphView::new(),
            wiki_view: crate::editor::wiki_view::WikiView::new(),
            nda_docs: std::collections::HashMap::new(),
            terminal_rx: None,
            terminal_input: String::new(),
            current_agent_task_id: 0,
            cancel_requested: false,
            completion_state: Default::default(),
            lsp_state: LspState::default(),
            terminal_state: crate::editor::terminal::TerminalState::new(80, 24),
            terminal_spawned: false,
            dap_client: None,
            keybindings_config: Default::default(),
            git_state: Default::default(),
            file_watcher: None,
            extension_registry: Default::default(),
            minimap_config: Default::default(),
            snippet_collection: Default::default(),
            snippet_search_query: String::new(),
            file_tree_filter: String::new(),
            skill_filter: String::new(),
            show_minimap: true,
            show_breadcrumbs: true,
            word_wrap: false,
            browse_state: Default::default(),
            checkpoint_manager: crate::editor::checkpoint::CheckpointManager::new(&workspace_root),
            agent_memory: crate::editor::agent_memory::AgentMemoryManager::new(&workspace_root),
            agent_memory_loaded: false,
            live_orchestration: crate::editor::live_orchestration::LiveOrchestrationState::new(),
            precomp_cache: crate::editor::speculative_precomp::PrecomputationCache::new(),
            semantic_index: None,
            semantic_search_active: false,
            inline_suggestions: Default::default(),
            test_generator: Default::default(),
            deploy_pipeline: None,
            voice_input: crate::editor::voice_commands::VoiceInputState::new(),
            knowledge_base: crate::editor::knowledge_base::KnowledgeBase::new(),
            knowledge_base_loaded: false,
            knowledge_query: String::new(),
            knowledge_ingest_input: String::new(),
            knowledge_results: Vec::new(),
            triggers: Default::default(),
            trigger_name_input: String::new(),
            trigger_interval_input: String::new(),
            trigger_prompt_input: String::new(),
            workflow_state: WorkflowAppState::default(),
            governance: GovernanceState::default(),
            peer_state: PeerCollabState::default(),
            multimodal_attachments: Vec::new(),
            continuation_ledger: None,
            plugin_registry: crate::editor::plugin_registry::PluginRegistry::new(&workspace_root),
            skill_files: Vec::new(),
            target_entries: Vec::new(),
            audit_findings: Vec::new(),
            persistent_memory: crate::agent::memory_store::PersistentMemory::open(&workspace_root),
            improvement_engine: {
                let mem = crate::agent::memory_store::PersistentMemory::open(&workspace_root);
                crate::agent::self_improve::ImprovementEngine::new(&mem)
            },
            shared_memory: crate::agent::shared_memory::SharedMemoryStore::new(),
            background_agents: crate::agent::background_agents::BackgroundAgentRegistry::new(),
            conflict_resolver: crate::agent::conflict_resolution::ConflictResolver::new(),
            collaboration: crate::agent::collaboration::CollaborationManager::new(),
            last_frame_ms: 0.0,
            last_frame_instant: None,
            cached_status_perf: String::new(),
            cached_profile_label: String::new(),
            cached_changes_header_right: String::new(),
            cached_changes_header_down: String::new(),
            cached_diff_stat: String::new(),
            cached_sym_header_right: String::new(),
            cached_sym_header_down: String::new(),
            gui_cmd_rx: None,
            gui_control_handle: None,
        }
    }
}
