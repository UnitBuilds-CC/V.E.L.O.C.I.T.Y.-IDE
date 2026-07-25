# Velocity Workspace Wiki

_Comprehensive Architecture & Technical Documentation for the Velocity Ecosystem_

This wiki provides in-depth technical documentation for all components of the Velocity codebase: a 3-crate Rust workspace (407 source files) forming an AI-native IDE, a pure-Rust browser engine, and a compiler/indexer runtime.

---

## Architecture Guides

| Page | Coverage |
|------|----------|
| [System Overview](architecture/system_overview.md) | 3-crate dependency graph, thread model, IPC topology, data flow from user prompt to agent response, workspace state directory |
| [velocity-mcp: Agent Loop & Orchestrator](architecture/velocity_mcp.md) | 4-provider reasoning loop, provider failover chain, team routing, DAG task scheduling, worktree isolation, MediatorArena, automation watchers |
| [velocity-mcp: Editor & IDE UI](architecture/editor_ide_ui.md) | VelocityApp struct (98 files), work modes, docking, code editor, chat panel, graph view, browser panel, orchestrator panel, smart sidebar, 30+ IDE feature modules |
| [velocity-mcp: Tool Registry & Windows Automation](architecture/tool_registry_wa.md) | MCP tool dispatch, system/browser/team/WA tool categories, WA platform architecture (29 files), UIA FFI, execution synthesis, NDA artifact persistence |
| [velocity-browser: Engine & Networking](architecture/velocity_browser.md) | Slab DOM tree, flexbox/grid layout, JS VM, WASM SIMD, engine capabilities (25 files), TLS 1.3, HTTP/2/3, WebRTC, session management, parser/style subsystems |
| [velocity-ide: Compiler & SiteMap](architecture/velocity_ide.md) | Rust-to-NDA compiler pipeline, RDF triple store, string hash registry, Merkle verification, automated wiki generator, sandbox, JIT, NDA interpreter, tokenizer |

---

## Subsystem Deep-Dives

| Page | Coverage |
|------|----------|
| [Agentic Browser Subsystem](subsystems/agentic_browser.md) | AOM tree extraction, action predictor engine, outcome scorer, reflection loop, provider scorer, OCR engine, zero-alloc NDA writer, vector memory |
| [Custom TLS 1.3 & Networking Stack](subsystems/custom_tls_net.md) | Crypto primitives (X25519, AES-GCM, ChaCha20), TLS 1.3 handshake, HTTP/2+WebSocket, QUIC, WebRTC, TLS fingerprint rotation, proxy resolver |
| [JavaScript & WASM Engine](subsystems/js_wasm_engine.md) | JS VM (ES6+), DOM bindings, Web APIs, event loop scheduler, WASM SIMD execution, Web Worker pool, event system |
| [MCP Tool Registry](subsystems/mcp_tool_registry.md) | Tool categories, dispatch flow, system/browser/team/WA tools |
| [Multi-Agent Task Orchestrator](subsystems/multi_agent_orchestrator.md) | DAG scheduling, worktree locks, team routing, expert teams, mission control, task timeline |
| [SiteMap & NDA Binary Compiler](subsystems/sitemap_nda_compiler.md) | RDF triple store, NDA format, string hash registry, Merkle verification, wiki builder |
| [NDA Format & Security Model](subsystems/nda_security.md) | NDA binary format spec, SHA-256 Merkle integrity chain, at-rest security model, NDA vs JSON boundary, agent authoring rules |

---

## References & Developer Guides

| Page | Coverage |
|------|----------|
| [API Reference](references/api_reference.md) | Code-verified public types and function signatures for all 3 crates: AiProvider, UiToAgentMessage, VelocityApp, SiteMap, browser re-exports |
| [Build & Development Workflow](references/build_workflow.md) | Cargo workspace config, Justfile commands, pre-commit hooks, testing strategy, developer onboarding, file organization rules |

---

## Codebase Crate Map

| Crate | Files | Key Modules |
|-------|-------|-------------|
| **velocity-mcp** | 220 | `editor/` (98), `wa/` (29), `registry/` (22), `compiler/` (20), `automation/` (14), `agent/` (13), `orchestrator/` (12) |
| **velocity-browser** | 109 | `engine/` (25), `net/` (17), `js/` (13), `agentic/` (10), `dom/` (9), `layout/` (7), `parser/` (6), `style/` (4) |
| **velocity-ide** | 78 | `compiler/` (45), `site_map/` (7), `model/` (5), `nda_int/` (5), `wiki/` (4), `sandbox/` (3), `bin/` (3) |

---

## Key Design Decisions

- **Sub-1,000 LOC rule**: All files strictly under 1,000 lines for clean isolation
- **NDA over JSON**: Binary format is canonical; JSON is import/export adapter only
- **SHA-256 Merkle integrity**: NDA security is integrity-based, not encryption-based
- **4-provider failover**: Cloudflare → OpenRouter → Azure → LocalOllama (circular)
- **rustls for TLS trust boundary**: From-scratch TLS 1.3 is an engineering artifact
- **crossbeam channels**: All inter-thread communication uses explicit channels, no shared mutable state
- **egui 0.35**: Native immediate-mode GUI, no web tech stack

---

_Generated for the Velocity Project Workspace — 2026-07-25_
