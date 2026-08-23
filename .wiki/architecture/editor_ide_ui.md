# velocity-mcp: Editor & IDE UI

The `editor/` module (119 files) is the largest subsystem in `velocity-mcp`. It implements a complete native IDE using `egui` 0.35 and `eframe`, with docking, code editing, AI chat, browser panel, orchestrator UI, and mode-specialized workflows.

---

## Application Shell & Docking Layout

### VelocityApp (`app/velocity_app/struct_def.rs`)

The root application struct holds all UI state. Key fields:

```rust
pub struct VelocityApp {
    // Agent communication
    pub agent_tx: Sender<UiToAgentMessage>,
    pub agent_rx: Receiver<AgentToUiMessage>,
    
    // Workspace
    pub workspace_root: PathBuf,
    pub tabs: Vec<Tab>,
    pub active_tab: Option<TabId>,
    pub buffers: HashMap<TabId, EditorBuffer>,
    pub dock_state: Option<DockState<Tab>>,
    
    // AI Provider
    pub provider: AiProvider,
    pub selected_model: String,
    pub thinking_enabled: bool,
    pub auto_approve: bool,
    
    // UI State
    pub chat: ChatPanelState,
    pub orchestrator: OrchestratorPanel,
    pub mission_control: MissionControlState,
    pub smart_sidebar: SmartSidebarState,
    pub task_timeline: TTState,
    pub graph_view: MerkleGraphView,
    pub wiki_view: WikiView,
    
    // IDE Features
    pub lsp_manager: Option<LspManager>,
    pub diagnostics: DiagnosticsState,
    pub terminal_state: TerminalState,
    pub dap_client: Option<DapClient>,
    pub git_state: GitState,
    pub completion_state: CompletionState,
    
    // Work Mode
    pub appearance: AppearanceSettings,
    pub mode_layouts: HashMap<WorkspaceProfile, ModeLayout>,
    // ... 60+ more fields
}
```

### Work Modes (WorkspaceProfile)

Velocity supports 4 specialized work modes, each with distinct panel layouts and visual themes:

| Mode | Profile | Focus | Default Panels |
|------|---------|-------|----------------|
| **Coder** | `WorkspaceProfile::Coder` | Code editing | Chat + Output, both sidebars visible |
| **Automation Operator** | `WorkspaceProfile::AutomationOperator` | Browser/WA automation | Orchestrator + Chat, left sidebar only |
| **Mission Control** | `WorkspaceProfile::MissionControl` | Multi-agent supervision | MissionControl + Chat, right sidebar only |
| **Accessibility** | `WorkspaceProfile::Accessibility` | Accessible operation | Chat + Output, both sidebars visible |

Mode switching (`set_work_mode()`) snapshots the current layout, applies the new mode's defaults, restores any user-customized layout for that mode, and persists the choice.

### Docking System

Uses `egui_dock::DockState<Tab>` for tab management. The dock is rebuilt on mode switch via `build_workspace_dock()` which:
1. Collects all open editor tabs
2. Adds mode-specific panel tabs (Chat, Output, Orchestrator, MissionControl)
3. Deduplicates by TabKind
4. Creates a new DockState with the combined tab set

### Session Persistence

`WorkspacePreferences` captures/restores across sessions:
- Appearance settings (theme, profile, palette)
- Provider and model selection
- Sidebar visibility and widths
- Per-mode custom layouts
- Open editor tabs and active tab
- Auto-approve and thinking toggle state

Stored at `.velocity/workspace-preferences.json`.

---

## Code Editor & Buffer Management

### EditorBuffer (`buffer.rs`)

Each open file has an `EditorBuffer` stored in `VelocityApp::buffers` keyed by `TabId`. Features:

- **Syntax highlighting**: Via `syntect` with Rust grammar
- **Auto-indent** (`auto_indent.rs`): Context-aware indentation on newline
- **Bracket matching** (`bracket_match.rs`): Highlights matching `{}`, `()`, `[]`
- **Code folding** (`code_folding.rs`): Collapse functions, impl blocks, modules
- **Minimap** (`minimap.rs`): Scaled-down file overview (toggleable)
- **Breadcrumbs** (`breadcrumbs.rs`): Symbol path navigation above editor
- **Find & Replace** (`find_replace.rs`): In-file search with regex support
- **Go-to-line** (Ctrl+G): Line number jump dialog
- **Go-to-symbol** (Ctrl+Shift+O): SiteMap-backed symbol switcher with fuzzy filter
- **Navigation history** (Alt+←/→): Back/forward cursor position stack
- **Unsaved changes tracking**: Confirmation dialog before closing modified tabs
- **External change detection**: Polls mtime to detect edits outside the IDE

