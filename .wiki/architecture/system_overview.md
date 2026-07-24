# System Overview Architecture

This document describes the high-level architecture of the Velocity workspace, detailing how its primary crates interoperate, communicate over IPC, and share state.

---

## 🏗️ Core Architecture Overview

Velocity is a high-performance, local-first AI development environment and browser engine built completely in Rust. The workspace is divided into three primary crates:

```
                  ┌─────────────────────────────────────────┐
                  │              velocity-mcp               │
                  │  (MCP Server, Egui IDE, Task Router)    │
                  └────────────┬───────────────┬────────────┘
                               │               │
            IPC / Shared Mem   │               │ Direct Rust Dependency
           (telemetry_shmem)   │               │
                               ▼               ▼
                  ┌─────────────────┐   ┌─────────────────┐
                  │ velocity-browser│   │   velocity-ide  │
                  │ (Browser Engine)│   │(SiteMap & Wiki) │
                  └─────────────────┘   └─────────────────┘
```

---

## 📦 Workspace Component Responsibilities

### 1. `velocity-browser`
- **Purpose**: Autonomous, agentic web browser and rendering engine.
- **Key Features**:
  - Custom HTML5 and CSS3 layout cascade engine.
  - Built-in QuickJS / V8 style JS interpreter with DOM bindings.
  - Zero-dependency custom TLS 1.3 stack (`net/tls13.rs`, `net/x25519.rs`, `net/aes_gcm.rs`).
  - Accessible Object Model (AOM) extractor for AI agent navigation.
  - Real-time agentic reflection and adaptive confidence scoring.

### 2. `velocity-mcp`
- **Purpose**: MCP Server interface, local LLM execution engine, and IDE application.
- **Key Features**:
  - Implements the Model Context Protocol (MCP) tool registry (`registry/`).
  - GPU-accelerated local LLM inference driver using Vulkan kernels (BitNet, Qwen layer drivers).
  - Rich egui GUI editor (`editor/`) featuring Mission Control, Task Timeline, Code Editor, Graph View, and Wiki View.
  - Windows Automation (`wa/`) engine for desktop UI automation.

### 3. `velocity-ide`
- **Purpose**: Workspace indexing, semantic code graph generation, and documentation builder.
- **Key Features**:
  - High-performance binary RDF SiteMap triple store (`site_map/`).
  - Rust source code to NDA (No-Delay Binary AST) compiler (`compiler/`).
  - Automated cross-linked Markdown wiki generator (`wiki/`).

---

## ⚡ Inter-Process Communication (IPC) & Shared Memory

`velocity-mcp` and `velocity-browser` communicate via low-latency telemetry shared memory (`telemetry_shmem.bin`), located in `.velocity/telemetry_shmem.bin`.

- **Shared Memory Struct**: `TelemetrySharedMemory` (`velocity-mcp/src/ipc/telemetry_share.rs`).
- **Ring Buffer**: Fixed 64KB atomic ring buffer used to exchange UI rendering actions, browser tab navigation events, and agent execution trace frames without heap allocations or serialisation overhead.

---

## 🔒 Data Boundaries & NDA Storage Format

Velocity uses a proprietary binary data serialization format called **NDA** (No-Delay Binary AST / Index):
- Saved under `.velocity/` (`sitemap.nda`, `changelog.nda`, `transcript.nda`).
- Ensures sub-millisecond loading times for large workspace symbol graphs and chat history.
- Isolates user code and agent execution records safely.
