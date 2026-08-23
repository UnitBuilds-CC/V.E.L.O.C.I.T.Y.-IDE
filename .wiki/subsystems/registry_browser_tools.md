# Registry Browser Tools & Native Bindings

_Browser-specific MCP tools: navigation, session management, browser workflow automation, and native browser engine bindings._

---

## Overview

The `velocity-mcp/src/registry/` module (22 files) manages the full MCP tool dispatch pipeline. The [MCP Tool Registry](mcp_tool_registry.md) article covers system tools and WA tools. This article covers the **browser-specific tools** and their native engine bindings.

---

## Module Structure

```
registry/
├── mod.rs              # call_tool dispatch entry, get_tools
├── dispatch.rs         # call_tool_in_workspace: workspace-scoped dispatch
├── parsers.rs          # Argument parsing helpers
├── system_tools.rs     # System tools (file read/write, search, etc.)
├── team_tools.rs       # Team routing tools
├── wa_tools.rs         # Windows Automation tools
├── types.rs            # Tool definition types
├── browser_tools/
│   ├── mod.rs          # handle_browser_tool: dispatch to sub-categories
│   ├── navigation.rs   # navigate, go_back, go_forward, get_url, get_title
│   ├── session.rs      # Session management: cookies, storage, auth
│   ├── workflow.rs      # Browser workflow: record, replay, step
│   └── native/         # Native browser engine bindings (7 files)
│       ├── mod.rs      # handle_native_tool dispatch
│       ├── dom.rs      # DOM queries: query_selector, get_inner_html
│       ├── click.rs    # Click simulation with event dispatch
│       ├── type_text.rs # Text input simulation
│       ├── screenshot.rs # Page screenshot capture
│       ├── scroll.rs   # Scroll operations
│       └── evaluate.rs # JS evaluation in page context
├── tool_definitions/   # Tool schema definitions (5 files)
│   ├── mod.rs          # get_tools: aggregate all tool schemas
│   ├── system.rs       # System tool schemas
│   ├── browser.rs      # Browser tool schemas
│   ├── wa.rs           # WA tool schemas
│   └── team.rs         # Team tool schemas
└── tests/              # Registry tests (5 files)
```

---

## Browser Tool Dispatch

```rust
pub fn handle_browser_tool(
    root: &Path, name: &str, arguments: &Value,
) -> Result<Option<String>, Box<dyn Error>> {
    // Try each sub-category in order
    if let Some(res) = navigation::handle_navigation_tool(root, name, arguments)? { return Ok(Some(res)); }
    if let Some(res) = native::handle_native_tool(root, name, arguments)? { return Ok(Some(res)); }
    if let Some(res) = session::handle_session_tool(root, name, arguments)? { return Ok(Some(res)); }
    if let Some(res) = workflow::handle_workflow_tool(root, name, arguments)? { return Ok(Some(res)); }
    Ok(None)
}
```

### Navigation Tools

- `browser_navigate` — Navigate to URL
- `browser_go_back` / `browser_go_forward` — History navigation
- `browser_get_url` — Current page URL
- `browser_get_title` — Page title

### Session Tools

- `browser_get_cookies` — List session cookies
- `browser_set_cookie` — Set a cookie
- `browser_clear_storage` — Clear localStorage/sessionStorage
- `browser_get_auth_state` — Extract bearer token + auth state

### Workflow Tools

- `browser_workflow_record` — Start recording a browser workflow
- `browser_workflow_replay` — Replay a recorded workflow
- `browser_workflow_step` — Execute one step of a workflow

---

## Native Browser Engine Bindings

The `native/` sub-module provides direct access to the velocity-browser engine:

### DOM Operations (`native/dom.rs`)

- `browser_query_selector` — CSS selector query → element list
- `browser_get_inner_html` — Element innerHTML
- `browser_get_attribute` — Element attribute value
- `browser_get_text_content` — Element text content

### Interaction (`native/click.rs`, `native/type_text.rs`)

- `browser_click` — Click element at CSS selector with full event dispatch (mousedown → mouseup → click)
- `browser_type_text` — Type text into focused input element
- `browser_focus` — Focus an element
- `browser_blur` — Blur an element

### Screenshot (`native/screenshot.rs`)

- `browser_screenshot` — Capture page as PNG/base64
- Uses the browser engine's screencast module for rendering

### Scroll (`native/scroll.rs`)

- `browser_scroll_to` — Scroll to element
- `browser_scroll_by` — Scroll by pixel offset

### JS Evaluation (`native/evaluate.rs`)

- `browser_evaluate` — Execute JavaScript in the page context
- Returns the result as a JSON value
- Uses the velocity-browser JS interpreter

---

## Tool Definitions

Each tool category has a schema definition file that produces the JSON schema for MCP `tools/list`:

```rust
pub fn get_tools() -> Vec<ToolDefinition> {
    let mut tools = Vec::new();
    tools.extend(tool_definitions::system::system_tools());
    tools.extend(tool_definitions::browser::browser_tools());
    tools.extend(tool_definitions::wa::wa_tools());
    tools.extend(tool_definitions::team::team_tools());
    tools
}
```

---

## Key Design Decisions

- **Cascading dispatch**: Browser tools try each sub-category in order — first match wins
- **Native engine access**: Browser tools operate directly on the velocity-browser engine, not through a remote debugging protocol
- **Schema-driven**: Tool definitions are centralized — `get_tools()` returns the complete list
- **Workspace-scoped dispatch**: `call_tool_in_workspace` ensures tools operate within the correct workspace root

---

## See Also

- [MCP Tool Registry](mcp_tool_registry.md) — System tools, WA tools, dispatch flow
- [Browser Engine & Networking](../architecture/velocity_browser.md) — Engine capabilities
- [JS Interpreter & Runtime](js_interpreter_runtime.md) — JS evaluation target
- [IPC, Protocol & Telemetry](ipc_protocol_telemetry.md) — Protocol transport for tool calls