### Code Completion (`completion.rs`)

`CompletionState` manages popup completion UI. Integrates with:
- LSP diagnostics for context
- SiteMap symbol index for workspace-aware suggestions

### Snippets (`snippets.rs`)

`SnippetCollection` loads from `.velocity/snippets.json` and provides template expansion in the editor.

### Inline Suggestions (`inline_suggestions.rs`)

`InlineSuggestionEngine` provides ghost-text suggestions (like Copilot). Currently initialized but marked `#[allow(dead_code)]` — feature is scaffolded but not yet active.

---

## Chat Panel & Streaming

### ChatPanelState (`chat_panel.rs`)

```rust
pub struct ChatPanelState {
    pub messages: Vec<...>,           // Chat history
    pub input: String,                 // Current input text
    pub agent_active: bool,            // Whether agent is currently responding
    pub pending_approvals: Vec<...>,   // Tool approval queue
    pub auto_approve: bool,            // Skip approval dialogs
    pub available_models: Vec<ModelInfo>,
    pub selected_model: String,
    pub thinking_enabled: bool,
    pub thinking_supported: bool,
    pub tools_supported: bool,
    pub models_loading: bool,
    pub show_thoughts: bool,           // Show/hide reasoning tokens
    pub provider: AiProvider,
}
```

### Streaming Behavior

1. Agent sends `AgentToUiMessage::OutputToken(chunk)` for each response fragment
2. UI appends to current assistant message in real-time
3. `ThoughtToken` variants are shown only when `show_thoughts` is enabled
4. `RequestToolApproval` pauses the loop until user approves/rejects
5. `ToolExecutionStarted` / `ToolExecutionFinished` provide visual feedback
6. `AgentFinished` finalizes the response

### Model Selection

Dropdown populated from `available_models` (fetched from provider API). Supports:
- Cloudflare Workers AI catalog (fetched via `fetch_model_catalog()`)
- OpenRouter catalog (fetched via `fetch_openrouter_models()`)
- Azure deployments (fetched via `fetch_azure_models()`)
- Local Ollama models (fetched via `fetch_local_ollama_models()`)

---

## Graph View (Code Explorer)

### MerkleGraphView (`graph_view.rs`)

Interactive visual code explorer backed by the SiteMap triple store:
- Displays symbol dependency graphs
- Shows caller/callee relationships
- Merkle hash verification for integrity
- Drill-down from file to function to call-site

The graph view queries `VelocityApp::cached_site_map` (an `Arc<SiteMap>`) with TTL-based refresh. Symbol relationships are cached per-symbol (`cached_relation_symbol`, `cached_callers`, `cached_deps`).

---

## Browser Panel & Native Bridge

### BrowseState (`browse_panel.rs`)

Web research sidebar for AI-assisted browsing:
- URL input and navigation
- Screenshot display from native browser engine
- AOM tree display
- Integration with browser tools in the registry

### Browser Engine UI (`browser/`)

The `editor/browser/` submodule (20 files) provides the IDE's browser control surface:

```
browser/
├── mod.rs              # Browser panel entry point
├── native_bridge.rs    # Bridge to velocity-browser engine
├── tests.rs            # Browser UI tests
├── engine/
│   ├── auth.rs         # Authentication state management
│   ├── auth_profiles.rs # Auth profile configuration
│   ├── checkpoints.rs  # Browser state checkpoints
│   ├── health.rs       # Browser health monitoring
│   ├── render_reports.rs # Rendering diagnostic reports
│   ├── reports.rs      # General browser reports
│   ├── runtime.rs      # Browser runtime lifecycle
│   ├── sessions.rs     # Session management UI
│   ├── session_actions.rs # Session action dispatch
│   ├── session_reports.rs # Session-specific reports
│   ├── snapshots.rs    # Page snapshot capture
│   ├── snapshot_diff.rs # Snapshot comparison
│   ├── types.rs        # Browser UI types
│   ├── url_helpers.rs  # URL parsing helpers
│   ├── waits.rs        # Wait-for-element logic
│   ├── workflows.rs    # Workflow recording/playback
│   └── workflow_runner.rs # Workflow execution
└── models/
    ├── helpers.rs       # UI model helpers
    ├── session_types.rs # Session UI types
    └── workflow_types.rs # Workflow UI types
```

---

## Orchestrator Panel & Mission Control

### OrchestratorPanel (`orchestrator/`)

Multi-agent task management UI:

