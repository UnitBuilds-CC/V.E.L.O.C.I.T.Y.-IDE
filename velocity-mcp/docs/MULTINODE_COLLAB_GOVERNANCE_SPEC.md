# Multi-Node Collaboration + Governance — Integration Spec

Status: **Proposed (spec-first; no code changes yet).** An **existing-systems audit
is complete (§11)** — Spine-1/2 are genuinely new; System B, the cross-node transport,
the node registry, and the RBAC engine already exist and are marked REUSE/EXTEND, not
rebuild. This document is the
authoritative design for three features that must be built as **one system**, not
three:

> **Implementation status (rolling):** **Phase 1 (Spine) COMPLETE.** 1a
> `automation/principal.rs` (`Principal`, `PrincipalKind`, `NodeId`, `OpId`,
> `VectorClock` + causal `compare`), 9 tests. 1b `automation/oplog.rs`
> (`Op`/`OpEffect`/`OpSet` + a last-write-wins `materialize`), 5 tests proving
> INV-2 (permutation-invariant), INV-3 (idempotent), INV-4 (legacy → inert
> `Service`/`Legacy`). 1c wired `actor: Principal` (`#[serde(default)]`) into
> `CodebaseEvent` — every existing caller unchanged, back-compat test added; the
> full `velocity_mcp` lib suite stays green (**2942 passed / 0 failed**). No real
> principal is emitted yet (defaults to `Service`); that lands in Phase 2/5.
> `cargo fmt` / `cargo clippy --lib -D warnings` / `cargo test` green.
> **Spine-3 COMPLETE:** `automation/audit.rs` — `caused_by` / `engagement` walk the
> op-log from `authorized_by` seeds through the `causal_deps` closure (deterministic,
> cycle-safe), linking the two audit stores *by id without merging them* (§3);
> 6 tests (INV-7). Full lib suite now **2948 passed / 0 failed**.
> **System A convergence core (Phase 3 primitive) COMPLETE:**
> `automation/partition.rs` — `partition_by` / `materialize_partitioned` give each
> scope its own sub-log that materializes independently; tests prove the
> per-partition union equals the global fold and that **granularity (D4) is a
> caller-supplied key seam, policy-only, never affecting the converged result**
> (INV-2 across partitions). 4 tests; full lib suite **2952 passed / 0 failed**.
> All purely additive — no live write path wired yet.
> **System A Tier-1 text convergence COMPLETE:** `automation/merge.rs` — a
> `MergePolicy` trait (the D1 seam) with `LastWriteWins` (rank `(clock, OpId)`, a
> total order ⇒ convergent + idempotent) materializing per-file text heads; tests
> cover clock-vs-OpId, permutation invariance (INV-2), idempotence (INV-3), and a
> swappable alternative policy proving D1 plugs in with no caller change. 6 tests;
> full lib suite **2958 passed / 0 failed**; clippy `--lib --tests -D warnings` clean.
> **D5 RESOLVED — the two write-gates UNIFY** (user decision: "unify, for
> maintainability"). **Stage 1 COMPLETE:** the authoritative lock table is now a
> single type `automation/write_gate.rs::WriteGate` (range + `ConflictKind` + TTL +
> semantic-check-when-a-`SiteMap`-is-supplied + `claim_whole_file`); `MediatorArena`
> is a thin delegating wrapper (public API unchanged → zero call-site churn for
> `scope.rs`/`runner.rs`/`code_editor.rs`/`render.rs`). Two new tests
> (`claim_and_range_lock_share_one_table`, `facade_claim_shares_the_mediator_table`)
> prove a whole-file claim and a range lock now conflict through ONE table — the R-1
> defect closed at the *type* level. Suite **2964 passed / 0 failed / 16 ignored**;
> fmt + clippy `--lib --tests -D warnings` clean.
> **Next: Stage 2 — point `CoordinationBus::claim_file/release_file` at the shared
> `WriteGate` (delete its private table, unify identity on `canonicalize_scope_path`),
> and thread one `Arc<MediatorArena>` from the app into the executor spawn so the
> chat-agent path and the orchestrator path consult the *same instance* (R-1 fully
> closed at runtime), then Phase 2 watcher → attributed Human ops.**
> **Stage 2 COMPLETE — D5 fully unified.** `CoordinationBus` no longer owns a
> `file_locks` map: `claim_file`/`release_file`/`is_locked_by_other`/`locked_files`/
> `lock_details` all delegate to a shared `Arc<WriteGate>` (single identity,
> broadcasts preserved, progress/help unchanged). `MediatorArena` now holds
> `Arc<WriteGate>` with `from_gate`/`gate_arc`; `run_agent_thread` takes the app's
> `Arc<MediatorArena>` and builds its bus on `mediator.gate_arc()` — the two live
> paths (chat executor + orchestrator) now consult the *one* table. Proven by
> `agent::coordination::tests::shared_gate_makes_bus_claims_visible_to_mediator`
> (an executor claim blocks the mediator and vice versa through one gate). Both
> binaries (`velocity-ide-gui`, `velocity-serve`) `cargo check` clean; suite
> **2965 passed / 0 failed / 16 ignored**; clippy `--lib --tests -D warnings` clean.
> **Next: Phase 2 — fold the existing watchers into op-append (attributed Human ops),
> then Phase 3 convergence replication over the peer transport, then Phase C
> RBAC-in-executor — each wired onto the now-unified gate.**
> **Phase B started — durable op-log store COMPLETE (physical source of truth).**
> `automation/op_log_store.rs::OpLogStore` persists the `OpSet` as JSONL at
> `.velocity/oplog/ops.jsonl`, reusing `event_store`'s atomic single-`write_all`
> append + tolerant load; added `Op::new_text` for watcher-emitted edits. No
> sequence mutex (op-log converges order/dup-independent — INV-2/INV-3). 7 tests;
> full lib suite **2972 passed / 0 failed / 16 ignored**; fmt + clippy `--lib
> --tests -D warnings` clean. **Now WIRED LIVE (corrected seam).** `automation/
> change_attribution.rs::ChangeAttributor` turns a detected change into a `Text` op
> attributed to a *constructor-supplied* `Principal` (`open_human` for the watcher,
> `open(workspace, agent_principal)` for the executor — one path, no special case),
> appends it to `OpLogStore`, and can re-flag dependents through `SiteMap::get_callers`
> (INV-5). The **feedback-loop guard is the durable log itself**: `record` re-reads the
> op-log and skips any `(path, content-digest)` already recorded, so two producers on
> different threads/processes dedupe against shared truth with no in-memory channel.
> Attribution is driven off `editor/file_watcher.rs` (the vendored `notify` watcher —
> the spec's named, **change-only** seam), wired in `velocity_app::
> attribute_external_changes` from the render loop, **not** `handle_ast_update` (which
> fires for every file on the AST watcher's *initial scan* and would flood the trail
> with fake edits — that earlier seam was reverted). 9 attribution tests; full lib
> suite **2981 passed / 0 failed / 16 ignored**; fmt + clippy `--lib --tests -D
> warnings` clean; `velocity-ide-gui` `cargo check --bins` clean.
> **Executor-side agent attribution WIRED (System B COMPLETE on both producers).**
> `loop_runner.rs` opens one shared `Arc<Mutex<ChangeAttributor>>` (an agent
> `Principal`, so parallel tool threads in a batch advance *one* node clock rather
> than minting colliding `op_id`s), and on a successful `write_file`/`apply_diff`
> release seam reads the on-disk bytes, derives the digest through the *same*
> `ChangeAttributor::digest_of_content` the human watcher uses, and `record`s the
> op — so the agent's `(path, digest)` lands in the log **before** the `notify`
> watcher echo and is deduped by the shared durable log, crediting the change to
> the agent, never a human. A failed tool (`tool_ok == false`) is never logged.
> Both producers now fold through one append path (spec §5 "no special path");
> `cross_producer_echo_is_deduped_via_the_shared_log` proves the cross-producer
> guard. Suite **2981 passed / 0 failed / 16 ignored** (seam is compile-gated, no
> new unit test); fmt + clippy `--all-targets -D warnings` clean.
> **Phase C started — `automation/governor.rs` + the mission chokepoint SHIPPED.**
> `Capability` gains `Collaborate` / `DispatchTo` (§6 cross-team engagement; all
> old org documents still load — additive serde variants only). `Governor` is the
> Principal-keyed front to `company_org::check_permission`: it resolves the role
> staffing a team through the existing `Role::agent_team` bridge
> (`principal_for_team`), denies unidentified principals under governance
> (INV-6), and appends **every denial to the durable op-log** as its own audit
> op (`Legacy` effect — inert for materialize *and* for the System-B dedupe set,
> so a deny advances the shared node clock without posing as a content change).
> An **ungoverned workspace (no org doc) passes everything** — governance is
> opt-in per workspace, never an accident of a missing file, which is how §6's
> unattended-safety rule is honored. Chokepoint §6.2 wired in `run_mission`:
> before any headless dispatch, live runs need `DispatchTo` on `team/<slug>`;
> denied → `Err("rbac: mission dispatch denied…")` + trail entry; approval-scoped
> → refused with a pointer to the preview; **dry runs stay open** (they dispatch
> nothing and mint no audit ops — the preview is the operator's decision tool).
> 8 governor tests + 4 chokepoint tests; suite **2993 passed / 0 failed /
> 16 ignored**; fmt + clippy `--all-targets` clean.
> **Chokepoint §6.2 completed for BOTH dispatch paths.** `run_mission` and
> `team_dispatch` now consult one seam — `Governor::require_dispatch(team)` —
> so the two paths cannot drift into enforcing different rules; the team_dispatch
> gate fires *after* team resolution, *before* member routing (nothing is spent
> on a denial). `principal_for_team` also resolves roles bound to the org's own
> team (empty `agent_team` → `org_team_slug`), closing the bridge's other half.
> 3 more governor tests + 1 team_tools chokepoint test; suite **2997 passed /
> 0 failed / 16 ignored**; fmt + clippy `--all-targets` clean. **Next: Phase 3
> convergence replication over the peer transport (the remaining System-A half),
> then chokepoints §6.1/§6.3/§6.4 (scope.rs, watcher human-scope, unified
> WriteGate) — those wait on the §6 'real human authentication' identity work
> (peer shared-secret token → org role), without which a gate at the write-table
> level could only deny the human editor, not scope it.**
> **Phase 3 convergence replication COMPLETE (System A's cross-node half closed).**
> `automation/replication.rs` is the whole new surface, exactly as D2 predicted:
> a `ReplicationView` (merged observed clock + exact `op_id` set), the
> clock-domination shipping rule `needs()` (dense per-node counters let a
> claimed `node#N` prove away every lower resend; an unlisted id is still
> shipped — the Legacy-effect denial shape), idempotent `merge_remote_ops`, and
> `handle_exchange` (merge, then answer in kind). Transport reuses the existing
> peer subsystem untouched: one `POST /peer/ops` arm in `peer_server.rs` over
> `PeerManager.workspace_root`'s durable log, plus `PeerManager::sync_ops_with_peer`
> on the client — no parallel sync channel was invented. Tests (10): two-node
> convergence to one materialized site-map state (INV-2 across nodes), full-log
> resend changes nothing (INV-3, the durable file does not even grow),
> replicated-in ops keep next-stamped `op_id`s collision-free on both sides,
> and a real-TCP round between two tempdir workspaces over an ephemeral port.
> `VectorClock::set_max` added (single-component join, needed by the wire view).
> Suite **3007 passed / 0 failed / 16 ignored**; fmt + clippy `--all-targets`
> clean. **Next: chokepoints §6.1/§6.3/§6.4 (scope.rs, watcher human-scope,
> unified WriteGate) — those wait on the §6 'real human authentication' identity
> work (peer shared-secret token → org role), without which a gate at the
> write-table level could only deny the human editor, not scope it. Periodic
> scheduling of `sync_ops_with_peer` (heartbeat-adjacent) is the remaining
> operational wire-up.**
> **§6.4 chokepoint SHIPPED — the write gate is now scoped, not merely deniable.**
> The identity blocker is closed by the *human* half of the join, symmetric with
> `agent_team`: `Role.humans: Vec<String>` (additive `serde(default)`, so every
> sealed org document still loads) lists the real `Principal.user_id`s staffing a
> role. `Governor::principal_for_human(user_id, node)` resolves that binding to a
> `role_id` (unbound → `None` → denied under governance, passed when ungoverned),
> and `Governor::require_write(&Principal)` mirrors `require_dispatch` — one seam,
> pass/fail `Result`, denials appended to the durable trail (INV-6) — so the two
> chokepoints cannot drift. It is wired **inline into the executor**: `loop_runner`
> opens a `Governor` over `workspace_root` once per run and, only when the
> workspace is governed, threads an `Arc<Governor>` + the OS-login user's
> `Principal` (the *same* identity the GUI human watcher attributes manual edits
> to, `struct_def`) into every write thread, which consults `require_write`
> **before** `claim_file` takes the edit lock — a write is admitted because the
> human's role holds `Write` on `WORKSPACE_RESOURCE`, not because a bare `"primary"`
> string holds the lock. Ungoverned workspaces yield `None` and the gate is never
> consulted, so the ~3000 tempdir executor tests are untouched (verified: no
> existing run is governed). 3 governor tests (bound human writes / unbound denied
> + audited / ungoverned inert); suite **3010 passed / 0 failed / 16 ignored**;
> fmt + clippy `--all-targets` clean.
> **§6.1 chokepoint SHIPPED — the headless orchestrator worker can no longer
> bypass the write gate (spec §11 R-1).** `worker::runner` consults the *same*
> `Governor::require_write` seam as §6.4 immediately *before* `acquire_scope_locks`
> takes any lock, resolving the acting principal from the OS-login user — the
> node's local identity, exactly as the chat executor does — so a governed
> workspace admits a mission's writes only when that human's role holds `Write`
> on `WORKSPACE_RESOURCE`, and a denial lands in the durable trail before a
> single scope lock is minted. This deliberately reuses the human identity join
> rather than threading `team_id` into `WorkerAssignment` (the orchestrator
> carries no team at lock time); the agent-side `principal_for_team` join remains
> available for a future per-mission scope when a task is team-attributed.
> Ungoverned workspaces short-circuit on `governed() == false`, so every tempdir
> orchestrator test is untouched. Suite **3010 passed / 0 failed / 16 ignored**;
> fmt + clippy `--all-targets` clean. **Both write admission paths (§6.1 headless,
> §6.4 interactive) now flow through one `require_write` seam over the unified
> `WriteGate` — the "write gates unified for maintainability" outcome.
> **System A operational entry point `PeerManager::sync_all_peers()` SHIPPED:** one
> call fans a `sync_ops_with_peer` round across every *online* peer, collecting
> per-peer failures instead of aborting (a dead node never stalls fleet
> convergence) and skipping offline peers entirely — the heartbeat-adjacent entry
> point a periodic trigger (or a manual "sync now") calls, so cross-node
> convergence no longer requires hand-targeting each peer. 1 test; suite **3011
> passed / 0 failed / 16 ignored**; fmt + clippy `--all-targets` clean.
> **Remaining operational wire-up:** a timer that *invokes* `sync_all_peers` on an
> interval (no such periodic host loop exists yet — `update_presence` is likewise
> only driven on demand); the convergence primitive and its fleet fan-out are both
> shipped and tested.**

- **(A) Convergent site map** (CRDT-style) replicated across nodes → shared workspaces.
- **(B) Manual-edit watcher** → every human change attributed and dependency re-flagged.
- **(C) Org RBAC + audit** wired inline into the executor, with a causal engagement trail.

Everything below is grounded in the current code; symbol references are given so a
reviewer can verify each claim against the tree.

---

## 0. Why these three are one problem

Today the three concerns are disconnected verticals with **two identity notions and
two audit stores that do not talk to each other**:

| Concern | Where it lives today | Identity | Persistence |
|---|---|---|---|
| Shared-write / coordination | `automation/mediator.rs`, `orchestrator/worker/scope.rs` | `agent_id = "task-{id}"` (String) | in-process `HashMap`, `Instant`, 1800 s TTL — **not durable, not cross-node** |
| Change / decision trail | `registry/event_store.rs` | none (no actor field on `CodebaseEvent`) | `.velocity/events/events.jsonl`, global `APPEND_LOCK`, **single process** |
| Governance / approval | `editor/company_org.rs` | `role_id`; humans collapse to `const HUMAN_OPERATOR: &str = "human-operator"` | `.velocity/org_approvals.nda` + verdict trail |

Building a watcher, a CRDT, and an inline gate *separately* on top of these yields
three half-systems that still cannot answer the user's core questions:
*"who changed this line, under whose authority, and what did it cascade to?"*

**Fix:** refactor onto a shared **spine** first (§1–§3); the three features then
become thin consumers of it.

---

## 1. Spine-1 — a single causal `Principal`

New type (proposed in `velocity-mcp/src/automation/principal.rs`, re-exported):

```rust
pub enum PrincipalKind { Agent, Human, Service }

pub struct Principal {
    pub kind: PrincipalKind,
    pub team_id:   Option<String>,   // Agent
    pub role_id:   Option<String>,   // Agent OR Human → key into company_org roles
    pub task_id:   Option<u64>,      // Agent  (replaces the bare "task-{id}" string)
    pub user_id:   Option<String>,   // Human  (real identity; ends the HUMAN_OPERATOR sentinel)
    pub node_id:   NodeId,           // which machine — required for cross-node clocks
    pub display:   String,
}
```

This single type resolves three requirements at once:

1. **Manual-edit "by whom"** — watcher emits ops whose principal is `kind=Human`.
2. **Manager A vs Manager B** — distinct `user_id`, not the shared sentinel.
3. **Cross-store join** — an `Agent` principal carries **both** `task_id` (executor)
   and `role_id` (org), so `mediator`/`scope` and `company_org` finally share a key.

`EditLock.agent_id` stays for display/back-compat but gains `principal: Principal`.
`CodebaseEvent` gains `actor: Principal` (`#[serde(default)]`, see §6).

---

## 2. Spine-2 — the op-log becomes the source of truth

`events.jsonl` evolves from "a log of tool calls" into a replicated **operation log**.
The site map becomes a *derived materialized view* rather than the authoritative store.

```rust
pub struct Op {
    pub op_id: OpId,                 // content-derived, ULID-like: (node_id, counter, hash)
    pub clock: VectorClock,          // { node_id -> counter }; causal order + concurrency
    pub principal: Principal,
    pub causal_deps: Vec<OpId>,      // ops this one supersedes / reads
    pub intent: String,              // the *why* — already captured as CodebaseEvent.context
    pub effect: OpEffect,
    pub outcome: EventOutcome,       // reuse existing enum: Success/Failure/Revert/Pending
    pub authorized_by: Option<ReqId>,// links to a company_org ActionRequest.id (Spine-3)
}

pub enum OpEffect {
    Triple { file_path: String, add: Vec<VcTriple>, rm: Vec<VcTriple> }, // site-map edge
    Text   { path: String, ndiff: NDiff },                               // file bytes
    Claim  { scope: PathBuf, range: (usize, usize) },                    // cross-node lease
    Release{ scope: PathBuf },
    Approve{ request_id: ReqId, decision: Verdict },                      // governance op
    …
}
```

**Materialization (the determinism that makes this a CRDT):**

```rust
pub fn materialize(ops: &OpSet) -> SiteMapState
//  = deterministic topo-sort of `ops` by causal_deps, tie-break by (clock, op_id),
//    then fold over an empty SiteMap with idempotent per-effect handlers.
```

This is legal because `SiteMap.root()` is already a pure function of content —
`recompute_root()` runs on every `put_file_snapshot` / `remove_file_snapshot`
(`velocity-ide/src/site_map/store.rs:513,537,565`). Therefore **any node that
replays the same op set yields the same `root()`** → convergence, with no central
authority. `root()` / `weight_root()` are **demoted to derived checkpoints**, used
for integrity + the existing staleness check, not as the write source.

**Back-compat:** a legacy `events.jsonl` line has no `principal`/`clock`/`deps`. It
loads as an `Op` with `principal.kind=Service{legacy}`, `causal_deps=[]`, and
`effect=Text{no-op}`: the decision trail and `read_file` "Decision Trail" keep
working untouched, and legacy lines are **not** treated as authoritative state —
only new op-tagged `Triple`/`Text` effects feed `materialize`. This keeps §2
shippable without rewriting history.

**Physical store (shipped — `automation/op_log_store.rs::OpLogStore`).** The
[`OpSet`] above is the *logical* source of truth; `OpLogStore` is its *physical*
home on disk at `<workspace>/.velocity/oplog/ops.jsonl`, so converged state
survives restarts and can be replayed/replicated. It mirrors the discipline
already proven in `registry/event_store.rs`: one JSON object per line, the
serialized line + newline written in a single `write_all` (so concurrent
appenders cannot interleave half-records — the historical `}{` collision bug),
and a tolerant `load` that skips blank/un-parseable lines instead of poisoning
the log. Unlike the event store there is **no sequence mutex**: an `Op` carries
its own `(node, counter)` `OpId` and `materialize` is order- and duplicate-
independent (INV-2/INV-3), which is precisely why the log can be append-only and
gossip-safe. `open`/`append`/`append_many`/`load`/`materialize` + `Op::new_text`
(the watcher's edit shape) ship with 7 tests (round-trip, one-atomic-line-per-op,
malformed/blank skip, missing-file→empty, duplicate→idempotent reload, 8-thread
concurrent-append no-interleave, text-op round-trip).

---

## 3. Spine-3 — one causal audit chain (link, do not merge)

Keep the two stores separate — `is_high_signal()` / `file_verdict()` in
`event_store.rs` and the org verdict trail both depend on their current shapes.
Instead, **cross-link by id**:

- A governance decision writes an `Approve{request_id, decision}` op (§2).
- Every op produced by an approved action sets `authorized_by = request_id`.
- `HUMAN_OPERATOR` is retained **only** as the deny-to-human fallback label when a
  policy names no approver; it is no longer the default identity for real actions.

Query: *"which edits are the transitive result of request R?"* = walk ops where
`authorized_by == R.id` **plus** their `causal_deps` closure. This is what lets
Manager B see *why* the system did ZYX after Manager A asked for XYZ.

---

## 4. System A — convergent site map + shared workspace

**Current:** `MediatorArena` (pessimistic, in-process, in-memory) + optimistic
`site_map.root() != planned_site_map_root` staleness check in the worker runner.

**Target:** keep what is tested, add convergence + replication.

- **Partition by scope.** Reuse each team member's existing `scope_patterns`
  (`registry/tool_definitions/team.rs`) as CRDT partition boundaries. Each partition
  carries its own sub-op-log and materializes independently → cheap merges; cross-
  partition references (the predicate-2 `Call` edges) reconcile through causal deps.
  Directly serves "Manufacturing team and AI team working in one repo."
- **Two-layer conflict handling (do not regress the current taxonomy):**
  - *Intra-node:* retain `MediatorArena::acquire_lock` and `ConflictKind::{DirectLine,
    ScopeOverlap, Semantic}` + `resolve_conflict` ("MEDIATION CONTRACT") as the fast path.
  - *Cross-node:* publish `Claim{scope}` / `Release{scope}` ops (advisory leases using
    the same kind labels), and resolve genuine divergence by **merge** instead of hard
    block. `ConflictKind::Semantic` (dependency coupling) is computed from the
    *materialized* map via `get_callers` — so a human edit that changes a caller
    re-flags dependents automatically (that is the whole §5 win, for free).
- **Text convergence tiers.** The site map holds symbol edges, not file bytes; text
  convergence is the remaining hard part. Ship **Tier 1 = per-file LWW** (conflict
  resolved by `clock` then `op_id`; `Claim` discourages same-file concurrency).
  Reserve a `MergePolicy` seam so a sequence CRDT (RGA/Fugue-style) can replace LWW
  per partition later, without touching the op model.
- **Replication transport (RESOLVED — reuse, do not build):** a full peer
  subsystem already exists and speaks the exact primitives we need. `agent/peer_server.rs`
  serves `/peer/*` over a dependency-free `std::net::TcpListener` (default port
  9191); `agent/peer_link.rs` provides `PeerManager`/`PeerIdentity` with **pairing
  via shared secret**, **chunked file transfer with SHA-256 integrity**, messaging,
  and task delegation; `agent/peer_bridge.rs` already pumps `PeerMessage` ⇄
  `AgentBroadcast` onto the local `CoordinationBus`; `agent/peer_robust.rs` adds the
  resilience layer. Ops carry `node_id` + `Principal` and ride this channel as
  gossip/relay. **No new transport or auth system is introduced here** (that is §6).
  Caveat: the peer layer carries *messages/files/tasks*, not merge-ordered ops — the
  op-gossip endpoint (`/peer/ops`) and vector-clock shuffling are the only new surface.
- **Liveness-based routing already exists:** `agent/instances.rs` `InstanceRegistry`
  (`.velocity/instances.json`) + `InstanceStatus::{Online,Degraded,Offline,Unknown}`
  probed via `/peer/health` with `HEALTH_STALE_SECS = 120`. This is the substrate for
  the user's "ping each node, route through whichever works" ask (see §11, Ollama).
- **⚠ Two write-gating mechanisms exist and must be reconciled before Phase 3/4** —
  `automation/mediator.rs::MediatorArena` (line-range + `ConflictKind`) *and*
  `agent/coordination.rs::CoordinationBus` file claims (`claim_file`/`release_file`,
  `FileClaimed`/`FileReleased`), already wired into the live loop. They are separate
  locks over the same files; see §11 R-1.

---

## 5. System B — manual-edit watcher

**Current (CORRECTED — a watcher is NOT missing; three exist, none attribute):**
change detection is already solved three times over, and external edits *already*
reach the durable site map in GUI mode. What is genuinely missing is only the
**principal (by whom)**, the **recorded op/event**, and a **headless consumer**.

- `automation/watcher.rs::spawn_ast_watcher` — 500 ms mtime poll → tree-sitter →
  `TelemetryRequest::AstUpdate/AstDelete` over shmem (with a circuit-breaker). It is
  mtime-based, so it **also re-fires on Velocity's own writes** (the §5 loop-guard
  problem already has to be handled here).
