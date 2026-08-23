# Velocity MCP Server and IDE Editor

## Classification
- **Category**: Primary Crate
- **Files**: ~258 source files
- **Criticality**: Critical — user-facing surface

## Summary

`velocity-mcp` is the primary crate containing the MCP server, native IDE editor (119 egui files), 4-provider agent loop (28 files including peer-to-peer, planning, reasoning), tool registry, automation system (14 files with task decomposition, instruction registry, model ranking), orchestrator (12 files with DAG scheduling, worktree isolation), connectors (7 files), and Windows Automation module (25 files).

## Module Breakdown

| Module | Files | Purpose |
|--------|-------|---------|
| `editor/` | 119 | egui IDE: code editor, chat, browser, orchestrator panels, team studio |
| `agent/` | 28 | 4-provider AI reasoning loop, peer-to-peer, planning, self-improvement |
| `wa/` | 25 | Windows UI Automation via COM FFI |
| `registry/` | 22 | MCP tool definitions and dispatch |
| `automation/` | 14 | Task decomposition, instruction registry, model ranking, coordinator |
| `orchestrator/` | 12 | DAG scheduler, worktree isolation, worker runner |
| `connectors/` | 7 | HTTP, OAuth2, webhooks, sync rules |
| `compiler/` | 5 | JIT compiler, tokenizer, parser loader |
| `protocol/` | 3 | JSON-RPC, NMCP binary |
| `ipc/` | 3 | Shared memory telemetry |
| `security/` | 1 | Encrypted secret storage (DPAPI-backed) |

## Key Subsystems

- **Team Routing**: `@team` directives → hybrid scoring → single expert routing
- **Automation Pipeline**: Goal → `AgentTaskKind` → coupling analysis → parallel workers
- **Expert Teams**: 3 presets (C#, Android, Doccit) + custom teams via Team Studio
- **Mission Control**: Briefs, interventions, auto-execute mode

## Entry Points

- `src/main.rs`: Binary entry with `--editor` flag, `--workspace <path>` to open a specific directory
- `src/lib.rs`: Library root exposing public APIs
