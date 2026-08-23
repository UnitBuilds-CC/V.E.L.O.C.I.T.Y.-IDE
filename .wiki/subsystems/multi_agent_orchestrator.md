# Multi-Agent Task Orchestrator

The orchestrator in `velocity-mcp` manages multi-agent task execution via a DAG scheduler, worktree filesystem isolation, dependency resolution, team routing, mission control tracking, and a full automation pipeline that decomposes goals into parallel routed sub-agent tasks.

> For full architecture details including thread model, IPC, and automation subsystem, see [velocity-mcp: Agent Loop & Orchestrator](../architecture/velocity_mcp.md).

---

## Quick Reference

### Module Structure

```
orchestrator/
├── mod.rs          # TaskId(u64) definition
├── blueprint.rs    # Task graph construction (TaskGraph DAG)
├── scheduler.rs    # DAG-based parallel dispatch (topological sort)
├── reconcile.rs    # Collision detection and merge
├── validator.rs    # Per-task output validation
├── worker/         # Worker thread pool, worktree management
│   ├── mod.rs      # LiveWorkerHandle, worker spawning
│   └── runner.rs   # spawn_live_worker(), run_assignment(), execute_live_task()
└── registry.rs     # Task registry and status tracking (TaskStatus enum)
```

### Automation Pipeline (Task Decomposition & Routing)

```
automation/
├── mod.rs                  # Module root, public re-exports
├── coordinator.rs          # WorkspaceCoordinator: plan_routed_tasks(), execute_parallel_tasks()
├── task_router.rs          # SiteMapTaskRouter: coupling analysis, model ranking, execution contracts
├── instruction_registry/   # Task kind taxonomy, decomposition policies, specialist templates
│   ├── mod.rs              # InstructionRegistry facade
│   ├── types.rs            # AgentTaskKind, DecompositionStyle, DecompositionPolicy
│   ├── defaults.rs         # 8 default templates + 8 default policies
│   ├── registry.rs         # Template/policy lookup
│   └── nda_format.rs       # NDA serialization for instruction artifacts
├── mediator.rs             # MediatorArena: file-level presence locking
├── model_quality.rs        # Model output quality scoring per task kind
├── build_runner.rs         # Build watcher: cargo check polling
├── watcher.rs              # AST file watcher: shmem-based update persistence
├── tester.rs               # Self-check automation
└── site_map_support.rs     # SiteMap integration helpers
```

### Task Lifecycle

```
PENDING → RUNNING → COMPLETED
                  → FAILED → (retry or abort)
         → BLOCKED (waiting on dependency)
```

### Editor Panel UI

```
editor/orchestrator/
├── mod.rs              # Panel entry
├── types.rs            # RoutedPlanState, OrchestratorDashboardSnapshot
├── tests.rs            # Panel tests
└── panel/
    ├── mod.rs
    ├── struct_def.rs   # OrchestratorPanel struct, set_routed_tasks()
    ├── execution.rs    # poll_live_workers(), retry_blocked_tasks(), stop_task()
    ├── policy_controls.rs # Worktree policy UI
    └── ui_render.rs    # render_task_card() with expert team assignment display
```

---

## Two Entry Paths

The multi-agent system has two distinct entry paths:

### Path 1: `@team` Directives (Single-Expert Routing)

Natural language team directives routed to a single best expert:

```
User: "@csharp-team fix the Blazor data grid binding issue"
  → parse_team_directive() extracts slug "csharp-team"
  → route_member() selects best member via hybrid scoring
  → compose_persona() builds per-member system message
  → run_agent_reasoning_loop() with member's provider/model
```

**Routing stages** (`editor/team_router.rs`):
1. **File-scope match**: Member whose `scope_patterns` match open files
2. **Keyword scoring**: role weight 2 + name weight 2 + scope weight 1 + skill weight 1
3. **LLM router fallback**: Ask model to pick best member
4. **Team lead fallback**: Default to team lead member

### Path 2: Mission Control Pipeline (Multi-Task Parallel Execution)

Full goal decomposition into parallel routed sub-agent tasks:

```
User: "Refactor the data access layer" (via Mission Control)
  → plan_routed_subagents() in agent_handlers.rs
  → infer_task_kind_from_goal() → AgentTaskKind::Refactor
  → WorkspaceCoordinator.plan_routed_tasks()
    → SiteMapTaskRouter.route_tasks()
      → Select DecompositionPolicy by task kind
      → partition_files_by_coupling() using SiteMap CALLS/DECLARES graph
      → rank_candidates() by model quality for task kind
      → Build ExecutionContract per partition
  → OrchestratorPanel.set_routed_tasks() builds TaskGraph
  → Auto-execute spawns parallel workers
```

---

## Automation & Task Decomposition

