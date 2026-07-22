# V.E.L.O.C.I.T.Y. Cognitive IDE

A premium, high-performance developer workspace and autonomous agentic environment built in pure Rust. V.E.L.O.C.I.T.Y. combines a native GPU-accelerated window interface with a pure-Rust browser control plane (`velocity-browser`), a self-correcting agentic compiler loop, and crisp sub-1k LOC component architecture.

---

## Architecture Overview

### 1. Pure-Rust Native Browser Control Plane (`velocity-browser`)
- **Engine Core**: 52 pure-Rust modules replacing legacy CDP wrappers.
- **DOM & Layout**: Slab-allocated DOM, shadow slots, flexbox, grid, and parallel layout solvers.
- **JS VM & Wasm**: Integrated JavaScript virtual machine, event loop scheduler, and SIMD-accelerated Wasm interpreter.
- **Network & TLS**: HTTP/2 & HTTP/3 (QUIC), WebSocket, WebRTC, native TLS stream with fingerprint rotator.
- **Stealth & Intelligence**: Hardware WebGPU/WebGL canvas rasterizer, stealth human behavior simulator, built-in OCR text engine, screencasting recorder (`ScreencastRecorder`), and site vector memory (`SiteVectorStore`).
- **Binary NDA Persistence**: Compact 18-byte Non-Deterministic Automata triples (`NdaTriple`) stored under `.velocity/browser_artifacts/`.

### 2. Multi-Provider AI Reasoning Engine (`velocity-mcp`)
- **4-Provider Automatic Failover**: Seamless failover chain across 4 backends:
  - `Cloudflare Workers AI` (`@cf/moonshotai/kimi-k2.7-code`)
  - `OpenRouter` (`tencent/hy3:free` / custom models)
  - `Azure OpenAI` (`gpt-4o` with deployment endpoint & `api-key` auth)
  - `Local Ollama` (`http://localhost:11434` / `llama3.2` / `qwen2.5-coder` / `deepseek-r1`)
- **History & Token Management**: Compressed history, reasoning effort payload routing, and ring-buffer activity logs.
- **Sub-Agent Worktree Isolation**: Git worktree sandbox (`WorktreeIsolationGuard`) for safe, isolated multi-agent execution runs.

### 3. Native IDE & UI Layer (`velocity-ide` & `velocity-mcp/src/editor`)
- **GUI Framework**: Hardware-accelerated `egui` interface with dark HSL color palette.
- **Workspace File Tree & Symbol History Inspector**: Browse workspace files, declarations, and inspect chronological change histories with context rationale.
- **Wasm Sandbox JIT & Property Fuzzing**: `WasmPluginRunner` and `PropertyFuzzer` in `velocity-ide` for sandbox code validation.
- **Cross-Platform Desktop Automation**: Unified `DesktopAutomationAdapter` bridging Windows UI Automation, Linux AT-SPI, and macOS Accessibility.

---

## Directory Structure

```text
velocity-workspace/
├── velocity-mcp/          # Rust MCP Server & Native IDE Editor
│   ├── src/
│   │   ├── agent/                # 4-provider reasoning loops, dispatchers, & NDA state
│   │   ├── registry/             # System, browser, & desktop tool definitions
│   │   ├── editor/               # GUI panels (app, chat, smart_sidebar, task_timeline, graph_view)
│   │   ├── automation/           # Task routing, mediator edit locks, AST watcher
│   │   ├── ipc/                  # Shared memory telemetry
│   │   ├── orchestrator/         # Worker scheduling, worktree isolation, blueprint DAG
│   │   └── wa/                   # Windows UI Automation & cross-platform desktop adapter
│   └── Cargo.toml
├── velocity-browser/      # Pure-Rust Browser Control Plane
│   ├── src/
│   │   ├── dom/                  # Slab DOM tree & mutation observers
│   │   ├── layout/               # Flexbox & grid track solvers
│   │   ├── js/                   # JS virtual machine & Wasm interpreter
│   │   ├── net/                  # HTTP/2/3, TLS fingerprint rotator, WebSocket
│   │   ├── agentic/              # AOM tree, OCR engine, action predictor
│   │   ├── screencast.rs         # Frame sequence recording & metadata
│   │   └── vector_memory.rs      # Spatial AOM site vector store
│   └── Cargo.toml
├── velocity-ide/          # Compiler & Shader Pipeline
│   ├── src/
│   │   ├── compiler/             # Lexer, parser, JIT sandbox, Wasm runner, Fuzzer
│   │   └── site_map/             # Merkle AST graph & SiteMap database
│   └── Cargo.toml
└── Cargo.toml             # Rust workspace manifest
```

---

## Configuration

Configure your environment in a `.env` file at the workspace root:

```env
# Primary LLM Provider ("cloudflare", "openrouter", "azure", or "ollama")
LLM_PROVIDER=cloudflare

# OpenRouter
OPENROUTER_API_KEY=your-openrouter-key
OPENROUTER_MODEL=tencent/hy3:free

# Azure OpenAI
AZURE_OPENAI_API_KEY=your-azure-key
AZURE_OPENAI_ENDPOINT=https://your-resource.openai.azure.com/
AZURE_OPENAI_DEPLOYMENT=gpt-4o
AZURE_OPENAI_API_VERSION=2024-06-01

# Local Ollama
OLLAMA_HOST=http://localhost:11434
OLLAMA_MODEL=llama3.2
```

---

## Getting Started

### Run the IDE

To launch the native editor workspace:

```powershell
cargo run --manifest-path velocity-mcp/Cargo.toml -- --editor
```

### Run Tests & Validation

To check and test all crates across the workspace:

```powershell
# Typecheck full workspace
cargo check --workspace

# Run all 123 unit tests
cargo test --workspace
```
