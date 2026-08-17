# Editor IDE UI Architecture

<cite>
**Referenced Files in This Document**
- [velocity-mcp/src/editor/app/velocity_app/mod.rs](file://velocity-mcp/src/editor/app/velocity_app/mod.rs)
- [velocity-mcp/src/editor/app/velocity_app/struct_def.rs](file://velocity-mcp/src/editor/app/velocity_app/struct_def.rs)
- [velocity-mcp/src/editor/code_editor.rs](file://velocity-mcp/src/editor/code_editor.rs)
- [velocity-mcp/src/editor/chat_panel.rs](file://velocity-mcp/src/editor/chat_panel.rs)
- [velocity-mcp/src/editor/browse_panel.rs](file://velocity-mcp/src/editor/browse_panel.rs)
- [velocity-mcp/src/editor/app/velocity_app/tier3_panels.rs](file://velocity-mcp/src/editor/app/velocity_app/tier3_panels.rs)
- [velocity-mcp/src/editor/bottom_panel.rs](file://velocity-mcp/src/editor/bottom_panel.rs)
- [velocity-mcp/src/editor/status_bar.rs](file://velocity-mcp/src/editor/status_bar.rs)
</cite>

## Overview

The Velocity IDE is built on `egui 0.35`, a native immediate-mode GUI framework. The editor layer (`velocity-mcp/src/editor/`) contains 119 files organized into feature modules, all rendered with hardware-accelerated graphics and a dark HSL color palette.

## VelocityApp Structure

The central `VelocityApp` struct (`struct_def.rs`) owns all IDE state:
- Work mode (code, browser, orchestrator, review)
- Panel visibility and docking with `mode_layouts` cache to reduce dock rebuild jitter
- `use_unified_header: bool` — when true, suppresses legacy toolbar top panel
- Toast notification queue (`toasts`) for transient user feedback — build/run actions push info toasts ("Build started...", "Execute started...")
- Active file buffers and cursor state
- Agent session and chat history
- Browser session state
- Orchestrator task state
- Team Studio draft fields: `team_name_input`, `team_description_input`, `team_agent_name_input`, `team_agent_role_input`, `team_agent_scope_input`, `team_agent_instructions_input`, `team_agent_target_index`
- `team_manager: TeamManager` — bridges Team Studio controls to agent runtime

### Workspace Preset Application

`apply_workspace_preset()` uses layout caching to avoid visual jitter:
- Stores per-profile layouts in `mode_layouts` map
- Only rebuilds dock when sidebar visibility or tab kinds actually change
- Constrains sidebar widths per profile (Coder/AutomationOperator: 200-420px left, 240-420px right)
- Only changes focus panel when dock was rebuilt or focused kind is missing

## Panel Architecture

### Primary Panels

| Panel | File | Purpose |
|-------|------|---------|
| Code Editor | `code_editor.rs` | Syntax-highlighted file editing with LSP-like features |
| Chat Panel | `chat_panel.rs` | Agent conversation and tool call display |
| Browse Panel | `browse_panel.rs` | Integrated browser engine view |
| Orchestrator Panel | `orchestrator/panel/` | Task timeline, execution status, policy controls |
| Smart Sidebar | `smart_sidebar.rs` | Context-aware file/symbol inspector |
| Bottom Panel | `bottom_panel.rs` | Terminal output, diagnostics, search results; tab indices exposed as named constants (`TAB_TERMINAL`, `TAB_PROBLEMS`, `TAB_DEBUG`, `TAB_OUTPUT`, `TAB_CHECKPOINTS`) with `MAX_PANEL_HEIGHT` (600px) clamp; tab scroll areas use adaptive `ui.available_height()` instead of fixed 180px |

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

### Chat Panel UX

The chat panel (`chat_panel.rs`) provides responsive composer layout for narrow dock splits:
- Input width adapts to available space via `(available_width - 78).max(0.0)` clamping
- Fixed-width provider (118px) and model (156px) selectors prevent composer resizing
- "Send" text button (64×30px) with hover text replaces icon-only button
- "Auto-approve" checkbox with hover explanation
- Provider/model row uses `horizontal_wrapped` to prevent overflow

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

Workspace profiles (`WorkspaceProfile` enum in `theme.rs`) with short labels:

| Profile | Short Label | Default Panels |
|---------|-------------|----------------|
| `Coder` | "Build" | File tree + code editor + chat |
| `AutomationOperator` | "Automate" | Orchestrator + chat + output |
| `MissionControl` | "Mission" | Mission control + chat + output |
| `Accessibility` | "Access" | Everything visible |

## Mission Control UI

The Mission Control panel (rendered in `app/render.rs`) uses compact labels and dynamic status indicators:
- **Heading**: "Mission" with subtitle "Define the outcome, approve a plan, then steer the work."
- **Tab labels**: "1  Plan" (or "1  Plan ready" when routed), "2  Work" (or "2  Work — N active" when tasks running), "3  Review"
- **Responsive layout**: Uses `horizontal_wrapped` for heading and tab row to prevent overflow in narrow panels

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+E` | Toggle left sidebar |
| `Ctrl+Shift+E` | Toggle right sidebar |
| `Ctrl+J` | Focus chat panel |
| `Ctrl+\`` | Focus output panel |
| `Ctrl+PageDown` / `Ctrl+PageUp` | Cycle tabs |

## Status Bar

The status bar (`status_bar.rs`) renders interactive elements with hover tooltips:
- Mode indicator → "Switch workspace mode"
- Build status dot → "View diagnostics"
- Line/column position → "Go to line"
- Provider label → "Open settings"

## Team Studio Direct Creation

`team_studio_ui.rs` provides a two-column direct creation layout:
- **Create a team**: Name + purpose fields, creates empty `ExpertTeam`, auto-expands gallery
- **Create an agent**: Name, role, scope (comma-separated paths), instructions + team assignment combo box. Creates `ExpertMember` and assigns to selected team
- Both flows use draft fields on `VelocityApp` and persist via `save_expert_teams()`
- Toast notifications for success, duplicate names, and save failures

**Section sources**
- [velocity-mcp/src/editor/app/velocity_app/struct_def.rs](file://velocity-mcp/src/editor/app/velocity_app/struct_def.rs)
- [velocity-mcp/Cargo.toml](file://velocity-mcp/Cargo.toml)
