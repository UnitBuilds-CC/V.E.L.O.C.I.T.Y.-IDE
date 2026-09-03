# Sub-1k LOC File Size Constraint Architecture Rule

## Classification
- **Category**: Architecture Constraint
- **Scope**: All crates, all source files
- **Criticality**: Design guideline / target — not universally met

## Summary

Most source files in the Velocity workspace are kept under 1,000 lines of code as a design target for clean module isolation. This is a guideline, not a hard constraint: a number of larger modules currently exceed it and are tracked for incremental refactoring.

## Rationale

- Prevents god-files with multiple responsibilities
- Makes code review manageable
- Enforces single-responsibility principle
- Keeps compilation units small for faster incremental builds

## Enforcement

- Checked during code review
- When a file approaches 1,000 LOC:
  1. Extract helper functions into sibling files
  2. Split into submodules (mod.rs + children)
  3. Move types into dedicated types.rs

## Exceptions

This is a target, not an enforced hard limit. Larger modules — panel rendering, the JS DOM bridge, the transformer model, and the NDA parsers — legitimately exceed 1,000 LOC and are tracked for refactoring.

## Current Status (as of 2026-09-02)

79 of 554 `.rs` files exceed 1,000 LOC. The largest offenders:

| File | LOC | Notes |
|------|-----|-------|
| `velocity-mcp/src/editor/app/velocity_app/tier3_panels.rs` | 4339 | Panel rendering. Needs split per panel. |
| `velocity-ide/src/pipeline_bridge.rs` | 3781 | Pipeline bridge. Needs decomposition. |
| `velocity-browser/src/js/interpreter/dom_bridge.rs` | 3310 | JS DOM bridge. Needs split per DOM API group. |
| `velocity-ide/src/model/transformer.rs` | 3260 | Transformer inference. Needs split per layer/stage. |
| `velocity-mcp/src/editor/app/velocity_app/ui_render.rs` | 2382 | Primary UI render entry point. Needs split per panel. |

### Resolved

| File | LOC | Resolution |
|------|-----|------------|
| `velocity-mcp/src/agent/executor/thread.rs` | 879 | FetchPanelData handler delegated to shared `system_tools::fetch_panel_data_value()`, `run_build` removed. Now under 1k. |
