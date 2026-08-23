# velocity-mcp: Agent Loop & Orchestrator

The `velocity-mcp` crate's agent subsystem implements a 4-provider autonomous reasoning loop with tool dispatch, team routing, DAG-based task orchestration, and worktree isolation for parallel sub-agent execution.

---

## Agent Loop Data Flow

The agent loop is the core AI execution engine. It runs on a dedicated thread (`run_agent_thread`) and communicates with the UI via crossbeam channels.

### Entry Point

```
velocity-mcp/src/agent/executor/thread.rs
  └── run_agent_thread(workspace_root, ui_rx, ui_tx)
        └── Main loop: receives UiToAgentMessage, dispatches to loop_runner
```

### Loop Runner (`executor/loop_runner.rs`)

The loop runner executes the reasoning cycle:

1. **Build request**: Assemble system prompt, chat history, tool definitions, and user message
2. **Dispatch to provider**: Send request to selected `AiProvider`
3. **Stream response**: Process `OutputToken` chunks, accumulate assistant content
4. **Detect tool calls**: Parse tool call JSON from response
5. **Execute or approve**: If `auto_approve` is enabled, execute immediately; otherwise request UI approval
6. **Feed result back**: Append tool result to context, loop to step 2
7. **Terminate**: When no tool calls remain, emit `AgentFinished`

### Headless Sub-Agents (`executor/headless.rs`)

For parallel task execution, `run_headless_subagent()` spawns an isolated agent instance:

```rust
pub struct HeadlessSubAgentRequest {
    pub workspace_root: PathBuf,
    pub provider: AiProvider,
    pub model: String,
    pub thinking: bool,
    pub prompt: String,
    pub cancel_rx: Option<Receiver<UiToAgentMessage>>,
    pub progress: Option<Arc<Mutex<HeadlessSubAgentProgress>>>,
}
```

Headless agents share the same dispatch logic but have no UI. Progress is reported via shared `Arc<Mutex<HeadlessSubAgentProgress>>` containing status updates, transcript, changed files, and operator notes.

---

## Provider Failover Chain

Velocity supports 7 AI providers with automatic failover on quota exhaustion or network timeout:

### AiProvider Enum (`agent/models.rs`)

```rust
pub enum AiProvider {
    CloudflareWorkersAi,  // Default, free-tier Kimi K2.7
    OpenRouter,           // Multi-model gateway
    AzureOpenAi,          // Enterprise GPT-4o / o1
    LocalOllama,          // Local inference (llama3.2, qwen2.5-coder, deepseek-r1)
    OpenAI,               // Direct OpenAI API
    Anthropic,            // Direct Anthropic API
    GoogleVertex,         // Google Vertex AI
}
```

### Failover Order

```rust
pub fn fallback_provider(current: AiProvider) -> AiProvider {
    match current {
        AiProvider::CloudflareWorkersAi => AiProvider::OpenRouter,
        AiProvider::OpenRouter          => AiProvider::AzureOpenAi,
        AiProvider::AzureOpenAi         => AiProvider::LocalOllama,
        AiProvider::LocalOllama         => AiProvider::CloudflareWorkersAi,
        AiProvider::OpenAI              => AiProvider::CloudflareWorkersAi,
        AiProvider::Anthropic           => AiProvider::CloudflareWorkersAi,
        AiProvider::GoogleVertex        => AiProvider::CloudflareWorkersAi,
    }
}
```

### Default Models per Provider

| Provider | Default Model | API Style |
|----------|---------------|-----------|
| CloudflareWorkersAi | `@cf/moonshotai/kimi-k2.7-code` | OpenAiTools |
| OpenRouter | `tencent/hy3:free` | OpenAiChat |
| AzureOpenAi | `gpt-4o` | OpenAiTools |
| LocalOllama | `llama3.2` | OpenAiTools |
| OpenAI | `gpt-4o` | OpenAiTools |
| Anthropic | `claude-3-5-sonnet-20241022` | OpenAiTools |
| GoogleVertex | `gemini-1.5-pro` | OpenAiTools |

### Model Catalog Fetching

- **Cloudflare**: `fetch_model_catalog()` queries `/accounts/{id}/ai/models/search`
- **OpenRouter**: `fetch_openrouter_models()` queries `/api/v1/models` with account rotation
- **Azure**: `fetch_azure_models()` returns configured deployments or defaults
- **Ollama**: `fetch_local_ollama_models()` returns hardcoded local model list

Model inference (`infer_model_info()`) detects tool support and thinking capability from model ID patterns (e.g., `deepseek-r1`, `o1-`, `qwq` → thinking supported).

---

## Team Router & Expert Teams

### Team Routing (`executor/team_routing.rs` & `editor/team_router.rs`)

The team router parses natural language instructions for `@team` or `/team` directives and routes domain-specific tasks to dedicated agent personas.

**Routing patterns**:
- `@browser-agent` → Browser automation specialist
- `@backend-agent` → Backend/code engineering
- `@ui-agent` → UI/frontend specialist
- Custom teams loaded from `expert_teams.nda` or `.velocity_teams.json`

### Expert Team Structure (`editor/expert_team.rs`)

Teams are loaded at startup via `load_expert_teams(&workspace_root)` and stored in `VelocityApp::expert_teams`. Each team has:
- Designated roles and system prompts
- Tool access permissions
- Member-specific knowledge stores (`AgentMemoryManager`)

---

## DAG Task Scheduling

The orchestrator (`velocity-mcp/src/orchestrator/`) decomposes large projects into parallel tasks using a Directed Acyclic Graph.

