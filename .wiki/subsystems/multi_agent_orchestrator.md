# Multi-Agent Task Orchestrator

The orchestrator in `velocity-mcp` manages multi-agent task execution, worktree filesystem isolation, dependency resolution, team routing, and mission control tracking.

---

## 🏛️ Task Graph DAG & Dependency Management

Tasks are organized into a Directed Acyclic Graph (DAG) managed by `velocity-mcp/src/editor/orchestrator/`:

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

## 🔒 Worktree Directory Lock Manager (`worktree.rs`)

To prevent multi-agent file access collisions during parallel execution:
- **Directory Scope Locking**: Acquires explicit read/write file locks per directory or file prefix before assigning a task to a subagent worker.
- **Out-of-Scope File Creation Detection**: Monitors file creations and flags operations that mutate files outside an agent's assigned scope.
- **NDA Execution Facts**: Records file edit contracts and snapshot diffs into NDA binary artifacts (`.velocity/execution_facts.nda`).

---

## 🤖 Team Router & Expert Teams (`team_router.rs` & `expert_team.rs`)

- **Routing Rules**: Parses natural language instructions and `@team` or `/team` directives.
- **Specialized Roles**: Routes domain-specific tasks to dedicated agent personas (e.g. `@browser-agent`, `@backend-agent`, `@ui-agent`).
- **Team Registry**: Dynamically loads team definitions from `expert_teams.nda` or `.velocity_teams.json`.

---

## 📊 Mission Control & Task Timeline (`task_timeline.rs`)

- Real-time event ring buffer (`test_ring_buffer_wrap`) capturing agent status changes (`PENDING`, `RUNNING`, `BLOCKED`, `COMPLETED`, `FAILED`).
- Visualizes worker logs, task notes, retry states, and artifact links directly inside Velocity IDE's Mission Control view.
