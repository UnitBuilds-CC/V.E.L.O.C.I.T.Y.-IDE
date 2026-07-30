# Directive: Make the JS Engine Agent-First (Not Just Spec-Conformant)

**Status:** Open — pick up alongside the current scoping/conformance work.
**Scope:** `velocity-browser/src/js/*`, `src/engine/trace.rs`, `src/session.rs`, `src/agent_api.rs`

## Context

The interpreter header calls itself "the agent-first JS surface", but today
"agent-first" only means *pragmatic subset* — enough JS to run real pages.
The engine has no agent-facing observability: errors are opaque strings that
get swallowed, `console.*` output is never captured, and intentional spec
deviations happen silently. The rest of the crate holds a higher bar —
`agent_api.rs` states "an action is never reported without its observation."
The JS engine must meet that same bar: **when page JS breaks or behaves
unexpectedly, the agent must be able to see what happened as readable facts,
never a mysteriously dead page.**

Review findings this directive is bound to:

1. **Opaque, swallowed errors.** Everything returns `Result<JsValue, String>`
   with no line/column/kind. `script_runner.rs::execute_single_script` logs
   and continues; `vm.rs::eval_statement` classifies parse errors by
   string-matching (`e.contains("unexpected token")`) and converts them to
   `Ok(JsValue::Undefined)`. Agents cannot distinguish "ran fine" from
   "silently failed to parse."
2. **`console.log` not captured.** `TraceCollector::console_traces` exists,
   but the interpreter's `console.*` natives never forward into it. Only
   manual engine-side `record_console()` calls appear.
3. **Silent spec deviations.** `assign_to_target` in `interpreter.rs`
   silently ignores `const` reassignment ("or could throw TypeError"), and a
   test codifies the silent-ignore. Silent divergence + zero observability is
   the worst combination for a debugging agent.
4. **No introspection surface.** `Scope` is already a `HashMap<String,
   JsValue>` chain and the event loop already tracks pending tasks, but none
   of it is exposed through `session`/agent APIs.

## Required Changes

### 1. Structured JS errors (do this FIRST — cheap now, expensive later)

Replace `String` errors with a structured type, threaded through lexer →
parser → evaluator while the interpreter is already being reworked:

```rust
pub struct JsError {
    pub kind: JsErrorKind,   // SyntaxError | TypeError | ReferenceError | RangeError | Thrown
    pub message: String,
    pub line: u32,           // 1-based, from token position
    pub col: u32,
}
```

- Lexer tokens must carry line/col (count during `lex()`).
- Parser errors become `JsError { kind: SyntaxError, .. }` — this removes the
  fragile `e.contains("unexpected token")` classification in `vm.rs`.
- `Signal::Throw` carries the thrown `JsValue`; convert to `JsError` with the
  location of the throw site at the `eval_script` boundary.
- Public boundary (`vm.rs`, `interpreter::eval_script`) returns
  `Result<JsValue, JsError>`. Keep a `Display` impl so existing string
  formatting call sites need minimal churn.

### 2. Wire `console.*` into `TraceCollector` (highest value-per-line fix)

When the interpreter dispatches `console.log/warn/error/info/debug` natives,
the formatted output must land in `TraceCollector::console_traces` with the
correct level. The interpreter has no `TraceCollector` reference today —
acceptable mechanisms: a scoped thread-local sink drained by the caller, or a
console-output buffer on the VM that `script_runner`/`session` flush into the
trace after each eval. Prefer whichever stays zero-alloc on the hot path.

**Acceptance:** a page running `console.error("boom")` produces a
`ConsoleTraceRecord { level: "error", message: "boom", .. }` retrievable via
the session, and `session.eval_js("console.log(1)")` does too.

### 3. Never diverge from spec silently

- `const` reassignment must throw `TypeError` per spec (real page code relies
  on catching it). Update `assign_to_target` to return an `EvalResult` so it
  can propagate `Signal::Throw`, and **rewrite the `const_immutable` test** to
  assert the throw instead of codifying silent-ignore.
- Anywhere the engine intentionally deviates or degrades (unsupported
  feature, skipped script type, graceful parse-failure), emit a
  `TraceCollector` record (`level: "warn"`, message naming the deviation and
  source location). Silent behavioral divergence is forbidden.

### 4. Surface JS failures in the agent observation path

- Script errors from page execution must be queryable, not just buried in an
  internal log: keep a per-page `Vec<JsError>` on the session (capped, like
  trace buffers) and expose it alongside `AgentActionResult` /
  console traces, so "what JS broke on this page?" is one call.
- Add a cheap scope snapshot API: `Scope::snapshot(scope) ->
  Vec<(String, String)>` (name, short value preview) walking the chain, and
  expose page globals through the session for agent inspection.
- Expose pending event-loop state (timer/microtask counts, next delays) from
  `JsEventLoopScheduler` through the session — agents need "is this page
  still going to do something?" to know when to re-observe.

## Non-Goals

- No spec-completeness push beyond what conformance work already covers.
- No debugger/breakpoint machinery — observation, not interactive stepping.
- Do not regress the zero-alloc philosophy on hot paths for tracing.

## Validation

`just validate` from `velocity-mcp/` must pass. Add tests for: parse error
carries line/col; `const` reassignment throws TypeError catchable by page
`try/catch`; `console.*` output reaches `TraceCollector`; script error
surfaces on the session after `execute_page_scripts`.
