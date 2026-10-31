# Runtime Rules Engine

Deterministic, scoped rules that are enforced below the model: a matching tool call
never reaches its handler, whatever the model believes about the instruction.

## Guarantees

1. **Enforcement is not advisory.** A blocking rule refuses the call at the dispatch
   choke point (`registry::call_tool_in_workspace`), which every MCP client and the
   embedded agent share. The model cannot talk its way past it.
2. **Rules refresh on every tool call.** The gate stats the rule files before
   evaluating and reloads them when their mtime or size changed. A rule written by
   one agent in a process applies to every other agent's next call; a rule written
   on disk applies to every process's next call.
3. **Every blocking refusal is directional.** The refusal names the matched
   resource, the rule, its intent, and — when the rule states one — what to use
   instead. A wall with no door is a bug, not a safeguard.
4. **A rule that cannot fire is not stored.** Creation and adjustment run a witness
   pair: one call the rule must block and one near-miss it must allow. Both are
   evaluated against the rule in isolation before the version is written.

## Non-goals for this version

- OS-level firewall compilation for egress rules. It requires elevation and mutates
  shared machine state; the dispatch gate is the enforcement point for now.
- Lifecycle-event rules (post-build cleanup, pre-commit coverage). The schema is
  additive so those triggers can be added without a migration.
- Cross-process budget counters. `tool_budget` counts calls in the process that
  enforces them; two processes keep two windows.

## Model

```text
RuleRecord { id, scope, current: Rule, history: Vec<Rule>, retired_at }
Rule       { id, name, description, kind, params, remedy, owner, provenance,
             version, created_at, updated_at, witnesses }
```

`description` is required and states intent ("the live database is off limits").
`remedy` states the alternative ("use 192.168.0.2 instead"). Both appear verbatim in
refusals. `history` is append-only: adjust writes a new version, retire sets
`retired_at`; nothing is erased, so "which version was in force" is answerable.

### Scopes

| Scope | Stored at | Lifetime |
|---|---|---|
| `task` | process memory, keyed by workspace | until the process exits |
| `workspace` | `<workspace>/.velocity/rules.json` | durable |
| `global` | `<user config>/rules.json` | durable, all workspaces |

Evaluation order is task → workspace → global; the first blocking rule wins, so a
task rule can shadow a broader one. Creation at any scope applies immediately.
Workspace and global creation is labelled with the owner and provenance.

### Kinds

| Kind | Params | Matches |
|---|---|---|
| `tool_ban` | `tools: [string]` | a tool name exactly |
| `egress_ban` | `targets: [ip \| cidr \| host]` | IPv4/CIDR/host targets anywhere in the call's arguments, URL hosts, `-h/--host/host=`, `user@host` |
| `path_ban` | `paths: [prefix]` | any path argument at or below the prefix |
| `command_pattern` | `patterns: [string]` | case-insensitive substring of a command/script argument |
| `tool_budget` | `tool_calls: int`, `window_minutes: int` | every call, counted in a rolling window per rule and process |

## Lifecycle and authority

| Action | Agent | User / supervisor |
|---|---|---|
| create | immediate, owner `agent` | immediate, owner `user` |
| adjust | files a pending request | applies immediately |
| retire | files a pending request | applies immediately |
| bypass (event / session / workspace) | files a pending request | issues a grant |

Monotonicity: an agent can add a rule or tighten one, never loosen or remove one.
Requests are stored beside the rule; approval re-verifies witnesses before writing.
A bypass is a separate grant, never a flag on the rule: `event` is single-use,
`session` and `workspace` expire after the requested minutes (default 60).
Grants are consumed at the gate, so an event bypass allows exactly one call.

## Refusal format

```text
action blocked — server "192.168.1.100"; rule "live-db" (workspace, rule-3f2a1c04):
the live database is off limits, use the dev server. Instead, use "192.168.0.2".
```

## Persistence and failure

- Atomic write: pretty JSON to `<path>.tmp`, then rename.
- A rule file that exists but does not parse fails **closed**: tool calls are
  refused with the file path and parse error until it is fixed. Silent disabling of
  safety rules is worse than a loud stoppage.
- The legacy `.velocity/policy.json` engine keeps running at the same choke point.
  The two are additive; migration of policy rules into `rules.json` is separate work.

## Tools

`rules_list`, `rule_show`, `rule_create`, `rule_adjust`, `rule_retire`,
`rule_bypass`, `rules_pending`. Adjust/retire/bypass return a pending request when
the caller is the agent (the tool surface does not expose approval to the model).

## Interface

Settings → Rules, three tabs, refreshed whenever the rule files' revision moves
(so a rule an agent created mid-run appears without restarting the IDE):

- **Active** — rules grouped by scope; selecting one opens its parameter
  sub-panel: editable name, intent, remedy, and kind-appropriate widgets
  (`tool_calls` / `window_minutes` drag values with their enforced bounds and
  unit for budgets; comma-separated lists for the bans), the version, owner and
  provenance line, the refusal the rule will produce, and Save / Retire / Bypass.
  Retiring arms the button first — a retire has no undo. Live bypass grants are
  listed at the top of this tab: a grant in force should be visible, not
  remembered.
- **Pending** — change requests filed by an agent, with the diff they carry and
  Approve / Reject. Approval exists only here; no tool argument reaches it.
- **History** — retired rules, and the full version chain of the selected rule
  with owner and age.

A command-text rule edited in the panel keeps its proof: the deny-witness's
banned fragment is substituted for the new one, and if no such fragment is
present the save is refused rather than stored unproven. Creating a command-text
rule asks for both proof commands (one blocked, one near-miss) in the form.

A budget has no single proving call — it fires on the one that crosses the
ceiling — so the panel and `rule_create` both preview that first blocked call
through `engine::budget_detail`, the same sentence the gate emits.

## Verification

Unit tests cover every matcher (IPv4 in a command, URL host, CIDR, path prefix
boundary, case-insensitive command text, budget window), witness rejection
(a rule that never fires, a rule whose allow witness also fires, a regex-style
command rule without witnesses), monotonicity and approval, grant expiry and
single-use consumption, atomic persistence and corrupt-file fail-closed, and the
gate itself through `call_tool_in_workspace`. The panel's own paths are tested
where they meet the engine: a rule created from the form blocks the next
dispatched call and its near-miss passes, and saving a tightened budget rebinds
the gate on the next call.
