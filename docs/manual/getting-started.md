# Getting Started

V.E.L.O.C.I.T.Y. is a native, GPU-accelerated IDE with a built-in multi-agent
reasoning engine, a pure-Rust browser control plane, and a desktop-automation
layer. This page gets you from a cold start to your first agent-assisted edit.

- [Manual index](README.md) · [Interface & shell](ui-and-shell.md) · [Settings & providers](settings-providers.md)

---

## 1. Install & launch

**Prerequisites:** Rust 1.87+ (stable, per `rust-toolchain.toml`) and Git.

```bash
git clone https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-IDE.git
cd V.E.L.O.C.I.T.Y.-IDE
cargo build --release

# Native GUI
cargo run --release --bin velocity_ide_gui

# Headless MCP server (for programmatic agents / editors over stdio)
cargo run --release --bin velocity_mcp -- --mode stdio
```

See [Deployment Guide](../DEPLOYMENT.md) for Docker / production setups and
[Runbook](../RUNBOOK.md) for operations.

## 2. Open a workspace

On first launch the **Welcome screen** shows the VELOCITY wordmark, a
**＋ New Task** button, secondary actions, and recent files. Open a project with
the header **folder** button (Workspace switcher), `Ctrl+O`, or by adding it to
the switcher list. The workspace name drives the identity pill at the bottom of
the activity bar.

## 3. Choose a workspace profile

Profiles reshape the whole UI (theme, density, dock tabs, right-panel sections,
palette filters). Pick one from **Layouts ▾** or a chord:

| Profile | Chord | Best for |
|---|---|---|
| **Coder** | `Ctrl+1` | Day-to-day editing, build/debug, coverage |
| **Automation Operator** | `Ctrl+2` | Desktop/browser automation, orchestrator, workflows |
| **Mission Control** | `Ctrl+3` | Launching and supervising multi-agent missions |
| **Accessibility** | `Ctrl+4` | High-contrast, audit-oriented work |

## 4. Configure a provider

Open **Settings** (`Ctrl+,`) → **Providers & credentials**, paste an API key for
at least one backend, and **Save provider settings**. Cloudflare Workers AI is
the default (`@cf/moonshotai/kimi-k2.7-code`); OpenRouter, Azure OpenAI, and
Local Ollama are the failover chain, but all 16 providers are configurable. Then
set your fallback **Agent defaults** (provider, model, thinking, auto-approve).
Full field-by-field detail is on the
[Settings & providers](settings-providers.md) page.

> You can also seed configuration via `~/.velocity/config.toml` or environment
> variables (`VELOCITY_API_KEY`, `RUST_LOG`). Cloudflare usage dashboards read
> `CF_ACCOUNT_N_ID` / `CF_ACCOUNT_N_TOKEN` from `.env`.

## 5. Ask the agent

Open **Chat** (`Ctrl+J`). Pick a provider and model in the header, type a
request, and `Enter` to send (`Shift+Enter` for a newline). Turn on the `👁
Reasoning` toggle to watch the model think. When a tool call needs approval, use
the pending-approval strip (**Approve**/**Decline**, or **Auto-approve**). Inline
ghost suggestions appear in the editor — **Tab** accepts, **Esc** dismisses.

## 6. Build, run, and check

- **Build** `Ctrl+B`, **Run** `Ctrl+R` (bottom **Terminal** shows output; the
  **Problems** tab lists diagnostics).
- **Search** `Ctrl+Shift+F` (literal or semantic), **Quick Open** `Ctrl+P`,
  **Go to Symbol** `Ctrl+Shift+O`.
- Review uncommitted work with **Git → Changes** (`Review changes`) and commit
  from there.

## 7. Scale up

- **Orchestrator** (`Ctrl+Shift+Y`) — plan multi-step work; hand-write tasks or
  **Route goal** to decompose one, then **Execute**.
- **Mission Control** (`Ctrl+3`) — plan → work → activity for supervised missions.
- **Nodes** (`Ctrl+Alt+B`) — route builds to distributed drone machines.
- **Team Studio** — assemble expert teams and route work to them.

Each is documented in depth on the [Panel reference](panels-reference.md) page.

---

## Where to look things up

| I want to… | Go to |
|---|---|
| Find a keyboard shortcut or command | [Keyboard shortcuts & commands](keyboard-and-commands.md) |
| Understand the window, rails, sidebars, overlays | [Interface & shell](ui-and-shell.md) |
| Use the editor, terminal, minimap, completion | [Editor surfaces](editor.md) |
| Learn what a specific panel does | [Panel reference](panels-reference.md) |
| Set up a provider / credential | [Settings & providers](settings-providers.md) |
| See every MCP tool an agent can call | [MCP tool reference](mcp-tools.md) |

---

*Next: [Interface & shell](ui-and-shell.md) · [Manual index](README.md).*
