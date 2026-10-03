# Keyboard Shortcuts & Command Palette

The single reference for every key chord and every command in Velocity IDE.

Chords are resolved from two layers, in this order:

1. **User customizations** read from `{workspace}/.velocity/keybindings.json`.
   If you rebind or add a chord here it is matched **first**.
2. **Built-in defaults** (the table below), handled by the fixed dispatch chain.

Code-folding chords (`view.fold*`) are intentionally **not** shipped — the
whole-document editor cannot collapse lines without desynchronizing the cursor,
so no shortcut is advertised for a feature that would only move a gutter marker.
A regression test asserts every advertised default is actually dispatchable.

---

## Default keybindings

These are the factory bindings (`KeybindingsConfig::defaults()`). Any of them
can be rebound in `.velocity/keybindings.json`.

| Command id | Chord | Context | Action |
|---|---|---|---|
| `file.new` | Ctrl+N | — | New file |
| `file.open` | Ctrl+O | — | Open file dialog |
| `file.save` | Ctrl+S | — | Save active file |
| `file.save_all` | Ctrl+Shift+S | — | Save all open files |
| `file.close` | Ctrl+W | — | Close active tab |
| `file.quick_open` | Ctrl+P | — | Quick Open (go to file) / command palette |
| `edit.undo` | Ctrl+Z | — | Undo |
| `edit.redo` | Ctrl+Shift+Z | — | Redo |
| `edit.find` | Ctrl+F | editorFocus | Find in editor (contextual: terminal find when the Terminal tab owns focus) |
| `edit.replace` | Ctrl+H | editorFocus | Find & Replace |
| `edit.find_next` | F3 | editorFocus | Next match |
| `edit.find_prev` | Shift+F3 | editorFocus | Previous match |
| `edit.next_change` | Ctrl+Alt+J | — | Jump to next unsaved change |
| `edit.prev_change` | Ctrl+Alt+K | — | Jump to previous unsaved change |
| `edit.next_problem` | F8 | — | Go to next problem |
| `edit.prev_problem` | Shift+F8 | — | Go to previous problem |
| `editor.select_expand` | Shift+Alt+Right | editorFocus | Expand selection |
| `editor.select_shrink` | Shift+Alt+Left | editorFocus | Shrink selection |
| `edit.indent` | Tab | editorFocus | Indent |
| `edit.dedent` | Shift+Tab | editorFocus | Dedent |
| `edit.toggle_comment` | Ctrl+/ | editorFocus | Toggle line comment |
| `edit.duplicate_line` | Ctrl+Shift+D | editorFocus | Duplicate line |
| `edit.delete_line` | Ctrl+Shift+K | editorFocus | Delete line |
| `edit.move_line_up` | Alt+Up | editorFocus | Move line up |
| `edit.move_line_down` | Alt+Down | editorFocus | Move line down |
| `nav.goto_line` | Ctrl+G | — | Go to line |
| `nav.goto_symbol` | Ctrl+Shift+O | — | Go to symbol |
| `nav.goto_definition` | F12 | editorFocus | Go to definition |
| `nav.find_references` | Shift+F12 | editorFocus | Find all references |
| `nav.back` | Alt+Left | — | Navigate back |
| `nav.forward` | Alt+Right | — | Navigate forward |
| `nav.next_tab` | Ctrl+PageDown | — | Next tab |
| `nav.prev_tab` | Ctrl+PageUp | — | Previous tab |
| `view.command_palette` | Ctrl+Shift+P | — | Command palette |
| `view.toggle_sidebar` | Ctrl+E | — | Toggle left sidebar |
| `view.toggle_terminal` | Ctrl+\` | — | Toggle bottom panel to Terminal |
| `view.toggle_chat` | Ctrl+J | — | Toggle Chat |
| `view.toggle_orchestrator` | Ctrl+Shift+Y | — | Toggle Orchestrator |
| `view.toggle_search` | Ctrl+Shift+F | — | Toggle Search |
| `view.toggle_settings` | Ctrl+, | — | Toggle Settings |
| `view.toggle_extensions` | Ctrl+Shift+X | — | Toggle Extensions |
| `view.toggle_activity` | Ctrl+Shift+A | — | Toggle Live Activity |
| `view.toggle_voice` | Ctrl+Shift+V | — | Toggle Voice Commands |
| `view.toggle_minimap` | Ctrl+Shift+M | — | Toggle minimap |
| `view.split_editor` | Ctrl+\\ | — | Split editor |
| `view.word_wrap` | Alt+Z | editorFocus | Toggle word wrap |
| `view.toggle_auto_indent` | Alt+Shift+Z | editorFocus | Toggle auto-indent |
| `view.toggle_auto_close_brackets` | Alt+Shift+B | editorFocus | Toggle auto-close brackets |
| `debug.start` | F5 | — | Start debugging |
| `debug.continue` | F5 | debugActive | Continue |
| `debug.stop` | Shift+F5 | — | Stop debugging |
| `debug.step_over` | F10 | debugActive | Step over |
| `debug.step_into` | F11 | debugActive | Step into |
| `debug.step_out` | Shift+F11 | debugActive | Step out |
| `debug.toggle_breakpoint` | F9 | editorFocus | Toggle breakpoint |
| `agent.request_inline_suggestion` | Ctrl+Shift+I | — | Request inline suggestion |
| `editor.rename_symbol` | F2 | — | Rename symbol |
| `editor.code_actions` | Alt+Enter | — | Code actions |
| `editor.format_document` | Shift+Alt+F | — | Format document |
| `editor.toggle_bookmark` | Ctrl+Shift+B | — | Toggle bookmark at caret |
| `build.build` | Ctrl+B | — | Build |
| `build.run` | Ctrl+R | — | Run |
| `build.rollback_deploy` | Ctrl+Alt+R | — | Rollback last deploy |
| `build.show_nodes` | Ctrl+Alt+B | — | Open Nodes panel |
| `git.switch_branch` | Ctrl+Shift+G | — | Branch switcher overlay |
| `mode.coder` | Ctrl+1 | — | Layout: Coder |
| `mode.operator` | Ctrl+2 | — | Layout: Automation Operator |
| `mode.mission` | Ctrl+3 | — | Layout: Mission Control |
| `mode.accessibility` | Ctrl+4 | — | Layout: Accessibility |
| `completion.trigger` | Ctrl+Space | editorFocus | Trigger completion |
| `editor.zoom_in` | Ctrl+= | — | Increase editor font zoom |
| `editor.zoom_out` | Ctrl+- | — | Decrease editor font zoom |
| `editor.zoom_reset` | Ctrl+0 | — | Reset editor font zoom |

> **The activity rails are mouse-navigated**, not chord-navigated. There is no
> `Ctrl+G`/`Ctrl+D`/`Ctrl+K` rail selector — `Ctrl+G` is *Go to Line*, and
> `Ctrl+D`/`Ctrl+K` are unbound. Some panels do have direct toggle chords
> (`Ctrl+J` Chat, `Ctrl+E` sidebar, `Ctrl+Shift+X` Extensions, etc.) listed
> above, but selecting a rail or its sub-tab is a click.

---

## Special in-widget chords

These are owned transiently by whatever widget currently holds focus:

- **Completion popup** (while visible): `↑` `↓` navigate, `Tab`/`Enter` accept, `Esc` dismiss.
- **Inline ghost text**: `Tab` accept, `Esc` dismiss.
- Typing `.` arms completion in supported languages.
- **Early return** — global chords are suspended while any overlay is open:
  command palette, quick open, workspace switcher, goto line, goto symbol,
  branch switcher, rename, code actions, disk hygiene.
- `Ctrl+P` and `Ctrl+Shift+P` both open the palette; `Ctrl+Tab` opens the MRU tab switcher.
- `F1` opens the keyboard cheat-sheet (same source table as this page).

---

## Command palette (all 110 commands)

Both the command palette and the `F1` cheat sheet render the same
`commands()` table. The palette filters by active mode, hidden categories, and
a case-insensitive fuzzy subsequence, then sorts priority-category first, then
alphabetically.

**File (18)** — New File `Ctrl+N` · Open File… `Ctrl+O` · Go to File… `Ctrl+P` ·
Next Tab `Ctrl+PageDown` · Previous Tab `Ctrl+PageUp` · Close Other Tabs ·
Close All Tabs · Revert File · Reopen Closed Tab `Ctrl+Shift+T` · Go to Line…
`Ctrl+G` · Go to Symbol… `Ctrl+Shift+O` · Go Back `Alt+Left` · Go Forward
`Alt+Right` · Go to Definition `F12` · Go to Declaration · Go to Type Definition ·
Go to Implementation · Find All References `Shift+F12` · Format Document
`Shift+Alt+F` · Save `Ctrl+S` · Save As… · Save All `Ctrl+Shift+S` · Close Tab `Ctrl+W`

**Problem navigation** — Go to Next Problem `F8` · Go to Previous Problem `Shift+F8`

**Edit (16)** — Duplicate Line `Ctrl+Shift+D` · Delete Line `Ctrl+Shift+K` ·
Move Line Up `Alt+Up` · Move Line Down `Alt+Down` · Toggle Line Comment `Ctrl+/` ·
Indent Lines `Tab` · Dedent Lines `Shift+Tab` · Jump to Next Change `Ctrl+Alt+J` ·
Jump to Previous Change `Ctrl+Alt+K` · Expand Selection `Shift+Alt+Right` ·
Shrink Selection `Shift+Alt+Left` · Undo `Ctrl+Z` · Redo `Ctrl+Shift+Z` ·
Find / Replace `Ctrl+H` · Find `Ctrl+F` · Find in Terminal `Ctrl+F` ·
Toggle Format On Save · Toggle Trim Trailing Whitespace On Save

**Debug (6, Coder-only)** — Start / Continue Debugging `F5` · Stop Debugging
`Shift+F5` · Step Over `F10` · Step Into `F11` · Step Out `Shift+F11` · Toggle
Breakpoint `F9`

**Build** — Build `Ctrl+B` (Coder) · Run `Ctrl+R` (Coder) · Deploy Pipeline
(Coder) · Rollback Deploy `Ctrl+Alt+R`

**Nodes** — Open Nodes Panel `Ctrl+Alt+B`

**Automation** — Run Selected Flow `Ctrl+Enter` (Automation Operator) · Triggers ·
Workflows · Governance

**Panels** — Chat `Ctrl+J` · Output `` Ctrl+` `` · Orchestrator `Ctrl+Shift+Y`
(Automation Operator) · Mission Control (Mission Control) · Search `Ctrl+Shift+F` ·
Review Changes · Usage · Settings `Ctrl+,` · Extensions · Live Activity (Mission
Control) · Test Coverage (Coder) · Voice Commands

