# Expert Team System and Task Routing

## Classification
- **Category**: Architecture / Multi-Agent
- **Files**: editor/expert_team.rs (559 LOC), editor/team_router.rs (444 LOC), agent/executor/team_routing.rs (313 LOC), editor/app/team_studio_ui.rs (402 LOC)
- **Criticality**: High — enables domain-specific agent routing

## Summary

The Expert Team system allows defining specialized agent teams with named members, each having distinct roles, provider/model assignments, skills, scope patterns, and workflow instructions. Tasks are routed to the best-matching team member via a 4-stage hybrid scoring system.

## Two Entry Paths

### Path 1: @team Directives (Single-Expert Routing)
```
"@csharp-team fix the Blazor data grid" → parse_team_directive()
  → route_member() → 4-stage hybrid scoring
  → compose_persona() → per-member system message
  → run_agent_reasoning_loop() with member's provider/model
```

### Path 2: Mission Control Pipeline (Multi-Task Parallel)
```
"Refactor the data layer" → plan_routed_subagents()
  → WorkspaceCoordinator.plan_routed_tasks()
  → OrchestratorPanel.set_routed_tasks()
  → Auto-execute parallel workers
```

## Routing Stages (4-stage hybrid)

1. **File-scope match**: Member whose `scope_patterns` match open files
2. **Keyword scoring**: role weight 2 + name weight 2 + scope weight 1 + skill weight 1
3. **LLM router fallback**: Ask model to pick best member
4. **Team lead fallback**: Default to team lead member

## ExpertTeam & ExpertMember

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

## Preset Teams

| Team | Members | Focus |
|------|---------|-------|
| C# Software Team | Lead Architect, Backend Developer, EF Data Specialist, NUnit QA | Blazor/ERP development |
| Android App Team | Platform specialists | Android development |
| Doccit Maintenance Team | Documentation specialists | Documentation maintenance |

## Team Studio UI

- Gallery with expandable team/member cards
- Team Builder Chat for natural language team creation
- Team activity log
- Persistence at `.velocity/expert_teams.nda` (encrypted)

## Key Files

| File | Purpose |
|------|---------|
| `editor/expert_team.rs` | Team/member definitions, load/save, find_expert_for_task() |
| `editor/team_router.rs` | Directive parsing, route_member(), resolve_team() |
| `agent/executor/team_routing.rs` | try_route_team_prompt(), compose_persona() |
| `editor/app/team_studio_ui.rs` | Full Team Studio UI |
| `editor/app/team_manager.rs` | launch_team(), cancel_running() |
| `editor/team_builder_chat.rs` | Natural language team creation |
