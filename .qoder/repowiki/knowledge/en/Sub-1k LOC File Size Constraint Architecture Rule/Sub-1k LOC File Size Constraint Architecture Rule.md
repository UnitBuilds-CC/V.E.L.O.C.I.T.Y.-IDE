# Sub-1k LOC File Size Constraint Architecture Rule

## Classification
- **Category**: Architecture Constraint
- **Scope**: All crates, all source files
- **Criticality**: Hard rule — enforced by convention

## Summary

Every source file in the Velocity workspace must remain under 1,000 lines of code. This is a non-negotiable architectural constraint for clean module isolation.

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

None. This rule applies to all files in all crates.

## Current Violations (as of 2026-08-17)

| File | LOC | Notes |
|------|-----|-------|
| `velocity-mcp/src/registry/system_tools.rs` | 1519 | System tool dispatch including fetch_panel_data_value, file ops, search, git, shell. Needs split into per-category handlers. |
| `velocity-mcp/src/editor/app/velocity_app/struct_def.rs` | 1135 | VelocityApp struct definition, workspace preset application, layout caching. Needs field grouping extraction. |
| `velocity-mcp/src/editor/app/velocity_app/ui_render.rs` | 2135 | Primary UI render entry point. Needs split per panel. |
| `velocity-mcp/src/editor/app/render.rs` | 1874 | App-level render orchestration. Needs split per work mode or panel group. |

### Resolved

| File | LOC | Resolution |
|------|-----|------------|
| `velocity-mcp/src/agent/executor/thread.rs` | 879 | FetchPanelData handler delegated to shared `system_tools::fetch_panel_data_value()`, `run_build` removed. Now under 1k. |
