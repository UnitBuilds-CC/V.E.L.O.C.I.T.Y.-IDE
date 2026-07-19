# V.E.L.O.C.I.T.Y. IDE Architecture

V.E.L.O.C.I.T.Y. is a hybrid AI-native IDE written in Rust. Its core responsibilities are: editing code, running an agentic reasoning loop, compiling user projects, and serializing artifacts into the `.nda` binary format.

For agent-facing NDA authoring guidance, see `docs/NDA_FORMAT.md`.
For NDA-vs-JSON boundary decisions, see `docs/NDA_BOUNDARIES.md`.

## Crate layout

- `main.rs` — binary entry point. Parses CLI flags, starts eframe + agent thread.
- `editor/` — UI layer.
  - `app.rs` — root `eframe::App`. Owns docking layout, command palette, keymap, status bar.
  - `theme.rs` — custom egui visuals, color palette, font setup.
  - `buffer.rs` — editor buffer model (Rope + String cache).
  - `code_editor.rs` — line-numbered, syntax-highlighted editor widget.
- `agent.rs` — background agent thread. Streams from Workers AI, dispatches tools, runs `cargo check`, self-corrects on compile errors.
- `registry.rs` — tool registry exposed to the agent (read_file, write_file, list_dir, execute_nda, etc.).
- `benchmark.rs` — kernel / GEMV / Vulkan benchmarks (legacy, heavy warnings).
- `compiler/` — compiler and tokenizer modules.
  - `driver.rs` — Vulkan compute driver (legacy, heavy warnings).
  - `tokenizer.rs` — byte-level tokenizer + NDA embedding table.
  - `jit.rs` / `shaders.rs` / `nmcp_binary.rs` — experimental codegen.
- `protocol/` — JSON-RPC / NMCP wire protocols.
- `ipc/` — shared-memory transport for NMCP server mode.
- `orchestrator/` *(new)* — meta-agent control plane for parallel sub-agent tasks.

## Thread boundaries

| Thread | Owner | Sends to UI | Receives from UI |
|--------|-------|-------------|------------------|
| UI (main) | `VelocityApp` | `UiToAgentMessage` | `AgentToUiMessage` |
| Agent | `run_agent_thread` | `AgentToUiMessage` | `UiToAgentMessage` |
| Orchestrator UI integration | `OrchestratorPanel` | same as above | same as above |

## Agent loop data flow

1. User prompt → `UiToAgentMessage::UserPrompt` → agent thread.
2. Agent builds request with tool definitions from `registry::get_tools()`.
3. Streaming response fills `assistant_content` and `accumulated_tools`.
4. Tool calls request approval via `RequestToolApproval`.
5. On approval, `registry::call_tool()` runs synchronously on the tool registry.
6. After each turn, `run_compilation_check()` runs `cargo check`;
   errors are injected as a new user message and `run_agent_reasoning_loop` recurses.

## IDE UI state

- `DockState<Tab>` from `egui_dock` is the single source of truth for open tabs.
- `active_tab` is updated by `egui_dock` focus callbacks, not maintained separately.
- `command_output` and `chat_history` are capped to a max size to avoid UI lag.

## Orchestrator (parallel agent execution)

See `src/orchestrator/mod.rs` plus the Mission Control/editor integration. The active orchestration stack now includes:

- `TaskGraph` — DAG of work packages.
- `Scheduler` — topological execution and phase planning.
- `Worker` — live provider-backed routed worker execution with scoped locking, cancellation, event streaming, and structured results.
- `Reconciler` — diff-based collision and scope-violation detection.
- `Validator` — per-worktree validation hooks such as `cargo check` / `cargo test`.
- `OrchestratorPanel` / Mission Control — operator-facing planning, launch, retry, reset, stop, note routing, and live task supervision.

Current integration is still local-workspace-first, but the runtime is no longer a placeholder: routed sub-agents execute through the real provider-backed worker path used by the active IDE.

## Known constraints

- `Cargo.toml` edits are blocked by workspace security checks, so dependency changes must be applied manually.
- Legacy modules (`benchmark.rs`, `compiler/driver.rs`, etc.) carry many warnings; they are marked `#[allow(...)]` to keep CI noise low while we refactor incrementally.
