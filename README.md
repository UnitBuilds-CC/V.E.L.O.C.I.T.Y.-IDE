# V.E.L.O.C.I.T.Y. Native IDE

A premium, high-performance developer workspace and agentic environment built in pure Rust. V.E.L.O.C.I.T.Y. combines a native dockable window interface with a self-correcting agentic compiler loop and robust DLL-level sandboxing.

---

## Key Features

- **Pure Rust Native Desktop GUI**: Built using `eframe` and `egui` for immediate-mode rendering, utilizing Vulkan GPU enumeration and acceleration for sub-millisecond frame dispatch.
- **Dockable Workspace Panels**: Powered by `egui_dock`, featuring layout persistence, multi-tab file management, real-time command terminal output, and a dedicated AI agent control panel.
- **Syntax Highlighting Code Editor**: A zero-allocation code viewer and editor integrated with `syntect` for syntax styling of Rust, Python, and configuration files.
- **Built-in agent (Antigravity)**:
  - **Dual Provider Support**: Seamless dynamic switching between **Cloudflare Workers AI** and **OpenRouter** (supporting premium models like `tencent/hy3:free`, `nvidia/nemotron-70b-instruct`, and more).
  - **Agentic Loop**: The agent loops recursively, executing tools, reading output, and self-correcting.
  - **Inline Tool Call Parsers**: Built-in parsers for text-based tool-calling models that bypass standard JSON tools (handles both `<tool_call>` XML-like blocks and `[Calling tool 'name' with arguments 'json']` bracket notations).
  - **Token Suppression & Stream Buffer**: Suppresses raw tool calls and JSON payloads from appearing in the user's chat panel, maintaining a clean chat stream.
- **Secure Sandbox Execution (`wuias_shield`)**: Integrated with a Windows DLL-level redirect sandbox. Commands and scripts inside `.nda` packages are executed within an isolated file-system environment.

---

## Directory Structure

```text
velocity-workspace/
├── velocity-mcp/          # Main MCP Server and Native IDE
│   ├── src/
│   │   ├── agent.rs       # Agent reasoning loops, SSE streams & history budget management
│   │   ├── registry.rs    # Tool definitions, DLL sandboxing, & fallback execution
│   │   ├── main.rs        # App entry point
│   │   └── editor/        # GUI panels (app, code_editor, theme, chat_panel, status_bar, etc.)
│   ├── docs/              # System architecture and UI upgrade guides
│   ├── scripts/           # Python check & helper utilities
│   └── Cargo.toml         # Rust dependency definitions
├── ide/                   # Legacy C++ / C# project files (deprecated)
└── agent/                 # Legacy Python agent components (deprecated)
```

---

## Configuration

Configure your environment by copying/creating a `.env` file in the workspace root:

```env
# Active LLM Provider ("openrouter" or "cloudflare")
LLM_PROVIDER=openrouter

# OpenRouter Configuration
OPENROUTER_API_KEY=your-api-key-here
OPENROUTER_MODEL=tencent/hy3:free

# Cloudflare Configuration
CLOUDFLARE_API_KEY=your-api-key-here
CLOUDFLARE_ACCOUNT_ID=your-account-id
```

---

## Getting Started

### Prerequisites

- [Rust toolchain](https://rustup.rs/) (Stable, 1.75+)
- Vulkan SDK (for GPU accelerated rendering)
- [Just](https://github.com/casey/just) (optional, for running shortcut recipes)

### Run the IDE

To build and run the native editor, run:

```powershell
# Using Just
just run

# Using Cargo directly
cargo run --manifest-path velocity-mcp/Cargo.toml -- --editor
```

---

## Justfile Recipes

Common development commands can be executed via `just`:

- `just check` - Run fast compiler typecheck.
- `just clippy` - Run clippy linting and enforce clean code practices.
- `just fmt` - Format the codebase.
- `just test` - Run unit tests (includes testing for the OpenRouter history compressor and relative path sandboxing).
- `just validate` - Run checks, tests, and clippy in one command.
