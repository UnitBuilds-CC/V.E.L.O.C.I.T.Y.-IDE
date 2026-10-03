# Panel Reference

Every dock tab and rail sub-panel in the IDE, one section each, grouped by area.
For how to *reach* a panel, each entry lists its menu route, rail/sub-tab, and
any palette command or chord. Settings & providers live on their own page
([Settings & providers](settings-providers.md)).

- [Interface & shell](ui-and-shell.md) · [Editor surfaces](editor.md) · [Manual index](README.md)

**Quick jump:** [Core](#core-workspaces) · [Orchestration](#orchestration--missions) ·
[Search & knowledge](#search--knowledge) · [Agents](#agents--activity) ·
[Build & debug](#build--debug) · [Automation](#automation) · [Git](#git) ·
[Workspace & peers](#workspace--peers) · [Rail-only](#rail-only-surfaces)

---

## Core workspaces

### Editor
*Files rail → Files (`Ctrl+E`); Quick Open `Ctrl+P`; File ▸ New/Open; double-click tree.*
The code surface for any open file. Full detail on the
[Editor surfaces](editor.md) page.

### Chat
*Navigate ▸ Chat (`Ctrl+J`); Chat rail sub-tab 0; palette `Chat`; `Ctrl+L` focuses input.*
The primary agent conversation.
- Header: `Chat` + `● Working`/`● Ready`; **Clear** (resets context) and **Interrupt** (while running).
- Provider `ComboBox` (16 providers, in order: CloudflareWorkersAi, OpenRouter, OpenAI, Anthropic, GoogleVertex, AzureOpenAi, LocalOllama, Deepseek, AlibabaQwen, Groq, Mistral, TogetherAi, FireworksAi, Perplexity, Cerebras, AwsBedrock) and a **dynamic** model `ComboBox` (populated from the provider). An `👁 Reasoning` toggle shows/hides thoughts.
- Messages: user bubbles (accent, right), agent bubbles (left), reasoning blocks (warning, italic, shown only with *Show thoughts*). Hover a message → **Copy**. Max 200 messages.
- Empty state offers 4 chips: *Explain this codebase*, *Find and fix bugs*, *Write tests for my code*, *Refactor a module*.
- Pending-approval strip: per-tool **Approve**/**Decline** + an **Auto-approve** checkbox.
- Input: multiline (`Enter` send, `Shift+Enter` newline) + **Attach file**.

### Output
*Navigate ▸ Output (`Ctrl+\``); palette `Output`.*
A command console distinct from the bottom-panel Output. `>`-prefixed input runs
`cmd /C` / `sh -c` in the workspace root; output is colour-coded (accent for
commands, red for errors, amber for warnings) with a **Clear** button and a
`human_bytes` size readout.

### Terminal (dock stub)
*Navigate ▸ Terminal; bottom tab 0 (`Ctrl+\``); palette `Find in Terminal`.*
The dock tab is a stub echoing the last command; the real PTY is the
[bottom-panel terminal](editor.md#terminal).

---

## Orchestration & missions

### Orchestrator
*Palette `Orchestrator` (`Ctrl+Shift+Y`, Automation Operator); a primary dock tab in that profile.*
Multi-agent task planning and execution.
- Header `Orchestrator` + `runtime_status`; **Policy** toggle (routing controls) and a **Show task graph** toggle.
- Stats: Tasks · Phases · BFS order · Done · Blocked · Workers `n/max`.
- Actions: **+ Add task** · **Route goal** (decompose a goal) · **Fix Cycle** / **Execute** / **Retry Blocked (n)** / **Reset** / **Clear plan**.
- **Max concurrent agents** `DragValue` 1–32 (default **8**).
- *Route goal:* outcome/constraints field → **Route it**.
- *Task draft:* Title, worker instruction, files-in-scope, `Depends on`, **Kind** (refactor, bug_fix, test, documentation, analysis, planning, merge, desktop_automation) → **Add to plan**.
- *Routing policy controls:* Task kind + Preferred policy (dynamic, from the workspace registry), Save/Reload; Template + Style (isolated_files / coupled_components / sequential_pipeline); shared expectations; **Persist policy edits**.
- Cycle warning when a dependency loop exists; **Cut back-edges** drops only the looping edges.
- Task cards: `○` Pending · `▹` Running · `✔` Done · `✖` Failed · `◆` Blocked; with `#{id}`, title, assigned expert, and scope.
- Task graph: left-to-right nodes per execution phase.

### Mission Control
*Mission Control profile primary dock tab; palette `Mission Control`; `Ctrl+3`.*
Three sub-tabs (labels update live): **1 Plan** · **2 Work** · **3 Activity**.
- **Plan:** describe the outcome; presets **Desktop smoke test** / **WA validation**; **Create plan**; *Launch after I review the plan* (never skips review); with a plan: **Approve & launch** · **Open work** · **Discard plan** + per-task scope cards.
- **Work:** selected task (scope, model) + *Send note to worker*; worker cards with **Inspect** / **Stop** / **Retry** / **Reset**; Desktop-Automation validation summary (artifacts / outputs / evidence); an **Intervention** queue (*Queue* a mid-flight correction; dispositions: apply to running agent / spawn follow-up / dismiss).
- **Activity:** live agent telemetry (model, current tool, last status, files, event/note counts) + a **Live Event Feed** (last 5/worker, 20 total) with icons for ToolStarted/ToolFinished/Status/FileChange/OperatorNote.

### Team Studio
*Workspace rail sub-tab 3; a dock tab in Automation Operator flows. Title `Team`.*
Expert-team management. Header `{n} team(s)` + **Reload** / **Launch Team** / **Cancel Running**.
- **Create a team** (name + purpose) and **Create an agent** (name, role, scope paths, instructions, target team) → **Create & assign agent**.
- Team gallery: cards with members, provider/model, skills, and instructions; footer `Use: @{slug} <task>`.
- **Team activity** log + a **💬 Team Builder** chat that drafts teams from a description.

---

## Search & knowledge

### Search
*Navigate ▸ Search (`Ctrl+Shift+F`); Search rail sub-tab 0; palette `Search`.*
Search & Replace across the workspace. Toggle **⊕ Semantic** ↔ **⊕ Literal**
(first semantic use builds the index). Filters: `Aa` case, `ab|` whole word, `.*`
regex; include-glob field; replace field + **Replace All** / **Replace in file**;
per-profile `Try:` chips.

### Semantic Search
*Tools ▸ Knowledge ▸ Semantic search; Search rail sub-tab 1. Title `Semantic`.*
Builds/rebuilds the TF-IDF index; once built, use the Search panel in semantic
mode. Badge `Index built`/`Not indexed`.

### Code Graph
*Tools ▸ Knowledge ▸ Code graph; Search rail sub-tab 2; palette `Code Graph`. Title `Graph`.*
Symbol explorer (`⟳ Refresh`). Two panes: a file/symbol tree and a detail pane
showing `↳ calls` / `↰ called by` relations and an **Open definition** link.
Empty until you run the indexer.

### Wiki
*Tools ▸ Knowledge ▸ Wiki; Knowledge rail sub-tab 0; palette `Wiki: Export to Markdown`.*
Generated from the site map. Toolbar: **Refresh** · **Export Markdown** (writes
interlinked `.wiki/` pages) · **Rebuild Index** · **Generate Detailed Page** (asks
the agent for a narrative; focuses Chat). A page tree (FILES / SYMBOLS) beside a
detail pane with relationship groups.

### Knowledge Base
*Tools ▸ Knowledge ▸ Knowledge base; Knowledge rail sub-tab 1; palette `Knowledge Base`.*
Ingest files/folders or **Index workspace**, then full-text **Search** the chunk
store. Results ranked with a score + snippet; a **SOURCES** list with per-source
chunk counts and **Clear all**.

### Snippets
*Build ▸ Advanced ▸ Snippets; Knowledge rail sub-tab 2.*
Filterable list of reusable snippets loaded from `.velocity/snippets.json`
(name, scope, prefix, description).

### NDA Document
*Knowledge rail sub-tab 3; Workspace ▸ New NDA / Import active file / Open browser viewer; palette `NDA: …`.*
Encrypted `.nda` documents. Header badge `sealed`/`portable` + `● unsaved`;
toolbar **Save** · **Commit Revision** (author-signed) · **Export HTML** ·
*Seal at rest*. Sub-views: **Canvas** (renders the draw-command list) ·
**Triples** (subject/predicate/object) · **History** (revision diff) · **Bytes**
(raw dump). An opaque envelope file shows a locked state instead of a document.

### Bookmarks
*Tools ▸ Knowledge ▸ Bookmarks; Files rail sub-tab 1; `Ctrl+Shift+B` toggles at caret.*
Named bookmarks with `path:line`; click to jump; **Clear**.

### Favorites
*Tools ▸ Knowledge ▸ Favorites; Files rail sub-tab 2.*
Starred files (star from the editor tab menu). Click to open; **✖** to remove; **Clear**.

---

## Agents & activity

### Live Activity
*Tools ▸ Agents ▸ Live activity; Agents rail sub-tab 0; `Ctrl+Shift+A`; palette `Live Activity` (Mission Control).*
Uptime + tasks/min; completed/failed/active strip; **WORKERS** cards (progress
bar, files changed); a **CONTEXT CACHE** readout (**Warm from Open Files**); a
stick-to-bottom **FEED**.

### Agent Roster
*Tools ▸ Agents ▸ Agent roster; Agents rail sub-tab 1. Title `Agents`.*
Running agents with a coloured status dot (Running/Done/Failed/other) + status.
Rail variant adds a `✓ ✕ ▶ ⋯` strip and per-task cards.

### Task Queue
*Tools ▸ Agents ▸ Task queue. Title `Queue`.*
Pending dispatches, `▪` coloured by status (Pending/Running/Failed/**Follow-up**). Read-only.

### Timeline
*Tools ▸ Agents ▸ Timeline; Agents rail sub-tab 4.*
Chronological task events (`started`/`completed`/`failed`/`cancelled`/`tool_call`/
`tool_result`/`phase_change`/`token_budget_update`/`session_marker`/`agent_marker`)
with durations. Persisted (sealed) to `.velocity/agentic/mission_activity.nda`. Read-only.

### Mission Metrics
*Tools ▸ Agents ▸ Mission metrics; Agents rail sub-tab 5. Title `Metrics`.*
Counts (Pending/Running/Done/Failed/Active workers) plus totals, avg duration,
tokens, and a success-rate bar. Read-only.

### Live Orchestration
*Tools ▸ Agents ▸ Orchestration; Agents rail sub-tab 2. Title `Orchestration`.*
Active workers (model, files, events) + an activity feed. Event taxonomy:
`▷ Spawned · ⋯ Progress · ✔ Done · ✖ Failed · ◆ Blocked · ⚠ Intervention ·
◇ Routed · ⊡ Checkpoint · ◉ Memory · ℹ Info`.

### Agent Memory
*Tools ▸ Knowledge ▸ Agent memory; Agents rail sub-tab 3; palette `Agent Memory`.*
Per-member memory stores (encrypted with NDA): **Load All** / **Save All**; cards
with category and keywords. *(Shares the visible dock title `Memory` with
Persistent Memory — distinguish by header text.)*

### Shared Memory
*Tools ▸ Knowledge ▸ Shared memory; palette `Shared Memory`.*
The cross-agent knowledge base for collaboration (entries + annotations). Read-only.

### Persistent Memory
*Tools ▸ Knowledge ▸ Persistent memory.*
Long-term store, NDA-encrypted at rest, with a `storage n/max` readout and stored
entries (key, access count, preview). Read-only. *(Dock title `Memory`, same as
Agent Memory.)*

### Background Agents
*Tools ▸ Agents ▸ Background agents.*
Autonomous non-blocking agents + their recent actions. Read-only.

### Conflict Resolver
*Tools ▸ Agents ▸ Conflict resolver.*
Resource locks and conflicts between concurrent agents (strategy, lock timeout);
**ACTIVE LOCKS** and **RECENT CONFLICTS**. Read-only.

### Self-Improvement Engine
*Tools ▸ Agents ▸ Self-improvement. Title `Improve`.*
Records execution failures, classifies them, and generates prompt refinements
(category, directive, confidence, occurrences). Read-only.

### Continuation Ledger
*Tools ▸ Agents ▸ Session continuity. Title `Ledger`.*
Cross-model context handoff: mission goal, scoped files, edit journal, progress,
and model provenance. Read-only.

---

## Build & debug

### Build (rail sub-tab)
*Build rail sub-tab 0; `Ctrl+B`.*
`✓ Build clean` / `✕ {n} error(s)`; **▶ Build** · **▶ Run** · **⏹ Stop**;
Provider / Model / GPU readouts; build-target picker.

### Test Generator
*Build ▸ Test generator; Build rail sub-tab 1.*
Analyzes coverage and emits test skeletons. **🔍 Analyze Coverage** ·
**✨ Generate Tests**; Configuration: Max tests per run (1–100, default 20),
*Public functions only*, *Include assertion placeholders*. Frameworks: Rust
builtin / Pytest / Jest / Mocha / JUnit. Untested-function and generated-test
sections with per-card confidence + **Copy code**.

### Test Coverage
*Build ▸ Test coverage; palette `Test Coverage` (Coder).*
**Analyze workspace** · **Analyze file (LSP)** · **Generate skeletons**; *Public
only* filter; a coverage % bar; untested functions and generated skeletons.

### Deploy Pipeline
*Build ▸ Deploy pipeline; palette `Deploy Pipeline` (Coder). Title `Pipeline`.*
Stages **Build ⚙ / Test ✓ / Package 📦 / Deploy ▲** with timings; **▶ Run
build+test** · **▲ Deploy** · **⇲ Rollback** (`Ctrl+Alt+R`); a last-10 deployments
list.

### <a id="debugger"></a>Debugger
*Build ▸ Debugger; Build rail sub-tab 3; also the bottom Debug tab.*
Attach to a process (**Attach**), then **Continue / Pause / Step Over / Step Into
/ Step Out / Stop**. The bottom debug tab adds state, **Call Stack / Variables /
Watches** columns, a breakpoint list, and **Add Watch**. Chords: `F5` /
`Shift+F5` / `F10` / `F11` / `Shift+F11` / `F9`.

### Language Servers
*Build ▸ Language servers; Build rail sub-tab 4. Title `LSP`.*
Detected LSP servers per language with `initialized`/`starting…`, command and
extensions, and a diagnostic count. **Detect servers** / **Re-detect servers**.

### <a id="precomp-cache"></a>Precomputation Cache
*Build ▸ Advanced ▸ Build cache. Title `Precomp`.*
Explains the speculative pre-computation cache (runs before each agent task, in
the background, keyed per task; entries hold paths, symbols, imports, summaries).
Toasts report pre-indexed files/symbols.

### <a id="inline-suggestions"></a>Inline Suggestions
*Build ▸ Advanced ▸ Inline suggestions. Title `Suggestions`.*
Ghost-text config **status** panel: an *Enabled* checkbox, read-only
trigger-delay / max-chars / min-confidence values, accept statistics, and cache
status. (The values themselves are fixed in `SuggestionConfig`; the editor
floating card is covered on the [Editor surfaces](editor.md) page.)

---

## Automation

### Automation Flows
*Tools ▸ Automation ▸ Automation flows.*
Read-only cards of flows built from orchestrator tasks (title, description,
scope). Create flows via the Orchestrator.

### Site Targets
*Tools ▸ Automation ▸ Targets.*
Recorded site targets from `.velocity_projects.nda`. Capture with the toolbar **●
Record**. Read-only.

### Recordings
*Tools ▸ Automation ▸ Recordings.*
Recorded automation sessions; start with the toolbar **● Record**. Read-only.

### Execution Logs
*Tools ▸ Automation ▸ Execution logs. Title `Logs`.*
The task timeline for automation runs (see [Timeline](#timeline)).

### Voice Commands
*Tools ▸ Automation ▸ Voice commands; Chat rail sub-tab 1; `Ctrl+Shift+V`; palette `Voice Commands`.*
Recognition stats; **Listening** toggle (or type a phrase + **Parse**); an intent
result card; history with example commands. Intents: Open File, Search, Run
Tests, Build, Deploy, Fix Error, Refactor, Create, Undo/Redo, Save, Agent Task,
Show Panel, Navigate, Unknown.

### Multimodal
*Tools ▸ Automation ▸ Multimodal; Chat rail sub-tab 2. Title `Attachments`.*
Images/documents/audio attached to chat turns (`image`/`document`/`audio`); the
rail variant adds **Clear all**, per-file rows, a **💬 Go to Chat** button, and an
attach form.

### Accessibility Audit
*Tools ▸ Automation ▸ Accessibility. Title `Audit`.*
WCAG findings (`{n} finding(s)`); run via the toolbar ✓ button. Backing helpers:
29 ARIA roles, high-contrast palettes, screen-reader simulation, ARIA inspector.

### Triggers
*Tools ▸ Automation ▸ Triggers; palette `Triggers`.*
Headless schedules (`--daemon`). Add a name + schedule spec (`5m`, `1h`,
`daily@09:00`) + prompt. Kinds: `schedule · {interval}`, `file-watch ·
{path}/{glob}`, `webhook`, `manual`; actions route to a workflow or agent. Per
trigger: **Disable/Enable** · **Run now** · **✖**; timing shows `due now`/`next
in …`.

### Workflows
*Tools ▸ Automation ▸ Workflows; palette `Workflows`.*
Sub-tabs **List** · **Visual** · **Templates**. **List:** create workflows, reorder/delete steps, add tool/agent steps,
view the last run. **Visual:** a node canvas (Agent/Tool/Connector/Condition),
connect nodes via `from`/`to` combos, snapshots, and **▶ Run Workflow**.
**Templates:** a gallery of built-in workflow templates grouped by category
(Code Quality, Testing, Automation, Review, Deployment, Research); each card
shows its step count and description with **Add to list** (instantiates the
template into your workflow list, persisted to the workspace) and **Open in
Visual** (loads it onto the node canvas for editing).

### Governance
*Tools ▸ Automation ▸ Governance; Workspace rail sub-tab 5.*
- **POLICY:** default decision (`allow`/`deny`/`needs-approval`), token & cost budgets, allow/deny/approve rules (`tool or *`, optional path prefix).
- **APPROVAL QUEUE:** pending items with `approved`/`denied` outcomes.
- **SECRETS:** named secret values (masked).
- **CONNECTORS:** base URLs + secret handles; presets **GitHub** / **Slack**.

---

## Git

### Changes
*Navigate ▸ Review changes; Tools ▸ Knowledge ▸ Recent changes; Git rail sub-tab 0.*
Stage/unstage (`⊕`/`⊖`, **Stage All**), per-file status (`M A D R ? !`, `S` when
staged), a diff block, a commit-message field, and **Commit** (disabled with the
reason from `commit_blocker()`). The dock tab also shows a **Recent Changes
Timeline** and **Commit History**.

### Branches
*Git rail sub-tab 1; `Ctrl+Shift+G` opens the switcher.*
Branch list with `(current)`, ahead/behind counts, and a refresh; error line when
the query fails.

### Commits
*Git rail sub-tab 2.*
`{n} commit(s)` + **Refresh log**; cards with short hash, date, message, author
(`git log --oneline -30`).

---

## Workspace & peers

### Extensions
*Workspace ▸ Extensions; Workspace rail sub-tab 0; `Ctrl+Shift+X`; palette `Extensions`.*
Manifest folders in `.velocity/extensions/`; **Rescan**; per-card state
(`active`/`installed`/`disabled`/`error`) with command/keybinding counts and
**Activate** / **Disable**.

### Plugin Registry
*Workspace ▸ Plugin registry; Workspace rail sub-tab 1. Title `Plugins`.*
Discovered plugin crates and the tools they add (name, version, author, tools).
Read-only.

### Skill Files
*Workspace ▸ Skills; Workspace rail sub-tab 2. Title `Skills`.*
Reusable capability definitions from `.velocity/skills/`, injected into agent
prompts when tasks route to team members. Rail variant has a **Filter skills…** box.

### Collaboration
*Workspace ▸ Collaboration.*
Shared editing sessions and user presence (users, sessions, participants,
messages). Read-only.

### Peers
*Workspace ▸ Peers; Workspace rail. Title `Peers`.*
Distributed-node chat/transfer surface. Sections:
1. **Peer server** — `Listening on :{port}`/`Stopped`, **Start**/**Stop**, port, identity.
2. **Add peer** — host/port/name → **Connect** (defaults port 9191).
3. **Connected peers** — capability chips, **Chat** / **Health Check** / **Remove**.
4. **Peer chat** — last 20 messages + send field.
5. **Active transfers** — `↑ OUT`/`↓ IN` with progress.
6. **Delegated tasks** — prompt + `TaskStatus` + progress.

### Usage
*Workspace ▸ Usage; Workspace rail sub-tab 4; palette `Usage`.*
Cloudflare **Account Usage** (`↻ Refresh`): requests today, remaining, tokens in/out,
and a per-account grid (Tier · Requests · Remaining · Tokens · Status) with limit
progress bars (`Active`/`At limit`/`Exhausted`).

---

## <a id="rail-only-surfaces"></a>Rail-only surfaces

Panels reachable only as rail sub-tabs (no `TabKind` dock tab):

- **Outline** (Files sub-tab 3): symbols in the active file; click to jump.
- **Browser** (Chat sub-tab 3): a research chat (`🌐 Browse`) — URL + question, streaming answer.
- **Nodes** (Nodes sub-tab 0, `Ctrl+Alt+B`): distributed build drones — where builds route; **Local**; per-drone **Ping** / **Build here** / **Forget**; register form (name, `host:port`, bearer token, work dir); **Run on target** command box.
- **Build** (Build sub-tab 0): see [Build](#build--debug).
- **Checkpoints** (bottom tab 4): automatic pre-agent snapshots; **↩ Restore** / **✖ Discard** (requires a git repo).
- **Mission Activity feed**: `Mission`/`Task #{id}` scoped event rows with timings.

---

*Next: [Settings & providers](settings-providers.md) · [Manual index](README.md).*
