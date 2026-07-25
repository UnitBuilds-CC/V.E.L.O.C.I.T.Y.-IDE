# API Reference

Summary of key public Rust types and functions across all three crates. All signatures are verified against actual source code.

---

## velocity-mcp: Agent Types

### AiProvider Enum

```rust
// velocity-mcp/src/agent/models.rs
pub enum AiProvider {
    CloudflareWorkersAi,
    OpenRouter,
    AzureOpenAi,
    LocalOllama,
    OpenAI,
    Anthropic,
    GoogleVertex,
}

impl AiProvider {
    pub fn label(self) -> &'static str;
    pub fn slug(self) -> &'static str;
    pub fn from_slug(value: &str) -> Option<AiProvider>;
}
```

### Agent Messages

```rust
// velocity-mcp/src/agent/models.rs
pub enum UiToAgentMessage {
    SetWorkspace(PathBuf),
    RefreshModels,
    RefreshUsage,
    ReloadProviderConfig,
    ApplySessionState { provider: AiProvider, model: String, thinking: bool },
    SetModel(String),
    SetThinking(bool),
    SetProvider(AiProvider),
    UserPrompt(String),
    ClearHistory,
    ApproveTool { id: String, arguments: Value },
    RejectTool { id: String },
    RunLocalBuild,
    RunLocalRun,
    CancelTask,
    ReloadTeams,
}

pub enum AgentToUiMessage {
    ThoughtToken(String),
    OutputToken(String),
    RequestToolApproval { id: String, tool_name: String, arguments: Value },
    ToolExecutionStarted { tool_name: String },
    ToolExecutionFinished { tool_name: String, result: String },
    StatusUpdate(String),
    AgentFinished,
    UpdateFileBuffer { path: PathBuf, content: String },
    ModelCatalog { models: Vec<ModelInfo>, selected: String, thinking: bool },
    AccountUsage { accounts: Vec<AccountUsageView>, date: String },
    ChatHistoryRestored(Vec<(String, String)>),
    ProviderChanged(AiProvider),
}
```

### Model Info

```rust
// velocity-mcp/src/agent/models.rs
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    pub api_style: ApiStyle,
    pub supports_tools: bool,
    pub supports_thinking: bool,
}

pub enum ApiStyle {
    OpenAiTools,
    OpenAiChat,
    PromptCompletion,
}
```

### Provider Functions

```rust
// velocity-mcp/src/agent/provider.rs
pub fn fallback_provider(current: AiProvider) -> AiProvider;
pub fn default_provider_model(provider: AiProvider) -> String;
pub fn fetch_model_catalog(accounts: &[CloudflareAccount]) -> Result<Vec<ModelInfo>, String>;
pub fn fetch_openrouter_models(or_accounts: &[OpenRouterAccount], usage_tracker: &UsageTracker) -> Result<Vec<ModelInfo>, String>;
pub fn fetch_azure_models(accounts: &[AzureOpenAiAccount]) -> Result<Vec<ModelInfo>, String>;
pub fn fetch_local_ollama_models(accounts: &[LocalOllamaAccount]) -> Result<Vec<ModelInfo>, String>;
pub fn infer_model_info(id: String, item: &Value) -> Option<ModelInfo>;
```

### Headless Sub-Agent

```rust
// velocity-mcp/src/agent/models.rs
pub struct HeadlessSubAgentRequest {
    pub workspace_root: PathBuf,
    pub provider: AiProvider,
    pub model: String,
    pub thinking: bool,
    pub prompt: String,
    pub cancel_rx: Option<Receiver<UiToAgentMessage>>,
    pub progress: Option<Arc<Mutex<HeadlessSubAgentProgress>>>,
}

// velocity-mcp/src/agent/executor/headless.rs
pub fn run_headless_subagent(request: HeadlessSubAgentRequest) -> HeadlessSubAgentResult;

// velocity-mcp/src/agent/executor/thread.rs
pub fn run_agent_thread(workspace_root: PathBuf, ui_rx: Receiver<UiToAgentMessage>, ui_tx: Sender<AgentToUiMessage>);
```

---

## velocity-mcp: Registry

```rust
// velocity-mcp/src/registry/mod.rs
pub fn call_tool(name: &str, arguments: &Value) -> Result<String, Box<dyn Error>>;
pub fn call_tool_in_workspace(root: &Path, name: &str, arguments: &Value) -> Result<String, Box<dyn Error>>;
pub fn get_tools() -> Vec<ToolDefinition>;
```

---

## velocity-mcp: Editor

