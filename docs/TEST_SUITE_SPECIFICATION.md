# Velocity Test Suite Specification

> **Status**: Active Implementation  
> **Created**: August 25, 2026  
> **Target**: ~1,275 tests across 6 crates  
> **Coverage Goal**: ~65% line coverage, 100% security boundary coverage

---

## Executive Summary

This specification defines the comprehensive test suite for the Velocity codebase (~313k LOC). The test suite follows a pyramid strategy: unit tests at the base, component and contract tests in the middle, and E2E/GUI regression tests at the top.

**Key Principles:**
- Test behavior and durable outcomes, not implementation details
- Use deterministic fakes — no production credentials or public network in CI
- Every tool is not "wired" until definition, dispatch, permission, valid call, and invalid call are covered
- Each fixed defect receives a regression test at the lowest layer that could have caught it

---

## Test Categories

### 1. Unit Tests (~700 tests)
**Scope**: Pure functions, parsing, state transitions, policies, layout calculations  
**Characteristics**: Deterministic, fast (<10ms each), no I/O  
**Command**: `cargo test -p <crate> --lib`

### 2. Component Tests (~250 tests)
**Scope**: One subsystem with filesystem/process/network seams replaced by fixtures  
**Characteristics**: Tests persistence round trips, error propagation, cancellation, event ordering  
**Command**: `cargo test -p <crate> --test <suite>`

### 3. Contract Tests (~100 tests)
**Scope**: Public MCP tools and JSON-RPC schemas  
**Characteristics**: Every advertised tool has an executable contract fixture  
**Command**: `cargo test -p velocity-e2e --test mcp_stdio`

### 4. Integration Tests (~100 tests)
**Scope**: Multi-module workflows, cross-crate interactions  
**Characteristics**: Tests expert teams lifecycle, task orchestration, browser workflows  
**Command**: `cargo test -p velocity-e2e`

### 5. E2E Tests (~50 tests)
**Scope**: Native binaries and primary workflows  
**Characteristics**: Exit status, durable artifacts, observable output  
**Command**: `cargo test -p velocity-e2e`

### 6. GUI Regression Tests (~25 tests)
**Scope**: egui state and desktop smoke runs  
**Characteristics**: Navigation/action state, layout persistence, screenshot baselines  
**Command**: Targeted app tests + Windows desktop smoke job

### 7. Security/Fuzz Tests (~50 tests)
**Scope**: Path traversal, malformed input, sandbox escape, fuzzing  
**Characteristics**: Negative tests, corpus-based, opt-in for expensive runs  
**Command**: `cargo test --workspace --features security-tests`

---

## Coverage Matrix by Crate

### velocity-mcp (258 files, ~200k LOC)

| Module | Unit | Component | Contract | Integration | E2E | GUI |
|--------|------|-----------|----------|-------------|-----|-----|
| Agent loop (provider failover) | 20 | 10 | - | 5 | - | - |
| Tool registry (100+ tools) | 50 | 30 | 100 | - | - | - |
| Expert teams | 30 | 15 | - | 10 | 2 | 5 |
| Task orchestrator | 25 | 10 | - | 8 | 2 | - |
| JSON-RPC protocol | 10 | 5 | 20 | - | 5 | - |
| Windows Automation | 15 | 10 | - | 5 | - | 3 |
| IDE state (egui) | 20 | 10 | - | 2 | - | 12 |
| Connectors/IPC/security | 30 | 10 | - | 5 | - | - |
| **Total** | **200** | **100** | **120** | **35** | **9** | **20** |

### velocity-browser (171 files, ~80k LOC)

