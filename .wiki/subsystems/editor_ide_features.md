# Editor IDE Feature Modules

_Deep dive into the 67+ editor feature modules: workflow automation, plugin system, LSP client, code intelligence, voice commands, and multimodal interaction._

---

## Overview

The `velocity-mcp/src/editor/` directory contains 119 modules total (67 feature modules + app/browser/orchestrator UI structure). The [Editor & IDE UI](../architecture/editor_ide_ui.md) article covers the app shell, docking, and panel layout. This article covers the **feature modules** in depth — the code intelligence, workflow automation, plugin infrastructure, and interaction modes that make Velocity a full IDE.

---

## Feature Module Map

### Workflow Automation (5 modules)

| Module | Lines | Purpose |
|--------|-------|---------|
| `workflow.rs` | 463 | Sequential + branching multi-step automation |
| `workflow_ai.rs` | — | AI-assisted workflow generation |
| `workflow_canvas.rs` | — | Visual workflow editor UI |
| `workflow_templates.rs` | — | Pre-built workflow templates |
| `workflow_version.rs` | — | Workflow versioning and migration |

**Workflow** is an ordered list of `WorkflowStep`s:
```rust
pub enum WorkflowStep {
    AgentTask { prompt: String, team: Option<String> },
    Tool { name: String, args: serde_json::Value },
    Connector { id: String, req: serde_json::Value },
    Condition { require: StepOutcome },
}
```

Workflows persist as JSON files under `.velocity/workflows/`. Each step produces a `StepOutcome`, and `Condition` steps inspect the previous outcome to short-circuit execution. The whole run produces a `WorkflowRun` for the governance audit log.

### Plugin System (2 modules)

| Module | Lines | Purpose |
|--------|-------|---------|
| `plugin_registry.rs` | 401 | Discovery, loading, lifecycle management |
| `plugin_sdk.rs` | — | Plugin manifest, permissions, handler trait |

```rust
pub struct PluginRegistry {
    plugins: HashMap<String, Box<dyn PluginHandler>>,
    load_order: Vec<String>,
    workspace_root: PathBuf,
    granted_permissions: HashMap<String, Vec<PluginPermission>>,
}
```

Plugins declare tools via `PluginManifest`, get dispatched through the registry, and operate under user-granted permissions. The SDK provides `PluginHandler`, `PluginResult`, and `PluginPermission` types.

### Code Intelligence (7 modules)

| Module | Lines | Purpose |
|--------|-------|---------|
| `lsp_client.rs` | 1455 | Full LSP client: go-to-def, hover, references, rename, diagnostics |
| `completion.rs` | — | Code completion with LSP + local fallback |
| `diagnostics.rs` | — | Error/warning display from LSP + compiler |
| `semantic_search.rs` | — | Symbol-based code search |
| `inline_suggestions.rs` | — | Ghost-text inline completions |
| `nda_document.rs` | — | NDA-format document model |
| `nda_viewer.rs` | — | Visual NDA document inspector |

**LSP Client** manages language server processes via JSON-RPC over stdin/stdout:
```rust
pub struct LspServerConfig {
    pub language_id: String,
    pub command: String,
    pub args: Vec<String>,
    pub root_uri: Option<String>,
    pub extensions: Vec<String>,
}
```

Includes `LspServerConfig::rust_analyzer()` factory for the Rust workspace.

### Voice & Multimodal (2 modules)

| Module | Lines | Purpose |
|--------|-------|---------|
| `voice_commands.rs` | 528 | Speech-to-task: intent parsing, command registry |
| `multimodal.rs` | — | Image/screenshot input for agent context |

**Voice Commands** parse speech into structured intent:
```rust
pub enum VoiceIntent {
    OpenFile, Search, RunTests, Build, Deploy,
    FixError, Refactor, Navigate, Explain,
}

pub struct VoiceCommand {
    pub raw_text: String,
    pub intent: VoiceIntent,
    pub parameters: HashMap<String, String>,
    pub confidence: f32,
    pub timestamp: Instant,
}
```

Windows Speech API integration via continuous recognition in a background process, feeding transcriptions into the command parser.

### Editor Core (15+ modules)

| Module | Purpose |
|--------|---------|
| `code_editor.rs` | Core text editor with syntax highlighting |
| `code_folding.rs` | Fold regions by brace/syntax level |
| `auto_indent.rs` | Automatic indentation on newline |
| `bracket_match.rs` | Matching bracket highlighting |
| `breadcrumbs.rs` | Symbol path breadcrumb bar |
| `find_replace.rs` | Find & replace with regex support |
| `minimap.rs` | Code minimap sidebar |
| `keybindings.rs` | Configurable keybinding system |
| `snippets.rs` | Code snippet expansion |
| `regex_engine.rs` | Regex engine for search |
| `browse_panel.rs` | File browser panel |
| `terminal.rs` | Integrated terminal emulator |
| `git_ui.rs` | Git status, diff, commit UI |
| `debugger.rs` | Debugger integration |
| `extensions.rs` | Extension point system |

### Agent UI (4 modules)

| Module | Purpose |
|--------|---------|
| `agent_ui_render.rs` | Zero-allocation agentic UI components |
| `agent_ui_state.rs` | Agent UI state management |
| `task_timeline.rs` | Visual task execution timeline |
| `smart_sidebar.rs` | Context-aware sidebar |

### Governance & Deploy (3 modules)

| Module | Purpose |
|--------|---------|
| `governance.rs` | Audit log and compliance tracking |
| `deploy_pipeline.rs` | Build → test → deploy automation |
| `test_generator.rs` | AI-assisted test generation |

### Other Features

| Module | Purpose |
|--------|---------|
| `checkpoint.rs` | File checkpoint/rollback |
| `continuation_ledger.rs` | Agent continuation tracking |
| `file_watcher.rs` | Filesystem change detection |
| `history.rs` | Edit history |
| `knowledge_base.rs` | In-IDE knowledge base viewer |
| `live_orchestration.rs` | Real-time orchestration display |
| `peer_panel.rs` | Cross-device peer status panel |
| `speculative_precomp.rs` | Speculative precomputation |
| `triggers.rs` | Event-driven automation triggers |

---

## Key Design Decisions

- **Modular features**: Each IDE capability is an independent module — can be enabled/disabled
- **LSP-first code intelligence**: Full language server protocol support, not just syntax highlighting
- **Workflow as data**: Workflows are serializable JSON — shareable, versionable, templateable
- **Plugin permissions**: User-granted per-plugin — no implicit capabilities
- **Voice-to-task**: Natural language → structured IDE actions, not just text insertion
- **Governance audit**: Every workflow run produces an auditable `WorkflowRun` record

---

## See Also

- [Editor & IDE UI](../architecture/editor_ide_ui.md) — App shell, docking, panel layout
- [Multi-Agent Task Orchestrator](multi_agent_orchestrator.md) — Orchestration panel and expert teams
- [MCP Tool Registry](mcp_tool_registry.md) — Tool dispatch used by workflow steps
