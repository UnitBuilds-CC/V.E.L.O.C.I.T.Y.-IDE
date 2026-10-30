# Interface & Shell

Everything around the editing surface: the window chrome, the activity rails,
the sidebars, the status bar, the bottom panel, the appearance system, and every
modal overlay. This page is the de-facto reference for what each control does
and where it lives.

- [Keyboard shortcuts & commands](keyboard-and-commands.md)
- [Editor surfaces](editor.md)
- [Panel reference (all panels)](panels-reference.md)
- [Settings & providers](settings-providers.md)
- [Manual index](README.md)

---

## 1. Window shell

### 1.1 Top menu bar

A single unified header (the one supported layout). Leading
**`VELOCITY`** wordmark in the accent colour, then these menus. Menu items
*focus* a panel (they never toggle it off).

| Menu | Items |
|---|---|
| **File** | New File `Ctrl+N` · Open File… `Ctrl+O` · Quick Open `Ctrl+P` · ─ · Save `Ctrl+S` · Save All `Ctrl+Shift+S` |
| **Navigate** | Command Palette `Ctrl+P` · ─ · Chat `Ctrl+J` · Search `Ctrl+Shift+F` · Research browser · Review changes · ─ · Output `Ctrl+\`` · Terminal |
| **Build** | Build `Ctrl+B` · Run `Ctrl+R` · ─ · Test generator · Test coverage · Deploy pipeline · Debugger · Language servers · ─ · *Advanced:* Snippets · Inline suggestions · Build cache |
| **Tools ▸ Agents** | Live activity · Agent roster · Background agents · Orchestration · Task queue · Timeline · Mission metrics · Conflict resolver · Self-improvement · Session continuity |
| **Tools ▸ Knowledge** | Knowledge base · Wiki · Code graph · Semantic search · Bookmarks · Favorites · Agent memory · Shared memory · Persistent memory · Recent changes |
| **Tools ▸ Automation** | Workflows · Triggers · Automation flows · Targets · Execution logs · Recordings · Voice commands · Multimodal · Accessibility · Governance |
| **Workspace** | *Workspace tools:* Extensions · Plugin registry · Skills · Collaboration · Peers · Usage · ─ · New NDA document · Import active file to NDA · Open NDA browser viewer |
| **Help** | Keyboard Shortcuts `F1` · ─ · Settings `Ctrl+,` |
| **Layouts ▾** | *Workspaces:* **Build** (Coder, `Ctrl+1`), **Mission** (Mission Control, `Ctrl+3`) · ─ · *Specialized layouts:* **Automation Operator** (`Ctrl+2`), **Accessibility** (`Ctrl+4`). Each row shows the layout description on hover. |

**Header right toolbar** (right-to-left icon buttons):

1. **Right-panel toggle** — `☑` (shown) / `☐` (hidden). Hover *Show/Hide right
   panel* (`Ctrl+Shift+E`).
2. **Left-panel toggle** — same icons. Hover *Show/Hide sidebar* (`Ctrl+E`).
3. **Workspace switcher** — folder button; opens the Switch Workspace overlay.

**Breadcrumb strip** sits above the editor (`›`-separated path segments) when
*Show breadcrumbs* is on (default **on**; toggle in Settings → Editor).

### 1.2 Activity bar (left rails)

A 56 px column of 48×48 rail buttons; the selected rail draws a 2.5 px accent
indicator. **Rails are selected by clicking** — there is no rail-selector
keyboard chord (`Ctrl+G` is *Go to Line*; `Ctrl+D` / `Ctrl+K` are unbound). Some
rails also have a direct panel-toggle chord that jumps straight to their first
sub-tab, noted below.

| # | Rail | Opens these sub-tabs | Direct toggle chord |
|---|---|---|---|
| 0 | **Files** | Files · Bookmarks · Favorites · Outline | `Ctrl+E` (sidebar) |
| 1 | **Search** | Search · Semantic · Code Graph | `Ctrl+Shift+F` |
| 2 | **Git** | Changes · Branches · Commits | — (click) |
| 3 | **Chat** | Chat · Voice · Multimodal · Browser | `Ctrl+J` |
| 4 | **Build** | Build · Test · Deploy · Debug · LSP | `Ctrl+B` |
| 5 | **Nodes** | Nodes | `Ctrl+Alt+B` |
| 6 | **Agents** | Activity · Roster · Orchestration · Memory · Timeline · Metrics | — (click) |
| 7 | **Knowledge** | Wiki · Knowledge Base · Snippets · NDA | — (click) |
| 8 | **Workspace** | Extensions · Plugins · Skills · Team Studio · Usage · Governance | `Ctrl+Shift+X` |

