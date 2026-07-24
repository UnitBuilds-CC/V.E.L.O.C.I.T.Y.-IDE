# Velocity MCP & IDE Application Architecture

The `velocity-mcp` crate serves as the core integration hub for the Velocity environment. It contains the Model Context Protocol (MCP) server, local GPU LLM execution engine, multi-agent orchestrator, egui GUI application, and Windows Automation platform.

---

## 🏛️ Module Overview

```
                          ┌────────────────────────┐
                          │      velocity-mcp      │
                          └───────────┬────────────┘
                                      │
         ┌───────────────┬────────────┼────────────┬───────────────┐
         │               │            │            │               │
         ▼               ▼            ▼            ▼               ▼
   ┌──────────┐    ┌───────────┐ ┌──────────┐ ┌──────────┐    ┌──────────┐
   │  editor  │    │  agent    │ │ registry │ │compiler  │    │    wa    │
   │(Egui GUI)│    │(Executor) │ │(MCP Tools│ │(Vulkan   │    │ (Windows │
   └──────────┘    └───────────┘ └──────────┘ │ Kernels) │    │Automate) │
                                              └──────────┘    └──────────┘
```

---

## 🔧 Core Modules

### 1. Editor UI (`src/editor/`)
Built with `egui` and `eframe` to deliver a zero-latency, cross-platform IDE interface.
- **`app/render.rs` & `app/velocity_app/`**: Main window shell, top menu bar, status bar, and tab manager.
- **`chat_panel.rs`**: Interactive AI chat window supporting stream rendering, tool call visualization, and user inputs.
- **`code_editor.rs` & `buffer.rs`**: Full-featured code editor with syntax highlighting, auto-indent (`auto_indent.rs`), bracket matching (`bracket_match.rs`), code folding (`code_folding.rs`), and line numbers.
- **`graph_view.rs`**: Interactive visual symbol dependency graph renderer.
- **`smart_sidebar.rs`**: Contextual sidebar showing files, symbols, agent active tasks, and project outline.
- **`task_timeline.rs`**: Mission activity log displaying agent progress and NDA artifacts.
- **`wiki_view.rs`**: Embedded Markdown wiki documentation browser (`.wiki/`).
- **`lsp_client.rs`, `debugger.rs`, `git_ui.rs`, `terminal.rs`**: Integrated developer tooling.

### 2. Local GPU LLM Kernels (`src/compiler/driver/`)
- **`vulkan_init.rs`**: Vulkan graphics API initialization for GPU compute shaders.
- **`bitnet_layer.rs` & `nda_bitnet_layer.rs`**: Ultra-fast 1.58-bit quantized matrix multiplication kernels.
- **`qwen_layer.rs` & `gemv.rs`**: Matrix-vector multiply kernels optimized for local Qwen model execution.

### 3. Agent Execution Engine (`src/agent/`)
- **`executor/loop_runner.rs`**: Agent reasoning and tool invocation loop.
- **`executor/thread.rs`**: Worker thread pool for asynchronous subagent execution.
- **`executor/team_routing.rs` & `team_router.rs`**: Natural language and `@team` directive router.
- **`nda.rs`**: Binary state encoder for context conservation.

### 4. MCP Tool Registry (`src/registry/`)
- Implements MCP tool registration and message routing.
- **`system_tools.rs`**: File editing, workspace search, terminal execution tools.
- **`browser_tools/`**: Web navigation, click, type, screenshot, and AOM snapshot tools.
- **`team_tools.rs`**: Team creation, expert team management tools.

### 5. Windows Automation Engine (`src/wa/`)
- Native Windows accessibility API automation layer.
- Enables autonomous desktop screen interaction, window control, and input synthesis.
