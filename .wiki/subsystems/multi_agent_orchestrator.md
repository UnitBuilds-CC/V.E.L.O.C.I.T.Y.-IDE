# Multi-Agent Task Orchestrator

The orchestrator in `velocity-mcp` manages multi-agent task execution via a DAG scheduler, worktree filesystem isolation, dependency resolution, team routing, and mission control tracking.

> For full architecture details including thread model, IPC, and automation subsystem, see [velocity-mcp: Agent Loop & Orchestrator](../architecture/velocity_mcp.md).

---

## Quick Reference

### Module Structure

```
orchestrator/
├── mod.rs          # TaskId(u64) definition
├── blueprint.rs    # Task graph construction
├── scheduler.rs    # DAG-based parallel dispatch
├── reconcile.rs    # Collision detection and merge
├── validator.rs    # Per-task output validation
├── worker/         # Worker thread pool, worktree management
└── registry.rs     # Task registry and status tracking
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
├── types.rs            # UI types
├── tests.rs            # Panel tests
└── panel/
    ├── mod.rs
    ├── struct_def.rs   # OrchestratorPanel struct
    ├── execution.rs    # Task execution controls
    ├── policy_controls.rs # Worktree policy UI
    └── ui_render.rs    # Panel rendering
```

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

---

## Worktree Directory Lock Manager

Prevents multi-agent file access collisions during parallel execution:

- **Directory Scope Locking**: Read/write file locks per directory prefix before task assignment
- **Out-of-Scope Detection**: Flags file creations outside an agent's assigned scope
- **NDA Execution Facts**: Records edit contracts to `.velocity/execution_facts.nda`

---

## Team Router & Expert Teams

### Routing (`executor/team_routing.rs` & `editor/team_router.rs`)

- Parses `@team` and `/team` directives from natural language
- Routes to specialized agent personas: `@browser-agent`, `@backend-agent`, `@ui-agent`
- Loads team definitions from `expert_teams.nda` or `.velocity_teams.json`

### Expert Team Structure (`editor/expert_team.rs`)

- Designated roles and system prompts per team
- Tool access permissions
- Per-member knowledge stores (`AgentMemoryManager`)

---

## Mission Control & Task Timeline

### MissionControlState (`mission_control.rs`)

- Real-time agent status monitoring
- Intervention queue for approval flow
- Build error count display

### TaskTimelineState (`task_timeline.rs`)

- Zero-allocation event ring buffer
- NDA-persisted mission activity log
- Session markers with descriptions
- Agent status change events

---

## See Also

- [Agent Loop & Orchestrator Architecture](../architecture/velocity_mcp.md) — Full data flow, provider failover, automation subsystem
- [Editor & IDE UI](../architecture/editor_ide_ui.md) — Orchestrator panel and mission control UI
