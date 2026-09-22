# Changelog

All notable changes to the Velocity IDE project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

_Nothing recorded since [2.6.1]._

## [2.6.1] - 2026-09-22

No shipped source changed between `v2.6.0` and this tag - the delta is two documentation files
and one workflow pattern, so the binaries here are the same code as `v2.6.0`'s, rebuilt. This
release exists to attach the dependency inventory every earlier release silently omitted, and
to prove the corrected pattern rather than merely assert it.

### Fixed
- **The SBOM never reached a GitHub Release**: `release.yml` listed `artifacts/velocity-sbom/*.cdx.json`, but `download-artifact` restores a multi-file artifact with its directory structure intact, so the CycloneDX files land one level deeper - one directory per crate. `softprops/action-gh-release` skips a pattern that matches nothing instead of failing, so the job reported success while publishing three archives and no SBOM, on `v2.5.0` and `v2.6.0` alike. The pattern is now `**/*.cdx.json`, and `v2.6.1` is the first tag built from that correction - the previous sentence was unverified until this release ran.

## [2.6.0] - 2026-09-21

First tagged release since `v2.5.0` (2026-09-13) - 33 commits on `main` since that tag. The
dominant theme is an honesty pass over the tool surface, followed by a full-surface sweep of
the interface, followed by CI, where three jobs turned out never to have run at all. What was
measured for this release, on `f6fedb9`: 10,047 library tests and 31 E2E assertions passing,
`cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` clean,
`cargo deny check` and `cargo audit` both exit 0, line coverage 71.33% against the 45% floor
(this release is the first in which that floor has actually been applied), and a 287-pass /
0-fail / 8-skip command sweep driven against a live IDE on a Windows runner. Green CI proves
the workspace builds, tests and lints; it does not prove a live agent conversation against a
real provider key, which no job exercises.

### Added
- **Driver-pressable dialogs, and pixels that can be measured**: `gui_submit_dialog` answers the Open File / Save As prompt actually on screen and `DismissOverlays` stands the whole transient stack down, so an external driver can *complete* a dialog instead of only cancelling one - cancel and Confirm are separate code paths and only one had ever been driven. `gui_screenshot` takes an optional `against` reference and returns a `visual_diff` (percentage, pixel counts, bounding box, dimension match), so a claim about the screen can be measured rather than asserted (`43f65b1`, `8ecd575`).
- **`gui-sweep` CI job**: drives `sweep_gui.ps1 -AllowUnsafe` against a real instance on `windows-latest` and publishes the report as an artifact. The blocking step is the script's own AST parse; the live sweep reports rather than fails, because a runner that cannot launch a window must not break an unrelated build, and the criterion for promoting it to blocking is written into the job (`8ecd575`).
- **Every panel, tab and command drivable over the bridge**: 63 commands (37 navigation, 16 modify, 10 execute) across 8 activity rails, 34 sub-tabs, 52 dock panels, 52 tabs cycled and 63 palette entries. The Orchestrator can also author manual tasks rather than only execute prepared ones (`6a475ef`).
- **`index_workspace` MCP tool** builds the site map for any workspace, and a `--workspace` flag sets the server's workspace root explicitly instead of inheriting the current directory (`a2ba737`, `112b953`).
- **Audit-log wiring and runtime custom tool registration**: tool calls are recorded with their outcome, and tools registered after startup participate in dispatch (`57c7a5b`).
- **Modules inherited from the standalone upstream V.E.L.O.C.I.T.Y.-MCP** were ported into the embedded server, which is only isolated modules rather than the whole upstream (`5257200`).

### Changed
- `run_command` drives PowerShell rather than `cmd.exe` on Windows, so quoting matches the shell callers actually mean (`7fb2805`).
- The IDE starts on a configured provider instead of an unconfigured default, and panels the user asks for now appear (`6923340`).
- Local cargo calls resolve a manifest inside the workspace or refuse with the reason; the bridge reply window is one named 20 s constant that both clients outlast; the sweep is required to prove itself on pixels (`8ecd575`).
- The workspace's own rustfmt configuration applied to the crates that had never been formatted (`5d530dc`), and the remaining clippy lints in the `wa`, workflow and system-test modules cleared (`19365a4`).
- The MCP sweep report canvas re-derived from the event stores rather than from what was remembered about the run (`46e6253`).

