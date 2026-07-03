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
- [x] Three-pane layout: sidebar (files + todos + plan), main (editor/viewer), footer (shell + status).
- [x] Bind keys for navigation and common actions.
- [x] Add plan viewer tab and improve sidebar organization.

### Phase 3 — Integration
- [x] Read workspace files through existing `agent/state.py` helpers.
- [x] Execute shell commands via `agent/tools.py` and stream output.
- [x] Display git status, branch, and recent commits in status bar.

### Phase 4 — Polish
- [x] Command palette for quick file open, search, and git actions.
- [x] Theme and styling consistent with V.E.L.O.C.I.T.Y. branding.
- [x] Update `requirements.txt` and add a run script.

## Current Focus
Polishing the Textual dashboard UI and wiring remaining dashboard actions into the agent tool registry.
