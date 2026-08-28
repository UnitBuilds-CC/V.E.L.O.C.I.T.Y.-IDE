# Contributing to Velocity IDE

Thank you for your interest in contributing to V.E.L.O.C.I.T.Y. Cognitive IDE! This document provides guidelines and workflows for contributing.

## Code of Conduct

Be respectful, constructive, and inclusive. Treat all contributors with dignity regardless of experience level.

## Getting Started

### Prerequisites

- **Rust 1.82+** (pinned via `rust-toolchain.toml`)
- **Git**
- **System dependencies** (Linux): `libgtk-3-dev libwebkit2gtk-4.1-dev libudev-dev`
- **just** (task runner): `cargo install just`

### Build from Source

```bash
git clone https://github.com/UnitBuilds/Velocity-IDE.git
cd Velocity-IDE

# Build all crates
cargo build --workspace

# Or use just
just build
```

### Run Tests

```bash
# Run all tests
cargo test --workspace

# Or use just
just test
```

## Development Workflow

### 1. Fork and Clone

```bash
git clone https://github.com/YOUR-USERNAME/Velocity-IDE.git
cd Velocity-IDE
```

### 2. Create a Branch

```bash
git checkout -b feature/your-feature-name
# or
git checkout -b fix/issue-description
```

### 3. Make Changes

- Follow existing code style (enforced by `rustfmt.toml`)
- Add tests for new functionality
- Update documentation for API changes
- Keep commits focused and atomic

### 4. Verify

```bash
# Format check
cargo fmt --all -- --check

# Lint check (zero warnings required)
cargo clippy --workspace --all-targets -- -D warnings

# Run all tests
cargo test --workspace

# Or use just
just pre-commit
```

### 5. Commit

```bash
git add .
git commit -m "feat: add descriptive commit message"
```

We follow [Conventional Commits](https://www.conventionalcommits.org/):

| Type | Description |
|------|-------------|
| `feat:` | New feature |
| `fix:` | Bug fix |
| `docs:` | Documentation changes |
| `refactor:` | Code restructuring (no behavior change) |
| `test:` | Adding or updating tests |
| `chore:` | Build, CI, or tooling changes |
| `perf:` | Performance improvements |
| `security:` | Security fixes |

### 6. Push and Open a PR

```bash
git push origin feature/your-feature-name
```

Open a Pull Request against `main` and fill in the PR template.

## Project Architecture

```
Velocity-IDE/
├── velocity-ide/        # Core runtime: NDA compiler, tokenizer, model inference
├── velocity-mcp/        # MCP server: editor, agent, connectors, health, metrics
├── velocity-browser/    # Browser control plane: sessions, workflows, auth
├── drone/               # Drone protocol agent
├── e2e/                 # End-to-end test harness
├── fuzz/                # Fuzz testing targets
└── docs/                # Documentation
```

### Key Modules

| Module | Responsibility |
|--------|---------------|
| `velocity-ide::compiler` | Rust-to-NDA compiler, JIT, shaders, Vulkan |
| `velocity-ide::model` | Transformer inference (FP32/FP4/FP2) |
| `velocity-ide::tokenizer` | BPE tokenizer with batch encoding |
| `velocity-mcp::editor` | GUI state management, panels, browser engine |
| `velocity-mcp::agent` | AI agent execution, planning, provider failover |
| `velocity-mcp::connectors` | External service integrations |
| `velocity-mcp::health` | Health check endpoints |
| `velocity-mcp::metrics` | Prometheus metrics collection |
| `velocity-mcp::telemetry` | Distributed tracing infrastructure |
| `velocity-browser` | Browser session management, workflow runner |

## Testing Guidelines

- **Unit tests**: Co-located in `#[cfg(test)] mod tests` within each module
- **Integration tests**: In `tests/` directories per crate
- **GUI tests**: Test state management (headless), not rendering
- **Fuzz tests**: In `fuzz/fuzz_targets/`

```bash
# Run specific test
cargo test -p velocity_mcp --lib metrics

# Run GUI integration tests
cargo test --test gui_integration -p velocity_mcp

# Run with output
cargo test --workspace -- --nocapture
```

## Code Style

- **Zero clippy warnings** enforced in CI (`-D warnings`)
- **rustfmt** enforced in CI (`cargo fmt --check`)
- Use `expect()` with descriptive messages over `unwrap()` in production code
- Document public APIs with `///` doc comments
- Use `tracing` macros for logging (not `println!`)
- Keep functions under ~100 lines where practical

## Reporting Issues

Use the [issue templates](https://github.com/UnitBuilds/Velocity-IDE/issues/new/choose) to report:
- Bug reports
- Feature requests
- Security vulnerabilities (see [SECURITY.md](./SECURITY.md))

## License

By contributing, you agree that your contributions will be licensed under the project's license.
