# V.E.L.O.C.I.T.Y. Cognitive IDE

A premium, high-performance developer workspace and autonomous agentic environment built in pure Rust. V.E.L.O.C.I.T.Y. combines a native GPU-accelerated interface with a pure-Rust browser control plane (`velocity-browser`), a self-correcting agentic compiler loop, and crisp sub-1k LOC component architecture.

---

## Quick Start

### Prerequisites

- Rust 1.75+ (stable)
- Git
- System dependencies (see [Deployment Guide](docs/DEPLOYMENT.md))

### Build & Run

```bash
# Clone the repository
git clone https://github.com/UnitBuilds/Velocity-IDE.git
cd Velocity-IDE

# Build all crates
cargo build --release

# Run the GUI
cargo run --release --bin velocity_ide

# Or run the MCP server (headless)
cargo run --release --bin velocity_mcp -- --mode stdio
```

### Run Tests

```bash
# Run all 8,883 tests
cargo test --workspace

# Run with coverage
cargo llvm-cov --workspace --lcov
```

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
Velocity-IDE/
├── Velocity-IDE/                # V.E.L.O.C.I.T.Y. Cognitive IDE (Rust workspace)
│   ├── velocity-mcp/            # MCP Server & Native IDE Editor
│   │   ├── src/
│   │   │   ├── agent/           # 4-provider reasoning loops, dispatchers, & NDA state
│   │   │   ├── registry/        # System, browser, & desktop tool definitions
│   │   │   ├── editor/          # GUI panels (app, chat, smart_sidebar, task_timeline, graph_view)
│   │   │   ├── automation/      # Task routing, mediator edit locks, AST watcher
│   │   │   ├── ipc/             # Shared memory telemetry
│   │   │   ├── orchestrator/    # Worker scheduling, worktree isolation, blueprint DAG
│   │   │   ├── health.rs        # Health check endpoints
│   │   │   └── wa/              # Windows UI Automation & cross-platform desktop adapter
│   │   └── Cargo.toml
│   ├── velocity-browser/        # Pure-Rust Browser Control Plane
│   │   ├── src/
│   │   │   ├── dom/             # Slab DOM tree & mutation observers
│   │   │   ├── layout/          # Flexbox & grid track solvers
│   │   │   ├── js/              # JS virtual machine & Wasm interpreter
│   │   │   ├── net/             # HTTP/2/3, TLS fingerprint rotator, WebSocket
│   │   │   ├── agentic/         # AOM tree, OCR engine, action predictor
│   │   │   ├── screencast.rs    # Frame sequence recording & metadata
│   │   │   └── vector_memory.rs # Spatial AOM site vector store
│   │   └── Cargo.toml
│   ├── velocity-ide/            # Compiler & Shader Pipeline
│   │   ├── src/
│   │   │   ├── compiler/        # Lexer, parser, JIT sandbox, Wasm runner, Fuzzer
│   │   │   ├── logging.rs       # Structured logging configuration
│   │   │   └── site_map/        # Merkle AST graph & SiteMap database
│   │   └── Cargo.toml
│   ├── docs/                    # Documentation
│   │   ├── DEPLOYMENT.md        # Deployment guide
│   │   └── RUNBOOK.md           # Operational runbook
│   └── Cargo.toml               # Rust workspace manifest
├── velocity-router/             # Multi-model orchestration service (separate repo)
├── velocity-website/            # Marketing & dashboard (separate repo)
└── memory/                      # Shared memory artifacts
```

---

## Configuration

Configure your environment in `~/.velocity/config.toml`:

```toml
[general]
workspace = "/path/to/workspace"
log_level = "info"

[providers.openai]
api_key = "sk-..."
base_url = "https://api.openai.com/v1"

[providers.anthropic]
api_key = "sk-ant-..."

[providers.cloudflare]
api_key = "..."
account_id = "..."
```

Or use environment variables:

```bash
export VELOCITY_API_KEY=your-api-key
export RUST_LOG=info
```

See [Deployment Guide](docs/DEPLOYMENT.md) for full configuration options.

## Documentation

- **[Deployment Guide](docs/DEPLOYMENT.md)** — Installation, Docker, Kubernetes, and production deployment
- **[Operational Runbook](docs/RUNBOOK.md)** — Monitoring, incident response, and maintenance procedures
- **[CHANGELOG](CHANGELOG.md)** — Version history and release notes

## Support

- **GitHub Issues:** [Report bugs](https://github.com/UnitBuilds/Velocity-IDE/issues)
- **GitHub Discussions:** [Ask questions](https://github.com/UnitBuilds/Velocity-IDE/discussions)
- **Email:** support@velocity-ide.com

## License

See [LICENSE](LICENSE) for details.
