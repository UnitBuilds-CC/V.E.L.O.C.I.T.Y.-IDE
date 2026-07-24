# MCP Tool Registry & Execution Subsystem

The `velocity-mcp` crate hosts a Model Context Protocol (MCP) server that exposes native tools to AI agents. The registry (`velocity-mcp/src/registry/`) handles tool registration, schema generation, argument validation, and invocation dispatch.

---

## 🛠️ Tool Category Inventory

### 1. System & Developer Tools (`system_tools.rs`)
- **`read_file`**: Read file contents with support for partial line slicing and character offset byte limits.
- **`write_to_file`**: Create new files or overwrite existing files safely.
- **`replace_file_content`**: Perform single contiguous block string replacements.
- **`multi_replace_file_content`**: Execute non-contiguous multi-chunk edits in a single atomic pass.
- **`run_command`**: Execute terminal shell commands (`powershell`, `bash`) with background task support.
- **`list_dir`**: List directory contents recursively with byte sizes and child counts.
- **`grep_search`**: Ripgrep-powered pattern search within files and directories.

### 2. Agentic Browser Tools (`browser_tools/`)
- **`browser_navigate`**: Navigate to a specified URL.
- **`browser_click`**: Click an element using element ID or selector.
- **`browser_type`**: Input text into form controls or editable areas.
- **`browser_get_aom`**: Extract the compact Accessible Object Model (AOM) tree.
- **`browser_take_screenshot`**: Capture full page or node screenshots.
- **`browser_workflow_record` / `browser_workflow_play`**: Record and playback browser interaction workflows.

### 3. Team & Agent Orchestration Tools (`team_tools.rs`)
- **`create_expert_team`**: Define a specialized subagent team with designated roles, system prompts, and tool access permissions.
- **`list_expert_teams`**: Query active agent teams and workspace router configuration.
- **`create_skill_file`**: Package a completed workflow into a reusable skill file (`.skill.md`).

### 4. Windows Automation Tools (`wa_tools.rs`)
- **`wa_click`**: Synthesize Windows UI element click.
- **`wa_type`**: Send text keystrokes to active Windows window.
- **`wa_capture`**: Capture UI automation window hierarchy and screenshot.
- **`wa_run_script`**: Execute scripted desktop macro sequences.

---

## ⚡ Dispatch Engine (`dispatch.rs`)

1. **Tool Identification**: Matches incoming tool call names against registered tool definitions (`tool_definitions/`).
2. **Schema & Argument Validation**: Validates JSON payloads against expected arguments.
3. **Execution**: Dispatches tool invocations asynchronously to dedicated thread pools or process handlers.
4. **Result Packaging**: Formats response payloads, error strings, and NDA artifacts back to the calling agent.