| Module | Unit | Component | Integration | E2E | Perf |
|--------|------|-----------|-------------|-----|------|
| DOM (slab tree, mutations) | 50 | 15 | 10 | 5 | - |
| JS interpreter (ES6+) | 150 | 20 | 10 | 10 | - |
| Layout (flexbox, grid) | 30 | 15 | 5 | 5 | 3 |
| Engine capabilities (39) | 40 | 10 | 10 | 10 | 5 |
| CAPTCHA solver (14) | 20 | 5 | - | 5 | - |
| Networking (TLS, HTTP) | 10 | 5 | 5 | 5* | - |
| Session management | 20 | 10 | 5 | 5 | - |
| Agentic features | 30 | 10 | 5 | 5 | - |
| **Total** | **350** | **90** | **50** | **50** | **8** |

*Networking tests with real network are opt-in

### velocity-ide (75 files, ~20k LOC)

| Module | Unit | Component | Integration | E2E | Security |
|--------|------|-----------|-------------|-----|----------|
| NDA compiler (lexer/parser) | 50 | 10 | 5 | 5 | 10 |
| JIT compiler | 20 | 5 | - | 2 | 3 |
| RDF triple store/SiteMap | 15 | 5 | 3 | 3 | - |
| Sandbox | 10 | 5 | 2 | 3 | 10 |
| Wiki generator | 5 | 2 | - | 1 | - |
| **Total** | **100** | **27** | **10** | **14** | **23** |

### drone (5 files, ~1.5k LOC)

| Module | Unit | Integration | E2E |
|--------|------|-------------|-----|
| HTTP server | 10 | 8 | 2 |
| File transfers | 5 | 4 | 2 |
| Task execution | 5 | 3 | 1 |
| Peer networking | 5 | 3 | 2 |
| **Total** | **25** | **18** | **7** |

### velocity-router (~16k LOC)

| Module | Unit | Integration | Load |
|--------|------|-------------|------|
| Task decomposition | 15 | 10 | 2 |
| Domain router | 15 | 10 | 2 |
| Parallel dispatcher | 10 | 8 | 3 |
| Circuit breaker | 10 | 5 | - |
| Persistence/ledger | 10 | 8 | - |
| API endpoints | 15 | 10 | 2 |
| **Total** | **75** | **51** | **9** |

---

## Implementation Phases

### Phase 1: Foundation (Week 1-2) — P0

**Goal**: Replace permissive smoke tests with deterministic contract tests

**Deliverables:**
1. **Test harness infrastructure**
   - `test_harness/` module with JSON-RPC process helpers
   - Fixture workspace factory (`tempfile`-based)
   - Fake providers (deterministic responses for all 4 AI providers)
   - Test utilities for common patterns

2. **MCP tool registry parity tests** (~100 tests)
   - For each tool: definition, dispatch, permission, valid call, invalid call
   - Cover: system tools, browser tools, team tools, WA tools

3. **Provider failover contract tests** (~20 tests)
   - Deterministic fakes for each provider
   - Failover chain, timeout, retry logic
   - Circular failover (Cloudflare → OpenRouter → Azure → LocalOllama)

4. **NDA compiler unit tests** (~100 tests)
   - Lexer diagnostics
   - Parser round trips
   - Sandbox escape attempts

**Exit Criteria:**
- All harness infrastructure merged
- 220+ new tests passing
- CI gates updated to run new tests

### Phase 2: Critical Workflows (Week 3-4) — P0

**Goal**: Test multi-module workflows end-to-end

**Deliverables:**
1. **Expert teams lifecycle** (~30 tests)
   - Create → route → edit → clone → import/export
   - Routing decisions (file scope, keyword, LLM router, lead fallback)
   - Validation (scope overlap, composition, health checks)

2. **Task orchestrator contracts** (~20 tests)
   - DAG scheduling with dependencies
   - Worktree isolation
   - Mission plan-review-execute lifecycle
   - Cancellation and timeout

3. **Browser engine integration** (~40 tests)
   - HTML → DOM → accessible summary
   - Form/input action → event/mutation trace
   - Storage/history persistence
   - Local fixture pages

4. **Velocity router integration** (~30 tests)
   - API endpoints with test database
   - Task decomposition → routing → dispatch
   - Circuit breaker state transitions

**Exit Criteria:**
- 120+ new tests passing
- Critical workflows have contract coverage
- E2E suite runs in <5 minutes

