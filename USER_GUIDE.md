# V.E.L.O.C.I.T.Y. IDE — User Guide

A complete guide to using the V.E.L.O.C.I.T.Y. Cognitive IDE — a native, GPU-accelerated developer workspace with autonomous agentic capabilities built in pure Rust.

---

## Table of Contents

- [Getting Started](#getting-started)
  - [System Requirements](#system-requirements)
  - [Installation](#installation)
  - [First Launch](#first-launch)
  - [Opening a Workspace](#opening-a-workspace)
- [Interface Overview](#interface-overview)
  - [The Activity Bar](#the-activity-bar)
  - [Workspace Modes](#workspace-modes)
  - [Themes & Appearance](#themes--appearance)
  - [Status Bar](#status-bar)
- [Command Palette](#command-palette)
- [Menu Bar Reference](#menu-bar-reference)
- [Working with Files](#working-with-files)
  - [File Tree](#file-tree)
  - [Bookmarks & Favorites](#bookmarks--favorites)
  - [Search & Replace](#search--replace)
  - [Semantic Search](#semantic-search)
  - [Code Graph](#code-graph)
- [Code Editing](#code-editing)
  - [Editor Features](#editor-features)
  - [Find & Replace](#find--replace)
  - [Code Folding](#code-folding)
  - [Inline Suggestions](#inline-suggestions)
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

1. Download the latest release from [GitHub Releases](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-IDE/releases)
2. Extract `velocity-v2.4.0-win-x64.zip` to a folder of your choice
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

---

## Interface Overview

### The Activity Bar

The activity bar is the vertical icon strip on the far left. It contains 8 categories:

| Icon | Label | Shortcut | Description |
|------|-------|----------|-------------|
| 📁 | **Files** | `Ctrl+E` | File tree, bookmarks, favorites |
|  | **Search** | `Ctrl+Shift+F` | Text search, semantic search, code graph |
| 🔀 | **Git** | `Ctrl+G` | Changes, branches, commits |
| 💬 | **Chat** | `Ctrl+J` | AI chat, voice, multimodal |
| 🔨 | **Build** | `Ctrl+B` | Build, test, deploy, debug, LSP |
| 🤖 | **Agents** | `Ctrl+D` | Activity, roster, orchestration, memory |
| 📖 | **Knowledge** | `Ctrl+K` | Wiki, knowledge base, snippets, NDA |
| ️ | **Workspace** | `Ctrl+Shift+X` | Extensions, plugins, skills, team, usage |

Click any icon to switch categories. The left sidebar updates to show the sub-panels for that category.

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
| **File** | New File, Open File, Save, Save All, Close Tab, Reopen Closed Tab, Go to Line, Go to Symbol, Go to Definition |
| **Build** | Build, Run, Deploy Pipeline, Rollback Deploy, Test Generator, Test Coverage |
| **Edit** | Find, Find & Replace |
| **Panels** | Chat, Output, Orchestrator, Mission Control, Search, Usage, Settings, Extensions, Voice Commands |
| **Agent** | Request Inline Suggestion, Approve All Tools, Decline All Tools, Plan Sub-Agents, Refresh Models |
| **Workspace** | Switch Mode (Coder/Operator/Mission/Accessibility), Reset Layout, Wiki Export, NDA Document operations |
| **View** | Toggle Sidebar, Toggle History, Reset Layout |
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
- Toggle bookmark with `F2` (or right-click → Toggle Bookmark)
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

---

## Code Editing

### Editor Features

The code editor supports:

- **Syntax highlighting** — 30+ languages via syntect
- **Auto-indent** — Automatic indentation on new lines
- **Bracket matching** — Highlights matching brackets
- **Breadcrumbs** — Navigation path at top of editor
- **Minimap** — Overview of file structure (right side)
- **Line numbers** — Click to set breakpoints
- **Word wrap** — Toggle with `Alt+Z`

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
| Anthropic | `claude-sonnet-4-20250514` | Requires API key |
| Azure OpenAI | `gpt-4o` | Requires deployment endpoint |
| Local Ollama | `llama3.2` | Runs locally, no API key |
| Deepseek | `deepseek-chat` | Requires API key |
| Google Vertex | `gemini-2.5-pro` | Requires GCP credentials |
| Groq | `llama-3.3-70b-versatile` | Requires API key |
| Mistral | `mistral-large-latest` | Requires API key |
| Together AI | `meta-llama/Meta-Llama-3.1-70B-Instruct-Turbo` | Requires API key |
| Fireworks AI | `accounts/fireworks/models/llama-v3p1-70b-instruct` | Requires API key |
| Perplexity | `llama-3.1-sonar-large-128k-online` | Requires API key |
| Cerebras | `llama3.1-70b` | Requires API key |
| AWS Bedrock | `anthropic.claude-3-5-sonnet-20241022-v2:0` | Requires AWS credentials |
| Alibaba Qwen | `qwen-max` | Requires API key |

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

The server exposes 50+ tools across 4 categories:

**System Tools** (27 tools):
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

**Browser Tools** (~15 tools): `web_navigate`, `browser_create_session`, `browser_runtime_capture`, `browser_runtime_visual_capture`, and more for headless browser automation.

**Windows Automation Tools**: UI automation, screenshot capture, registry access, advanced input simulation.

**Team Tools**: `create_expert_team`, `list_expert_teams`, `update_expert_team`, `create_skill_file`, `list_skills`.

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

- **Status indicators:** Modified (M), Added (A), Deleted (D), Renamed (R), Untracked (U), Conflicted (C)
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
- **Supported extensions:** 30+ (md, txt, rs, py, js, ts, go, java, etc.)

**Usage:** The agent queries the knowledge base when answering questions about your codebase.

### Agent Memory

Agent memory persists learnings across sessions:

- **Per-member storage** — Each agent member has isolated memory
- **Categories:** Pattern, preference, architecture, lesson, context
- **Search:** Keyword-based with scoring
- **Encryption:** NDA-encrypted storage per member ID
- **Context injection:** Memories are automatically injected into agent prompts

**Viewing memory:** Open **Agents → Memory** sub-panel.

### Persistent Memory

Beyond per-session agent memory, Velocity IDE maintains **persistent memory** that survives across restarts:

- **NDA-encrypted at rest** — Stored in `.velocity/` with AES-256-GCM encryption
- **Per-workspace** — Each workspace has its own isolated memory store
- **Categories:** Pattern, preference, architecture, lesson, context
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
    "commands": ["myExtension.hello"],
    "keybindings": [{"key": "Ctrl+Alt+H", "command": "myExtension.hello"}],
    "themes": ["myTheme"],
    "languages": ["myLang"],
    "snippets": ["mySnippet"]
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
└── weight_root      # Persisted model weight root hash
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

### Drone Subsystem

Drones are **lightweight, portable agent endpoints** deployable on any machine — they don't require the full IDE.

**Use cases:**
- Remote execution on different hardware (e.g., GPU machine, ARM device)
- E2E testing across multiple machines
- CI/CD integration
- Edge computing

**How drones work:**
1. Deploy `velocity-drone` on the target machine
2. The drone starts an HTTP server on port 9191
3. Pair the drone with your IDE via `POST /peer/pair`
4. Send files and tasks to the drone

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
├── provider-settings.json  # Provider credentials (workspace-local)
├── oauth2_state.json    # OAuth2 tokens (encrypted)
└── connectors.json      # Connector configs (secrets as handles only)
```

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

### Keybindings

Keybindings are configurable via `.velocity/keybindings.json`:

```json
{
  "bindings": [
    {"key": "Ctrl+Shift+P", "command": "view.command_palette"},
    {"key": "Ctrl+E", "command": "view.toggle_sidebar", "when": "editorFocus"}
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

### View

| Command | Shortcut |
|---------|----------|
| Command palette | `Ctrl+Shift+P` |
| Toggle sidebar | `Ctrl+E` |
| Toggle terminal | `` Ctrl+` `` |
| Toggle chat | `Ctrl+J` |
| Toggle orchestrator | `Ctrl+Shift+Y` |
| Toggle search | `Ctrl+Shift+F` |
| Toggle settings | `Ctrl+,` |
| Toggle extensions | `Ctrl+Shift+X` |
| Toggle activity | `Ctrl+Shift+A` |
| Toggle voice | `Ctrl+Shift+V` |
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

---

## Support

- **Documentation:** [README.md](README.md)
- **Deployment Guide:** [docs/DEPLOYMENT.md](docs/DEPLOYMENT.md)
- **Operational Runbook:** [docs/RUNBOOK.md](docs/RUNBOOK.md)
- **GitHub Issues:** [Report bugs](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-IDE/issues)
- **GitHub Discussions:** [Ask questions](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-IDE/discussions)
- **Email:** support@velocity-ide.com

---

*V.E.L.O.C.I.T.Y. IDE v2.4.0 — Built with Rust, powered by AI. Comprehensive user guide covering all IDE features, agent orchestration, automation, security, and integration.*