**Knowledge** — Code Graph · Knowledge Base · Bookmarks · Agent Memory · Shared Memory

**Workspace** — Research Browser · Clean Build Artifacts… `Ctrl+Shift+K` ·
Mode: Coder `Ctrl+1` · Mode: Automation Operator `Ctrl+2` · Mode: Mission Control
`Ctrl+3` · Mode: Accessibility `Ctrl+4` · Mode: Reset Layout to Default ·
Wiki: Export to Markdown · NDA: New Document · NDA: Import Active File ·
NDA: Open Browser Viewer

**Agent** — Request Inline Suggestion `Ctrl+Shift+I` · Approve All Tools ·
Decline All Tools · Plan Sub-Agents · Refresh Models

**View** — Toggle Sidebar `Ctrl+E` · Toggle Auto Save · Toggle History · Reset
Layout · Git: Switch Branch · Increase Editor Font Zoom `Ctrl+=` · Decrease Editor
Font Zoom `Ctrl+-` · Reset Editor Font Zoom `Ctrl+0` · Toggle Minimap
`Ctrl+Shift+M` · Toggle Auto Indent · Toggle Auto Close Brackets · Split Editor `Ctrl+\`

---

*Next: [Interface & Shell](ui-and-shell.md) · [Command reference index](README.md).*
