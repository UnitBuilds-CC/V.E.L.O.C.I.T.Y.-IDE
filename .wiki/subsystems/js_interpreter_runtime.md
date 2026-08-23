# JavaScript Interpreter & Runtime

_Pure-Rust ES6+ JavaScript interpreter with DOM bridge, browser environment, agent empowerment layer, and full web platform APIs._

---

## Overview

The `velocity-browser/src/js/interpreter/` subsystem is a complete JavaScript runtime implemented entirely in Rust. It provides a lexer, parser, AST, evaluator, DOM bridge, browser environment APIs, and agent-specific empowerment primitives — all without embedding an external JS engine.

Total: **27 source files + 16 test files** (43 files).

---

## Module Structure

```
js/interpreter/
├── mod.rs              # Public API re-exports (lex, eval, call_function, property ops)
├── lexer.rs            # Tokenizer: source text → Vec<Token>
├── token.rs            # Token enum (all JS operators, keywords, literals)
├── ast.rs              # AST nodes: Stmt, Expr, VarKind, DestructurePattern
├── parser.rs           # Recursive-descent parser: Vec<Token> → Vec<Stmt>
├── eval.rs             # Core evaluator: eval_program, eval_stmt, eval_expr_node (2420 lines)
├── eval_script.rs      # Entry points: eval_script, eval_expr
├── function.rs         # Function creation, call_function, call_function_with_this, parseInt, parseFloat
├── method_dispatch.rs  # call_method: dynamic method resolution on JsValue
├── property.rs         # Property access: get/set/has/delete/ownKeys/enumerableKeys/ownPropertyNames
├── coercion.rs         # JS type coercion: to_string, to_number, to_boolean, typeof_str
├── collections.rs      # Array/Object built-in methods
├── constructors.rs     # eval_new: constructor invocation (new Date(), new Map(), etc.)
├── core_methods.rs     # Core built-in method implementations
├── native.rs           # call_native, JSON.parse/stringify, encodeURI/decodeURI
├── dom_bridge.rs       # In-memory DOM: document.*, Element, thread-local DOM_NODES (3403 lines)
├── browser_env.rs      # setTimeout/setInterval, window/navigator/location, localStorage (1984 lines)
├── agent_layer.rs      # Agent empowerment: selectors, interactive elements, page summary, DOM diff (1635 lines)
├── web_apis.rs         # Web API implementations (fetch, XMLHttpRequest subset, etc.)
├── web_apis2.rs        # Additional web APIs
├── web_platform.rs     # Web platform primitives
├── module.rs           # ES module import/export: apply_import
├── intl.rs             # Internationalization: Intl.DateTimeFormat, NumberFormat
├── streams.rs          # ReadableStream/WritableStream primitives
├── signal.rs           # Signal/ reactive primitives
├── canvas.rs           # Canvas 2D drawing API stubs
├── console.rs          # console.log/warn/error/debug
└── tests/              # 16 test files
```

---

## Core Pipeline

### Lexer → Parser → AST → Evaluator

```
Source text
    │
    ▼ lex()
Vec<Token>
    │
    ▼ parse()
Vec<Stmt>  (AST)
    │
    ▼ eval_program() / eval_expr_node()
JsValue
```

### Token Types

Full JavaScript token set: arithmetic, comparison, logical, assignment, bitwise, string/template literals, regex, optional chaining (`?.`), nullish coalescing (`??`), spread (`...`), arrow (`=>`), async/await keywords.

### AST Nodes

```rust
pub enum VarKind { Var, Let, Const, Using }

pub enum Stmt {
    Expr(Expr),
    VarDecl { kind, name, init },
    DestructureDecl { pattern, init },  // Object + Array destructuring
    Block(Vec<Stmt>),
    If { cond, then_branch, else_branch },
    While { cond, body },
    DoWhile { body, cond },
    For { init, cond, update, body },
    ForIn { var_name, object, body },
    ForOf { var_name, object, body },
    ForAwaitOf { var_name, object, body },  // async iteration
    Return(Option<Expr>),
    // ... break, continue, throw, try/catch/finally, switch, labeled
}
```

