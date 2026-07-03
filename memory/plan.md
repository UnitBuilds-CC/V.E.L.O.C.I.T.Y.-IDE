# Plan: V.E.L.O.C.I.T.Y. IDE

## Goal
Build a terminal-based IDE dashboard that gives the agent (and the user) a
unified view of the workspace, files, shell, todos, plan, and git state.

## Phases

### Phase 1 — Foundation
- [x] Inventory existing agent harness, tools, and state modules.
- [x] Create `ide/app.py` as the Textual entry point.
- [x] Add `ide/widgets.py` with reusable panels (file tree, editor, shell, status).

### Phase 2 — Layout
- [ ] Three-pane layout: sidebar (files + todos), main (editor/viewer), footer (shell + status).
- [ ] Bind keys for navigation and common actions.

### Phase 3 — Integration
- [ ] Read workspace files through existing `agent/state.py` helpers.
- [ ] Execute shell commands via `agent/tools.py` and stream output.
- [ ] Display git status, branch, and recent commits in status bar.

### Phase 4 — Polish
- [ ] Command palette for quick file open, search, and git actions.
- [ ] Theme and styling consistent with V.E.L.O.C.I.T.Y. branding.
- [ ] Update `requirements.txt` and add a run script.

## Current Focus
Implementing the Textual dashboard UI (`ide/app.py`).