### Module Structure

```
orchestrator/
├── mod.rs          # TaskId definition, module declarations
├── blueprint.rs    # Task graph construction from project requirements
├── scheduler.rs    # DAG-based parallel task dispatch
├── reconcile.rs    # Collision detection and merge reconciliation
├── validator.rs    # Per-task output validation
├── worker/         # Worker thread pool and worktree management
└── registry.rs     # Task registry and status tracking
```

### Task Lifecycle

```
PENDING → RUNNING → COMPLETED
                  → FAILED → (retry or abort)
         → BLOCKED (waiting on dependency)
```

### Scheduler Behavior

1. **Blueprint construction**: Parse project requirements into task nodes with dependencies
2. **Topological sort**: Determine execution order respecting dependency edges
3. **Parallel dispatch**: Launch independent tasks on worker threads simultaneously
4. **Reconciliation**: When multiple tasks modify overlapping files, detect collisions and merge
5. **Validation**: Verify each task's output against acceptance criteria

---

## Worktree Isolation & Lock Manager

### Purpose

When multiple sub-agents work in parallel, file access collisions must be prevented. The worktree system provides directory-scoped locking.

### Lock Acquisition

1. Before assigning a task, the scheduler acquires read/write locks for the task's directory scope
2. Locks are tracked by `TaskId` and file path prefix
3. Out-of-scope file creation is detected and flagged

### NDA Execution Facts

File edit contracts and snapshot diffs are recorded into NDA binary artifacts:
- `.velocity/execution_facts.nda` — per-task edit contracts
- Enables post-run auditing and replay

### MediatorArena (`automation/mediator.rs`)

The `MediatorArena` provides file-level presence locking for the UI↔agent boundary:

```rust
// In main.rs:
let mediator = std::sync::Arc::new(automation::MediatorArena::new());

// Presence lock with TTL:
mediator.prune_stale_locks(Duration::from_secs(2));
mediator.release_locks_for_agent(&agent_id);
mediator.acquire_lock(file_path, line_range, agent_id, &site_map_guard);
```

When a conflict is detected (agent and user editing the same lines), `resolve_conflict()` generates a warning message for the UI.

---

## Automation Subsystem

The `automation/` module provides background watchers, coordination, and the full task decomposition pipeline:

| Module | Purpose |
|--------|--------|
| `coordinator.rs` | `WorkspaceCoordinator`: `plan_routed_tasks()`, `execute_parallel_tasks()` |
| `task_router.rs` | `SiteMapTaskRouter`: coupling analysis, model ranking, execution contracts |
| `instruction_registry/` | Task kind taxonomy (8 kinds), decomposition policies, specialist templates |
| `watcher.rs` | AST file watcher — monitors source files, sends updates via shmem |
| `build_runner.rs` | Build watcher — polls `cargo check` output, updates diagnostics |
| `mediator.rs` | File presence locking, conflict resolution |
| `model_quality.rs` | Model output quality scoring per task kind |
| `tester.rs` | Self-check automation (`--check` flag) |
| `site_map_support.rs` | SiteMap integration helpers |

### Task Decomposition Pipeline

The full pipeline from goal to parallel execution:

```
1. Goal received (chat input or mission brief)
       │
       ▼
2. infer_task_kind_from_goal() → AgentTaskKind
   (Refactor | BugFix | Test | Documentation | Analysis | Planning | Merge | DesktopAutomation)
       │
       ▼
3. WorkspaceCoordinator.plan_routed_tasks()
       │
       ├── Select DecompositionPolicy by task kind
       │   (e.g., Refactor → CoupledComponents, BugFix → IsolatedFiles)
       │
       ├── partition_files_by_coupling()
       │   Query SiteMap CALLS/DECLARES graph edges
       │   Group files sharing edges → coupled components
       │   Each component = one RoutedSubAgentTask
       │
       ├── rank_candidates()
       │   Rank models by quality score for task kind
       │   Produce fallback chain (best → fallback → local)
       │
       └── build_execution_contract()
           Versioned contract: scope, fallback chain, expectations
       │
       ▼
4. OrchestratorPanel.set_routed_tasks()
   build_routed_graph() → TaskGraph
       │
       ▼
5. Auto-execute: spawn parallel workers
   Each worker runs headless sub-agent with contract
```

### AgentTaskKind & Decomposition Policies

| Task Kind | Decomposition Style | Specialist Template |
|-----------|-------------------|--------------------|
| Refactor | CoupledComponents | refactor-guardian |
| BugFix | IsolatedFiles | bugfix-responder |
| Test | IsolatedFiles | test-hardener |
| Documentation | IsolatedFiles | docs-curator |
| Analysis | IsolatedFiles | analysis-cartographer |
| Planning | SequentialPipeline | planning-architect |
| Merge | CoupledComponents | merge-mediator |
| DesktopAutomation | IsolatedFiles | desktop-wa-operator |

### AST Watcher Flow

```
File change detected
    → spawn_ast_watcher() picks up event
    → Sends TelemetryRequest::AstUpdate via shared memory
    → TelemetryServer receives, locks SiteMap mutex
    → persist_ast_update() writes triples to SiteMap
    → SiteMap.flush() persists to disk
    → Latency recorded in TELEMETRY_LATENCY_US atomic
```

### Build Watcher Flow

```
spawn_build_watcher(workspace_root, interval_secs=5)
    → Polls cargo check every 5 seconds
    → Parses errors/warnings
    → Updates build_diagnostics.json
    → UI polls for diagnostic updates
```
