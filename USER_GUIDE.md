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
- [Browser Automation](#browser-automation)
  - [Browse Panel](#browse-panel)
  - [Automation Flows](#automation-flows)
  - [Targets & Recordings](#targets--recordings)
- [Git Integration](#git-integration)
  - [Changes Panel](#changes-panel)
  - [Branches & Commits](#branches--commits)
- [Wiki & Knowledge](#wiki--knowledge)
  - [Wiki System](#wiki-system)
  - [Knowledge Base](#knowledge-base)
  - [Agent Memory](#agent-memory)
- [Build & Deploy](#build--deploy)
  - [Build Panel](#build-panel)
  - [Test Generator](#test-generator)
  - [Deploy Pipeline](#deploy-pipeline)
  - [Debugger](#debugger)
- [Plugins & Extensions](#plugins--extensions)
  - [Plugin Registry](#plugin-registry)
  - [Extensions](#extensions)
  - [Skills](#skills)
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

*V.E.L.O.C.I.T.Y. IDE v2.4.0 — Built with Rust, powered by AI.*
