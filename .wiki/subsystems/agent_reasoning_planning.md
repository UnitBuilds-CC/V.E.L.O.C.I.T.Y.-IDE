# Agent Reasoning, Planning & Self-Improvement

The `agent/` module within `velocity-mcp` (28 source files) implements the core AI reasoning engine: tree-of-thought exploration, multi-step task planning with validation, persistent memory for cross-session learning, and a self-improvement engine that analyzes failures and refines prompts.

---

## Agent Reasoning Engine

### Tree of Thought (`agent/reasoning.rs`)

Structured reasoning for complex problems — explore multiple solution paths in parallel before committing to execution:

```rust
pub struct Thought {
    pub id: String,
    pub content: String,
    pub parent: Option<String>,
    pub children: Vec<String>,
    pub confidence: f32,          // 0.0 to 1.0
    pub evaluation: ThoughtEvaluation,
    pub depth: usize,
    pub explored: bool,
}

pub enum ThoughtEvaluation {
    Promising,  // score: 0.8
    Neutral,    // score: 0.5
    Unlikely,   // score: 0.2
    Invalid,    // score: 0.0
}

pub struct ReasoningTree {
    pub problem: String,
    pub thoughts: HashMap<String, Thought>,
    pub roots: Vec<String>,
    pub max_depth: usize,
}
```

**Usage flow**:
1. Agent receives complex problem
2. Root thought(s) generated as entry points
3. Tree expanded by exploring children of promising thoughts
4. Each thought evaluated and scored
5. Best-scoring branch selected for execution

---

## Multi-Step Task Planner

### Plan Structure (`agent/planning.rs`)

```rust
pub struct PlanStep {
    pub id: String,
    pub description: String,
    pub action: String,
    pub depends_on: Vec<String>,
    pub confidence: f32,
    pub status: StepStatus,
    pub output: Option<String>,
    pub complexity: u8,    // 1 = trivial, 5 = very complex
    pub validated: bool,
}

pub enum StepStatus {
    Pending, InProgress, Completed, Failed, Skipped, Blocked,
}

pub struct Plan {
    pub goal: String,
    pub steps: Vec<PlanStep>,
    pub status: PlanStatus,
}
```

### Plan Decomposition (`decompose_task()`)

Heuristic task decomposition based on pattern matching:

| Pattern | Task Kind | Example |
|---------|-----------|---------|
| "implement X" | Refactor | "implement the data grid" |
| "fix X" | BugFix | "fix the null reference" |
| "test X" | Test | "test the auth module" |
| "document X" | Documentation | "document the API" |
| "analyze X" | Analysis | "analyze the dependencies" |

### Plan Validation

Each step is validated before execution:
- **Dependency check**: All `depends_on` steps must be `Completed`
- **Confidence threshold**: Steps below 0.3 confidence are flagged
- **Complexity gating**: Steps with complexity > 4 are split further

---

## Persistent Memory

### PersistentMemory (`agent/memory_store.rs`)

Cross-session learning store persisted to disk:

```rust
pub struct PersistentMemory {
    workspace: PathBuf,
    // Learned patterns, failure records, success patterns
}
```

**Stored data**:
- Learned directives (patterns that worked)
- Failure records (what went wrong and why)
- Success patterns (proven approaches)
- Session history summaries

### Memory Integration

At session start, the loop runner injects learned directives into the system prompt:

```rust
let learned_directives = ImprovementEngine::recall_directives(&memory, 5);
// Injected into system message under "## Previously Learned Patterns"
```

---

## Self-Improvement Engine

### ImprovementEngine (`agent/self_improve.rs`)

Analyzes agent failures and refines prompts for future sessions:

```rust
pub struct ImprovementEngine {
    memory: &'a PersistentMemory,
    // Failure analysis state
}
```

**Improvement cycle**:
1. Agent encounters failure (tool error, build failure, wrong output)
2. Engine analyzes root cause
3. Generates corrective directive
4. Directive stored in PersistentMemory
5. Next session loads directive into system prompt

**Directive recall**: Top-N directives by relevance injected at session start (default: 5).

---

## Agent Execution Loop

### run_agent_reasoning_loop (`agent/executor/loop_runner.rs`)

The core execution loop (1,280 lines) integrates all reasoning components:

