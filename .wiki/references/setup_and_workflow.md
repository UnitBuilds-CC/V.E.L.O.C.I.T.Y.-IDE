# Setup & Workflow Guide

This guide provides step-by-step instructions for setting up your development environment, building the workspace, running unit/integration tests, and generating documentation.

---

## 🛠️ Prerequisites & Requirements

- **Rust**: Rust toolchain (MSRV 1.75+) installed via `rustup`.
- **Python**: Python 3.10+ (for helper scripts and test drivers).
- **Vulkan SDK**: Optional but recommended for GPU-accelerated local LLM execution.
- **Operating System**: Windows 10/11 (for full Windows Automation support), Linux, or macOS.

---

## 🔨 Building the Workspace

### 1. Build All Crates (Debug Profile)
```bash
cd velocity-workspace
cargo build
```

### 2. Build Individual Crates
```bash
# Build Velocity IDE & SiteMap Indexer
cargo build -p velocity-ide

# Build Velocity MCP Server & Egui App
cargo build -p velocity_mcp

# Build Velocity Browser Engine
cargo build -p velocity-browser
```

### 3. Release Build (Optimized)
```bash
cargo build --release
```

---

## 🧪 Running Tests

Run the workspace test suite (219+ unit and integration tests):
```bash
cargo test
```

Run tests for a specific crate:
```bash
cargo test -p velocity-browser
cargo test -p velocity_mcp
cargo test -p velocity-ide
```

---

## 📖 Generating & Exporting Wiki Documentation

### 1. Automatic Export via Velocity IDE
Launch the `velocity_mcp` desktop application, navigate to the **Wiki** tab, and click **Export to Markdown**. This writes interlinked Markdown files into `.wiki/`.

### 2. Programmatic Wiki Generation in Rust
```rust
use velocity_ide::site_map::SiteMap;
use velocity_ide::wiki::{build_wiki, export_markdown};

let sm = SiteMap::open(std::path::Path::new(".velocity/site_map"), 0)?;
let model = build_wiki(&sm);
let written = export_markdown(&model, std::path::Path::new(".wiki"))?;
println!("Wrote {} pages to .wiki/", written);
```
