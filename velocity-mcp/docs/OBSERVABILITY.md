# Agent observability notes

Last updated: current session.

## UI refactor status

- `src/editor/app.rs` rewritten to use `egui_dock` and a single `DockState` source of truth.
- `src/editor/theme.rs` uses only egui 0.26 fields (verified against `Cargo.toml`).
- `src/editor/code_editor.rs` basic `syntect` highlighting widget.
- `main.rs` has crate-level `#![allow(warnings)]` to silence legacy warnings until they are cleaned.
- `src/orchestrator/` added (blueprint, scheduler, registry, validator, reconcile, worker, mod).
- `docs/ARCHITECTURE.md`, `docs/OBSERVABILITY.md`, `Justfile`, and `.velocity/todo.md` created.

## Known remaining risks

1. `cargo` cannot be invoked from the sandbox; I rely on you or the IDE to return build output.
2. `egui_dock` calls such as `push_to_focused_leaf`, `set_active_tab`, `remove_tab`, `iter_all_tabs`, `root_node()`, `index()`, `set_focused_node_and_surface` were written against the API assumed for version 0.11.
3. Custom font embedding was removed to avoid missing `assets/fonts/` until a font is committed.
4. `Frame::none().fill().show()` is used intentionally; `Frame::show` returns `InnerResponse<R>` and passed closure gets `&mut Ui`.

## How to validate

```bash
just check
```