```
orchestrator/
├── mod.rs          # Panel entry point
├── types.rs        # RoutedPlanState, OrchestratorDashboardSnapshot
├── tests.rs        # Panel tests
└── panel/
    ├── mod.rs          # Panel module root
    ├── struct_def.rs   # OrchestratorPanel struct, set_routed_tasks()
    ├── execution.rs    # poll_live_workers(), retry_blocked_tasks(), stop_task()
    ├── policy_controls.rs # Worktree policy UI
    └── ui_render.rs    # render_task_card() with expert team assignment display
```

Displays:
- Task DAG visualization
- Worker status and progress
- Worktree lock state
- Policy controls for file scope
- **Expert team assignment** on each task card via `active_team.find_expert_for_task()`

### MissionControlState (`mission_control.rs`)

```rust
pub struct MissionControlState {
    pub brief: Option<String>,
    pub interventions: Vec<Intervention>,
    pub auto_execute: bool,
    pub selected_task_id: Option<TaskId>,
}
```

High-level multi-agent supervision view:
- Real-time agent status monitoring
- Intervention queue (`next_intervention_id` for approval flow)
- Task timeline with NDA-persisted activity log
- Build error count display
- Auto-execute mode for hands-off operation

---

## Team Studio & Expert Team Management

### Team Studio UI (`app/team_studio_ui.rs`)

Full team management interface (402 lines):

**Gallery View**: Expandable team cards showing:
- Team name, description, member count
- Per-member cards: name, role, provider/model, skills, workflow instructions
- Preset vs custom team indicator

**Team Builder Chat**: Natural language team creation:
- Headless sub-agent with `TEAM_BUILDER_SYSTEM_PROMPT`
- Supports `create_expert_team` and `create_skill_file` tools
- Conversational team definition

**Team Activity Log**: Tracks team operations and routing decisions

### TeamManager (`app/team_manager.rs`)

Lightweight UI-facing team manager:
- `launch_team()`: Sends `@slug launch` prompt to agent runtime
- `cancel_running()`: Sends CancelTask message to stop team operations

### Expert Team Persistence

- Teams stored at `.velocity/expert_teams.nda` (encrypted)
- Loaded at startup via `load_expert_teams(&workspace_root)`
- 3 preset teams: C# Software Team, Android App Team, Doccit Maintenance Team
- Custom teams created via Team Studio or `create_expert_team` tool

---

## Smart Sidebar & Task Timeline

### SmartSidebarState (`smart_sidebar.rs`)

Context-aware sidebar with ring-buffer filtered diagnostics:
- File tree with mtime-based change detection
- Symbol inspector (SiteMap-backed)
- Active changes tracking
- Mode-specific filtering (`filter_for_mode()`)
- Collapsible sections (`right_changes_collapsed`, `right_symbol_collapsed`)

### TaskTimelineState (`task_timeline.rs`)

Zero-allocation mission activity event feed:
- Session markers with descriptions
- NDA-persisted activity log (`persist_mission_activity_nda()`)
- Agent status change events
- Worker log display

---

## Additional IDE Features

| Module | Purpose |
|--------|---------|
| `lsp_client.rs` | LSP client manager (`LspManager::auto_detect()`) |
| `diagnostics.rs` | Aggregated LSP diagnostics display |
| `terminal.rs` | Interactive terminal emulator (80x24 default) |
| `debugger.rs` | DAP (Debug Adapter Protocol) client |
| `git_ui.rs` | Git integration (status, diff, commit) |
| `keybindings.rs` | Configurable keybinding system |
| `extensions.rs` | Extension registry (scaffolded) |
| `checkpoint.rs` | Workspace checkpoint/rollback (git-stash) |
| `agent_memory.rs` | Per-member agent knowledge store |
| `live_orchestration.rs` | Live multi-agent activity feed |
| `speculative_precomp.rs` | Pre-computation cache for agents |
| `semantic_search.rs` | TF-IDF semantic search index |
| `deploy_pipeline.rs` | Build/test/deploy pipeline manager |
| `voice_commands.rs` | Voice-to-task input (scaffolded) |
| `test_generator.rs` | Auto-generated test coverage analyzer |
| `search.rs` | Workspace-wide search with `SearchHit` results |
| `wiki_view.rs` | Embedded Markdown wiki browser |
| `theme.rs` | Appearance settings, palette, workspace profiles |
| `toast.rs` | Non-intrusive notification toasts |
| `status_bar.rs` | Bottom status bar rendering |
| `usage_panel.rs` | AI provider usage statistics |
| `team_builder_chat.rs` | Team creation sub-chat UI |
| `skill_file.rs` | Skill file packaging UI |
| `bottom_panel.rs` | Bottom panel tab management |
| `sidebar_tabs.rs` | Sidebar tab organization, bookmarks |
| `toolbar_actions.rs` | Top toolbar action buttons |
| `mode_config.rs` | Per-mode configuration |
