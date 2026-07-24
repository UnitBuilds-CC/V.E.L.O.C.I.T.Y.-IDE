# Velocity Workspace Wiki

_Comprehensive Architecture & Technical Documentation for the Velocity Ecosystem_

Welcome to the **Velocity Workspace Wiki**. This wiki provides in-depth technical documentation covering the design, implementation, subsystems, APIs, and workflows for all components of the Velocity codebase.

---

## 🏛️ Architecture Guides

- **[System Overview](architecture/system_overview.md)** — High-level architecture, multi-crate organization, and IPC telemetry shared memory.
- **[Velocity Browser Engine](architecture/velocity_browser.md)** — Custom Rust-based browser engine (DOM, layout, custom JS VM, GPU compositor, rendering).
- **[Velocity MCP & Editor](architecture/velocity_mcp.md)** — Model Context Protocol (MCP) server, local Vulkan LLM inference kernels (BitNet/Qwen), agent orchestrator, and egui-based IDE.
- **[Velocity IDE & SiteMap Indexer](architecture/velocity_ide.md)** — RDF triple SiteMap indexer, string hash registry, and automated Markdown wiki generator.
- **[Windows Automation (WA) Platform](architecture/wa_automation.md)** — Native Windows UI automation platform, screen state capture, and action execution.

---

## ⚙️ Subsystem Documentation

- **[Agentic Browser Engine](subsystems/agentic_browser.md)** — Accessible Object Model (AOM), adaptive confidence scoring, outcome/provider scoring, and self-reflection loops.
- **[Custom TLS 1.3 & Networking Stack](subsystems/custom_tls_net.md)** — Pure Rust TLS 1.3 stack, crypto primitives (AES-GCM, ChaCha20Poly1305, X25519), HTTP/2, WebSockets, and WebRTC.
- **[JavaScript & WASM Engine](subsystems/js_wasm_engine.md)** — QuickJS-compatible JS VM, DOM bindings, async event loop, script runner, and WASM SIMD execution.
- **[MCP Tool Registry](subsystems/mcp_tool_registry.md)** — Tool registration, system tools, browser tools, team tools, and WA tool execution dispatch.
- **[Multi-Agent Task Orchestrator](subsystems/multi_agent_orchestrator.md)** — Task dependency graph (DAG), worktree directory lock manager, team routing, and expert teams.
- **[SiteMap & NDA Binary Compiler](subsystems/sitemap_nda_compiler.md)** — RDF binary triple store, NDA (No-Delay Binary AST) formats, and Rust symbol indexing.

---

## 📚 References & Developer Guides

- **[API Reference](references/api_reference.md)** — Summary of public Rust traits, core data structures, and IPC protocols across all crates.
- **[Setup & Workflow Guide](references/setup_and_workflow.md)** — Developer onboarding, environment setup, `cargo build`, testing, and debugging guidelines.

---

## 📂 Codebase Crate Map

| Crate Path | Description | Key Modules |
| :--- | :--- | :--- |
| **[`velocity-browser`](../velocity-browser/)** | High-performance modular browser engine written in Rust | `agentic`, `dom`, `engine`, `js`, `layout`, `net`, `parser`, `style` |
| **[`velocity-mcp`](../velocity-mcp/)** | MCP server, local LLM execution engine, and egui-based IDE | `agent`, `automation`, `compiler`, `editor`, `orchestrator`, `registry`, `wa` |
| **[`velocity-ide`](../velocity-ide/)** | Codebase indexer, RDF SiteMap, and automated Wiki generator | `site_map`, `wiki`, `compiler` |

---

_Generated for the Velocity Project Workspace_