```rust
// velocity-mcp/src/editor/app/velocity_app/struct_def.rs
pub struct VelocityApp {
    pub agent_tx: Sender<UiToAgentMessage>,
    pub agent_rx: Receiver<AgentToUiMessage>,
    pub workspace_root: PathBuf,
    pub tabs: Vec<Tab>,
    pub active_tab: Option<TabId>,
    pub buffers: HashMap<TabId, EditorBuffer>,
    pub dock_state: Option<DockState<Tab>>,
    pub chat: ChatPanelState,
    pub provider: AiProvider,
    pub selected_model: String,
    pub thinking_enabled: bool,
    pub auto_approve: bool,
    pub appearance: AppearanceSettings,
    pub orchestrator: OrchestratorPanel,
    pub mission_control: MissionControlState,
    // ... 60+ more fields (see struct_def.rs for full definition)
}

impl VelocityApp {
    pub fn new(cc: &eframe::CreationContext, workspace_root: PathBuf,
               agent_tx: Sender<UiToAgentMessage>, agent_rx: Receiver<AgentToUiMessage>,
               gpu_name: String, mediator: Arc<MediatorArena>) -> Self;
    pub fn set_work_mode(&mut self, profile: WorkspaceProfile);
    pub fn save_workspace_preferences(&mut self);
    pub fn restore_workspace_preferences(&mut self);
    pub fn apply_workspace_profile(&mut self, profile: WorkspaceProfile);
    pub fn palette(&self) -> IdePalette;
}
```

---

## velocity-mcp: Orchestrator

```rust
// velocity-mcp/src/orchestrator/mod.rs
pub struct TaskId(pub u64);
```

---

## velocity-ide: SiteMap

```rust
// velocity-ide/src/site_map/mod.rs
pub struct SiteMap { ... }

impl SiteMap {
    pub fn open(dir: &Path, flags: u32) -> Result<Self>;
    pub fn register_string(&mut self, s: &str) -> Result<u64>;
    pub fn put_node(&mut self, node: &NdaNode) -> Result<()>;
    pub fn put_file_snapshot(&mut self, file: &str, triples: &[VcTriple]) -> Result<()>;
    pub fn remove_file_snapshot(&mut self, file: &str) -> Result<()>;
    pub fn flush(&mut self) -> Result<()>;
}

pub struct VcTriple {
    pub subject_hash: u64,
    pub predicate_id: u16,
    pub object_hash: u64,
}

pub enum NdaNode {
    Triple { subject_hash: u64, predicate_id: u16, object_hash: u64 },
}
```

---

## velocity-ide: Wiki

```rust
// velocity-ide/src/wiki/generate.rs
pub fn build_wiki(sm: &SiteMap) -> WikiModel;

// velocity-ide/src/wiki/markdown.rs
pub fn export_markdown(model: &WikiModel, dir: &Path) -> Result<usize>;
```

---

## velocity-browser: Public Re-exports

```rust
// velocity-browser/src/lib.rs (selected re-exports)
pub use dom::{SlabDomTree, MutationBatcher, NativeMutationObserver, CustomElementRegistry, ...};
pub use layout::{FlexLayoutEngine, GridTrackSolver, ParallelLayoutEngine, LayoutBox, ...};
pub use js::{JsVirtualMachine, JsEventLoopScheduler, WasmInterpreter, WebWorkerPool, ...};
pub use net::{HttpClient, QuicConnection, NativeWsClient, TlsFingerprintRotator, WebRtcTransport, ...};
pub use agentic::{AgenticAomTree, ActionPredictorEngine, VelocityOcrEngine, ZeroAllocNdaWriter, ...};
pub use nda::{NdaDictionary, NdaDocument, NdaFact, NdaObject, NdaTriple};
pub use parser::{HtmlParser, Html5Tokenizer, FastCssParser, StreamJitTokenizer, ...};
pub use session::BrowserSession;
pub use session_cookie_store::{CookieStore, CookieRecord, SameSitePolicy};
pub use session_history::{HistoryStack, HistoryItem};
pub use session_storage::SessionStorageDisk;
pub use session_indexeddb::{IndexedDbStorage, IndexedDbRecord};
pub use session_swarm::SwarmSessionOrchestrator;
pub use style::{StyleCascader, CssAnimation, FontShaperEngine, ScopedCssMatcher, Specificity, ...};
```

---

## See Also

- [System Overview](../architecture/system_overview.md) — Thread model and IPC topology
- [velocity-mcp: Agent Loop](../architecture/velocity_mcp.md) — Agent data flow
- [velocity-ide: Compiler & SiteMap](../architecture/velocity_ide.md) — Compilation pipeline
