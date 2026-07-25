# Build & Development Workflow

Developer onboarding, workspace configuration, build commands, testing strategy, and pre-commit hooks for the Velocity workspace.

---

## Workspace Configuration

### Cargo Workspace

The workspace root is `velocity-workspace/Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["velocity-mcp", "velocity-ide", "velocity-browser"]
exclude = ["archive", "bin", "scratch", "memory", ".velocity", "browsing"]
```

### Crate Dependencies

```
velocity-mcp → velocity-ide (path dependency)
velocity-mcp → velocity-browser (path dependency)
velocity-browser → velocity-ide (path dependency)
```

### Key External Dependencies

| Dependency | Version | Used By | Purpose |
|-----------|---------|---------|---------|
| `egui` / `eframe` | 0.35 | velocity-mcp | Native GUI framework |
| `egui_dock` | 0.20 | velocity-mcp | Tab docking system |
| `crossbeam-channel` | 0.5 | velocity-mcp | Inter-thread communication |
| `serde` / `serde_json` | 1.0 | all crates | Serialization |
| `sha2` | 0.10 | velocity-mcp, velocity-ide | SHA-256 hashing |
| `ureq` | 2.9 | velocity-mcp, velocity-ide | HTTP client |
| `ash` | 0.37.3 | velocity-mcp, velocity-ide | Vulkan API bindings |
| `rustls` | 0.23 | velocity-browser | TLS 1.3 trust boundary |
| `tree-sitter` | 0.20 | velocity-mcp | Incremental parsing |
| `syntect` | 5.2 | velocity-mcp | Syntax highlighting |
| `ropey` | 1.6 | velocity-mcp | Text buffer management |
| `windows` | 0.58 | velocity-mcp (Windows) | UIA FFI, COM, threading |

---

## Build Commands & Justfile

### Justfile Commands

The Justfile at `velocity-workspace/velocity-mcp/Justfile` provides:

| Command | Purpose |
|---------|---------|
| `just validate` | Full gate: `cargo check` + `cargo test` + `cargo clippy` |
| `just check` | Fast typecheck (`cargo check --all-targets`) |
| `just test` | Run test suite (`cargo test`) |

### Direct Cargo Commands

```powershell
# Build all crates (debug)
cargo build

# Build all crates (release, optimized)
cargo build --release

# Build a specific crate
cargo build -p velocity_mcp
cargo build -p velocity-ide
cargo build -p velocity-browser

# Fast typecheck without full build
cargo check --workspace --all-targets

# Run all tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p velocity_mcp
cargo test -p velocity-ide
cargo test -p velocity-browser

# Clippy lint check
cargo clippy --workspace --all-targets
```

### Build Profiles

```toml
[profile.dev]
debug = 1  # Minimal debug symbols (saves disk space, prevents MSVC linker errors)
```

---

## Pre-commit Hook

### Location

`.git/hooks/pre-commit` (or configured via your hook manager)

### Trigger Logic

The pre-commit hook runs on every `git commit`:
1. **If Rust files changed** (`*.rs`): Run `cargo check --workspace --all-targets`
2. **If Cargo.toml changed**: Run `cargo check --workspace`
3. **If no Rust files changed**: Skip (fast path)

### Runtime Dependencies

- Rust toolchain (MSRV 1.75+)
- `cargo` in PATH
- PowerShell on Windows (the hook script uses `;` not `&&`)

### Manual Hook Trigger

```powershell
# Run the validation gate manually
just validate

# Or run individual checks
just check
just test
```

---

## Testing Strategy

### Test File Distribution

| Crate | Test Files | Coverage Areas |
|-------|-----------|----------------|
| velocity-mcp | 10 | agent, editor, browser UI, orchestrator, registry (system/browser/team/WA) |
| velocity-browser | 5 | AOM, session storage quota, full engine, native engine, sessions |
| velocity-ide | 4 | SiteMap, wiki, NDA JIT, tokenizer |

### Test Types

- **Unit tests**: Inline `#[cfg(test)] mod tests` in source files
- **Integration tests**: `tests/` directories in each crate
- **Browser engine tests**: `full_engine_tests.rs`, `native_engine_tests.rs`
- **Session tests**: `session_tests.rs` for browser session lifecycle

### Running Tests

```powershell
# All tests, all crates
cargo test --workspace

# Specific test module
cargo test -p velocity_mcp agent::tests
cargo test -p velocity-browser session_tests

# Show test output
cargo test -- --nocapture

# Run a specific test function
cargo test -p velocity-ide site_map::tests::test_triple_insert
```

### Acceptance Gate

Per `DELIVERY.md`:
```
All tests must pass before any commit lands on master.
cargo test --workspace → exit code 0 required.
```

---

## Developer Onboarding

### Prerequisites

- **Rust**: 1.75+ via `rustup`
- **Windows**: 10/11 for full WA support (Linux/macOS for other features)
- **Vulkan SDK**: Optional, for GPU-accelerated local LLM kernels
- **Python**: 3.10+ (for helper scripts in `scratch/`)

### First Build

```powershell
cd velocity-workspace
cargo build
```

### Launch the IDE

```powershell
# Default mode: egui native IDE
cargo run -p velocity_mcp

# Or use the built binary directly
.\target\debug\velocity_mcp.exe
```

### MCP Server Modes

```powershell
# JSON-RPC over stdin/stdout
velocity_mcp --mode stdio

# Binary protocol over shared memory
velocity_mcp --mode shmem --buffer-path nmcp_buffer.bin
```

### Environment Variables

| Variable | Purpose |
|----------|---------|
| `OPENROUTER_API_KEY` | OpenRouter API key for model access |
| Cloudflare account tokens | Configured via workspace provider settings |
| Azure OpenAI keys | Configured via workspace provider settings |
| Ollama host | Default `http://localhost:11434` |

### Workspace State Directory

After first launch, `.velocity/` contains:
- `workspace-preferences.json` — UI layout, theme, provider settings
- `sitemap.nda` — Symbol relationship database
- `telemetry_shmem.bin` — IPC shared memory
- `build_diagnostics.json` — Last cargo check result
- `agentic/` — Agentic run data

---

## File Organization Rules

### Sub-1,000 LOC Rule

All files across all crates are strictly refactored into modular sub-files under **1,000 lines of code**. This guarantees:
- Clean component isolation
- Fast compile times for incremental builds
- Readable single-file context

### Module Naming

- `mod.rs` — Module root with re-exports
- `struct_def.rs` — Struct definitions (separated from logic)
- `types.rs` — Shared type definitions
- `tests.rs` — Unit tests (behind `#[cfg(test)]`)
- `helpers.rs` — Utility functions

### Archive & Scratch

- `archive/` — Archived Python code (not compiled, not tested)
- `scratch/` — Experimental scripts (not compiled, not tested)
- These are excluded from the Cargo workspace
