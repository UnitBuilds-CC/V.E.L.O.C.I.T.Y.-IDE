# Settings & Providers

The Settings panel — open with `Ctrl+,`, Help ▸ Settings, the gear at the bottom
of the activity bar, or palette `Settings`. It has four sections: **Appearance**,
**System**, **Agent defaults**, and **Providers & credentials**.

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

- **Provider** — the same 16 providers, in the same order, as the
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

**Alibaba Qwen** is the one provider with the extra token-plan options: when the
*Use Token Plan API key* box is enabled it takes precedence over the standard
key; the base URL defaults to the `token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode`
endpoint when left empty.

### Where credentials live

- Provider keys set here persist to the workspace provider-settings file
  (`.velocity/provider-settings.json`).
- Cloudflare **account usage** additionally reads `CF_ACCOUNT_N_ID` /
  `CF_ACCOUNT_N_TOKEN` from `.env` (see [Usage](panels-reference.md#usage)).
- Secrets are masked in the UI and scrubbed from logs; see
  [SECURITY.md → Credential Boundary](../../SECURITY.md).

---

*Next: [Getting started](getting-started.md) · [Manual index](README.md).*
