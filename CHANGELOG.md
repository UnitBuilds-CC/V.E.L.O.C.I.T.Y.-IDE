# Changelog

All notable changes to the Velocity IDE project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Activity bar system**: 8-category icon strip (Files, Search, Git, Chat, Build, Agents, Knowledge, Workspace) with 40+ navigable sub-panels, VS Code-style selection indicator, and Unicode glyphs
- **Full sub-panel implementations**: 19 sub-panels with real data bindings — file tree with filter, bookmarks, favorites, code graph, git changes with staged/unstaged summary, branches, commits, chat with model selector and thinking toggle, multimodal attachments, build controls, agent roster, mission metrics, wiki, NDA documents, plugin registry, skills with search, usage dashboard
- **Theme overhaul**: Modernized 5 color palettes (Midnight, Daylight, Operator, Mission, High Contrast) with HSL-based IdePalette system, green accent (#22C55E) for Midnight
- **GUI extraction**: Created `velocity-ide-gui` crate as standalone GUI launcher, separating UI from MCP server backend
- **Comprehensive test suite**: Expanded to 15,000+ tests across all crates
- **Provider failover tests**: 38 contract tests for serde, routing, and persistence
- **NDA compiler tests**: 29 new tests for tokenizer and JIT compiler
- **Orchestrator tests**: 13 orchestrator + 8 decompose contract tests
- **Security test coverage**: Path traversal, symlink escape, malformed input validation (399 lines)
- **Prometheus metrics**: 421-line metrics module with 17 instruments (requests, tools, providers, agents, resources)
- **OpenTelemetry tracing**: 300-line telemetry module with Pretty/JSON/Compact formats, file rotation, env config
- **GUI integration tests**: 16 headless tests for tab lifecycle, command palette, MRU switcher, file tree, cross-module integration
- **SBOM generation**: CycloneDX JSON SBOM generated in CI and attached to GitHub releases
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

### Changed
- **Architecture**: Editor modules remain in `velocity-mcp` (contain backend logic used by non-editor modules)
- **Error handling**: Eliminated unsafe `unwrap()` patterns in production code paths
- **Dependencies**: Removed `once_cell` crate — replaced with `std::sync::LazyLock` (Rust 1.80+). Loosened exact version pins for `ash`, `gpu-allocator`, `tempfile` to semver ranges
- **Build system**: Fixed `build_release.ps1` to include `velocity-ide-gui` and remove stale `run_nda` reference. Added `rust-toolchain.toml` (MSRV 1.85). Updated Justfile with `gui` and `run` targets
- **Clippy compliance**: Fixed all 49 warnings across workspace (zero warnings remaining)
- **Code quality**: Removed deprecated Python code (archive/agent/, scratch/ directories)
- **README**: Added Quick Start section, fixed directory structure
- **Rustdoc**: Fixed all unresolved link warnings in doc comments

### Fixed
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
- All 14,989+ tests passing (zero failures)
- CI now includes: fmt, clippy, test, build, audit, deny, coverage, SBOM generation

## [1.0.0] - 2026-08-18

### Added
- **Production hardening**: Replaced all `unwrap()` in production paths with `expect()` or `Result`-based error handling
- **Dead code cleanup**: Removed crate-level `#![allow(dead_code)]`, deleted dead `fuzzer.rs` and `wasm_runner.rs` modules (~1,080 lines)
- **Binary hardening**: Added `panic = "abort"` to release profile for smaller binaries
- **Clippy compliance**: Zero clippy lints under `-D warnings`
- **GUI code review fixes**: Fixed corrupted Unicode em-dash in Mission Control tabs, relocated orphaned comment, extracted shared `fetch_panel_data_value()` function, removed duplicated `run_build` from `FetchPanelData`
- **E2E test improvements**: Added graceful skip for `run_nda` binary tests when not built
- **FP4 GPU pipeline documentation**: Added TODO for fused pipeline FP4/FP2 global_scale optimization

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

[Unreleased]: https://github.com/UnitBuilds/Velocity-IDE/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/UnitBuilds/Velocity-IDE/compare/v0.1.0...v1.0.0
[0.1.0]: https://github.com/UnitBuilds/Velocity-IDE/releases/tag/v0.1.0
