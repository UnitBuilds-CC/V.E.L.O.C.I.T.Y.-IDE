# velocity-mcp: Tool Registry & Windows Automation

The `registry/` module implements the Model Context Protocol (MCP) tool system — tool definitions, argument validation, and dispatch. The `wa/` module (29 files) provides native Windows desktop automation via UIA FFI.

---

## Tool Registry Architecture

### Module Structure

```
registry/
├── mod.rs              # Public API: call_tool(), get_tools()
├── dispatch.rs         # Tool invocation dispatch (call_tool_in_workspace)
├── parsers.rs          # Argument parsing helpers
├── types.rs            # Tool definition types
├── tool_definitions/   # Tool schema definitions (JSON Schema)
├── system_tools.rs     # File, search, terminal tools
├── browser_tools/      # Web navigation, click, type, screenshot
├── team_tools.rs       # Team creation, expert management
├── wa_tools.rs         # Windows automation tools
└── tests/              # Per-category test suites
    ├── system_tests.rs
    ├── browser_tests.rs
    ├── team_tests.rs
    └── wa_tests.rs
```

### Public API

```rust
// registry/mod.rs
pub fn call_tool(name: &str, arguments: &Value) -> Result<String, Box<dyn Error>>;
pub use dispatch::call_tool_in_workspace;
pub use tool_definitions::get_tools;
```

`call_tool()` resolves the current working directory and delegates to `call_tool_in_workspace()`, which:
1. Matches the tool name against registered definitions
2. Validates JSON arguments against the tool's schema
3. Dispatches to the appropriate handler function
4. Returns the result as a JSON string

### Tool Definition Schema

Tools are defined with JSON Schema-compatible argument specifications via `get_tools()`. Each tool has:
- **name**: Unique identifier (e.g., `read_file`, `browser_navigate`)
- **description**: Human-readable purpose
- **parameters**: JSON Schema object defining expected arguments
- **handler**: Function pointer or dispatch match

---

## System Tools

Located in `system_tools.rs`:

| Tool | Description |
|------|-------------|
| `read_file` | Read file contents with partial line slicing and byte limits |
| `write_to_file` | Create or overwrite files safely |
| `replace_file_content` | Single contiguous block string replacement |
| `multi_replace_file_content` | Non-contiguous multi-chunk atomic edits |
| `run_command` | Execute shell commands with background task support |
| `list_dir` | Recursive directory listing with sizes and child counts |
| `grep_search` | Ripgrep-powered pattern search across files |

These tools operate with workspace path sandboxing — all file operations are constrained to the workspace root.

---

## Browser Tools

Located in `browser_tools/`:

| Tool | Description |
|------|-------------|
| `browser_navigate` | Navigate to URL in the native browser engine |
| `browser_click` | Click element by ID or CSS selector |
| `browser_type` | Input text into form controls |
| `browser_get_aom` | Extract compact Accessible Object Model tree |
| `browser_take_screenshot` | Capture full page or node screenshots |
| `browser_workflow_record` | Record browser interaction sequence |
| `browser_workflow_play` | Replay recorded workflow |

These tools interface directly with `velocity-browser`'s session and AOM subsystems.

---

## Team Tools

Located in `team_tools.rs`:

| Tool | Description |
|------|-------------|
| `create_expert_team` | Define specialized agent team with roles and prompts |
| `list_expert_teams` | Query active teams and router configuration |
| `create_skill_file` | Package workflow into reusable `.skill.md` file |

---

## Windows Automation Platform

The `wa/` module provides native Windows desktop automation without external dependencies. It interfaces directly with Windows UI Automation (UIA) APIs via FFI.

### Module Structure

```
wa/
├── mod.rs              # Module root, public API
├── platform.rs         # UIA FFI initialization and tree inspection
├── uia_ffi.rs          # Raw COM FFI bindings for UIA
├── execution.rs        # Mouse/keyboard synthesis (no hardware locks)
├── runtime.rs          # Action parsing, retry, timeout, verification
├── storage.rs          # NDA snapshot persistence
├── selector.rs         # Element selection strategies
├── screenshot.rs       # Screen capture
├── ocr.rs              # On-screen text recognition
├── clipboard.rs        # Clipboard operations
├── advanced_input.rs   # Complex input sequences
├── browser_bridge.rs   # Browser↔WA integration bridge
├── events.rs           # UIA event subscriptions
├── file_dialog.rs      # Native file dialog handling
├── multi_monitor.rs    # Multi-display support
├── notifications.rs    # Windows toast notifications
├── process_mgmt.rs     # Process lifecycle management
├── recording.rs        # Action recording and playback
├── recovery.rs         # Error recovery strategies
├── registry.rs         # WA tool registration
├── triggers.rs         # Event-triggered automation
├── virtual_desktop.rs  # Virtual desktop management
├── window_mgmt.rs      # Window control (move, resize, focus)
├── model.rs            # WA data models
├── nda.rs              # NDA serialization for WA artifacts
├── payloads.rs         # Request/response payload types
├── reports.rs          # Execution reports
└── scripts.rs          # Scripted desktop macro sequences
```

### WA Tools

Exposed via the MCP registry (`wa_tools.rs`):

| Tool | Description |
|------|-------------|
| `wa_click` | Synthesize mouse click on UI element |
| `wa_type` | Send keystrokes to active window |
| `wa_capture` | Capture UI automation tree hierarchy |
| `wa_run_script` | Execute scripted desktop macro sequence |

### Platform Architecture

```
wa/
├── platform (WinUI Capture & Tree Inspect)
│   ├── UIA FFI via uia_ffi.rs
│   ├── Element tree construction
│   └── Bounding rect, AutomationID, accessible text extraction
├── runtime (Action Exec & Script Runner)
│   ├── Action request parsing from MCP tools
│   ├── Retry conditions and timeout management
│   └── Step verification and safe halt on failure
└── storage (Snapshot & NDA Persistence)
    ├── Window state capture
    ├── Action execution logs
    └── NDA binary snapshot persistence
```

### Execution Model

The `execution.rs` module synthesizes input without physical hardware locks:
- Mouse clicks (single, double, right-click)
- Keyboard input (individual keys and text strings)
- Window drag operations
- All operations work even when the window is not in the foreground

### Recovery & Verification

`recovery.rs` provides error recovery strategies:
- Retry on transient UIA failures
- Fallback element selection strategies
- Safe halt on verification failures (prevents cascading errors)

---

## WA Tool Execution & Verification

### Dispatch Flow

```
1. Agent calls wa_click/wa_type/wa_capture/wa_run_script
       │
       ▼
2. registry::call_tool_in_workspace() matches wa_tools handler
       │
       ▼
3. wa/runtime.rs parses action request
       │
       ▼
4. wa/platform.rs captures current UI tree via UIA FFI
       │
       ▼
5. wa/selector.rs finds target element
       │
       ▼
6. wa/execution.rs synthesizes input
       │
       ▼
7. wa/runtime.rs verifies step success
       │
       ▼
8. Result packaged as JSON → AgentToUiMessage::ToolExecutionFinished
```

### NDA Artifact Persistence

All WA operations produce NDA artifacts:
- `.velocity/wa_snapshots/` — window state captures
- Execution logs with element trees and action results
- Enables post-run auditing and playback verification
