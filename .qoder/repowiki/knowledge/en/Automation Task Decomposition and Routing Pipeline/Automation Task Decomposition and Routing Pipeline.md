# Automation Task Decomposition and Routing Pipeline

## Classification
- **Category**: Architecture / Pipeline
- **Files**: velocity-mcp/src/automation/ (14 files)
- **Criticality**: Critical — bridges user goals to parallel multi-agent execution

## Summary

The automation module decomposes user goals into parallel routed sub-agent tasks using SiteMap coupling analysis, model quality ranking, and specialist instruction templates. It is the bridge between Mission Control's goal input and the orchestrator's DAG-based parallel execution.

## Pipeline Flow

```
Goal (chat input or mission brief)
    │
    ▼
infer_task_kind_from_goal() → AgentTaskKind
    │
    ▼
WorkspaceCoordinator.plan_routed_tasks()
    ├── Select DecompositionPolicy by task kind
    ├── partition_files_by_coupling() via SiteMap CALLS/DECLARES graph
    ├── rank_candidates() by model quality for task kind
    └── build_execution_contract() per partition
    │
    ▼
OrchestratorPanel.set_routed_tasks() → TaskGraph
    │
    ▼
Auto-execute: spawn parallel workers with headless sub-agents
```

## AgentTaskKind (8 kinds)

| Kind | Default Decomposition | Specialist Template |
|------|----------------------|--------------------|
| Refactor | CoupledComponents | refactor-guardian |
| BugFix | IsolatedFiles | bugfix-responder |
| Test | IsolatedFiles | test-hardener |
| Documentation | IsolatedFiles | docs-curator |
| Analysis | IsolatedFiles | analysis-cartographer |
| Planning | SequentialPipeline | planning-architect |
| Merge | CoupledComponents | merge-mediator |
| DesktopAutomation | IsolatedFiles | desktop-wa-operator |

## DecompositionStyle (3 strategies)

- **IsolatedFiles**: Each file is an independent task
- **CoupledComponents**: Files sharing SiteMap graph edges grouped together
- **SequentialPipeline**: Tasks executed in dependency order

## Key Types

- `WorkspaceCoordinator` — orchestrates plan_routed_tasks() and execute_parallel_tasks()
- `SiteMapTaskRouter` — coupling analysis, model ranking, execution contracts
- `RoutedSubAgentTask` — one decomposed task with provider/model/scope/contract
- `ExecutionContract` — versioned contract with scope, fallback chain, expectations
- `InstructionRegistry` — template/policy lookup

## Key Files

| File | Lines | Purpose |
|------|-------|---------|
| `coordinator.rs` | 203 | WorkspaceCoordinator: plan + execute |
| `task_router.rs` | 659 | SiteMapTaskRouter: coupling, ranking, contracts |
| `instruction_registry/types.rs` | 123 | AgentTaskKind, DecompositionStyle, DecompositionPolicy |
| `instruction_registry/defaults.rs` | 188 | 8 default templates + 8 default policies |
| `model_quality.rs` | 331 | Model quality scoring per task kind |
| `mediator.rs` | 476 | MediatorArena: file-level presence locking |
