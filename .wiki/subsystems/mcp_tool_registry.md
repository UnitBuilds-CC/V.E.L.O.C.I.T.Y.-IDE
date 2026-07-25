# MCP Tool Registry

The `velocity-mcp` crate hosts a Model Context Protocol (MCP) server that exposes native tools to AI agents. The registry handles tool registration, schema generation, argument validation, and invocation dispatch.

> For full tool inventory and dispatch architecture, see [velocity-mcp: Tool Registry & Windows Automation](../architecture/tool_registry_wa.md).

---

## Quick Reference

### Module Structure

```
registry/
├── mod.rs              # Public API: call_tool(), get_tools()
├── dispatch.rs         # call_tool_in_workspace() dispatch
├── parsers.rs          # Argument parsing helpers
├── types.rs            # Tool definition types
├── tool_definitions/   # JSON Schema tool definitions
├── system_tools.rs     # File, search, terminal tools
├── browser_tools/      # Web navigation, AOM, screenshots
├── team_tools.rs       # Team creation, expert management
├── wa_tools.rs         # Windows automation tools
└── tests/              # Per-category test suites
```

### Tool Categories

| Category | Module | Tools |
|----------|--------|-------|
| System | `system_tools.rs` | `read_file`, `write_to_file`, `replace_file_content`, `multi_replace_file_content`, `run_command`, `list_dir`, `grep_search` |
| Browser | `browser_tools/` | `browser_navigate`, `browser_click`, `browser_type`, `browser_get_aom`, `browser_take_screenshot`, `browser_workflow_record`, `browser_workflow_play` |
| Team | `team_tools.rs` | `create_expert_team`, `list_expert_teams`, `create_skill_file` |
| Windows Automation | `wa_tools.rs` | `wa_click`, `wa_type`, `wa_capture`, `wa_run_script` |

### Dispatch Flow

```
1. Agent sends tool call (name + JSON arguments)
       │
       ▼
2. call_tool_in_workspace() matches name against get_tools() definitions
       │
       ▼
3. Argument validation against JSON Schema
       │
       ▼
4. Route to handler: system_tools / browser_tools / team_tools / wa_tools
       │
       ▼
5. Execute with workspace path sandboxing
       │
       ▼
6. Return JSON result string
```

---

## See Also

- [Tool Registry & Windows Automation Architecture](../architecture/tool_registry_wa.md) — Full dispatch details
- [Agent Loop & Orchestrator](../architecture/velocity_mcp.md) — How tools are called by the agent
