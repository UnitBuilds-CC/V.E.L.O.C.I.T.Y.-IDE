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
| `velocity-mcp/src/agent/executor/thread.rs` | 1041 | Agent thread entry, API key resolution, reasoning loop spawn, FetchPanelData handler with run_build action. Needs split. |
