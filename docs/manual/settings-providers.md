# Settings & Providers

The Settings panel — open with `Ctrl+,`, Help ▸ Settings, the gear at the bottom
of the activity bar, or palette `Settings`. It has five sections: **Appearance**,
**System**, **Agent defaults**, **Providers & credentials**, and **Rules**.

- [Interface & shell](ui-and-shell.md) · [Panel reference](panels-reference.md) · [Manual index](README.md)

---

## Appearance

- **Workspace profile** — Coder / Automation Operator / Mission Control /
  Accessibility (with each profile's description). Switching a profile applies
  its density and scale and rebuilds the layout; it also applies its theme
  unless you have named one below.
- **Theme** — Midnight / Daylight / Operator / Mission / High Contrast. The
  dropdown shows the palette actually in use. A theme named here survives later
  profile switches.
- **Follow system light/dark** — offered where the OS publishes the preference
  (Windows today): the desktop decides Daylight or Midnight, re-checked on a
  short throttle. Naming a theme turns it off, and so does switching profile —
  each profile is advertised by its own palette. `VELOCITY_OS_THEME`
  (`light`/`dark`/`off`) stands in for a desktop that publishes nothing.
- **Density** — Compact / Comfortable / Spacious.
- **Scale** — **UI scale** slider (0.85–1.35) and **Code scale** slider
  (0.5–3.0); **Reset defaults** and **Reset layout**.
- **Editor** (nested checkboxes) — Show breadcrumbs · Word wrap · Auto-indent on
  Enter · Auto-close brackets · Trim trailing whitespace on save.

## System

Shows the active **GPU** adapter name (mono).

## Agent defaults

Sets the fallback agent configuration and pushes it live
(`UiToAgentMessage::{SetProvider, SetModel, SetThinking, RefreshModels}`),
persisting preferences.

- **Provider** — the same 17 providers, in the same order, as the
  [Chat](panels-reference.md#chat) panel.
- **Model** — **dynamic** per provider; **↻ Models** (↔ `Loading…`) refreshes the list.
- **Thinking** (enabled only when the model supports it) · **Auto-approve tools** · **Show thoughts**.

## Providers & credentials

Collapsible headers per provider; secret fields are password-masked. **Save
provider settings** writes to the workspace provider settings file; **Reload**
re-reads it. Badges show `● {Provider}` plus **Workspace configured** (green) or
**Env fallback** (amber). Long model strings truncate at 36 chars.

| Provider | Open by default | Fields |
|---|---|---|
| **Cloudflare Workers AI** | yes | Account ID · API token (secret) · Tier (`free or paid`) · Label (default) |
| **OpenRouter** | yes | API key (secret) · Tier · Label (`OR-Default`) |
| **Azure OpenAI** | no | Endpoint · API key (secret) · Deployment (`gpt-4o`) · API version (`2024-06-01`) · Tier (`paid`) · Label (`Azure-Default`) |
| **Local Ollama** | no | Host (`http://localhost:11434`) · Default model (`llama3.2`) · Label (`Local-Ollama`) |
| **OpenAI** | no | API key (secret) · Label (`OpenAI-Default`) |
| **Google Vertex / Gemini** | no | API key (secret) · Label |
| **Deepseek** | no | API key (secret) · Label |
| **Groq** | no | API key (secret) · Label |
| **Mistral AI** | no | API key (secret) · Label |
| **Together AI** | no | API key (secret) · Label |
| **Fireworks AI** | no | API key (secret) · Label |
| **Perplexity** | no | API key (secret) · Label |
| **Cerebras** | no | API key (secret) · Label |
| **AWS Bedrock** | no | API key (secret, *Bedrock proxy key — set `BEDROCK_PROXY_URL` env var*) · Label |
| **Anthropic** | no | API key (secret) · Label |
| **Alibaba Qwen** | no | API key (secret) · Label · **Pay-per-token plan**: *Use Token Plan API key* checkbox · Token plan API key (`sk-sp-…`) · Token plan base URL |

**Local llama.cpp** is the one backend of the seventeen with no credential
section here: a local `llama-server` takes no key, and its endpoint comes from
the `LLAMA_CPP_HOST` environment variable of the process that runs the agent
(default `http://localhost:8080`). You can select it from either provider picker
and dispatch to it, but you cannot point it at a different host from this panel —
and the failover chain only routes to it when that variable is set, since
otherwise the loop has no way to know a server is listening. AWS Bedrock is the
other key-free backend — it has a section because its optional proxy key lives
there, but neither it nor llama.cpp takes a row in the credential file this
panel writes.

**Alibaba Qwen** is the one provider with the extra token-plan options: when the
*Use Token Plan API key* box is enabled it takes precedence over the standard
key; the base URL defaults to the `token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode`
endpoint when left empty.

### Where credentials live

- Provider keys set here persist to the **user-level** `provider-settings.json`
  — `%APPDATA%/Velocity/provider-settings.json` on Windows,
  `$XDG_CONFIG_HOME/velocity/` or `$HOME/.config/velocity/provider-settings.json`
  elsewhere (`VELOCITY_CONFIG_DIR` relocates that directory). Nothing
  credential-shaped is stored in the workspace.
- Every backend in that file also has an environment fallback, which the loaders
  consult only when the file does not already configure that provider: Cloudflare
  `CF_ACCOUNT_{n}_ID` with `CF_ACCOUNT_{n}_TOKEN` (`n` runs 1 to 30), OpenRouter
  `OPENROUTER_ACCOUNT_{n}_KEY` or `OPENROUTER_API_KEY`, Azure
  `AZURE_OPENAI_ACCOUNT_{n}_KEY` with its endpoint. A `.env` file in the working
  directory is loaded as part of that read (see
  [Usage](panels-reference.md#usage)).
- Secrets are masked in the UI and scrubbed from logs; see
  [SECURITY.md → Credential Boundary](../../SECURITY.md).

---

## Rules

The deterministic rule engine: constraints checked *below* the model, so a rule binds
whether or not the agent remembers being told. Design and guarantees:
[rules-engine.md](../rules-engine.md).

- **Header** — *RULES — ENFORCED OUTSIDE THE MODEL*, with the promise that an edit here
  takes effect on the next call, in every agent. A corrupt rule file replaces the list with
  `⚠ rules unavailable — tool calls are being refused until this is fixed: …`, because a
  silently disabled safety rule is worse than a stopped one.
- **Tabs** — **Active (n)** · **Pending (n)** · **History (n)**, plus **Reload** to re-read
  the files on demand. The panel also refreshes itself whenever a rule file changes, so a
  rule an agent created mid-run appears without restarting the IDE.
- **Active** — rules grouped **This task** / **This workspace** / **Every workspace**, most
  specific first, with any **Bypasses in effect** listed above them. Selecting a rule opens
  its editor: **Intent**, **Remedy**, the kind's own fields (comma-separated lists for the
  bans; **Tool calls** 1–100,000 and **Window** 1–10,080 `min` for a budget), the version,
  owner and provenance line, and under *The agent reads this when it fires:* the exact
  sentence the refusal will carry. Then **Save changes**, **Retire** (the button arms on the
  first click — a retire has no undo) and **Bypass** → **Grant**.
- **Kinds** — `tool_ban` (banned tools) · `egress_ban` (banned targets: IPv4, CIDR or
  hostname) · `path_ban` (banned paths, workspace-relative prefixes) · `command_pattern`
  (banned text, case-insensitive) · `tool_budget` (a call ceiling over a rolling window).
- **Pending** — what an agent proposed against a durable rule (adjust, retire, bypass), with
  the diff it carries and **Approve** / **Reject**. Approval lives here and nowhere else: no
  tool argument can grant an agent its own exemption.
- **History** — retired rules, and the full version chain of the selected rule with owner
  and age.
- **Add a rule** — **Name** · **Intent** · **Kind** · **Scope** · the kind's parameters ·
  **Remedy** · **Source** (what made this necessary), and for a command-text rule the two
  proof fields **Blocks** and **Allows**. A rule whose witness pair does not actually fire
  is refused at creation rather than stored as something that does nothing later.

Rules persist to `<workspace>/.velocity/rules.json` and, for the global scope,
`<user config>/rules.json`; task rules are memory-only and end with the process. The eight
`rule*` MCP tools ([mcp-tools.md → rules](mcp-tools.md#rules)) are always on the surface,
because the worker about to repeat the mistake is the one that can record the rule stopping
it.

---

*Next: [Getting started](getting-started.md) · [Manual index](README.md).*
