# V.E.L.O.C.I.T.Y. IDE — User Manual

The de-facto reference for the entire IDE: every panel, sub-panel, dropdown,
command, shortcut, and agent tool. Each page is cross-checked against the
source, so it matches the running binary.

> Prefer the source of truth? The tables here are generated from
> `velocity-mcp/src/editor/` (panels, shell, commands, keybindings) and
> `velocity-mcp/src/registry/tool_definitions/` (MCP tools).

---

## Pages

| Page | Covers |
|---|---|
| [Getting started](getting-started.md) | Install, launch, workspace, profiles, first provider, first chat, first build |
| [Interface & shell](ui-and-shell.md) | Menu bar, activity rails, sub-tab strip, sidebars, status bar, bottom panel, welcome screen, appearance (profiles/themes/density/scale), toasts, all overlays & dialogs |
| [Editor surfaces](editor.md) | Code editor, gutter, minimap, completion, inline diagnostics, find/replace, terminal, smart sidebar, inline suggestions |
| [Panel reference](panels-reference.md) | Every dock tab and rail sub-panel, grouped: core, orchestration, search/knowledge, agents, build/debug, automation, git, workspace/peers, rail-only |
| [Settings & providers](settings-providers.md) | Appearance, system, agent defaults, and all 16 provider credential forms |
| [Keyboard shortcuts & commands](keyboard-and-commands.md) | The 57 default keybindings, special in-widget chords, and all 110 command-palette commands |
| [MCP tool reference](mcp-tools.md) | All **336** agent-facing tools, grouped by registry module (browser · wa · system · team · company · drone · instances · generation) |

---

## How the UI is organised

- **Activity rails** (left) select a category; each rail owns an independent
  **sub-tab** row. Rails are chosen by **click** — most have no selector chord.
- **Dock tabs** float in the centre; a rail sub-panel and a dock tab can render
  the same feature with slightly different chrome (noted per panel).
- **Workspace profiles** (`Ctrl+1`–`Ctrl+4`) reshape theme, density, dock tabs,
  right-panel sections, and the command palette together.
- The **command palette** (`Ctrl+Shift+P`) and the **F1 cheat-sheet** render the
  same command table.

### Known naming collisions

- Two panels share the visible dock title **Memory**: *Agent Memory* and
  *Persistent Memory*. Tell them apart by the header text.
- Two rails use the same **SQUARES_FOUR** glyph: *Nodes* and *Workspace*.

---

## Related documents

- [User guide (overview + architecture)](../../USER_GUIDE.md)
- [README](../../README.md) · [CHANGELOG](../../CHANGELOG.md)
- [Deployment guide](../DEPLOYMENT.md) · [Operational runbook](../RUNBOOK.md)
- [Security policy](../../SECURITY.md)
