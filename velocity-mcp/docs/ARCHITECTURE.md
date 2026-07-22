# V.E.L.O.C.I.T.Y. IDE Architecture

V.E.L.O.C.I.T.Y. is a high-performance, AI-native IDE written in pure Rust. Its core responsibilities are: editing code, executing a 4-provider autonomous agentic reasoning loop, browser automation without CDP dependencies, compiling user projects, and serializing state into 18-byte NDA (`.nda`) binary format.

For agent-facing NDA authoring guidance, see `docs/NDA_FORMAT.md`.
For NDA-vs-JSON boundary decisions, see `docs/NDA_BOUNDARIES.md`.

## Workspace Crate Layout

- **`velocity-mcp`** — Main Rust MCP Server & Native IDE Editor
  - `agent/` — 4-provider reasoning loops (`loop_runner.rs`, `dispatch.rs`), OpenRouter/Cloudflare/Azure/Ollama dispatchers, history compressor.
  - `editor/` — Native egui UI layer:
    - `app/` — Root `VelocityApp` layout, docking, command palette, keymaps.
    - `chat_panel.rs` — Streaming markdown chat panel with model selection dropdown.
    - `smart_sidebar.rs` — Ring-buffer context-aware sidebar with diagnostic filtering.
    - `task_timeline.rs` — Zero-allocation mission activity event feed.
    - `graph_view.rs` — Workspace File Tree & Symbol Change History Explorer.
  - `registry/` — System, native browser, and desktop automation tool registry.
  - `orchestrator/` — DAG work package scheduling, `WorktreeIsolationGuard` sub-agent sandbox, and live worker handles.
  - `wa/` — Windows UI Automation & `DesktopAutomationAdapter` cross-platform accessibility framework.
- **`velocity-browser`** — Pure-Rust Native Browser Control Plane (52 Modules)
  - `dom/` — Slab DOM tree, shadow slots, mutation batcher.
  - `layout/` — Flexbox, grid track solvers, parallel layout engine.
  - `js/` — JS virtual machine, event loop scheduler, Wasm SIMD interpreter.
  - `net/` — HTTP/2/3 (QUIC), TLS fingerprint rotator, WebSocket, WebRtc.
  - `agentic/` — Spatial AOM tree, Velocity OCR text engine, action predictor.
  - `screencast.rs` — `ScreencastRecorder` frame metadata logger.
  - `vector_memory.rs` — `SiteVectorStore` spatial AOM site memory.
- **`velocity-ide`** — Compiler Driver & AST Engine
  - `compiler/` — Lexer, parser, JIT sandbox, `WasmPluginRunner`, `PropertyFuzzer`.
  - `site_map/` — Merkle AST graph indexer and `SiteMap` database.

## Thread Boundaries

| Thread | Owner | Sends to UI | Receives from UI |
|--------|-------|-------------|------------------|
| UI (main) | `VelocityApp` | `UiToAgentMessage` | `AgentToUiMessage` |
| Agent Thread | `run_agent_thread` | `AgentToUiMessage` | `UiToAgentMessage` |
| Worker Threads | `LiveWorkerHandle` | `WorkerThreadEvent` | `UiToAgentMessage::CancelTask` |

## 4-Provider AI Reasoning Loop Data Flow

1. User prompt → `UiToAgentMessage::UserPrompt` → `run_agent_thread`.
2. Agent inspects configured provider (`CloudflareWorkersAi`, `OpenRouter`, `AzureOpenAi`, `LocalOllama`).
3. Streaming response fills `assistant_content` and dispatches tools.
4. If a provider encounters quota exhaustion or network timeout, the loop seamlessly fails over to the next provider in the chain: `CloudflareWorkersAi` -> `OpenRouter` -> `AzureOpenAi` -> `LocalOllama`.
5. Tool execution runs with exact workspace path sandboxing.
6. After modifications, `run_compilation_check()` validates compiler status with `cargo check`.

## Sub-1,000 LOC Modular Architecture Rule

All files across all crates in the workspace are strictly refactored into modular sub-files under **1,000 lines of code**, guaranteeing clean component isolation and maintainability.