### WorkspaceCoordinator (`automation/coordinator.rs`)

The coordinator bridges goal decomposition with the orchestrator:

```rust
pub struct WorkspaceCoordinator;

impl WorkspaceCoordinator {
    pub fn plan_routed_tasks(
        workspace_root: &Path,
        goal: &str,
        scope_files: &[PathBuf],
        task_kind: AgentTaskKind,
    ) -> Vec<RoutedSubAgentTask>;

    pub fn execute_parallel_tasks(
        workspace_root: &Path,
        tasks: &[RoutedSubAgentTask],
    ) -> Vec<TaskResult>;
}
```

### SiteMapTaskRouter (`automation/task_router.rs`)

The task router decomposes work using SiteMap coupling analysis:

**File Coupling Partitioning** (`partition_files_by_coupling()`):
- Queries SiteMap for CALLS and DECLARES graph edges
- Groups files that share edges into coupled components
- Each component becomes one `RoutedSubAgentTask`

**Model Quality Ranking** (`rank_candidates()`):
- Ranks available models by quality score for the task kind
- Produces a fallback chain (best → fallback → local)
- Stored in `ExecutionContract` for worker execution

**Execution Contract** (`build_execution_contract()`):
- Versioned contract with scope files, fallback chain, expectations
- Worker uses this to select provider/model and enforce scope

### Instruction Registry (`automation/instruction_registry/`)

**AgentTaskKind** — 8 task taxonomies:

| Kind | Description | Default Decomposition |
|------|-------------|----------------------|
| `Refactor` | Code restructuring | CoupledComponents |
| `BugFix` | Defect resolution | IsolatedFiles |
| `Test` | Test creation/hardening | IsolatedFiles |
| `Documentation` | Docs generation | IsolatedFiles |
| `Analysis` | Code exploration | IsolatedFiles |
| `Planning` | Architecture design | SequentialPipeline |
| `Merge` | Branch reconciliation | CoupledComponents |
| `DesktopAutomation` | WA script execution | IsolatedFiles |

**DecompositionStyle** — 3 partitioning strategies:

| Style | Behavior |
|-------|----------|
| `IsolatedFiles` | Each file is an independent task |
| `CoupledComponents` | Files sharing graph edges grouped together |
| `SequentialPipeline` | Tasks executed in dependency order |

**Specialist Instruction Templates** — 8 default templates:

| Template ID | Role | System Prompt Focus |
|-------------|------|--------------------|
| `planning-architect` | Architect | Decompose goals, identify dependencies |
| `refactor-guardian` | Refactorer | Preserve behavior, improve structure |
| `bugfix-responder` | Debugger | Root-cause analysis, minimal fix |
| `test-hardener` | QA Engineer | Edge cases, regression coverage |
| `analysis-cartographer` | Analyst | Map code relationships, document |
| `docs-curator` | Technical Writer | Clear, accurate documentation |
| `merge-mediator` | Merger | Resolve conflicts, integrate changes |
| `desktop-wa-operator` | WA Operator | Windows automation execution |

---

## DAG Task Scheduling

Tasks are organized into a Directed Acyclic Graph:

```
                    ┌─────────────────────────┐
                    │    Root Mission Task    │
                    └────────────┬────────────┘
                                 │
           ┌─────────────────────┴─────────────────────┐
           ▼                                           ▼
┌─────────────────────┐                     ┌─────────────────────┐
│  Worker Task A      │                     │  Worker Task B      │
│  (Frontend/UI)      │                     │  (Backend Engine)   │
└──────────┬──────────┘                     └──────────┬──────────┘
           │                                           │
           └─────────────────────┬─────────────────────┘
                                 ▼
                    ┌─────────────────────────┐
                    │  Reconciliation Task    │
                    │  (Integration & Verify) │
                    └─────────────────────────┘
```

### TaskGraph (`orchestrator/blueprint.rs`)

```rust
pub struct Task {
    pub id: TaskId,
    pub title: String,
    pub description: String,
    pub scope: Vec<PathBuf>,
    pub dependencies: Vec<TaskId>,
    pub output: Option<String>,
}

pub struct TaskGraph {
    tasks: HashMap<TaskId, Task>,
}

impl TaskGraph {
    pub fn ready(&self, completed: &HashSet<TaskId>) -> Vec<TaskId>;
    pub fn dependents(&self, id: TaskId) -> Vec<TaskId>;
    pub fn leaves(&self) -> Vec<TaskId>;
}
```

### Scheduler (`orchestrator/scheduler.rs`)

`plan()` builds phase-based execution plan via topological sort:
- Tasks in the same phase are independent and can run in parallel
- Cycle detection via strongly connected components

---

## Worker Execution

### spawn_live_worker (`orchestrator/worker/runner.rs`)