```
1. Build request (system prompt + chat history + tools)
       │
       ▼
2. Inject learned directives from PersistentMemory
       │
       ▼
3. Dispatch to provider (with fallback chain)
       │
       ▼
4. Stream response → detect tool calls
       │
       ▼
5. If tool calls:
   ├── Create checkpoint (for rollback)
   ├── Execute tool via registry
   ├── Check build diagnostics (LSP-gated writes)
   ├── Feed result back to context
   └── Loop to step 3
       │
       ▼
6. On failure:
   ├── Self-improvement engine analyzes
   ├── Store failure directive
   ├── Attempt provider fallback
   └── Retry with refined prompt
       │
       ▼
7. On success:
   ├── Store success pattern
   └── Emit AgentFinished
```

**Key features**:
- **Max 15 loops** per reasoning cycle (configurable)
- **Checkpoint/rollback**: Failed file-modifying batches can be rolled back
- **LSP-gated writes**: Prevents repeated overwrites when build diagnostics report errors
- **Provider fallback**: Cloudflare → OpenRouter → Azure → LocalOllama (circular)

---

## Workspace Checkpointing

### CheckpointManager (`agent/checkpoint.rs`)

Safe, reversible tool operations via workspace checkpoints:

```rust
pub struct CheckpointManager {
    workspace_root: PathBuf,
    checkpoints: Vec<Checkpoint>,
}
```

- Creates git-stash-like snapshots before file-modifying operations
- `last_checkpoint_id` tracks the most recent checkpoint
- On batch failure: roll back to last checkpoint
- Enables safe experimentation without permanent changes

---

## Provider Management

### AiProvider & ModelInfo (`agent/models.rs`)

```rust
pub enum AiProvider {
    CloudflareWorkersAi,
    OpenRouter,
    AzureOpenAi,
    LocalOllama,
    OpenAI,
    Anthropic,
    GoogleVertex,
}

pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub supports_tools: bool,
    pub supports_thinking: bool,
    pub api_style: ApiStyle,
}
```

### Provider Dispatch (`agent/provider.rs`)

Handles API communication for each provider:
- Request formatting per API style (OpenAiTools, OpenAiChat)
- Response streaming and parsing
- Token counting and usage tracking
- Error handling and timeout management

### Model Inference (`infer_model_info()`)

Detects capabilities from model ID patterns:
- `deepseek-r1`, `o1-`, `qwq` → thinking supported
- `gpt-4`, `claude-3`, `kimi-k2` → tools supported
- Fallback to conservative defaults

---

## Peer-to-Peer System

### Module Structure

```
agent/
├── peer_link.rs      # Direct peer-to-peer communication link (1,361 lines)
├── peer_robust.rs    # Robust peer connection with retry (1,020 lines)
├── peer_server.rs    # Incoming peer connection handler (539 lines)
├── peer_bridge.rs    # Bridge between peer network and agent runtime (477 lines)
├── crypto.rs         # Encrypted peer communication (365 lines)
├── shared_memory.rs  # Shared memory for peer data exchange (513 lines)
├── collaboration.rs  # Real-time collaboration protocol (537 lines)
└── conflict_resolution.rs # Merge conflict resolution for concurrent edits (611 lines)
```

### Peer Communication

Peers communicate via encrypted channels:
- **peer_link**: Direct TCP/TLS connection between IDE instances
- **peer_robust**: Automatic reconnection with exponential backoff
- **peer_server**: Listens for incoming peer connections
- **peer_bridge**: Translates peer protocol messages to agent runtime commands

### Collaboration Protocol

Real-time collaboration between multiple IDE instances:
- Shared editing with conflict detection
- Cursor presence broadcasting
- File lock negotiation via MediatorArena
- Change propagation with causal ordering

---

## Headless Sub-Agents

### run_headless_subagent (`agent/executor/headless.rs`)

Isolated agent instances for parallel task execution:

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

Headless agents:
- Share the same dispatch logic as the main agent
- Have no UI — progress reported via shared state
- Used by the orchestrator for parallel task execution
- Support cancellation via `cancel_rx`

---

## Background Agents

### BackgroundAgents (`agent/background_agents.rs`)

Long-running agent processes that operate independently:
- Persist across IDE sessions
- Resume from checkpoints
- Report progress via background channels
- Used for large-scale analysis and refactoring tasks

---

## See Also

- [Multi-Agent Task Orchestrator](multi_agent_orchestrator.md) — DAG scheduling and team routing
- [velocity-mcp: Agent Loop & Orchestrator](../architecture/velocity_mcp.md) — Full data flow
- [System Overview](../architecture/system_overview.md) — Thread model and IPC topology