### Phase 3: GUI Confidence (Week 5) — P1

**Goal**: Deterministic egui state tests + minimal Windows visual smoke

**Deliverables:**
1. **IDE state tests** (~20 tests)
   - Command palette actions
   - Build/Mission layout selection
   - Panel focus/toggle
   - Sidebar state persistence
   - Team Studio create/assign

2. **Windows desktop smoke** (opt-in, ~5 tests)
   - Startup, layout switching, command palette
   - Screenshot baselines (deterministic rendering only)

**Exit Criteria:**
- 25+ GUI tests passing
- State tests run in CI without rendering
- Visual smoke tests documented for manual execution

### Phase 4: Hardening (Week 6-7) — P1

**Goal**: Negative tests, fuzzing, security

**Deliverables:**
1. **Security tests** (~30 tests)
   - Path traversal and symlink escape
   - Malformed RPC/NDA frames
   - Secret redaction
   - Process cleanup, crash recovery

2. **Fuzz/corpus tests** (~20 tests)
   - Parsers (HTML, CSS, JS, NDA)
   - RPC frame parsing
   - JIT compiler input

3. **Cancellation/timeout tests** (~15 tests)
   - Provider requests
   - Task execution
   - Browser navigation

**Exit Criteria:**
- 65+ security/fuzz tests passing
- No critical vulnerabilities in security test report
- Fuzz corpus committed to repo

### Phase 5: Performance & Coverage (Week 8+) — P2

**Goal**: Benchmarks, coverage tracking, mutation testing

**Deliverables:**
1. **Performance benchmarks** (~10 tests)
   - Browser rendering (layout, paint)
   - NDA compiler throughput
   - Router dispatch latency

2. **Coverage tracking**
   - Per-crate coverage reports in CI
   - Flaky-test quarantine mechanism
   - Nightly full-suite runs

3. **Mutation testing** (high-risk modules)
   - Provider failover logic
   - Task routing decisions
   - Security boundaries

**Exit Criteria:**
- Coverage reports generated in CI
- Baseline performance metrics established
- Mutation testing running on critical modules

---

## Test Infrastructure

### Fixtures & Fakes

**Fake Providers** (`test_harness/src/fake_providers.rs`):
```rust
pub struct FakeProvider {
    pub name: AiProvider,
    pub responses: Vec<FakeResponse>,
    pub fail_after: Option<usize>,
    pub latency_ms: Option<u64>,
}

pub fn create_fake_cloudflare() -> FakeProvider { ... }
pub fn create_fake_openrouter() -> FakeProvider { ... }
pub fn create_fake_azure() -> FakeProvider { ... }
pub fn create_fake_ollama() -> FakeProvider { ... }
```

**Fixture Workspace** (`test_harness/src/workspace.rs`):
```rust
pub struct FixtureWorkspace {
    pub root: TempDir,
    pub files: Vec<PathBuf>,
}

pub fn create_fixture_workspace(files: &[(&str, &str)]) -> FixtureWorkspace { ... }
```

**HTML Fixtures** (`test_harness/src/html_fixtures.rs`):
```rust
pub fn simple_page() -> String { ... }
pub fn form_page() -> String { ... }
pub fn interactive_page() -> String { ... }
```

### Test Harnesses

**JSON-RPC Process Helper** (`test_harness/src/jsonrpc.rs`):
```rust
pub struct JsonRpcClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl JsonRpcClient {
    pub fn spawn_mcp_server() -> Self { ... }
    pub fn initialize(&mut self) -> Result<Value> { ... }
    pub fn call_tool(&mut self, name: &str, args: Value) -> Result<Value> { ... }
    pub fn list_tools(&mut self) -> Result<Vec<Tool>> { ... }
}
```

