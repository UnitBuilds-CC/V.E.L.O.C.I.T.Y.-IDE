# Browser Engine Architecture

<cite>
**Referenced Files in This Document**
- [velocity-browser/src/lib.rs](file://velocity-browser/src/lib.rs)
- [velocity-browser/src/dom/mod.rs](file://velocity-browser/src/dom/mod.rs)
- [velocity-browser/src/dom/slab_tree.rs](file://velocity-browser/src/dom/slab_tree.rs)
- [velocity-browser/src/layout/mod.rs](file://velocity-browser/src/layout/mod.rs)
- [velocity-browser/src/js/mod.rs](file://velocity-browser/src/js/mod.rs)
- [velocity-browser/src/net/mod.rs](file://velocity-browser/src/net/mod.rs)
- [velocity-browser/src/engine/mod.rs](file://velocity-browser/src/engine/mod.rs)
- [velocity-browser/src/agentic/mod.rs](file://velocity-browser/src/agentic/mod.rs)
</cite>

## Overview

`velocity-browser` is a pure-Rust browser control plane with no CDP (Chrome DevTools Protocol) or Chromium dependency. It implements DOM manipulation, layout solving, JavaScript execution, and networking from scratch — totaling 171 source files.

## Subsystem Breakdown

### DOM (`src/dom/` — 9 files)
- **Slab Tree** (`slab_tree.rs`): Memory-efficient slab-allocated DOM tree
- **Shadow Slots** (`shadow_slots.rs`): Web Components shadow DOM support
- **Mutation Observer** (`mutation_observer.rs`, `mutation_batcher.rs`): DOM change notification
- **Intersection Observer** (`intersection_observer.rs`): Viewport intersection tracking
- **Custom Elements** (`custom_elements.rs`): Web component registration
- **Form** (`form.rs`): Form element handling
- **Tree** (`tree.rs`): Tree traversal and query APIs

### Layout (`src/layout/` — 7 files)
- Flexbox track solver
- CSS Grid track solver
- Parallel layout computation
- Box model calculation
- Inline layout

### JavaScript Engine (`src/js/` — 56 files)
- JavaScript VM (ES6+ subset)
- DOM bindings for JS
- Web API implementations
- Event loop scheduler
- WASM SIMD interpreter
- Web Worker pool

### Networking (`src/net/` — 19 files)
- HTTP/2 implementation
- HTTP/3 (QUIC) support
- WebSocket protocol
- WebRTC data channels
- Custom TLS 1.3 stack (engineering artifact)
- TLS fingerprint rotation
- Proxy resolver

### Engine Capabilities (`src/engine/` — 39 files)
- Session management (auth, cookies, history, storage)
- Browser workflows and workflow runner
- Snapshots and snapshot diffing
- Health monitoring
- URL helpers and wait conditions
- Checkpoint/restore
- Render reports

### Agentic Features (`src/agentic/` — 10 files)
- **AOM Tree** (`aom_tree.rs`): Accessibility Object Model for agent interaction
- **Action Predictor** (`action_predictor.rs`): Predict next user actions
- **Outcome Scorer** (`outcome_scorer.rs`): Score action outcomes
- **Reflection** (`reflection.rs`): Agent self-reflection on browser state
- **Provider Scorer** (`provider_scorer.rs`): Score provider performance
- **OCR Engine** (`ocr_map.rs`): Text extraction from rendered pages
- **NDA Encoder** (`nda_encoder.rs`): Encode browser state to NDA format
- **Zero-Alloc Writer** (`zero_alloc_writer.rs`): Allocation-free NDA writes
- **Adaptive Confidence** (`adaptive_confidence.rs`): Dynamic confidence thresholds

## Session Management

Sessions are managed through dedicated modules:
- `session.rs` — Core session lifecycle
- `session_auth.rs` — Authentication state
- `session_cookie_store.rs` — Cookie jar
- `session_history.rs` — Navigation history
- `session_storage.rs` — Web storage API
- `session_storage_events.rs` — Storage event dispatch
- `session_storage_quota.rs` — Quota enforcement
- `session_indexeddb.rs` — IndexedDB emulation
- `session_swarm.rs` — Multi-session coordination

## Other Subsystems

- **Parser** (`src/parser/` — 7 files): HTML parser
- **Style** (`src/style/` — 5 files): CSS style resolution
- **Screencast** (`src/screencast.rs`): Frame sequence recording
- **Vector Memory** (`src/vector_memory.rs`): Spatial AOM site vector store

**Section sources**
- [velocity-browser/src/lib.rs](file://velocity-browser/src/lib.rs)
- [velocity-browser/Cargo.toml](file://velocity-browser/Cargo.toml)
