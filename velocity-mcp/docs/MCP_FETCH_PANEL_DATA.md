# `fetch_panel_data` MCP tool

`fetch_panel_data` gives agents read-only structured access to information shown in the IDE. It removes the need to open a panel merely to gather workspace context.

## Request

```json
{
  "name": "fetch_panel_data",
  "arguments": {
    "panel": "files",
    "relativePath": "."
  }
}
```

`panel` is required. `relativePath` is used only by `files` and defaults to the workspace root.

| Panel | Returned data |
| --- | --- |
| `teams` | Persisted expert-team roster, members, provider/model assignments, skills, and scopes. |
| `wiki` | Workspace wiki file/symbol counts and page relationship summaries. |
| `graph` | File and symbol counts plus the 20 files with the most indexed symbols. |
| `bookmarks` | Contents of `.velocity/bookmarks.json`, or an empty bookmark list. |
| `files` | One directory level of names, directory flags, and byte sizes. |

The tool is read-only, workspace-root sandboxed, and passes through the existing governance tool gate. Use `run_command` only when an action is truly needed; it may require approval under workspace policy.
