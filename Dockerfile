# syntax=docker/dockerfile:1.6
# Multi-stage build for Velocity IDE with BuildKit cache mounts
# Requires: DOCKER_BUILDKIT=1

# Stage 1: Build all Rust binaries
FROM rust:1.87-slim-bookworm AS builder

# Install build dependencies + sccache + lld
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libudev-dev \
    libgtk-3-dev \
    clang \
    lld \
    && rm -rf /var/lib/apt/lists/*

# Install sccache for build caching
RUN cargo install sccache --locked

WORKDIR /build

# Configure cargo to use sccache and lld
RUN mkdir -p .cargo && \
    echo '[build]' >> .cargo/config.toml && \
    echo 'rustc-wrapper = "/usr/local/cargo/bin/sccache"' >> .cargo/config.toml && \
    echo '[target.x86_64-unknown-linux-gnu]' >> .cargo/config.toml && \
    echo 'linker = "clang"' >> .cargo/config.toml && \
    echo 'rustflags = ["-C", "link-arg=-fuse-ld=lld"]' >> .cargo/config.toml

# Copy workspace manifests first for dependency caching
COPY Cargo.toml Cargo.lock ./
COPY velocity-mcp/Cargo.toml velocity-mcp/
COPY velocity-ide/Cargo.toml velocity-ide/
COPY velocity-ide-gui/Cargo.toml velocity-ide-gui/
COPY velocity-browser/Cargo.toml velocity-browser/
COPY drone/Cargo.toml drone/
COPY e2e/Cargo.toml e2e/

# Create dummy source files to cache dependency builds
RUN mkdir -p velocity-mcp/src && echo "fn main() {}" > velocity-mcp/src/main.rs && \
    mkdir -p velocity-ide/src && echo "fn main() {}" > velocity-ide/src/main.rs && \
    mkdir -p velocity-ide-gui/src && echo "fn main() {}" > velocity-ide-gui/src/main.rs && \
    mkdir -p velocity-browser/src && echo "fn main() {}" > velocity-browser/src/main.rs && \
    mkdir -p drone/src && echo "fn main() {}" > drone/src/main.rs && \
    mkdir -p e2e/src && echo "fn main() {}" > e2e/src/main.rs

# Build dependencies with cache mounts (this layer is cached unless Cargo.toml changes)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    --mount=type=cache,target=/root/.cache/sccache \
    cargo build --release

# Copy actual source code
COPY . .

# Touch source files to invalidate dummy builds
RUN find . -name "*.rs" -exec touch {} +

# Build release binaries with cache mounts
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    --mount=type=cache,target=/root/.cache/sccache \
    cargo build --release --bin velocity_ide --bin velocity_mcp --bin velocity_ide_gui --bin velocity-drone

# Stage 2: Runtime image
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libgtk-3-0 \
    libwebkit2gtk-4.1-0 \
    git \
    ripgrep \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -s /bin/bash velocity
USER velocity
WORKDIR /home/velocity

# Copy binaries from builder
COPY --from=builder --chown=velocity:velocity /build/target/release/velocity_ide /usr/local/bin/
COPY --from=builder --chown=velocity:velocity /build/target/release/velocity_mcp /usr/local/bin/
COPY --from=builder --chown=velocity:velocity /build/target/release/velocity_ide_gui /usr/local/bin/
COPY --from=builder --chown=velocity:velocity /build/target/release/velocity-drone /usr/local/bin/

# Set up workspace directory
RUN mkdir -p /home/velocity/workspace
VOLUME ["/home/velocity/workspace"]

# Default command
CMD ["velocity_ide_gui", "--help"]
