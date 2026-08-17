# Velocity MCP Server and IDE Editor

## Classification
- **Category**: Primary Crate
- **Files**: ~257 source files
- **Criticality**: Critical — user-facing surface

## Summary

`velocity-mcp` is the primary crate containing the MCP server, native IDE editor (119 egui files), 4-provider agent loop, tool registry, automation system, orchestrator, and Windows Automation module (29 files). The fake benchmark module was removed; real LLM inference lives in `velocity-ide/src/model/`.

## Module Breakdown

| Module | Files | Purpose |
|--------|-------|---------|
| `editor/` | 119 | egui IDE: code editor, chat, browser, orchestrator panels |
| `wa/` | 29 | Windows UI Automation via COM FFI |
| `registry/` | 29 | MCP tool definitions and dispatch |
| `agent/` | 28 | 4-provider AI reasoning loop |
| `compiler/` | 4 | JIT compiler, tokenizer, parser loader |
| `automation/` | 14 | Task routing, build runner, watchers |
| `orchestrator/` | 12 | DAG scheduler, worktree isolation |
| `connectors/` | 8 | HTTP, OAuth2, webhooks, sync |
| `protocol/` | 3 | JSON-RPC, NMCP binary |
| `ipc/` | 3 | Shared memory telemetry |
| `benchmark/` | 0 | Removed — real benchmarks in `velocity-ide/src/compiler/driver/vulkan_benchmark.rs` |
| `security/` | 2 | Secrets management |

## Entry Points

- `src/main.rs`: Binary entry with `--editor` flag, `--workspace <path>` to open a specific directory
- `src/lib.rs`: Library root exposing public APIs
