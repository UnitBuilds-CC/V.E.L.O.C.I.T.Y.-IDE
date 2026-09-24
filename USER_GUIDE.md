# V.E.L.O.C.I.T.Y. IDE — User Guide

A complete guide to using the V.E.L.O.C.I.T.Y. Cognitive IDE — a native, GPU-accelerated developer workspace with autonomous agentic capabilities built in pure Rust.

---

## Table of Contents

- [Getting Started](#getting-started)
  - [System Requirements](#system-requirements)
  - [Installation](#installation)
  - [First Launch](#first-launch)
  - [Opening a Workspace](#opening-a-workspace)
  - [Quick Start Tutorial](#quick-start-tutorial)
- [Interface Overview](#interface-overview)
  - [The Activity Bar](#the-activity-bar)
  - [Workspace Modes](#workspace-modes)
  - [Themes & Appearance](#themes--appearance)
  - [Status Bar](#status-bar)
  - [Toast Notifications](#toast-notifications)
  - [Bottom Panel](#bottom-panel)
  - [Accessibility Mode](#accessibility-mode)
- [Command Palette](#command-palette)
- [Menu Bar Reference](#menu-bar-reference)
- [Working with Files](#working-with-files)
  - [File Tree](#file-tree)
  - [Bookmarks & Favorites](#bookmarks--favorites)
  - [Search & Replace](#search--replace)
  - [Semantic Search](#semantic-search)
  - [Code Graph](#code-graph)
  - [Outline](#outline)
  - [Local File History](#local-file-history)
- [Code Editing](#code-editing)
  - [Editor Features](#editor-features)
  - [Find & Replace](#find--replace)
  - [Code Folding](#code-folding)
  - [Inline Suggestions](#inline-suggestions)
  - [Code Snippets](#code-snippets)
  - [Diagnostics (Problems Panel)](#diagnostics-problems-panel)
- [AI Chat & Agents](#ai-chat--agents)
  - [Chat Panel](#chat-panel)
  - [Model Selection](#model-selection)
  - [Agent Approvals](#agent-approvals)
  - [Voice Commands](#voice-commands)
  - [Multimodal Input](#multimodal-input)
- [MCP Server Integration](#mcp-server-integration)
  - [How It Works](#how-it-works)
  - [Starting the MCP Server](#starting-the-mcp-server)
  - [Configuring External Assistants](#configuring-external-assistants)
  - [Available MCP Tools](#available-mcp-tools)
  - [GUI Control Bridge](#gui-control-bridge)
- [Browser Automation](#browser-automation)
  - [Browse Panel](#browse-panel)
  - [Automation Flows](#automation-flows)
  - [Targets & Recordings](#targets--recordings)
  - [Windows Desktop Automation](#windows-desktop-automation)
- [Git Integration](#git-integration)
  - [Changes Panel](#changes-panel)
  - [Branches & Commits](#branches--commits)
- [Wiki & Knowledge](#wiki--knowledge)
  - [Wiki System](#wiki-system)
  - [Knowledge Base](#knowledge-base)
  - [Agent Memory](#agent-memory)
  - [Persistent Memory](#persistent-memory)
  - [Shared Memory (Multi-Agent Collaboration)](#shared-memory-multi-agent-collaboration)
- [Orchestrator & Agent Teams](#orchestrator--agent-teams)
  - [Orchestrator](#orchestrator)
  - [Team Studio](#team-studio)
  - [Agent Roster](#agent-roster)
  - [Background Agents](#background-agents)
  - [Self-Improvement Engine](#self-improvement-engine)
  - [Conflict Resolver](#conflict-resolver)
  - [Session Continuity (Continuation Ledger)](#session-continuity-continuation-ledger)
- [Build & Deploy](#build--deploy)
  - [Build Panel](#build-panel)
  - [Test Generator](#test-generator)
  - [Deploy Pipeline](#deploy-pipeline)
  - [Debugger](#debugger)
  - [Workflow Canvas](#workflow-canvas)
  - [Workflow Templates](#workflow-templates)
  - [Workflow Version History](#workflow-version-history)
  - [Triggers & Unattended Execution](#triggers--unattended-execution)
  - [Governance & Policy Engine](#governance--policy-engine)
  - [Workspace Checkpoints](#workspace-checkpoints)
  - [Precompilation Cache](#precompilation-cache)
- [Plugins & Extensions](#plugins--extensions)
  - [Plugin Registry](#plugin-registry)
  - [Extensions](#extensions)
  - [Skills](#skills)
- [Connectors & External Services](#connectors--external-services)
  - [Connector Types](#connector-types)
  - [Setting Up a Connector](#setting-up-a-connector)
  - [OAuth2 Integration](#oauth2-integration)
  - [Sync Engine](#sync-engine)
  - [Webhooks](#webhooks)
  - [Integration Templates](#integration-templates)
- [Site Map & NDA Format](#site-map--nda-format)
  - [Site Map](#site-map)
  - [NDA Format](#nda-format)
- [Collaboration & Drones](#collaboration--drones)
  - [Collaboration Manager](#collaboration-manager)
  - [Peer Collaboration Panel](#peer-collaboration-panel)
  - [Drone Subsystem](#drone-subsystem)
- [Security Model](#security-model)
- [Usage Tracking](#usage-tracking)
- [Workspace Preferences](#workspace-preferences)
- [Settings & Configuration](#settings--configuration)
  - [Appearance Settings](#appearance-settings)
  - [Keybindings](#keybindings)
  - [Provider Configuration](#provider-configuration)
    - [Registering a Provider](#registering-a-provider)
    - [Saving & Reloading Credentials](#saving--reloading-credentials)
    - [Refreshing the Model Catalog](#refreshing-the-model-catalog)
    - [Selecting a Model](#selecting-a-model)
    - [Workspace Provider Settings File](#workspace-provider-settings-file)
    - [Environment Variables](#environment-variables)
    - [Automatic Failover](#automatic-failover)
- [Keyboard Shortcuts Reference](#keyboard-shortcuts-reference)
- [Troubleshooting](#troubleshooting)
  - [Common Issues](#common-issues)
  - [Data & Storage](#data--storage)
- [Frequently Asked Questions](#frequently-asked-questions)

---

## Getting Started

### System Requirements

**Minimum:**
- **CPU:** 4 cores
- **RAM:** 8 GB
- **Disk:** 500 MB (runtime), 2 GB (build from source)
- **OS:** Windows 10+, Ubuntu 20.04+, macOS 12+

**Recommended:**
- **CPU:** 8+ cores
- **RAM:** 16+ GB
- **GPU:** Vulkan-capable (for hardware acceleration)
- **Network:** For AI provider access

### Installation

#### Pre-built Binary (Windows)

1. Download `velocity-windows.zip` from the latest [GitHub Releases](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-IDE/releases) page (it is the Windows asset of the release; Linux and macOS ship as `.tar.gz`)
2. Extract it to a folder of your choice - the archive holds `velocity_ide.exe`, `velocity_ide_gui.exe`, `velocity_mcp.exe` and `velocity-drone.exe`
3. Run `velocity_ide_gui.exe`

#### Build from Source

```bash
# Clone the repository
git clone https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-IDE.git
cd V.E.L.O.C.I.T.Y.-IDE

# Build the GUI (release mode for best performance)
cargo build --release

# Run the GUI
cargo run --release --bin velocity_ide_gui
```

**Linux dependencies:**
```bash
sudo apt-get install -y libgtk-3-dev libwebkit2gtk-4.1-dev libudev-dev
```

### First Launch

![Velocity IDE Main Window](C:\Users\visse\AppData\Local\Temp\qoder-computer-use-images\c569a90c\img-1789211260374158000-175248.png)

On first launch, you'll see the main IDE window with:

- **Title bar** — "V.E.L.O.C.I.T.Y. IDE - Native Workspace Editor"
- **Menu bar** — File, Navigate, Build, Tools, Workspace, Help
- **Activity bar** (left) — 8 category icons for quick navigation
- **Left sidebar** — File tree, search, git, and other panels
- **Editor area** (center) — Tabbed document editor
- **Right sidebar** — Context-sensitive panels (Symbol Context, Active Changes, AI Suggestions)
- **Status bar** (bottom) — Build status, git branch, cursor position, provider info

### Opening a Workspace

1. Click **File → Open Folder** or press `Ctrl+O`
2. Navigate to your project directory
3. Click **Select Folder**

The IDE will index your workspace and populate the file tree. For large projects, indexing happens in the background — you can start working immediately.

### Quick Start Tutorial

New to Velocity IDE? Here's a walkthrough of a typical development workflow:

**Step 1: Open your project**
1. Launch Velocity IDE
2. Press `Ctrl+O` and select your project folder
3. The file tree populates in the left sidebar

**Step 2: Configure your AI provider**
1. Press `Ctrl+,` to open Settings
2. Scroll to **Providers & credentials**
3. Expand your provider (e.g., Cloudflare Workers AI) and enter credentials
4. Click **Save provider settings** (green button)
5. In **Agent defaults**, click **↻ Models** to load available models
6. Select a model from the dropdown

**Step 3: Explore your codebase**
1. Press `Ctrl+Shift+F` to search across files
2. Press `Ctrl+Shift+O` to jump to a symbol by name
3. Click **Knowledge → Wiki** to see auto-generated documentation
4. Click **Search → Code Graph** to visualize symbol relationships

**Step 4: Chat with the AI agent**
1. Press `Ctrl+J` to open the chat panel
2. Try a suggestion chip: "Explain this codebase"
3. Ask specific questions: "Where is authentication handled?"
4. Request changes: "Add error handling to the login function"
5. Review and approve the agent's proposed changes

**Step 5: Build and test**
1. Press `Ctrl+B` to build the project
2. Click errors in the build panel to jump to source locations
3. Use **Build → Test generator** to create tests for a module
4. Press `F5` to start debugging, `F9` to set breakpoints

**Step 6: Commit your changes**
1. Click the **Git** icon in the activity bar (or press `Ctrl+E` to toggle the sidebar, then select Git)
2. Review modified files, stage changes with the + icon
3. Enter a commit message and click **Commit**

---

## Interface Overview

### The Activity Bar

The activity bar is the vertical icon strip on the far left. It contains 8 categories:

| Icon | Label | Description |
|------|-------|-------------|
| 📁 | **Files** | File tree, bookmarks, favorites |
| 🔍 | **Search** | Text search, semantic search, code graph |
| 🔀 | **Git** | Changes, branches, commits |
| 💬 | **Chat** | AI chat, voice, multimodal |
| 🔨 | **Build** | Build, test, deploy, debug, LSP |
| 🤖 | **Agents** | Activity, roster, orchestration, memory |
| 📖 | **Knowledge** | Wiki, knowledge base, snippets, NDA |
| ️ | **Workspace** | Extensions, plugins, skills, team, usage |

Click any icon to switch categories. The left sidebar updates to show the sub-panels for that category. Use `Ctrl+E` to toggle the sidebar, `Ctrl+Shift+F` for search, `Ctrl+G` for go-to-line, `Ctrl+J` for chat.

### Workspace Modes

Velocity IDE has 4 workspace modes, each optimized for different workflows:

| Mode | Shortcut | Icon | Best For |
|------|----------|------|----------|
| **Coder** | `Ctrl+1` | `</>` | General coding and agent review |
| **Automation Operator** | `Ctrl+2` | 🤖 | Browser and desktop automation |
| **Mission Control** | `Ctrl+3` | 📊 | Monitoring multiple agents |
| **Accessibility** | `Ctrl+4` | 👁️ | High contrast, larger text |

Each mode changes:
- **Left sidebar tabs** — Different panels relevant to the workflow
- **Right panels** — Context-sensitive inspection tools
- **Bottom panel layout** — Tabbed, split, or dashboard view
- **Toolbar actions** — Mode-specific quick actions

**Right panels per mode:**

| Mode | Right Panels |
|------|-------------|
| **Coder** | Symbol Context, Active Changes, AI Suggestions |
| **Automation Operator** | Flow Inspector (⧉), Element Picker (⊞), Action Log (≡) |
| **Mission Control** | Agent Detail (⊙), Task Inspector (⊟), Alerts (⚠) |
| **Accessibility** | Accessibility Tree (⬿), Contrast Checker (◐), ARIA Inspector (⊜) |

**Switching modes:** Click the mode badge in the status bar (bottom-left) or use the keyboard shortcut.

### Themes & Appearance

Velocity IDE ships with 5 color themes:

| Theme | Description | Accent Color |
|-------|-------------|--------------|
| **Midnight** | Deep neutral dark | Green |
| **Daylight** | Light theme with warm greys | Blue |
| **Operator** | Cyberpunk terminal aesthetic | Cyan |
| **Mission** | Command deck indigo | Indigo |
| **High Contrast** | Pure black, maximum contrast | Sky blue |

**Changing themes:**
1. Open Settings (`Ctrl+,`)
2. Navigate to **Appearance**
3. Select a theme from the dropdown

**Density settings:**
- **Compact** — Tight spacing, maximum information density
- **Comfortable** — Balanced spacing (default for Coder mode)
- **Spacious** — Relaxed spacing (default for Mission/Accessibility modes)

**UI scaling:** Adjust `ui_scale` (default 1.15) and `code_scale` (default 1.12) in Appearance settings to scale interface and code fonts independently.

### Status Bar

The status bar at the bottom shows:

- **Mode badge** (left) — Click to switch workspace modes
- **Build indicator** — ✓ or ✗ with build status, click to view diagnostics
- **Git branch** — Current branch name
- **Cursor position** — Line and column (click to go to line)
- **Provider pill** — Current AI provider and model (click to open settings)
- **Command palette** — `Ctrl+P` shortcut reminder
- **Workspace switcher** — Click to switch between known workspace projects (also `Ctrl+Shift+W`)

### Toast Notifications

Toast notifications appear as **non-intrusive overlays** in the bottom-right corner to inform you about IDE events:

**Notification levels:**
| Level | Color | Example |
|-------|-------|---------|
| Info | Blue | "File saved successfully" |
| Success | Green | "Build completed" |
| Warning | Amber | "API key expiring soon" |
| Error | Red | "Provider connection failed" |

**Behavior:**
- **Duration:** 4 seconds before auto-dismiss
- **Progress bar** — Shows remaining time on the newest toast
- **Dismissible** — Click the × button to close immediately
- **Fade-out** — Toasts fade out in the last 2 seconds
- **Stacking** — Multiple toasts stack vertically

### Bottom Panel

The bottom panel provides tabbed access to build output, diagnostics, and more. It is resizable (drag the top edge, max height 600px) and collapsible.

**Tabs vary by workspace mode:**

| Mode | Tabs |
|------|------|
| **Coder** | Terminal, Problems, Output, Checkpoints, Chat |
| **Automation Operator** | Split view: Live Action Preview + Console |
| **Mission Control** | Dashboard (agent grid) |
| **Accessibility** | Audit Results, Keyboard Nav Map, Chat |

**Coder mode tabs:**
- **Terminal** — Command input with `$ ` prompt and output buffer
- **Problems** — Error and warning counts with diagnostic messages (see [Diagnostics](#diagnostics-problems-panel))
- **Output** — Build and run output with color-coded lines (errors in red, warnings in amber, commands in accent color)
- **Checkpoints** — Workspace checkpoint list with restore/discard actions (see [Workspace Checkpoints](#workspace-checkpoints))
- **Chat** — Quick chat access

### Accessibility Mode

Accessibility mode (`Ctrl+4`) provides comprehensive features for users with disabilities:

**Screen Reader Simulation:**
- Simulates a screen reader that announces focused elements
- Navigate with `Tab` / `Shift+Tab` through focusable elements
- Speech buffer shows recent announcements
- Elements are announced with their role, name, and value

**Accessibility Tree:**
- Hierarchical tree of all UI elements with 29 ARIA roles: Alert, Button, Checkbox, Dialog, Document, Form, Heading, Image, Link, List, ListItem, Menu, MenuItem, Navigation, ProgressBar, Radio, Region, Search, Slider, StatusBar, Tab, TabList, TabPanel, TextBox, Toolbar, Tree, TreeItem, Window, and more
- Each element shows its role, name, bounds, and focusability
- Available as a right panel: **Accessibility Tree** (icon ⬿)

**High Contrast Palette:**
- Dark mode: Pure black background `[0,0,0]`, white foreground `[255,255,255]` — **21:1 contrast ratio** (exceeds WCAG AAA)
- Light mode: White background, black foreground
- WCAG 2.1 relative luminance formula used for contrast calculations
- Customizable per-element colors for selection, cursor, errors, warnings, and focus ring

**Right panels in Accessibility mode:**
| Panel | Icon | Purpose |
|-------|------|---------|
| Accessibility Tree | ⬿ | Browse the full UI element hierarchy |
| Contrast Checker | ◐ | Verify contrast ratios meet WCAG standards |
| ARIA Inspector | ⊜ | Inspect ARIA roles and properties of elements |

**Toolbar actions:** Audit (✓), Contrast (◐), SR Sim (♿)

**Keyboard navigation map:** 10 default bindings for full keyboard control — see the [Accessibility Mode shortcuts](#accessibility-mode) table in the Keyboard Shortcuts Reference.

---

## Command Palette

The Command Palette is a fuzzy-search overlay for quickly executing any IDE command.

**Opening:** `Ctrl+Shift+P` or `Ctrl+P`

**How it works:**
1. Type to filter commands using fuzzy matching (e.g., "tsb" matches "Toggle Sidebar")
2. Matched characters are highlighted with accent color
3. Use arrow keys to navigate, `Enter` to execute, `Escape` to close
4. Commands are grouped by category with headers

**Available command categories:**

| Category | Examples |
|----------|----------|
| **File** | New File, Open File, Quick Open, Save, Save As, Save All, Close Tab, Close Other Tabs, Reopen Closed Tab, Go to Line, Go to Symbol, Go to Definition, Find All References, Show Hover Info, Next Tab, Previous Tab |
| **Build** | Build, Run, Deploy Pipeline, Rollback Deploy, Test Generator, Test Coverage |
| **Edit** | Find, Find & Replace |
| **Panels** | Chat, Output, Orchestrator, Mission Control, Search, Usage, Settings, Extensions, Voice Commands, Live Activity, Test Coverage |
| **Agent** | Request Inline Suggestion, Approve All Tools, Decline All Tools, Plan Sub-Agents, Refresh Models |
| **Workspace** | Switch Mode (Coder/Operator/Mission/Accessibility), Reset Layout, Wiki Export, NDA Document operations, Switch Workspace |
| **View** | Toggle Sidebar, Toggle Minimap, Toggle History, Split Editor, Reset Layout |
| **Knowledge** | Code Graph, Knowledge Base, Bookmarks, Agent Memory, Shared Memory |
| **Automation** | Triggers, Workflows, Governance |

Commands are context-aware — some only appear in specific workspace modes. Keyboard shortcuts are shown right-aligned in monospace next to each command.

---

## Menu Bar Reference

The menu bar at the top provides access to all IDE features. Here is the complete menu structure:

### File Menu
| Item | Shortcut | Description |
|------|----------|-------------|
| New File | `Ctrl+N` | Create a new untitled file |
| Open File... | `Ctrl+O` | Browse and open a file |
| Quick Open | `Ctrl+P` | Fuzzy-search file by name |
| Save | `Ctrl+S` | Save the active file |
| Save All | `Ctrl+Shift+S` | Save all modified files |

### Navigate Menu
| Item | Shortcut | Description |
|------|----------|-------------|
| Command Palette | `Ctrl+P` | Open the command palette |
| Chat | `Ctrl+J` | Focus the chat panel |
| Search | `Ctrl+Shift+F` | Focus the search panel |
| Research browser | — | Open the research browser panel |
| Review changes | — | Open the git review panel |
| Output | `` Ctrl+` `` | Toggle the output panel |
| Terminal | — | Open the terminal |

### Build Menu
| Item | Shortcut | Description |
|------|----------|-------------|
| Build | `Ctrl+B` | Trigger a project build |
| Run | `Ctrl+R` | Run the project |
| Test generator | — | Generate tests for selected code |
| Test coverage | — | View test coverage analysis |
| Deploy pipeline | — | Open the deploy pipeline view |
| Debugger | — | Open the debugger |
| Language servers | — | View LSP status |
| Snippets | — | Manage code snippets |
| Inline suggestions | `Ctrl+Shift+I` | Trigger AI inline suggestion |
| Build cache | — | View build cache status |

### Tools Menu

The Tools menu has three submenus:

**Tools > Agents:**
Live Activity, Agent Roster, Background Agents, Orchestration, Task Queue, Timeline, Mission Metrics, Conflict Resolver, Self-Improvement, Session Continuity

**Tools > Knowledge:**
Knowledge Base, Wiki, Code Graph, Semantic Search, Bookmarks, Favorites, Agent Memory, Shared Memory, Persistent Memory, Recent Changes

**Tools > Automation:**
Workflows, Triggers, Automation Flows, Targets, Execution Logs, Recordings, Voice Commands, Multimodal, Accessibility, Governance

### Workspace Menu
| Item | Description |
|------|-------------|
| Extensions | Open the extensions panel |
| Plugin registry | Open the MCP plugin registry |
| Skills | Manage agent skills |
| Collaboration | Open the collaboration panel |
| Peers | View connected peer devices |
| Usage | View API usage statistics |
| New NDA document | Create a new NDA document |
| Import active file to NDA | Convert the current file to NDA format |
| Open NDA browser viewer | Launch the PWA viewer for NDA documents |

### Help Menu
| Item | Shortcut | Description |
|------|----------|-------------|
| Keyboard Shortcuts | `F1` | Show all keyboard shortcuts |
| Settings | `Ctrl+,` | Open IDE settings |

---

## Working with Files

### File Tree

The file tree shows your workspace structure with:

- **Folder expand/collapse** — Click arrows or folder names
- **File icons** — Language-specific icons for common file types
- **Filter** — Type in the "Filter files..." box to narrow the tree
- **Context menu** — Right-click files for Open, Rename, Delete, Copy Path

**Opening files:** Click any file to open it in the editor. Multiple files open in tabs.

### Bookmarks & Favorites

**Bookmarks** mark specific locations in files for quick navigation:
- Toggle bookmark with `Ctrl+Shift+B` (or right-click → Toggle Bookmark)
- Navigate bookmarks with the Bookmarks sub-panel

**Favorites** pin frequently-used files to the top of the file tree:
- Right-click a file → **Add to Favorites**
- Access favorites from the Favorites sub-panel under Files

### Search & Replace

**Project-wide search** (`Ctrl+Shift+F`):
- Case-insensitive by default
- Skips hidden files, `target/`, `node_modules/`
- Shows file path, line number, and matching line
- Click a result to jump to that location

**Replace all:**
1. Open Search panel
2. Enter search term and replacement
3. Click **Replace All** — shows count of files changed and replacements made

**In-file find/replace** (`Ctrl+F` / `Ctrl+H`):
- Overlay appears in the editor
- Toggle case sensitivity, regex, whole word
- `F3` / `Shift+F3` to cycle through matches
- Replace one or replace all in current file

### Semantic Search

Semantic search finds code by meaning, not exact text:
- Uses TF-IDF + cosine similarity over code symbols
- Finds related functions, types, and variables even with different names
- Access via **Search → Semantic** sub-panel

### Code Graph

The code graph visualizes symbol relationships:
- Shows function call chains, type dependencies, module structure
- Interactive — click nodes to navigate
- Access via **Search → Code Graph** sub-panel

### Outline

The Outline panel shows the symbol structure of the currently open file:
- Lists functions, structs, classes, enums, and other symbols
- Click any symbol to jump to its definition
- Updates automatically as you switch between files
- Uses both keyword-based extraction and LSP document symbols
- Available as a sidebar tab in Coder mode

### Local File History

Velocity IDE automatically saves snapshots of your files as you edit, so you can recover previous versions:

- **Snapshots per file:** Up to 20 automatic snapshots
- **Storage:** `.velocity/history/` (NDA-encrypted index)
- **Age labels:** Each snapshot shows relative time (e.g., "5m ago", "2h ago", "3d ago")
- **Diff view:** Compare any snapshot against the current version with a line-based diff (`+` added, `-` removed)
- **Recovery:** Click a snapshot to view its content; restore it to replace the current file

File history is completely automatic — no configuration needed. Every time you save a file, a snapshot is recorded. The oldest snapshots are pruned when the limit of 20 is reached.

---

## Code Editing

### Editor Features

The code editor supports:

- **Syntax highlighting** — 30+ languages via syntect
- **Auto-indent** — Automatic indentation on new lines
- **Bracket matching** — Highlights matching brackets
- **Breadcrumbs** — Navigation path at top of editor
- **Minimap** — Overview of file structure (right side), toggle with `Ctrl+Shift+M`
- **Line numbers** — Click to set breakpoints
- **Word wrap** — Toggle with `Alt+Z` or via Settings → Editor
- **Split editor** — `Ctrl+\` opens the same file in a side-by-side view for viewing different parts simultaneously
- **Hover info** — Shows LSP hover information for the symbol under the cursor (also available via Command Palette → Show Hover Info)

### Unsaved Changes

When you close a tab with unsaved changes (`Ctrl+W`), a confirmation dialog appears with three options:
- **Save** — Save changes and close
- **Don't Save** — Discard changes and close
- **Cancel** — Keep the tab open

### Find & Replace

**Find** (`Ctrl+F`):
- Opens find overlay in editor
- Case sensitive toggle, regex support, whole word toggle
- Match highlighting with count
- `F3` next match, `Shift+F3` previous match

**Replace** (`Ctrl+H`):
- Extends find overlay with replacement field
- Replace one (current match) or Replace All (in file)

### Code Folding

Fold code blocks to focus on high-level structure:

| Action | Shortcut |
|--------|----------|
| Fold current block | `Ctrl+Shift+[` |
| Unfold current block | `Ctrl+Shift+]` |
| Fold all | `Ctrl+K Ctrl+0` |
| Unfold all | `Ctrl+K Ctrl+J` |

### Inline Suggestions

AI-powered inline code suggestions:
- Trigger with `Ctrl+Shift+I`
- Suggestions appear as ghost text
- Press `Tab` to accept, `Escape` to dismiss

### Code Snippets

Snippets are templates for common code patterns with interactive placeholders:

**Using snippets:**
1. Type a snippet prefix (e.g., `fn`, `test`, `struct`)
2. Press `Ctrl+Space` to trigger completion
3. The snippet expands with highlighted placeholders
4. Press `Tab` to jump to the next placeholder, `Shift+Tab` to go back
5. Press `Escape` to exit the snippet session

**Placeholder syntax:**
| Syntax | Description | Example |
|--------|-------------|---------|
| `$N` | Tab stop (cursor position) | `$1`, `$2`, `$0` (final) |
| `${N:default}` | Placeholder with default text | `${1:my_function}` |
| `${N\|opt1,opt2\|}` | Choice placeholder | `${1\|pub,priv\|}` |
| `${N/regex/format/}` | Transform placeholder | `${1/./\u$0/}` |

**Built-in variables:** `$TM_FILENAME` (current file name), `$TM_FILENAME_BASE` (name without extension), `$CLIPBOARD` (clipboard content)

**Custom snippets:** Create `.velocity/snippets.json` in your workspace using VS Code-compatible format:

```json
{
  "Print Debug": {
    "prefix": "dbg",
    "body": ["println!(\"${1:debug}: {:?}\", ${1:value});"],
    "description": "Print debug statement"
  }
}
```

**Built-in Rust snippets:** `fn`, `impl`, `test`, `match`, `struct`, `enum`, `for`, `if`

### Diagnostics (Problems Panel)

The Problems panel shows compiler and linter diagnostics for your codebase:

**Opening:** Click the **Problems** tab in the bottom panel, or note the error/warning counts in the status bar.

**Features:**
- **Error/warning squiggles** — Colored underlines in the editor (red = error, yellow = warning, blue = info, grey = hint)
- **Inline popups** — Hover over a squiggly line to see the full diagnostic message
- **Filter tabs:** All, Errors, Warnings
- **Per-file counts** — Error and warning counts shown per file
- **Click to jump** — Click any diagnostic to navigate to the exact file, line, and column

**Severity levels:**
| Level | Indicator | Color |
|-------|-----------|-------|
| Error | Red squiggle | Red |
| Warning | Yellow squiggle | Amber |
| Info | Blue squiggle | Blue |
| Hint | Grey squiggle | Grey |

---

## AI Chat & Agents

### Chat Panel

The chat panel is your primary interface for AI-assisted development.

**Opening chat:** `Ctrl+J` or click the Chat icon in the activity bar.

**Chat features:**
- **Multi-turn conversation** — Context is preserved across messages
- **Markdown rendering** — Headings, code blocks with copy button, lists, bold/italic
- **Suggestion chips** — Quick-start prompts:
  - "Explain this codebase"
  - "Find and fix bugs"
  - "Write tests for my code"
  - "Refactor a module"
- **Message history** — Last 200 messages, persisted across sessions
- **Clear** — Reset conversation context
- **Interrupt** — Cancel running agent task

**Sending messages:**
- `Enter` — Send message
- `Shift+Enter` — New line
- `Ctrl+L` — Focus chat input

### Model Selection

Velocity IDE supports 16 AI providers with automatic failover:

| Provider | Default Model | Notes |
|----------|---------------|-------|
| Cloudflare Workers AI | `@cf/moonshotai/kimi-k2.7-code` | Default, no API key needed |
| OpenRouter | `tencent/hy3:free` | Access to 100+ models |
| OpenAI | `gpt-4o` | Requires API key |
| Anthropic | `claude-3-5-sonnet-20241022` | Requires API key |
| Azure OpenAI | `gpt-4o` | Requires deployment endpoint |
| Local Ollama | `llama3.2` | Runs locally, no API key |
| Deepseek | `deepseek-chat` | Requires API key |
| Google Vertex | `gemini-1.5-pro` | Requires GCP credentials |
| Groq | `llama-3.3-70b-versatile` | Requires API key |
| Mistral | `mistral-large-latest` | Requires API key |
| Together AI | `meta-llama/Meta-Llama-3.1-405B-Instruct-Turbo` | Requires API key |
| Fireworks AI | `accounts/fireworks/models/llama-v3p3-70b-instruct` | Requires API key |
| Perplexity | `sonar-pro` | Requires API key |
| Cerebras | `llama-3.3-70b` | Requires API key |
| AWS Bedrock | `anthropic.claude-3-sonnet-20240229-v1:0` | Requires AWS credentials |
| Alibaba Qwen | `qwen-plus` | Requires API key |

**Changing models:**
1. Open Chat panel
2. Click the model dropdown at the top
3. Select a provider, then a model

> **First time setup?** See [Registering a Provider](#registering-a-provider) for step-by-step instructions on configuring credentials and refreshing the model catalog.

**Reasoning toggle:** Click the "Show thoughts" checkbox to see the agent's reasoning process.

### Agent Approvals

When the agent wants to perform actions (edit files, run commands, etc.), it requests approval:

- **Pending approvals** appear in the chat panel
- **Approve** — Allow the action
- **Decline** — Block the action
- **Auto-approve** — Check the box to automatically approve all actions (use with caution)

### Voice Commands

Voice commands let you control the IDE hands-free (Windows only):

**Activating voice:** `Ctrl+Shift+V` or click the Voice sub-panel under Chat.

**Supported intents:**
- Open file — "Open main.rs"
- Search — "Search for authentication"
- Run tests — "Run the tests"
- Build — "Build the project"
- Deploy — "Deploy to production"
- Fix error — "Fix the last error"
- Refactor — "Refactor this function"
- Create — "Create a new file called utils.rs"
- Undo/Redo — "Undo that"
- Save — "Save the file"
- Agent task — "Ask the agent to explain this code"
- Show panel — "Show the git panel"
- Navigate — "Go to line 42"

### Multimodal Input

Attach files to chat messages for context:
1. Click the paperclip icon in the chat input
2. Enter the file path or browse
3. The file content is included in the agent's context

---

## MCP Server Integration

Velocity IDE includes a built-in **Model Context Protocol (MCP)** server that allows external AI assistants (Claude Desktop, ChatGPT, Cursor, etc.) to interact with your workspace.

### How It Works

The MCP server runs as a separate process and communicates via **JSON-RPC 2.0** over stdio — the standard MCP transport. External assistants spawn the server as a child process and exchange requests/responses over stdin/stdout.

### Starting the MCP Server

```bash
# Default stdio mode (for external AI assistants)
velocity_mcp --mode stdio

# Shared memory mode (for IPC with the GUI process)
velocity_mcp --mode shmem --buffer-path <path>
```

### Configuring External Assistants

To connect an external AI assistant, add Velocity as an MCP server in the assistant's configuration:

**Claude Desktop** (`claude_desktop_config.json`):
```json
{
  "mcpServers": {
    "velocity-ide": {
      "command": "C:\\path\\to\\velocity_mcp.exe",
      "args": ["--mode", "stdio"]
    }
  }
}
```

**Other MCP-compatible assistants** follow the same pattern — specify the binary path and `--mode stdio` argument.

### Available MCP Tools

The server exposes 250+ tools across 5 categories:

**System Tools** (28 tools):
| Tool | Description |
|------|-------------|
| `read_file` | Read file contents |
| `write_file` | Write file contents |
| `list_dir` | List directory contents |
| `delete_file` | Delete a file |
| `grep_search` | Search files with regex |
| `run_command` | Execute a shell command |
| `convert_to_nda` | Convert a file to NDA document format |
| `read_nda` | Read an NDA document |
| `execute_nda` | Execute an NDA program |
| `fetch_panel_data` | Get data from IDE panels (teams, wiki, graph, bookmarks, files) |
| `agent_checkpoint_create` | Create an agent checkpoint |
| `agent_checkpoint_restore` | Restore from a checkpoint |
| `agent_checkpoint_list` | List all checkpoints |
| `agent_memory_remember` | Store a memory for the agent |
| `agent_memory_recall` | Search agent memories |
| `agent_memory_forget` | Delete an agent memory |
| `code_generate_tests` | Generate tests for code |
| `code_coverage_analyze` | Analyze test coverage |
| `knowledge_ingest` | Ingest text into the knowledge base |
| `knowledge_search` | Search the knowledge base |
| `workflow_run` | Run an automation workflow |
| `connector_call` | Call an external connector |
| `generate_image` | Generate an image via AI |
| `describe_image` | Describe an image via AI |
| `gui_open_file` | Open a file in the GUI |
| `gui_get_state` | Get current GUI state |
| `gui_navigate_panel` | Navigate to a GUI panel |
| `gui_quit` | Quit the GUI application |

**Browser Tools** (109 tools): `web_navigate`, `browser_create_session`, `browser_runtime_capture`, `browser_runtime_visual_capture`, and many more for headless browser automation.

**Windows Automation Tools** (86 tools): UI automation, screenshot capture, registry access, advanced input simulation.

**Team Tools** (20 tools): `create_expert_team`, `create_skill_file`, `list_expert_teams`, `list_skills`, `update_expert_team`, `update_team_member`, `add_team_member`, `remove_team_member`, `validate_team`, `check_scope_overlaps`, `clone_expert_team`, `export_expert_team`, `import_expert_team`, `debug_routing`, `team_analytics`, `team_health_check`, `list_providers`, `create_team_quick`, `bulk_import_members`, `team_changelog`.

**Drone Tools** (10 tools): Deploy and control remote drones for distributed execution, screen capture, GUI automation, and network monitoring.
| Tool | Description |
|------|-------------|
| `drone_deploy` | Deploy the drone binary to a remote machine via SSH |
| `drone_command` | Send a shell command to a drone for async execution |
| `drone_task_status` | Check the status of a submitted drone task |
| `drone_status` | Query drone health, identity, and capabilities |
| `drone_screenshot` | Capture a screenshot from the remote machine |
| `drone_type_keys` | Simulate keyboard input on the remote machine |
| `drone_click` | Simulate a mouse click at coordinates on the remote machine |
| `drone_network_stats` | Get network statistics from the remote machine |
| `drone_upload` | Upload a file to the remote drone with SHA-256 verification |
| `drone_pair` | Pair a drone with the IDE for peer protocol messaging |

### GUI Control Bridge

In addition to MCP, the running IDE exposes a **TCP control bridge** at `localhost:19821` for external processes to control the GUI:

| Command | Description |
|---------|-------------|
| `OpenFile { path }` | Open a file in the editor |
| `GetState` | Get current IDE state as JSON |
| `NavigatePanel { panel }` | Switch to a specific panel |
| `Screenshot` | Capture the IDE window as an image |
| `Quit` | Close the IDE |

This is useful for scripting IDE interactions or integrating with CI/CD pipelines.

---

## Browser Automation

### Browse Panel

The Browse panel provides AI-powered web research in the sidebar:

1. Open **Chat → Browse** sub-panel (or use the Browse panel in Operator mode)
2. Enter a plain-language question or URL + question
3. The IDE spawns a headless browser agent
4. Results stream back as a transcript

**Example queries:**
- "What's the latest version of Rust?"
- "Find the API docs for egui 0.35"
- "Check if github.com/UnitBuilds is accessible"

### Automation Flows

In **Automation Operator** mode (`Ctrl+2`), you can create and run automation flows:

**Flows panel** — Lists all automation flows with status (running/failed), step count, and last run time.

**Creating a flow:**
1. Open Operator mode (`Ctrl+2`)
2. Click **New Flow**
3. Add steps: Agent Task, Tool Call, Connector, or Condition
4. Configure each step
5. Click **Run Flow** (`Ctrl+Enter`)

**Flow types:**
- **Sequential** — Steps run in order
- **Branching** — Conditional paths based on results

### Targets & Recordings

**Targets** — Registered websites for automation:
- URL, label, last visited timestamp
- Used by flows to know which sites to interact with

**Recordings** — Saved action sequences:
- Record a browsing session
- Replay it as an automation flow
- Edit steps to parameterize

### Windows Desktop Automation

In **Automation Operator** mode (`Ctrl+2`), Velocity IDE provides full Windows desktop automation via UI Automation (UIA):

**Recording desktop interactions:**
1. Open **Automation → Recordings** panel
2. Click **Start Recording**
3. Interact with any Windows application normally
4. Click **Stop Recording** — the session is saved as a replayable WaScript artifact

**What gets recorded:** Clicks, double-clicks, text input, key combinations, focus changes, scroll events, drag-and-drop, and window activations — each with precise UIA node targets and timing.

**UIA capabilities:**
- **Element selection** — Find any UI element by automation ID, name, control type, or class name
- **Advanced input** — Simulate keyboard and mouse input at the OS level
- **Clipboard** — Read/write clipboard content
- **Screenshots** — Capture the screen or specific windows
- **OCR** — Read text from screen regions using optical character recognition
- **Window management** — Move, resize, minimize, maximize, and arrange windows
- **Multi-monitor** — Automate across multiple displays
- **Virtual desktops** — Switch between Windows virtual desktops
- **Process management** — Start, stop, and monitor processes
- **File dialogs** — Handle native open/save file dialogs
- **Toast notifications** — Read and interact with Windows notifications
- **Windows Registry** — Read registry keys for automation configuration

**Cross-context bridge:** Velocity can automate workflows that span both browser and desktop — for example, download a file in the browser, then automatically open it in a desktop application. The bridge handles context switching, file appearance monitoring, and clipboard transfers.

**Automation triggers:** Set up automatic triggers that fire when conditions are met (file changes, build completion, specific log patterns, etc.). Configure via **Automation → Triggers**.

---

## Git Integration

### Changes Panel

The Git Changes panel shows modified files:

- **Status indicators:** Modified (M), Added (A), Deleted (D), Renamed (R), Untracked (?), Conflicted (!)
- **Stage/unstage** — Click the +/− icons or right-click → Stage/Unstage
- **Stage All / Unstage All** — Buttons at the top
- **Diff view** — Click a file to see the diff
- **Commit** — Enter a commit message and click **Commit**

### Branches & Commits

**Branches panel:**
- Current branch with ahead/behind counts
- List of all branches
- Create, delete, switch branches

**Commits panel:**
- Git log with hash, author, date, message
- Click a commit to see the diff
- Checkout previous commits

---

## Wiki & Knowledge

### Wiki System

The Wiki automatically generates documentation from your codebase:

**Opening wiki:** Click **Knowledge → Wiki** in the activity bar.

**Wiki features:**
- **Tree view** — Pages organized by category (Overview, Files, Symbols)
- **Filter** — Search box to find pages
- **Page detail** — Title, kind label, summary, relationships (calls/called-by)
- **Navigation** — Click relationships to jump between pages

**Toolbar actions:**
- **Refresh** — Rebuild wiki from current site map
- **Export Markdown** — Write interlinked `.wiki/` pages to disk (committable to git)
- **Rebuild Index** — Compile all `.rs` files to populate the name dictionary (use after adding new files)
- **Generate Detailed Page** — Ask the agent to write a narrative for the selected page

### Knowledge Base

The knowledge base stores chunks of text for semantic retrieval:

- **Chunk size:** 25 lines / 1600 characters per chunk
- **Ranking:** TF-IDF + cosine similarity
- **Storage:** `.velocity/knowledge/store.json`
- **Ingest:** Text, file paths, or directories recursively
- **Supported extensions:** 28 (md, txt, rs, py, js, ts, tsx, jsx, go, java, c, cpp, h, hpp, cs, rb, toml, yaml, yml, json, csv, sql, sh, html, css, log, ini, cfg)

**Usage:** The agent queries the knowledge base when answering questions about your codebase.

### Agent Memory

Agent memory persists learnings across sessions:

- **Per-member storage** — Each agent member has isolated memory
- **Tag-based organization** — Memories are tagged with freeform labels (e.g., `tool`, `file_io`, `success`) for filtering
- **Search:** Keyword-based with relevance scoring (0.0–1.0)
- **Encryption:** NDA-encrypted storage per member ID
- **Context injection:** Memories are automatically injected into agent prompts

**Viewing memory:** Open **Agents → Memory** sub-panel.

### Persistent Memory

Beyond per-session agent memory, Velocity IDE maintains **persistent memory** that survives across restarts:

- **NDA-encrypted at rest** — Stored in `.velocity/` with AES-256-GCM encryption
- **Per-workspace** — Each workspace has its own isolated memory store
- **Tag-based organization** — Entries are tagged with freeform labels for filtering and search
- **Agent-accessible** — Agents can `remember`, `recall`, and `forget` entries via MCP tools
- **Self-improvement integration** — The self-improvement engine stores learned failure patterns here

### Shared Memory (Multi-Agent Collaboration)

When multiple agents work together, **shared memory** provides a common knowledge store:

- **Knowledge entries** — Structured entries with title, content, category, tags, and access level
- **Categories:** Architecture, Conventions, Known Issues, Guides, Project Facts, Agent Patterns, Notes
- **Access control:** Public, Team Only, or Private per entry
- **File annotations** — Attach notes, warnings, TODOs, or questions to specific file locations and line ranges
- **Search** — Keyword search across all entries, filterable by tag or category

**Viewing shared memory:** Open **Tools → Knowledge → Shared Memory**.

---

## Orchestrator & Agent Teams

### Orchestrator

The Orchestrator is a **meta-agent control plane** that decomposes large goals into a parallel task graph and dispatches sub-agent workers.

**Opening:** `Ctrl+Shift+Y` or **Tools → Agents → Orchestration**

**How it works:**
1. You provide a high-level goal
2. The orchestrator decomposes it into a **Task Graph** (DAG) of scoped sub-tasks
3. Tasks are topologically sorted into parallel phases — independent tasks run concurrently
4. Each task is assigned to a sub-agent worker with its own provider, model, and thinking configuration
5. The orchestrator monitors workers, detects file collisions, and validates outputs

**Task lifecycle:**
| Status | Meaning |
|--------|---------|
| Pending | Waiting to be dispatched |
| Running | Actively being processed by a worker |
| Done | Completed successfully |
| Failed | Worker encountered an error |
| Blocked | Waiting on a dependency or collision |

**Collision detection:** When multiple tasks modify the same file, the orchestrator detects the conflict and either serializes the tasks or flags them for manual resolution. Scope violations (files touched outside a task's declared scope) are also caught.

**UI features:**
- **Execute** — Start the task graph
- **Reset** — Clear all task statuses and start over
- **Retry Blocked** — Retry tasks that are stuck
- **Policy editor** — Customize orchestration policies
- **Live monitoring** — Real-time stats: tasks, phases, done, blocked, active workers

### Team Studio

The Team Studio lets you create and manage **expert teams** of specialized AI agents.

**Opening:** **Agents → Roster** or the Team Studio panel

**Creating a team:**
1. Click **New Team**
2. Enter a name and optional purpose
3. The team card expands for editing

**Adding agents to a team:**
1. Click **New Agent** within a team
2. Enter name, role/specialty, scope paths (comma-separated file patterns), and operating instructions
3. The agent inherits the current provider and model

**Team gallery:** Expandable cards show each team's members with their name, role, provider, model, skills, and workflow instructions.

**Launching a team:**
1. Click **Launch Team** on an expanded team card
2. All agents start working on their assigned scopes
3. Use **Cancel Running** to stop all agents

**AI-assisted team creation:** The Team Builder Chat at the bottom lets you describe a team in natural language — the AI creates the team structure for you.

**Usage syntax:** Once created, reference teams with `@<slug> <task>` or "send it to the \<name\> team".

### Agent Roster

The Agent Roster sidebar shows all active agents with:
- **Live status indicators** — Running (green), Idle (grey), Failed (red), Blocked (yellow)
- **Orchestrator snapshot** — Done/failed/running/pending task counts
- **Runtime status** — Active worker count and uptime
- **Quick actions** — Inspect, cancel, or reassign individual agents

### Background Agents

Background agents are **autonomous monitors** that run independently of the main agent loop:

**Opening:** **Tools → Agents → Background Agents**

**Default monitors:**
| Monitor | Interval | Default State |
|---------|----------|---------------|
| Git Status | 60 seconds | Enabled |
| Build Health (`cargo check`) | 5 minutes | Disabled (opt-in) |
| Dependency Updates (`cargo outdated`) | 24 hours | Disabled (opt-in) |

**Monitor types:**
- **File Changes** — Watch a directory for modifications matching patterns
- **Build Health** — Run build/test commands periodically
- **Dependency Updates** — Check for outdated dependencies
- **Log Errors** — Scan log files for error patterns
- **Git Status** — Monitor uncommitted changes and behind-remote status
- **Custom** — User-defined periodic check with a custom prompt

**Actions:** When a monitor detects something, it creates an action with severity (Info, Suggestion, Warning, Critical), title, description, and suggestion. Actions appear in the RECENT ACTIONS section and can be acknowledged.

**Configuration:** State is persisted to `.velocity/background_agents.json`. Enable/disable monitors and adjust intervals from the UI.

### Self-Improvement Engine

The self-improvement engine **automatically learns from agent failures** to improve future sessions:

**Opening:** **Tools → Agents → Self-Improvement**

**How it works:**
1. During a session, every tool failure is classified into a category (Syntax, Logic, Permission, Timeout, Dependency, Not Found, Rejected, Network, Unknown)
2. At session end, patterns with 2+ occurrences generate corrective prompt directives
3. These directives are stored in persistent memory
4. At the start of future sessions, relevant directives are loaded and injected into the agent's system prompt

**Example directives:**
- *Syntax:* "Before writing code, verify syntax against the target language's grammar..."
- *Permission:* "Check file permissions and ownership before write operations..."
- *Timeout:* "Break long operations into smaller steps..."

**User interaction:** Fully automatic — no configuration needed. The engine runs silently as part of the agent loop. You can view accumulated failure patterns and success statistics in the Self-Improvement panel.

### Conflict Resolver

When multiple agents or users operate concurrently, the conflict resolver tracks and manages resource contention:

**Opening:** **Tools → Agents → Conflict Resolver**

**Locking model:**
- **Exclusive locks** — Block all other actors (same actor can re-lock)
- **Shared locks** — Coexist with other shared locks, block exclusive locks
- **Auto-expiry** — Locks expire after 5 minutes to prevent deadlocks

**Conflict detection:** Write+write, create+create, delete+anything, read+write, and execute+update operations all conflict. Read+read never conflicts.

**Resolution strategies:**
| Strategy | Description |
|----------|-------------|
| Keep Latest | Use the most recent modification (default) |
| Keep First | Use the first operation's result |
| Keep Second | Use the second operation's result |
| Merge | Attempt to merge both changes |
| Discard Both | Discard both and flag for manual resolution |
| Manual | Require user intervention |

**Semantic coupling:** The resolver uses the SiteMap to detect when different files call each other, preventing concurrent edits to tightly coupled code even if they don't overlap line-by-line.

### Session Continuity (Continuation Ledger)

The continuation ledger enables **cross-model context handoff** when a model fails mid-task or gets swapped:

**Opening:** **Tools → Agents → Session Continuity**

**What it captures:**
- **Mission spec** — Goal, task kind, expectations, constraints
- **Scope environment** — Files in scope with symbols, line counts, and roles
- **Edit journal** — Completed edits (with diffs and intents) and any partial in-progress edit
- **Progress state** — Step-by-step checklist with Done/InProgress/Pending/Failed statuses
- **Model provenance** — Which model attempted what, how long it took, and why it failed

**How it helps:** When a new model takes over, it receives a structured continuation prompt with all the context needed to resume without gaps or duplication — no raw transcript dumps, just structured deltas and progress markers.

---

## Build & Deploy

### Build Panel

The Build panel shows compilation output:

- **Build button** — Trigger a build (`Ctrl+B`)
- **Live output** — Streaming build logs
- **Error/warning counts** — Summary at the top
- **Click errors** — Jump to the source location

### Test Generator

The Test Generator creates tests automatically:

1. Open **Build → Test** sub-panel
2. Select a file or module
3. Click **Generate Tests**
4. Review and accept generated tests

### Deploy Pipeline

The Deploy Pipeline manages deployment workflows:

- **Pipeline view** — Visual representation of deployment stages
- **Rollback** — `Ctrl+Alt+R` to rollback the last deployment
- **Status** — Shows current deployment state

### Debugger

The debugger supports:

- **Breakpoints** — `F9` to toggle, click line numbers
- **Step over** — `F10`
- **Step into** — `F11`
- **Step out** — `Shift+F11`
- **Continue** — `F5`
- **Stop** — `Shift+F5`

### Workflow Canvas

The Workflow Canvas is a **visual, node-based designer** for building multi-step agent workflows:

**Opening:** **Tools → Automation → Workflows**

**Node types:**
| Node | Color | Purpose |
|------|-------|---------|
| Start | Green | Entry point — every workflow begins here |
| Agent Task | Blue | Sends a prompt to an AI agent (optionally routed to a team) |
| Tool | Orange | Executes an MCP tool with arguments |
| Connector | Purple | Makes an HTTP request via a configured connector |
| Condition | Deep Orange | Branch point — evaluates a description to true/false |
| End | Grey | Exit point — workflow terminates here |

**Canvas controls:**
- **Drag nodes** to reposition them
- **Connect ports** — drag from an output port (`ok`, `fail`, `true`, `false`) to another node's input
- **Pan** — middle mouse button or `Ctrl+drag`
- **Zoom** — scroll wheel (range: 0.3× to 3.0×)
- **Delete** — select a node and press Delete

**Execution:** Click **Run** to execute the workflow. Nodes are processed in topological order. Each node shows a status overlay: Idle, Running, Succeeded, Failed, or Skipped. The canvas detects cycles and prevents execution of invalid graphs.

### Workflow Templates

Eight pre-built templates provide one-click workflow creation:

| Template | Category | Description |
|----------|----------|-------------|
| Code Review Pipeline | Code Quality | Compilation check, lint, then summarize changes |
| Test & Report | Testing | Run tests, check coverage, generate summary |
| Safe Refactor | Code Quality | Analyze code, apply refactor, validate with tests |
| Research & Document | Research | Browse web for topic, summarize findings, write docs |
| Build, Deploy & Verify | Deployment | Build project, deploy, run smoke tests |
| Bug Investigation | Review | Read logs, analyze error, propose fix, validate |
| Feature Implementation | Automation | Plan feature, implement, test, document |
| Dependency Audit | Code Quality | Check outdated deps, analyze breaking changes, update safely |

To use a template: open the Workflow Canvas, click **New from Template**, and select one. The canvas is pre-populated with nodes and edges — customize as needed.

### Workflow Version History

Every workflow save creates an automatic version snapshot:

- **Versioning** — Auto-incrementing version numbers starting at 1
- **Browse history** — View all versions with timestamps and notes
- **Compare versions** — Structural diff showing nodes/edges added and removed
- **Rollback** — Restore any previous version with one click
- **Storage:** `.velocity/workflow_versions/{id}.json`

### Triggers & Unattended Execution

Triggers automate workflow execution based on events or schedules:

**Opening:** **Tools → Automation → Triggers**

**Trigger types:**
| Type | Description | Example |
|------|-------------|---------|
| Schedule | Fire at regular intervals | `30s`, `5m`, `1h`, `2d`, `daily@09:00` |
| File Watch | Fire when files change | Path + glob pattern (e.g., `src/**/*.rs`) |
| Webhook | Fire on incoming HTTP POST | Token-authenticated endpoint |
| Manual | Fire on demand | Click to execute |

**Actions:** Each trigger performs one of:
- **Run Workflow** — Execute a specific workflow by ID
- **Agent Prompt** — Send a prompt directly to the agent

**Configuration:** Triggers are persisted to `.velocity/triggers.json`. Enable/disable individual triggers from the UI. Schedule triggers support both interval-based (`5m` = every 5 minutes) and daily-at (`daily@09:00` = 9 AM UTC) scheduling.

### Governance & Policy Engine

The Policy Engine controls what the agent is allowed to do:

**Opening:** **Tools → Automation → Governance**

**Policy rules:** Each rule matches on tool name, path prefix, or domain and applies one of:
| Effect | Description |
|--------|-------------|
| Allow | Permit the operation unconditionally |
| Deny | Block the operation |
| RequireApproval | Queue the operation for manual approval |

**Budget enforcement:** Set maximum token usage or cost limits. When the budget is exhausted, all operations are denied until reset.

**Approval queue:** Operations requiring approval appear in a queue with:
- Tool name, summary, and creation timestamp
- **Approve** or **Deny** buttons per item
- Status tracking: Pending → Approved/Denied

**Rule matching:** Rules are evaluated in order — first match wins. Tool names support wildcards (`*` matches any tool). Path prefixes match file paths. Domain matching checks URL-related arguments.

**Storage:** Policies are saved to `.velocity/policy.json`, approval queue to `.velocity/approvals.json`.

### Workspace Checkpoints

Checkpoints are **git-stash-based snapshots** taken before agent operations, letting you safely undo agent changes:

**Opening:** **Checkpoints** tab in the bottom panel

**How checkpoints work:**
1. Before an agent modifies files, a checkpoint is automatically created
2. The checkpoint is stored as a git stash with a descriptive label
3. If you're unhappy with the changes, click **Restore** to revert to the checkpoint
4. Or click **Discard** to remove the checkpoint and keep the changes

**Checkpoint info:** Each checkpoint shows its label, creation time, and number of files changed.

**Agent MCP tools:** `agent_checkpoint_create`, `agent_checkpoint_restore`, `agent_checkpoint_list` — agents can create and manage checkpoints programmatically.

### Precompilation Cache

The precompilation cache **speculatively indexes files** for faster agent execution:

**Opening:** **Build → Build Cache** or the Precomp Cache sidebar panel

**What it does:**
- Extracts symbol outlines (functions, structs, enums, traits, etc.) from source files
- Reads import statements from the first 50 lines of each file
- Generates top-level summaries for quick context
- Caches results by task ID for reuse across agent operations

**How it helps:** When an agent needs context about your codebase, it can pull from the precomp cache instead of re-reading and re-parsing files — significantly reducing latency for large projects.

**Warm-up:** The IDE automatically pre-indexes your open editor files on startup. Files larger than 2 MB are skipped.

---

## Plugins & Extensions

### Plugin Registry

The Plugin Registry manages MCP plugins:

- **Register** — Add a new plugin by ID
- **Unregister** — Remove a plugin
- **List** — View all registered plugins
- **Permissions** — Manage per-plugin permissions
- **Tool dispatch** — Plugins expose tools in `plugin_id::tool_name` format

### Extensions

Extensions live in `.velocity/extensions/` and can be WASM or Lua:

**Extension manifest:**
```json
{
  "id": "my-extension",
  "name": "My Extension",
  "version": "1.0.0",
  "author": "You",
  "description": "What it does",
  "entry_point": "main.wasm",
  "activation_events": ["onStartup", "onFileOpen"],
  "contributions": {
    "commands": [{"id": "myExtension.hello", "title": "Hello", "category": "MyExt"}],
    "keybindings": [{"key": "Ctrl+Alt+H", "command": "myExtension.hello"}],
    "themes": [{"label": "MyTheme", "path": "themes/my.json"}],
    "languages": [{"id": "myLang", "extensions": [".my"], "configuration": "./lang.json"}],
    "snippets": [{"language": "myLang", "path": "snippets/my.json"}]
  }
}
```

**States:** Installed, Active, Disabled, Error

### Skills

Skills are reusable instruction sets for the agent:

- **Skill files** — Markdown files that teach the agent specific workflows
- **Activation** — Skills activate based on context or explicit invocation
- **Management** — View and manage skills in **Workspace → Skills**

---

## Connectors & External Services

The Connector system lets the IDE integrate with external services like GitHub, GitLab, Jira, Slack, Discord, and Notion.

### Connector Types

| Service | Base URL | Auth Method |
|---------|----------|-------------|
| GitHub | `https://api.github.com` | Bearer token |
| GitLab | `https://gitlab.com/api/v4` | `PRIVATE-TOKEN` header |
| Jira | `https://api.atlassian.com/ex/jira/{cloud_id}` | Bearer token |
| Slack | `https://slack.com/api` | Bearer token |
| Discord | `https://discord.com/api/v10` | Bearer token |
| Notion | `https://api.notion.com/v1` | Bearer token + version header |
| Webhook | Arbitrary URL | None (POST-only) |
| Generic REST | Any URL | Bearer, custom header, or query param |

### Setting Up a Connector

1. Open **Workspace → Connectors** or use the `connector_call` MCP tool
2. Click **Add Connector**
3. Select a service template or choose Generic REST
4. Enter the base URL and authentication credentials
5. Click **Save** — credentials are stored encrypted in the secret store

### OAuth2 Integration

For services that support OAuth2 (GitHub, GitLab, Notion):
1. The IDE initiates an authorization code flow
2. Your browser opens to the service's consent page
3. After approval, the IDE exchanges the code for an access token
4. Tokens are automatically refreshed before expiry (30-second early window)
5. State is persisted to `.velocity/oauth2_state.json`

### Sync Engine

The sync engine provides **bi-directional synchronization** between your workspace and external services:

- **Directions:** Pull Only, Push Only, or Bi-Directional
- **Poll intervals:** Configurable per rule (e.g., every 60 seconds, every hour)
- **Field mappings:** Map local fields to remote fields
- **Filters:** Only sync specific resource types or matching patterns
- **Conflict resolution:** When both sides change, choose Keep Local, Keep Remote, Keep Both, or Discard Both

### Webhooks

**Outgoing webhooks** fire HTTP POST requests when events occur:
- Workflow Completed / Failed
- Build Completed / Failed
- File Changed
- Task Started / Completed
- Critical Alert
- Custom events

**Incoming webhooks** receive HTTP POST from external services with HMAC-SHA256 signature verification for authenticity.

### Integration Templates

Six built-in templates provide one-step setup:
- **GitHub** — Issues + PRs sync, CI/CD webhooks
- **GitLab** — Issues + MRs sync, build webhooks
- **Jira** — Issues + sprints sync
- **Slack** — Notifications for critical alerts, build status, task updates
- **Discord** — Build notifications, agent alerts
- **Notion** — Pages + databases sync

---

## Site Map & NDA Format

### Site Map

The Site Map is the IDE's **semantic memory** — a persistent, content-addressed knowledge store that powers Go to Symbol, the code graph, and the compiler cache.

**Location:** `.velocity/site_map/`

**What it stores:**
- **Semantic triples** — Subject-predicate-object relationships between code symbols (e.g., "function A calls function B")
- **NDA program nodes** — Compiled AST nodes with 38 opcodes covering the full NDA vocabulary
- **Key-value records** — Cached token embeddings for fast retrieval
- **String dictionary** — Registered symbol names mapped to content hashes

**How it's generated:** During NDA compilation, every emitted node is hashed through a Merkle verifier. When a scope closes, child hashes fold into a parent hash. The root hash carries the top-level integrity check — if it mismatches, the compilation is rejected as structurally invalid.

**Directory structure:**
```
.velocity/site_map/
├── index.json       # Metadata index (hash → entry for all entries)
├── kv/              # Token key-value pair records
├── nodes/           # NDA program nodes (AST elements)
├── programs/        # Complete NDA programs
├── metadata.nda     # NDA-encrypted metadata (includes weight root hash)
└── metadata.json    # Plaintext metadata (includes weight root hash)
```

**How the IDE uses it:**
- **Go to Symbol** (`Ctrl+Shift+O`) — Fuzzy-searches the site map's symbol index
- **Code graph** — Derives caller/callee relationships from triples
- **Compiler cache** — Checks site map for cache hits before recompiling
- **Conflict resolution** — Semantic coupling detection uses triples to find related files

**Merkle integrity:** Every NDA program is verified during generation. Corrupted objects cannot be stored because the hash would mismatch, ensuring the site map is always internally consistent.

### NDA Format

**NDA (Non-linear Decomposed Attention)** is Velocity IDE's native binary format, serving two purposes:

#### NDA Weight Matrices (Model Weights)

A binary format for extremely compact neural network weight storage where **inference is pure add/subtract — no multiplications**.

| Version | Bits/Weight | Encoding | Description |
|---------|-------------|----------|-------------|
| v1 | 2 | Ternary {-1, 0, +1} | Legacy: active+pos bitmaps |
| v2 | 2 | Quad {-2, -1, +1, +2} | Current: sign+extra bitmaps |
| v3 | 4 | FP4 E2M1 | Blockwise logarithmic, double-quantized |
| v4 | 2 | FP2 E1M0 | Blockwise logarithmic, most compact |

**v2 decode rule (XNOR, no multiplication):**
- `sign=0, extra=0` → -2 (subtract twice)
- `sign=0, extra=1` → -1 (subtract once)
- `sign=1, extra=0` → +1 (add once)
- `sign=1, extra=1` → +2 (add twice)

Model weights are stored in `models/` as `.nda` files (e.g., `models/qwen-coder-0.5b/`).

#### NDA Documents (Portable Documents)

A separate use of `.nda` for **portable, self-describing documents** with semantic provenance:

**Contents:**
- Semantic triples (subject-predicate-object)
- Display commands (DrawText, DrawImage)
- Revision history with author identity

**Two modes:**
- **Portable** — Plain NDA1 layout, openable in any browser via the PWA viewer
- **Sealed** — Encrypted with AES-256-GCM using the workspace key; not browser-viewable

**Working with NDA documents:**
1. **New NDA Document** — Workspace menu → creates a blank NDA editor tab
2. **Import Active File to NDA** — Workspace menu → converts the current file (text becomes DrawText commands, images become DrawImage commands)
3. **Open NDA Browser Viewer** — Workspace menu → writes a standalone PWA HTML viewer to `.velocity/nda_viewer.html` and opens it in your browser

**NDA document editor views:**
- **Canvas** — Visual rendering of the document
- **Triples** — Semantic data (subject-predicate-object)
- **History** — Revision chain with author info
- **Bytes** — Raw hex view

**Files using .nda extension:**
- Model weights: `models/qwen-coder-0.5b/*.nda`
- Expert teams: `.velocity/expert_teams.nda`
- Skills: `.velocity/skills/<id>.nda`
- Automation runs: `.velocity/wa-runs/*.wa-run.nda`
- Account usage: `memory/.account_usage.nda`
- Build diagnostics: `.velocity/build_diagnostics.nda`
- Fact stores: `runs/desktop/facts.nda`

---

## Collaboration & Drones

### Collaboration Manager

The Collaboration Manager supports **multi-user, multi-agent teamwork** with role-based access control:

**Opening:** **Workspace → Collaboration**

**User roles:**
| Role | Permissions |
|------|-------------|
| Owner | Full access including team management |
| Admin | Manage workflows and run agents |
| Editor | Run agents and view sessions |
| Viewer | View only |

**Features:**
- **Presence tracking** — See who's online via heartbeats (5-minute timeout)
- **Shared sessions** — Create, join, pause, and complete collaborative sessions
- **Session messaging** — Chat-style communication between users and agents within a session
- **Session lifecycle:** Draft → Active → Paused → Completed (or Abandoned)
- **Message history** — Last 500 messages per session with FIFO eviction

**Cross-device peers:** Connect other devices running Velocity IDE via peer links. Each peer advertises capabilities (GUI automation, file execution, etc.) and can be delegated tasks remotely.

**Persistence:** Collaboration state is saved to `.velocity/collaboration.json`.

### Peer Collaboration Panel

The Peer panel manages **direct device-to-device connections** for file transfer, task delegation, and chat:

**Opening:** **Workspace → Peers**

**Peer Server:**
- **Start/Stop** — Control the local peer server
- **Port** — Configurable port (default 9191)
- **Local Peer ID** — Your unique identifier shown when listening

**Connecting to a peer:**
1. Enter the peer's host, port, and name in the **Add Peer** section
2. Click **Connect** — a pairing handshake is performed
3. The peer appears in **Connected Peers** with online/offline status

**Peer capabilities:** Each peer advertises what it can do:
| Capability | Description |
|------------|-------------|
| File Execution | Run files and scripts |
| Test Runner | Execute test suites |
| Screen Capture | Capture screenshots via GDI |
| GUI Automation | Simulate keyboard/mouse input |
| Build System | Compile and build projects |
| Network Monitor | Track connections and traffic |
| General | General-purpose tasks |

**Per-peer actions:**
- **Chat** — Send text messages directly to the peer (messages show direction arrows ←/→ with timestamps)
- **Health Check** — Query the peer's status and capabilities
- **Remove** — Disconnect and remove the peer

**Active transfers:** Real-time view of file uploads/downloads with:
- Direction (↑ Outgoing / ↓ Incoming)
- Filename and progress percentage
- Completion checkmark when finished

**Delegated tasks:** Send prompts to peers for remote execution:
- Task status: Pending → Running → Completed/Failed/Cancelled
- Progress percentage and error messages
- Attach files to provide context

**Message types:** PairRequest, PairAccepted, PairRejected, Heartbeat, Chat, TaskRequest, TaskProgress, TaskComplete, TaskFailed, FileTransferStart, FileTransferChunk, FileTransferComplete, StatusRequest, StatusResponse

### Drone Subsystem

Drones are **lightweight, portable agent endpoints** deployable on any machine — they don't require the full IDE.

**Use cases:**
- Remote execution on different hardware (e.g., GPU machine, ARM device)
- E2E testing across multiple machines
- CI/CD integration
- Edge computing
- Screen capture and GUI automation on remote machines

**How drones work:**
1. Deploy `velocity-drone` on the target machine — either manually or via the `drone_deploy` MCP tool
2. The drone starts an HTTP server on port 9191
3. Pair the drone with your IDE via `drone_pair` or `POST /peer/pair`
4. Send files and tasks to the drone via MCP tools or the peer protocol

**Quick start (MCP tools):**
```
# Deploy drone to a remote machine
drone_deploy(host="192.168.1.100", ssh_user="admin")

# Check drone health
drone_status(drone_url="http://192.168.1.100:9191")

# Run a command remotely
drone_command(drone_url="http://192.168.1.100:9191", command="uname -a")

# Capture remote screenshot
drone_screenshot(drone_url="http://192.168.1.100:9191")
```

**Drone API endpoints:**
| Endpoint | Method | Description |
|----------|--------|-------------|
| `/peer/health` | GET | Status, capabilities, uptime |
| `/peer/identity` | GET | Full drone identity (ID, name, environment, capabilities) |
| `/peer/pair` | POST | Pair the drone with an IDE instance |
| `/peer/message` | POST | Send a message to the drone |
| `/peer/file/start` | POST | Begin a chunked file upload |
| `/peer/file/chunk` | POST | Upload a file chunk (base64) |
| `/peer/file/complete` | POST | Finalize upload with SHA-256 verification |
| `/peer/task` | POST | Delegate a task (shell command) |
| `/peer/task/{id}/status` | GET | Poll task progress |

**Capabilities advertised:** `file_execution`, `test_runner`, `build_system`, `screen_capture`, `gui_automation`, `network_monitor`, `general`

**File transfer:** Files are uploaded in chunks with SHA-256 verification. The drone validates the checksum and stores files in `.velocity/drops/` with path traversal protection.

**Task execution:** Tasks run asynchronously (background thread, max 8 concurrent). Each task records exit code, stdout, and stderr.

---

## Security Model

Velocity IDE implements defense-in-depth security across all layers.

### Path Traversal Protection

All file operations go through `resolve_workspace_path()` which:
1. **Canonicalizes** the path (resolves symlinks, `..`, `.`)
2. **Verifies** the canonical path starts with the workspace root
3. **Rejects** any path that escapes the workspace with "Access Denied: Path escapes workspace root"

This applies to reads, writes, and deletes — preventing both accidental and malicious path traversal attacks.

### NDA Encryption at Rest

Sensitive data is encrypted using the workspace's master key:

- **Master key:** One 32-byte key per workspace, stored in `.velocity/nda.key`
- **Key sealing:** The master key is sealed via **Windows DPAPI** (`CryptProtectData`) — tied to the current OS user account and machine
- **Subkey derivation:** Per-artifact subkeys are derived via **HKDF** for domain separation (secrets, site map, transcripts never share a key)
- **Encryption:** **AES-256-GCM** with hardware AES-NI acceleration
- **Envelope format:** `NDA1` — includes nonce, ciphertext, and authentication tag

**What's encrypted:**
- Agent memory and transcripts
- Account usage data
- Expert team configurations
- Build diagnostics
- Site map entries
- All `.nda` sealed documents

### Secret Store

API keys, tokens, and passwords are managed through an encrypted secret store:

- **Storage:** `.velocity/secrets.nda` — sealed under the `secrets` artifact class
- **Never in plaintext:** Secrets are only stored encrypted; if key material is unavailable, the save fails loudly
- **Handles, not values:** The connector registry stores only handles (names) into the secret store, never the actual secrets
- **Masked display:** The UI shows first 4 characters + bullets (e.g., `sk-a••••`)
- **Redaction:** All known secret values are scrubbed from text before logging or UI display

### Hardcoded Secret Detection

When agents write files, `scan_file_content()` automatically checks for patterns like `api_key = "sk-` or `secret = "` and warns about potential hardcoded secrets.

### Workspace Key Hierarchy

```
.velocity/
├── nda.key              # Master key (DPAPI-sealed)
├── secrets.nda          # Encrypted secret store
├── oauth2_state.json    # OAuth2 tokens (encrypted)
└── connectors.json      # Connector configs (secrets as handles only)
```

> **Note:** Provider credentials (API keys) are stored at the **user level**
> (`%APPDATA%/Velocity/provider-settings.json` on Windows), not per-workspace.
> This prevents API keys from ending up in cloud-synced directories.

---

## Usage Tracking

Velocity IDE tracks daily API usage per account to help you stay within limits.

**Opening:** **Workspace → Usage**

### What's Tracked

| Metric | Description |
|--------|-------------|
| Requests | Number of API calls made today |
| Tokens in | Input tokens consumed |
| Tokens out | Output tokens generated |
| Daily limit | Maximum requests per day |
| Remaining | Requests remaining today |
| Exhausted | Whether the account has hit its limit |

### Daily Limits

| Tier | Default Limit | Override |
|------|---------------|----------|
| Free | 50 requests/day | `CF_ACCOUNT_{n}_DAILY_LIMIT` env var |
| Paid | 500 requests/day | `CF_ACCOUNT_{n}_DAILY_LIMIT` env var |

Limits reset at midnight UTC. When an account is exhausted, the IDE automatically switches to the next available account (if you have multiple configured).

### Multi-Account Rotation

Velocity supports up to 30 Cloudflare Workers AI accounts and multiple OpenRouter accounts:

```
CF_ACCOUNT_1_ID=abc...  CF_ACCOUNT_1_TOKEN=xyz...  CF_ACCOUNT_1_TIER=free
CF_ACCOUNT_2_ID=def...  CF_ACCOUNT_2_TOKEN=uvw...  CF_ACCOUNT_2_TIER=paid
OPENROUTER_ACCOUNT_1_KEY=sk-or-...
OPENROUTER_ACCOUNT_2_KEY=sk-or-...
```

The IDE randomly selects from non-exhausted accounts for each request, providing automatic load balancing across your API keys.

### Storage

Usage data is dual-written to:
- `memory/.account_usage.nda` — NDA-encrypted (authoritative)
- `memory/.account_usage.json` — Plaintext backup

Data is keyed by UTC date and auto-resets when the date changes.

---

## Workspace Preferences

Velocity IDE persists your workspace configuration to `.velocity/workspace-preferences.json`.

### What's Stored

| Setting | Description |
|---------|-------------|
| `appearance` | Theme, workspace mode/profile |
| `provider` | Active AI provider label |
| `selected_model` | Active model ID |
| `auto_approve` | Auto-approve tool actions |
| `show_thoughts` | Show model reasoning |
| `thinking_enabled` | Extended thinking mode |
| `left_sidebar_visible` | Left sidebar visibility |
| `left_sidebar_width` | Left sidebar width |
| `right_sidebar_visible` | Right sidebar visibility |
| `right_sidebar_width` | Right sidebar width |
| `mode_layouts` | Per-mode panel arrangements |
| `open_tabs` | File paths from last session |
| `active_tab` | Active editor tab from last session |

### Session Restoration

When you reopen the IDE, it automatically:
1. Restores your theme, mode, and sidebar layout
2. Reopens all editor tabs from your last session
3. Restores the active provider and model
4. Reapplies per-mode panel arrangements

### Per-Mode Layouts

Each workspace mode (Coder, Automation Operator, Mission Control, Accessibility) remembers its own sidebar visibility and widths. Switching modes restores your customized layout for that mode.

### Manual Editing

You can edit `.velocity/workspace-preferences.json` directly. Changes take effect after restarting the IDE or reloading provider settings from the Settings panel.

---

## Settings & Configuration

### Appearance Settings

Open Settings with `Ctrl+,` and navigate to **Appearance**:

- **Profile** — Coder, Automation Operator, Mission Control, Accessibility
- **Theme** — Midnight, Daylight, Operator, Mission, High Contrast
- **Density** — Compact, Comfortable, Spacious
- **UI Scale** — Float multiplier for interface fonts (default 1.15)
- **Code Scale** — Float multiplier for code fonts (default 1.12)

### Editor Settings

In Settings → **Editor** section:

- **Show breadcrumbs** — Toggle navigation breadcrumbs above the editor (default: on)
- **Word wrap** — Toggle word wrap in the editor (also `Alt+Z`)

### Keybindings

Keybindings are configurable via `.velocity/keybindings.json`:

```json
{
  "bindings": [
    {
      "command": "view.command_palette",
      "binding": {"key": "P", "ctrl": true, "shift": true, "alt": false},
      "when": null
    },
    {
      "command": "view.toggle_sidebar",
      "binding": {"key": "E", "ctrl": true, "shift": false, "alt": false},
      "when": "editorFocus"
    }
  ]
}
```

**Conflict detection** — The IDE warns you if two commands share the same shortcut.

**Context clauses** — Use `"when"` to limit bindings to specific contexts:
- `editorFocus` — Only when editor has focus
- `debugActive` — Only when debugger is running

### Provider Configuration

Velocity IDE manages AI providers through two mechanisms: the **in-app Settings UI** (recommended) and a **workspace provider settings file** for persistent credential storage.

**Opening provider settings:**
1. Open Settings (`Ctrl+,`)
2. Scroll to the **Providers & credentials** section
3. Each provider appears as a collapsible header with a status badge (green = configured, grey = unconfigured)

#### Registering a Provider

Each provider has specific fields you need to fill in. Expand the provider's section and enter your credentials:

**Cloudflare Workers AI** (default, no API key required for free tier):
| Field | Description | Example |
|-------|-------------|---------|
| Account ID | Your Cloudflare account ID | `abc123...` |
| API token | Cloudflare API token with Workers AI access | `xyz789...` |
| Tier | `free` or `paid` | `free` |
| Label | Display name in the UI | `default` |

**OpenRouter** (access to 100+ models through one key):
| Field | Description | Example |
|-------|-------------|---------|
| API key | OpenRouter API key | `sk-or-...` |
| Tier | `free` or `paid` | `free` |
| Label | Display name | `OR-Default` |

**Azure OpenAI**:
| Field | Description | Example |
|-------|-------------|---------|
| Endpoint | Your Azure resource endpoint | `https://my-resource.openai.azure.com` |
| API key | Azure OpenAI key | `abc123...` |
| Deployment | Deployment name | `gpt-4o` |
| API version | API version string | `2024-06-01` |
| Tier | `free` or `paid` | `paid` |

**Local Ollama** (runs models on your machine):
| Field | Description | Example |
|-------|-------------|---------|
| Host | Ollama server URL | `http://localhost:11434` |
| Default model | Model to use by default | `llama3.2` |
| Label | Display name | `Local-Ollama` |

**API-key providers** (OpenAI, Anthropic, Google Vertex, Deepseek, Groq, Mistral, Alibaba Qwen, Together AI, Fireworks AI, Perplexity, Cerebras, AWS Bedrock):

Each of these providers has a single **API key** field. Expand the provider section and paste your key.

#### Saving & Reloading Credentials

After entering credentials:
- Click **Save provider settings** (green button) to write credentials to the workspace settings file
- Click **Reload** to re-read credentials from disk (useful if you edited the file externally)
- Credentials are stored in `.velocity/workspace-preferences.json` within your workspace root (NDA-encrypted)

> **Security note:** API keys are stored locally in your workspace. Never commit the `.velocity/workspace-preferences.json` file to version control — it is already excluded by the default `.gitignore`.

#### Refreshing the Model Catalog

After configuring a provider, you need to refresh the model list so the IDE can discover available models:

1. Open Settings (`Ctrl+,`)
2. In the **Agent defaults** section, select your provider from the dropdown
3. Click the **↻ Models** button
4. The IDE queries the provider's API and populates the model dropdown
5. Select a model from the dropdown

The model catalog is cached for 10 minutes to reduce API calls. If you don't see newly available models, wait a few minutes and click **↻ Models** again.

> **Tip:** When you switch providers, the IDE automatically refreshes the model list for the new provider.

#### Selecting a Model

Once models are loaded:
1. In Settings → **Agent defaults**, choose a provider and model from the dropdowns
2. Or use the model selector in the Chat panel header
3. The selected model is shown in the status bar provider pill (bottom-right)
4. Your selection persists across sessions

#### Workspace Provider Settings File

The `.velocity/workspace-preferences.json` file stores all provider credentials in JSON format:

```json
{
  "cloudflare": {
    "account_id": "your-account-id",
    "api_token": "your-api-token",
    "tier": "free",
    "label": "default"
  },
  "openrouter": {
    "api_key": "sk-or-...",
    "tier": "free",
    "label": "OR-Default"
  },
  "azure_openai": {
    "endpoint": "https://your-resource.openai.azure.com",
    "api_key": "...",
    "deployment": "gpt-4o",
    "api_version": "2024-06-01",
    "tier": "paid",
    "label": "Azure-Default"
  },
  "ollama": {
    "host": "http://localhost:11434",
    "default_model": "llama3.2",
    "label": "Local-Ollama"
  },
  "openai":    { "api_key": "sk-...", "tier": "paid", "label": "OpenAI" },
  "anthropic": { "api_key": "sk-ant-...", "tier": "paid", "label": "Anthropic" },
  "google":    { "api_key": "...", "tier": "paid", "label": "Google" },
  "deepseek":  { "api_key": "...", "tier": "paid", "label": "Deepseek" },
  "groq":      { "api_key": "gsk_...", "tier": "paid", "label": "Groq" },
  "mistral":   { "api_key": "...", "tier": "paid", "label": "Mistral" },
  "alibaba":   { "api_key": "...", "tier": "paid", "label": "Alibaba" },
  "together":  { "api_key": "...", "tier": "paid", "label": "Together" },
  "fireworks": { "api_key": "...", "tier": "paid", "label": "Fireworks" },
  "perplexity":{ "api_key": "...", "tier": "paid", "label": "Perplexity" },
  "cerebras":  { "api_key": "...", "tier": "paid", "label": "Cerebras" },
  "bedrock":   { "api_key": "...", "tier": "paid", "label": "Bedrock" }
}
```

You can edit this file directly and click **Reload** in Settings to apply changes.

#### Environment Variables

As an alternative to the settings file, you can configure providers via environment variables:

| Variable | Description |
|----------|-------------|
| `VELOCITY_API_KEY` | Default API key (used when no provider-specific key is set) |
| `CF_ACCOUNT_N_ID` | Cloudflare account ID (where N is the account number: 1, 2, ...) |
| `CF_ACCOUNT_N_TOKEN` | Cloudflare API token |
| `CF_ACCOUNT_N_DAILY_LIMIT` | Override daily request limit for a Cloudflare account |
| `OPENROUTER_API_KEY` | OpenRouter API key |
| `BEDROCK_PROXY_URL` | URL for the AWS Bedrock proxy endpoint |
| `RUST_LOG` | Log level (`trace`, `debug`, `info`, `warn`, `error`) |

Environment variables take precedence over the workspace settings file. This is useful for CI/CD pipelines or shared workstations where you don't want to write credentials to disk.

#### Automatic Failover

Velocity IDE supports automatic failover across providers. If the active provider returns an error (rate limit, auth failure, timeout), the IDE automatically tries the next configured provider. To maximize failover reliability:
- Configure at least 2 providers
- Keep Cloudflare Workers AI as a fallback (it has a generous free tier)
- Ensure API keys are valid and not expired

---

## Keyboard Shortcuts Reference

### File Operations

| Command | Shortcut |
|---------|----------|
| New file | `Ctrl+N` |
| Open file | `Ctrl+O` |
| Save | `Ctrl+S` |
| Save all | `Ctrl+Shift+S` |
| Close file | `Ctrl+W` |
| Quick open | `Ctrl+P` |
| Split editor | `Ctrl+\` |
| Switch workspace | `Ctrl+Shift+W` |

### Edit Operations

| Command | Shortcut |
|---------|----------|
| Undo | `Ctrl+Z` |
| Redo | `Ctrl+Shift+Z` |
| Find | `Ctrl+F` |
| Replace | `Ctrl+H` |
| Find next | `F3` |
| Find previous | `Shift+F3` |
| Indent | `Tab` |
| Dedent | `Shift+Tab` |
| Toggle comment | `Ctrl+/` |
| Duplicate line | `Ctrl+Shift+D` |
| Delete line | `Ctrl+Shift+K` |
| Move line up | `Alt+Up` |
| Move line down | `Alt+Down` |

### Navigation

| Command | Shortcut |
|---------|----------|
| Go to line | `Ctrl+G` |
| Go to symbol | `Ctrl+Shift+O` |
| Go to definition | `F12` |
| Find references | `Shift+F12` |
| Back | `Alt+Left` |
| Forward | `Alt+Right` |
| Next tab | `Ctrl+PageDown` |
| Previous tab | `Ctrl+PageUp` |
| MRU tab switcher | `Ctrl+Tab` |
| MRU switcher (reverse) | `Ctrl+Shift+Tab` |

**Ctrl+Tab MRU Switcher:** Hold `Ctrl` and tap `Tab` to open a most-recently-used tab switcher overlay. Keep holding `Ctrl` and tap `Tab` to cycle forward, `Ctrl+Shift+Tab` to cycle backward. Release `Ctrl` to switch to the selected tab. The overlay shows a scrollable list of all open tabs with the selected one highlighted. Also works by clicking a tab in the overlay.

### View

| Command | Shortcut |
|---------|----------|
| Command palette | `Ctrl+Shift+P` |
| Toggle sidebar | `Ctrl+E` |
| Toggle right sidebar | `Ctrl+Shift+E` |
| Toggle terminal | `` Ctrl+` `` |
| Toggle chat | `Ctrl+J` |
| Toggle orchestrator | `Ctrl+Shift+Y` |
| Toggle search | `Ctrl+Shift+F` |
| Toggle settings | `Ctrl+,` |
| Toggle extensions | `Ctrl+Shift+X` |
| Toggle activity | `Ctrl+Shift+A` |
| Toggle voice | `Ctrl+Shift+V` |
| Toggle minimap | `Ctrl+Shift+M` |
| Fold | `Ctrl+Shift+[` |
| Unfold | `Ctrl+Shift+]` |
| Fold all | `Ctrl+K Ctrl+0` |
| Unfold all | `Ctrl+K Ctrl+J` |
| Word wrap | `Alt+Z` |

### Debug

| Command | Shortcut |
|---------|----------|
| Start debug | `F5` |
| Stop debug | `Shift+F5` |
| Step over | `F10` |
| Step into | `F11` |
| Step out | `Shift+F11` |
| Toggle breakpoint | `F9` |
| Continue | `F5` |

### Build & Mode

| Command | Shortcut |
|---------|----------|
| Build | `Ctrl+B` |
| Run | `Ctrl+R` |
| Rollback deploy | `Ctrl+Alt+R` |
| Coder mode | `Ctrl+1` |
| Operator mode | `Ctrl+2` |
| Mission mode | `Ctrl+3` |
| Accessibility mode | `Ctrl+4` |
| Trigger completion | `Ctrl+Space` |
| Inline suggestion | `Ctrl+Shift+I` |

### Accessibility Mode

| Key | Action |
|-----|--------|
| `Tab` | Focus next element |
| `Shift+Tab` | Focus previous element |
| `Enter` | Activate focused element |
| `Space` | Toggle checkbox/button |
| `Escape` | Dismiss dialog / clear focus |
| `F6` | Next panel region |
| `Shift+F6` | Previous panel region |
| `F10` | Context menu |
| `Ctrl+/` | Focus search |
| `Ctrl+G` | Go to line |

---

## Troubleshooting

### Build Failures

**Missing GTK libraries (Linux):**
```bash
sudo apt-get install libgtk-3-dev libwebkit2gtk-4.1-dev
```

**Rust toolchain outdated:**
```bash
rustup update stable
```

**Disk space error during build:**
```bash
cargo clean
```

### Runtime Issues

**GPU acceleration not working:**
- Verify Vulkan runtime is installed
- Run `vulkaninfo` to check
- The IDE automatically falls back to CPU rendering

**API key not recognized:**
- Verify `VELOCITY_API_KEY` is set in environment or config
- Check config file syntax
- Restart the IDE after config changes

**High memory usage:**
- Idle memory: ~245 MB
- Wiki indexing can peak higher
- Adjust `RUST_LOG` to reduce log volume
- Monitor with Task Manager / htop

**Chat not responding:**
- Check provider status in the status bar
- Verify API key is valid
- Try switching to a different provider (automatic failover should handle this)

**File tree not showing files:**
- Click **Refresh** in the file tree
- Check that the workspace folder is correct
- Verify file permissions

### Performance Tips

- Use **release builds** for production (`cargo build --release`)
- Use **debug builds** for development (faster compilation)
- Enable **GPU acceleration** for smoother rendering
- Use **Compact density** for maximum information density
- Use **Spacious density** for reduced eye strain during long sessions

### Common Issues

**Model list is empty after switching providers:**
- Click **↻ Models** in Settings → Agent defaults to refresh
- The model catalog is cached for 10 minutes — if you just configured a provider, wait a moment and retry
- Verify your API key is correct by checking the provider status badge (green = configured)

**Agent approval dialog doesn't appear:**
- Check that **Auto-approve tools** is not enabled in Settings → Agent defaults
- If auto-approve is on, the agent executes tools without asking — disable it for more control

**Wiki shows zero pages:**
- Click **Rebuild Index** in the Wiki toolbar to compile source files into the name dictionary
- The wiki generates pages from semantic triples stored in the site map — if no triples exist, the wiki will be empty
- After adding new source files, click **Rebuild Index** again

**Knowledge base search returns no results:**
- Ensure content has been ingested via the Knowledge Base panel
- The knowledge base uses TF-IDF ranking — very common words may not match well
- Check that `.velocity/knowledge/store.json` exists and is not empty

**Provider settings not saving:**
- Provider credentials are stored at `%APPDATA%/Velocity/provider-settings.json` (Windows)
- Ensure the `%APPDATA%/Velocity/` directory exists and is writable
- Check that `provider-settings.json` is not read-only
- On Windows, verify that Windows Defender or antivirus is not blocking writes

**NDA document appears blank in browser viewer:**
- Ensure the document is in **Portable** mode, not **Sealed** (sealed documents require the workspace key)
- Check that `.velocity/nda_viewer.html` was written correctly
- Try opening the viewer directly: navigate to the file in your browser

**Git panel shows no changes:**
- Click **Refresh** in the Git panel
- Verify you're in a git repository (check for `.git/` directory)
- Ensure files are actually modified (check with `git status` in terminal)

**Voice commands not working (Windows):**
- Ensure microphone permissions are granted in Windows Settings → Privacy → Microphone
- Verify the Windows Speech Recognition service is running
- Voice commands are only available on Windows

**Orchestrator tasks stuck in "Blocked":**
- Check for file collisions — multiple tasks may be trying to edit the same file
- Click **Retry Blocked** to attempt re-execution
- Use **Reset** to clear all task statuses and start over

**Extensions not loading:**
- Verify the extension manifest (`manifest.json`) is valid JSON
- Check that `entry_point` points to a valid `.wasm` or `.lua` file
- Ensure the extension is in `.velocity/extensions/`
- Check the extension state in **Workspace → Extensions** — it should show "Active"

### Data & Storage

**Where does Velocity IDE store data?**

| Location | Contents |
|----------|----------|
| `.velocity/site_map/` | Semantic code index (Merkle-verified) |
| `.velocity/workspace-preferences.json` | UI settings, open tabs, provider/model selection |
| `%APPDATA%/Velocity/provider-settings.json` | Provider API keys and credentials (user-level, not per-workspace) |
| `.velocity/nda.key` | Workspace encryption master key (DPAPI-sealed) |
| `.velocity/secrets.nda` | Encrypted secret store |
| `.velocity/connectors.json` | External service connector configs |
| `.velocity/knowledge/store.json` | Knowledge base chunks |
| `.velocity/extensions/` | Installed extensions |
| `.velocity/skills/` | Agent skill definitions |
| `.velocity/expert_teams.nda` | Expert team configurations |
| `.velocity/background_agents.json` | Background agent monitor configs |
| `.velocity/collaboration.json` | Collaboration manager state |
| `.velocity/shared_memory.json` | Shared knowledge entries |
| `memory/.account_usage.nda` | API usage tracking (encrypted) |

**Can I delete `.velocity/` to start fresh?**
Yes, but you'll lose: wiki data, knowledge base, agent memory, provider settings, and all workspace preferences. The site map will be rebuilt from source files on next open. Provider settings and API keys will need to be re-entered.

**Is it safe to commit `.velocity/` to git?**
No. The default `.gitignore` excludes `.velocity/` because it contains API keys, encryption keys, and machine-specific data. Only `.velocity/workspace-preferences.json` (if you strip provider credentials) and wiki exports (`.wiki/`) are safe to commit.

---

## Frequently Asked Questions

**Q: Do I need an API key to use Velocity IDE?**
A: No. Cloudflare Workers AI has a free tier that works without an API key (using the built-in default account). However, for production use, configuring your own credentials gives you higher limits and access to more models.

**Q: Can I use Velocity IDE offline?**
A: Yes, for editing, building, and browsing your codebase. AI chat, inline suggestions, and browser automation require network access. Local Ollama provides fully offline AI capabilities if you have models downloaded.

**Q: How do I update Velocity IDE?**
A: Download the latest release from GitHub Releases and replace the binary. If you built from source, run `git pull` then `cargo build --release`.

**Q: Can I use multiple AI providers at once?**
A: Yes. Configure multiple providers in Settings → Providers & credentials. The IDE uses one active provider at a time but automatically fails over to the next configured provider if the active one returns an error.

**Q: What happens to my chat history?**
A: Chat history (up to 200 messages) is persisted to NDA-encrypted storage and restored when you reopen the IDE. Use the **Clear** button in the chat panel to reset the conversation.

**Q: Can I customize the keyboard shortcuts?**
A: Yes. Edit `.velocity/keybindings.json` — see the [Keybindings](#keybindings) section for the format. The IDE warns you if two commands share the same shortcut.

**Q: Does Velocity IDE support remote development?**
A: Yes, via the Drone subsystem. Deploy `velocity-drone` on a remote machine, pair it with your IDE, and send files/tasks for remote execution.

**Q: How do I report a bug?**
A: Open an issue on [GitHub Issues](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-IDE/issues) with steps to reproduce, expected behavior, and actual behavior. Include your OS version and Velocity IDE version (shown in the title bar).

**Q: Can I contribute to Velocity IDE?**
A: Yes! See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines. We welcome bug reports, feature requests, and pull requests.

**Q: What languages does the editor support?**
A: Syntax highlighting for 28+ languages including Rust, Python, JavaScript, TypeScript, Go, Java, C/C++, C#, Ruby, HTML, CSS, JSON, YAML, TOML, Markdown, SQL, Shell, and more. The AI agent can work with any language.

---

## Support

- **Documentation:** [README.md](README.md)
- **Deployment Guide:** [docs/DEPLOYMENT.md](docs/DEPLOYMENT.md)
- **Operational Runbook:** [docs/RUNBOOK.md](docs/RUNBOOK.md)
- **GitHub Issues:** [Report bugs](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-IDE/issues)
- **GitHub Discussions:** [Ask questions](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-IDE/discussions)
- **Email:** support@unitbuilds.com

---

*V.E.L.O.C.I.T.Y. IDE v2.4.0 — Built with Rust, powered by AI. Comprehensive user guide covering all IDE features, agent orchestration, automation, security, and integration.*