**Browser Test Harness** (`test_harness/src/browser.rs`):
```rust
pub struct BrowserTestHarness {
    session: BrowserSession,
}

impl BrowserTestHarness {
    pub fn load_html(&mut self, html: &str) -> Result<()> { ... }
    pub fn click(&mut self, selector: &str) -> Result<()> { ... }
    pub fn type_text(&mut self, selector: &str, text: &str) -> Result<()> { ... }
    pub fn get_dom(&self) -> Result<DomSnapshot> { ... }
    pub fn get_events(&self) -> Result<Vec<Event>> { ... }
}
```

**GUI State Harness** (`test_harness/src/gui.rs`):
```rust
pub struct GuiTestHarness {
    app: VelocityApp,
}

impl GuiTestHarness {
    pub fn new() -> Self { ... }
    pub fn execute_command(&mut self, cmd: &str) -> Result<()> { ... }
    pub fn switch_layout(&mut self, layout: WorkMode) -> Result<()> { ... }
    pub fn get_panel_state(&self) -> PanelState { ... }
    pub fn get_sidebar_state(&self) -> SidebarState { ... }
}
```

---

## CI/CD Integration

### Required Gates (Every PR)
```powershell
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --no-fail-fast  # Unit + component + contract
cargo clippy --workspace -- -D warnings
cargo test -p velocity-e2e            # E2E with fakes
```

### Opt-In Gates (Nightly or Manual)
```powershell
cargo test --workspace --features real-network  # Real provider tests
cargo test --workspace --features gpu           # GPU-dependent tests
cargo test --workspace --features desktop       # Windows desktop smoke
cargo test --workspace --features fuzz          # Fuzz tests
```

### Coverage Reporting
```powershell
cargo tarpaulin --workspace --out Xml --output-dir coverage/
# Upload to codecov.io or similar
```

---

## Coverage Goals

| Crate | Unit | Component | Contract | E2E | Total |
|-------|------|-----------|----------|-----|-------|
| velocity-mcp | 60% | 50% | 80% (tools) | 40% | **65%** |
| velocity-browser | 70% | 50% | N/A | 50% | **60%** |
| velocity-ide | 70% | 60% | N/A | 50% | **65%** |
| drone | 60% | 50% | N/A | 40% | **55%** |
| velocity-router | 70% | 60% | N/A | 50% | **65%** |

**Overall target**: ~65% line coverage with 100% coverage of security boundaries and public APIs

---

## Success Metrics

**Quantitative:**
- ~1,275 tests total
- ~65% line coverage
- E2E suite runs in <5 minutes
- Unit tests run in <2 minutes
- Zero flaky tests in required CI gates

**Qualitative:**
- Every tool has contract tests
- Every critical workflow has integration tests
- Every bug fix has a regression test
- Security boundaries are fully covered

---

## Risk Mitigation

**Risk**: Tests are flaky due to timing/networking  
**Mitigation**: Use deterministic fakes, explicit timeouts, retry logic

**Risk**: GUI tests are brittle  
**Mitigation**: Test state, not rendering; use visual smoke tests sparingly

**Risk**: Test suite runs too slowly  
**Mitigation**: Parallelize where possible, cache fixtures, quarantine slow tests

**Risk**: Coverage goals are not met  
**Mitigation**: Track coverage weekly, prioritize critical paths, add tests incrementally

---

## Conclusion

This test suite specification provides a comprehensive roadmap for achieving full test coverage of the Velocity codebase. The phased approach ensures we build a solid foundation first, then layer on more complex tests. The emphasis on deterministic fakes and behavior-based testing ensures the suite remains maintainable and reliable.

**Next Steps:**
1. Begin Phase 1 implementation (test harness + contract tests)
2. Set up CI coverage reporting
3. Track progress weekly against this specification
4. Adjust priorities based on bug findings and coverage gaps

---

**References:**
- [TESTING_STRATEGY.md](./TESTING_STRATEGY.md) — Original testing strategy document
- [Velocity Wiki](../.wiki/index.md) — Architecture documentation
- [Expert Teams Specification](../.qoder/repowiki/en/content/Architecture/Expert%20Teams%20Enhancement%20Specification.md) — Feature specifications
