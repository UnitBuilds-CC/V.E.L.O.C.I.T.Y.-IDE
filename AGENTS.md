# AGENTS.md

## Project Positioning

V.E.L.O.C.I.T.Y. is a Rust workspace (`resolver = "2"`) containing six crates:

| Crate | Role |
|-------|------|
| `velocity-mcp` | Main MCP server, native IDE editor, 4-provider agent loop |
| `velocity-browser` | Pure-Rust browser control plane (DOM, layout, JS VM, net) |
| `velocity-ide` | Compiler driver, AST engine, sandbox, site-map tooling |
| `velocity-ide-gui` | Standalone GUI binary (egui/eframe workspace editor) |
| `velocity-drone` | Drone protocol agent (local + remote dual-mode) |
| `velocity-e2e` | End-to-end test harness |

## Directory Routes

```
velocity-workspace/
├── velocity-mcp/       # MCP server + IDE editor (primary crate)
│   ├── src/agent/      # Provider dispatch, reasoning loop
│   ├── src/editor/     # egui UI, VelocityApp, tier3 sub-panels
│   ├── src/registry/   # Tool registry, system tools
│   ├── src/ipc/        # Shared memory, protocol
│   └── docs/           # Architecture & format docs
├── velocity-browser/   # Browser engine (no CDP)
│   ├── src/dom/        # Slab DOM tree, shadow slots, mutations
│   ├── src/layout/     # Flexbox, grid, parallel layout
│   └── src/agentic/    # AOM tree, OCR, action predictor
└── velocity-ide/       # Compiler & AST
    ├── src/compiler/   # Lexer, parser, JIT, shaders
    │   ├── driver/     # Vulkan init, GEMV, BitNet, pipeline exec
    │   └── nda_jit/    # JIT compiler, optimizer, x86 emitter
    ├── src/model/      # Transformer model, weights, config
    ├── src/nda_int/    # NDA interpreter
    └── src/site_map/   # Triple store, Merkle verifier
```

## Commands

The `validate` gate below lives in `velocity-mcp/Justfile` and runs from `velocity-mcp/`. General workspace tasks (`build`, `test`, `lint`, `ci`, `pre-commit`) live in the root `justfile`:

| Command | Purpose |
|---------|---------|
| `just validate` | Full gate: `cargo check --all-targets` + `cargo test` + `cargo clippy -- -D warnings` |
| `just test` | Run test suite (`cargo test`) |
| `just check` | Fast typecheck (`cargo check --all-targets`) |
| `just clippy` | Static analysis only |
| `just fmt` | Format code |

**Validate route:** `velocity-mcp/Justfile` → `validate` recipe.

## High-Risk Areas

Changes in these areas require extra care and full `just validate`:

1. **Provider dispatch** (`velocity-mcp/src/agent/dispatch.rs`, `loop_runner.rs`)
   - 4-provider reasoning loop (OpenRouter, Cloudflare, Azure, Ollama).
   - Breaking dispatch breaks the entire agent subsystem.

2. **DOM / Layout engine** (`velocity-browser/src/dom/`, `velocity-browser/src/layout/`)
   - Slab-tree DOM, shadow slots, mutation batcher.
   - Flexbox/grid track solvers, parallel layout.
   - regressions cascade into rendering and agentic AOM.

3. **NDA serialization** (`.nda` binary format, 18-byte records)
   - Touches `velocity-mcp/src/protocol/`, `velocity-ide/src/nda_int/`.
   - Format docs: `velocity-mcp/docs/NDA_FORMAT.md`, `NDA_BOUNDARIES.md`.
   - Schema drift corrupts persisted state across sessions.

## Key Documentation

- Architecture: [`velocity-mcp/docs/ARCHITECTURE.md`](velocity-mcp/docs/ARCHITECTURE.md)
- Product vision: [`PRODUCT.md`](PRODUCT.md)
- NDA format spec: [`velocity-mcp/docs/NDA_FORMAT.md`](velocity-mcp/docs/NDA_FORMAT.md)
- NDA boundary decisions: [`velocity-mcp/docs/NDA_BOUNDARIES.md`](velocity-mcp/docs/NDA_BOUNDARIES.md)