- `editor/file_watcher.rs` — an **event-driven `notify` (`RecommendedWatcher`) watcher**,
  300 ms debounce, emits `FileChangeEvent{path, kind, timestamp}`. `notify` is
  **already vendored** (answers D3: reuse it, do not add a second dependency).
- `automation/build_runner.rs::spawn_build_watcher` — a third (build-triggered) poller.
- `velocity-ide-gui/src/main.rs::handle_ast_update` → `persist_ast_update(&mut SiteMap)`
  — external edits **already mutate the durable SiteMap** (GUI mode only; no headless
  equivalent, no `CodebaseEvent`, no actor).
- `velocity-ide-gui/src/main.rs::handle_presence_update` — cursor presence is *already*
  mediated through `MediatorArena::acquire_lock`, but under a **hard-coded
  `agent_id = "Agent_Thread"`** — precisely the missing-`Principal` gap §1 removes.

So System B is an **attribution + durability + headless-parity** job layered on
existing watchers, **not** a new watcher. Building a fourth would be the exact
duplicate-work failure this audit exists to prevent.

**Target:** human edits are just another `Op` producer — no special path.

- **Reuse the diff engine + watcher already written.** Drive off
  `editor/file_watcher.rs` (the vendored `notify` watcher, event-driven + debounced)
  for *what* changed, and reuse `orchestrator/worker/scope.rs` `snapshot_scope` /
  `detect_scoped_changes` / `collect_workspace_files` to *classify* it — do **not**
  stand up a new watcher. `automation/watcher.rs` already parses the AST → triples;
  route its output through the same op-append path instead of only the shmem→GUI path.
