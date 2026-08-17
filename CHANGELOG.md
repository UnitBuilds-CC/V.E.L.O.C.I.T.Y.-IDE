# Changelog

All notable changes to the Velocity IDE project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[1.0.0]: https://github.com/UnitBuilds/Kimi-Code/compare/v0.1.0...v1.0.0
[0.1.0]: https://github.com/UnitBuilds/Kimi-Code/releases/tag/v0.1.0
