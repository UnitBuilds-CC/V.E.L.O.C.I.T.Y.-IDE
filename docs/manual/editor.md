# Editor Surfaces

The code-editing area and everything that floats over it: the gutter, minimap,
completion, diagnostics, find/replace, the terminal, and the smart sidebar.

- [Interface & shell](ui-and-shell.md)
- [Panel reference (all panels)](panels-reference.md)
- [Keyboard shortcuts & commands](keyboard-and-commands.md)
- [Manual index](README.md)

---

## The editor tab

Each open file is an `Editor{path, buffer_id}` dock tab, titled with the file
name (or `untitled`). Open one from the Files tree (double-click), Quick Open
(`Ctrl+P`), or the File menu. **Split editor** (`Ctrl+\`) shows two panes.

**Editor tab context menu:** Close Tab · Close Other Tabs · Close All Tabs · ─ ·
Copy Path.

**Text context menu (right-click a selection):** Cut · Copy · Paste · ─ · Select
All · Toggle Line Comment · Format Document · Code Actions… · Go to Definition ·
Rename Symbol… · ─ · Copy Path.

---

## Gutter

Three marker columns plus the line number (mono, right-aligned). A line edit-locked
by another agent shows `L###` in the warning colour.

| Marker | Meaning |
|---|---|
| `●` | Breakpoint (`F9` toggles at caret) |
| `▶` | Fold region |
| `▮` / `▉` | Diff bar (added / changed), success or accent |
| `●` | Diagnostic (error / warning tint) |

Line height = code size × 1.4; the editor scrolls 10 lines past the end. Syntax
highlighting is via syntect (`base16-ocean.dark`, falling back to
`InspiredGitHub`; unknown extensions treated as `txt`). `Ctrl`/`Cmd`-click a
symbol → go to definition. A bracket-match readout shows
`Bracket match: line {a} ↔ line {b}`.

**Editor options** (set from Settings or the toggle commands): cursor offset,
diagnostic lines, breakpoints, collapsed lines, word wrap (`Alt+Z`), semantic
tokens, auto-indent (`Alt+Shift+Z`), indent style, auto-close brackets
(`Alt+Shift+B`), highlight ranges.

---

## Minimap

Toggle `Ctrl+Shift+M`. 80 px wide; renders trimmed lines in `text_muted` over
`bg_tertiary`, a viewport indicator (`accent` fill/stroke), and 3 px highlight
bars (breakpoints in the error colour). Click to jump. Covers up to 5000 lines.

---

## Completion popup

Appears as you type (typing `.` arms it in supported languages; `Ctrl+Space`
triggers manually). Up to **15** rows in a bordered frame. Kind badges:
`K` Keyword · `f` Function · `v` Variable · `T` Type · `M` Module · `.` Field ·
`S` Snippet · `F` File. The selected row expands its docs; snippet placeholders
render as `$0` / `${1:foo}`.

**Chords while open:** `↑ ↓` navigate · `Tab`/`Enter` accept · `Esc` dismiss.

---

## Inline diagnostics

A hover popup (max 400 px) shows `✖ ⚠ ℹ 💡` plus `[code]` and the message. The
diagnostics panel header reads `✖ {errors}` / `⚠ {warnings}` with **All /
Errors / Warnings** filters; rows are `{icon} {file}:{line}:{col} {message}`.

---

## Find & Replace

`Ctrl+F` opens Find (contextual: when the Terminal tab owns focus, `Ctrl+F`
opens the terminal find bar instead); `Ctrl+H` opens Find & Replace.

- Toggles: `Aa` match case, `.*` regex (each lights when active).
- Replace field hint: `Replace with… ($1 groups)`.
- Buttons: **Replace**, **All**, **▲ / ▼** navigate; match counter
  `{current}/{total}` or `No results`; `Bad regex` / `No matches` warnings; `X`
  closes.

---

<a id="terminal"></a>
## Terminal

The real terminal lives in the **bottom panel** (tab 0, `Ctrl+\``); the dock
*Terminal* tab is only a stub that echoes the last command output.

- Monospace 13 pt; selection highlight; **1000** rows shown from a 5000-row
  scrollback; sticks to bottom.
- Full SGR colour (16-colour + 216 cube + grayscale), underline, italic, and
  OSC 8 links (accent-underlined; hold `Ctrl`/`Cmd` to click links).
- Prompt `❯` with a single-row multiline input (hint `Enter command...`).
- **Find bar** (via the palette's *Find in Terminal*): field `Find in terminal`,
  `▲`/`▼` (disabled with no match), counter `{n}/{total}`, **Close**;
  `Enter`/`Shift+Enter`/`Esc`.
- Shells: `powershell.exe` on Windows; `$SHELL` or `/bin/bash` elsewhere.
  Default grid 120×30.

---

<a id="smart-sidebar"></a>
## Smart sidebar

The context list inside the [right sidebar](ui-and-shell.md#15-right-sidebar).
Header `Context` + item count; empty state `No context available yet`. Entries
render as `F S * > x ! - n` for File / Symbol / Suggest / Action / Error / Warn /
TODO / Note.

---

## Agent inline suggestions

Ghost-text appears in the editor; **Tab** accepts, **Esc** dismisses. A floating
suggestion card (bottom-right, max 420 px) reads `✨ Suggestion`, the source and
confidence, a 240-char preview, and **Accept (Tab)** / **Dismiss (Esc)** buttons,
with an accept-rate footer. Request one manually with `Ctrl+Shift+I`. See the
[Inline Suggestions panel](panels-reference.md#inline-suggestions).

---

*Next: [Panel reference](panels-reference.md) · [Manual index](README.md).*
