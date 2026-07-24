# Velocity Browser Architecture

The `velocity-browser` crate is a lightweight, zero-dependency, agent-first browser engine implemented in pure Rust. It is engineered specifically for speed, deterministic automation, security, and native AI integration.

---

## 🏛️ Subsystem Architecture

```
                       ┌─────────────────────────┐
                       │     velocity-browser    │
                       └────────────┬────────────┘
                                    │
    ┌────────────────┬──────────────┼──────────────┬────────────────┐
    │                │              │              │                │
    ▼                ▼              ▼              ▼                ▼
┌───────┐      ┌──────────┐    ┌──────────┐   ┌─────────┐      ┌─────────┐
│  parser│      │    dom   │    │  layout  │   │   js    │      │   net   │
│ (HTML/│      │ (Tree &  │    │  (Style &│   │ (VM &   │      │(TLS 1.3/│
│  CSS) │      │ Observer)│    │ Flow Box)│   │ WebAPIs)│      │  HTTP2) │
└───────┘      └──────────┘    └──────────┘   └─────────┘      └─────────┘
```

---

## 🔧 Component Overview

### 1. Parser (`src/parser/`)
- **`html.rs`**: Tokenizer and tree constructor implementing HTML5 parsing rules. Handles malformed markups, auto-closing tags, and script injection protection.
- **`css.rs`**: Fast CSS selector parser supporting class, ID, element, pseudo-class, and combined selectors into computed style trees.

### 2. Document Object Model (`src/dom/`)
- **`tree.rs`**: Arena-allocated DOM tree structure optimizing node traversal and parent/sibling indexing.
- **`mutation_observer.rs` & `mutation_batcher.rs`**: Fine-grained DOM mutation event tracking that batches DOM changes into atomic payloads for AI reflection engines.
- **`custom_elements.rs` & `intersection_observer.rs`**: Support for Web Components and element visibility monitoring.

### 3. Layout & Styling (`src/layout/` & `src/style/`)
- **`style/cascade.rs`**: Implements CSS cascade rules, specificity calculations, and property inheritance.
- **`layout/engine.rs`**: Layout engine that computes element dimensions, positions, flexbox layouts, grid formatting contexts, and text wrapping.

### 4. JavaScript & WASM Engine (`src/js/`)
- **`vm.rs` & `interpreter.rs`**: Lightweight JS virtual machine supporting ES6+ syntax, scope chains, closure environments, and object prototypes.
- **`dom_api.rs` & `web_apis.rs`**: Native Rust implementations of browser APIs (`fetch`, `console`, `setTimeout`, `Document`, `Element`).
- **`event_loop.rs`**: Microtask and macrotask queue scheduler for asynchronous operations.
- **`wasm_simd.rs`**: Vectorized execution engine for WebAssembly modules.

### 5. Custom Networking & Security (`src/net/`)
- **`tls13.rs`, `tls_handshake.rs`, `tls_record.rs`**: Custom implementation of TLS 1.3 in Rust.
- **`x25519.rs`, `aes_gcm.rs`, `chacha20poly1305.rs`**: Pure Rust cryptographic primitives avoiding external C/C++ openssl dependencies.
- **`http2_ws.rs`, `webrtc.rs`, `bluetooth.rs`**: Protocol stack supporting HTTP/2 multiplexing, WebSockets, WebRTC data channels, and Web Bluetooth.

### 6. Engine Capabilities & GPU Compositor (`src/engine/`)
- **`gpu_compositor.rs` & `webgpu.rs`**: GPU rendering pipeline for hardware-accelerated composite layers.
- **`canvas.rs` & `canvas_context.rs`**: 2D HTML5 canvas rendering context implementation.
- **`geolocation.rs`, `push_notifications.rs`, `service_worker.rs`**: Modern browser platform APIs.

---

## 🤖 Agentic Browser Integration

Located in `src/agentic/`:
- **`aom_tree.rs`**: Converts raw DOM into an Accessible Object Model (AOM) optimized for LLM token efficiency.
- **`action_predictor.rs`**: Evaluates interactive page elements and predicts next-step user/agent actions.
- **`outcome_scorer.rs` & `reflection.rs`**: Scores interaction results to inform agent retry loops.
