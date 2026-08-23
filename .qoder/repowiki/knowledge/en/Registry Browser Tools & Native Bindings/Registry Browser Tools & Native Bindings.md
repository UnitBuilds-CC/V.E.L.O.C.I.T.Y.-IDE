# Registry Browser Tools & Native Bindings

## Classification
- **Category**: MCP Tools / Browser Integration
- **Files**: velocity-mcp/src/registry/ (22 files: browser_tools/ 12, tool_definitions/ 5, core 5)
- **Criticality**: High — agent browser interaction surface

## Summary

Browser-specific MCP tools with cascading dispatch: navigation, session management, workflow record/replay, and native browser engine bindings (DOM queries, click simulation, text input, screenshot, scroll, JS evaluation).

## Dispatch Chain

```
handle_browser_tool()
    ├── navigation::handle_navigation_tool()  — navigate, back, forward, get_url
    ├── native::handle_native_tool()          — DOM, click, type, screenshot, scroll, evaluate
    ├── session::handle_session_tool()        — cookies, storage, auth state
    └── workflow::handle_workflow_tool()      — record, replay, step
```

## Native Engine Bindings

- `browser_query_selector` — CSS selector → element list
- `browser_click` — Full event dispatch (mousedown → mouseup → click)
- `browser_type_text` — Text input simulation
- `browser_screenshot` — Page capture via screencast module
- `browser_evaluate` — JS execution via velocity-browser interpreter

## Design Decisions

- Cascading dispatch: first match wins
- Native engine access: no remote debugging protocol
- Schema-driven: centralized `get_tools()` returns all tool definitions