- **Emit an `Edit`/`Triple` op with `principal.kind=Human`** for each detected change,
  through the *same* append path as agent ops → attribution, mediation, and audit
  come for free.
- **Dependency re-flag is automatic:** the op advances `materialize`, `root()` moves,
  and `get_callers` surfaces affected dependents → the same `Semantic` contract an
  agent edit would raise. Other agents see "line N of module X changed by <Principal>,
  reason <context>" in their `read_file` Decision Trail.
- **Feedback-loop guard:** the watcher must ignore changes Velocity itself wrote
  (correlate observed file hashes against the last emitted op's `effect` digest) so
  an agent edit is never double-recorded as a human edit.

**SHIPPED (`automation/change_attribution.rs::ChangeAttributor`, driven by
`editor/file_watcher.rs`).** The attribution + durability layer above is now live on
the GUI path: `velocity_app::attribute_external_changes` folds each *change-only*
`notify` event (Created/Modified) into the `OpLogStore` as a `Text` op attributed to the
OS login user via `open_human`, keyed by a content digest (`digest_of_content`). The
producer is a constructor argument, so the executor's agent ops will ride the *same*
`record` path (`open(workspace, agent_principal)`). Dependency re-flagging is exposed
through `record_with_site_map` → `SiteMap::get_callers` (the notify seam uses `record`
and lets the Decision Trail surface dependents at read time, since the site-map write
lags the notify event). The feedback-loop guard is the durable log: `record` re-reads
the op-log and drops any `(path, digest)` already present, so an agent write recorded
first is never re-logged as a human echo. **No fourth watcher was built; the earlier
`handle_ast_update` seam was reverted because it fires on the AST watcher's initial
scan (flooding the trail).** The executor agent-attribution append is the remaining
live-wiring follow-up (its `(path, digest)` is already computed at the write seam).

---

## 6. System C — inline RBAC + audit

**Current:** `company_org::check_permission(org, role_id, …)` is a deterministic,
deny-by-default pure function, but is **not invoked on the executor path** (grep
shows it referenced only from org tests/tools + the Governance GUI rail).

**Target:** hard gate at three chokepoints, keyed by `Principal`:

1. `orchestrator/worker/scope.rs::acquire_scope_locks` — before any lock is taken.
2. `registry/team_tools.rs` `team_dispatch` and `automation/mission_runner.rs`
   `run_mission` — before a headless sub-agent runs.
3. The §5 watcher — a human may only edit within granted scope.

- **New capability for cross-team engagement:** extend `enum Capability` with
  `Collaborate` / `DispatchTo`, so "AI department may engage Manufacturing agents"
  becomes a `CredentialGrant`, and the shared transcript becomes a **writable**
  collaborative op-stream rather than a read-only view.
- **Real human authentication:** `user_id` sourced from a LAN principal token (reuse
  the peer shared-secret pairing in `peer_link.rs` + the `InstanceRegistry` bearer
  precedent) and mapped to an org `role_id`.
- **Fourth chokepoint (added by audit):** the two write-gates — `MediatorArena::acquire_lock`
  *and* `CoordinationBus::claim_file` — both admit writes and must consult the same
  `check_permission` decision, or one path silently bypasses RBAC (see §11 R-1).
- **Unattended safety:** moving `check_permission` onto the executor path must not
  silently block an unattended mission on a missing approver — preserve the existing
  explicit `unattended=true` authorization discipline in `run_rewrite_mission`.

---

## 7. Coordinated data-model changes (the "hand-in-hand" risk surface)

Done out of order these cause silent breakage, so they are specified together:

1. `CodebaseEvent` + new `Op`: add `actor/principal`, `clock`, `causal_deps`,
   `authorized_by` — **all `#[serde(default)]`** so old JSONL/`.nda` still load.
   Follow the precedent already proven this quarter: `SweepSummary.degraded`
   (`#[serde(default)] pub degraded: bool`) with a dedicated
   "written-before-the-field loads-as-default" back-compat test.
2. `EditLock`: keep `agent_id: String`, add `principal: Principal` + `node_id`.
3. Two audit stores stay two stores — **link by `request_id`/`op_id`, never
   physically merge** (protects `is_high_signal`, `file_verdict`, verdict trail).
4. `check_permission` moving onto the executor path keeps `HUMAN_OPERATOR` only as
   the fallback label.

---

## 8. Invariants that MUST hold (each maps to a required test)

- **INV-1 Staleness ≠ conflict.** `record_stale_plan` (planned vs live `root()`) and
  `record_shared_write_conflict` (mediator) remain two distinct recorded classes; the
  refactor must not collapse them.
- **INV-2 Convergence.** Any permutation of a causally-consistent op set yields the
  same `materialize()` `root()`.
- **INV-3 Idempotence.** Replaying a duplicated `op_id` changes nothing.
- **INV-4 Back-compat.** Pre-refactor `events.jsonl` and `org_approvals.nda` load and
  render unchanged.
- **INV-5 Human edits re-flag dependents.** A watcher op that changes a caller causes
  a `Semantic` conflict for holders of the callee.
- **INV-6 Deny-by-default.** An unauthorized `Principal` cannot acquire scope,
  dispatch, or land a human edit; the denial is itself an audit op.
- **INV-7 Causal audit.** Given `request_id`, all resulting edits are retrievable.

---

## 9. Phased, gated rollout

Each phase ends with `cargo fmt` / `cargo clippy --all-targets -D warnings` /
`cargo test` green (via `--manifest-path`, per the cloud-synced workspace rule) and a
fresh `velocity_mcp` build, matching the mission-runner discipline.

1. **Phase 1 — Spine.** `Principal` type; add `actor` to `CodebaseEvent`/`Op`;
   vector clock; `materialize(replay)` stub with back-compat serde. *Prerequisite for
   everything else — identity must exist first.* Tests: INV-4, INV-3.
2. **Phase 2 — Watcher attribution (System B).** *Fold the three existing watchers*
   (`file_watcher.rs`/`automation/watcher.rs`) into the op path — do not build a new
   one; emit Human ops; headless persistence parity (close R-3); loop guard (R-2).
   Tests: INV-5, INV-1. *(First user-visible win.)*
3. **Phase 3 — Convergence (System A core).** *First settle D5: unify/reconcile the
   two write-gates (MediatorArena + CoordinationBus, R-1).* Then per-scope partitions;
   set-CRDT for triples; text Tier-1 LWW with `MergePolicy` seam. Tests: INV-2, INV-3.
4. **Phase 4 — Replication.** Cross-node `Claim/Release` + clock ordering; gossip/relay
   over the **existing** peer subsystem (`/peer/*` + shared-secret pairing) + node
   registry (add only `/peer/ops`); generalize the Ollama host into per-node capability
   (§11). Shared workspace across machines lands here. Tests: INV-2 across two nodes.
5. **Phase 5 — Inline RBAC + audit (System C).** Gate at the three chokepoints;
   `Collaborate`/`DispatchTo` grants; human authN; causal op-linking.
   Tests: INV-6, INV-7.
6. **Phase 6 — GUI.** Nodes rail shows per-principal locks/conflicts; Governance rail
   renders the cross-linked audit timeline; cross-team engagement UI.

Dependency graph: **1 → {2, 3, 5}; 3 → 4; 5 leans on 1 (identity) and 3 (single log).**

---

## 10. Open decisions (require user input before their phase begins)

- **D1 Text merge depth:** per-file LWW (cheap, adequate when scopes rarely overlap)
  vs a sequence CRDT (true human+agent simultaneous co-editing). Affects Phase 3.
  *(Tier-1 LWW shipped behind `merge::MergePolicy`; D1 now selects which trait impl
  a partition uses — the seam is built and proven swappable, so D1 is a later,
  isolated choice, not a blocker.)*
- **D2 Transport — RESOLVED by audit:** reuse the existing peer subsystem
  (`peer_server.rs` `/peer/*` + `peer_link.rs` shared-secret pairing +
  `peer_bridge.rs`⇄`CoordinationBus`). The only new surface is a merge-ordered
  `/peer/ops` gossip endpoint; do **not** invent a parallel sync channel.
- **D3 Watcher backend — RESOLVED by audit:** reuse the vendored `notify` watcher
  (`editor/file_watcher.rs`); fold `automation/watcher.rs` AST output into the op path.
  No new dependency.
- **D4 Partition granularity:** per `scope_pattern` vs per-module vs whole-workspace.
  Affects Phase 3 merge cost. *(Mechanism shipped — `partition::partition_by` takes
  the key fn from the caller, so this is now purely a policy/perf choice, provably
  independent of the converged result; only the default key function need be chosen.)*
- **D5 Write-gate unification — RESOLVED (decision: UNIFY).** Collapse
  `MediatorArena` + `CoordinationBus` file-claims into **one lock table** so the two
  write paths can no longer admit overlapping writes without seeing each other (R-1).
  Staged to avoid breaking live paths:
  - **Stage 1 (DONE):** one authoritative type — `automation/write_gate.rs::WriteGate`.
    `MediatorArena` delegates to it (API + tests unchanged); a whole-file claim == the
    orchestrator's `WHOLE_SCOPE` range lock, single identity fn `canonicalize_scope_path`.
    Capability preserved: range/`DirectLine`/`ScopeOverlap`/`Semantic`/TTL all live on in
    `WriteGate`; a claim passes `None` for the map so it is *never* newly blocked by a
    semantic coupling it was never subject to (behavior-preserving for the executor).
  - **Stage 2 (DONE):** `CoordinationBus` drops its private `file_locks` map and forwards
    `claim_file`/`release_file`/`is_locked_by_other`/`locked_files`/`lock_details` to a
    shared `Arc<WriteGate>` (broadcasts kept; progress/help stay on the bus); the app's one
    arena is threaded into `executor::thread::run_agent_thread(..., mediator)` which builds
    the bus via `CoordinationBus::with_gate(mediator.gate_arc())`. Identity unified on
    `canonicalize_scope_path`. Verified: `shared_gate_makes_bus_claims_visible_to_mediator`,
    both binaries `cargo check` clean, suite **2965 passed / 0 failed**.

---

## 11. Existing-Systems Reconciliation (audit before build)

Per the standing rule — *check for existing systems before any segment so we build
on them instead of breaking duplicates* — every planned component was grepped
against the tree. Verdict per component:

| Planned component | Existing in tree? | Action | Concrete anchor |
|---|---|---|---|
| **Spine-1 `Principal`** | ✗ no per-human/per-node identity; only `agent_id="task-{id}"`, `"Agent_Thread"`, `HUMAN_OPERATOR` sentinel | **BUILD** (new) — then *replace* the 3 hard-coded identities | `scope.rs`, `main.rs:264`, `company_org.rs` |
| **Spine-2 op-log / vector clock / CRDT / `materialize`** | ✗ none (grep: no `vector.?clock`/`crdt`/`replicat`/`merge` in `velocity-mcp`) | **BUILD** (new) — demote existing `root()` to derived checkpoint | `event_store.rs`, `site_map/store.rs` |
| **Spine-3 causal audit link** | ~ two stores exist but unlinked | **EXTEND** — add `authorized_by`/`op_id` cross-refs, do not merge stores | `event_store.rs`, `company_org.rs` |
| **System B watcher (detect change)** | ✓✓ THREE engines + `notify` vendored + GUI already persists external AST edits | **REUSE** — attribute + record op + headless parity; build nothing new | `automation/watcher.rs`, `editor/file_watcher.rs`, `build_runner.rs`, `main.rs::handle_ast_update` |
| **System A intra-node locking** | ✓ `MediatorArena` **and** `CoordinationBus` claims | **UNIFY (D5)** into one `write_gate::WriteGate` table; `MediatorArena` delegates (Stage 1 done), bus to forward + share instance (Stage 2) | `write_gate.rs`, `mediator.rs`, `coordination.rs` |
| **System A cross-node transport** | ✓ full peer subsystem (`/peer/*`, pairing, file xfer, bridge→bus) | **REUSE** — add only `/peer/ops` gossip | `peer_server.rs`, `peer_link.rs`, `peer_bridge.rs`, `peer_robust.rs` |
| **Node registry + liveness** | ✓ `InstanceRegistry` + `InstanceStatus` health-ping + roles/platform | **REUSE** for `node_id` + Ollama routing | `agent/instances.rs` |
| **System C RBAC engine** | ✓ `check_permission` (deny-by-default, pure) — but off the executor/watcher path | **EXTEND + WIRE** onto chokepoints (incl. both write-gates) | `company_org.rs` |
| **Credential guard / secrets** | ✓ `credential_guard::CredentialScope`, `ScopedSecret`, `CredentialBlocked` | **REUSE** for LAN principal tokens | `company_org.rs` |

### Conflict / duplication risks found

- **R-1 — two independent write-gates.** `MediatorArena` (line-range, `ConflictKind`,
  1800 s TTL, in-process) and `CoordinationBus::claim_file/release_file` (whole-file,
  `FileLock{agent_id,claimed_at}`, wired into the live loop) both admit writes to the
  same files but **do not consult each other**. A `Principal`+`check_permission` shim
  must front *both*, and Phase 3/4 convergence must treat claims and locks as one op
  stream, or an agent can bypass mediation via `claim_file`. Decide D5 first.
- **R-2 — watcher loop hazard is already live.** `automation/watcher.rs` is
  mtime-based, so it re-fires on Velocity's own writes today; the §5 "ignore our own
  writes" guard is a **pre-existing requirement**, not a new-feature nicety. Any new
  op-append on that path would double-record agent edits unless the guard ships with it.
- **R-3 — GUI-only persistence.** `handle_ast_update`→`persist_ast_update` mutates
  the durable SiteMap **only** when the GUI TelemetryServer is up; headless runs have
  no consumer, so external edits are silently dropped there. System B's "headless
  parity" is closing this gap, not adding a watcher.
- **R-4 — do not add a 4th watcher or a 2nd transport or a 3rd RBAC.** The three
  tempting new-builds (notify watcher, sync channel, permission gate) each already
  exist; the spec now mandates reuse.

### Ollama on an offline LAN (the routing sub-question, answered by reuse)

The "ping each node, use whichever one has a working API" model does **not** need a
gateway PC. `InstanceRegistry` already probes `/peer/health` → `InstanceStatus` and
`build_fallback_chain` already terminates at `LocalOllama` (`localhost:11434`).
Generalize that hard-coded host to a **per-node ollama capability** advertised in
`PeerCapability`/`InstanceRecord`, and let a node with a reachable Ollama expose it
over `/peer/*` so peers proxy through the healthy node instead of every call funneling
to one box. This is an extension of existing liveness + peer transport, slotted at
Phase 4 (no new discovery protocol).

---

## 12. Grounding index (files/symbols this spec depends on)

- `automation/mediator.rs` — `MediatorArena`, `ConflictKind::{DirectLine,ScopeOverlap,Semantic}`,
  `resolve_conflict`, in-process locks.
- `orchestrator/worker/scope.rs` — `acquire_scope_locks`, `snapshot_scope`,
  `detect_scoped_changes`, `collect_workspace_files`.
- `registry/event_store.rs` — `CodebaseEvent`, `record_with_metadata`,
  `record_shared_write_conflict`, `record_stale_plan`, `record_task_outcome`,
  `is_high_signal`, `APPEND_LOCK`.
- `editor/company_org.rs` — `check_permission`, `enum Capability`, `CredentialGrant`,
  `ActionRequest`, `approve_and_resume`, `HUMAN_OPERATOR`.
- `velocity-ide/src/site_map/store.rs` — `VcTriple`, `put_file_snapshot`,
  `remove_file_snapshot`, `get_callers`, `recompute_root`, `root`, `weight_root`.
- `velocity-mcp/docs/VC.md` — Merkle-AST VC background; determinism rationale.
- `registry/team_tools.rs`, `automation/mission_runner.rs` — `team_dispatch`,
  `run_mission`, `unattended` authorization, `scope_patterns`, fallback chain.
- `agent/peer_server.rs`, `agent/peer_link.rs`, `agent/peer_bridge.rs`,
  `agent/peer_robust.rs` — `/peer/*` HTTP (port 9191), `PeerManager`/`PeerIdentity`,
  shared-secret pairing, SHA-256 file xfer, `PeerMessage`⇄`AgentBroadcast` bridge.
- `agent/coordination.rs` — `CoordinationBus`, `claim_file`/`release_file`,
  `FileClaimed`/`FileReleased`, `AgentBroadcast` (2nd write-gate; R-1).
- `agent/instances.rs` — `InstanceRegistry`, `InstanceRecord`, `InstanceRole`,
  `InstanceStatus`, `/peer/health` ping, `HEALTH_STALE_SECS` (liveness routing).
- `automation/watcher.rs`, `editor/file_watcher.rs`, `automation/build_runner.rs` —
  the three existing change-detection engines; `notify` (`RecommendedWatcher`) already
  vendored (R-2/R-4).
- `velocity-ide-gui/src/main.rs` — `handle_ast_update`/`persist_ast_update` (durable
  SiteMap from external edits, GUI-only; R-3), `handle_presence_update`
  (`agent_id="Agent_Thread"` hard-coded; the missing-`Principal` gap §1 removes).
