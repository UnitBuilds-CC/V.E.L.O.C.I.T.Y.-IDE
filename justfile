# Velocity IDE - Development Tasks
# Requires: https://github.com/casey/just

# Default task
default:
    @just --list

# ─── Build ─────────────────────────────────────────────────────────────────

# Build all crates (debug)
build:
    cargo build --workspace

# Build all crates (release)
build-release:
    cargo build --workspace --release

# Build specific crate
build-crate CRATE:
    cargo build -p {{CRATE}}

# ─── Test ──────────────────────────────────────────────────────────────────

# Run all tests
test:
    cargo test --workspace

# Run tests with output
test-verbose:
    cargo test --workspace -- --nocapture

# Run specific test
test-filter FILTER:
    cargo test --workspace {{FILTER}}

# Run tests with coverage
test-coverage:
    cargo llvm-cov --workspace --lcov --output-path lcov.info
    @echo "Coverage report generated: lcov.info"

# ─── Lint & Format ─────────────────────────────────────────────────────────

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

# ─── Documentation ─────────────────────────────────────────────────────────

# Generate documentation
doc:
    cargo doc --workspace --no-deps --open

# Generate documentation (open in browser)
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
    cargo doc --workspace --no-deps 2>&1 | findstr /i "warning" || echo "No doc warnings"

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

# Watch for changes and rebuild
watch:
    cargo watch --clear --execute "cargo check"

# Run benchmarks
bench:
    cargo bench --workspace

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