### Evaluator (`eval.rs`, 2420 lines)

The core evaluator handles:
- All statement types with proper scoping (block-scope for let/const)
- Destructuring patterns (object + array)
- Proxy traps with depth limiting (`MAX_PROXY_TRAP_DEPTH = 8`)
- Promise capture via thread-local `PROMISE_CAPTURE`
- Explicit resource management (`using` declarations with `Scope::add_disposable`)

---

## DOM Bridge (`dom_bridge.rs`, 3403 lines)

A complete in-memory DOM implementation backed by thread-local storage:

```rust
thread_local! {
    static DOM_NODES: RefCell<Vec<DomNode>> = ...;
    static DOM_ROOT: RefCell<Option<usize>> = ...;
    static FOCUSED_NODE: Cell<Option<usize>> = ...;
}

struct DomNode {
    tag: String,
    attributes: HashMap<String, String>,
    children: Vec<usize>,     // indices into DOM_NODES
    parent: Option<usize>,
    text_content: String,
    node_type: u8,            // 1=Element, 3=Text, 11=Fragment
    event_listeners: HashMap<String, Vec<JsValue>>,
}
```

Provides `document.*` methods: `createElement`, `getElementById`, `querySelector`, `querySelectorAll`, `addEventListener`, `appendChild`, `innerHTML`, `textContent`, `style`, `classList`, `focus()`, and more.

Each interpreter instance gets its own isolated DOM — matching browser per-origin isolation.

---

## Browser Environment (`browser_env.rs`, 1984 lines)

Thread-local browser APIs:
- **Timers**: `setTimeout`, `setInterval`, `clearTimeout`, `clearInterval` with `flush_timers()` pump
- **Globals**: `window`, `navigator`, `location`, `document`
- **Storage**: `localStorage`, `sessionStorage` (in-memory, per-interpreter)
- **Network gate**: `set_network_enabled(false)` disables fetch/XHR

---

## Agent Empowerment Layer (`agent_layer.rs`, 1635 lines)

Zero-allocation primitives that turn the DOM into an LLM superpower:

| Capability | Description |
|-----------|-------------|
| **Selector generation** | Unique CSS selectors for any DOM node (`#id`, `tag[attr=val]`, `:nth-child`) |
| **Interactive elements** | Buttons, inputs, links → compact NDA-ready list |
| **Content extraction** | Strip nav/footer/ads → main text only |
| **Page summary** | Title, headings, stats in a few hundred bytes |
| **DOM diff** | Snapshot comparison for wait-for-settlement detection |
| **Table extraction** | Tables → structured headers + rows |
| **Page-to-Markdown** | Densest page representation for LLM consumption |
| **Bulk form fill** | One call fills N fields |
| **Link map** | Deduplicated navigation targets |

Exported via `export_agent_state_nda()` — produces a compact NDA triple representation of the page state.

---

## Key Design Decisions

- **Pure Rust, no embedded engine**: Full control over execution, no V8/SpiderMonkey dependency
- **Thread-local DOM**: Per-interpreter isolation without locks, matches browser origin model
- **Agent-first**: The agent_layer provides semantic DOM access optimized for LLM consumption, not raw HTML dumps
- **ES6+ coverage**: Destructuring, for-of, for-await-of, async/await, Proxy, modules, Intl
- **`using` declarations**: Explicit resource management (TC39 stage 3) with disposable tracking

---

## See Also

- [JS & WASM Engine](js_wasm_engine.md) — WASM SIMD execution and Web Worker pool
- [Agentic Browser Subsystem](agentic_browser.md) — AOM tree extraction, action predictor
- [Browser Engine & Networking](../architecture/velocity_browser.md) — Slab DOM tree, flexbox layout, TLS
