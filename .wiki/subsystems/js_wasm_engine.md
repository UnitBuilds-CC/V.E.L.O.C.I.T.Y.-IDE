# JavaScript & WASM Engine

The `velocity-browser/src/js/` directory implements a standalone, fast JavaScript runtime and WebAssembly executor embedded directly inside `velocity-browser`.

---

## ⚡ Execution Architecture

```
                       ┌─────────────────────────┐
                       │     velocity-browser    │
                       └────────────┬────────────┘
                                    │
    ┌────────────────┬──────────────┼──────────────┬────────────────┐
    │                │              │              │                │
    ▼                ▼              ▼              ▼                ▼
┌───────┐      ┌──────────┐    ┌──────────┐   ┌─────────┐      ┌─────────┐
│  vm   │      │interpreter│   │  dom_api │   │event_loop│     │wasm_simd│
│(Heap &│      │(Bytecode │    │ (Native  │   │(Task    │      │(SIMD    │
│State) │      │ Execution│    │ Bindings)│   │ Scheduler)    │Exec)    │
└───────┘      └──────────┘    └──────────┘   └─────────┘      └─────────┘
```

---

## 🔧 Subsystem Components

### 1. Virtual Machine & Interpreter (`vm.rs` & `interpreter.rs`)
- **Memory Management**: Arena-allocated value heap supporting primitive types (Numbers, Strings, Booleans, Null, Undefined) and Object/Function references.
- **Execution**: Stack-based bytecode interpreter implementing ES6+ semantics (closures, scope chains (`scope.rs`), prototype chains, lexical environments).

### 2. DOM & Web API Bindings (`dom_api.rs` & `web_apis.rs`)
- Native Rust implementations exposed directly to JavaScript execution contexts:
  - `document.querySelector`, `document.getElementById`, `element.addEventListener`
  - `fetch(url, options)`, `console.log`, `console.error`
  - `setTimeout`, `setInterval`, `clearTimeout`
  - `localStorage`, `sessionStorage`

### 3. Asynchronous Event Loop (`event_loop.rs`)
- Single-threaded non-blocking event loop implementing HTML Living Standard event dispatching.
- Manages microtask queues (Promises) and macrotask queues (Timers, Network I/O, UI events).

### 4. WebAssembly SIMD Acceleration (`wasm_simd.rs`)
- WebAssembly binary decoder and execution runtime.
- Leverages 128-bit vector CPU instruction sets (SSE4.1, AVX2, NEON) for high-performance math and image processing scripts.
