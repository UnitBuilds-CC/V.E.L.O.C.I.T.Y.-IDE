# API Reference

This document summarizes key public Rust traits, core data structures, and IPC protocols across `velocity-browser`, `velocity-mcp`, and `velocity-ide`.

---

## 🌐 `velocity-browser` API Reference

### Core Structs & Traits

```rust
// velocity-browser/src/session.rs
pub struct BrowserSession {
    pub fn new() -> Self;
    pub fn navigate(&mut self, url: &str) -> Result<()>;
    pub fn get_aom(&self) -> AgenticAomTree;
}

// velocity-browser/src/dom/tree.rs
pub struct DomTree {
    pub fn parse_html(html: &str) -> Self;
    pub fn query_selector(&self, selector: &str) -> Option<NodeId>;
}

// velocity-browser/src/net/tls13.rs
pub struct Tls13Client {
    pub fn connect(host: &str, port: u16) -> Result<TlsStream>;
}

// velocity-browser/src/agentic/aom_tree.rs
pub struct AgenticAomTree {
    pub nodes: Vec<AomNode>,
    pub fn to_compact_prompt(&self) -> String;
}
```

---

## 🛠️ `velocity-mcp` API Reference

### Core Structs & Enums

```rust
// velocity-mcp/src/registry/mod.rs
pub struct ToolRegistry {
    pub fn register_tool(&mut self, tool: ToolDefinition);
    pub fn dispatch(&self, name: &str, args: Value) -> Pin<Box<dyn Future<Output = Result<Value>>>>;
}

// velocity-mcp/src/editor/app/velocity_app/struct_def.rs
pub struct VelocityApp {
    pub fn new(cc: &eframe::CreationContext) -> Self;
    pub fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame);
}

// velocity-mcp/src/orchestrator/worker/worktree.rs
pub struct WorktreeLockManager {
    pub fn acquire_lock(&self, scope: &Path) -> Result<WorktreeGuard>;
    pub fn verify_execution_contract(&self, guard: &WorktreeGuard) -> VerificationReport;
}

// velocity-mcp/src/ipc/telemetry_share.rs
pub struct TelemetrySharedMemory {
    pub fn open_or_create(path: &Path) -> Result<Self>;
    pub fn write_trace(&self, event: &TelemetryEvent);
}
```

---

## 🗺️ `velocity-ide` API Reference

### Core Structs & Functions

```rust
// velocity-ide/src/site_map/mod.rs
pub struct SiteMap {
    pub fn open(dir: &Path, flags: u32) -> Result<Self>;
    pub fn register_string(&mut self, s: &str) -> Result<u64>;
    pub fn put_file_snapshot(&mut self, file: &str, triples: &[VcTriple]) -> Result<()>;
}

// velocity-ide/src/wiki/generate.rs
pub fn build_wiki(sm: &SiteMap) -> WikiModel;

// velocity-ide/src/wiki/markdown.rs
pub fn export_markdown(model: &WikiModel, dir: &Path) -> Result<usize>;
```
