# Velocity Workspace Wiki

_Comprehensive Architecture & Technical Documentation for the Velocity Ecosystem_

This wiki provides in-depth technical documentation for all components of the Velocity codebase: a 5-crate Rust workspace (514+ source files) forming an AI-native IDE, a pure-Rust browser engine, a compiler/indexer runtime with GPU inference, a portable drone agent endpoint, and an end-to-end test harness.

---

## Architecture Guides

| Page | Coverage |
|------|----------|
| [System Overview](architecture/system_overview.md) | 5-crate dependency graph, thread model, IPC topology, data flow from user prompt to agent response, workspace state directory |
| [velocity-mcp: Agent Loop & Orchestrator](architecture/velocity_mcp.md) | 4-provider reasoning loop, provider failover chain, team routing, DAG task scheduling, worktree isolation, MediatorArena, automation watchers |
| [velocity-mcp: Editor & IDE UI](architecture/editor_ide_ui.md) | VelocityApp struct (119 files), work modes, docking, code editor, chat panel, graph view, browser panel, orchestrator panel, smart sidebar, 30+ IDE feature modules |
| [velocity-mcp: Tool Registry & Windows Automation](architecture/tool_registry_wa.md) | MCP tool dispatch, system/browser/team/WA tool categories, WA platform architecture (29 files), UIA FFI, execution synthesis, NDA artifact persistence |
| [velocity-browser: Engine & Networking](architecture/velocity_browser.md) | Slab DOM tree, flexbox/grid layout, JS VM, WASM SIMD, engine capabilities (39 files), CAPTCHA solver (14), TLS 1.3, HTTP/2/3, WebRTC, session management, parser/style subsystems |
| [velocity-ide: Compiler & SiteMap](architecture/velocity_ide.md) | Rust-to-NDA compiler pipeline, RDF triple store, string hash registry, Merkle verification, automated wiki generator, sandbox, JIT, NDA interpreter, tokenizer |

---

## Subsystem Deep-Dives

| Page | Coverage |
|------|----------|
| [Agentic Browser Subsystem](subsystems/agentic_browser.md) | AOM tree extraction, action predictor engine, outcome scorer, reflection loop, provider scorer, OCR engine, zero-alloc NDA writer, vector memory |
| [Agent Crypto & Peer Networking](subsystems/agent_crypto_peers.md) | DPAPI-backed AES-256-GCM encryption, HKDF key derivation, peer bridge, coordination bus, peer link/robust/server |
| [Agent Reasoning & Self-Improvement](subsystems/agent_reasoning_planning.md) | Tree-of-thought reasoning, multi-step planning, persistent memory, self-improvement engine, peer-to-peer system, background agents, checkpointing |
| [Browser Session Management](subsystems/browser_session_management.md) | BrowserSession, auth reseeder, cookie store, history, IndexedDB, localStorage, vector memory |
| [CAPTCHA Solver Engine](subsystems/captcha_solver_engine.md) | Visual fingerprinting, template replay, provider detection, state machine, spline shape matching, shadow matching, rule engine, temporal monitor |
| [Compiler, JIT & GPU Inference](subsystems/compiler_jit_gpu.md) | Vulkan compute pipeline, SPIR-V shaders (18), NDA JIT compiler (x86-64), BitNet ternary layers, Qwen support, GPU GEMV |
| [Connectors & Security](subsystems/connectors_security.md) | External service connectors, OAuth2, webhooks, sync rules, encrypted secret store, DPAPI-backed credential management |
| [Custom TLS 1.3 & Networking Stack](subsystems/custom_tls_net.md) | Crypto primitives (X25519, AES-GCM, ChaCha20), TLS 1.3 handshake, HTTP/2+WebSocket, QUIC, WebRTC, TLS fingerprint rotation, proxy resolver |
| [Drone Subsystem](subsystems/drone_subsystem.md) | Portable agent endpoint, HTTP server, drone identity, file transfers, task execution, cross-device collaboration |
| [Editor IDE Feature Modules](subsystems/editor_ide_features.md) | Workflow automation, plugin system, LSP client, code intelligence, voice commands, multimodal, governance, deploy pipeline |
| [IPC, Protocol & Telemetry](subsystems/ipc_protocol_telemetry.md) | 64KB shared memory IPC, native Windows events, JSON-RPC stdio, NMCP binary protocol, structured telemetry |
| [JavaScript & WASM Engine](subsystems/js_wasm_engine.md) | JS VM (ES6+), DOM bindings, Web APIs, event loop scheduler, WASM SIMD execution, Web Worker pool, event system |
| [JavaScript Interpreter & Runtime](subsystems/js_interpreter_runtime.md) | Pure-Rust ES6+ interpreter: lexer, parser, AST, evaluator (2420 LOC), DOM bridge (3403 LOC), browser env, agent empowerment layer |
| [MCP Tool Registry](subsystems/mcp_tool_registry.md) | Tool categories, dispatch flow, system/browser/team/WA tools |
| [Multi-Agent Task Orchestrator](subsystems/multi_agent_orchestrator.md) | DAG scheduling, worktree locks, team routing, expert teams, mission control, automation pipeline, instruction registry, task decomposition, model ranking |
| [Registry Browser Tools & Native Bindings](subsystems/registry_browser_tools.md) | Browser navigation, session, workflow tools, native engine bindings (DOM, click, type, screenshot, scroll, evaluate) |
| [SiteMap & NDA Binary Compiler](subsystems/sitemap_nda_compiler.md) | RDF triple store, NDA format, string hash registry, Merkle verification, wiki builder |
| [NDA Format & Security Model](subsystems/nda_security.md) | NDA binary format spec, SHA-256 Merkle integrity chain, at-rest security model, NDA vs JSON boundary, agent authoring rules |

---

## References & Developer Guides

| Page | Coverage |
|------|----------|
| [API Reference](references/api_reference.md) | Code-verified public types and function signatures for all 5 crates: AiProvider, UiToAgentMessage, VelocityApp, SiteMap, browser re-exports |
| [Build & Development Workflow](references/build_workflow.md) | Cargo workspace config, Justfile commands, pre-commit hooks, testing strategy, developer onboarding, file organization rules |

---

## Codebase Crate Map

| Crate | Files | Key Modules |
|-------|-------|-------------|
| **velocity-mcp** | 258 | `editor/` (119), `agent/` (28), `wa/` (29), `registry/` (29), `automation/` (14), `orchestrator/` (12), `connectors/` (8), `compiler/` (4), `ipc/` (4), `protocol/` (3), `security/` (2) |
| **velocity-browser** | 171 | `engine/` (39 incl. captcha/14), `net/` (19), `js/` (56 incl. interpreter/27+tests/16), `agentic/` (10), `dom/` (9), `layout/` (7), `parser/` (6), `style/` (5), root (20) |
| **velocity-ide** | 75 | `compiler/` (43: driver/12, nda_jit/9, shaders/18), `site_map/` (7), `model/` (5), `nda_int/` (5), `wiki/` (4), `sandbox/` (3) |
| **velocity-drone** | 5 | `core.rs`, `server.rs`, `safety.rs`, `lib.rs`, `main.rs` |
| **e2e** | 5 | Integration tests: browser engine, load benchmarks, MCP stdio, NDA pipeline |

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

_Generated for the Velocity Project Workspace — 2026-08-23_
