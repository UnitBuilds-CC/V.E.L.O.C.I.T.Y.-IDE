# Editor IDE UI Architecture

<cite>
**Referenced Files in This Document**
- [velocity-mcp/src/editor/app/velocity_app/mod.rs](file://velocity-mcp/src/editor/app/velocity_app/mod.rs)
- [velocity-mcp/src/editor/app/velocity_app/struct_def.rs](file://velocity-mcp/src/editor/app/velocity_app/struct_def.rs)
- [velocity-mcp/src/editor/code_editor.rs](file://velocity-mcp/src/editor/code_editor.rs)
- [velocity-mcp/src/editor/chat_panel.rs](file://velocity-mcp/src/editor/chat_panel.rs)
- [velocity-mcp/src/editor/browse_panel.rs](file://velocity-mcp/src/editor/browse_panel.rs)
- [velocity-mcp/src/editor/app/velocity_app/tier3_panels.rs](file://velocity-mcp/src/editor/app/velocity_app/tier3_panels.rs)
</cite>

## Overview

The Velocity IDE is built on `egui 0.35`, a native immediate-mode GUI framework. The editor layer (`velocity-mcp/src/editor/`) contains 98 files organized into feature modules, all rendered with hardware-accelerated graphics and a dark HSL color palette.

## VelocityApp Structure

The central `VelocityApp` struct (`struct_def.rs`) owns all IDE state:
- Work mode (code, browser, orchestrator, review)
- Panel visibility and docking
- Active file buffers and cursor state
- Agent session and chat history
- Browser session state
- Orchestrator task state

## Panel Architecture

### Primary Panels

| Panel | File | Purpose |
|-------|------|---------|
| Code Editor | `code_editor.rs` | Syntax-highlighted file editing with LSP-like features |
| Chat Panel | `chat_panel.rs` | Agent conversation and tool call display |
| Browse Panel | `browse_panel.rs` | Integrated browser engine view |
| Orchestrator Panel | `orchestrator/panel/` | Task timeline, execution status, policy controls |
| Smart Sidebar | `smart_sidebar.rs` | Context-aware file/symbol inspector |
| Bottom Panel | `bottom_panel.rs` | Terminal output, diagnostics, search results |

### Code Editor Features

The code editor (`code_editor.rs`) includes:
- **Auto-indent** (`auto_indent.rs`): Context-aware indentation
- **Bracket matching** (`bracket_match.rs`): Highlight matching pairs
- **Code folding** (`code_folding.rs`): Collapse/expand code blocks
- **Completion** (`completion.rs`): Auto-completion suggestions
- **Breadcrumbs** (`breadcrumbs.rs`): Navigation breadcrumb trail
- **Buffer management** (`buffer.rs`): Multi-file buffer handling
- **Syntax highlighting**: Language-aware rendering
- **Agent memory** (`agent_memory.rs`): Inline agent context display
- **Agent UI** (`agent_ui_render.rs`, `agent_ui_state.rs`): Agent status visualization

### App Submodules

| Module | Files | Responsibility |
|--------|-------|----------------|
| `app/velocity_app/` | 10 | Core app struct, actions, agent handlers, editor actions, overlays, tier3 panels, UI render, workflows |
| `app/` | 5 | App state types, tests, desktop automation bridge |
| `browser/` | 20+ | Browser engine bridge, native bridge, auth, sessions, snapshots, workflows |
| `orchestrator/` | 7 | Task orchestration panel, execution, policy, types |

### Browser Integration

The editor embeds the browser engine via `editor/browser/`:
- **Native Bridge** (`browser/native_bridge.rs`): Connect egui rendering to browser engine
- **Engine modules**: Auth, checkpoints, health, sessions, snapshots, workflows
- **Models**: Session types, workflow types

### Supporting Modules

| Module | Purpose |
|--------|---------|
| `checkpoint.rs` | Save/restore editor state |
| `file_tree.rs` | Workspace file browser |
| `graph_view.rs` | Dependency/symbol graph visualization |
| `history_inspector.rs` | Chronological change history |
| `search_panel.rs` | Workspace-wide search |
| `settings.rs` | IDE preferences |
| `task_timeline.rs` | Agent task timeline |
| `terminal.rs` | Integrated terminal |
| `theme.rs` | Dark HSL color palette |

## Work Modes

1. **Code Mode**: File tree + code editor + chat sidebar
2. **Browser Mode**: Browser panel + agent controls + session list
3. **Orchestrator Mode**: Task timeline + execution panel + agent status
4. **Review Mode**: Diff view + agent commentary + approval controls

**Section sources**
- [velocity-mcp/src/editor/app/velocity_app/struct_def.rs](file://velocity-mcp/src/editor/app/velocity_app/struct_def.rs)
- [velocity-mcp/Cargo.toml](file://velocity-mcp/Cargo.toml)
