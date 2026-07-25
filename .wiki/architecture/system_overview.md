# System Overview

High-level architecture of the Velocity workspace: a 3-crate Rust cargo workspace forming an AI-native IDE, a pure-Rust browser engine, and a compiler/indexer runtime.

---

## Crate Dependency Graph

```
┌─────────────────────────────────────────────────────────────────────┐
│                        velocity-mcp                                  │
│  (MCP Server · egui Native IDE · 4-Provider Agent Loop · WA)        │
│  220 source files                                                    │
└──────────┬──────────────────────────────────────┬────────────────────┘
           │                                      │
           │ direct Rust dependency               │ direct Rust dependency
           │ (path = "../velocity-ide")           │ (path = "../velocity-browser")
           ▼                                      ▼
┌──────────────────────────┐       ┌──────────────────────────────────┐
│      velocity-ide         │       │        velocity-browser           │
│  (Compiler · SiteMap ·   │       │  (DOM · Layout · JS VM · Net ·   │
│   Wiki · Sandbox · NDA)  │       │   Agentic AOM · Engine · Session) │
│  78 source files          │       │  109 source files                 │
└──────────────────────────┘       └──────────┬───────────────────────┘
                                              │
                                              │ direct Rust dependency
                                              │ (path = "../velocity-ide")
                                              ▼
                                   ┌──────────────────────────┐
                                   │      velocity-ide         │
                                   │  (shared dependency)      │
                                   └──────────────────────────┘
```

**Dependency direction is strictly one-way**: `velocity-mcp` depends on both `velocity-ide` and `velocity-browser`. `velocity-browser` depends on `velocity-ide`. `velocity-ide` has no intra-workspace dependencies.

### Module Inventory by Crate

| Crate | Module | Files | Purpose |
|-------|--------|-------|---------|
| velocity-mcp | `editor/` | 98 | egui native IDE: app shell, panels, browser UI, orchestrator UI |
| velocity-mcp | `wa/` | 29 | Windows Automation: UIA FFI, execution, screenshots, recording |
| velocity-mcp | `registry/` | 22 | MCP tool definitions, dispatch, system/browser/team/WA tools |
| velocity-mcp | `compiler/` | 20 | Vulkan driver, BitNet/Qwen kernels, JIT, tokenizer |
| velocity-mcp | `automation/` | 14 | Build watcher, AST watcher, mediator, task router, coordinator |
| velocity-mcp | `agent/` | 13 | 4-provider loop, dispatch, headless subagents, team routing |
| velocity-mcp | `orchestrator/` | 12 | DAG scheduler, blueprint, reconcile, worker, validator |
| velocity-mcp | `benchmark/` | 4 | Performance benchmark suite |
| velocity-mcp | `protocol/` | 3 | JSON-RPC stdio loop, shared-memory binary protocol |
| velocity-mcp | `ipc/` | 3 | Telemetry shared memory server/client |
| velocity-browser | `engine/` | 25 | Canvas, WebGPU, GPU compositor, crypto, service workers, stealth |
| velocity-browser | `net/` | 17 | HTTP/2, QUIC, TLS fingerprint rotator, WebSocket, WebRTC, Bluetooth |
| velocity-browser | `js/` | 13 | JS VM, event loop, WASM SIMD, web worker pool, DOM bindings |
| velocity-browser | `agentic/` | 10 | AOM tree, action predictor, OCR, reflection, vector memory |
| velocity-browser | `dom/` | 9 | Slab DOM tree, mutation batcher, custom elements, shadow slots |
| velocity-browser | `layout/` | 7 | Flexbox, grid track solver, parallel layout engine |
| velocity-browser | `parser/` | 6 | HTML5 tokenizer, CSS parser, stream JIT tokenizer |
| velocity-browser | `style/` | 4 | CSS cascade, animations, font shaper, specificity |
| velocity-ide | `compiler/` | 45 | Rust lexer/parser, NDA JIT, Vulkan driver, property fuzzer |
| velocity-ide | `site_map/` | 7 | RDF triple store, string hash registry, Merkle verifier |
| velocity-ide | `model/` | 5 | Data model types |
| velocity-ide | `nda_int/` | 5 | NDA interpreter/runtime |
| velocity-ide | `wiki/` | 4 | Automated Markdown wiki generator |
| velocity-ide | `sandbox/` | 3 | JIT sandbox, Wasm plugin runner |
| velocity-ide | `bin/` | 3 | Binary entry points (bench_nda_vs_rust, run_nda, test_tok) |

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
