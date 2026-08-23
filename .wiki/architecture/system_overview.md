# System Overview

High-level architecture of the Velocity workspace: a 5-crate Rust cargo workspace forming an AI-native IDE, a pure-Rust browser engine, a compiler/indexer runtime, a portable drone agent endpoint, and an end-to-end test harness.

---

## Crate Dependency Graph

```
┌─────────────────────────────────────────────────────────────────────┐
│                        velocity-mcp                                  │
│  (MCP Server · egui Native IDE · 4-Provider Agent Loop · WA)        │
│  258 source files                                                    │
└──────────┬──────────────────────────────────────┬────────────────────┘
           │                                      │
           │ direct Rust dependency               │ direct Rust dependency
           │ (path = "../velocity-ide")           │ (path = "../velocity-browser")
           ▼                                      ▼
┌──────────────────────────┐       ┌──────────────────────────────────┐
│      velocity-ide         │       │        velocity-browser           │
│  (Compiler · SiteMap ·   │       │  (DOM · Layout · JS VM · Net ·   │
│   Wiki · Sandbox · NDA)  │       │   Agentic AOM · Engine · Session) │
│  75 source files          │       │  171 source files                 │
└──────────────────────────┘       └──────────┬───────────────────────┘
                                              │
                                              │ direct Rust dependency
                                              │ (path = "../velocity-ide")
                                              ▼
                                   ┌──────────────────────────┐
                                   │      velocity-ide         │
                                   │  (shared dependency)      │
                                   └──────────────────────────┘

┌──────────────────────────┐       ┌──────────────────────────────────┐
│     velocity-drone        │       │           e2e                     │
│  (Portable Agent · HTTP  │       │  (Integration Tests · Browser ·   │
│   Server · File Xfer)    │       │   Load Bench · MCP · NDA)         │
│  5 source files           │       │  5 source files                   │
└──────────────────────────┘       └──────────────────────────────────┘
```

**Dependency direction is strictly one-way**: `velocity-mcp` depends on both `velocity-ide` and `velocity-browser`. `velocity-browser` depends on `velocity-ide`. `velocity-ide` has no intra-workspace dependencies.

### Module Inventory by Crate

| Crate | Module | Files | Purpose |
|-------|--------|-------|---------|
| velocity-mcp | `editor/` | 119 | egui native IDE: app shell, panels, browser UI, orchestrator UI |
| velocity-mcp | `agent/` | 28 | 4-provider loop, dispatch, headless subagents, team routing, reasoning, planning, peer-to-peer |
| velocity-mcp | `wa/` | 29 | Windows Automation: UIA FFI, execution, screenshots, recording |
| velocity-mcp | `registry/` | 29 | MCP tool definitions, dispatch, system/browser/team/WA tools |
| velocity-mcp | `automation/` | 14 | Build watcher, AST watcher, mediator, task router, coordinator, instruction registry |
| velocity-mcp | `orchestrator/` | 12 | DAG scheduler, blueprint, reconcile, worker, validator |
| velocity-mcp | `connectors/` | 8 | External service connectors, OAuth2, webhooks, sync |
| velocity-mcp | `compiler/` | 4 | Vulkan driver, BitNet/Qwen kernels, JIT, tokenizer |
| velocity-mcp | `protocol/` | 3 | JSON-RPC stdio loop, shared-memory binary protocol |
| velocity-mcp | `ipc/` | 4 | Telemetry shared memory server/client |
| velocity-mcp | `security/` | 2 | Encrypted secret storage (DPAPI-backed) |
| velocity-browser | `engine/` | 39 | Canvas, WebGPU, GPU compositor, crypto, service workers, stealth, CAPTCHA solver (14) |
| velocity-browser | `net/` | 19 | HTTP/2, QUIC, TLS fingerprint rotator, WebSocket, WebRTC, Bluetooth |
| velocity-browser | `js/` | 56 | JS VM, interpreter (27), interpreter tests (16), event loop, WASM SIMD, web worker pool |
| velocity-browser | `agentic/` | 10 | AOM tree, action predictor, OCR, reflection, vector memory |
| velocity-browser | `dom/` | 9 | Slab DOM tree, mutation batcher, custom elements, shadow slots |
| velocity-browser | `session*` | 10 | Session management, cookies, history, storage, IndexedDB, swarm |
| velocity-browser | `layout/` | 7 | Flexbox, grid track solver, parallel layout engine |
| velocity-browser | `parser/` | 6 | HTML5 tokenizer, CSS parser, stream JIT tokenizer |
| velocity-browser | `style/` | 5 | CSS cascade, animations, font shaper, transitions |
| velocity-browser | root | 20 | Top-level modules: session, nda, aom, screencast, vector_memory, etc. |
| velocity-ide | `compiler/` | 43 | Rust lexer/parser, NDA JIT (9), Vulkan driver (12), SPIR-V shaders (18) |
| velocity-ide | `site_map/` | 7 | RDF triple store, string hash registry, Merkle verifier |
| velocity-ide | `model/` | 5 | Data model types |
| velocity-ide | `nda_int/` | 5 | NDA interpreter/runtime |
| velocity-ide | `wiki/` | 4 | Automated Markdown wiki generator |
| velocity-ide | `sandbox/` | 3 | JIT sandbox, Wasm plugin runner |
| velocity-drone | `src/` | 5 | Drone core, HTTP server, safety, lib |
| e2e | `tests/` | 5 | Browser engine, load benchmarks, MCP stdio, NDA pipeline |