Workers execute in separate threads with full isolation:

1. **Acquire scope locks** via MediatorArena
2. **Snapshot files** before modification
3. **Execute task** via `execute_live_task()` → headless sub-agent
4. **Detect changes** via file diff
5. **Write execution facts** to `.velocity/execution_facts.nda`
6. **Release locks**

### ContinuationLedger (`editor/continuation_ledger.rs`)

Cross-model handoff context for sequential tasks:
- Records what each worker produced
- Next worker receives predecessor's output as context
- Enables multi-model pipelines (e.g., planner → coder → tester)

---

## Worktree Directory Lock Manager

Prevents multi-agent file access collisions during parallel execution:

- **Directory Scope Locking**: Read/write file locks per directory prefix before task assignment
- **Out-of-Scope Detection**: Flags file creations outside an agent's assigned scope
- **NDA Execution Facts**: Records edit contracts to `.velocity/execution_facts.nda`

### Collision Detection (`orchestrator/reconcile.rs`)

```rust
pub fn detect_collisions(tasks: &[Task]) -> Vec<(PathBuf, Vec<TaskId>)>;
pub fn scope_violations(task: &Task, changed_files: &[PathBuf]) -> Vec<PathBuf>;
```

---

## Team Router & Expert Teams

### Routing (`executor/team_routing.rs` & `editor/team_router.rs`)

- Parses `@team` and `/team` directives from natural language
- Routes to specialized agent personas: `@browser-agent`, `@backend-agent`, `@ui-agent`
- Loads team definitions from `expert_teams.nda` or `.velocity_teams.json`

### Expert Team Structure (`editor/expert_team.rs`)

Teams are loaded at startup via `load_expert_teams(&workspace_root)`:

```rust
pub struct ExpertMember {
    pub id: String,
    pub name: String,
    pub role: String,
    pub provider: Option<AiProvider>,
    pub model_id: Option<String>,
    pub skills: Vec<String>,
    pub scope_patterns: Vec<String>,
    pub tools: Vec<String>,
    pub workflow_instructions: Option<String>,
}

pub struct ExpertTeam {
    pub id: String,
    pub name: String,
    pub description: String,
    pub members: Vec<ExpertMember>,
    pub is_preset: bool,
}
```

**3 Preset Teams**:
- **C# Software Team**: Lead Architect, Backend Developer, EF Data Specialist, NUnit QA
- **Android App Development Team**: Platform specialists
- **Doccit Maintenance Team**: Documentation specialists

### Team Studio UI (`editor/app/team_studio_ui.rs`)

Full team management interface:
- Gallery with expandable team cards
- Member cards showing provider/model/skills/instructions
- Team creation wizard
- Team Builder chat for natural language team creation
- Team activity log

---

## Mission Control & Task Timeline

### MissionControlState (`mission_control.rs`)

```rust
pub struct MissionControlState {
    pub brief: Option<String>,
    pub interventions: Vec<Intervention>,
    pub auto_execute: bool,
    pub selected_task_id: Option<TaskId>,
}
```

- Real-time agent status monitoring
- Intervention queue for approval flow
- Build error count display
- Auto-execute mode for hands-off operation

### TaskTimelineState (`task_timeline.rs`)

- Zero-allocation event ring buffer
- NDA-persisted mission activity log
- Session markers with descriptions
- Agent status change events

---

## plan_routed_subagents() — The Full Pipeline

Located in `editor/app/velocity_app/agent_handlers.rs`, this is the bridge function:

```
1. Get goal from chat input or mission brief
       │
       ▼
2. infer_task_kind_from_goal() → AgentTaskKind
       │
       ▼
3. Collect scope files (open editors or workspace-wide)
       │
       ▼
4. WorkspaceCoordinator.plan_routed_tasks()
   → SiteMapTaskRouter.route_tasks()
   → Coupling analysis + model ranking + execution contracts
       │
       ▼
5. OrchestratorPanel.set_routed_tasks()
   → build_routed_graph() converts to TaskGraph
       │
       ▼
6. If auto_execute enabled:
   → poll_live_workers() spawns ready tasks
   → Each worker runs headless sub-agent with contract
       │
       ▼
7. render_task_card() shows assigned expert from active team
```

---

## See Also

- [Agent Loop & Orchestrator Architecture](../architecture/velocity_mcp.md) — Full data flow, provider failover, automation subsystem
- [Editor & IDE UI](../architecture/editor_ide_ui.md) — Orchestrator panel and mission control UI
- [Agent Reasoning & Self-Improvement](agent_reasoning_planning.md) — Tree-of-thought, planning, self-improvement engine
- [Drone Subsystem](drone_subsystem.md) — Portable agent endpoint for cross-device collaboration