At the **bottom of the rail**: a **GEAR** button (hover *Settings* `Ctrl+,`) that
focuses Settings, then an **identity pill** showing two initials derived from the
workspace folder name (uppercase letters first, else the first two characters;
fallback **`V`**).

> Selecting a rail remembers its last sub-tab independently — the nine rails each
> hold their own sub-tab index, so switching back to *Agents* returns you to
> whichever agent sub-tab you were on.

### 1.3 Sub-tab strip

Under the rails, a wrapped row of icon+label tabs for the selected rail (28 px
min height, accent underline when selected, 14 pt header showing the active
tab's label below). Out-of-range stored indices clamp back to the first tab.

### 1.4 Left sidebar

Resizable (`Ctrl+E` toggles). Default width **240 px** (min 180, max
`min(1400, 60% of viewport)`). Header shows the workspace folder name (13 pt),
a git-branch glyph, and the current branch name. Content is the selected rail's
sub-tab panel (file tree, search, git changes, etc. — see
[Panel reference](panels-reference.md)).

### 1.5 Right sidebar

Hidden by default (toggle `Ctrl+Shift+E`; width 220–600, default 280). Filled
with collapsible context sections that **change with the active workspace
profile**:

| Profile | Right-panel sections |
|---|---|
| **Coder** | ◎ Symbol Context · ± Active Changes · ★ AI Suggestions |
| **Automation Operator** | ≉ Flow Inspector · ⊞ Element Picker · ≡ Action Log |
| **Mission Control** | ⊙ Agent Detail · ⊟ Task Inspector · ⚠ Alerts |
| **Accessibility** | ⊿ Accessibility Tree · ◐ Contrast Checker · ⊜ ARIA Inspector |

Sections show `▸/▾ Changes`, `▸/▾ {symbol}()` with *Callers (n)* /
*Dependencies (n)*, and a `◌` empty state, plus the [smart sidebar](editor.md#smart-sidebar).

### 1.6 Status bar

Left → right: **profile pill** (accent), **build pill** (`✓ build` / `✕ build`,
hover *View diagnostics*), **branch**, **`Ln {n}, Col {n}`** (hover *Go to
line*), and git **blame** (italic). Centre: the transient **status message**.
Right (RTL): **provider/model pill** (mono; strips `@tag/`, truncates long names
to `…`+last 31) and the **`⌘ Ctrl+P`** palette button. Every segment is clickable
and routes to the matching panel or overlay.

### 1.7 Bottom panel

Toggle with `Ctrl+\``. Tabs in order:

| Tab | Contents |
|---|---|
| **Terminal** (0) | The real PTY terminal — see [editor → Terminal](editor.md#terminal). |
| **Problems** (1) | `✓ No problems detected` when clean; otherwise `{n} error(s)` / `{n} warning(s)` with each diagnostic (mono). |
| **Debug** (2) | Debug session controls — see [Debugger panel](panels-reference.md#debugger). |
| **Output** (3) | Agent metrics bar (state, tokens, cost, tools, thinking) + pending approvals when an agent is active; otherwise raw `command_output` or an empty state with a *Run build* button. |
| **Checkpoints** (4) | Automatic pre-agent snapshots — see [rail-only surfaces](panels-reference.md#rail-only-surfaces). |

Max height 600 px; defaults to Terminal, collapsed.

### 1.8 Welcome screen

Shown when no dock tab is open: the **VELOCITY** wordmark (32 pt), a workspace
card, a **GET STARTED** block with a primary **`＋ New Task`** (`Ctrl+J`) and four
secondary action rows, a **RECENT FILES** list, and footer hints.

---

## 2. Appearance & modes

### 2.1 Workspace profiles

Four profiles, each a full layout + theme + density + palette-filter preset
(`Ctrl+1`–`Ctrl+4`, or the **Layouts ▾** menu):

| Profile | Shortcut | Glyph | Default theme | Density | Palette priority | Hidden categories | "Try:" search chips |
|---|---|---|---|---|---|---|---|
| **Coder** | `Ctrl+1` | `<>` | Midnight | Comfortable | File, Build, View | — | `TODO`, `fn `, `struct ` |
| **Automation Operator** | `Ctrl+2` | 🤖 | Operator | Compact | Automation, Panels | Build | `desktop`, `browser`, `automation` |
| **Mission Control** | `Ctrl+3` | ◉ | Mission | Spacious | Agent, Workspace | Build, File | `worker`, `task`, `approval` |
| **Accessibility** | `Ctrl+4` | 👁 | High Contrast | Spacious | View, File | Build | `theme`, `contrast`, `scale` |

Switching a profile applies its density + scale, and its theme unless you have
named one in Settings → Appearance, rebuilds the dock with
its primary tabs, swaps the right-panel sections, and re-filters / re-prioritises
the command palette. **Reset defaults** returns the profile's own theme.

### 2.2 Themes

Five palettes: **Midnight** (dark), **Daylight** (light), **Operator**,
**Mission**, **High Contrast**. Each is a set of 14 semantic tokens
(`bg_primary`, `bg_secondary`, `bg_tertiary`, `text`, `text_muted`,
`text_disabled`, `accent`, `info`, `border`, `success`, `warning`, `error`,
`surface_hover`, `text_on_accent`) — pick one in Settings → Appearance.
Where the OS publishes a light/dark preference (Windows today), **Follow system
light/dark** lets it settle the palette instead: light desktop → Daylight, dark
desktop → Midnight, re-checked on a short throttle while the window is open. A
workspace with no stored preferences starts with that option on; picking a theme
by hand, or switching profile, turns it off — the last deliberate appearance
action wins, and each profile is advertised by its own palette. `VELOCITY_OS_THEME`
(`light`/`dark`/`off`) supplies the preference on a machine that publishes none.
Fonts are Inter (UI) and JetBrains Mono (code), plus the Phosphor icon family.

### 2.3 Density

**Compact**, **Comfortable**, **Spacious** — adjusts item spacing, button
padding, and window margins.

### 2.4 Scale

Two sliders in Settings: **UI scale** (0.85–1.35, default 1.15) scales the body
font; **Code scale** (0.5–3.0, default 1.12) scales the editor font. All profile
presets ship `1.15 / 1.12`. Editor-only font zoom is separate (`Ctrl+=` /
`Ctrl+-` / `Ctrl+0`).

### 2.5 Toasts

Notifications stack bottom-right (320 px, 4 s default TTL, level-tinted stroke
and a countdown bar on the top toast). Levels: **Info** (accent), **Success**,
**Warning**, **Error**. Each has a close **✕**.

---

## 3. Overlays & dialogs

Modal surfaces. Global chords are suspended while any of these is open;
`Esc` closes them.

| Overlay | Open via | Controls | Keys |
|---|---|---|---|
| **Command palette** | `Ctrl+Shift+P` / `Ctrl+P` | filter box + sorted command rows (label + shortcut) | `↑ ↓ Enter Esc` |
| **Keyboard cheat-sheet** | `F1` | all shortcuts grouped by category | `F1`, `Esc` |
| **Quick Open** | `Ctrl+P` | fuzzy file filter | `↑ ↓ Enter Esc` |
| **Workspace switcher** | header folder button | `●/○` project list | `↑ ↓ Enter Esc` |
| **Go to Line** | `Ctrl+G` | line-number field | `Enter Esc` |
| **Go to Symbol** | `Ctrl+Shift+O` | symbol filter | `↑ ↓ Enter Esc` |
| **Branch switcher** | `Ctrl+Shift+G` | branch filter + create | `↑ ↓ Enter Esc`, `n` = new branch |
| **Rename symbol** | `F2` | new-name field, Rename / Cancel | `Enter Esc` |
| **Code actions** | `Alt+Enter` | `💡` action list | `Esc` |
| **References** | `Shift+F12` | `path:line` selectable list | `Esc` |
| **Call hierarchy** | (from Code Graph) | Incoming / Outgoing call rows | `Esc` |
| **Signature help** | (typing a call) | parameter highlight + overload `i/n` | — |
| **MRU tab switcher** | `Ctrl+Tab` | recent-tab list, `(closed)` suffix | `Ctrl+Tab`, `Esc` |
| **Disk hygiene** | `Ctrl+Shift+K` (Clean Build Artifacts) | reclaimable readout, per-tree checkboxes, Dry run / Confirm / Reclaim / Cancel | `Esc` disarms |
| **Open File dialog** | `Ctrl+O` | workspace tree + relative path, Open / Cancel | — |
| **Save As dialog** | Save As… | relative path, Save / Cancel | — |
| **Confirm close** | closing a dirty tab | file name, Save / Don't Save / Cancel | — |
| **New file / folder** | tree context menu | name field, OK / Cancel | — |
| **Delete confirmation** | tree context menu | `Delete {name}?`, Delete / Cancel | — |
| **Full diff** | (review changes) | side-by-side diff, `(+added / -removed)` | `Esc` |
| **Node browser** | (Nodes panel) | remote folder picker: path bar, Go/Up/Refresh, filter, Select | — |
| **File-tree context menu** | right-click tree | 🖫 New File · 🗁 New Folder · ─ · Copy Path · Rename… · 🗑 Delete… | — |

---

*Next: [Editor surfaces](editor.md) · [Panel reference](panels-reference.md) · [Manual index](README.md).*