---

## Thread Model & Concurrency

Velocity uses a multi-threaded architecture with explicit channel-based communication. All inter-thread messaging uses `crossbeam_channel` (bounded or unbounded) to avoid shared mutable state.

### Thread Topology

| Thread | Owner | Sends To | Receives From |
|--------|-------|----------|---------------|
| **UI (main)** | `VelocityApp` (egui) | `UiToAgentMessage` via `agent_tx` | `AgentToUiMessage` via `agent_rx` |
| **Agent Thread** | `run_agent_thread()` | `AgentToUiMessage` via `agent_tx` | `UiToAgentMessage` via `agent_rx` |
| **Telemetry Server** | `TelemetryServer::open()` | AST update responses | Shared memory requests from IDE/watchers |
| **AST File Watcher** | `spawn_ast_watcher()` | AST update requests via shmem | Filesystem change events |
| **Build Watcher** | `spawn_build_watcher()` | Build diagnostics via shmem | Cargo output polling |
| **Worker Threads** | `LiveWorkerHandle` (orchestrator) | `WorkerThreadEvent` | `UiToAgentMessage::CancelTask` |
| **Headless Sub-Agents** | `run_headless_subagent()` | Progress via `Arc<Mutex<HeadlessSubAgentProgress>>` | Cancel signal via `cancel_rx` |

### Key Concurrency Patterns

- **Channel topology**: Two unbounded crossbeam channels (`ui_tx`/`agent_rx` and `agent_tx`/`ui_rx`) form the bidirectional UI↔agent bridge.
- **MediatorArena**: File-level presence locking with TTL-based stale lock pruning (2-second TTL). Prevents agent/user edit conflicts.
- **SiteMap mutex**: `Mutex<SiteMap>` shared between telemetry server and main thread for AST update persistence.
- **Atomic ring buffer**: 64KB shared memory segment (`telemetry_shmem.bin`) for zero-allocation telemetry exchange.
- **Arc<Mutex<Progress>>**: Headless sub-agents report progress via shared mutable state with mutex protection.

---

## IPC Topology

Velocity uses two IPC mechanisms:

### 1. Crossbeam Channels (UI ↔ Agent)

The primary communication path between the egui UI and the agent reasoning loop:

```
UI Thread                          Agent Thread
┌──────────────┐                   ┌──────────────────┐
│ VelocityApp  │──UiToAgentMsg───▶│ run_agent_thread │
│              │                   │                  │
│              │◀──AgentToUiMsg────│                  │
└──────────────┘                   └──────────────────┘
     crossbeam_channel                  crossbeam_channel
```

