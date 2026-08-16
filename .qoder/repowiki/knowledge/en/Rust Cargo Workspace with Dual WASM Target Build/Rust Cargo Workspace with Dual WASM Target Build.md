# Rust Cargo Workspace with Dual WASM Target Build

## Classification
- **Category**: Build System
- **Files**: Cargo.toml (workspace + 5 crates)
- **Criticality**: High — build infrastructure

## Summary

The workspace uses Cargo with resolver "2" and supports both native (x86_64-pc-windows-msvc) and WASM (wasm32-unknown-unknown) targets. The browser crate compiles to WASM for web deployment.

## Build Commands

```powershell
cargo check --workspace          # Fast typecheck
cargo build                      # Debug build
cargo build --release            # Release (stripped, thin LTO, opt-size)
cargo test --workspace           # All tests
rustup target add wasm32-unknown-unknown  # WASM target
```

## Profile Configuration

- **dev**: `debug = 1` (minimal symbols, prevents MSVC linker overflow)
- **release**: `strip = true`, `lto = "thin"`, `opt-level = "s"`, `codegen-units = 16`

## Justfile (velocity-mcp/)

Provides shortcuts: `validate`, `test`, `check`, `clippy`, `fmt`
