# Rust Code Review

## Description
Review Rust code changes in the Velocity workspace for correctness, safety, performance, and adherence to project conventions. Use when reviewing pull requests, agent-generated code, or before committing changes to critical modules.

## When to Use
- Reviewing a PR or code change set
- Evaluating agent-generated code quality
- Before merging changes to high-risk areas (agent dispatch, DOM, NDA format, editor app)
- When checking if a new module follows project conventions

## Review Checklist

### 1. Project Conventions
- [ ] File is under 1,000 LOC (hard rule)
- [ ] Code is formatted with `cargo fmt`
- [ ] No clippy warnings (`cargo clippy -- -D warnings`)
- [ ] Public items have `///` doc comments
- [ ] Module has `//!` module-level docs
- [ ] Naming follows Rust conventions (snake_case, PascalCase, UPPER_SNAKE_CASE)

### 2. Safety and Correctness
- [ ] No `unsafe` blocks without clear justification and safety comment
- [ ] No `unwrap()` or `expect()` in library code (use proper error handling)
- [ ] No panicking in hot paths
- [ ] Error types are specific and informative
- [ ] Resource cleanup is handled (Drop, RAII)

### 3. Architecture Compliance
- [ ] Changes respect crate boundaries (MCP → Browser → IDE, no reverse deps)
- [ ] NDA format changes include migration logic
- [ ] No shared mutable state (use crossbeam channels)
- [ ] New tools are registered in the tool registry
- [ ] New providers implement the provider trait

### 4. Performance
- [ ] No unnecessary allocations in hot paths
- [ ] Buffers are reused where possible
- [ ] No blocking calls on the main thread
- [ ] GPU operations use proper synchronization

### 5. Testing
- [ ] New behavior has behavior tests (not just smoke tests)
- [ ] Tests verify monotonicity relationships where applicable
- [ ] Tests verify determinism (same input → same output)
- [ ] Edge cases covered (empty input, max enum, zero state)
- [ ] Test names describe what they test

### 6. High-Risk Area Checks

#### Provider Dispatch (`agent/executor/`)
- [ ] Failover chain is preserved (Cloudflare → OpenRouter → Azure → Ollama)
- [ ] Error handling doesn't silently drop failures
- [ ] Token counting is accurate

#### DOM / Layout (`velocity-browser/src/dom/`, `layout/`)
- [ ] Slab tree invariants maintained
- [ ] Mutation batching is correct
- [ ] Layout solver terminates

#### NDA Format (`nda.rs`, `nda_int/`, `protocol/nmcp_binary.rs`)
- [ ] 18-byte record layout preserved
- [ ] Merkle chain updated on writes
- [ ] String hash registry consistency
- [ ] Backward compatibility maintained

#### Editor App (`editor/app/velocity_app/struct_def.rs`)
- [ ] VelocityApp struct changes don't break panel rendering
- [ ] State initialization is complete
- [ ] Work mode transitions are handled

## Severity Levels

| Level | Meaning | Action |
|-------|---------|--------|
| **Critical** | Correctness bug, data loss, security issue | Must fix before merge |
| **High** | Architecture violation, performance regression | Must fix before merge |
| **Medium** | Missing tests, poor error handling, convention violation | Should fix before merge |
| **Low** | Style, naming, documentation gaps | Can fix in follow-up |

## Output Format

```
## Code Review: <module/file>

### Summary
<1-2 sentence overview>

### Issues Found

#### [Critical/High/Medium/Low] <title>
- **File**: `path/to/file.rs:L42`
- **Issue**: <description>
- **Fix**: <suggested fix>

### Verdict
<PASS / PASS WITH COMMENTS / REQUEST CHANGES>
```
