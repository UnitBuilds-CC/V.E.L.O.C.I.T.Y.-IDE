# Development Guide

<cite>
**Referenced Files in This Document**
- [Cargo.toml](file://Cargo.toml)
- [AGENTS.md](file://AGENTS.md)
- [velocity-mcp/Justfile](file://velocity-mcp/Justfile)
- [velocity-mcp/Cargo.toml](file://velocity-mcp/Cargo.toml)
- [velocity-browser/Cargo.toml](file://velocity-browser/Cargo.toml)
- [velocity-ide/Cargo.toml](file://velocity-ide/Cargo.toml)
- [.github/workflows/ci.yml](file://.github/workflows/ci.yml)
</cite>

## Table of Contents
1. [Introduction](#introduction)
2. [Workspace Organization](#workspace-organization)
3. [Build System and Toolchain](#build-system-and-toolchain)
4. [Testing Procedures](#testing-procedures)
5. [Code Style and Conventions](#code-style-and-conventions)
6. [Sub-1k LOC Rule](#sub-1k-loc-rule)
7. [Adding New Features](#adding-new-features)
8. [High-Risk Areas](#high-risk-areas)
9. [CI Pipeline](#ci-pipeline)

## Introduction

This guide covers development conventions, build workflows, testing strategy, and code organization for the Velocity workspace. It is intended for contributors working on any of the three primary crates or the supporting drone/e2e crates.

## Workspace Organization

The workspace uses `resolver = "2"` and contains five member crates:

| Crate | Purpose | File Count |
|-------|---------|------------|
| `velocity-mcp` | MCP server, IDE editor, agent loop, Windows automation | ~257 |
| `velocity-browser` | Pure-Rust browser engine | ~171 |
| `velocity-ide` | NDA compiler, JIT, site map, model inference, dual-path engine | ~77 |
| `drone` | Safety monitor process | ~5 |
| `e2e` | End-to-end integration tests | ~4 |

Excluded from workspace: `archive/`, `bin/`, `scratch/`, `memory/`, `.velocity/`, `browsing/`, `fuzz/`.

### Module Ownership

Each crate owns its domain:
- **velocity-mcp**: User-facing IDE, agent reasoning, tool dispatch, automation, desktop control
- **velocity-browser**: Web engine (DOM, layout, JS, networking, sessions, agentic features)
- **velocity-ide**: Compilation pipeline, NDA format, model inference, site map indexing

Cross-crate dependencies flow: `velocity-mcp` → `velocity-browser` → `velocity-ide`. The browser and IDE crates should not depend on the MCP crate.

## Build System and Toolchain

### Rust Configuration

```toml
[profile.dev]
debug = 1          # Minimal debug symbols — prevents MSVC linker overflow

[profile.release]
strip = true       # Strip symbols
lto = "thin"       # Thin LTO for good reduction without excessive compile
opt-level = "s"    # Optimize for size
codegen-units = 16 # Parallel codegen
```

### Essential Commands

```powershell
# From workspace root
cargo check --workspace          # Fast typecheck
cargo build                      # Debug build
cargo build --release            # Release build
cargo test --workspace           # All tests
cargo fmt --all                  # Format
cargo clippy --workspace -- -D warnings  # Lint

# From velocity-mcp/ (Justfile)
just validate                    # Full gate: check + test + clippy
just test                        # cargo test
just check                       # cargo check --all-targets
just clippy                      # cargo clippy
just fmt                         # cargo fmt
```

### WASM Target

The browser crate supports WASM compilation. Ensure the target is installed:
```powershell
rustup target add wasm32-unknown-unknown
```

## Testing Procedures

### Unit Tests
- Place alongside modules using `#[cfg(test)]`
- Run with `cargo test -p <crate>`
- Each crate maintains its own test suite

### Integration Tests
- `e2e/tests/` — browser engine, MCP stdio, NDA pipeline
- `velocity-browser/tests/` — engine integration, session tests
- `velocity-mcp/tests/` — tool registry, protocol tests
- `velocity-ide/tests/` — compiler pipeline tests

### Fuzz Targets
- `fuzz/fuzz_targets/` — HTML parser, NDA lexer, NDA parser, WASM runner
- Run with `cargo fuzz run <target>`

### Test Quality Standards
- Prefer behavior tests over smoke tests
- Test monotonicity relationships (higher X → higher/lower Y)
- Test determinism (same input → same output)
- Test edge cases (empty input, max enum, zero state)
- Target 15-25 behavior tests per module

## Code Style and Conventions

### Formatting
- Use `cargo fmt` — all code must be formatted
- 4-space indentation (Rust default)

### Linting
- `cargo clippy -- -D warnings` — zero warnings policy
- Address all clippy suggestions before committing

### Naming
- `snake_case` for functions, variables, modules
- `PascalCase` for types, traits, enums
- `UPPER_SNAKE_CASE` for constants
- Prefix test functions with `test_`

### Documentation
- `///` doc comments on all public items
- `//!` module-level docs on every `mod.rs`
- Include examples in doc comments for public APIs

## Sub-1k LOC Rule

**All files must remain under 1,000 lines of code.** This is a hard architectural constraint for clean isolation. If a file approaches this limit:
1. Extract helper functions into sibling files
2. Split into submodules (e.g., `mod.rs` + child modules)
3. Move types into dedicated `types.rs`

## Adding New Features

### Adding a New Module
1. Create the file under the appropriate crate's `src/` directory
2. Register it in the parent `mod.rs`
3. Keep under 1,000 LOC
4. Add `#[cfg(test)]` module with behavior tests
5. Run `just validate` before committing

### Adding a New Tool
1. Define the tool in `velocity-mcp/src/registry/tool_definitions/`
2. Implement dispatch in the appropriate category (`system_tools.rs`, `browser_tools/`, `team_tools.rs`, `wa_tools.rs`)
3. Register in `velocity-mcp/src/registry/dispatch.rs`
4. Add tests in `velocity-mcp/src/registry/tests/`

### Adding a New Provider
1. Implement the provider trait in `velocity-mcp/src/agent/provider.rs`
2. Add to the failover chain in `velocity-mcp/src/agent/executor/dispatch.rs`
3. Add configuration in `.env` parsing
4. Test failover behavior

## High-Risk Areas

Changes in these areas require extra care and full `just validate`:

1. **Provider dispatch** (`velocity-mcp/src/agent/executor/dispatch.rs`, `loop_runner.rs`)
   - 4-provider reasoning loop. Breaking dispatch breaks all agent subsystems.

2. **DOM / Layout engine** (`velocity-browser/src/dom/`, `velocity-browser/src/layout/`)
   - Slab-tree DOM, shadow slots, flexbox/grid solvers. Regressions cascade into rendering and AOM.

3. **NDA serialization** (`velocity-mcp/src/protocol/`, `velocity-ide/src/nda_int/`)
   - 18-byte binary format. Schema drift corrupts persisted state across sessions.

4. **Editor app struct** (`velocity-mcp/src/editor/app/velocity_app/struct_def.rs`)
   - Central VelocityApp struct. Changes affect all UI panels.

## CI Pipeline

The CI pipeline (`.github/workflows/ci.yml`) runs:
1. `cargo check --workspace --all-targets`
2. `cargo test --workspace`
3. `cargo clippy --workspace -- -D warnings`
4. `cargo fmt --all -- --check`

All gates must pass before merge.

**Section sources**
- [AGENTS.md](file://AGENTS.md)
- [Cargo.toml](file://Cargo.toml)
- [velocity-mcp/Justfile](file://velocity-mcp/Justfile)
- [.github/workflows/ci.yml](file://.github/workflows/ci.yml)