**`UiToAgentMessage` variants**: `UserPrompt`, `SetModel`, `SetProvider`, `SetThinking`, `ApproveTool`, `RejectTool`, `CancelTask`, `ClearHistory`, `RefreshModels`, `RunLocalBuild`, `RunLocalRun`, `ReloadTeams`, `ApplySessionState`, `ReloadProviderConfig`, `SetWorkspace`.

**`AgentToUiMessage` variants**: `OutputToken`, `ThoughtToken`, `RequestToolApproval`, `ToolExecutionStarted`, `ToolExecutionFinished`, `StatusUpdate`, `AgentFinished`, `UpdateFileBuffer`, `ModelCatalog`, `AccountUsage`, `ChatHistoryRestored`, `ProviderChanged`.

### 2. Shared Memory (Telemetry)

A 64KB atomic ring buffer in `telemetry_shmem.bin` for:
- **AST updates**: File watcher detects changes → telemetry server persists to SiteMap
- **AST deletes**: File removal → telemetry server removes from SiteMap
- **Presence updates**: Cursor position → MediatorArena checks for conflicts

---

## Data Flow: User Prompt to Agent Response

```
1. User types prompt in ChatPanel
       │
       ▼
2. UI sends UiToAgentMessage::UserPrompt(text) via agent_tx
       │
       ▼
3. run_agent_thread() receives on agent_rx
       │
       ▼
4. Agent builds request with:
   - Selected provider (AiProvider enum)
   - Selected model (ModelInfo)
   - Chat history (Vec<ChatMessage>)
   - Tool definitions (from registry::get_tools())
       │
       ▼
5. Provider dispatch (executor/dispatch.rs):
   - CloudflareWorkersAi → OpenRouter → AzureOpenAi → LocalOllama
   - On failure: fallback_provider() rotates to next
       │
       ▼
6. Streaming response → AgentToUiMessage::OutputToken chunks
       │
       ▼
7. If tool calls detected:
   - AgentToUiMessage::RequestToolApproval → UI shows approval dialog
   - User approves → UiToAgentMessage::ApproveTool
   - Tool executes via registry::call_tool_in_workspace()
   - Result → AgentToUiMessage::ToolExecutionFinished
   - Loop continues with tool result in context
       │
       ▼
8. Agent finishes → AgentToUiMessage::AgentFinished
       │
       ▼
9. UI renders final state, updates chat history
```

---

## Workspace State Directory

All persistent state lives under `.velocity/` in the workspace root:

| File | Purpose |
|------|---------|
| `.velocity/sitemap.nda` | RDF triple store (symbol relationships) |
| `.velocity/changelog.nda` | Workspace git history & incremental edits |
| `.velocity/transcript.nda` | Agent conversation logs |
| `.velocity/workspace-preferences.json` | UI layout, theme, provider, model selection |
| `.velocity/telemetry_shmem.bin` | IPC shared memory segment |
| `.velocity/nda.key` | NDA encryption key (DPAPI-protected on Windows) |
| `.velocity/build_diagnostics.json` | Last cargo check result |
| `.velocity/agentic/` | Agentic run data, task snapshots |
| `.velocity/site_map/` | SiteMap disk-backed index |

---

## Build & Entry Points

The workspace produces a single binary `velocity_mcp` with multiple modes:

| Command | Mode | Description |
|---------|------|-------------|
| `velocity_mcp` (no args) | Editor | Launch egui native IDE |
| `velocity_mcp --editor` | Editor | Explicit editor launch |
| `velocity_mcp --mode stdio` | MCP Server | JSON-RPC over stdin/stdout |
| `velocity_mcp --mode shmem` | MCP Server | Binary protocol over shared memory |
| `velocity_mcp --benchmark` | Benchmark | Run performance suite |
| `velocity_mcp --tokenize <text>` | Tokenizer | NDA embedded tokenizer demo |
| `velocity_mcp --check` | Self-check | Run `cargo check` and exit |