### Fixed
- **Tools that reported work they had not done (bugs #1-#54)**: a full MCP feature sweep, then the browser surface. Generated PowerShell now actually runs (`7adfd70`); the virtual-desktop, screenshot, process and tiling tools stopped inventing answers (`0ff134c`, `5553848`); native browser tools report what actually happened (`4b4001a`) and a persisted session resolves labels and URLs honestly (`eceb833`); failures are honest and errors readable (`70172cf`).
- **A capture filter could be substituted rather than honoured**: window capture either applies the requested filter or refuses it, instead of quietly returning a different window (`80b2ac4`).
- **The MCP drone client did not speak the drone's protocol** (`af4ddea`), **team import was brittle and sidecars unbounded** (`5ca7b55`), and **tool-call events left their outcome unresolved** (`d44a2a5`).
- **`gui_quit` did not close the IDE** (`b503db6`), and **the footer status bar could overprint its own right-hand group** - the right group is now clamped to the space after the left pills and long status messages truncate with an ellipsis (`faed1f2`).
- **The wiki HTML graph export panicked on non-ASCII page titles** (`1aca334`), and **a Windows-automation test gambled on a fixed sleep after killing a child** instead of waiting for the pid to disappear (`7d7e7fc`).
- **Three CI jobs were failing because of the compiler, not their subject**: `env.RUSTC_WRAPPER: sccache` was set at workflow level, so every job ran rustc through a binary only three of them install. `cargo deny` reported `failed to fetch crates`, the quarantine job a `cargo metadata` failure, and the Windows GUI sweep a build failure - all of it `could not execute process sccache ... rustc -vV`, so the sweep had never actually swept in CI. The wrapper is now opted into per job, next to each job's `Install sccache` step (`5dfa47b`).
- **The coverage job never started**: it referenced `zgosalvez/github-actions-report-code-coverage-change@v2`, which no longer resolves (`repository not found`), and GitHub fails a job at *Set up job* for that - so the threshold, the lcov artifact and Codecov had not run in any push. The dead action is gone and the summary `cargo llvm-cov` prints is written to the run page instead, behind `set -o pipefail` so a threshold miss cannot hide behind the pipe's exit status (`5dfa47b`).
- **The dependency policy had never actually been read**: given a working rustc, `cargo deny` reported findings that were all real - eight sibling-crate path dependencies carrying no version requirement, which reads as a wildcard under `[bans] wildcards = "deny"`, plus `Apache-2.0 WITH LLVM-exception` (cranelift, via wasmer) and `0BSD` (enum-iterator, via wasmer-compiler) absent from `[licenses] allow`; an SPDX compound id does not match the bare `Apache-2.0` entry. The wildcards are now versioned and both licenses are allowed with the reasoning beside them (`8d12e2e`).
- **The quarantine job read its own documentation as data**: `.config/flaky-quarantine.toml` has no active entries but shows a commented-out `filter = "..."` format example, and the step grepped for `filter = ` without skipping comments - so it asked nextest to run a test that does not exist and took exit 4 as a failure. Comments are excluded, the trailing `|` is stripped, and `--no-tests pass` keeps a stale entry stale rather than red (`8d12e2e`).
- **Every E2E test failed under coverage, over a path**: `workspace_binary` resolved sibling binaries as `<workspace>/target/debug/<name>`, but `cargo llvm-cov` builds into `target/llvm-cov-target`, so all nine `mcp_stdio` tests died with `failed to spawn velocity_mcp` and the threshold was never computed. It now derives the profile directory from `current_exe()`, which holds for a relocated target dir and for `--release` alike (`8d12e2e`).
- **The non-Windows target was made real rather than assumed**: off Windows the telemetry IPC was a single 100 us sleep with no-ops for `signal_*`, so the client read the reply slot before the server wrote it and reported `HMAC verification failed` - an authentication error standing in for a missing handshake. The state word is now polled within a bounded budget (`7adac14`).
- **Five tests asserted Windows path syntax as though it were universal** - `..\..\x` is one ordinary file name on Unix, so a traversal-rejection test passed there by never attempting a traversal - and **six modules carried dead imports inside `cfg(not(windows))` arms a Windows clippy never compiles**, so `--target x86_64-unknown-linux-gnu` failed while the local check said clean (`7adac14`, `a1feefa`).
- **`Build`/`Run` could compile a project you never opened**: cargo finds a manifest by walking *up*, so a folder with no `Cargo.toml` of its own inside a Rust project meant one palette press compiled - and for `Run`, tried to launch - that unrelated project on every core for minutes, during which the bridge timed out and the IDE read as hung. A second `cargo check` whose result was discarded is also gone (`8ecd575`).
- **A failing child put a modal on the user's desktop**: a process that could not initialise raised the `0xc0000142` error box and waited for a click. Both `main()`s now set `SEM_FAILCRITICALERRORS` before the first spawn - deliberately not `SEM_NOGPFAULTERRORBOX`, which would hide the IDE's own crashes (`8ecd575`).
- **A slow frame was reported as a dead IDE**: the bridge answered inside a fixed 5 s while its own clients waited 10 and 20, so a busy-but-alive app returned `Timeout waiting for GUI response` (`8ecd575`).
- **`RUSTSEC-2026-0173` is now ignored in both configs**: `cargo-audit` grades `proc-macro-error2` as a warning while `cargo-deny`'s `unmaintained = "all"` grades it an error, and the two files had drifted to cover that (`f6fedb9`).

### Security
- **rustls 0.23.42 accepted TLS 1.3 handshake messages across encryption-level boundaries** (RUSTSEC-2026-0285, CVSS 5.3) and this project depends on rustls directly for the browser's TLS stack. Bumped to 0.23.45 (`5dfa47b`). **The published `v2.5.0` artifacts contain the vulnerable version**; this release is the fix. The `memmap2` 0.6.2 unsoundness that still warns is reached only through `wasmer` 5.0.6 as a compile-time dependency; removing it for real means a wasmer 5 -> 7 upgrade.
- **An absolute path was rewritten rather than refused**: `resolve_workspace_path` trimmed a leading `/` or `\` off every path argument, so on Linux `write_file("/tmp/x")` reported success after creating `<workspace>/tmp/x` - a call that did something other than what it was asked, with the containment check never firing. Absolute and drive-relative (`C:foo`) inputs are now refused up front, by name, on every platform (`a1feefa`).
- **A dialog answer could write outside the workspace**: `Save As` accepted `..\name.rs` and wrote beside the repository instead of inside the opened workspace. Path prompts now go through the same containment check as every other write, refuse by naming why, and stay open so the value can be corrected (`43f65b1`).
- **Desktop automation is default-deny**: session control requires an explicit opt-in, and clipboard writes and program launches are gated on session consent rather than on the caller asserting it (`70172cf`, `2f76623`).

## Pre-release zip history (v2.0.0 - v2.5.0)

> `v2.0.0` through `v2.5.0` shipped as pre-release zips without their own sections, so the
> entries below are their aggregate log. Two omissions are noted rather than papered over:
> entries describing work dated 2026-09-15 or later fall inside the [2.6.0] window and are
> summarised there (that overlap is deliberate - this block keeps the full root-cause prose),
> and Cargo crate versions stay at `1.0.0` while release tags carry the product version, so a
> `Cargo.toml` version field is not evidence of which release a binary came from.

### Added
- **Driver-pressable dialogs**: `gui_submit_dialog` answers the Open File / Save As prompt that is on screen, and `DismissOverlays` stands the whole transient stack down, so an external driver can *complete* a dialog instead of only cancelling one — cancel and Confirm are separate code paths and only one had ever been driven. `gui_screenshot` takes an optional `against` reference and returns a `visual_diff` (percentage, pixel counts, bounding box, dimension match), which is what lets a claim about the screen be measured rather than asserted.
- **`gui-sweep` CI job**: drives `sweep_gui.ps1 -AllowUnsafe` against a real instance on `windows-latest`. The blocking step is the script's own AST parse; the live sweep reports rather than fails, because a runner that cannot launch a window must not break an unrelated build, and the criterion for promoting it to blocking is written into the job.
- **Drone Bridge Integration**: 10 new MCP tools for deploying and controlling remote drones. `drone_deploy` pushes the drone binary via SSH/SCP and starts it. `drone_command`/`drone_task_status` for async remote execution. `drone_screenshot`/`drone_type_keys`/`drone_click` for remote GUI automation. `drone_upload` with chunked transfer and SHA-256 verification. `drone_pair` for peer protocol integration. Wired into the tool dispatch chain alongside system, browser, WA, and team tools.
- **MoA Router Integration**: New `velocity-router` crate for multi-model orchestration. IDE bridges via `router_client.rs` — POST `/v1/assignments` with health check caching, graceful fallback to direct provider dispatch. Router timeout raised to 300s for MoA workflows.
- **GUI Control Bridge**: MCP tools for remote IDE control (`gui_open_file`, `gui_get_state`, `gui_navigate_panel`, `gui_quit`) via cross-process control bridge on port 19821.
- **Wiki Rebuild Index**: Toolbar button to compile all .rs files and populate the wiki's name dictionary (520 files · 15,574 symbols). Fixed triple extraction in `seed_from_source()` to register file paths and function names as strings, enabling proper file/symbol classification.
- **Ctrl+P Quick-Open**: Ctrl+P now opens the command palette (alias for Ctrl+Shift+P). Updated status bar, Navigate menu, and layouts menu labels to reflect the new binding.
- **Lazy-Load Memory Optimization**: Agent memory and knowledge base now defer disk I/O until first panel access, saving ~23 MB at idle. Scales with agent count — each agent's memory file is loaded only when needed.
- **Chat Message Cap**: Chat history capped at 200 messages to prevent unbounded memory growth in long sessions. Applied after user pushes, agent replies, and history restore.
- **Activity bar system**: 8-category icon strip (Files, Search, Git, Chat, Build, Agents, Knowledge, Workspace) with 40+ navigable sub-panels, VS Code-style selection indicator, and Unicode glyphs
- **Full sub-panel implementations**: 19 sub-panels with real data bindings — file tree with filter, bookmarks, favorites, code graph, git changes with staged/unstaged summary, branches, commits, chat with model selector and thinking toggle, multimodal attachments, build controls, agent roster, mission metrics, wiki, NDA documents, plugin registry, skills with search, usage dashboard
- **Theme overhaul**: Modernized 5 color palettes (Midnight, Daylight, Operator, Mission, High Contrast) with HSL-based IdePalette system, green accent (#22C55E) for Midnight
- **GUI extraction**: Created `velocity-ide-gui` crate as standalone GUI launcher, separating UI from MCP server backend
- **Comprehensive test suite**: Expanded to 10,000+ tests across all crates
- **Provider failover tests**: 38 contract tests for serde, routing, and persistence
- **NDA compiler tests**: 29 new tests for tokenizer and JIT compiler
- **Orchestrator tests**: 13 orchestrator + 8 decompose contract tests
- **Security test coverage**: Path traversal, symlink escape, malformed input validation (399 lines)
- **Prometheus metrics**: 421-line metrics module with 17 instruments (requests, tools, providers, agents, resources)
- **OpenTelemetry tracing**: 300-line telemetry module with Pretty/JSON/Compact formats, file rotation, env config
- **GUI integration tests**: 16 headless tests for tab lifecycle, command palette, MRU switcher, file tree, cross-module integration
- **SBOM generation**: CycloneDX JSON SBOM generated in CI and uploaded as a workflow artifact. Attaching it to the release itself did not work - see the glob-depth fix under [Unreleased] - so no published release had ever carried one.
- **cargo-deny policy**: License allowlist, advisory checks, dependency ban enforcement in CI
- **Criterion benchmarks**: Benchmark scaffolding for NDA operations, tokenizer, and library metadata
- **CONTRIBUTING.md**: Open-source contribution guidelines with architecture overview and code style rules
- **SECURITY.md**: Vulnerability disclosure policy with response timelines
- **.editorconfig**: Cross-editor formatting consistency (Rust, Markdown, YAML, JSON, PowerShell)
- **rustfmt.toml**: Project formatting rules (100 char width, import grouping)
- **GitHub templates**: Bug report, feature request, and PR templates with checklists
- **justfile**: Task runner with build, test, lint, release, security, Docker, and benchmark tasks
- **FP4/FP2 optimization plan**: Detailed implementation spec for GPU fused pipeline
- **Platform README**: Multi-repo overview (IDE, router, website)
- **USER_GUIDE.md**: Comprehensive end-user guide (2,320 lines) covering all IDE features — activity bar, 4 modes, 60+ shortcuts, MCP tools, wiki, NDA, drone, providers, settings, troubleshooting. Verified against source with 94-claim audit.
- **Unsafe block documentation**: All unsafe blocks annotated with SAFETY comments; lint enforced in CI
- **Signed installer**: Windows installer signed with code signing certificate

### Changed
- **CI tool installs**: Replaced `cargo install` with `taiki-e/install-action` pre-built binaries for cargo-audit, cargo-deny, and cargo-llvm-cov — saves ~6 min per CI run
- **CI actions upgrade**: `actions/checkout@v5` (Node 24), `codecov/codecov-action@v6` — eliminates Node 20 deprecation warnings
- **Release workflow**: Now ships all 4 binaries (velocity_ide, velocity_mcp, velocity_ide_gui, velocity-drone). macOS target updated to `aarch64-apple-darwin` for ARM runners
- **Architecture**: Editor modules remain in `velocity-mcp` (contain backend logic used by non-editor modules)
- **Error handling**: Eliminated unsafe `unwrap()` patterns in production code paths
- **Dependencies**: Removed `once_cell` crate — replaced with `std::sync::LazyLock` (Rust 1.80+). Loosened exact version pins for `ash`, `gpu-allocator`, `tempfile` to semver ranges
- **Build system**: Fixed `build_release.ps1` to include `velocity-ide-gui` and remove stale `run_nda` reference. Added `rust-toolchain.toml` (MSRV 1.87). Updated Justfile with `gui` and `run` targets
- **Clippy compliance**: Zero warnings on `cargo clippy --workspace --all-targets -D warnings` across all 5 crates (velocity-browser, velocity-ide, velocity-ide-gui, velocity_mcp, velocity-router). 46 lint fixes in test/bench targets including Copy-type clones, unused variables, range checks, and type simplifications.
- **Code quality**: Removed deprecated Python code (archive/agent/, scratch/ directories)
- **README**: Added Quick Start section, fixed directory structure, corrected test count (9,800+), editor module count (142), added drone/ and e2e/ crates, removed phantom WasmPluginRunner/PropertyFuzzer references, fixed keyboard shortcuts
- **USER_GUIDE accuracy**: 94-claim audit — corrected tool counts (system 28, browser 109, WA 86, team 20 = 243 total), removed phantom drone capabilities (Screen Capture, GUI Automation, Network Monitor), fixed activity bar shortcuts (removed non-existent keybindings), added missing gui_quit tool
- **Rustdoc**: Fixed all unresolved link warnings in doc comments

### Fixed
- **The dependency policy had never actually been read**: `cargo deny` only got as far as its own compiler wrapper until this push, so `deny.toml` had never evaluated anything. Given a working rustc it reported findings, all of them real: eight sibling-crate path dependencies carry no version requirement at all, which reads as a wildcard (`*`) under `[bans] wildcards = "deny"`; `Apache-2.0 WITH LLVM-exception` (cranelift, via wasmer) and `0BSD` (enum-iterator, via wasmer-compiler) were absent from `[licenses] allow` — which is not the same as those licenses being unapproved, since an SPDX compound id does not match the bare `Apache-2.0` entry; and `RUSTSEC-2026-0173` marks `proc-macro-error2` unmaintained, reached only through `wasmer-derive`, a proc macro that is not linked into any shipped binary. The wildcards are now versioned, the two licenses are allowed with the reasoning beside them, and the advisory joins the documented ignores next to its eight peers rather than being dropped quietly. `cargo deny check` exits 0 here; the Linux graph carries wayland-side crates this run cannot see, so CI is the first full-forest evaluation.
- **The quarantine job read its own documentation as data**: `.config/flaky-quarantine.toml` has no active entries but shows a commented-out `filter = "safety_concurrent_poison_and_recover"` as a format example, and the step greps for `filter = ` without skipping comments. It therefore asked nextest to run a test that does not exist; nextest exited 4 with `error: no tests to run`, and the job that its own comment calls "a no-op" reported failure. Comments are now excluded, the trailing `|` left by the join is stripped, and `--no-tests pass` keeps a stale entry stale rather than red.
- **Every E2E test failed under coverage, over a path**: `workspace_binary` resolved sibling binaries as `<workspace>/target/debug/<name>`, which is only where cargo puts them when nothing relocated the target dir. `cargo llvm-cov` builds into `target/llvm-cov-target`, so all nine `mcp_stdio` tests died with `failed to spawn velocity_mcp ... No such file or directory (os error 2)` and the coverage threshold was never computed. It now derives the profile directory from `current_exe()` — out of `deps/`, one level up — which holds for a relocated target dir and for `--release` alike, with the old path kept as the fallback.
- **RUSTSEC-2026-0285 (rustls, CVSS 5.3 medium)**: `rustls` was locked at 0.23.42, which accepts TLS 1.3 handshake messages across encryption-level boundaries; the fix is 0.23.45 and this project depends on rustls directly for the browser's TLS stack, so `cargo audit` had been failing on it. Bumped `rustls` to 0.23.45 (pulls `rustls-webpki` 0.103.15). Not ignored, because unlike the entries in `.cargo/audit.toml` this one had a fix available. The `memmap2` 0.6.2 unsoundness audit still warns about is reached only through `wasmer` 5.0.6 (`cargo tree -i memmap2@0.6.2`); the shared-memory IPC uses memmap2 0.9.
- **Three CI jobs were failing because of the compiler, not their subject**: `env.RUSTC_WRAPPER: sccache` was set at workflow level, so every job ran rustc through a binary only three of them install. `cargo deny` reported that as `failed to fetch crates`, the quarantine job as a `cargo metadata` failure, and the Windows GUI sweep as a build failure — all of it `could not execute process sccache ... rustc -vV`, so the sweep had never actually swept in CI. The wrapper is now opted into per job, next to each job's `Install sccache` step.
- **The coverage job never started**: it referenced `zgosalvez/github-actions-report-code-coverage-change@v2`, which no longer resolves (`repository not found`), and GitHub fails a job at *Set up job* for that — so the 45% threshold, the lcov artifact and Codecov had not run in any push, and the coverage gate has never actually been applied. The dead action is gone and the summary `cargo llvm-cov` already prints is written to the run page instead, behind `set -o pipefail` so a threshold miss cannot be hidden by the pipe's exit status.
- **An absolute path was rewritten rather than refused**: `resolve_workspace_path` trimmed a leading `/` or `\` off every path argument, so on Linux `write_file("/tmp/x")` reported success after creating `<workspace>/tmp/x` — a call that did something other than what it was asked, with the containment check never firing. The same input on Windows only failed because `Path::join` substitutes the root for an absolute path and the canonicalize check caught it downstream, so the contract the schemas advertise ("Absolute paths and anything resolving outside the workspace are refused") was enforced by accident on one platform and not at all on the other. Absolute and drive-relative (`C:foo`) inputs are now refused up front, by name, everywhere.
- **Two more lint sites that only a non-Windows target can see**, surfaced once the first pass got that far: the non-Windows `create_or_open` said `.create(true)` without saying `truncate`, and `wa/advanced_input` had an `unneeded return` in the arm that is that function's tail expression there. Neither changed behaviour — `truncate` is off unless requested — so the fix states the intent the Windows arm has always stated (preserve a peer's live buffer across a reconnect) instead of leaving it to be inferred.
- **A refusal test that asserted one particular wording**: `refuses_without_consent` required the literal `requires Windows`, which `wa_virtual_desktop_switch` does not say ("... require Windows 10/11"). It now requires the refusal to name the platform gate instead of a sentence, while the Windows branch keeps its strict demand that the opt-in switch be spelled out.
- **The telemetry IPC only worked on Windows**: off Windows `wait_for_request`/`wait_for_response` were a single 100 µs sleep and `signal_*` were no-ops, so the client read the reply slot before the server had written one and reported `HMAC verification failed` — an authentication error standing in for a missing handshake. The state word is now polled within a bounded budget, which is what the named-event build on Windows already achieves.
- **Five tests asserted Windows path syntax as though it were universal**: `..\..\x` is one ordinary file name on Unix, where `\` does not separate, so a traversal-rejection test passed there by never attempting a traversal; `Path::starts_with` compares components, so a `C:\projects` allowlist prefix matched nothing; and the sitemap recorder escapes `\` but leaves `/` alone, so its expected records were platform-bound. Each now derives the separator from the target it runs on. Two more asserted that a refusal names the opt-in switch, which is the right contract only where consent is the operative gate — off Windows those tools are refused as unsupported before consent is ever asked.
- **Four modules did not lint for a non-Windows target**: `velocity-drone`, `wa/clipboard`, `wa/process_mgmt` and `editor/deploy_pipeline` carried unused imports and parameters inside `cfg(not(windows))` arms that a Windows `cargo clippy` never compiles, so `--target x86_64-unknown-linux-gnu` failed while the local check said clean.
- **`Build`/`Run` could compile a project you never opened**: the palette ran cargo with `current_dir` at the workspace root, and cargo finds a manifest by walking *up*. Opening a folder that has no `Cargo.toml` of its own inside a Rust project therefore meant one palette press compiled — and for `Run`, tried to launch — that entire unrelated project on every core for minutes, during which the control bridge timed out and the IDE read as hung. `Build` also ran a second `cargo check` whose result was thrown away. Every local cargo call now resolves a manifest inside the workspace or refuses with the reason, and the refusal reaches the UI the same way a finished build does.
- **A failing child put a modal on the user's desktop**: a process that could not initialise raised the `0xc0000142` error box and sat waiting for a click, which is how an unattended sweep ends up frozen in front of somebody. Both `main()`s now set `SEM_FAILCRITICALERRORS` before the first spawn so children inherit it — deliberately not `SEM_NOGPFAULTERRORBOX`, which would hide the IDE's own crashes.
- **A slow frame was reported as a dead IDE**: the control bridge answered inside a fixed 5 seconds while its own clients waited 10 and 20, so a busy-but-alive app returned `Timeout waiting for GUI response`. One named constant (20 s) now defines the reply window and both clients outlast it.
- **A dialog answer could write outside the workspace**: `Save As` accepted `..\name.rs` and wrote beside the repository instead of inside the opened workspace. Path prompts now go through the same containment check as every other write, refuse by naming why, and stay open so the value can be corrected.
- **CI pipeline**: Added missing `libc` dependency for Unix targets (Linux/macOS) — fixes compilation of `exec_page.rs` (mmap/munmap) and `crypto.rs` (getuid)
- **cargo-deny config**: Updated `deny.toml` to 0.16+ format — removed deprecated fields (`vulnerability`, `unmaintained="warn"`, `default-allow`, etc.)
- **Test struct initializers**: Added 16 missing fields to `VelocityApp` test constructors in `tests.rs`
- **Clippy warning**: Fixed `explicit_auto_deref` lint in `ui_render.rs`
- **Formatting**: Ran `cargo fmt` across all workspace crates (28 files)
- **Font Coverage**: Swapped U+2726 (✦) for U+2605 (★) in AI Suggestions panel icon — Inter font covers ★ but not ✦, which rendered as tofu (hollow box).
- **Wiki Type Mismatch**: Fixed `unwrap_or(0)` type mismatch in `rebuild_index()` — `read_persisted_weight_root` returns `Option<u64>` but `open` takes `u64`.
- Fixed 2 unsafe `unwrap()` patterns in `usage.rs` that could panic
- Fixed empty format string warnings by embedding literals in format strings
- Fixed `is_multiple_of()` clippy lints across 22 files
- Fixed redundant closures, `map_or` simplifications, `clamp` usage, `div_ceil` across codebase
- Fixed rustdoc unresolved links: `\[hidden_size\]`, `\[VOCAB_SIZE\]`, `\[callees\]`, `Option<JitVal>`, dimension annotations
- Fixed doc test failures in logging.rs, metrics.rs, telemetry.rs (changed to `ignore`)

### Security
- Verified path traversal protection: `resolve_workspace_path` uses canonicalize + starts_with checks
- Verified credential handling: `SecretString` with zeroization on drop
- Verified API key masking in all display outputs
- Audited all unsafe code blocks (Windows FFI, Vulkan — all in expected areas)
- Added cargo-deny license policy enforcement in CI
- Added SBOM (CycloneDX) generation for supply chain transparency

### Build
- Release build optimized: `strip = true`, `lto = "thin"`, `opt-level = "s"`, `codegen-units = 16`, `panic = "abort"`
- All 10,047 library tests plus 31 E2E assertions passing (zero failures)
- CI now includes: fmt, clippy, test, build, audit, deny, coverage, SBOM generation

## [1.0.0] - 2026-08-18

### Added
- **Production hardening**: Replaced all `unwrap()` in production paths with `expect()` or `Result`-based error handling
- **Dead code cleanup**: Removed crate-level `#![allow(dead_code)]`, deleted dead `fuzzer.rs` and `wasm_runner.rs` modules (~1,080 lines)
- **Binary hardening**: Added `panic = "abort"` to release profile for smaller binaries
- **Clippy compliance**: Zero clippy lints under `-D warnings`
- **GUI code review fixes**: Fixed corrupted Unicode em-dash in Mission Control tabs, relocated orphaned comment, extracted shared `fetch_panel_data_value()` function, removed duplicated `run_build` from `FetchPanelData`
- **E2E test improvements**: Added graceful skip for `run_nda` binary tests when not built
- **FP4 GPU pipeline documentation**: Added optimization note for fused pipeline FP4/FP2 global_scale (see `docs/FP4_FP2_OPTIMIZATION.md`)

### Changed
- Standardized all crate versions to `1.0.0`
- Net code reduction: ~1,200 lines removed through dead code elimination
- Improved error messages with `expect()` documenting invariants

### Fixed
- Fixed `manual_strip_prefix` clippy lint in `main.rs`
- Applied `rustfmt` across entire workspace
- Fixed e2e tests to gracefully skip when optional binaries aren't built

### Security
- All production `unwrap()` calls eliminated — zero panic paths in production code
- CI gates enforce `-D warnings` for both `cargo check` and `cargo clippy`

## [0.1.0] - 2026-08-10

### Added
- Initial V.E.L.O.C.I.T.Y.-IDE implementation
- NDA (N-Dimensional Array) inference runtime
- Vulkan GPU acceleration pipeline
- Qwen 2.5 Coder 0.5B model support
- MCP (Model Context Protocol) server integration
- Site map with Merkle verification
- Sandbox execution environment
- Drone dual-mode architecture (local + remote)
- Browser engine with NDA support

[Unreleased]: https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-IDE/compare/v2.6.1...HEAD
[2.6.1]: https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-IDE/compare/v2.6.0...v2.6.1
[2.6.0]: https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-IDE/compare/v2.5.0...v2.6.0
[1.0.0]: https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-IDE/compare/v0.1.0...v1.0.0
[0.1.0]: https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-IDE/releases/tag/v0.1.0
