# Velocity IDE - Development Tasks
# Requires: https://github.com/casey/just

# Default task
default:
    @just --list

# ─── Build ─────────────────────────────────────────────────────────────────

# Build all crates (debug) with sccache
build:
    #!/usr/bin/env bash
    set -euo pipefail
    if command -v sccache &> /dev/null; then
        export RUSTC_WRAPPER=sccache
        echo "✓ Using sccache for compilation"
    fi
    cargo build --workspace

# Build all crates (release)
build-release:
    cargo build --workspace --release

# Build specific crate
build-crate CRATE:
    cargo build -p {{CRATE}}

# Build only changed crates (fast iteration)
build-changed:
    #!/usr/bin/env bash
    set -euo pipefail
    if command -v sccache &> /dev/null; then
        export RUSTC_WRAPPER=sccache
    fi
    # Build only the main binaries, not the whole workspace
    cargo build --bin velocity_ide --bin velocity_mcp --bin velocity_ide_gui --bin velocity-drone

# ─── Test ──────────────────────────────────────────────────────────────────

# Run all tests
test:
    cargo test --workspace

# Run all tests in parallel across crates (Windows)
test-all:
    powershell -ExecutionPolicy Bypass -File ./run_tests_parallel.ps1

# Run velocity-mcp tests only
test-mcp:
    cargo test -p velocity-mcp

# Run velocity-browser tests only
test-browser:
    cargo test -p velocity-browser

# Run tests with output
test-verbose:
    cargo test --workspace -- --nocapture

# Run specific test
test-filter FILTER:
    cargo test --workspace {{FILTER}}

# Run tests for a specific crate only (fast)
test-crate CRATE:
    cargo test -p {{CRATE}}

# Run tests with coverage
test-coverage:
    cargo llvm-cov --workspace --lcov --output-path lcov.info
    @echo "Coverage report generated: lcov.info"

# ─── Lint & Format ─────────────────────────────────────────────────────────

# Check entire workspace (fast type-check without codegen)
check-all:
    cargo check --workspace

# Check formatting
fmt-check:
    cargo fmt --all -- --check

# Fix formatting
fmt:
    cargo fmt --all

# Run clippy
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Fix clippy warnings
clippy-fix:
    cargo clippy --workspace --fix --allow-dirty --allow-staged

# Run all lints
lint: fmt-check clippy

# Fix all lints
lint-fix: fmt clippy-fix

# ─── Run ───────────────────────────────────────────────────────────────────

# Run the GUI
run:
    cargo run --bin velocity_ide

# Run the GUI (release)
run-release:
    cargo run --release --bin velocity_ide

# Run MCP server (stdio mode)
run-mcp:
    cargo run --bin velocity_mcp -- --mode stdio

# Run MCP server (shmem mode)
run-mcp-shmem:
    cargo run --bin velocity_mcp -- --mode shmem

# ─── Clean ─────────────────────────────────────────────────────────────────

# Clean build artifacts
clean:
    cargo clean

# Clean and rebuild
rebuild: clean build

# Clean sccache stats
clean-sccache:
    #!/usr/bin/env bash
    if command -v sccache &> /dev/null; then
        sccache --zero-stats
        echo "✓ sccache stats cleared"
    else
        echo "⚠ sccache not installed"
    fi

# ─── Documentation ─────────────────────────────────────────────────────────

# Generate documentation
doc:
    cargo doc --workspace --no-deps

# Generate documentation and open in browser
doc-open:
    cargo doc --workspace --no-deps --open

# ─── Dependency Management ─────────────────────────────────────────────────

# Update dependencies
update:
    cargo update

# Check for outdated dependencies
outdated:
    cargo outdated

# Audit dependencies
audit:
    cargo audit

# Deny check (licenses, bans, advisories)
deny:
    cargo deny check

# ─── CI/CD ─────────────────────────────────────────────────────────────────

# Run all CI checks
ci: lint test deny audit doc-check
    @echo "✓ All CI checks passed"

# Pre-commit checks
pre-commit: fmt-check clippy
    @echo "✓ Pre-commit checks passed"

# Check documentation for warnings
doc-check:
    cargo doc --workspace --no-deps 2>&1

# ─── Release ───────────────────────────────────────────────────────────────

# Create release build
release:
    cargo build --release
    @echo "Release binaries in target/release/"

# Package for distribution
package: release
    @echo "Packaging for distribution..."
    # Add packaging logic here

# ─── Development ───────────────────────────────────────────────────────────

# Watch for changes and rebuild (with sccache)
watch:
    #!/usr/bin/env bash
    set -euo pipefail
    if command -v sccache &> /dev/null; then
        export RUSTC_WRAPPER=sccache
        echo "✓ Using sccache for watch mode"
    fi
    cargo watch --clear -x check

# Watch and run on changes (hot reload pattern)
watch-run:
    #!/usr/bin/env bash
    set -euo pipefail
    if command -v sccache &> /dev/null; then
        export RUSTC_WRAPPER=sccache
    fi
    cargo watch --clear -x "run --bin velocity_ide"

# Watch specific crate
watch-crate CRATE:
    #!/usr/bin/env bash
    set -euo pipefail
    if command -v sccache &> /dev/null; then
        export RUSTC_WRAPPER=sccache
    fi
    cargo watch --clear -x "check -p {{CRATE}}"

# Run benchmarks
bench:
    cargo bench --workspace

# Run benchmarks with regression detection (compare vs saved baseline)
bench-check:
    cargo bench --workspace -- --noplot

# Save current benchmark results as new baseline
bench-save-baseline:
    cargo bench --workspace -- --noplot --save-baseline baseline

# Run GUI integration tests
test-gui:
    cargo test --test gui_integration -p velocity_mcp

# Generate SBOM (requires cargo-cyclonedx)
sbom:
    cargo cyclonedx --format json --all
    @echo "SBOM generated: *.cdx.json"

# Generate flamegraph (requires cargo-flamegraph)
flamegraph:
    cargo flamegraph --bin velocity_mcp

# Show sccache statistics
sccache-stats:
    #!/usr/bin/env bash
    if command -v sccache &> /dev/null; then
        sccache --show-stats
    else
        echo "⚠ sccache not installed. Install with: cargo install sccache"
    fi

# ─── Database ──────────────────────────────────────────────────────────────

# Reset development database
db-reset:
    @echo "Resetting development database..."
    # Add database reset logic here

# ─── Utilities ─────────────────────────────────────────────────────────────

# Count lines of code
loc:
    tokei .

# Count tests
test-count:
    cargo test --workspace -- --list | grep -c "test$$"

# Show dependency tree
deps:
    cargo tree --depth 1

# Check for security vulnerabilities
security:
    cargo audit
    cargo deny check advisories
