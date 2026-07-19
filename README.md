# V.E.L.O.C.I.T.Y. Native IDE

A premium, high-performance developer workspace and agentic environment built in pure Rust. V.E.L.O.C.I.T.Y. combines a native dockable window interface with a self-correcting agentic compiler loop and robust DLL-level sandboxing.

---

## Architecture Overview

### Rust IDE (velocity-mcp)
- **GUI Framework**: `egui` with `egui_dock` for dockable panels
- **Rendering**: Vulkan GPU acceleration for sub-millisecond frame dispatch
- **Core Features**:
  - Syntax highlighting code editor with `syntect`
  - Real-time command terminal
  - AI agent control panel with dual providers (OpenRouter, Cloudflare)
  - DLL-level sandboxing for tool execution
  - Agent reasoning loops with history budget management

### Go Browser (browsing/)
- **Orchestration**: Swarm coordination, node agents, graph drivers
- **API Server**: RESTful API for browser control and automation
- **Database**: Sitemap, vault, and graph storage
- **Capabilities**:
  - Automated web crawling and comparison testing
  - Session management with security policies
  - CAPTCHA solving and challenge handling
  - Orchestrator for distributed browser instances
  - Native host integration for browser automation
  - Wireguard VPN integration

### MCP Integration
- Both Rust IDE and Go browser expose MCP interfaces
- Tool registry bridges IDE commands to browser/orchestration capabilities
- Shared protocol for seamless cross-language communication

---

## Directory Structure

```text
velocity-workspace/
├── velocity-mcp/          # Main Rust MCP Server + Native GUI IDE (egui-based)
│   ├── src/
│   │   ├── agent.rs              # Agent reasoning loops, SSE streams & history management
│   │   ├── registry.rs           # Tool definitions, DLL sandboxing, & fallback execution
│   │   ├── main.rs               # App entry point
│   │   ├── editor/               # GUI panels (app, code_editor, theme, chat, status_bar, browser)
│   │   ├── automation/           # Build runner, test orchestration, mediators
│   │   ├── compiler/             # Tokenizer, JIT, shader compilation
│   │   ├── protocol/             # NMCP binary & JSON-RPC implementations
│   │   ├── ipc/                  # Shared memory & telemetry
│   │   └── orchestrator/         # Worker scheduling, validation, reconciliation
│   ├── docs/                     # System architecture and UI guides
│   ├── scripts/                  # Helper utilities
│   └── Cargo.toml
├── browsing/              # Go-based browser engine and orchestration
│   ├── cmd/               # Executables (API, crawler, orchestrator, native_host, mcp, etc.)
│   ├── pkg/               # Core packages (browser, graph, vault, swarm, db, etc.)
│   ├── dashboard/         # Web dashboard
│   ├── extension/         # Browser extension
│   ├── go.mod             # Go dependencies
│   └── Sovereign.Containerfile
├── archive/               # Deprecated components
│   ├── ide/               # Legacy Python Textual TUI IDE
│   └── agent/             # Legacy Python agent
└── Cargo.toml             # Rust workspace manifest
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
