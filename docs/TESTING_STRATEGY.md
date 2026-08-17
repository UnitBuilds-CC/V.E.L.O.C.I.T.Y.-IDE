# Testing Strategy

Velocity is a native IDE, MCP server, browser engine, orchestration platform, and NDA compiler. A credible release gate must test each boundary at the lowest useful layer, then prove the critical user workflows across boundaries.

## Test pyramid

| Layer | Scope | Required evidence | Command |
| --- | --- | --- | --- |
| Unit | Pure parsing, state transitions, policies, and layout calculations | Deterministic happy-path, boundary, rejection, and regression tests | `cargo test -p <crate> --lib` |
| Component | One subsystem with filesystem/process/network seams replaced by temporary fixtures | Persistence round trips, error propagation, cancellation, and event ordering | `cargo test -p <crate> --test <suite>` |
| Contract | Public MCP tools and JSON-RPC schemas | Every advertised tool has an executable contract fixture; invalid arguments fail safely | `cargo test -p velocity-e2e --test mcp_stdio` |
| End-to-end | Native binaries and primary workflows | Exit status, durable artifacts, and observable output—not merely a spawned process | `cargo test -p velocity-e2e` |
| GUI regression | egui state and real desktop smoke runs | Navigation/action state, layout persistence, and screenshot/accessibility baselines | targeted app tests plus Windows desktop smoke job |

## System coverage matrix

- **MCP and registry:** tool definition ↔ dispatcher parity; JSON-RPC initialize/list/call; workspace isolation; governance approval/denial; file traversal rejection; checkpoint, memory, knowledge, team, browser, workflow, and Windows Automation contracts.
- **Agent and orchestration:** provider failover with deterministic fakes; plan/review/execute lifecycle; cancellation; task dependency graph; event ordering; persisted run evidence.
- **IDE and GUI:** command palette actions; Build/Mission layout selection; panel focus/toggle; sidebar state; Team Studio create/assign; Mission review-before-execution; Orchestrator graph fit and scroll bounds. Keep most of these as deterministic state tests; use a small set of Windows visual smoke tests for actual rendering.
- **Browser:** HTML → DOM → accessible summary → form/input action → event/mutation trace → storage/history. Use local fixture pages and never make CI depend on the public network.
- **NDA/compiler:** lexer/parser diagnostics, serialization round trips, site-map persistence, sandbox execution, malformed input, and backward-compatibility fixtures.
- **Desktop automation:** selector resolution, approval policy, timeout/cancellation, evidence capture, and a small opt-in Windows desktop suite running against a fixture app.
- **Security/reliability:** path traversal and symlink escape attempts, malformed RPC/NDA frames, secret redaction, process cleanup, crash recovery, and dependency audit.

## Test design rules

1. Test behavior and durable outcomes, not implementation details or whether a binary happens to exist.
2. Use `tempfile` workspaces, fixture HTML, fake providers, and explicit timeouts. No production credentials, desktop state, or public-network dependency in required CI tests.
3. Each fixed defect receives a regression test at the lowest layer that could have caught it.
4. Mark external-provider, GPU, and real-desktop tests as opt-in; required CI must remain deterministic.
5. A tool is not considered wired until its definition, dispatch, permission behavior, valid call, and invalid call are covered by one contract fixture.

## Delivery plan

1. **Foundation:** replace permissive E2E smoke tests; centralize JSON-RPC process helpers with startup/read deadlines; add tool-registry parity tests and a fixture workspace factory.
2. **Critical workflows:** add MCP workspace mutation/fetch contracts, Mission plan-review-execute contracts, browser interaction workflows, NDA source-to-artifact workflows, and agent/provider failure contracts.
3. **GUI confidence:** expose pure action/state transitions for app tests; add Windows desktop smoke coverage for startup, layout switching, command palette, and graph visibility; store approved screenshots only where rendering determinism permits it.
4. **Hardening:** add negative/fuzz corpus tests for parsers and RPC, Windows Automation fixture tests, cancellation/timeout tests, and nightly coverage plus mutation testing for high-risk modules.
5. **Release evidence:** publish unit/component/E2E counts, coverage trend, flaky-test quarantine, and platform matrix results with each release.

## Local gates

From the workspace root:

```powershell
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --no-fail-fast
cargo clippy --workspace -- -D warnings
cargo test -p velocity-e2e
```

When the IDE is running locally, set a separate target directory for test builds so the live executable is never replaced:

```powershell
$env:CARGO_TARGET_DIR = "$PWD\target-test"
cargo test -p velocity-e2e --test browser_engine
```
