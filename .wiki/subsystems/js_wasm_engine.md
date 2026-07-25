# JavaScript & WASM Engine

The `js/` module within `velocity-browser` (13 files) implements a complete JavaScript virtual machine, DOM bindings, async event loop, Web Worker pool, and WASM SIMD execution — all in pure Rust.

> For full module inventory and type details, see [velocity-browser: Engine & Networking](../architecture/velocity_browser.md#js-virtual-machine--wasm).

---

## JS Virtual Machine

### Core Architecture

```rust
pub struct JsVirtualMachine { ... }
pub enum JsValue { ... }  // Dynamic JS value representation
```

**Capabilities**:
- ES6+ syntax support
- Scope chains with closure environments
- Object prototype chain
- Dynamic typing via `JsValue` enum

### Interpreter (`vm.rs`, `interpreter.rs`)

Instruction dispatch loop with:
- Lexical scoping (block, function, global)
- Closure capture and environment chains
- Prototype-based inheritance
- Error/exception handling with try/catch

---

## DOM Bindings & Web APIs

### DOM API (`dom_api.rs`)

Native Rust implementations of standard DOM interfaces:
- `Document` — createElement, querySelector, getElementById
- `Element` — getAttribute, setAttribute, classList, style
- `Node` — appendChild, removeChild, cloneNode
- `Event` — addEventListener, dispatchEvent

### Web APIs (`web_apis.rs`)

Browser platform APIs implemented in Rust:
- `fetch()` — HTTP request via the native networking stack
- `console.log/warn/error` — Output to agent console
- `setTimeout/setInterval` — Timer registration with event loop
- `localStorage/sessionStorage` — Storage API
- `navigator` — User agent, language, platform info

---

## Event Loop Scheduler

### Implementation (`event_loop.rs`)

```rust
pub struct JsEventLoopScheduler { ... }
pub struct ScheduledTask { ... }
pub enum TaskKind { ... }
```

**Two-tier queue system**:
- **Microtask queue**: Promise `.then()` callbacks, `queueMicrotask()`, mutation observers
- **Macrotask queue**: `setTimeout()`, `setInterval()`, I/O callbacks, UI rendering

**Execution model**: Process all microtasks to completion before each macrotask.

---

## WASM SIMD Execution

### Implementation (`wasm_simd.rs`)

```rust
pub struct WasmInterpreter { ... }
pub struct WasmSimdPipeline { ... }
pub struct WasmV128Vector { ... }
pub enum WasmValue { I32(i32), I64(i64), F32(f32), F64(f64), V128(WasmV128Vector) }
```

- 128-bit SIMD vector operations
- WebAssembly module loading and validation
- Host function imports for browser API access
- Vectorized math for compute-heavy workloads

---

## Web Worker Pool

### Implementation (`worker_pool.rs`)

```rust
pub struct WebWorkerPool { ... }
pub struct WorkerThread { ... }
pub enum WorkerMessage { ... }
```

- Isolated execution scope per worker
- Message-passing communication (no shared memory)
- Worker lifecycle management (spawn, terminate)

---

## Event System

### Implementation (`events.rs`, `pointer.rs`)

```rust
pub struct JsEventListener { ... }
pub struct SyntheticEventDispatcher { ... }
pub struct PointerEvent { ... }
```

- DOM event listener registration and removal
- Synthetic event dispatch (click, input, submit, etc.)
- Pointer event handling (mouse, touch, pen)
- Event bubbling and capture phase support

---

## See Also

- [velocity-browser: Engine & Networking](../architecture/velocity_browser.md) — Full browser module inventory
- [Custom TLS 1.3 & Networking](custom_tls_net.md) — Network protocol stack
