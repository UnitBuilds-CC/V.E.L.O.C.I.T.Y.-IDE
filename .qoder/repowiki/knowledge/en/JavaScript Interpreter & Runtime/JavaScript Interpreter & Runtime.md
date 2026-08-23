# JavaScript Interpreter & Runtime

## Classification
- **Category**: Browser Engine / Runtime
- **Files**: velocity-browser/src/js/interpreter/ (27 source + 16 test = 43 files)
- **Criticality**: Critical — pure-Rust ES6+ JS execution

## Summary

Complete JavaScript runtime in pure Rust: lexer → parser → AST → evaluator pipeline, in-memory DOM bridge, browser environment APIs (timers, storage, globals), and an agent empowerment layer for LLM-optimized page access.

## Key Files

| File | Lines | Purpose |
|------|-------|---------|
| `eval.rs` | 2420 | Core evaluator: all statement/expression types |
| `dom_bridge.rs` | 3403 | In-memory DOM: document.*, Element, thread-local |
| `browser_env.rs` | 1984 | setTimeout/setInterval, window, localStorage |
| `agent_layer.rs` | 1635 | CSS selectors, interactive elements, page-to-markdown |
| `lexer.rs` | 715 | Full JS tokenizer |
| `ast.rs` | 205 | Stmt, Expr, VarKind, DestructurePattern |

## Key Design Decisions

- Pure Rust, no embedded V8/SpiderMonkey
- Thread-local DOM per interpreter (browser-like isolation)
- Agent empowerment: semantic DOM access, not raw HTML dumps
- ES6+ coverage: destructuring, for-of, for-await-of, Proxy, modules
- `using` declarations (TC39 explicit resource management)
