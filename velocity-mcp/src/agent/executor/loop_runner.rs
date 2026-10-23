use super::super::checkpoint::CheckpointManager;
use super::super::coordination::CoordinationBus;
use super::super::memory_store::PersistentMemory;
use super::super::models::*;
use super::super::nda::*;
use super::super::provider::*;
use super::super::self_improve::ImprovementEngine;
use super::context_budget::{compress_history_to_window, declared_window};
use super::router_client::{
    self, AssignmentRequest, AssignmentStatus, ExecutionMode, ExecutionTier,
};
use super::thread::{
    apply_headless_control_messages, is_test_failure_report, run_post_write_check,
    run_written_tests, TestVerdict,
};
use super::utils::{
    build_request, estimate_tokens, ollama_model_reasons, parse_bare_json_tool_calls,
    parse_claude_inline_tool_calls, parse_dsml_tool_calls, sanitize_chat_token, send_usage_update,
    LOCAL_OLLAMA_NUM_CTX,
};
use crate::registry;
use crate::safety::SafeMutex;
use crate::usage::{
    AzureOpenAiAccount, CloudflareAccount, LocalOllamaAccount, OpenRouterAccount, UsageTracker,
    WorkspaceRouterSettings,
};
use crossbeam_channel::{Receiver, Sender};
use serde_json::{json, Value};
use std::io::BufRead;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// How many times a turn may be re-prompted after it ended with an announced
/// action (or unexecutable tool markup) and no tool call. Past this the run
/// stops as *blocked* — never as a completed final answer. Raised from 2 now
/// that an artifact turn in write-force mode is re-issued from a reconstructed
/// clean context (see the `pruned_history` block) rather than the model's own
/// accumulated no-action narration: each retry is a genuine fresh attempt, so
/// the guard only needs to bound a truly stuck model, not a self-imitation
/// spiral the context used to feed.
const MAX_AUTO_CONTINUATIONS: u32 = 5;

/// Upper bound on the "you owe us files" directive injected when an artifact
/// task tries to end with a final answer but has written nothing. Raised from
/// 2 because each of those retries now runs on a reconstructed clean write
/// context (see the `pruned_history` block) at write-force temperature, so it
/// is a genuine independent attempt rather than the model shouting into the
/// same bloated history. Still bounded: a model that cannot comply at all must
/// eventually stop honestly as `Blocked` so the caller can retry or fall back.
const MAX_ARTIFACT_NUDGES: u32 = 4;

/// Upper bound on the "your writes landed outside the file the task named"
/// directive. Budgeted apart from [`MAX_ARTIFACT_NUDGES`] because they steer two
/// different failures: a model that has been chased for writing nothing can
/// still be the one that writes everything beside the deliverable, and sharing a
/// cap let the first mistake spend the turns owed to the second.
const MAX_OFF_TARGET_REFUSALS: u32 = 3;

/// Upper bound on compiler-feedback repair turns. When the post-run build check
/// finds errors, the diagnostics are fed back to the model so it can fix them
/// with a fresh `edit_file` (or `write_file` for a whole-file change) instead of
/// the run stopping on the first failed compile. This is what turns the local
/// MoE's "wrote a real port but with 2-3 type/borrow slips" into a passing
/// deliverable; without it the loop breaks `Blocked` immediately and the
/// dispatch falls through to a fallback provider.
/// Bounded because a model that cannot fix its own type errors must stop
/// honestly rather than loop until the wall-clock budget is spent.
const MAX_COMPILE_REPAIR_TURNS: u32 = 3;

/// Repairs the post-run TEST gate may ask for, counted separately from
/// [`MAX_COMPILE_REPAIR_TURNS`]. Getting a crate to compile and getting its
/// assertions right are different jobs — see [`repair_budget`]. Two is enough
/// for the measured failure shape (a wrong literal, then a second guess) without
/// letting a run that cannot count tokens spend the whole wall clock.
const MAX_TEST_REPAIR_TURNS: u32 = 2;

/// The tools that change files and therefore need a workspace lock, a changelog
/// entry and the "this run produced an artifact" credit. `apply_diff` used to be
/// listed here without ever being registered, which quietly made those branches
/// dead and — worse — the prompt text naming it taught models to call a tool
/// that cannot exist.
const FILE_EDITING_TOOLS: [&str; 2] = ["write_file", "edit_file"];

/// The multimodal generation family (see tool_definitions/generation.rs). Offered
/// to a code-writing sub-agent it is a pure hallucination surface — none of these
/// belong in a porting or file-production task — so an artifact dispatch hides them
/// from the model's toolset rather than waiting for the model to behave.
const MEDIA_GENERATION_TOOLS: &[&str] = &[
    "generate_image",
    "generate_video",
    "generate_media",
    "list_generation_models",
    "register_generation_model",
];

/// Append one streamed content token to `assistant_content`, then flush only
/// the *safe* (non-markup) prefix to the UI. Inline tool markup
/// (`<tool_call>`, `[Calling tool`) and its closing markers are suppressed so
/// a broken/partial call is never surfaced as prose. Shared by the OpenAI SSE
/// path (`delta.content`) and the native Ollama `/api/chat` path
/// (`message.content`), which both deliver content as per-token deltas.
fn absorb_content_delta(
    tok: &str,
    assistant_content: &mut String,
    streamed_len: &mut usize,
    suppressing: &mut bool,
    ui_tx: &Sender<AgentToUiMessage>,
) {
    assistant_content.push_str(tok);
    let ac = &**assistant_content;
    loop {
        if *suppressing {
            let search = &ac[*streamed_len..];
            let end = search
                .find("</function>")
                .map(|p| (p, "</function>".len()))
                .or_else(|| {
                    search
                        .find("</tool_call>")
                        .map(|p| (p, "</tool_call>".len()))
                })
                .or_else(|| search.find(']').map(|p| (p, 1)));
            if let Some((p, mlen)) = end {
                *streamed_len += p + mlen;
                *suppressing = false;
            } else {
                break;
            }
        } else {
            let search = &ac[*streamed_len..];
            let tc1 = search.find("<tool_call>").map(|p| (p, false));
            let tc2 = search.find("[Calling tool").map(|p| (p, true));
            let detected = match (tc1, tc2) {
                (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            };
            if let Some((p, _is_bracket)) = detected {
                let safe = &search[..p];
                if !safe.is_empty() {
                    ui_tx
                        .send(AgentToUiMessage::OutputToken(sanitize_chat_token(safe)))
                        .ok();
                }
                *streamed_len += p;
                *suppressing = true;
            } else {
                let total = ac.len();
                let mut safe_end = total.saturating_sub(14);
                while safe_end > *streamed_len && !ac.is_char_boundary(safe_end) {
                    safe_end -= 1;
                }
                if safe_end > *streamed_len {
                    let chunk = &ac[*streamed_len..safe_end];
                    ui_tx
                        .send(AgentToUiMessage::OutputToken(sanitize_chat_token(chunk)))
                        .ok();
                    *streamed_len = safe_end;
                }
                break;
            }
        }
    }
}

/// Keep only the tool calls that arrived in full on a stream abandoned at the
/// wall-clock deadline.
///
/// A call cut off mid-argument-list has JSON that cannot parse, and executing
/// it means writing a truncated file — strictly worse than stopping, since the
/// partial artifact is what a later gate or fallback attempt then has to
/// untangle. Arguments that close cleanly can only have been emitted complete,
/// so those calls are real work the model finished before the clock ran out and
/// are worth landing.
fn salvage_complete_calls(calls: Vec<ToolCallAccumulator>) -> Vec<ToolCallAccumulator> {
    calls
        .into_iter()
        .filter(|c| serde_json::from_str::<Value>(&c.arguments).is_ok())
        .collect()
}

pub fn run_agent_reasoning_loop(
    workspace_root: &PathBuf,
    accounts: &[CloudflareAccount],
    or_accounts: &[OpenRouterAccount],
    azure_accounts: &[AzureOpenAiAccount],
    ollama_accounts: &[LocalOllamaAccount],
    model: &str,
    profile: &ModelInfo,
    provider: AiProvider,
    thinking: bool,
    message_history: &mut Vec<ChatMessage>,
    usage_tracker: &mut UsageTracker,
    ui_rx: &Receiver<UiToAgentMessage>,
    cancel_rx: Option<&Receiver<UiToAgentMessage>>,
    progress: Option<&Arc<Mutex<HeadlessSubAgentProgress>>>,
    ui_tx: &Sender<AgentToUiMessage>,
    deferred_messages: &mut Vec<UiToAgentMessage>,
    coordination_bus: &CoordinationBus,
    router_settings: Option<&WorkspaceRouterSettings>,
    max_loops_override: Option<usize>,
    deadline: Option<std::time::Instant>,
    requires_artifact: bool,
) -> AgentLoopOutcome {
    let mut sitemap_needed = false;
    let mut loop_count: usize = 0;
    let loop_started = std::time::Instant::now();
    // Captured up-front so a timeout reports the budget it was *given*, not
    // whatever happened to be left when it expired.
    let budget_secs = deadline
        .map(|at| at.saturating_duration_since(loop_started).as_secs())
        .unwrap_or(0);
    let max_loops: usize = max_loops_override.unwrap_or(15);
    // How this run ends, for callers that cannot read a transcript. Starts as
    // the turn-budget case because that is what falling out of the `while`
    // below means; every earlier exit overwrites it with its own reason.
    let mut outcome = AgentLoopOutcome::Blocked {
        reason: format!(
            "turn budget of {} exhausted without a final answer",
            max_loops
        ),
    };
    let mut current_provider = provider;
    let mut current_model = model.to_string();
    let mut current_profile = profile.clone();
    let mut current_thinking = thinking && current_profile.supports_thinking;
    // A local reasoning model (e.g. qwen3.6:35b-a3b) with a few billion active
    // parameters reads the injected cross-session context literally: a recalled
    // `read_file` excerpt in the system prompt is mistaken for "files the user
    // just shared", and the model drifts off the actual instruction into
    // reviewing it. Measured on the LiteLLM crucible — the model hallucinated
    // having "read __init__.py" straight out of the recall block and never
    // wrote the deliverable. For these models we keep the prompt lean and
    // on-task instead of padding it with recall that derails small contexts.
    // Key off the SAME name-based detector the think value uses, not
    // `profile.supports_thinking`: a local qwen3.x build is not flagged thinking
    // in its catalog profile yet still emits a reasoning trace and still needs a
    // lean prompt. Driving this off the profile let qwen3.6:35b-a3b keep memory
    // recall + accumulated reasoning in history, ballooning the prompt to 12k+
    // tokens that re-prefills at ~100 tok/s on the CPU-offloaded buildbox MoE.
    let lean_context_for_reasoning_model =
        current_provider == AiProvider::LocalOllama && ollama_model_reasons(&current_model);
    let mut fallback_attempts: usize = 0;
    // Budget for same-provider retries on hard transport failures; bounded so
    // a permanently dead provider still terminates the run (as "blocked").
    let mut hard_fail_retries: u32 = 0;
    // Budget for auto-continuations when the model announces an action but
    // returns no tool call ("Let me fix X:" → turn ends). Bounded so a model
    // that genuinely loops on announcements still terminates.
    let mut auto_continuations: u32 = 0;
    // Wall-clock seconds spent on turns that produced no tool call. See
    // `wasted_time_ends_the_run`.
    let mut wasted_turn_secs: u64 = 0;
    // Write-pressure tracking: consecutive turns that ran a tool batch without
    // touching a single file. The agent_eval re-run showed flash-class models
    // spending the whole turn budget on read_file/list_dir/cargo-test auditing
    // and never transitioning to writing. When the streak passes the threshold,
    // inject a directive forcing the switch to writing (or, for a genuinely
    // read-only mission, delivery of the final report instead of more reading).
    let mut turns_since_write: u32 = 0;
    let mut write_pressure_nudges: u32 = 0;
    // Artifact-deliverable honesty. For a task whose promised deliverable is
    // files, track whether the loop ever produced one (a successful
    // write_file/edit_file). If it reaches a final answer having written
    // nothing, the steering below forces another attempt and then an honest
    // stop — this is the exact false `succeeded: true` the LiteLLM 14B phase-1
    // run produced without ever touching the repository.
    let mut produced_artifact = false;
    let mut artifact_nudges: u32 = 0;
    // Steering turns spent on writes that landed beside the named deliverable.
    let mut off_target_refusals: u32 = 0;
    let mut compile_repair_attempts: u32 = 0;
    // Spendable repairs the TEST gate has left, counted apart from the build's.
    let mut test_repair_attempts: u32 = 0;
    // Attempts given back because the armed turn moved no file. Bounded so a
    // model that refuses every directive cannot be re-armed forever.
    let mut refunded_repair_attempts: u32 = 0;
    // How many times this run has refused each distinct tool-call payload. The
    // error-signature map below cannot see this class of repeat: a call the tool
    // itself refused never reached a compiler, so no diagnostic exists to
    // compare. `lr7b_v12` spent turns 2-5 replaying one `edit_file` whose
    // `oldString` matched two places, and each replay earned the same two-site
    // explanation again — four refusals, four identical answers, no file moved.
    let mut refused_call_counts: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();
    // One entry per compile message a gate handed back: the identity of every
    // error in it. A repair that turns E0053 into E0594 and back produces
    // diagnostics that differ every time, so `diagnostics_changed` alone reads
    // that circle as progress and spends the budget on it — which is exactly how
    // the gateprobe2 run lost three repairs on one `&self`/`&mut self` flip.
    let mut repair_error_history: Vec<Vec<String>> = Vec::new();
    // When a compile check fails, the repair directive is just appended text and
    // the next turn keeps full tools + normal sampling — so the model answers
    // with prose and no `write_file`, the unchanged file re-compiles to the
    // *identical* errors, and a repair attempt is burned with zero progress
    // (measured on the qwen3.6 convert run: 3 attempts, 4 byte-identical error
    // sets). A pending repair instead forces the next turn through the
    // write-only reconstruction, which is the only context that reliably made
    // this MoE actually emit the call.
    let mut compile_repair_pending = false;
    // The directive an armed repair owes the next turn — diagnostics and, when
    // the report is the other face of an error already shown, the note that names
    // the pendulum. It holds the finished text, not just the errors, because a
    // repair turn rebuilds a lean context from scratch (the only shape this MoE
    // reliably acts on) and that rebuild deletes the history message the repair
    // pushed. Keeping the advice in the same armed state is what stops the
    // rebuilt turn from re-issuing the losing move it was just told about.
    let mut pending_repair_directive: Option<String> = None;
    // Which file the failing compilation belongs to, recovered from the rustc
    // diagnostics. A repair turn may rewrite only this file, which is what makes
    // treating a fenced code block as its full content safe.
    let mut compile_repair_target: Option<String> = None;
    // The workspace-relative path of the most recent successful write. This is
    // the file a compile repair should rewrite, and it is authoritative: cargo
    // reports diagnostics relative to its own manifest directory, which is not
    // necessarily the workspace root (a `rust_port/` subcrate reports
    // `src/x.rs`, but `write_file` needs `rust_port/src/x.rs`).
    let mut last_write_path: Option<String> = None;
    // Every Rust file this run put on disk. A batch can write two modules, and
    // a gate that only looks at the last one blesses a crate whose test code
    // broke in the other.
    let mut run_rust_writes: Vec<String> = Vec::new();
    // The diagnostics the write-side gate last saw, so it can tell a genuine new
    // breakage from the same one the model was already handed.
    let mut write_gate_diagnostics: Option<String> = None;
    // How many gate reports have been shown to the model WITHOUT arming a repair
    // turn. The repair budget bounds loop-initiated interrupts; a run whose budget
    // is spent should not be shouted at every turn either.
    let mut unarmed_repair_notes: u32 = 0;
    // Set once the post-run TEST gate has rejected this artifact (build clean,
    // assertions red). From then on the crate is graded on its tests, so every
    // repair write has to be test-checked the turn it lands — see the write-side
    // gate below.
    let mut test_gate_engaged = false;
    // MoA router is attempted once per mission: it failed, timed out, or was
    // unavailable → don't pay the health check (and risk the 120 s sync
    // block) again on a later turn. Router interception of mid-mission
    // turns was also producing text answers where tool calls were due.
    let mut router_gave_up = false;

    // Phase 2: Workspace checkpointing for safe, reversible tool operations
    let mut checkpoint_mgr = CheckpointManager::new(workspace_root);
    // Checkpoint created before the most recent file-modifying batch, so a
    // fully-failed batch can be rolled back.
    let mut last_checkpoint_id: Option<usize> = None;

    // Phase 3: Persistent memory for cross-session learning
    let mut memory = PersistentMemory::open(workspace_root);

    // Phase 4: Self-improvement engine — analyzes failures and refines prompts
    let mut improve_engine = ImprovementEngine::new(&memory);

    // Inject previously-learned directives into system prompt at session start
    let learned_directives = ImprovementEngine::recall_directives(&memory, 5);
    if !learned_directives.is_empty() && !lean_context_for_reasoning_model {
        if let Some(sys_msg) = message_history.iter_mut().find(|m| m.role == "system") {
            sys_msg
                .content
                .push_str("\n\n## Previously Learned Patterns\n");
            for d in &learned_directives {
                sys_msg.content.push_str(&format!("- {}\n", d));
            }
        }
    }

    // T2b: LSP-gated writes — track files written this session to prevent
    // repeated overwrites when build diagnostics report errors.
    let lsp_written_files: Arc<Mutex<std::collections::HashSet<PathBuf>>> =
        Arc::new(Mutex::new(std::collections::HashSet::new()));

    // System B: agent writes are attributed into the durable op-log through the
    // *same* append path the human watcher uses (see automation/change_attribution),
    // keyed by the content digest read back from disk. Recording the agent's write
    // first means the AST watcher's later echo of the identical bytes is deduped —
    // the op-log is the feedback-loop guard, so an agent edit is never double-logged
    // as a human one. Shared (`Arc<Mutex<_>>`) so parallel tool threads in a
    // batch advance one node clock rather than each minting colliding `op_id`s.
    let agent_attribution = Arc::new(Mutex::new(crate::automation::ChangeAttributor::open(
        workspace_root,
        crate::automation::Principal::agent(crate::automation::NodeId::local(), 0, None, None),
    )));

    // System C (spec §6.4): org RBAC consulted inline at the executor's write
    // admission, so a write is scoped by the *role* behind the actor rather than
    // trusted because a bare `"primary"` string holds the edit lock. The local
    // interactive agent writes on behalf of the OS-login user — the very identity
    // the GUI human watcher attributes manual edits to (struct_def) — so its
    // `Write` grant is what scopes the edit. Governance is opt-in: an ungoverned
    // workspace yields `None` and the gate is never consulted (unattended missions
    // are never blocked on a missing binding), while a governed one resolves the
    // human's role and lets each write thread call `require_write` — a denial is
    // audited to the durable trail (INV-6), an allow mints nothing.
    let write_governor: Option<(
        Arc<crate::automation::Governor>,
        crate::automation::Principal,
    )> = {
        let governor = crate::automation::Governor::open(workspace_root);
        if governor.governed() {
            let os_user = std::env::var("USERNAME")
                .or_else(|_| std::env::var("USER"))
                .unwrap_or_else(|_| "operator".to_string());
            let principal =
                governor.principal_for_human(&os_user, crate::automation::NodeId::local());
            Some((Arc::new(governor), principal))
        } else {
            None
        }
    };

    let registered_tools = registry::get_tools();
    // A software deliverable never involves generating images/video or editing the
    // generation-model registry, yet the whole catalog is otherwise offered to every
    // dispatched model — exactly the surface a small local model grabs when it drifts
    // off-task (observed: qwen2.5-coder calling generate_image / register_generation_model
    // for happyhorse and wan2.7 instead of writing the mapped file). For an artifact
    // dispatch we hide the generation family from BOTH the offered schema and the
    // bare-JSON recognition set, so the model can't call — or even have a prose JSON
    // call interpreted as — a tool it can't see. Report tasks and IDE turns keep the
    // full set; a media-capable IDE agent legitimately needs these.
    //
    // Additionally, artifact tasks suffer from tool overload: a 3B-parameter MoE model
    // presented with 337 tools (browser control, Windows automation, org management, etc.)
    // becomes confused and repeatedly calls list_dir instead of write_file. For artifact
    // dispatches we narrow to only the essential file-manipulation tools so the model
    // can focus on producing the deliverable.
    let artifact_essential_tools = [
        "list_dir",
        "read_file",
        "write_file",
        "edit_file",
        "grep_search",
        "glob",
    ];
    let cf_tools: Vec<Value> = registered_tools
        .iter()
        .filter(|t| {
            if requires_artifact {
                // Artifact task: only expose essential file tools
                artifact_essential_tools.contains(&t.name.as_str())
            } else {
                // Non-artifact task: hide only generation tools
                !hides_generation_tool(false, &t.name)
            }
        })
        .map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema
                }
            })
        })
        .collect();

    // The single-tool subset exposed once the model has been steered toward
    // the deliverable but still hasn't written it. See the per-turn selection
    // below for why withholding exploration tools is the difference between a
    // small reasoning model stalling and actually emitting the write.
    let write_only_tools: Vec<Value> = cf_tools
        .iter()
        .filter(|t| {
            t["function"]["name"]
                .as_str()
                .is_some_and(|n| FILE_EDITING_TOOLS.contains(&n))
        })
        .cloned()
        .collect();

    // A "fix this file" dispatch has nothing to create. Locking it to
    // `write_file` alone would force the whole ~25 KB port back through the
    // model's output budget — the exact failure `edit_file` was added to remove
    // — so a task that names a file already on disk keeps both editors.
    // Detecting it from the task text is safe because the result only chooses
    // which schemas to advertise; it never decides where a file is written.
    let task_text = message_history
        .iter()
        .find(|m| m.role == "user" && !m.content.starts_with("[System notice]"))
        .map(|m| m.content.clone())
        .unwrap_or_default();
    let named_target =
        existing_file_named_by_task(workspace_root, &task_text).filter(|_| requires_artifact);
    let preexisting_target = named_target.is_some();

    // The one exploration schema a locked turn keeps, and only when there is
    // something to read: a "fix this file" dispatch asks for edits whose
    // `oldString` is copied verbatim from a file the model is not otherwise
    // allowed to open. `lr7b_v15` is what that costs — refused a whole-file
    // write, it answered with 54 edits whose `oldString` and `newString` were the
    // same identifier, because the item names quoted in that refusal were the
    // only text from the file it had ever seen. Execution stays one path wide
    // (`read_of_named_target`); this only stops advertising a tool the turn
    // would have honoured.
    let locked_read_tools: Vec<Value> = if preexisting_target {
        cf_tools
            .iter()
            .filter(|t| t["function"]["name"].as_str() == Some("read_file"))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    // Does this run owe Rust code? Only a task that says so in its own text —
    // the safe default for every other artifact task is that any file it writes
    // is the deliverable, because the loop has no general idea what shape that
    // deliverable should be.
    let artifact_is_rust = requires_artifact && task_text.contains(".rs");

    // The items the deliverable declared when the run opened. A repair is graded
    // by asking rustc and the crate's tests whether they are satisfied — and both
    // go quiet when the code they were complaining about is deleted. pendulum1 is
    // the measurement: the dispatch handed the agent a port whose `impl ... for
    // Snapshot` contradicted its own trait, and the write that "fixed" it removed
    // the entire module (3 references left, 23,880 -> 23,440 bytes) and was
    // declared green. Recording the names the file arrived with is what makes a
    // deletion distinguishable from a repair.
    let mut artifact_surface = ArtifactSurface::default();
    if let Some(rel) = named_target.as_ref().filter(|_| artifact_is_rust) {
        if let Ok(text) = std::fs::read_to_string(workspace_root.join(rel)) {
            artifact_surface = ArtifactSurface {
                target: Some(rel.clone()),
                names: rust_item_names(&text),
                refusals: 0,
            };
        }
    }

    // Tool names the loop will actually accept. The bare-JSON tool-call recovery
    // below only treats a `{"name": ...}` object as a call when its name is one
    // of these, so a model's legitimate JSON final answer is never hijacked.
    let known_tool_names: std::collections::HashSet<String> = registered_tools
        .iter()
        .map(|t| t.name.clone())
        .filter(|n| {
            if requires_artifact {
                artifact_essential_tools.contains(&n.as_str())
            } else {
                !hides_generation_tool(false, n)
            }
        })
        .collect();

    // A "finish this file" dispatch starts blind: the only channel that shows the
    // model what the compiler thinks of the code is the gate that runs *after* a
    // write, so to learn what is wrong it has to write something first. Measured
    // on repair17b it did precisely that — it wrote `rust_port/check.sh` and
    // `compile.sh`, shell scripts no artifact tool can run, and those writes
    // counted as the deliverable and released the exploration lock. So run the
    // check the model cannot: when the task names a file already on disk and the
    // crate does not build, hand turn 1 the same verbatim diagnostics a repair
    // turn gets. The cost is one `cargo check`; the saving is the five to fifteen
    // `read_file` turns a model otherwise spends rediscovering rustc's output.
    if preexisting_target
        && crate::automation::build_runner::cargo_manifest_dir(workspace_root).is_ok()
    {
        match crate::agent::executor::thread::run_compilation_check(workspace_root) {
            Err(errors) if is_rustc_diagnostic_report(&errors) => {
                // Recorded as an encounter, not as a repair: the model has now
                // been shown these errors once, so if its first write leaves them
                // standing the gate's recurring-error note fires on the second.
                let sites = error_signatures(&errors, Some(workspace_root));
                ui_tx
                    .send(AgentToUiMessage::StatusUpdate(format!(
                        "The crate does not build yet — seeding turn 1 with cargo's own diagnostics \
                         ({} error site(s)) instead of making the model rediscover them...",
                        sites.iter().filter(|s| !is_item_signature(s)).count()
                    )))
                    .ok();
                repair_error_history.push(sites);
                compile_repair_pending = true;
                pending_repair_directive = Some(compile_repair_directive(&errors, workspace_root));
            }
            // A build failure that is not rustc output (no toolchain, a cargo the
            // OS killed) is nothing the model can be handed usefully.
            Err(_) => {}
            // The other clean-build case a from-scratch convert starts from: the
            // deliverable exists but is an EMPTY file, which compiles and passes
            // every test the crate already has. Nothing in the gate stack can see
            // that — convert22 spent its one write on a nested copy of the
            // workspace and the run still reported 52 green tests. The task text
            // named the destination; stating the file's actual state beside it is
            // what makes the destination something the model can act on.
            Ok(()) => {
                if let Some(rel) = named_target.as_deref() {
                    if std::fs::metadata(workspace_root.join(rel))
                        .map(|meta| meta.len() == 0)
                        .unwrap_or(false)
                    {
                        ui_tx
                            .send(AgentToUiMessage::StatusUpdate(format!(
                                "'{rel}' is empty and the crate builds without it — the whole \
                                 deliverable has to be written at that exact path...",
                            )))
                            .ok();
                        message_history.push(ChatMessage {
                            role: "user".to_string(),
                            content: format!(
                                "[System notice] '{rel}' exists but is 0 bytes, so the crate's \
                                 clean build says nothing about this task. The deliverable is the \
                                 file at that exact workspace-relative path: write it there, not \
                                 beside it or inside a copy of the workspace."
                            ),
                            name: None,
                            tool_call_id: None,
                            tool_calls: None,
                        });
                    }
                }
            }
        }
    }

    // Every exit below records its own ending through this, and the flag says
    // whether the handover on disk describes *this* run.
    let mut ending = RunEnding {
        workspace_root,
        recorded: false,
    };

    while loop_count < max_loops {
        // Wall-clock bound, checked at the turn boundary rather than mid-turn:
        // files already written stay written and the transcript so far stays
        // meaningful, so a stopped run reports real partial progress instead of
        // being killed inside a tool call. Without this an unattended dispatch
        // had no upper bound at all — the caller's own budget expired while the
        // loop kept issuing turns, and one porting run went 21 minutes past it.
        if let Some(at) = deadline {
            if std::time::Instant::now() >= at {
                ui_tx
                    .send(AgentToUiMessage::StatusUpdate(format!(
                        "Stopping before turn {}: wall-clock budget of {}s spent after {} completed turn(s)",
                        loop_count + 1,
                        budget_secs,
                        loop_count
                    )))
                    .ok();
                outcome = ending.stopped(
                    loop_count,
                    "wall-clock budget exhausted",
                    AgentLoopOutcome::TimedOut {
                        elapsed_secs: loop_started.elapsed().as_secs(),
                        budget_secs,
                        turns: loop_count,
                    },
                );
                break;
            }
        }
        if apply_headless_control_messages(cancel_rx, message_history, ui_tx, progress) {
            // Structured, not a status line to be substring-matched later: the
            // orchestrator worker needs to tell "an operator stopped this" from
            // "the model failed" so it does not retry a cancelled run on the
            // next provider in the chain.
            outcome = ending.stopped(
                loop_count,
                "cancelled by operator",
                AgentLoopOutcome::Cancelled,
            );
            break;
        }
        loop_count += 1;
        // Per-turn latency telemetry: without this, "the agent feels slow"
        // is unfalsifiable — provider time, tool time, and idle overhead all
        // blur together. Reported in the status line so any run's transcript
        // shows where the wall-clock actually went.
        let turn_start = std::time::Instant::now();
        ui_tx
            .send(AgentToUiMessage::StatusUpdate(format!(
                "Querying {} (Turn {})\u{2026}",
                current_provider.label(),
                loop_count
            )))
            .ok();

        // T1a: Inject recalled memories into system prompt for context-aware
        // planning. Lean-prompt models already skip this block entirely (see
        // `lean_context_for_reasoning_model` above, which was written after a
        // recalled `read_file` excerpt was mistaken for the task); the rest get
        // the same rule applied to the *content* rather than the model class,
        // because `qwen2.5-coder:7b` does not reason and still derailed on it.
        if !memory.is_empty() && !lean_context_for_reasoning_model {
            let last_user_msg = message_history
                .iter()
                .rev()
                .find(|m| m.role == "user")
                .map(|m| m.content.clone())
                .unwrap_or_default();
            if !last_user_msg.is_empty() {
                let hits = memory.recall_for_prompt(&last_user_msg, 3);
                if !hits.is_empty() {
                    let recall_block: String = hits
                        .iter()
                        .map(|h| format!("- [{}] {}", h.entry.key, h.entry.content))
                        .collect::<Vec<_>>()
                        .join("\n");
                    if let Some(sys_msg) = message_history.iter_mut().find(|m| m.role == "system") {
                        // Remove any previous recall block before appending new one
                        if let Some(idx) = sys_msg.content.find("\n\n## Recalled Context") {
                            sys_msg.content.truncate(idx);
                        }
                        sys_msg.content.push_str(&format!(
                            "\n\n## Recalled Context (from past sessions)\n{}",
                            recall_block
                        ));
                    }
                }
            }
        }

        // Once the model has been steered toward the deliverable (it announced a
        // write without emitting a call, an artifact nudge fired, or a streak of
        // turns went by with no edit) and still has written nothing, narrow the
        // exposed tools to the file editors only for this turn. Two effects, both
        // needed: (1) the model cannot keep exploring, so its context stops
        // growing and the verbose reasoning + the multi-KB write call actually fit
        // the output budget instead of being truncated before the tool call; (2)
        // with only writing tools visible the model commits to the write instead
        // of stalling on "let me look again". Measured on the LiteLLM crucible:
        // qwen3.6:35b-a3b emitted the write_file call only when list/read tools
        // were withheld alongside the enlarged reasoning-model budget.
        let repair_this_turn = compile_repair_pending;
        if repair_this_turn {
            compile_repair_pending = false;
        }
        // Held here rather than taken inside the rebuild below, because the
        // request it builds may never be answered: a retryable transport
        // failure has to hand it back and resend the same armed turn. It is
        // still single-use — every route that arms it also leaves the same text
        // in `write_gate_diagnostics`, so a turn that never rebuilds loses
        // nothing.
        let armed_repair_directive = pending_repair_directive.take();
        // A wrong-path refusal steers the same way an artifact nudge does: the
        // turn after it must be write-only with a lean context, or the model
        // re-reads the workspace and re-emits the same nested file.
        let steered_after_pause =
            artifact_nudges > 0 || off_target_refusals > 0 || auto_continuations > 0;
        let steered_after_reads = write_pressure_nudges > 0;
        // The gate's own verdict on this run's last write, still standing.
        let build_known_broken = write_gate_diagnostics.is_some();
        let force_write_only = should_lock_exploration(
            requires_artifact,
            produced_artifact,
            steered_after_pause,
            steered_after_reads,
            repair_this_turn,
            build_known_broken,
        );
        let rebuild_write_context =
            should_rebuild_write_context(force_write_only, steered_after_pause, repair_this_turn);
        // Which file-editing tool a forced turn may use. Before any file exists,
        // `edit_file` has nothing to patch, so advertising it only buys a
        // "not found in workspace" refusal on a nudge the run cannot spare; on a
        // repair turn, or when the task already names a file that is on disk, the
        // targeted editor is the only call shape that fits the output budget. The
        // advertised schema and the exploration lock below both derive from this
        // list, so the model is never told about a tool the turn does not offer.
        // A gate rejection settles the question outright: this run wrote a file,
        // so it is on disk and patchable, whatever the task named.
        let turn_write_tools: &[&str] =
            if repair_this_turn || preexisting_target || build_known_broken {
                &FILE_EDITING_TOOLS
            } else {
                &["write_file"]
            };
        let mut locked_tools: Vec<Value> = write_only_tools
            .iter()
            .filter(|t| {
                t["function"]["name"]
                    .as_str()
                    .is_some_and(|n| turn_write_tools.contains(&n))
            })
            .cloned()
            .collect();
        // The lock also admits a read of the deliverable itself, so advertise
        // it rather than leaving the model to guess that the path exists.
        locked_tools.extend(locked_read_tools.iter().cloned());
        let turn_tools: &Vec<Value> = if force_write_only {
            &locked_tools
        } else {
            &cf_tools
        };
        // The editors this turn can actually execute. Any directive that tells
        // the model to write has to be built from this list, not from a fixed
        // tool name: naming a tool the turn doesn't offer burns a nudge on a
        // guaranteed refusal.
        let advertised_editors: &[&str] = if force_write_only {
            turn_write_tools
        } else {
            &FILE_EDITING_TOOLS
        };

        // Two failure modes stall a small reasoning model once steering fires,
        // and the live crucible dump proved both are present at once:
        //   (1) Self-imitation — an empty "I need to write the file now"
        //       announcement with no call enters history, and every later turn
        //       mirrors the no-action narration instead of emitting the call
        //       (turns 6..10 repeated it verbatim while history grew 15→36).
        //   (2) Context pressure — even the pruned-to-tool-calls history was
        //       23+ messages, so the model's attention never lands on the one
        //       instruction that matters: call write_file NOW. The direct-API
        //       probe wrote the file reliably ONLY from a ~6-message context
        //       (system + task + last one/two tool exchanges + a hard directive)
        //       with write-only tools and the enlarged budget.
        // So when write-force is active we reconstruct exactly that clean
        // skeleton: system prompt, the original task, the last TWO
        // assistant(tool_calls)+tool(result) exchanges (enough exploration data
        // to fill a map), then a fresh imperative directive. Every prose
        // announcement turn and every stacked [System notice] is dropped so
        // there is nothing off-action to imitate. OpenAI-compatible endpoints
        // reject a tool result with no preceding tool_calls turn, hence keeping
        // exchanges as whole pairs.
        let pruned_history: Vec<ChatMessage> = if rebuild_write_context {
            let mut hist: Vec<ChatMessage> = Vec::new();
            // System prompt (first system message only).
            if let Some(sys) = message_history.iter().find(|m| m.role == "system") {
                hist.push(sys.clone());
            }
            // Original task = first user message that is not an injected notice.
            if let Some(task) = message_history
                .iter()
                .find(|m| m.role == "user" && !m.content.starts_with("[System notice]"))
            {
                hist.push(task.clone());
            }
            // Collect (assistant-with-tool_calls index, following tool-result
            // indices) pairs, then keep only the last two pairs.
            let mut pair_starts: Vec<usize> = message_history
                .iter()
                .enumerate()
                .filter(|(_, m)| m.role == "assistant" && m.tool_calls.is_some())
                .map(|(i, _)| i)
                .rev()
                .take(2)
                .collect();
            pair_starts.sort_unstable();
            for start in pair_starts {
                hist.push(message_history[start].clone());
                // This call's tool results: the contiguous run of `tool`
                // messages immediately after the assistant turn.
                let mut j = start + 1;
                while j < message_history.len() && message_history[j].role == "tool" {
                    hist.push(message_history[j].clone());
                    j += 1;
                }
            }
            // Hard imperative directive as the final turn — the exact shape
            // that flipped the probe from narration to an executed write. On a
            // compile-repair turn it carries the live rustc diagnostics so the
            // model fixes the named errors instead of re-narrating intent.
            // A rejected write whose repair budget is already spent has no armed
            // turn, but the rebuild erases the exchanges that said so; a blind
            // "write something now" over a red build asks for another file that
            // does not compile, so the standing gate report fills in.
            let directive = write_context_directive(
                armed_repair_directive.clone(),
                write_gate_diagnostics.as_deref(),
                turn_write_tools,
                workspace_root,
            );
            hist.push(ChatMessage {
                role: "user".to_string(),
                content: directive,
                name: None,
                tool_call_id: None,
                tool_calls: None,
            });
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(format!(
                    "Reconstructed clean {}-message write context (from {} in history) to break the no-action loop",
                    hist.len(),
                    message_history.len()
                )))
                .ok();
            hist
        } else {
            message_history.clone()
        };

        // Compress to the window this request will actually declare, not to the
        // window the model advertises: a local server silently drops the front of
        // a prompt that overflows its allocation.
        let compressed_history = compress_history_to_window(
            &pruned_history,
            declared_window(current_provider, &current_model),
            current_profile.supports_tools,
            turn_tools,
        );

        let mut request_body = build_request(
            &current_profile,
            &current_model,
            &compressed_history,
            turn_tools,
            current_thinking,
            current_provider,
        );
        // On a write-forced artifact turn, bias the sampler hard toward the
        // one action we need. A small reasoning model left at default
        // temperature sometimes narrates its intent as prose instead of firing
        // the call (the run-to-run flip that let one crucible succeed and the
        // next fall back); near-greedy decoding on top of the reconstructed
        // clean context makes the write_file call the overwhelmingly likely
        // continuation. Set on both the OpenAI-style field and Ollama's
        // `options` block so whichever shim reads it gets the low value.
        if force_write_only {
            request_body["temperature"] = json!(0.2);
            // Qwen3 keeps emitting a `reasoning` trace even with think:false,
            // and on a write turn that trace re-deliberates the whole map.
            // Measured on the probe: at the standard 14000 budget a slow sample
            // hit `finish_reason=length` mid-reasoning and never reached the
            // call. The reconstructed write context is only a handful of
            // messages, so there is room to lift the output ceiling; 18000 out
            // of a 20480 window leaves the reasoning free to finish before the
            // tool call.
            request_body["max_tokens"] = json!(18000);
            if let Some(opts) = request_body
                .get_mut("options")
                .and_then(|o| o.as_object_mut())
            {
                opts.insert("temperature".to_string(), json!(0.2));
                opts.insert("num_ctx".to_string(), json!(LOCAL_OLLAMA_NUM_CTX));
                opts.insert("num_predict".to_string(), json!(18000));
            }
        }
        write_last_request_artifacts(
            workspace_root,
            &current_profile,
            &current_model,
            current_provider,
            current_thinking,
            &compressed_history,
            turn_tools,
            &request_body,
        );

        // ─── Velocity Router MoA Dispatch ─────────────────────────────────────
        // First turn only, and only while the router has not already given up
        // this mission: an alive-but-slow router that blocks 120 s per attempt
        // must not stall every subsequent tool-loop turn.
        let router_handled = if let Some(router_cfg) = router_settings {
            if !router_gave_up
                && loop_count == 1
                && router_cfg.enabled
                && !router_cfg.api_key.trim().is_empty()
            {
                // Check router health (cached after first check)
                let router_available = router_client::is_router_available()
                    || router_client::refresh_router_availability(&router_cfg.url);

                if router_available {
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "Routing through Velocity MoA (Turn {})\u{2026}",
                            loop_count
                        )))
                        .ok();

                    // Extract the user's prompt from message history
                    let user_prompt = message_history
                        .iter()
                        .rev()
                        .find(|m| m.role == "user")
                        .map(|m| m.content.clone())
                        .unwrap_or_default();

                    if !user_prompt.is_empty() {
                        // Gather workspace context: extract file paths from the prompt
                        // and read their contents for the MoA specialists.
                        let (file_paths, context) =
                            gather_workspace_context(workspace_root, &user_prompt);

                        let router_request = AssignmentRequest {
                            task: user_prompt,
                            tier: ExecutionTier::Ultimate,
                            context: if context.is_empty() {
                                None
                            } else {
                                Some(context)
                            },
                            file_paths,
                            mode: ExecutionMode::Sync,
                            max_cost_usd: None,
                        };

                        let router_started = std::time::Instant::now();
                        match router_client::submit_assignment(
                            &router_cfg.url,
                            &router_cfg.api_key,
                            &router_request,
                        ) {
                            Ok(response) => {
                                if response.status == AssignmentStatus::Completed {
                                    // Plausibility gate: a real MoA pass fans out to
                                    // several specialists and assembles their answers;
                                    // it cannot finish in under three seconds. A
                                    // "Completed" that fast is a cached or echoed
                                    // assignment — in the Sept 2026 agent_eval run an
                                    // instant response whose text mirrored an
                                    // unrelated session ended the chat turn with zero
                                    // model work. Ignore it and dispatch directly.
                                    let elapsed = router_started.elapsed();
                                    if elapsed < std::time::Duration::from_secs(3) {
                                        ui_tx
                                            .send(AgentToUiMessage::StatusUpdate(format!(
                                                "MoA answered in {}ms — too fast to be a real synthesis; ignoring the cached response.",
                                                elapsed.as_millis()
                                            )))
                                            .ok();
                                        router_gave_up = true;
                                        false
                                    } else if let Some(assembled) = response.assembled_output {
                                        // Stream the assembled output to the UI
                                        ui_tx
                                            .send(AgentToUiMessage::OutputToken(
                                                sanitize_chat_token(&assembled),
                                            ))
                                            .ok();

                                        // Add to message history
                                        message_history.push(ChatMessage {
                                            role: "assistant".to_string(),
                                            content: assembled.clone(),
                                            name: None,
                                            tool_call_id: None,
                                            tool_calls: None,
                                        });

                                        // Log routing info if available
                                        if let Some(plan) = &response.plan {
                                            let subtask_count = plan.subtasks.len();
                                            let models_used: Vec<_> = plan
                                                .subtasks
                                                .iter()
                                                .map(|s| format!("{} ({})", s.model, s.domain))
                                                .collect();
                                            ui_tx
                                                .send(AgentToUiMessage::StatusUpdate(format!(
                                                    "MoA: {} sub-tasks → {}",
                                                    subtask_count,
                                                    models_used.join(", ")
                                                )))
                                                .ok();
                                        }

                                        if let Some(cost) = &response.cost {
                                            ui_tx
                                                .send(AgentToUiMessage::StatusUpdate(format!(
                                                    "MoA cost: ${:.6} ({} tokens)",
                                                    cost.total_cost_usd, cost.total_tokens
                                                )))
                                                .ok();
                                        }

                                        // Router handled it — skip direct dispatch
                                        true
                                    } else {
                                        ui_tx
                                            .send(AgentToUiMessage::StatusUpdate(
                                                "MoA completed but produced no output. Falling back to direct dispatch.".to_string(),
                                            ))
                                            .ok();
                                        false
                                    }
                                } else if response.status == AssignmentStatus::Failed {
                                    let err =
                                        response.error.unwrap_or_else(|| "Unknown error".into());
                                    ui_tx
                                        .send(AgentToUiMessage::StatusUpdate(format!(
                                            "MoA failed: {}. Falling back to direct dispatch.",
                                            err
                                        )))
                                        .ok();
                                    false
                                } else {
                                    ui_tx
                                        .send(AgentToUiMessage::StatusUpdate(format!(
                                            "MoA status: {:?}. Falling back to direct dispatch.",
                                            response.status
                                        )))
                                        .ok();
                                    false
                                }
                            }
                            Err(e) => {
                                ui_tx
                                    .send(AgentToUiMessage::StatusUpdate(format!(
                                        "MoA unavailable ({}). Falling back to direct dispatch.",
                                        e
                                    )))
                                    .ok();
                                router_client::mark_router_unavailable();
                                router_gave_up = true;
                                false
                            }
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        // If the router handled the request, continue to the next iteration
        // (tool call processing, etc.)
        if router_handled {
            // Skip the rest of the dispatch loop — the response is already
            // in message_history and streamed to the UI.
            // The loop will continue to check for tool calls, etc.
            continue;
        }

        // ─── Direct Provider Dispatch (fallback) ──────────────────────────────
        let mut used_account: Option<&CloudflareAccount> = None;
        let mut used_or_account: Option<&OpenRouterAccount> = None;
        // Cleared first so the cause read back below can only describe *this*
        // request and never a failure from an earlier turn.
        super::dispatch::clear_last_request_error();
        let ureq_response = match current_provider {
            AiProvider::OpenRouter => {
                let (res, acct) = super::dispatch::execute_openrouter_request(
                    or_accounts,
                    accounts,
                    usage_tracker,
                    &request_body,
                    ui_tx,
                );
                used_or_account = acct;
                res
            }
            AiProvider::CloudflareWorkersAi => {
                let (res, acct) = super::dispatch::execute_cloudflare_request(
                    accounts,
                    or_accounts,
                    usage_tracker,
                    &request_body,
                    ui_tx,
                );
                used_account = acct;
                res
            }
            AiProvider::AzureOpenAi => {
                super::dispatch::execute_azure_request(azure_accounts, &request_body, ui_tx)
            }
            AiProvider::LocalOllama => super::dispatch::execute_ollama_request(
                ollama_accounts,
                &request_body,
                ui_tx,
                workspace_root,
            ),
            AiProvider::Deepseek => {
                super::dispatch::execute_deepseek_request(&request_body, ui_tx, workspace_root)
            }
            AiProvider::AlibabaQwen => {
                super::dispatch::execute_alibaba_qwen_request(&request_body, ui_tx, workspace_root)
            }
            AiProvider::AwsBedrock => {
                super::dispatch::execute_bedrock_request(&request_body, ui_tx, workspace_root)
            }
            AiProvider::Groq => {
                super::dispatch::execute_groq_request(&request_body, ui_tx, workspace_root)
            }
            AiProvider::Mistral => {
                super::dispatch::execute_mistral_request(&request_body, ui_tx, workspace_root)
            }
            AiProvider::OpenAI => {
                super::dispatch::execute_openai_request(&request_body, ui_tx, workspace_root)
            }
            AiProvider::GoogleVertex => {
                super::dispatch::execute_google_request(&request_body, ui_tx, workspace_root)
            }
            AiProvider::TogetherAi => {
                super::dispatch::execute_together_request(&request_body, ui_tx, workspace_root)
            }
            AiProvider::FireworksAi => {
                super::dispatch::execute_fireworks_request(&request_body, ui_tx, workspace_root)
            }
            AiProvider::Perplexity => {
                super::dispatch::execute_perplexity_request(&request_body, ui_tx, workspace_root)
            }
            AiProvider::Cerebras => {
                super::dispatch::execute_cerebras_request(&request_body, ui_tx, workspace_root)
            }
            AiProvider::Anthropic => {
                super::dispatch::execute_anthropic_request(&request_body, ui_tx, workspace_root)
            }
        };

        let response = match ureq_response {
            Some(res) => res,
            None => {
                // The real cause, recorded by the dispatch layer as it failed.
                // Read at the top of this arm, before any retry decision,
                // because both ways out — another provider, or the same request
                // after a sleep — have to see it. Before this existed the loop
                // could only guess from the provider name ("…or
                // DASHSCOPE_API_KEY missing.") and that guess was wrong whenever
                // the key was present and the actual problem was a 404 model
                // name, a 429 or a dead socket.
                let request_error = super::dispatch::take_last_request_error();
                let resend_may_help = !request_error
                    .as_ref()
                    .map(|e| e.is_resend_futile())
                    .unwrap_or(false);
                // Whatever this turn's request was built from, it is being sent
                // again, so the armed state has to come back with it. The lean
                // write-context rebuild consumed the directive to build the
                // request and the turn top cleared the flag; without this the
                // resend goes out as a bare task line with every tool advertised
                // — `blip_repair` measured exactly that on a seeded first turn,
                // losing the diagnostics the run had paid a `cargo check` for.
                if resend_may_help {
                    if repair_this_turn {
                        compile_repair_pending = true;
                    }
                    // Moved, not cloned: handing the directive back is this
                    // local's only remaining job, and the next turn re-takes it
                    // from `pending_repair_directive`.
                    pending_repair_directive = armed_repair_directive;
                }
                let fallback = fallback_provider(current_provider);
                let fallback_available = match fallback {
                    AiProvider::OpenRouter => {
                        !or_accounts.is_empty() || !openrouter_api_key().trim().is_empty()
                    }
                    AiProvider::CloudflareWorkersAi => !accounts.is_empty(),
                    AiProvider::AzureOpenAi => !azure_accounts.is_empty(),
                    AiProvider::LocalOllama => !ollama_accounts.is_empty(),
                    AiProvider::Deepseek => !std::env::var("DEEPSEEK_API_KEY")
                        .unwrap_or_default()
                        .trim()
                        .is_empty(),
                    AiProvider::AlibabaQwen => !std::env::var("DASHSCOPE_API_KEY")
                        .unwrap_or_default()
                        .trim()
                        .is_empty(),
                    AiProvider::AwsBedrock => std::env::var("BEDROCK_PROXY_URL").is_ok(),
                    AiProvider::Groq => !std::env::var("GROQ_API_KEY")
                        .unwrap_or_default()
                        .trim()
                        .is_empty(),
                    AiProvider::Mistral => !std::env::var("MISTRAL_API_KEY")
                        .unwrap_or_default()
                        .trim()
                        .is_empty(),
                    AiProvider::OpenAI => !std::env::var("OPENAI_API_KEY")
                        .unwrap_or_default()
                        .trim()
                        .is_empty(),
                    AiProvider::GoogleVertex => !std::env::var("GOOGLE_API_KEY")
                        .unwrap_or_default()
                        .trim()
                        .is_empty(),
                    AiProvider::TogetherAi => !std::env::var("TOGETHER_API_KEY")
                        .unwrap_or_default()
                        .trim()
                        .is_empty(),
                    AiProvider::FireworksAi => !std::env::var("FIREWORKS_API_KEY")
                        .unwrap_or_default()
                        .trim()
                        .is_empty(),
                    AiProvider::Perplexity => !std::env::var("PERPLEXITY_API_KEY")
                        .unwrap_or_default()
                        .trim()
                        .is_empty(),
                    AiProvider::Cerebras => !std::env::var("CEREBRAS_API_KEY")
                        .unwrap_or_default()
                        .trim()
                        .is_empty(),
                    AiProvider::Anthropic => !std::env::var("ANTHROPIC_API_KEY")
                        .unwrap_or_default()
                        .trim()
                        .is_empty(),
                };

                if fallback_attempts < 7 && fallback_available {
                    fallback_attempts += 1;
                    let prev_provider = current_provider;
                    current_provider = fallback;
                    current_model = default_provider_model(current_provider);
                    current_profile = match current_provider {
                        AiProvider::OpenRouter => default_model_info(&current_model),
                        AiProvider::CloudflareWorkersAi => {
                            enrich_model_profile(accounts, &default_model_info(&current_model))
                        }
                        _ => default_model_info(&current_model),
                    };
                    current_thinking = thinking && current_profile.supports_thinking;
                    hard_fail_retries = 0; // fresh provider earns its own retries

                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "Provider {} unavailable/exhausted (Attempt {}/7). Seamlessly falling back to default provider {} (Model: {})...",
                            prev_provider.label(),
                            fallback_attempts,
                            current_provider.label(),
                            current_model
                        )))
                        .ok();
                    ui_tx
                        .send(AgentToUiMessage::ProviderChanged(current_provider))
                        .ok();

                    loop_count = loop_count.saturating_sub(1);
                    continue;
                }

                // No other provider to move to. Transport failures are often
                // transient (connection reset, TLS hiccup, upstream blip), so
                // retry the same request with backoff before giving up — but not
                // when the recorded cause says resending cannot change the answer.
                // gateprobe4 printed "client errors are deterministic" and then
                // resent the identical request twice anyway, ten wasted seconds
                // per model candidate on the way to BLOCKED.
                if hard_fail_retries < 2 && resend_may_help {
                    hard_fail_retries += 1;
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "Provider {} failed and no fallback provider is configured. Retrying same request in 5s (attempt {}/2)...",
                            current_provider.label(),
                            hard_fail_retries
                        )))
                        .ok();
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    loop_count = loop_count.saturating_sub(1);
                    continue;
                }

                let err_msg = match &request_error {
                    Some(err) => err.describe(),
                    // No recorded cause means some path returned None without
                    // going through the shared reporter. Say so rather than
                    // inventing a credential problem.
                    None => format!(
                        "{} returned no response and recorded no reason (model: {}).",
                        current_provider.label(),
                        current_model
                    ),
                };
                // A whole-provider failure (no key, no accounts, transport dead,
                // retries spent) cannot be fixed by another model on the same
                // provider; a per-model rejection can. That distinction drives
                // the dispatcher's failover, so it is carried in the outcome
                // variant instead of being re-derived from this sentence.
                let provider_wide = request_error
                    .as_ref()
                    .map(|e| e.is_provider_wide())
                    .unwrap_or(true);
                // Record the failure honestly in the handover so the next
                // session (and the UI) know the mission ended mid-flight
                // rather than silently "finishing" — see `RunEnding`, which
                // derives the state and the `interrupted` flag from the outcome
                // assigned below.
                // "after retries" describes a run that retried. When the cause
                // said resending could not change the answer and the loop
                // believed it, saying it anyway would report a wait that never
                // happened.
                let retry_note = if hard_fail_retries > 0 {
                    " after retries"
                } else {
                    ""
                };
                ui_tx
                    .send(AgentToUiMessage::StatusUpdate(format!(
                        "Agent BLOCKED: {} ({}) could not fulfill the request{}.",
                        current_provider.label(),
                        current_model,
                        retry_note
                    )))
                    .ok();
                ui_tx
                    .send(AgentToUiMessage::OutputToken(format!(
                        "\n\nError: {err_msg}"
                    )))
                    .ok();
                outcome = ending.stopped(
                    loop_count,
                    "provider unavailable",
                    if provider_wide {
                        AgentLoopOutcome::ProviderUnavailable { reason: err_msg }
                    } else {
                        AgentLoopOutcome::Blocked {
                            reason: format!("model '{}' rejected: {}", current_model, err_msg),
                        }
                    },
                );
                break;
            }
        };

        let tokens_in = compressed_history
            .iter()
            .map(|m| estimate_tokens(&m.content))
            .sum::<u64>();

        let mut assistant_content = String::new();
        let mut reasoning_content = String::new();
        let mut accumulated_tools: Vec<ToolCallAccumulator> = Vec::new();
        let mut streamed_len: usize = 0;
        let mut suppressing = false;

        // Anthropic returns a non-streaming JSON response in a different
        // format than the OpenAI SSE events the stream parser expects.
        // Handle it here, then skip the stream-reading loop.
        let inner: Box<dyn std::io::Read + Send> = if current_provider == AiProvider::Anthropic {
            let body_str = response.into_string().unwrap_or_default();
            if let Ok(parsed) = serde_json::from_str::<Value>(&body_str) {
                if let Some(content_blocks) = parsed["content"].as_array() {
                    for block in content_blocks {
                        match block["type"].as_str() {
                            Some("text") => {
                                if let Some(text) = block["text"].as_str() {
                                    assistant_content.push_str(text);
                                    ui_tx
                                        .send(AgentToUiMessage::OutputToken(sanitize_chat_token(
                                            text,
                                        )))
                                        .ok();
                                }
                            }
                            Some("thinking") => {
                                if let Some(text) = block["thinking"].as_str() {
                                    reasoning_content.push_str(text);
                                    ui_tx
                                        .send(AgentToUiMessage::ThoughtToken(text.to_string()))
                                        .ok();
                                }
                            }
                            Some("tool_use") => {
                                let name = block["name"].as_str().unwrap_or("").to_string();
                                let id = block["id"].as_str().unwrap_or("").to_string();
                                let input = block["input"].clone();
                                let arguments = serde_json::to_string(&input)
                                    .unwrap_or_else(|_| "{}".to_string());
                                accumulated_tools.push(ToolCallAccumulator {
                                    id,
                                    name,
                                    arguments,
                                });
                            }
                            _ => {}
                        }
                    }
                }
                if let Some(err) = parsed["error"]["message"].as_str() {
                    ui_tx
                        .send(AgentToUiMessage::OutputToken(format!(
                            "\n\nAnthropic error: {err}"
                        )))
                        .ok();
                }
            }
            streamed_len = assistant_content.len();
            Box::new(std::io::empty())
        } else {
            Box::new(response.into_reader())
        };

        // Read on a worker thread so the wall-clock deadline is enforceable
        // *inside* a line, not only between lines. Ollama's native endpoint sends
        // the whole assistant message — a 20 KB tool-call payload included — as a
        // single JSON line, so a per-line check cannot fire until that line's
        // generation ends: measured, a 149s budget overran by 98s that way, with
        // ureq's 600s read timeout as the hard ceiling. The thread owns the
        // socket reader; this loop only ever waits on it.
        let (line_tx, line_rx) = std::sync::mpsc::channel::<Result<String, String>>();
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(inner);
            let mut buf = String::new();
            loop {
                buf.clear();
                match reader.read_line(&mut buf) {
                    Ok(0) => break,
                    Ok(_) => {
                        if line_tx.send(Ok(buf.clone())).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = line_tx.send(Err(format!("{e:?}")));
                        break;
                    }
                }
            }
        });

        let mut cut_at_deadline = false;
        loop {
            if apply_headless_control_messages(cancel_rx, message_history, ui_tx, progress) {
                break;
            }
            let mut interrupted = false;
            while let Ok(ui_msg) = ui_rx.try_recv() {
                match ui_msg {
                    UiToAgentMessage::CancelTask => {
                        ui_tx
                            .send(AgentToUiMessage::StatusUpdate(
                                "Task interrupted by operator.".to_string(),
                            ))
                            .ok();
                        interrupted = true;
                        break;
                    }
                    other => {
                        deferred_messages.push(other);
                    }
                }
            }
            if interrupted {
                break;
            }
            let line = match deadline {
                Some(at) => match line_rx
                    .recv_timeout(at.saturating_duration_since(std::time::Instant::now()))
                {
                    Ok(line) => line,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        cut_at_deadline = true;
                        break;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                },
                None => match line_rx.recv() {
                    Ok(line) => line,
                    Err(_) => break,
                },
            };
            match line {
                Ok(line_buf) => {
                    let cleaned = line_buf.trim();
                    if cleaned.is_empty() {
                        continue;
                    }
                    if cleaned == "data: [DONE]" {
                        break;
                    }
                    if !cleaned.starts_with("data: ") {
                        if let Ok(parsed) = serde_json::from_str::<Value>(cleaned) {
                            if let Some(msg) = parsed.get("message") {
                                if let Some(t) = msg.get("thinking").and_then(|v| v.as_str()) {
                                    if !t.is_empty() {
                                        reasoning_content.push_str(t);
                                        ui_tx
                                            .send(AgentToUiMessage::ThoughtToken(t.to_string()))
                                            .ok();
                                    }
                                }
                                if let Some(tok) = msg.get("content").and_then(|v| v.as_str()) {
                                    if !tok.is_empty() {
                                        absorb_content_delta(
                                            tok,
                                            &mut assistant_content,
                                            &mut streamed_len,
                                            &mut suppressing,
                                            ui_tx,
                                        );
                                    }
                                }
                                if let Some(tcs) = msg.get("tool_calls").and_then(|v| v.as_array())
                                {
                                    for tc in tcs {
                                        let name = tc["function"]["name"]
                                            .as_str()
                                            .unwrap_or("")
                                            .to_string();
                                        let args = &tc["function"]["arguments"];
                                        let args_str = match args {
                                            Value::String(s) => s.clone(),
                                            other => serde_json::to_string(other)
                                                .unwrap_or_else(|_| "{}".to_string()),
                                        };
                                        accumulated_tools.push(ToolCallAccumulator {
                                            id: format!("native_{}", accumulated_tools.len()),
                                            name,
                                            arguments: args_str,
                                        });
                                    }
                                }
                            }
                            if parsed.get("done").and_then(|v| v.as_bool()) == Some(true) {
                                break;
                            }
                        }
                    }

                    if let Some(json_part) = cleaned.strip_prefix("data: ") {
                        if let Ok(parsed) = serde_json::from_str::<Value>(json_part) {
                            if let Some(choices) = parsed["choices"].as_array() {
                                if let Some(first_choice) = choices.first() {
                                    let delta = &first_choice["delta"];

                                    if let Some(r) = delta["reasoning_content"]
                                        .as_str()
                                        .or_else(|| delta["reasoning"].as_str())
                                    {
                                        reasoning_content.push_str(r);
                                        ui_tx
                                            .send(AgentToUiMessage::ThoughtToken(r.to_string()))
                                            .ok();
                                    }

                                    if let Some(tok) = delta["content"].as_str() {
                                        absorb_content_delta(
                                            tok,
                                            &mut assistant_content,
                                            &mut streamed_len,
                                            &mut suppressing,
                                            ui_tx,
                                        );
                                    }

                                    if let Some(tool_calls) = delta["tool_calls"].as_array() {
                                        for tc in tool_calls {
                                            let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                                            while accumulated_tools.len() <= idx {
                                                accumulated_tools.push(ToolCallAccumulator {
                                                    id: String::new(),
                                                    name: String::new(),
                                                    arguments: String::new(),
                                                });
                                            }
                                            if let Some(id) = tc["id"].as_str() {
                                                accumulated_tools[idx].id.push_str(id);
                                            }
                                            if let Some(func) = tc["function"].as_object() {
                                                if let Some(n) =
                                                    func.get("name").and_then(|v| v.as_str())
                                                {
                                                    accumulated_tools[idx].name.push_str(n);
                                                }
                                                if let Some(a) =
                                                    func.get("arguments").and_then(|v| v.as_str())
                                                {
                                                    accumulated_tools[idx].arguments.push_str(a);
                                                }
                                            }
                                        }
                                    }
                                }
                            } else if let Some(content) = parsed["response"]
                                .as_str()
                                .or_else(|| parsed["output"].as_str())
                                .or_else(|| parsed["text"].as_str())
                            {
                                assistant_content.push_str(content);
                                streamed_len += content.len();
                                ui_tx
                                    .send(AgentToUiMessage::OutputToken(sanitize_chat_token(
                                        content,
                                    )))
                                    .ok();
                            }
                        }
                    }
                }
                Err(msg) => {
                    ui_tx
                        .send(AgentToUiMessage::OutputToken(format!(
                            "\nError reading stream: {msg}"
                        )))
                        .ok();
                    break;
                }
            }
        }

        // Stream closed (or Anthropic body consumed): everything up to here
        // is provider latency — connect, prefill, and token generation.
        let provider_ms = turn_start.elapsed().as_millis();

        // Out of wall-clock time: judge this turn's tool calls by what actually
        // arrived, never by what the stream meant to send. A turn that finished
        // streaming just after the deadline keeps all of its calls (none of them
        // truncated); a cut-off one keeps only the arguments that closed.
        let out_of_time = deadline.is_some_and(|at| std::time::Instant::now() >= at);
        if out_of_time {
            accumulated_tools = salvage_complete_calls(accumulated_tools);
            if cut_at_deadline && accumulated_tools.is_empty() {
                ui_tx
                    .send(AgentToUiMessage::StatusUpdate(format!(
                        "Wall-clock budget of {}s expired {} into turn {}: its unfinished reply is discarded rather than executed, because acting on a truncated tool call would leave a half-written file.",
                        budget_secs,
                        provider_ms / 1000,
                        loop_count
                    )))
                    .ok();
                outcome = ending.stopped(
                    loop_count,
                    "wall-clock budget exhausted mid-turn",
                    AgentLoopOutcome::TimedOut {
                        elapsed_secs: loop_started.elapsed().as_secs(),
                        budget_secs,
                        turns: loop_count.saturating_sub(1),
                    },
                );
                break;
            }
            if cut_at_deadline {
                ui_tx
                    .send(AgentToUiMessage::StatusUpdate(format!(
                        "Wall-clock budget of {}s expired {} into turn {}: landing only the {} tool call(s) whose arguments arrived in full.",
                        budget_secs,
                        provider_ms / 1000,
                        loop_count,
                        accumulated_tools.len()
                    )))
                    .ok();
            }
        }

        if !suppressing && streamed_len <= assistant_content.len() {
            let mut flush_start = streamed_len;
            while flush_start > 0 && !assistant_content.is_char_boundary(flush_start) {
                flush_start -= 1;
            }
            let tail = &assistant_content[flush_start..];
            if !tail.is_empty() {
                ui_tx
                    .send(AgentToUiMessage::OutputToken(sanitize_chat_token(tail)))
                    .ok();
            }
        }

        if accumulated_tools.is_empty() && assistant_content.contains("<tool_call>") {
            let mut clean_content = String::new();
            let mut rest = assistant_content.as_str();
            while let Some(start) = rest.find("<tool_call>") {
                clean_content.push_str(&rest[..start]);
                let after_open = &rest[start + "<tool_call>".len()..];

                let ef = after_open
                    .find("</function>")
                    .map(|p| (p, p + "</function>".len()));
                let et = after_open
                    .find("</tool_call>")
                    .map(|p| (p, p + "</tool_call>".len()));
                let en = after_open.find("<tool_call>").and_then(|p| {
                    if p > 0 {
                        Some((p, p))
                    } else {
                        None
                    }
                });
                let best = [ef, et, en]
                    .into_iter()
                    .flatten()
                    .min_by_key(|(pos, _)| *pos);

                let (block, remainder) = if let Some((end_pos, after_end)) = best {
                    (&after_open[..end_pos], &after_open[after_end..])
                } else {
                    (after_open, "")
                };

                if let Some(fname_start) = block.find("<function=") {
                    let fname_rest = &block[fname_start + "<function=".len()..];
                    let fname_end = fname_rest
                        .find('>')
                        .or_else(|| fname_rest.find('\n'))
                        .unwrap_or(fname_rest.len());
                    let fname = fname_rest[..fname_end].trim().to_string();
                    if !fname.is_empty() {
                        let mut args = serde_json::Map::new();
                        let mut param_rest = fname_rest;
                        while let Some(ps) = param_rest.find("<parameter=") {
                            let after_ps = &param_rest[ps + "<parameter=".len()..];
                            let key_end = match after_ps.find('>') {
                                Some(pos) => pos,
                                None => break,
                            };
                            let key = after_ps[..key_end].to_string();
                            let val_start = (key_end + 1).min(after_ps.len());
                            let val_end = after_ps[val_start..]
                                .find("</parameter>")
                                .map(|e| val_start + e)
                                .unwrap_or(after_ps.len());
                            let val = after_ps[val_start..val_end].trim().to_string();
                            args.insert(key, Value::String(val));
                            let next_slice_start =
                                (val_end + "</parameter>".len()).min(after_ps.len());
                            param_rest = &after_ps[next_slice_start..];
                        }
                        let call_id = format!("inline_{}", accumulated_tools.len());
                        accumulated_tools.push(ToolCallAccumulator {
                            id: call_id,
                            name: fname,
                            arguments: serde_json::to_string(&args)
                                .unwrap_or_else(|_| "{}".to_string()),
                        });
                    }
                }
                rest = remainder;
                if rest.is_empty() {
                    break;
                }
            }
            clean_content.push_str(rest);
            assistant_content = clean_content.trim().to_string();
        }

        // --- Wrapperless native-fragment dialect --------------------------
        // Token-plan gateways (live glm-5.2 runs) split the native
        // function-calling payload into delta content tokens instead of
        // the tool_calls channel and interleave fragments with prose on
        // the same lines, which a line-based reader misses. The
        // character-level scanner recovers calls wherever the markers
        // sit; literals are assembled at runtime in the helper below.
        if accumulated_tools.is_empty() {
            let (frag_prose, frag_calls, _frag_truncated) =
                parse_function_fragments(&assistant_content);
            if !frag_calls.is_empty() {
                for (fname, args) in frag_calls {
                    accumulated_tools.push(ToolCallAccumulator {
                        id: format!("frag_{}", accumulated_tools.len()),
                        name: fname,
                        arguments: serde_json::to_string(&Value::Object(args))
                            .unwrap_or_else(|_| "{}".to_string()),
                    });
                }
                assistant_content = frag_prose;
            }
            // A truncated scan with no complete call leaves the content
            // untouched: the residue stripper and the auto-continue
            // notice below handle a call cut off at EOF.
        }

        if accumulated_tools.is_empty() && assistant_content.contains("[Calling tool ") {
            let marker = "[Calling tool ";
            let mut clean2 = String::new();
            let mut rest2 = assistant_content.as_str();
            while let Some(start) = rest2.find(marker) {
                clean2.push_str(&rest2[..start]);
                let after = &rest2[start + marker.len()..];
                if let Some(name_end) = after.find('\'') {
                    let raw_name = &after[..name_end];
                    let args_marker = " with arguments '";
                    if let Some(args_start_rel) = after[name_end..].find(args_marker) {
                        let args_start = name_end + args_start_rel + args_marker.len();
                        let args_section = &after[args_start..];
                        let args_end = args_section.find("']").unwrap_or(args_section.len());
                        let args_str = &args_section[..args_end];
                        let arguments: Value = serde_json::from_str(args_str).unwrap_or(json!({}));
                        let call_id = format!("bracket_{}", accumulated_tools.len());
                        accumulated_tools.push(ToolCallAccumulator {
                            id: call_id,
                            name: raw_name.to_string(),
                            arguments: serde_json::to_string(&arguments)
                                .unwrap_or_else(|_| "{}".to_string()),
                        });
                        let consumed_in_after = args_start + args_end + "']".len();
                        rest2 = &rest2[start + marker.len() + consumed_in_after.min(after.len())..];
                    } else {
                        clean2.push_str(&rest2[start..]);
                        rest2 = &rest2[rest2.len()..];
                        break;
                    }
                } else {
                    clean2.push_str(&rest2[start..]);
                    rest2 = &rest2[rest2.len()..];
                    break;
                }
            }
            clean2.push_str(rest2);
            if !accumulated_tools.is_empty() {
                assistant_content = clean2.trim().to_string();
            }
        }

        // --- Claude-family inline dialect (namespace-qualified invoke tree) ---
        // A model whose profile claims no tool support receives the tool
        // catalog as prompt text and answers in the Anthropic wire format it
        // was trained on: an invoke marker with a quoted name attribute,
        // quoted-name parameter markers, provider-qualified tag names and a
        // call-list wrapper. None of the readers above match that spelling, so
        // without this the turn degrades into a "final answer" and the mission
        // stops after one turn (LiteLLM crucible, phase 1, three runs).
        if accumulated_tools.is_empty() {
            let (cleaned, claude_calls) = parse_claude_inline_tool_calls(&assistant_content);
            for call in claude_calls {
                accumulated_tools.push(ToolCallAccumulator {
                    id: format!("claude_inline_{}", accumulated_tools.len()),
                    name: call.name,
                    arguments: serde_json::to_string(&Value::Object(call.arguments))
                        .unwrap_or_else(|_| "{}".to_string()),
                });
            }
            // Take the cleaned prose even when no call survived: this parser
            // also drops complete reasoning spans, and leaving them in place
            // would let the residue stripper see a tag it cannot repair and
            // mis-flag a real answer as broken markup.
            assistant_content = cleaned;
        }

        // --- DSML (DeepSeek Markup Language) fallback parser ---
        // Some DeepSeek-family models emit tool calls as structured text using
        // the fullwidth-pipe delimiter U+FF5C instead of native function_call
        // deltas. Without this the loop sees "final answer, no tool calls"
        // and the worker makes zero progress. See `parse_dsml_tool_calls`.
        if accumulated_tools.is_empty() {
            let (cleaned, dsml_calls) = parse_dsml_tool_calls(&assistant_content);
            if !dsml_calls.is_empty() {
                for call in dsml_calls {
                    accumulated_tools.push(ToolCallAccumulator {
                        id: format!("dsml_{}", accumulated_tools.len()),
                        name: call.name,
                        arguments: serde_json::to_string(&call.arguments)
                            .unwrap_or_else(|_| "{}".to_string()),
                    });
                }
                assistant_content = cleaned;
            }
        }

        // --- Bare OpenAI-style JSON tool calls in content ---
        // Ollama's OpenAI-compatible shim returns some Qwen tool calls as text
        // content shaped like {"name": "...", "arguments": {...}} instead of
        // structured delta.tool_calls. None of the parsers above match that, so
        // without this the turn collapses to "final answer, no tool calls" and a
        // local-model mission stalls after one turn (LiteLLM crucible, phase 1,
        // 14B). Only objects whose `name` is a registered tool are recovered, so
        // a genuine JSON final answer is left untouched.
        if accumulated_tools.is_empty() {
            let (cleaned, json_calls) =
                parse_bare_json_tool_calls(&assistant_content, &known_tool_names);
            if !json_calls.is_empty() {
                for call in json_calls {
                    accumulated_tools.push(ToolCallAccumulator {
                        id: format!("bare_json_{}", accumulated_tools.len()),
                        name: call.name,
                        arguments: serde_json::to_string(&call.arguments)
                            .unwrap_or_else(|_| "{}".to_string()),
                    });
                }
                assistant_content = cleaned;
            }
        }

        // --- Fenced code block on a compile-repair turn ---
        // The repair directive demands the corrected file via write_file, and the
        // model does the repair but pastes it back as a ```rust block in prose
        // (measured: 10,558 chars, done_reason=stop, zero tool_calls). Recovery is
        // confined to a repair turn whose failing diagnostics named a file, so the
        // block's destination is known and cannot be guessed wrong — and a normal
        // turn that merely illustrates a snippet is never hijacked.
        if accumulated_tools.is_empty() && repair_this_turn {
            if let Some(rel) = compile_repair_target.as_deref() {
                if let Some(body) = extract_fenced_code_block(&assistant_content, 600) {
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "Model returned the fix as a fenced code block, not a tool call \u{2014} \
                             recovering it as a {} byte write to '{}'",
                            body.len(),
                            rel
                        )))
                        .ok();
                    accumulated_tools.push(ToolCallAccumulator {
                        id: format!("fenced_repair_{}", accumulated_tools.len()),
                        name: "write_file".to_string(),
                        arguments: serde_json::to_string(&json!({
                            "relativeFilePath": rel,
                            "content": body,
                        }))
                        .unwrap_or_else(|_| "{}".to_string()),
                    });
                }
            }
        }

        // Unparsed tool-call markup: a stream cut mid-call can leave residue
        // none of the parsers above consumed — e.g. the inline parser ate the
        // opener and function name, and only the lone function-close tag (plus
        // maybe a trailing call-close tag) survives. glm-5.2 did exactly this:
        // the turn's visible content collapsed to a bare close tag, the
        // announcement detector saw non-prose on its last line, and the loop
        // accepted it as a "final answer" one turn into the mission. Strip the
        // residue so the prose underneath is judged, and remember it was there:
        // a turn carrying markup can never be a deliberate final answer.
        let mut markup_residue = false;
        if accumulated_tools.is_empty() && !reasoning_content.is_empty() {
            // Qwen3-family models in Ollama emit a reasoning field even with
            // think: false. On turns where the model loses capacity it puts
            // the intended tool call in reasoning prose instead of the native
            // tool_calls channel. Attempt to recover structured calls from the
            // reasoning text using the same fragment and JSON parsers the
            // content channel already uses.
            let (frag_prose, frag_calls, _) = parse_function_fragments(&reasoning_content);
            if !frag_calls.is_empty() {
                for (fname, args) in frag_calls {
                    accumulated_tools.push(ToolCallAccumulator {
                        id: format!("reason_frag_{}", accumulated_tools.len()),
                        name: fname,
                        arguments: serde_json::to_string(&Value::Object(args))
                            .unwrap_or_else(|_| "{}".to_string()),
                    });
                }
                reasoning_content = frag_prose;
            }
        }
        if accumulated_tools.is_empty() {
            let (stripped, had_residue) = strip_tool_markup_residue(&assistant_content);
            if had_residue {
                markup_residue = true;
                assistant_content = stripped.into_owned();
            }
        }

        // DeepSeek-family routes sometimes stream the whole answer into the
        // reasoning channel and leave the content channel empty. A pure-empty
        // turn auto-continues anyway; giving it the real prose lets the model
        // see its own reasoning in history instead of a blank turn. The flag
        // keeps the emptiness visible even after the fallback fills content.
        let mut content_was_empty = false;
        if accumulated_tools.is_empty() && assistant_content.trim().is_empty() {
            content_was_empty = true;
            let reasoning_tail = reasoning_content.trim().to_string();
            if !reasoning_tail.is_empty() {
                assistant_content = reasoning_tail;
            }
        }

        let final_tool_calls_value = if !accumulated_tools.is_empty() {
            let tc_json: Vec<Value> = accumulated_tools
                .iter()
                .map(|t| {
                    json!({
                        "id": t.id,
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "arguments": t.arguments
                        }
                    })
                })
                .collect();
            Some(Value::Array(tc_json))
        } else {
            None
        };

        // When thinking mode is explicitly disabled (think: false sent to
        // Ollama), the model still produces a reasoning field as a Qwen3
        // implementation artifact. Storing it in history wrapped in
        // … forces the model to mirror that format on later turns,
        // which degrades native tool_calls into inline markup and causes
        // "unfinished tool markup" blocks. Skip the wrapper when thinking
        // was deliberately turned off for this provider.
        let final_saved_content = if !reasoning_content.is_empty() && current_thinking {
            format!(
                "<think>\n{}\n</think>\n{}",
                reasoning_content, assistant_content
            )
        } else {
            assistant_content.clone()
        };

        message_history.push(ChatMessage {
            role: "assistant".to_string(),
            content: final_saved_content,
            name: None,
            tool_call_id: None,
            tool_calls: final_tool_calls_value.clone(),
        });

        if let Some(account) = used_account {
            let tokens_out =
                estimate_tokens(&assistant_content) + estimate_tokens(&reasoning_content);
            usage_tracker.record_request(
                account.n,
                &account.label,
                &account.tier,
                tokens_in,
                tokens_out,
            );
            send_usage_update(usage_tracker, accounts, or_accounts, ui_tx);
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(format!(
                    "Using account: {} ({} req today)",
                    account.label,
                    usage_tracker
                        .build_views(accounts, or_accounts)
                        .iter()
                        .find(|v| v.n == account.n && v.label == account.label)
                        .map(|v| v.requests)
                        .unwrap_or(0)
                )))
                .ok();
        }

        if let Some(account) = used_or_account {
            let tokens_out =
                estimate_tokens(&assistant_content) + estimate_tokens(&reasoning_content);
            usage_tracker.record_or_request(
                account.n,
                &account.label,
                &account.tier,
                tokens_in,
                tokens_out,
            );
            send_usage_update(usage_tracker, accounts, or_accounts, ui_tx);
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(format!(
                    "Using OpenRouter: {} ({} req today)",
                    account.label,
                    usage_tracker
                        .build_views(accounts, or_accounts)
                        .iter()
                        .find(|v| v.label == account.label)
                        .map(|v| v.requests)
                        .unwrap_or(0)
                )))
                .ok();
        }

        if let Some(ref tcs) = final_tool_calls_value {
            let Some(tool_calls_arr) = tcs.as_array() else {
                ui_tx
                    .send(AgentToUiMessage::StatusUpdate(
                        "Unexpected tool_calls format from provider, skipping.".to_string(),
                    ))
                    .ok();
                // Fall through to break
                outcome = ending.stopped(
                    loop_count,
                    "unparseable tool_calls payload",
                    AgentLoopOutcome::Blocked {
                        reason: "provider returned an unparseable tool_calls payload".to_string(),
                    },
                );
                break;
            };

            let mut pending_ids = std::collections::HashSet::new();
            let mut tool_specs = Vec::new();
            let mut truncated_calls = 0usize;

            for tc in tool_calls_arr {
                let call_id = tc["id"].as_str().unwrap_or("").to_string();
                let tool_name = tc["function"]["name"].as_str().unwrap_or("").to_string();
                let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                let arguments: Value = match serde_json::from_str(args_str) {
                    Ok(v) => v,
                    Err(_) => {
                        // The stream died mid-arguments (socket timeout,
                        // provider drop). Executing a truncated payload as `{}`
                        // produced misleading tool errors and polluted history
                        // — reject the call and tell the model exactly why.
                        truncated_calls += 1;
                        ui_tx
                            .send(AgentToUiMessage::StatusUpdate(format!(
                                "Tool call '{}' had incomplete JSON arguments (stream truncated) \u{2014} not executed.",
                                tool_name
                            )))
                            .ok();
                        message_history.push(ChatMessage {
                            role: "tool".to_string(),
                            content: format!(
                                "Error: The arguments JSON for this '{}' call was incomplete or malformed (output likely cut off mid-stream) and the call was NOT executed. Re-issue the tool call with complete arguments. A write_file call replaces the whole file, so never send a fragment of one: if the content is too large for a single call, split it into several complete files (one module per file) and write each file in full.",
                                tool_name
                            ),
                            name: Some(tool_name.clone()),
                            tool_call_id: Some(call_id),
                            tool_calls: None,
                        });
                        continue;
                    }
                };

                // When the deliverable is overdue, this turn was issued with
                // only the file-editing tools exposed. Refuse to execute
                // exploration calls: a small reasoning model that keeps "just
                // checking one more thing" never converges, and running the
                // read/list only grows the context that is already drowning its
                // tool-calling. The rejection points back at the tools actually
                // advertised this turn — `turn_write_tools`, which is write_file
                // alone until a file exists to patch (verified against the live
                // MoE on the LiteLLM crucible).
                let write_names = turn_write_tools.join(" / ");
                if force_write_only
                    && read_of_named_target(&tool_name, &arguments, named_target.as_deref())
                {
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "Exploration locked \u{2014} allowing '{tool_name}' of the \
                             deliverable itself"
                        )))
                        .ok();
                }
                if force_write_only
                    && !turn_write_tools.contains(&tool_name.as_str())
                    && !read_of_named_target(&tool_name, &arguments, named_target.as_deref())
                {
                    // A model refused once may not ask again, so the refusal names
                    // the one read the lock still allows — the file it must edit.
                    let still_readable = if preexisting_target {
                        named_target
                            .as_deref()
                            .map(|t| {
                                format!(
                                    " One read is still allowed, and it is the one you need: \
                                     `read_file` with relativeFilePath \"{t}\", plus startLine \
                                     and endLine to bring back just the region the diagnostics \
                                     name."
                                )
                            })
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "Exploration locked \u{2014} refused '{}'; only {write_names} are \
                             available now.",
                            tool_name
                        )))
                        .ok();
                    message_history.push(ChatMessage {
                        role: "tool".to_string(),
                        content: format!(
                            "Error: Exploration tools are locked for this turn because the file \
                             this run must deliver is not yet on disk in the form the task asked \
                             for. The only tools available are {write_names} — call one NOW with \
                             the content built from what you have already seen. An incomplete \
                             file is acceptable and far better than no file.{still_readable}",
                        ),
                        name: Some(tool_name.clone()),
                        tool_call_id: Some(call_id),
                        tool_calls: None,
                    });
                    continue;
                }

                ui_tx
                    .send(AgentToUiMessage::StatusUpdate(format!(
                        "Requesting approval for tool: {}",
                        tool_name
                    )))
                    .ok();
                ui_tx
                    .send(AgentToUiMessage::RequestToolApproval {
                        id: call_id.clone(),
                        tool_name: tool_name.clone(),
                        arguments: arguments.clone(),
                    })
                    .ok();

                pending_ids.insert(call_id.clone());
                tool_specs.push((call_id, tool_name, arguments));
            }

            if tool_specs.is_empty() && truncated_calls > 0 {
                // Every call in the batch was rejected for truncated
                // arguments. Don't end the run — record the gap honestly and
                // give the model another turn to re-issue them.
                write_handover_nda(
                    workspace_root,
                    "tool_error",
                    loop_count,
                    "truncated tool-call arguments",
                    false,
                );
                save_chatlogs_nda(workspace_root, message_history);
                continue;
            }

            let mut resolved_approvals = std::collections::HashMap::new();

            while !pending_ids.is_empty() {
                if apply_headless_control_messages(cancel_rx, message_history, ui_tx, progress) {
                    pending_ids.clear();
                    break;
                }
                if let Ok(ui_msg) = ui_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                    match ui_msg {
                        UiToAgentMessage::ApproveTool { id, arguments } => {
                            if pending_ids.remove(&id) {
                                resolved_approvals.insert(id, Some(arguments));
                            }
                        }
                        UiToAgentMessage::RejectTool { id } => {
                            if pending_ids.remove(&id) {
                                resolved_approvals.insert(id, None);
                            }
                        }
                        UiToAgentMessage::CancelTask => {
                            pending_ids.clear();
                            ui_tx
                                .send(AgentToUiMessage::StatusUpdate(
                                    "Task interrupted by operator.".to_string(),
                                ))
                                .ok();
                            break;
                        }
                        other => {
                            deferred_messages.push(other);
                        }
                    }
                } else if apply_headless_control_messages(
                    cancel_rx,
                    message_history,
                    ui_tx,
                    progress,
                ) {
                    pending_ids.clear();
                    break;
                }
            }

            // Phase 2: Checkpoint before file-modifying tool batch
            let file_modifying = ["write_file", "edit_file", "delete_file", "run_command"];
            let has_file_mod = tool_specs
                .iter()
                .any(|(_, name, _)| file_modifying.contains(&name.as_str()));
            // run_command is "file-modifying" for checkpointing, but an agent that
            // only re-rames cargo test is still auditing. The write-pressure streak
            // counts only turns whose batch actually edited files.
            let has_write = tool_specs.iter().any(|(_, name, _)| {
                FILE_EDITING_TOOLS.contains(&name.as_str()) || name == "delete_file"
            });
            if has_file_mod && checkpoint_mgr.enabled {
                let label = format!("before tool batch (loop {})", loop_count);
                if let Some(cp_id) = checkpoint_mgr.checkpoint(&label) {
                    last_checkpoint_id = Some(cp_id);
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "Checkpoint #{} created: {}",
                            cp_id, label
                        )))
                        .ok();
                }
            }

            // What each pending call carries, keyed by its id. The threads below
            // consume the arguments, and the results loop still has to recognise a
            // replay of a call an earlier turn already refused.
            let call_identities: Vec<(String, String)> = tool_specs
                .iter()
                .map(|(id, name, args)| (id.clone(), call_identity(name, args)))
                .collect();

            let mut handles = Vec::new();
            let bus_clone = coordination_bus.clone();
            let lsp_written_clone = lsp_written_files.clone();
            // The deliverable's opening surface, so a whole-file write that would
            // gut it is refused before it lands instead of graded afterwards.
            let surface_target = artifact_surface.target.clone();
            let surface_names = artifact_surface.names.clone();
            // The path the exploration lock still lets through, so a refusal can
            // name it instead of ordering an edit built from text never seen.
            let lock_readable = named_target.clone();

            for (call_id, tool_name, _original_arguments) in tool_specs {
                let approval = resolved_approvals.get(&call_id).cloned().flatten();
                let workspace_root_clone = workspace_root.clone();
                let ui_tx_clone = ui_tx.clone();
                let bus = bus_clone.clone();
                let lsp_written = lsp_written_clone.clone();
                let attribution = agent_attribution.clone();
                let write_governor = write_governor.clone();
                let surface_target = surface_target.clone();
                let surface_names = surface_names.clone();
                let lock_readable = lock_readable.clone();

                let handle = std::thread::spawn(move || {
                    let (tool_result, file_buffer_update, changelog_entry) = if let Some(
                        approved_args,
                    ) = approval
                    {
                        // Schema gate: reject a call whose arguments violate the
                        // tool's declared contract before it runs. Executing
                        // `write_file` with blank `content`, or a call missing a
                        // required parameter, otherwise produces a broken artifact
                        // that only the compiler catches turns later.
                        if let Some(reason) = validate_tool_call(&tool_name, &approved_args) {
                            return (call_id, tool_name, reason, None, None);
                        }

                        // Surface gate: a whole-file write that would delete most
                        // of the deliverable's items is refused here, while the
                        // items are still on disk to be edited. Afterwards the
                        // guard can only name them, and naming is not restoring.
                        if let Some(reason) = gutting_rewrite_refusal(
                            &tool_name,
                            &approved_args,
                            surface_target.as_deref(),
                            &surface_names,
                            lock_readable.as_deref(),
                        ) {
                            return (call_id, tool_name, reason, None, None);
                        }

                        ui_tx_clone
                            .send(AgentToUiMessage::ToolExecutionStarted {
                                tool_name: tool_name.clone(),
                            })
                            .ok();

                        // T1b: Claim file via coordination bus before writing
                        let file_to_lock: Option<PathBuf> = match tool_name.as_str() {
                            "write_file" | "edit_file" | "delete_file" => approved_args
                                ["relativeFilePath"]
                                .as_str()
                                .or_else(|| approved_args["path"].as_str())
                                .map(|p| workspace_root_clone.join(p)),
                            _ => None,
                        };
                        if let Some(ref lock_path) = file_to_lock {
                            // System C §6.4: consult org RBAC before taking the edit
                            // lock. Only a governed workspace yields `Some`; the denial
                            // is already audit-stamped by `require_write` (INV-6).
                            if let Some((governor, principal)) = write_governor.as_ref() {
                                if let Err(reason) = governor.require_write(principal) {
                                    return (
                                        call_id,
                                        tool_name,
                                        format!("Error: {reason}"),
                                        None,
                                        None,
                                    );
                                }
                            }
                            if !bus.claim_file("primary", lock_path) {
                                return (
                                    call_id,
                                    tool_name,
                                    format!(
                                        "Error: File '{}' is locked by another agent.",
                                        lock_path.display()
                                    ),
                                    None,
                                    None,
                                );
                            }
                        }

                        // T2b: LSP gate — if this file was already written this session
                        // and build diagnostics still report errors referencing it,
                        // block the write to force the agent to fix errors first.
                        if let Some(ref lock_path) = file_to_lock {
                            if lsp_written.lock_safe().contains(lock_path) {
                                let diag = crate::automation::read_latest_diagnostics(
                                    &workspace_root_clone,
                                );
                                // A skipped check (no Rust project) has nothing to gate on.
                                if !diag.success && !diag.skipped {
                                    let file_str = lock_path.display().to_string();
                                    let rel_str = lock_path
                                        .strip_prefix(&workspace_root_clone)
                                        .map(|p| p.display().to_string())
                                        .unwrap_or_default();
                                    let has_file_errors = diag
                                        .errors
                                        .iter()
                                        .any(|e| e.contains(&file_str) || e.contains(&rel_str));
                                    if has_file_errors {
                                        let relevant: Vec<&String> = diag
                                            .errors
                                            .iter()
                                            .filter(|e| {
                                                e.contains(&file_str) || e.contains(&rel_str)
                                            })
                                            .take(5)
                                            .collect();
                                        return (
                                            call_id,
                                            tool_name,
                                            format!(
                                                "BLOCKED: '{}' has unresolved compilation errors from your previous write. \
                                                    Fix these errors before writing again:\n{}",
                                                rel_str,
                                                relevant
                                                    .iter()
                                                    .map(|e| format!("  {}", e))
                                                    .collect::<Vec<_>>()
                                                    .join("\n")
                                            ),
                                            None,
                                            None,
                                        );
                                    }
                                }
                            }
                        }

                        let mut file_buffer_update = None;
                        let mut changelog_entry = None;
                        // Whether the tool ran without error — gates System-B
                        // attribution so a failed write is never logged as a change.
                        let mut tool_ok = false;
                        let tool_result = match registry::call_tool_in_workspace(
                            &workspace_root_clone,
                            &tool_name,
                            &approved_args,
                        ) {
                            Ok(res) => {
                                tool_ok = true;
                                if tool_name == "write_file" {
                                    if let Some(rel_path) =
                                        approved_args["relativeFilePath"].as_str()
                                    {
                                        let full_path = workspace_root_clone.join(rel_path);
                                        if let Some(content) = approved_args["content"].as_str() {
                                            file_buffer_update =
                                                Some((full_path, content.to_string()));
                                        }
                                        changelog_entry =
                                            Some((rel_path.to_string(), "write_file"));
                                    }
                                } else if tool_name == "edit_file" {
                                    if let Some(rel_path) =
                                        approved_args["relativeFilePath"].as_str()
                                    {
                                        let full_path = workspace_root_clone.join(rel_path);
                                        // An edit has no `content` argument to hand
                                        // over, and the edited bytes are already on
                                        // disk — read them so the editor buffer shows
                                        // the file as it now stands, not pre-edit.
                                        if let Ok(content) = std::fs::read_to_string(&full_path) {
                                            file_buffer_update = Some((full_path, content));
                                        }
                                        changelog_entry = Some((rel_path.to_string(), "edit_file"));
                                    }
                                }
                                res
                            }
                            Err(e) => {
                                // A hallucinated tool name deserves better feedback
                                // than `ToolNotFound("patch_file")`: the observed run
                                // burned two separate turns guessing the same missing
                                // tool. Naming the closest registered tools lets the
                                // model self-correct on the very next call.
                                if let Some(crate::errors::ToolError::ToolNotFound(missing)) =
                                    e.downcast_ref::<crate::errors::ToolError>()
                                {
                                    let all_names: Vec<String> =
                                        registry::tool_definitions::get_tools()
                                            .iter()
                                            .map(|t| t.name.clone())
                                            .collect();
                                    unknown_tool_message(missing, &all_names)
                                } else {
                                    // `{e:#}` keeps anyhow's cause chain visible
                                    // while printing the message the way its
                                    // author wrote it. `{:?}` escaped newlines and
                                    // wrapped everything in quotes, so a
                                    // multi-line "which parameter was wrong and
                                    // why" arrived as one unreadable blob — the
                                    // model resents what it cannot parse.
                                    format!("Error executing tool: {e:#}")
                                }
                            }
                        };

                        // T1b: Release file lock after execution
                        if let Some(ref lock_path) = file_to_lock {
                            bus.release_file("primary", lock_path);
                            // T2b: Track written files for LSP gating
                            if FILE_EDITING_TOOLS.contains(&tool_name.as_str()) {
                                lsp_written.lock_safe().insert(lock_path.clone());
                                // System B: attribute the agent's write into the
                                // durable op-log, keyed by the on-disk content digest
                                // (identical to what the AST watcher will compute), so
                                // the watcher's later echo is deduped by the shared log
                                // and the change is credited to the agent, not a human.
                                if tool_ok {
                                    if let Ok(content) = std::fs::read_to_string(lock_path) {
                                        let rel = lock_path
                                            .strip_prefix(&workspace_root_clone)
                                            .unwrap_or(lock_path.as_path())
                                            .to_string_lossy()
                                            .replace('\\', "/");
                                        let digest =
                                            crate::automation::ChangeAttributor::digest_of_content(
                                                &content,
                                            );
                                        let intent = approved_args["context"]
                                            .as_str()
                                            .unwrap_or("agent write");
                                        let _ =
                                            attribution.lock_safe().record(&rel, &digest, intent);
                                    }
                                }
                            }
                        }

                        ui_tx_clone
                            .send(AgentToUiMessage::ToolExecutionFinished {
                                tool_name: tool_name.clone(),
                                result: tool_result.clone(),
                            })
                            .ok();

                        (tool_result, file_buffer_update, changelog_entry)
                    } else {
                        (
                            "Error: Tool execution rejected by the user.".to_string(),
                            None,
                            None,
                        )
                    };

                    (
                        call_id,
                        tool_name,
                        tool_result,
                        file_buffer_update,
                        changelog_entry,
                    )
                });

                handles.push(handle);
            }

            let mut thread_results = Vec::new();
            for h in handles {
                if let Ok(res) = h.join() {
                    thread_results.push(res);
                }
            }

            // Turn latency is now visible: model time versus tool time.
            {
                let total_ms = turn_start.elapsed().as_millis();
                let tool_ms = total_ms.saturating_sub(provider_ms);
                ui_tx
                    .send(AgentToUiMessage::StatusUpdate(format!(
                        "Turn {}: model {:.1}s + tools {:.1}s ({} call(s))",
                        loop_count,
                        provider_ms as f64 / 1000.0,
                        tool_ms as f64 / 1000.0,
                        thread_results.len()
                    )))
                    .ok();
            }

            let mut any_success = false;
            let mut any_rejected = false;
            let mut any_error = false;
            // Rust files this batch put on disk. The write-side gate keys off
            // them so it never re-judges a file an earlier turn already gated,
            // and off nothing else: an all-or-nothing batch that only refused
            // left no new code behind, so there is nothing to check.
            let mut batch_rust_writes: Vec<String> = Vec::new();

            for (call_id, tool_name, tool_result, file_buffer_update, changelog_entry) in
                thread_results
            {
                let refused = is_refusal(&tool_result);
                if refused {
                    any_error = true;
                    // Phase 3: Penalize failed strategy
                    let mem_key = format!("tool:{}:error", tool_name);
                    memory.reinforce(&mem_key, -0.1);
                    // Phase 4: Record failure for self-improvement analysis
                    improve_engine.record_failure(&tool_name, &tool_result, loop_count);
                } else if tool_result.contains("rejected by the user") {
                    any_rejected = true;
                    improve_engine.record_failure(&tool_name, "rejected by the user", loop_count);
                } else {
                    any_success = true;
                    // A write that actually ran is the deliverable an artifact
                    // task owes. delete_file is deliberately excluded: removing a
                    // file is a change, not the file the task promised to create.
                    // And for a task that names a `.rs` target, neither is a
                    // script or a note — a scratch `run_check.sh` is not the port,
                    // and counting it as one releases the exploration lock the run
                    // still needs. `produced_artifact` also decides the
                    // blocked-vs-completed verdict below, so over-counting a junk
                    // file reports a deliverable that was never graded. And a task
                    // that names the file it owes gets no credit for a write that
                    // landed somewhere else — see `write_misses_named_target`.
                    if FILE_EDITING_TOOLS.contains(&tool_name.as_str()) {
                        let written = changelog_entry.as_ref().map(|(rel, _)| rel.as_str());
                        produced_artifact |= counts_as_artifact(written, artifact_is_rust)
                            && !write_misses_named_target(written, named_target.as_deref());
                        if let Some((rel, _)) = changelog_entry.as_ref() {
                            last_write_path = Some(rel.clone());
                            if rel.replace('\\', "/").ends_with(".rs") {
                                batch_rust_writes.push(rel.clone());
                                remember_run_write(&mut run_rust_writes, rel);
                            }
                        }
                    }
                    // Phase 3: Remember successful tool usage
                    let mem_key = format!("tool:{}:success", tool_name);
                    // Byte 200 can land inside a multi-byte character (tool
                    // results carry file content from real projects), which
                    // panics the whole loop — snap to a char boundary.
                    let mut cut = 200.min(tool_result.len());
                    while cut > 0 && !tool_result.is_char_boundary(cut) {
                        cut -= 1;
                    }
                    let summary = &tool_result[..cut];
                    memory.remember(
                        &mem_key,
                        &format!("{} -> {}", tool_name, summary),
                        &["tool", "success"],
                        0.8,
                    );
                    // Phase 4: Record success for ratio tracking
                    improve_engine.record_success();
                }

                if let Some((path, content)) = file_buffer_update {
                    ui_tx
                        .send(AgentToUiMessage::UpdateFileBuffer { path, content })
                        .ok();
                }
                if let Some((rel_path, action)) = changelog_entry {
                    append_changelog_nda(workspace_root, &rel_path, action);
                    sitemap_needed = true;
                }

                // A replay is invisible to every other repeat check in this loop:
                // they all key off compiler diagnostics, and a call the tool
                // refused never produced one. `lr7b_v12` spent four turns on one
                // `edit_file` whose `oldString` matched two places, each turn
                // handed the same two-site explanation and each turn sending the
                // same bytes back — so from the second refusal on, say that the
                // repetition is itself the problem.
                let replay = call_identities
                    .iter()
                    .find(|(id, _)| id == &call_id)
                    .and_then(|(_, identity)| {
                        note_refusal_repeat(&mut refused_call_counts, identity, refused)
                    });
                let outgoing = match replay {
                    Some(times) => {
                        ui_tx
                            .send(AgentToUiMessage::StatusUpdate(format!(
                                "'{tool_name}' refused the identical call {times} times \u{2014} \
                                 told the model it is a replay"
                            )))
                            .ok();
                        let note = repeated_refusal_note(times, &tool_result);
                        tool_result + &note
                    }
                    None => tool_result,
                };

                message_history.push(ChatMessage {
                    role: "tool".to_string(),
                    content: bound_listing_output(
                        &tool_name,
                        outgoing,
                        requires_artifact,
                        current_profile.supports_thinking,
                    ),
                    name: Some(tool_name.clone()),
                    tool_call_id: Some(call_id),
                    tool_calls: None,
                });
            }

            if any_error {
                // Phase 2: a batch that produced errors and no successes left the
                // workspace in a bad state (e.g. a failed run_command with partial
                // side effects). Roll back to the pre-batch checkpoint so broken
                // changes don't accumulate across loops.
                if !any_success {
                    if let Some(cp_id) = last_checkpoint_id.take() {
                        match checkpoint_mgr.restore(cp_id) {
                            Ok(()) => {
                                ui_tx
                                    .send(AgentToUiMessage::StatusUpdate(format!(
                                        "Batch failed \u{2014} rolled back to checkpoint #{}.",
                                        cp_id
                                    )))
                                    .ok();
                            }
                            Err(e) => {
                                ui_tx
                                    .send(AgentToUiMessage::StatusUpdate(format!(
                                        "Rollback to checkpoint #{} failed: {}",
                                        cp_id, e
                                    )))
                                    .ok();
                            }
                        }
                    }
                }
                write_handover_nda(
                    workspace_root,
                    "tool_error",
                    loop_count,
                    "tool error in batch",
                    false,
                );
            } else if any_rejected {
                write_handover_nda(
                    workspace_root,
                    "tool_rejected",
                    loop_count,
                    "user reject in batch",
                    false,
                );
            } else if any_success {
                write_handover_nda(workspace_root, "executing", loop_count, "batch ok", false);
            }

            turns_since_write = if has_write { 0 } else { turns_since_write + 1 };
            if should_nudge_write_pressure(
                turns_since_write,
                write_pressure_nudges,
                loop_count + 1 < max_loops,
            ) {
                write_pressure_nudges += 1;
                turns_since_write = 0;
                ui_tx
                    .send(AgentToUiMessage::StatusUpdate(format!(
                        "No file edits for {} consecutive turns \u{2014} nudging implementation ({}/{})...",
                        WRITE_PRESSURE_STREAK, write_pressure_nudges, MAX_WRITE_PRESSURE_NUDGES
                    )))
                    .ok();
                message_history.push(ChatMessage {
                    role: "user".to_string(),
                    content: write_pressure_directive(advertised_editors),
                    name: None,
                    tool_call_id: None,
                    tool_calls: None,
                });
            }

            save_chatlogs_nda(workspace_root, message_history);

            // An armed repair turn whose calls were all refused moved no file, so
            // the attempt charged when the directive went out was never spent: the
            // gate below grades writes, and there was no write to grade. Which
            // counter gives it back is read off the report still standing, because
            // every route that arms a repair leaves its text there.
            if should_refund_repair_attempt(
                repair_this_turn,
                !batch_rust_writes.is_empty(),
                any_error,
                refunded_repair_attempts,
            ) {
                refunded_repair_attempts += 1;
                let standing = write_gate_diagnostics.as_deref().unwrap_or_default();
                let (max, is_test) = repair_budget(standing);
                let attempts = if is_test {
                    &mut test_repair_attempts
                } else {
                    &mut compile_repair_attempts
                };
                *attempts = attempts.saturating_sub(1);
                ui_tx
                    .send(AgentToUiMessage::StatusUpdate(format!(
                        "{} repair turn changed no file \u{2014} attempt returned ({}/{max}, {} \
                         refund(s) left)",
                        gate_failure_label(standing),
                        *attempts,
                        MAX_REFUNDED_REPAIR_ATTEMPTS - refunded_repair_attempts
                    )))
                    .ok();
            }

            // Gate the write itself, not only the turn that follows a pause.
            //
            // The completion gate below sits on the *no tool call* branch, so a
            // model that keeps working after a broken write is never told: on the
            // run-8 gate probe the repair at turn 14 left rustc's E0594 standing,
            // turns 15 and 16 were spent re-reading the file the compiler had
            // already rejected, and the run died on its turn budget with the last
            // repair attempt never used. An error the loop already has should
            // reach the model on the turn that made it.
            if !batch_rust_writes.is_empty() {
                let written: Vec<&str> = run_rust_writes.iter().map(String::as_str).collect();
                // A build break is visible on the turn that causes it; an
                // assertion is not, because `run_post_write_check` stops at
                // `cargo check --all-targets`. So the crate's tests were only
                // ever run when the model PAUSED — and testgate4 paid for that:
                // repair 3/3 landed an `edit_file` that traded one red assertion
                // for another, the loop asked for another turn anyway, the model
                // went exploring with `grep_search` (93s model + 104s tool), and
                // the wall clock died before anything could tell it the repair
                // had not worked. Once the test gate has rejected the artifact,
                // grade every repair on the same terms, the turn it lands.
                let build = run_post_write_check(workspace_root, &written);
                // Consult the crate's tests as soon as the build they run on is
                // clean. `tests_ran` is the whole rule; see it for why the gate
                // used to wait for a rejection first.
                let tests =
                    if should_grade_tests_now(build.is_ok(), test_gate_engaged, preexisting_target)
                    {
                        Some(run_written_tests(workspace_root, &written))
                    } else {
                        None
                    };
                match grade_write(build, tests) {
                    // The mission is genuinely finished: clean build and green
                    // tests. Waiting for a no-tool-call turn to notice only risks
                    // another "improvement" to a file that already works.
                    WriteGrade::Validated(summary) => {
                        // A green grade is a verdict on the CRATE. It says
                        // nothing about the file the task owes, and crucible run
                        // `convert22` measured the gap: the model wrote the whole
                        // 32,920-byte port to
                        // 'litellm_ws/rust_port/src/budget_manager.rs' — a legal
                        // workspace-relative path that nested a second copy of
                        // the workspace inside itself — while the file the task
                        // named stayed at 0 bytes. An empty `pub mod
                        // budget_manager;` builds clean and the crate's 52 tests
                        // from other modules pass, so the run ended reporting
                        // "Repair validated — 52 passed" over a deliverable that
                        // was never written. Withhold the grade, name both paths,
                        // and bound the steering with a budget of its own:
                        // sharing `MAX_ARTIFACT_NUDGES` meant a model chased for
                        // writing nothing and then for writing in the wrong place
                        // spent one cap on two different failures.
                        if writes_miss_named_target(named_target.as_deref(), &run_rust_writes) {
                            let target = named_target.as_deref().unwrap_or_default();
                            let directive =
                                wrong_path_directive(target, &run_rust_writes, turn_write_tools);
                            if should_steer_wrong_path(off_target_refusals) {
                                off_target_refusals += 1;
                                ui_tx
                                    .send(AgentToUiMessage::StatusUpdate(format!(
                                        "The crate builds and its tests pass, but no write landed \
                                         on '{}' ({}/{}) — not accepting this as the deliverable",
                                        target, off_target_refusals, MAX_OFF_TARGET_REFUSALS
                                    )))
                                    .ok();
                                write_gate_diagnostics = Some(directive.clone());
                                pending_repair_directive = Some(directive.clone());
                                message_history.push(ChatMessage {
                                    role: "user".to_string(),
                                    content: directive,
                                    name: None,
                                    tool_call_id: None,
                                    tool_calls: None,
                                });
                            } else {
                                let reason = off_target_refusal(Some(target), &run_rust_writes)
                                    .unwrap_or_default();
                                ui_tx
                                    .send(AgentToUiMessage::StatusUpdate(format!(
                                        "Deliverable is NOT validated — after {} steering turn(s), \
                                         {reason}",
                                        off_target_refusals
                                    )))
                                    .ok();
                                outcome = ending.stopped(
                                    loop_count,
                                    "artifact writes landed outside the path the task names",
                                    AgentLoopOutcome::Blocked { reason },
                                );
                                break;
                            }
                        }
                        // rustc and the test harness both go quiet when the code
                        // they were complaining about is deleted, so a clean grade
                        // is only accepted once the deliverable still declares the
                        // items it arrived with.
                        else if let Some(report) =
                            artifact_surface.withheld_report(workspace_root)
                        {
                            ui_tx
                                .send(AgentToUiMessage::StatusUpdate(format!(
                                    "The build is clean, but the write bought it by deleting \
                                     items the artifact started with ({}/{}) — not accepting \
                                     this as fixed",
                                    artifact_surface.refusals, MAX_SURFACE_REFUSALS
                                )))
                                .ok();
                            write_gate_diagnostics = Some(report.clone());
                            pending_repair_directive = Some(report.clone());
                            message_history.push(ChatMessage {
                                role: "user".to_string(),
                                content: report,
                                name: None,
                                tool_call_id: None,
                                tool_calls: None,
                            });
                        } else if let Some(reason) = artifact_surface.collapse(workspace_root) {
                            // Out of refusal budget is not the same as correct.
                            // The guard stops interrupting so a real refactor
                            // cannot be held hostage, but `lr7b_v6b` showed what
                            // that costs when the model is not refactoring: it
                            // wrote a 495-byte file over a 26 KB port, the crate
                            // built, the 53 tests that survived passed, and the
                            // run reported a validated repair. A deliverable
                            // that lost most of what it arrived with did not
                            // complete the task, whatever the compiler says.
                            ui_tx
                                .send(AgentToUiMessage::StatusUpdate(format!(
                                    "Deliverable is NOT validated — {reason}"
                                )))
                                .ok();
                            outcome = ending.stopped(
                                loop_count,
                                "a clean build bought by deleting the artifact's items",
                                AgentLoopOutcome::Blocked { reason },
                            );
                            break;
                        } else {
                            // Whether or not the refusal budget still has room, the
                            // caller is told what the artifact no longer contains:
                            // a test tally quoted over a smaller file is an
                            // over-claim, and this is the loop's own success line.
                            let summary = format!(
                                "{summary}{}",
                                artifact_surface.shrink_note(workspace_root)
                            );
                            ui_tx
                                .send(AgentToUiMessage::StatusUpdate(format!(
                                    "Repair validated — {summary}"
                                )))
                                .ok();
                            // Only an artifact dispatch is done when the files are:
                            // nobody is waiting for prose, so ending here saves the
                            // turn testgate4 spent exploring a finished crate. An
                            // interactive run still owes the user an explanation, so
                            // report the verdict and let the model take the turn the
                            // completion gate used to end anyway.
                            if requires_artifact {
                                outcome = ending.completed(
                                    loop_count,
                                    &format!("compiler and tests validated; {summary}"),
                                );
                                break;
                            }
                            write_gate_diagnostics = None;
                        }
                    }
                    // The gate abstained (`cargo test` could not launch, or was
                    // killed). Say so rather than implying a re-run happened.
                    WriteGrade::NotJudged(why) => {
                        ui_tx
                            .send(AgentToUiMessage::StatusUpdate(format!(
                                "Tests not re-run after your write: {why}"
                            )))
                            .ok();
                        write_gate_diagnostics = None;
                    }
                    WriteGrade::Repair(e) => {
                        let checked = if is_test_failure_report(&e) {
                            "Tests"
                        } else {
                            "Build status"
                        };
                        let changed = write_gate_diagnostics.as_deref() != Some(e.as_str());
                        write_gate_diagnostics = Some(e.clone());
                        // A directive belongs to the report it was built from. Drop
                        // the previous one here, so a rejection this turn merely
                        // displays cannot inherit advice about last turn's error.
                        pending_repair_directive = None;
                        ui_tx
                            .send(AgentToUiMessage::StatusUpdate(format!(
                                "{checked} after your write: {e}"
                            )))
                            .ok();
                        let (max, is_test) = repair_budget(&e);
                        let attempts = if is_test {
                            &mut test_repair_attempts
                        } else {
                            &mut compile_repair_attempts
                        };
                        if should_arm_write_repair(
                            *attempts < max,
                            changed,
                            prior_encounters(&e, &repair_error_history, Some(workspace_root)) < 2,
                        ) {
                            arm_compile_repair(
                                &e,
                                workspace_root,
                                last_write_path.as_deref(),
                                attempts,
                                max,
                                &mut compile_repair_pending,
                                &mut pending_repair_directive,
                                &mut compile_repair_target,
                                &mut repair_error_history,
                                message_history,
                                ui_tx,
                            );
                        } else if should_note_unarmed_repair(changed, unarmed_repair_notes) {
                            // No repair TURN is armed — the budget is spent or this
                            // error class has already come back twice — but the loop
                            // still holds a rustc report the model has never seen.
                            // repair20 measured what silence costs: turn 9's write left
                            // E0107 standing, the counter already read 3/3, and turns
                            // 10-16 spent themselves grepping and re-reading the file the
                            // compiler had rejected, ending the run one error deep with
                            // the model never told. The exploration lock now holds for
                            // as long as this report stands, so the only calls the next
                            // turns can make are edits — which makes showing the error,
                            // not saving the turn, the whole of the repair.
                            unarmed_repair_notes += 1;
                            ui_tx
                                .send(AgentToUiMessage::StatusUpdate(format!(
                                    "{} repair turns used — showing the diagnostics again \
                                     without arming a repair turn ({}/{})",
                                    gate_failure_label(&e),
                                    unarmed_repair_notes,
                                    MAX_UNARMED_REPAIR_NOTES
                                )))
                                .ok();
                            let directive = repair_directive_for_model(
                                &e,
                                workspace_root,
                                &recurring_error_note(
                                    &e,
                                    &repair_error_history,
                                    Some(workspace_root),
                                ),
                            );
                            // Held as armed state too: a later lean rebuild deletes
                            // this message, and the report it fixes stays standing.
                            pending_repair_directive = Some(directive.clone());
                            message_history.push(ChatMessage {
                                role: "user".to_string(),
                                content: directive,
                                name: None,
                                tool_call_id: None,
                                tool_calls: None,
                            });
                        }
                    }
                    WriteGrade::BuildClean => write_gate_diagnostics = None,
                }
            }
        } else if sitemap_needed {
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(
                    "Running automatic compilation validation...".to_string(),
                ))
                .ok();
            let gate_written: Vec<&str> = run_rust_writes.iter().map(String::as_str).collect();
            // A clean `cargo check --all-targets` proves the code and its test
            // code COMPILE. That is not what a port is graded on: a probe run
            // ended with "Compiler validation succeeded!" over an artifact whose
            // `cargo test` reported 68 passed / 2 failed. Run the tests, and only
            // after the build is green — a crate that does not compile has no
            // test binary, and its rustc diagnostics are the thing to repair.
            let verdict = validate_written_artifact(
                workspace_root,
                &gate_written,
                &mut artifact_surface,
                ui_tx,
            );
            match verdict {
                Ok(test_note) => {
                    // Steering happened (or could not) already: this is an exit
                    // site, so a green crate whose deliverable path was never
                    // written reports a refusal instead of a completion.
                    if let Some(reason) =
                        off_target_refusal(named_target.as_deref(), &run_rust_writes)
                    {
                        ui_tx
                            .send(AgentToUiMessage::StatusUpdate(format!(
                                "Deliverable is NOT validated — {reason}"
                            )))
                            .ok();
                        outcome = ending.stopped(
                            loop_count,
                            "artifact writes landed outside the path the task names",
                            AgentLoopOutcome::Blocked { reason },
                        );
                        break;
                    }
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "Compiler validation succeeded — {test_note}"
                        )))
                        .ok();
                    // A green post-run build is a finished mission even though
                    // the last turn made no tool call.
                    outcome =
                        ending.completed(loop_count, &format!("compiler validated; {test_note}"));
                    break;
                }
                Err(e) => {
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "{}: {e}",
                            gate_failure_label(&e)
                        )))
                        .ok();
                    // Remember WHY the crate was rejected, so the write-side gate
                    // can keep grading on the same terms. A build break is
                    // already caught the turn it lands; a red assertion is not,
                    // because `run_post_write_check` stops at `--all-targets`.
                    if is_test_failure_report(&e) {
                        test_gate_engaged = true;
                    }
                    // The deliverable was written but does not compile, or it
                    // compiles and its own tests reject it. Feed the exact
                    // diagnostics back and let the model issue a corrected write
                    // before giving up — this is the difference between a port
                    // with 2-3 fixable slips and a blocked run.
                    let (max, is_test) = repair_budget(&e);
                    let attempts = if is_test {
                        &mut test_repair_attempts
                    } else {
                        &mut compile_repair_attempts
                    };
                    if loop_count < max_loops
                        && arm_compile_repair(
                            &e,
                            workspace_root,
                            last_write_path.as_deref(),
                            attempts,
                            max,
                            &mut compile_repair_pending,
                            &mut pending_repair_directive,
                            &mut compile_repair_target,
                            &mut repair_error_history,
                            message_history,
                            ui_tx,
                        )
                    {
                        // Consume the turn (no rollback): each repair is a real
                        // model call, so budget exhaustion — not free retries —
                        // bounds this loop.
                        continue;
                    }
                    outcome = ending.stopped(
                        loop_count,
                        "post-run validation failed",
                        AgentLoopOutcome::Blocked {
                            reason: format!("post-run validation failed: {e}"),
                        },
                    );
                    break;
                }
            }
        } else {
            // This turn produced no tool call, so everything it cost is progress
            // the run did not make. Counted before the decision so a model that
            // stalls on planning turns is stopped by cost, not only by count.
            wasted_turn_secs += provider_ms as u64 / 1000;
            match decide_no_tool_call_ending(
                markup_residue,
                content_was_empty,
                &assistant_content,
                auto_continuations,
                wasted_turn_secs,
                budget_secs,
            ) {
                NoToolCallEnding::Stalled => {
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "Turn {}: {:.0}s of the {}s budget spent on turns that announced an action and made no tool call — stopping rather than trying again, so the rest goes to the next attempt",
                            loop_count, wasted_turn_secs, budget_secs
                        )))
                        .ok();
                    outcome = ending.stopped(
                        loop_count,
                        "planning turns with no tool call exhausted the time budget",
                        AgentLoopOutcome::Blocked {
                            reason: format!(
                                "{} turn(s) announced an action without making a tool call, costing {}s of a {}s budget",
                                auto_continuations + 1,
                                wasted_turn_secs,
                                budget_secs
                            ),
                        },
                    );
                    break;
                }
                NoToolCallEnding::Nudge => {
                    // The model ended its turn with an announcement ("Let me
                    // fix the test:") or an empty response instead of a tool
                    // call or a final summary. Historically this surfaced as
                    // "Agent finished" with the mission half-done. Nudge it to
                    // act, without user intervention. markup_residue covers the
                    // sneakier variant: the turn carried tool-call markup that
                    // no parser could execute — the model INTENDED to act, so
                    // ending the run here is never correct.
                    auto_continuations += 1;
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "Turn announced an action but made no tool call — auto-continuing ({}/{})...",
                            auto_continuations, MAX_AUTO_CONTINUATIONS
                        )))
                        .ok();
                    let notice = if markup_residue {
                        "[System notice] Your previous response contained a tool call whose markup arrived truncated or malformed, so it could not be executed and nothing happened. Re-issue that tool call now as a clean, complete call with valid arguments.".to_string()
                    } else {
                        "[System notice] Your previous response announced an action but contained no tool call, so nothing was executed and the turn ended. If work remains, make the tool calls now; if the mission is truly complete, reply with your final summary instead of another announcement.".to_string()
                    };
                    message_history.push(ChatMessage {
                        role: "user".to_string(),
                        content: notice,
                        name: None,
                        tool_call_id: None,
                        tool_calls: None,
                    });
                    loop_count = loop_count.saturating_sub(1);
                    continue;
                }
                NoToolCallEnding::Blocked => {
                    // Budget spent and the turn is STILL not a deliberate
                    // answer. Reporting "final answer, no tool calls" plus a
                    // completed handover is precisely what let the LiteLLM
                    // crucible finish three runs in a row one turn in: broken
                    // tool markup was re-noticed twice, given up on, and then
                    // recorded as success with nothing executed. Stop honestly
                    // so the orchestrator can retry, fall back, or escalate
                    // instead of trusting a phase result never produced.
                    //
                    // However, if an artifact was already produced in an earlier
                    // turn, the deliverable exists and a later turn's malformed
                    // markup should not override that success. The model may
                    // have written the file and then attempted another action
                    // that got truncated — the task is still complete.
                    //
                    // "Exists" is doing the work in that reasoning, so it has to
                    // be measured: a run whose target lost most of the items it
                    // arrived with has not produced the deliverable, it deleted
                    // it, and the unfinished-markup branch is where such a run
                    // would otherwise be recorded as completed.
                    if let Some(reason) = artifact_surface.collapse(workspace_root) {
                        ui_tx
                            .send(AgentToUiMessage::StatusUpdate(format!(
                                "Deliverable is NOT validated — {reason}"
                            )))
                            .ok();
                        outcome = ending.stopped(
                            loop_count,
                            "the deliverable lost most of the items it arrived with",
                            AgentLoopOutcome::Blocked { reason },
                        );
                        break;
                    }
                    if produced_artifact {
                        ui_tx
                            .send(AgentToUiMessage::StatusUpdate(format!(
                                "Turn {}: model {:.1}s (artifact already produced; ignoring later malformed markup)",
                                loop_count,
                                provider_ms as f64 / 1000.0
                            )))
                            .ok();
                        outcome = ending.completed(
                            loop_count,
                            &format!("completed{}", artifact_surface.shrink_note(workspace_root)),
                        );
                        break;
                    }
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "Turn {}: model {:.1}s (BLOCKED: turn carried unfinished tool markup, no executable call after {} auto-continuations)",
                            loop_count,
                            provider_ms as f64 / 1000.0,
                            auto_continuations
                        )))
                        .ok();
                    outcome = ending.stopped(
                        loop_count,
                        "unfinished tool markup; no executable tool call",
                        AgentLoopOutcome::Blocked {
                            reason: "unfinished tool markup; no executable tool call".to_string(),
                        },
                    );
                    break;
                }
                NoToolCallEnding::FinalAnswer => {
                    // An artifact task that has written nothing is NOT a clean
                    // success, however confident its closing prose. Steer first
                    // (bounded), then stop honestly so the caller can retry or
                    // fall back instead of trusting an empty deliverable.
                    if should_nudge_missing_artifact(
                        requires_artifact,
                        produced_artifact,
                        artifact_nudges,
                        loop_count + 1 < max_loops,
                    ) {
                        artifact_nudges += 1;
                        ui_tx
                            .send(AgentToUiMessage::StatusUpdate(format!(
                                "Artifact deliverable with no files written \u{2014} requiring the write tool ({}/{})...",
                                artifact_nudges, MAX_ARTIFACT_NUDGES
                            )))
                            .ok();
                        message_history.push(ChatMessage {
                            role: "user".to_string(),
                            content: artifact_delivery_directive(turn_write_tools),
                            name: None,
                            tool_call_id: None,
                            tool_calls: None,
                        });
                        // Consume the turn (no `loop_count` rollback): a model
                        // that will not comply runs out of budget, not out of
                        // free retries.
                        continue;
                    }
                    if requires_artifact && !produced_artifact {
                        ui_tx
                            .send(AgentToUiMessage::StatusUpdate(format!(
                                "Turn {}: model {:.1}s (BLOCKED: artifact task wrote no files after {} nudges)",
                                loop_count,
                                provider_ms as f64 / 1000.0,
                                artifact_nudges
                            )))
                            .ok();
                        outcome = ending.stopped(
                            loop_count,
                            "artifact deliverable, no files written",
                            AgentLoopOutcome::Blocked {
                                reason: "task deliverable was files but no file was ever written"
                                    .to_string(),
                            },
                        );
                        break;
                    }
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "Turn {}: model {:.1}s (final answer, no tool calls)",
                            loop_count,
                            provider_ms as f64 / 1000.0
                        )))
                        .ok();
                    outcome = ending.completed(
                        loop_count,
                        &format!("completed{}", artifact_surface.shrink_note(workspace_root)),
                    );
                    break;
                }
            }
        }
    }

    save_chatlogs_nda(workspace_root, message_history);

    // A run that spends its budget while repairing walks past the completion
    // gate above: that gate sits on the no-tool-call branch, and this loop
    // exited from a tool-calling turn. The verdict then describes the answer
    // that never came, not the files that are on disk. Probe run `testgate1`
    // paid for it — the model corrected the port's two red assertions over six
    // turns, landed `cargo test` at 70 passed / 0 failed, hit its wall clock on
    // the way to a final summary, and reported failure over a finished
    // deliverable. A budget-exhausted exit also writes no handover at all, so
    // nothing on record even says a validation was possible. Judge the artifact,
    // then report what cargo saw.
    let closing_written: Vec<&str> = run_rust_writes.iter().map(String::as_str).collect();
    if should_close_validate(
        requires_artifact,
        !closing_written.is_empty(),
        outcome.is_completed(),
    ) {
        ui_tx
            .send(AgentToUiMessage::StatusUpdate(
                "Run ended without a final answer — validating the files it wrote...".to_string(),
            ))
            .ok();
        match validate_written_artifact(
            workspace_root,
            &closing_written,
            &mut artifact_surface,
            ui_tx,
        ) {
            Ok(note) => {
                if let Some(reason) = off_target_refusal(named_target.as_deref(), &run_rust_writes)
                {
                    // The build that passes belongs to the crate, not to the
                    // file the task owed. Refuse the claim and keep the budget
                    // reason as the outcome, exactly as the `Err` arm below
                    // does: the retry the orchestrator takes is the one with
                    // more time, and this line is what tells it the time has to
                    // go into writing at the named path.
                    ending.note(loop_count, &reason, &outcome);
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "Deliverable is NOT validated — {reason}"
                        )))
                        .ok();
                } else {
                    ui_tx
                        .send(AgentToUiMessage::StatusUpdate(format!(
                            "Deliverable validated — the Rust this run wrote builds and {note} \
                             (the budget ran out before a final answer, so the task may still be \
                             part-done; the crate on disk is sound)"
                        )))
                        .ok();
                    outcome = ending.completed(loop_count, &format!("compiler validated; {note}"));
                }
            }
            Err(e) => {
                // The handover note quotes the offending line, which is what an
                // orchestrator reading it needs; the item key only helps the loop
                // decide whether an error moved, so no workspace lookup here.
                let mut sigs = error_signatures(&e, None);
                sigs.truncate(3);
                let note = format!("{}: {}", gate_failure_label(&e), sigs.join("; "));
                ending.note(loop_count, &note, &outcome);
                ui_tx
                    .send(AgentToUiMessage::StatusUpdate(format!(
                        "Deliverable is NOT validated — {note}"
                    )))
                    .ok();
                // The outcome keeps its budget reason: "turn budget of N
                // exhausted" is what tells the orchestrator a retry with more
                // budget is the fix, while the status line above is what tells
                // it what the retry is for.
            }
        }
    }

    // The `while` above records an ending at every `break` that has one to
    // report. A run that instead falls out of it on its turn budget — or that
    // was cancelled before any gate could judge it — would otherwise leave the
    // *previous* run's handover on disk, which reads as a verdict on this one.
    if !ending.recorded {
        ending.note(
            loop_count,
            "turn budget exhausted without a final answer",
            &outcome,
        );
    }

    // Phase 4: Run self-improvement analysis and persist learnings
    if improve_engine.has_data() {
        improve_engine.persist_to_memory(&mut memory);
        let failure_count = improve_engine.failure_count();
        if let Some(addendum) = improve_engine.generate_prompt_addendum(0.3) {
            // Store the generated addendum for next session's system prompt
            memory.remember(
                "self_improve:prompt_addendum",
                &addendum,
                &["self_improve", "prompt"],
                0.85,
            );
        }
        ui_tx
            .send(AgentToUiMessage::StatusUpdate(format!(
                "Self-improvement: analyzed {} failure(s), updated strategy memory.",
                failure_count
            )))
            .ok();
    }

    // Phase 3: Persist memory to disk
    let _ = memory.save();

    // Phase 2: Report checkpoint status, then tear down the session's snapshots.
    let cp_count = checkpoint_mgr.count();
    if cp_count > 0 {
        let last_id = checkpoint_mgr.list().last().map(|c| c.id);
        let diff_summary = last_id
            .and_then(|id| checkpoint_mgr.diff_since(id).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let msg = match diff_summary {
            Some(diff) => format!(
                "Session complete. {} checkpoint(s) taken; changes since the last one:\n{}",
                cp_count, diff
            ),
            None => format!(
                "Session complete. {} checkpoint(s) taken; no net changes since the last one.",
                cp_count
            ),
        };
        ui_tx.send(AgentToUiMessage::StatusUpdate(msg)).ok();
        // Snapshots are dangling git commits reclaimed by gc; drop our tracking.
        checkpoint_mgr.cleanup();
    }

    if sitemap_needed {
        write_sitemap_nda(workspace_root);
    }
    convert_jsonl_to_nda(workspace_root);
    ui_tx
        .send(AgentToUiMessage::StatusUpdate(
            "Agent workflow finished. Idling.".to_string(),
        ))
        .ok();
    ui_tx.send(AgentToUiMessage::AgentFinished).ok();
    outcome
}

// ─── Workspace Context Gathering for MoA Router ─────────────────────────────

/// Extract file paths mentioned in the user's prompt and read their contents
/// to provide context to the MoA specialists.
///
/// Returns (file_paths, context_string) where context_string includes the
/// actual file contents so the models can analyze real code.
fn gather_workspace_context(workspace_root: &PathBuf, prompt: &str) -> (Vec<String>, String) {
    let mut file_paths = Vec::new();
    let mut context_parts = Vec::new();

    // Common source file extensions to look for
    let source_extensions = [
        ".rs", ".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".java", ".c", ".cpp", ".h", ".hpp",
        ".css", ".scss", ".html", ".vue", ".svelte", ".toml", ".json", ".yaml", ".yml", ".md",
    ];

    // Extract file paths from the prompt using simple pattern matching
    for word in prompt.split_whitespace() {
        let cleaned = word.trim_matches(|c: char| {
            !c.is_alphanumeric() && c != '.' && c != '/' && c != '\\' && c != '-' && c != '_'
        });

        // Check if it looks like a file path
        if cleaned.contains('.') || cleaned.contains('/') || cleaned.contains('\\') {
            let has_extension = source_extensions.iter().any(|ext| cleaned.ends_with(ext));

            if has_extension {
                // Try to resolve the path relative to workspace root
                let path = if std::path::Path::new(cleaned).is_absolute() {
                    std::path::PathBuf::from(cleaned)
                } else {
                    workspace_root.join(cleaned)
                };

                // Read the file if it exists and is reasonably sized (< 100KB)
                if path.exists() && path.is_file() {
                    if let Ok(metadata) = path.metadata() {
                        if metadata.len() < 100_000 {
                            if let Ok(contents) = std::fs::read_to_string(&path) {
                                let relative_path = path
                                    .strip_prefix(workspace_root)
                                    .unwrap_or(&path)
                                    .to_string_lossy()
                                    .to_string();

                                file_paths.push(relative_path.clone());
                                context_parts
                                    .push(format!("=== {} ===\n{}\n", relative_path, contents));
                            }
                        }
                    }
                }
            }
        }
    }

    // Also gather a high-level overview of the workspace structure
    let workspace_overview = gather_workspace_overview(workspace_root);
    if !workspace_overview.is_empty() {
        context_parts.insert(
            0,
            format!("=== Workspace Structure ===\n{}\n", workspace_overview),
        );
    }

    let context = context_parts.join("\n");
    (file_paths, context)
}

/// Gather a high-level overview of the workspace structure (top-level directories
/// and key files) to give the MoA specialists context about the project.
fn gather_workspace_overview(workspace_root: &PathBuf) -> String {
    let mut overview = String::new();

    // List top-level directories
    if let Ok(entries) = std::fs::read_dir(workspace_root) {
        let mut dirs = Vec::new();
        let mut files = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden directories and common non-source directories
            if name.starts_with('.') || name == "target" || name == "node_modules" || name == "dist"
            {
                continue;
            }

            if path.is_dir() {
                dirs.push(name);
            } else if path.is_file() {
                // Only include key config files
                if [
                    "Cargo.toml",
                    "package.json",
                    "README.md",
                    "Cargo.lock",
                    "tsconfig.json",
                ]
                .contains(&name.as_str())
                {
                    files.push(name);
                }
            }
        }

        dirs.sort();
        files.sort();

        if !dirs.is_empty() {
            overview.push_str(&format!("Directories: {}\n", dirs.join(", ")));
        }
        if !files.is_empty() {
            overview.push_str(&format!("Key files: {}\n", files.join(", ")));
        }
    }

    // Read Cargo.toml or package.json for project metadata
    let cargo_toml = workspace_root.join("Cargo.toml");
    if cargo_toml.exists() {
        if let Ok(contents) = std::fs::read_to_string(&cargo_toml) {
            // Extract just the [package] section
            if let Some(start) = contents.find("[package]") {
                if let Some(end) = contents[start..].find("\n[").map(|p| start + p) {
                    overview.push_str(&format!(
                        "\n=== Cargo.toml [package] ===\n{}\n",
                        &contents[start..end]
                    ));
                }
            }
        }
    }

    overview
}

/// Consecutive read-only turns after which the agent gets a hard directive to
/// stop auditing and produce changes (or its final report).
const WRITE_PRESSURE_STREAK: u32 = 5;
/// Upper bound on injected directives so a stubborn model isn't shouted at
/// every single turn for the rest of the run.
const MAX_WRITE_PRESSURE_NUDGES: u32 = 3;

/// Pure decision function behind the write-pressure nudge: fire when the
/// read-only streak passes the threshold, the nudge budget isn't exhausted,
/// and at least one more model turn remains to act on the directive.
fn should_nudge_write_pressure(streak: u32, nudges_sent: u32, has_more_turns: bool) -> bool {
    streak >= WRITE_PRESSURE_STREAK && nudges_sent < MAX_WRITE_PRESSURE_NUDGES && has_more_turns
}

/// Whether a turn may offer only the file editors. Three steers all say the
/// model has been told to write and still hasn't: it announced an action with
/// no call, an artifact task was nudged for having written no files, or a
/// streak of turns has gone by without an edit.
///
/// The third is the one the loop used to miss. A lock that only fires after the
/// model *pauses* cannot bind a model that never pauses: measured on repair17a,
/// `qwen3.6:35b-a3b` spent 591s and 16 turns on `read_file` / `list_dir`, was
/// write-nudged twice, and lost its exploration tools only on the turn the turn
/// budget ran out — a lock that arrives with the run it was meant to save
/// locks nothing, and the fallback model then got 309s and spent two more turns
/// reading before the wall clock cut it mid-write.
///
/// `build_known_broken` ignores `produced_artifact`, because that is the hole
/// repair20 fell through: the run HAD written its file, so no steer could lock it
/// again, turns 10-16 re-read and re-grepped the very file rustc had rejected,
/// and the run died on its turn budget one error deep. A write the gate refused
/// is not a finished artifact — it is the thing the next turn has to edit.
fn should_lock_exploration(
    requires_artifact: bool,
    produced_artifact: bool,
    steered_after_pause: bool,
    steered_after_reads: bool,
    repair_this_turn: bool,
    build_known_broken: bool,
) -> bool {
    // A pending compile-repair forces a rewrite even though the artifact was
    // already produced (it just doesn't compile), and needs no artifact mandate
    // of its own. `pending_repair_directive` is consumed by the directive
    // selection below for exactly this one turn.
    if repair_this_turn {
        return true;
    }
    requires_artifact
        && (build_known_broken
            || (!produced_artifact && (steered_after_pause || steered_after_reads)))
}

/// Whether this write is the deliverable, for the purposes of the artifact
/// lock and the blocked-vs-completed verdict.
///
/// A Rust task is satisfied by Rust: a helper script or a scratch note beside it
/// is work, not the thing the caller asked for, and counting it as the artifact
/// both unlocks exploration and reports a deliverable no gate ever graded. When
/// the task never said what language it wants, the loop cannot recognise a junk
/// file, so every write counts. A write whose path the changelog could not report
/// counts too — absence of proof is not proof of junk.
fn counts_as_artifact(written: Option<&str>, artifact_is_rust: bool) -> bool {
    match written {
        Some(path) => !artifact_is_rust || path.replace('\\', "/").ends_with(".rs"),
        None => true,
    }
}

/// Whether this write missed the one path the task named.
///
/// Both sides arrive as the model typed them — the target from the task text,
/// the write from the tool's own `relativeFilePath` argument — so separators
/// are normalised before comparing. `false` whenever there is no rule to
/// apply: no named target, or a write whose path the changelog could not
/// report. A path that merely *looks* odd is not refused here; `write_file`
/// accepts any path inside the workspace, and the loop's job is to notice that
/// the file it owes is still empty.
fn write_misses_named_target(written: Option<&str>, named: Option<&str>) -> bool {
    match (written, named) {
        (Some(path), Some(target)) => path.replace('\\', "/") != target.replace('\\', "/"),
        _ => false,
    }
}

/// Whether a locked turn may still read the file the task names.
///
/// The lock exists to stop a small model wandering the workspace instead of
/// writing, and `lr7b_v14` shows it working: 30 turns, no reads, no wandering.
/// The deliverable itself is not wandering, though, and locking it produced the
/// opposite of convergence. That run's dispatch text told the model to "use
/// edit_file with an oldString copied verbatim from the file (read_file the
/// region first if you are not sure of the exact text)" while the lock refused
/// the read at turn 3, so every payload it sent afterwards was built from a file
/// it had invented — 700 bytes standing in for a 26 KB port — and the write gate
/// refused all sixteen of them. A model that cannot see the text it is told to
/// quote verbatim cannot produce an edit that matches.
fn read_of_named_target(tool_name: &str, args: &Value, named_target: Option<&str>) -> bool {
    if tool_name != "read_file" {
        return false;
    }
    let Some(target) = named_target else {
        // Nothing named, nothing to aim a read at: the lock stays whole rather
        // than opening for every path in the workspace.
        return false;
    };
    let Some(path) = args["relativeFilePath"]
        .as_str()
        .or_else(|| args["path"].as_str())
    else {
        return false;
    };
    !write_misses_named_target(Some(path), Some(target))
}

/// Whether every Rust write of the run landed off the named target, so a clean
/// build and green tests cannot be read as "the deliverable is done". An empty
/// write list is not this rule's problem — that is the no-files-written nudge.
fn writes_miss_named_target(named: Option<&str>, written: &[String]) -> bool {
    let Some(target) = named else {
        return false;
    };
    !written.is_empty()
        && written
            .iter()
            .all(|w| write_misses_named_target(Some(w), Some(target)))
}

/// The sentence to report when a green cargo verdict cannot be the deliverable,
/// quoting the path the task named beside the paths actually written. `None`
/// when there is nothing to refuse.
fn off_target_refusal(named: Option<&str>, written: &[String]) -> Option<String> {
    if !writes_miss_named_target(named, written) {
        return None;
    }
    let target = named.unwrap_or_default();
    Some(format!(
        "the crate that builds is not the deliverable: every write landed outside '{target}' ({})",
        written.join(", ")
    ))
}

/// The steer for a run whose writes all landed beside the deliverable. Quotes
/// both paths, because "write the deliverable" is advice the model already
/// believes it followed: convert22's reruns kept re-emitting the same nested
/// file, and the only fact that can change that is seeing where its write
/// actually went next to where the task said it should go.
fn wrong_path_directive(named: &str, written: &[String], editors: &[&str]) -> String {
    let names = editors.join(" or ");
    let wrote = written.join(", ");
    format!(
        "[System notice] The crate builds and its tests pass, but they are not your deliverable: \
         the task names '{named}' and every file you wrote landed elsewhere ({wrote}). Nothing has \
         been written at '{named}'. Call {names} with relativeFilePath set to exactly '{named}' and \
         put the deliverable's contents there; paths are relative to the workspace root, so one \
         that begins with the workspace's own folder name describes a file inside a copy of it. \
         Then reply with the single path you wrote."
    )
}

/// Whether the run may spend another turn telling the model that its writes
/// missed the path the task named.
///
/// Deliberately a budget of its own rather than [`MAX_ARTIFACT_NUDGES`]: the
/// two steer different mistakes, and `convert22`'s reruns showed a model that
/// had already been chased for writing nothing still needing turns to be told
/// *where* to write. One shared cap ends the run at whichever mistake came
/// first and reports the other one as unfixable.
fn should_steer_wrong_path(refusals_sent: u32) -> bool {
    refusals_sent < MAX_OFF_TARGET_REFUSALS
}

/// How many distinct Rust files a run's writes are tracked in. Bounded because
/// the list is passed to the gate as its test scope and quoted back to the
/// model as evidence; a model that rewrites a whole tree must not grow either
/// without limit.
const MAX_TRACKED_RUN_WRITES: usize = 32;

/// Record one Rust write of the run, comparing paths the way the tool sees
/// them.
///
/// A write of `a/b.rs` and a later write of `a\b.rs` are the same file on every
/// platform this loop runs on, but a raw `Vec<String>::contains` counted them
/// twice — and the duplicate then appeared as two files in the off-target
/// refusal's evidence list, quoting one path as though it were two failures.
fn remember_run_write(writes: &mut Vec<String>, rel: &str) {
    let normalised = rel.replace('\\', "/");
    if writes.len() >= MAX_TRACKED_RUN_WRITES
        || writes.iter().any(|w| w.replace('\\', "/") == normalised)
    {
        return;
    }
    writes.push(rel.to_string());
}

/// The handover state and `interrupted` flag that an outcome requires.
///
/// Terminal exits used to write their handover strings *beside* the outcome
/// instead of deriving them from it, and they disagreed: the unparseable
/// tool_calls exit recorded `idle`/`completed` while returning `Blocked`, and
/// the artifact-record site before it claimed a stop was a completion. Deriving
/// both fields from the outcome removes the choice, which is the same class of
/// over-claim that let `convert22` report a green run over an empty deliverable.
fn handover_for_outcome(outcome: &AgentLoopOutcome) -> (&'static str, bool) {
    match outcome {
        AgentLoopOutcome::Completed => ("idle", false),
        AgentLoopOutcome::Cancelled => ("cancelled", true),
        AgentLoopOutcome::Blocked { .. }
        | AgentLoopOutcome::ProviderUnavailable { .. }
        | AgentLoopOutcome::TimedOut { .. } => ("blocked", true),
    }
}

/// Records how this run ended, in the one place that knows what an ending
/// means. The mid-loop progress records (`executing`, `tool_error`,
/// `tool_rejected`) stay direct calls to [`write_handover_nda`]: they describe a
/// turn, not a verdict, and must not mark the run as finished.
struct RunEnding<'a> {
    workspace_root: &'a PathBuf,
    recorded: bool,
}

impl<'a> RunEnding<'a> {
    /// Write the handover that matches `outcome` and note that this run's ending
    /// is on record.
    fn note(&mut self, turn: usize, build_status: &str, outcome: &AgentLoopOutcome) {
        let (state, interrupted) = handover_for_outcome(outcome);
        write_handover_nda(self.workspace_root, state, turn, build_status, interrupted);
        self.recorded = true;
    }

    /// The run stopped short of an answer. Returns the outcome so the call site
    /// breaks on the same value it just recorded.
    fn stopped(
        &mut self,
        turn: usize,
        build_status: &str,
        outcome: AgentLoopOutcome,
    ) -> AgentLoopOutcome {
        self.note(turn, build_status, &outcome);
        outcome
    }

    /// The run delivered what it was asked for.
    fn completed(&mut self, turn: usize, build_status: &str) -> AgentLoopOutcome {
        self.stopped(turn, build_status, AgentLoopOutcome::Completed)
    }
}

/// Whether this turn rebuilds the lean write skeleton instead of sending the
/// full transcript. Deliberately narrower than the tool lock above.
///
/// The rebuild exists to delete prose announcement turns that later turns copy,
/// and it keeps only the last two tool exchanges. After a pause that is exactly
/// right; after a streak of *productive* reads it would throw away the only copy
/// of the file text the model has, and a `edit_file` needs its `oldString`
/// copied verbatim. The write directive already reached history from the
/// write-pressure nudge, so a streak turn gets steered without going amnesiac.
fn should_rebuild_write_context(
    lock_exploration: bool,
    steered_after_pause: bool,
    repair_this_turn: bool,
) -> bool {
    lock_exploration && (steered_after_pause || repair_this_turn)
}

fn write_pressure_directive(editors: &[&str]) -> String {
    // Two rules the old fixed text broke. It named `write_file` whatever the
    // turn offered, and it demanded "the file's current full contents" — on a
    // task whose file already exists, that is the one call shape guaranteed not
    // to fit the model's output budget, which is why the model kept re-reading
    // instead of answering: the instruction it was shouted at was unsatisfiable.
    let names = editors.join(" or ");
    let shape = if editors.contains(&"edit_file") {
        "Where the file already exists, call edit_file with the smallest oldString \
         copied VERBATIM from it, indentation included; do not re-send a whole file \
         you could have patched. Where nothing exists yet, send the complete new file."
    } else {
        "write_file replaces the entire file, so every line it must keep has to be present."
    };
    format!(
        "[System notice] You have spent several consecutive turns reading, listing and running \
         diagnostics without editing a single file. You already have enough context: RE-ORIENT \
         NOW and issue a {names} call this turn instead of more exploration. {shape} If the \
         mission is genuinely read-only, stop exploring and deliver your final report as plain text."
    )
}

/// Pure decision behind the artifact-deliverable gate: fire only when the task
/// actually owes files, nothing has been written yet, the nudge budget remains,
/// and at least one more model turn exists to act on the directive. A read-only
/// or report task (`requires_artifact == false`) never trips it, so its final
/// answer stays a valid completion.
fn should_nudge_missing_artifact(
    requires_artifact: bool,
    produced: bool,
    nudges_sent: u32,
    has_more_turns: bool,
) -> bool {
    requires_artifact && !produced && nudges_sent < MAX_ARTIFACT_NUDGES && has_more_turns
}

/// Whether a generation-family tool is hidden from a dispatched model's toolset.
/// Only an artifact (software-deliverable) run narrows; report tasks and IDE turns
/// pass `requires_artifact = false` and keep the full catalog.
fn hides_generation_tool(requires_artifact: bool, name: &str) -> bool {
    requires_artifact && MEDIA_GENERATION_TOOLS.contains(&name)
}

fn artifact_delivery_directive(editors: &[&str]) -> String {
    // `turn_write_tools`, not the full editor pair: nothing has been written
    // yet, and `edit_file` refuses a file that doesn't exist. A from-scratch
    // port must be told `write_file` or it spends the nudge on a refusal.
    let names = editors.join(" or ");
    format!(
        "[System notice] You ended with a text answer but wrote NO files. This task's deliverable \
         is files you must create with the {names} tool \u{2014} a response with no writes is a \
         failed task, not a completed one. Call {names} now for every file the task names, at the \
         exact paths it gives (creating parent directories as needed), then reply with a single \
         line listing the paths you wrote."
    )
}

/// The workspace-relative path a task text points at that already exists.
///
/// Used only to decide whether a locked turn may also patch, and what the run
/// remembers about the file it was handed, so a false positive costs an extra
/// schema and a false negative costs nothing a scratch write would not have had
/// to do anyway. Absolute and drive-letter tokens are skipped: joining one onto
/// the root would replace the root and report existence for a path the tools
/// could never be asked to write.
fn existing_file_named_by_task(root: &std::path::Path, task: &str) -> Option<String> {
    let quoted = |c: char| matches!(c, '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}');
    let punct = |c: char| matches!(c, '.' | ',' | ';' | ':');
    task.split_whitespace().find_map(|token| {
        // Sentence punctuation belongs to the prose, not the path — but only at
        // the end, so a leading `.` in `.velocity/clones/…` survives. A path is
        // often written `'src/x.rs'.`, quote inside period, so the wrap set is
        // peeled again after the punctuation pass.
        let path = token.trim_matches(quoted);
        let path = path.trim_end_matches(punct);
        let path = path.trim_end_matches(quoted);
        if path.is_empty() || path.contains(':') || path.starts_with('/') || path.starts_with('\\')
        {
            return None;
        }
        // A bare word with no separator and no extension is prose, not a path.
        if !path.contains('/') && !path.contains('.') {
            return None;
        }
        root.join(path).is_file().then(|| path.to_string())
    })
}

/// The names of the items a Rust file declares.
///
/// A line scanner, not a parser: a keyword only counts as a declaration when it
/// starts the line or follows nothing but modifiers (`pub`, `async`, `unsafe`,
/// `extern`, `default`), so a `fn` inside prose, a comment or a string literal
/// adds no name. `pub(crate)` is read as `pub`, and `impl` blocks are covered by
/// the `fn`s inside them. Test functions are `fn`s, so deleting a test removes a
/// name this reports.
fn rust_item_names(src: &str) -> Vec<String> {
    const MODIFIERS: [&str; 5] = ["pub", "async", "unsafe", "extern", "default"];
    const ITEMS: [&str; 6] = ["fn", "struct", "enum", "trait", "mod", "type"];
    let ident_head = |word: &str| -> String {
        word.chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect()
    };
    let mut names: Vec<String> = Vec::new();
    for raw in src.lines() {
        let line = raw.trim_start();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let mut words = line.split_whitespace();
        let mut head = ident_head(words.next().unwrap_or_default());
        while MODIFIERS.contains(&head.as_str()) {
            head = ident_head(words.next().unwrap_or_default());
        }
        if !ITEMS.contains(&head.as_str()) {
            continue;
        }
        let name = ident_head(words.next().unwrap_or_default());
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// The items the deliverable had when the run started that are no longer in it.
///
/// An unreadable file reports nothing missing rather than everything: a crate
/// whose target vanished fails its own build gate, and this guard exists for the
/// case the build gate cannot see — a smaller file that compiles.
fn deleted_artifact_items(
    workspace_root: &std::path::Path,
    target: &str,
    baseline: &[String],
) -> Vec<String> {
    if baseline.is_empty() {
        return Vec::new();
    }
    let Ok(text) = std::fs::read_to_string(workspace_root.join(target)) else {
        return Vec::new();
    };
    let now = rust_item_names(&text);
    baseline
        .iter()
        .filter(|name| !now.contains(name))
        .cloned()
        .collect()
}

/// The items a whole-file write would remove from the deliverable.
///
/// The comparison `deleted_artifact_items` makes after a write, made before it:
/// the baseline is the surface the file arrived with and the "now" is what the
/// incoming content declares. Only `write_file` is judged this way, because it is
/// the only call that replaces text the model may never have seen — a repair turn
/// runs under the exploration lock, so `lr7b_v13`'s turn 5 wrote over a 54-item
/// port without ever reading it. An `edit_file` deletes text it was handed.
fn items_a_rewrite_would_delete(baseline: &[String], content: &str) -> Vec<String> {
    if baseline.is_empty() {
        return Vec::new();
    }
    let incoming = rust_item_names(content);
    baseline
        .iter()
        .filter(|name| !incoming.contains(*name))
        .cloned()
        .collect()
}

/// Whether a whole-file write over the deliverable is refused before it lands.
///
/// `baseline == 0` is not a shrug but "this run has no recorded surface" — a
/// first write of a new artifact, or a task whose deliverable is not Rust — and
/// there is nothing to protect. The threshold is `ArtifactSurface::collapse`'s:
/// at least half the items gone. Below it a refactor that moves a few names to a
/// sibling module still lands, which is the case the post-hoc guard also waves
/// through with a warning.
fn should_refuse_gutting_rewrite(missing: usize, baseline: usize) -> bool {
    baseline > 0 && missing * 2 >= baseline
}

/// The refusal for a `write_file` that would gut the deliverable, or `None` when
/// the write may proceed.
///
/// Refusing beats grading: once the bytes are gone the surface guard can only
/// name the missing items, and "put those items back" is an order the model
/// cannot follow — the source is off disk, `read_file` is refused on a repair
/// turn, and the rebuilt 7-message context cannot be carrying a paste from five
/// turns ago. `lr7b_v13` spent its last three turns on that impossible demand and
/// ended with a 143-byte file and a Blocked verdict.
///
/// `named_target` is the one path a locked turn may still `read_file`, and it is
/// what makes the order below followable. Without it the refusal quotes item
/// names, and names are the only text from the file the model has seen —
/// `lr7b_v15` answered this refusal with 54 edits whose `oldString` and
/// `newString` were both a bare identifier copied off that list.
fn gutting_rewrite_refusal(
    tool_name: &str,
    args: &Value,
    surface_target: Option<&str>,
    surface_names: &[String],
    named_target: Option<&str>,
) -> Option<String> {
    if tool_name != "write_file" || surface_names.is_empty() {
        return None;
    }
    let path = args["relativeFilePath"]
        .as_str()
        .or_else(|| args["path"].as_str())?;
    // A surface with no target is a surface this gate cannot aim at, so it stays
    // inert rather than watching every write in the run.
    let target = surface_target?;
    if write_misses_named_target(Some(path), Some(target)) {
        return None;
    }
    let content = args["content"].as_str()?;
    let missing = items_a_rewrite_would_delete(surface_names, content);
    should_refuse_gutting_rewrite(missing.len(), surface_names.len()).then(|| {
        let shown: Vec<&String> = missing.iter().take(8).collect();
        let list = shown
            .iter()
            .map(|n| format!("`{n}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let tail = if missing.len() > shown.len() {
            format!(" and {} more", missing.len() - shown.len())
        } else {
            String::new()
        };
        // The names above are a list, not a payload. A model that has never read
        // the file will otherwise send them straight back as edit text.
        let readable = named_target
            .map(|t| {
                format!(
                    " Those names are a list, not the text to send. The text is on disk in \
                     \"{t}\": `read_file` with that relativeFilePath is allowed on this turn, and \
                     startLine/endLine bring back just the region the diagnostics name."
                )
            })
            .unwrap_or_default();
        // "Error executing tool" is the prefix `is_refusal` classifies on, so this
        // refusal is charged, refunded and replay-counted like any other.
        format!(
            "Error executing tool: {tool_name} replaces {path} whole, and the content you sent \
             does not declare {} of the {} items that file arrived with ({list}{tail}). It was \
             NOT written, so those items are still on disk — and they would not be recoverable \
             if it had been, because nothing in this run can hand you their source again once a \
             write has removed it. A crate that builds because its code is gone is not a fix. \
             Change only what the diagnostics name, with `edit_file`: its `edits` take the exact \
             text to replace and the exact text to put there, so the rest of the file stays as \
             it is.{readable}",
            missing.len(),
            surface_names.len()
        )
    })
}

/// Marks the gate report that is the loop's own verdict about the artifact's
/// surface, not a compiler's or a test harness's output.
///
/// The prefix is the identifier because a deletion verdict carries no rustc
/// diagnostics to match on, and the label a status line shows has to be able to
/// tell "the artifact shrank" from "compilation failed".
const DELETION_REPORT_PREAMBLE: &str = "[System notice] The build is clean and the tests pass, \
                                        but only because your write DELETED code";

fn is_deletion_report(errors: &str) -> bool {
    errors.starts_with(DELETION_REPORT_PREAMBLE)
}

/// The report handed back when a clean grade was bought by deleting code.
fn deleted_items_directive(target: &str, missing: &[String]) -> String {
    let shown: Vec<&String> = missing.iter().take(12).collect();
    let list = shown
        .iter()
        .map(|n| format!("`{n}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let tail = if missing.len() > shown.len() {
        format!(" (and {} more)", missing.len() - shown.len())
    } else {
        String::new()
    };
    format!(
        "{DELETION_REPORT_PREAMBLE} instead of fixing it. These items were in '{target}' when \
         this run started and are not in it now: {list}{tail}. Removing the code an error was \
         reported in makes the error disappear without resolving it, and a smaller file that \
         happens to compile is not the deliverable. Re-add the items above and fix them where \
         they are wrong, and do not report success until they are back.\n\
         If this task genuinely asked you to REMOVE these items, do not re-add them: reply with \
         one line saying the task asked for the removal, and the run ends reporting the file as \
         smaller. Deleting them again on a write turn is what costs the run its remaining turns."
    )
}

/// What the file a dispatch names looked like when the run opened.
///
/// A repair is graded by asking rustc and the crate's tests whether they are
/// satisfied, and both go quiet when the code they were complaining about is
/// deleted. pendulum1 measured the hole: the task handed the agent a port whose
/// `impl ... for Snapshot` contradicted its own trait, and the write that "fixed"
/// it removed the entire module — 3 references left, 23,880 → 23,440 bytes — over
/// a clean build and 70 passing tests. The names the file arrived with are what
/// makes that deletion distinguishable from a repair.
#[derive(Default)]
struct ArtifactSurface {
    /// The path the task named, as the task wrote it.
    target: Option<String>,
    /// The items declared in that file before this run touched it.
    names: Vec<String>,
    /// Clean grades already refused for losing items. Bounded: a refactor that
    /// genuinely moves code between modules trips the same floor, and a crate
    /// that builds and passes its tests must not be held hostage by it forever.
    refusals: u32,
}

impl ArtifactSurface {
    /// The report to hand back instead of accepting a clean grade, or `None`
    /// when the grade stands — nothing recorded, nothing missing, or the run has
    /// already been told [`MAX_SURFACE_REFUSALS`] times.
    fn withheld_report(&mut self, workspace_root: &std::path::Path) -> Option<String> {
        if self.names.is_empty() || self.refusals >= MAX_SURFACE_REFUSALS {
            return None;
        }
        let target = self.target.as_deref()?;
        let missing = deleted_artifact_items(workspace_root, target, &self.names);
        if missing.is_empty() {
            return None;
        }
        self.refusals += 1;
        Some(deleted_items_directive(target, &missing))
    }

    /// Why a deliverable that lost at least half the items it arrived with is not
    /// a finished task, or `None` when the surface still stands.
    ///
    /// `lr7b_v6b` is why the bounded refusal needed a terminal verdict: the model
    /// answered a clean build it had bought by deleting the file (26 KB → 495 B,
    /// 68 tests → 53) with two refusals spent, and the third grade was accepted
    /// because the budget said the interrupting had to stop. Interrupting is what
    /// is bounded; reporting a gutted deliverable as validated is not. A refactor
    /// that moves a few names to a sibling module keeps most of the surface and
    /// still ends green with the warning below.
    fn collapse(&self, workspace_root: &std::path::Path) -> Option<String> {
        let missing = self.missing_items(workspace_root)?;
        if missing.len() * 2 < self.names.len() {
            return None;
        }
        let target = self.target.as_deref().unwrap_or("the deliverable");
        let shown: Vec<&String> = missing.iter().take(8).collect();
        Some(format!(
            "the build went quiet because {} of the {} items {target} arrived with were \
             deleted ({})",
            missing.len(),
            self.names.len(),
            shown
                .iter()
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }

    fn missing_items(&self, workspace_root: &std::path::Path) -> Option<Vec<String>> {
        if self.names.is_empty() {
            return None;
        }
        let target = self.target.as_deref()?;
        Some(deleted_artifact_items(workspace_root, target, &self.names))
    }

    /// The items the deliverable no longer declares, phrased for a success line.
    ///
    /// A refusal budget bounds the *interrupts*, not the caller's right to know
    /// what it is being handed: once the loop stops sending the model back, a run
    /// that ends green still reports the shrink instead of quoting a test tally
    /// over a smaller file.
    fn shrink_note(&self, workspace_root: &std::path::Path) -> String {
        let Some(missing) = self.missing_items(workspace_root) else {
            return String::new();
        };
        if missing.is_empty() {
            return String::new();
        }
        let target = self.target.as_deref().unwrap_or("the deliverable");
        let shown: Vec<&String> = missing.iter().take(12).collect();
        let list = shown
            .iter()
            .map(|n| format!("`{n}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let tail = if missing.len() > shown.len() {
            format!(" … +{}", missing.len() - shown.len())
        } else {
            String::new()
        };
        format!(
            " — WARNING: {} item(s) the file started with are no longer declared in {target}: \
             {list}{tail}",
            missing.len()
        )
    }
}

/// The external crates the crate under `workspace_root` declares.
///
/// Three answers, not two: `None` is "there is no manifest I can read", while
/// `Some(vec![])` is "the manifest really declares nothing". Collapsing them was
/// the bug this closes — the repair directive used to assert `this crate
/// declares ZERO dependencies — std only` to every model in every workspace,
/// which is true of the offline porting fixture it was written for and false
/// everywhere else: in a crate that depends on serde it sent the agent to
/// hand-roll JSON in `format!` while forbidding it from opening Cargo.toml.
///
/// A line scanner rather than a TOML parser: only the dependency tables are read
/// and an unparseable manifest degrades to `None`.
fn declared_dependencies(workspace_root: &std::path::Path) -> Option<Vec<String>> {
    let dir = crate::automation::build_runner::cargo_manifest_dir(workspace_root).ok()?;
    let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).ok()?;
    let is_dep_table = |seg: &str| seg == "dependencies" || seg.ends_with("-dependencies");

    let mut names: Vec<String> = Vec::new();
    let mut in_dep_list = false;
    for raw in manifest.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(table) = line.strip_prefix('[').and_then(|t| t.split(']').next()) {
            let segs: Vec<String> = table
                .split('.')
                .map(|s| s.trim().trim_matches('\'').to_string())
                .collect();
            in_dep_list = is_dep_table(segs.last()?);
            // `[dependencies.foo]` names the crate in the header instead of in a
            // `foo = "1"` line, so harvest it here.
            if !in_dep_list && segs.iter().any(|s| is_dep_table(s)) {
                names.push(segs.last()?.clone());
            }
            continue;
        }
        if !in_dep_list {
            continue;
        }
        let key = line.split('=').next().unwrap_or("").trim();
        let key = key.trim_matches('"').trim_matches('\'').trim();
        if !key.is_empty() && !key.contains([' ', '[', ']']) {
            names.push(key.to_string());
        }
    }
    names.sort();
    names.dedup();
    Some(names)
}

/// The dependency rule inside a repair directive, from what the manifest says.
///
/// `compile_repair_directive` cannot hedge: a model on its last turn reads one
/// instruction and acts, so a wrong claim about whether it may use a crate costs
/// the whole repair.
fn dependency_repair_rule(workspace_root: &std::path::Path) -> String {
    match declared_dependencies(workspace_root) {
        None => "Do not assume anything about this crate's dependencies: add none and do not \
                touch Cargo.toml, and fix each error inside the source instead."
            .to_string(),
        Some(names) if names.is_empty() => "this crate declares ZERO dependencies \u{2014} std \
                only, so an error about a missing external crate is not fixed by adding it: do \
                NOT touch Cargo.toml, REIMPLEMENT that behaviour with std (hand-roll the JSON \
                with String/format!/a small enum, replace iterators, etc)"
            .to_string(),
        Some(names) => {
            const SHOW: usize = 12;
            let shown = names
                .iter()
                .take(SHOW)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            let rest = if names.len() > SHOW {
                format!(", +{} more", names.len() - SHOW)
            } else {
                String::new()
            };
            format!(
                "this crate declares dependencies \u{2014} {shown}{rest} \u{2014} so reach for one \
                 of those before hand-rolling it with std, and do NOT add another or edit \
                 Cargo.toml: if rustc cannot find a crate, it is not declared here, so fix the \
                 code to use what is available"
            )
        }
    }
}

/// rustc's advice for an unresolved crate tells the model to do the one thing
/// the rest of the directive forbids.
///
/// For `E0433` rustc prints `= help: if you wanted to use a crate named `X`,
/// use `cargo add X` to add it to your `Cargo.toml``. No task this loop runs
/// edits Cargo.toml, and in a zero-dependency crate the only correct fix is to
/// reimplement with std. `convert11`'s port carried `serde_json` through all
/// three build repairs while the pasted diagnostic said *add it* one line and
/// the rule beneath it said *do not touch Cargo.toml* — a contradiction a 35B
/// model resolves by changing nothing, which costs three turns and 850 seconds.
/// The advice is therefore replaced with the answer that is actually true for
/// this workspace; anything else in the diagnostic is left verbatim.
fn correct_missing_crate_help(errors: &str, declared: Option<&[String]>) -> String {
    let mut out = String::with_capacity(errors.len());
    for line in errors.lines() {
        match missing_crate_help(line, declared) {
            Some(replacement) => out.push_str(&replacement),
            None => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

/// The rewritten form of one rustc `cargo add` help line, or `None` to leave
/// the line exactly as rustc wrote it.
fn missing_crate_help(line: &str, declared: Option<&[String]>) -> Option<String> {
    const LEAD: &str = "= help: if you wanted to use a crate named `";
    let indent_len = line.len() - line.trim_start().len();
    let rest = line.trim_start().strip_prefix(LEAD)?;
    let crate_name = rest.split('`').next().filter(|n| !n.is_empty())?;
    let declared = declared?;
    // A crate the manifest *does* declare is failing for some other reason
    // (a feature gate, a renamed import), and rustc's own hint may be right.
    if declared.iter().any(|d| same_crate(d, crate_name)) {
        return None;
    }
    let note = if declared.is_empty() {
        format!(
            "= note: `{crate_name}` is NOT available and `cargo add` is not the fix: this crate \
             declares ZERO dependencies, Cargo.toml is off-limits, so REIMPLEMENT that behaviour \
             with std (hand-roll the formatting with String/format!/write!, keep the same types \
             and behaviour) and delete every use of `{crate_name}` from the file"
        )
    } else {
        const SHOW: usize = 12;
        let shown = declared
            .iter()
            .take(SHOW)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "= note: `{crate_name}` is not among this crate's declared dependencies ({shown}) \
             and Cargo.toml is off-limits, so `cargo add` is not the fix: use one of those or \
             std, and delete every use of `{crate_name}` from the file"
        )
    };
    Some(format!("{}{}", " ".repeat(indent_len), note))
}

/// Crate names arrive from two sources that disagree on punctuation: Cargo.toml
/// allows `serde-json` while `use serde_json::` cannot.
fn same_crate(declared: &str, used: &str) -> bool {
    declared.replace('-', "_") == used.replace('-', "_")
}

/// Whether a write-side gate should interrupt the model with a repair turn.
///
/// `attempts_left` bounds how often this can happen; `diagnostics_changed` is
/// what makes the bound mean something. A write that leaves rustc with
/// byte-identical diagnostics achieved nothing, and the convert run this
/// machinery was tuned on burned all three repair attempts on exactly that —
/// four identical error sets, zero edits — because the loop re-armed on a file it
/// had already told the model to fix.
///
/// `not_repeating` is the other way a repair turn buys nothing: a message whose
/// errors have already come back twice is a loop the model is inside, and one
/// more interrupt only costs the turn that the completion gate needs to report
/// the failure honestly.
fn should_arm_write_repair(
    attempts_left: bool,
    diagnostics_changed: bool,
    not_repeating: bool,
) -> bool {
    attempts_left && diagnostics_changed && not_repeating
}

/// Upper bound on repair attempts handed back. A refund returns budget the model
/// never spent; it is not a licence to keep interrupting. Two covers a run whose
/// first repair turns are refused without letting a model that ignores every
/// directive be re-armed indefinitely — the turn budget still ends the run.
const MAX_REFUNDED_REPAIR_ATTEMPTS: u32 = 2;

/// Whether an armed repair turn gives its attempt back.
///
/// `arm_compile_repair` charges the attempt when the directive is armed, so the
/// charge lands before the model has done anything with it. `lr7b_v9` and
/// `lr7b_v11` both drained their budget that way: the arms went out, and the
/// turns that followed produced one `edit_file` each, refused because its
/// `oldString` quoted text an earlier write had already replaced. Nothing reached
/// the file, so the gate never ran again — it grades writes — and each run ended
/// with an error still standing and no attempts left to spend on it. An attempt
/// buys a repair the compiler got to grade; a turn that moved no file is not one.
fn should_refund_repair_attempt(
    armed_this_turn: bool,
    wrote_this_turn: bool,
    a_call_was_refused: bool,
    refunds_used: u32,
) -> bool {
    armed_this_turn
        && !wrote_this_turn
        && a_call_was_refused
        && refunds_used < MAX_REFUNDED_REPAIR_ATTEMPTS
}

/// The identity of a tool call for repeat detection: the tool and its arguments.
///
/// A `Value` object is a `BTreeMap` unless serde_json is built with
/// `preserve_order`, so one payload spelled with its keys in a different order
/// formats to the same string — and `lr7b_v12`'s transcript shows this model
/// doing exactly that between turns, `relativeFilePath` first in one call and
/// last in the next. If that feature is ever turned on, this must sort the keys
/// itself or the same call starts reading as a different one.
fn call_identity(tool_name: &str, arguments: &Value) -> String {
    format!("{tool_name}|{arguments}")
}

/// Whether a tool result is a refusal, by the same test the results loop uses to
/// set `any_error`. Kept beside it so a call counted as a repeat is a call the
/// loop already charged for.
fn is_refusal(result: &str) -> bool {
    result.contains("Error executing tool") || result.starts_with("BLOCKED:")
}

/// Record how one call ended and report a replay.
///
/// Returns the number of times this run has now refused this identical payload,
/// from the second refusal onward. Anything that is not a refusal — a write that
/// landed, or a call the user turned away before it ran — clears the count: the
/// same arguments can land once the file underneath them changes, and a stale
/// count would accuse that call of being a replay it is not.
fn note_refusal_repeat(
    counts: &mut std::collections::HashMap<String, u32>,
    identity: &str,
    refused: bool,
) -> Option<u32> {
    if !refused {
        counts.remove(identity);
        return None;
    }
    let seen = counts.entry(identity.to_string()).or_insert(0);
    *seen += 1;
    (*seen > 1).then_some(*seen)
}

/// What a model that keeps replaying one refused call should do instead, read
/// off the refusal it is being handed again.
///
/// This is derived rather than fixed because the fixed version was actively
/// harmful. `lr7b_v17` replayed an edit that `already_applied_at` had refused
/// with "this change is already on disk … at line 285" four times, and every
/// replay carried a note advising "an `oldString` long enough to match the one
/// place you mean". The model obeyed the note — its own words were "I will
/// ensure the `oldString` is exactly what the compiler is expecting" — and
/// re-sent the identical payload, because no spelling of `oldString` can make an
/// applied change apply again. A note that contradicts the refusal it is
/// attached to wins, being the last thing in the message, so it has to agree
/// with it.
///
/// The branches run most specific first because the refusals share words: a
/// near-miss refusal also says its `oldString` is not in the file, and the
/// brace-drift refusal says "not in place of it" in ordinary prose, so the
/// ambiguous-site branch keys on `places in` and not on `place`.
fn replay_advice(refusal: &str) -> &'static str {
    if refusal.contains("is where they part") {
        "The refusal quotes the file's own version of that line after \"the file says\". Put it \
         into the `oldString` in place of the line you wrote and send the same edit again. Every \
         other line of the `oldString` already matches the file, so re-reading it learns nothing \
         new and re-sending this call unchanged cannot apply."
    } else if refusal.contains("already on disk") {
        "The change is in the file already, so no version of this call can do anything. The \
         error the compiler still reports is at a different line: `read_file` the region that \
         line is in, copy what you see there into `oldString`, and change that instead."
    } else if refusal.contains("byte-identical") {
        "A call whose `newString` equals its `oldString` changes no characters however often it \
         is sent. Write the replacement the error asks for."
    } else if refusal.contains("places in") {
        "Widen the `oldString` — include the lines around it — until only the one place you mean \
         still matches, and send that."
    } else if refusal.contains("is not in") {
        "The text to replace has to be copied out of the file, not recalled: `read_file` the \
         region the diagnostics name and paste what it returns into `oldString` verbatim."
    } else {
        "The refusal above says what is wrong with this call. The next one has to differ in \
         substance, not in spelling."
    }
}

/// What a repeated refusal adds to the answer the tool already gave.
///
/// Appended, never substituted: a repair turn rebuilds a lean context from
/// scratch, so the tool's own explanation may be the only copy of it the model
/// still has, and replacing it would trade a refusal it can read for one it
/// cannot. The note's job is to say the replaying is the problem, which is the
/// one fact no tool result can know.
fn repeated_refusal_note(times: u32, refusal: &str) -> String {
    format!(
        "\n\n--- REPEATED CALL ({times} times) ---\n\
         This run has now sent this exact call, byte for byte, {times} times and every attempt \
         was refused. Sending it again cannot work: the refusal above is read from the file as \
         it stands, and these arguments describe the same change, so the next one will say the \
         same thing. {}",
        replay_advice(refusal)
    )
}

/// Upper bound on gate reports shown without a repair turn. Same reason the
/// write-pressure nudge has one: a directive repeated every turn stops being
/// information and starts being noise the model talks around.
const MAX_UNARMED_REPAIR_NOTES: u32 = 2;

/// Upper bound on clean grades refused because the artifact lost items. Two is
/// enough to tell a model that deleted code is not a fix and still let a run
/// that moved code between modules finish with a crate that builds and passes.
const MAX_SURFACE_REFUSALS: u32 = 2;

/// Whether to hand the model a rejection that no repair turn will be armed for.
///
/// `diagnostics_changed` is what makes this cheap: a write that produced
/// byte-identical rustc output was already shown that output, and repeating it
/// spends context on nothing. The count bounds the case where every write fails
/// differently and the loop would otherwise attach a fresh multi-KB directive
/// for the rest of the run.
fn should_note_unarmed_repair(diagnostics_changed: bool, notes_sent: u32) -> bool {
    diagnostics_changed && notes_sent < MAX_UNARMED_REPAIR_NOTES
}

/// The identity of each error in a rustc message: its code, the file it was
/// reported in, and the source line it pointed at — plus, when `workspace_root`
/// is given and that file is readable, the item the error was reported inside.
///
/// Line numbers cannot identify an error across a repair — every edit shifts
/// them — but the flagged line's own text survives a shift of many lines, and it
/// is what distinguishes a repair that swapped `&mut self` for `&self` from one
/// that actually moved toward a build.
///
/// The line text has one blind spot on its own, and `lr7b_v10` measured it: a
/// model that REWRITES the offending line changes the key every round while the
/// error stands exactly where it was. That probe reported `E0308` in
/// `budget_manager.rs` five times across the run, each round quoting a different
/// expression at the same place, so nothing repeated — no loop note, no
/// narrowing, and the turns went on poking the error it already knew. A
/// `(code, file, item)` triple survives both a line shift and a rewrite of the
/// line, and still separates an error that moves into a DIFFERENT item, which is
/// progress and must not read as a loop.
///
/// Both keys are emitted for one error, adjacent, the line text first: either
/// half matching an earlier round means the error did not move, and the line key
/// keeps the detector working when the file cannot be read.
fn error_signatures(errors: &str, workspace_root: Option<&std::path::Path>) -> Vec<String> {
    // One read per file per call: a report quotes the same crate's source for
    // every error, and this runs on every red build.
    let mut files: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let base = workspace_root.map(|root| {
        crate::automation::build_runner::cargo_manifest_dir(root)
            .unwrap_or_else(|_| root.to_path_buf())
    });
    let mut sigs: Vec<String> = Vec::new();
    let mut code = String::new();
    let mut site: Option<(String, String, usize, String)> = None;
    for raw in errors.lines() {
        let t = raw.trim();
        if let Some(rest) = t.strip_prefix("error[E") {
            code = format!(
                "E{}",
                rest.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
            );
            site = None;
        } else if t.starts_with("error:") {
            code.clear();
            site = None;
        } else if let Some(loc) = t.strip_prefix("--> ").or_else(|| t.strip_prefix("-->")) {
            let mut parts = loc.rsplit(['/', '\\']).next().unwrap_or(loc).split(':');
            let file = parts.next().unwrap_or("").to_string();
            // The key keeps the bare file name, but the item lookup has to OPEN
            // the file, and `budget_manager.rs` alone resolves from nowhere.
            let path = arrow_location(t)
                .map(|(p, _)| p)
                .unwrap_or_else(|| file.clone());
            site = parts
                .next()
                .and_then(|n| n.trim().parse::<usize>().ok())
                .map(|n| (code.clone(), file, n, path));
        } else if let Some((c, f, n, path)) = site.clone() {
            // The code-echo line for that location, e.g. `387 | &mut self,`.
            let num: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
            if num.parse::<usize>() == Ok(n) {
                if let Some((_, src)) = t.split_once('|') {
                    let src = src.trim();
                    if !src.is_empty() {
                        // Two errors can share one half of their identity — the
                        // same expression in two functions, or one function twice
                        // — and a key list that counts them twice makes
                        // "how many errors" mean nothing.
                        let line_key = format!("{c}|{f}|{src}");
                        if !sigs.contains(&line_key) {
                            sigs.push(line_key);
                        }
                        if let (Some(base), Some(root)) = (base.as_deref(), workspace_root) {
                            let lines = files
                                .entry(path.clone())
                                .or_insert_with(|| source_lines(base, root, &path));
                            if let Some(item) = enclosing_item_name(lines, n) {
                                let key = format!("{c}|{f}|inside {item}");
                                if !sigs.contains(&key) {
                                    sigs.push(key);
                                }
                            }
                        }
                        site = None;
                    }
                }
            }
        }
    }
    sigs
}

/// How many earlier gate messages contained at least one of these same errors.
fn prior_encounters(
    errors: &str,
    history: &[Vec<String>],
    workspace_root: Option<&std::path::Path>,
) -> usize {
    let sigs = error_signatures(errors, workspace_root);
    history
        .iter()
        .filter(|earlier| earlier.iter().any(|s| sigs.contains(s)))
        .count()
}

/// What to say when the model is re-reporting an error it has already been told
/// to fix: name the error, say how many times it has come back, and stop
/// describing the two faces of one contradiction as if they were separate work.
///
/// Returns "" for a first encounter, so the ordinary directive is untouched.
fn recurring_error_note(
    errors: &str,
    history: &[Vec<String>],
    workspace_root: Option<&std::path::Path>,
) -> String {
    let sigs = error_signatures(errors, workspace_root);
    if sigs.is_empty() || history.is_empty() {
        return String::new();
    }
    let repeats: Vec<&String> = {
        let mut seen: Vec<&String> = Vec::new();
        for s in &sigs {
            if history.iter().any(|h| h.contains(s)) && !seen.contains(&s) {
                seen.push(s);
            }
        }
        seen
    };
    // The mutability contradiction has one standard resolution, and naming it is
    // the difference between another failed flip and a build. Both halves may not
    // be in the SAME round: gateprobe5's model turned E0053 into E0594 by
    // flipping `&mut self` to `&self`, which erases the old signature instead of
    // repeating it, so exact-repeat detection alone never sees that pendulum. One
    // side now plus the other side last round is the whole signal — while an
    // unrelated error in a history that merely once contained the pair stays
    // silent, because lecturing about a borrow flip the model is not doing is
    // noise that costs excerpt budget.
    let current = error_codes_of(&sigs);
    let mut previous: Vec<&str> = Vec::new();
    for round in history {
        previous.extend(error_codes_of(round));
    }
    let holds = |codes: &[&str], code: &str| codes.contains(&code);
    let flip = (holds(&current, "E0053") && holds(&previous, "E0594"))
        || (holds(&current, "E0594") && holds(&previous, "E0053"))
        || (holds(&current, "E0053") && holds(&current, "E0594"));
    // The other pendulum `lr7b_v4` walked into: a field declared `&'a T` filled
    // with a local. Handing over the local is `E0308`; handing over `&local` is
    // `E0515`. The model alternates between the two faces forever, because each
    // edit is locally reasonable and neither is the requirement — the requirement
    // is an OWNER that outlives the call.
    let local_flip = (holds(&current, "E0308") && holds(&previous, "E0515"))
        || (holds(&current, "E0515") && holds(&previous, "E0308"))
        || (holds(&current, "E0308") && holds(&current, "E0515"));
    if repeats.is_empty() {
        let mut note = String::new();
        if flip {
            note.push_str(&format!("\n\n{MOVING_A_BORROW_ERROR_NOTE}"));
        }
        if local_flip {
            note.push_str(&format!("\n\n{BORROWING_A_LOCAL_ERROR_NOTE}"));
        }
        return note;
    }
    let mut listing = Vec::new();
    for s in &repeats {
        if listing.len() >= 3 {
            break;
        }
        let (code, file, src) = split_signature(s);
        // One error carries two keys, so both can repeat in the same round. Name
        // it by its line when a line of that code and file survived — that is the
        // text the model can aim an edit at — and fall back to the item when only
        // the place is the same, which is the case a line key cannot see.
        let prefix = format!("{code}|{file}|");
        match src.strip_prefix("inside ") {
            Some(line) => {
                if repeats
                    .iter()
                    .any(|o| o.starts_with(prefix.as_str()) && !is_item_signature(o))
                {
                    continue;
                }
                listing.push(format!("`{code}` in `{file}` inside `{line}`"));
            }
            None => listing.push(format!("`{code}` in `{file}` at `{src}`")),
        }
    }
    let named = listing.join(", ");
    let mut note = format!(
        "\n\nYou are re-reporting {named}: an earlier write of yours already produced it and \
         the repair that removed it brought it back. That is a loop, not progress. Two errors \
         that are one requirement seen from both sides cannot be fixed one at a time — find the \
         change that satisfies both, and do not restore the state that produced the first one."
    );
    if flip {
        note.push('\n');
        note.push_str(MOVING_A_BORROW_ERROR_NOTE);
    }
    if local_flip {
        note.push('\n');
        note.push_str(BORROWING_A_LOCAL_ERROR_NOTE);
    }
    note
}

/// The error codes carried by one round of signatures.
fn error_codes_of(round: &[String]) -> Vec<&str> {
    round
        .iter()
        .map(String::as_str)
        .filter_map(|s| s.split('|').next())
        .collect()
}

/// The part of a gate report the model has already been shown and has not moved.
///
/// `lr7b_v9` (qwen2.5-coder:7b, 643 s, NOT GREEN) is the reason this exists. The
/// fixture failed on two errors, and the harness gave the model both, correctly,
/// every round: the verbatim box at each site, both type declarations and the
/// per-code rules — verified by running `compile_repair_directive` on the real
/// report, which came out at 10 140 bytes with nothing clipped. The model fixed
/// neither. It issued four `edit_file` calls, ALL of them aimed at the same
/// second site, and never once touched `user_record: result_record` — after the
/// first one applied, rustfmt re-wrapped that line and every later replay of the
/// same `oldString` was refused as "not in the file", which still cost a repair
/// attempt. Shown two errors and left to choose, a 7B re-picks the one it knows
/// how to poke; a full, correct, non-contradictory directive does not make that
/// choice any easier.
///
/// So a repeat is answered by removing the choice: once the report has come back
/// with an error the model was already shown, only that error is shown again. The
/// dropped one is not forgiven — the next gate re-reports it, and by then this
/// round's edit has landed. `None` when there is nothing to narrow to, which
/// leaves the full report alone: a first encounter must still see every error, and
/// a report where all of them repeat gains nothing from hiding half.
fn recurring_error_focus(
    errors: &str,
    history: &[Vec<String>],
    workspace_root: Option<&std::path::Path>,
) -> Option<String> {
    if history.is_empty() {
        return None;
    }
    let mut blocks: Vec<String> = Vec::new();
    for line in errors.lines() {
        let t = line.trim_start();
        let is_heading = t.starts_with("error[") || t.starts_with("error:");
        // Each `error` heading opens a block that runs to the next one; cargo's
        // preamble before the first heading is not something the model can act on.
        if is_heading {
            blocks.push(line.to_string());
        } else if let Some(last) = blocks.last_mut() {
            last.push('\n');
            last.push_str(line);
        }
    }
    if blocks.len() < 2 {
        return None;
    }
    let mut kept: Vec<&String> = Vec::new();
    let mut dropped = 0usize;
    for block in &blocks {
        let sigs = error_signatures(block, workspace_root);
        // A block whose site cannot be read is kept: narrowing must never hide an
        // error the loop cannot prove the model has already seen.
        let repeats = sigs.is_empty() || history.iter().any(|h| sigs.iter().any(|s| h.contains(s)));
        if repeats {
            kept.push(block);
        } else {
            dropped += 1;
        }
    }
    if dropped == 0 || kept.is_empty() {
        return None;
    }
    let mut out = kept
        .iter()
        .map(|b| b.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Some(out)
}

/// Shown with a narrowed report, so the model does not read the missing error as
/// the whole job and stop.
const ONE_ERROR_AT_A_TIME_NOTE: &str = "Fix ONLY the error below, with one edit_file call that \
                                        changes the line it points at. Another error exists in \
                                        this file and will be shown to you again as soon as this \
                                        one is gone — do not spend this turn on it.";

/// The one standard resolution of an `E0053`/`E0594` pair, worded for a model
/// that has been reading it as two separate complaints.
const MOVING_A_BORROW_ERROR_NOTE: &str =
    "A method declared `&self` whose body must write is not repairable by flipping the \
     receiver between `&self` and `&mut self`: each flip just moves the error to the other \
     side of the same borrow. Make the field writable through a shared borrow instead \
     (`std::cell::RefCell` plus `borrow_mut()`, or a lock), keep the signatures as they are \
     declared, and fix the remaining errors rather than only this one.";

/// The one standard resolution of an `E0308`/`E0515` pair at one assignment.
///
/// Written because the loop watched a 7B discover both faces by itself and then
/// oscillate between them: `lr7b_v4` spent its last two repairs flipping
/// `user_record: result_record` (E0308) into `user_record: &result_record`
/// (E0515) and back. Each edit is defensible in isolation, which is exactly why
/// the model cannot see that the pair is one unsatisfied requirement — the field
/// needs an OWNER whose lifetime reaches past the call, and a local cannot be one
/// however many `&` are drawn in front of it.
const BORROWING_A_LOCAL_ERROR_NOTE: &str =
    "You are flipping one assignment between `x` and `&x`, which trades `E0308` for `E0515` \
     forever: the receiver wants `&'a T` and the value is owned by this function, so it dies at \
     the return no matter how many `&` are drawn in front of it. Stop editing that expression. \
     Move the VALUE into something that outlives the call first — the map or struct field of \
     `self` it belongs to — and then borrow it back from there: `self.map.insert(k, v);` \
     followed by `self.map.get(&k)` / `self.map.get_mut(&k)` (with `.expect(..)` if the insert \
     just made it certain), or the existing binding you inserted, taken out of `self` rather \
     than out of a local. Any `self.save()` / `&mut self` call has to happen BEFORE that borrow \
     is taken, not after.";

/// `(code, file, source line)` back out of a signature, tolerating a missing
/// code (a plain `error:` heading has none).
fn split_signature(sig: &str) -> (String, String, String) {
    let mut parts = sig.splitn(3, '|');
    let code = {
        let c = parts.next().unwrap_or("");
        if c.is_empty() {
            "error"
        } else {
            c
        }
    };
    (
        code.to_string(),
        parts.next().unwrap_or("").to_string(),
        parts.next().unwrap_or("").to_string(),
    )
}

/// Whether a signature names the ITEM an error sits in rather than the line it
/// was quoted at. `error_signatures` emits both, line text first.
fn is_item_signature(sig: &str) -> bool {
    sig.splitn(3, '|')
        .nth(2)
        .is_some_and(|tail| tail.starts_with("inside "))
}

/// What a gate message is about, in the few words a status line needs.
///
/// A model told "Compilation failed" about a build that passed and a test that
/// did not learns the wrong lesson: it goes looking for a type error instead of
/// reading the assertion that fired.
fn gate_failure_label(errors: &str) -> &'static str {
    if is_deletion_report(errors) {
        "The artifact shrank"
    } else if is_test_failure_report(errors) {
        "The crate's tests failed"
    } else {
        "Compilation failed"
    }
}

/// Does a build report carry rustc's own diagnostics, as opposed to a toolchain
/// that failed to launch or a manifest that could not be found?
///
/// Seeding a turn with `your crate does not compile: Failed to execute cargo
/// check: Os { code: 2 ... }` would send the model off to repair a build
/// environment it was never asked to touch, and it would arm the write lock to
/// do it. Every real rustc block has a location or an `error:` line; neither
/// failure mode above can produce one.
fn is_rustc_diagnostic_report(errors: &str) -> bool {
    errors.contains("--> ") || errors.contains("error[E") || errors.contains("error:")
}

/// What the write-side gate concluded about a batch that just touched Rust.
#[derive(Debug, PartialEq, Eq)]
enum WriteGrade {
    /// The build, or the tests once they are this run's contract, rejected the
    /// write. Carries the diagnostics to hand back.
    Repair(String),
    /// Clean build and green tests: the artifact is finished.
    Validated(String),
    /// Clean build, but the test gate abstained — never a pass.
    NotJudged(String),
    /// Clean build and nothing to report, because the tests were not consulted
    /// (the run has not been judged on them yet).
    BuildClean,
}

/// Grade a Rust write on the build and, when the loop owes it, on the tests.
///
/// The gate used to stop at the build, so a broken assertion only reached the
/// model when it PAUSED. testgate4 is the cost: repair 3/3 traded one red
/// assertion for another, nothing said so, and the run spent its last ~200s on a
/// `grep_search` turn before the wall clock ran out with the fixable failure
/// still standing. `tests` is `None` until the post-run test gate has rejected
/// the artifact — before that a port may legitimately be half-finished, and a
/// crate that does not compile has no test binary to run anyway.
/// Run the crate's tests against this write, or wait for the build?
///
/// A port the model is still authoring can build and still be half-written, so
/// `cargo test` there would grade an incomplete file — the gate used to wait
/// until the test side had already rejected the artifact once. That wait is wrong
/// for the other shape of task: when the dispatch names a file that was already
/// on disk, the crate arrived complete-but-broken, so a clean build *is* the
/// signal that the work is done. Measured on repair19, the last build repair
/// landed clean on turn 4 and the loop then spent twelve more turns and 500s
/// re-reading a finished file — one turn took 179.8s — with the tests run only by
/// the end-of-run validation. Grading on the turn the build goes clean both ends
/// the run where it earned it and removes the window in which a wandering model
/// can edit a green artifact back to red.
fn should_grade_tests_now(
    build_clean: bool,
    test_gate_engaged: bool,
    preexisting_target: bool,
) -> bool {
    build_clean && (test_gate_engaged || preexisting_target)
}

fn grade_write(
    build: Result<(), String>,
    tests: Option<Result<TestVerdict, String>>,
) -> WriteGrade {
    match build {
        // A broken build outranks any test verdict: `cargo test` on a crate that
        // does not compile cannot certify it.
        Err(e) => WriteGrade::Repair(e),
        Ok(()) => match tests {
            None => WriteGrade::BuildClean,
            Some(Err(e)) => WriteGrade::Repair(e),
            Some(Ok(TestVerdict::Passed(summary))) => WriteGrade::Validated(summary),
            Some(Ok(TestVerdict::NotJudged(why))) => WriteGrade::NotJudged(why),
        },
    }
}

/// Does the deliverable on disk satisfy the contract: the crate builds, *and*
/// its own tests pass, *and* the file the task named still declares the items it
/// arrived with?
///
/// `Ok` carries the tally cargo printed, so a success line reports a number the
/// gate observed instead of a bare "validation succeeded". A clean
/// `cargo check --all-targets` only proves the test code COMPILES; a probe run
/// used that to announce success over a port whose `cargo test` reported 68
/// passed / 2 failed.
///
/// `Err` is the report to hand back for repair — rustc's diagnostics, the
/// test gate's rustc-shaped blocks, or [`ArtifactSurface`]'s deleted-items
/// verdict — as delivered by [`arm_compile_repair`].
fn validate_written_artifact(
    workspace_root: &std::path::Path,
    written: &[&str],
    surface: &mut ArtifactSurface,
    ui_tx: &Sender<AgentToUiMessage>,
) -> Result<String, String> {
    match run_post_write_check(workspace_root, written) {
        Err(e) => Err(e),
        Ok(()) => {
            ui_tx
                .send(AgentToUiMessage::StatusUpdate(
                    "The build is clean — running the crate's tests...".to_string(),
                ))
                .ok();
            let tests = run_written_tests(workspace_root, written);
            let note = match &tests {
                Ok(TestVerdict::Passed(summary)) => summary.clone(),
                Ok(TestVerdict::NotJudged(why)) => format!("tests not judged: {why}"),
                Err(_) => String::new(),
            };
            match tests {
                Err(e) => Err(e),
                Ok(_) => match surface.withheld_report(workspace_root) {
                    Some(report) => Err(report),
                    // The refusals are what is bounded; a grade over a gutted
                    // file is not a pass. `lr7b_v6b` ended "Compiler validation
                    // succeeded — test result: ok. 53 passed" on a 495-byte
                    // rewrite of a 26 KB port because the refusal budget said the
                    // interrupting had to stop. Both exit gates that call this
                    // report the returned `Err` as NOT validated, so vetoing
                    // here covers them both.
                    None => match surface.collapse(workspace_root) {
                        Some(reason) => Err(reason),
                        None => Ok(format!("{}{}", note, surface.shrink_note(workspace_root))),
                    },
                },
            }
        }
    }
}

/// Whether the loop owes the run a final look at what is on disk.
///
/// The completion gate sits on the no-tool-call branch, so a run that spends its
/// turn or wall budget while repairing never falls through it. Only an artifact
/// task can be judged from files, and only if this run wrote Rust to judge.
fn should_close_validate(requires_artifact: bool, wrote_rust: bool, completed: bool) -> bool {
    requires_artifact && wrote_rust && !completed
}

/// `(attempts_allowed, is_a_test_failure)` for one gate report: which repair
/// budget a gate failure draws on, and how deep that budget is.
///
/// Syntax and assertions are different jobs, and a port can genuinely need
/// several of each: convert10's first attempt spent all three build repairs
/// turning `E0433` into `E0277` into a lifetime slip, got the library to
/// compile, and its `cargo test` then reported 77 passed / 7 failed — at which
/// point the shared counter refused the repair, the run died with seven fixable
/// assertions on disk, and it still had ~1400s of wall clock left. The gate that
/// grades the deliverable has to be able to ask for help.
fn repair_budget(errors: &str) -> (u32, bool) {
    if is_test_failure_report(errors) {
        (MAX_TEST_REPAIR_TURNS, true)
    } else {
        (MAX_COMPILE_REPAIR_TURNS, false)
    }
}

/// What a gate report looks like once the loop has decided to hand it to the
/// model: the directive to fix this file, plus the note that names the pendulum
/// when this report is the other face of an error already shown.
///
/// Every route that delivers a rejection goes through here — the armed repair,
/// the budget-exhausted note, and the lean write context a repair turn rebuilds —
/// because a run that flipped `&self`/`&mut self` until its budget ran out is
/// precisely the run that has to be told the two errors are one requirement, and
/// a route that skips the note just asks for the losing move again.
fn repair_directive_for_model(
    errors: &str,
    workspace_root: &std::path::Path,
    note: &str,
) -> String {
    format!("{}{note}", compile_repair_directive(errors, workspace_root))
}

/// The final turn of the rebuilt write context: the report the model has to act
/// on, in the order the loop trusts it.
///
/// An armed repair's directive comes first and already carries its pendulum note.
/// With none armed, a gate report that is standing but has no repair turn left
/// still beats a blind "write something now", which is how a red build gets handed
/// another file that does not compile.
fn write_context_directive(
    pending: Option<String>,
    gate_diagnostics: Option<&str>,
    write_tools: &[&str],
    workspace_root: &std::path::Path,
) -> String {
    match pending {
        Some(directive) => directive,
        None => match gate_diagnostics {
            Some(errors) => compile_repair_directive(errors, workspace_root),
            None => force_write_directive(&write_tools.join(" or ")),
        },
    }
}

/// Consume one repair attempt and push the directive that forces the next turn
/// to patch `written` instead of talking about it.
///
/// Both compile gates and the post-run test gate route through here so the
/// attempt count, the armed flags, the error history and the directive can never
/// be updated by one and not the other. The history is what lets the directive
/// tell a model that it is going in circles: a `&self`/`&mut self` flip produces
/// different text every time, so nothing else in the loop can see the repeat.
#[allow(clippy::too_many_arguments)]
fn arm_compile_repair(
    errors: &str,
    workspace_root: &std::path::Path,
    written: Option<&str>,
    attempts: &mut u32,
    max: u32,
    pending: &mut bool,
    pending_directive: &mut Option<String>,
    repair_target: &mut Option<String>,
    error_history: &mut Vec<Vec<String>>,
    message_history: &mut Vec<ChatMessage>,
    ui_tx: &Sender<AgentToUiMessage>,
) -> bool {
    if *attempts >= max {
        return false;
    }
    let note = recurring_error_note(errors, error_history, Some(workspace_root));
    // A second round on the same report drops the errors the model has not been
    // shown moving, so it cannot spend the turn re-poking the one it already knows.
    // The instruction to fix that one alone goes FIRST rather than into the trailing
    // note: `lr7b_v9`'s pendulum advice did ride at the end of a 10 KB directive and
    // the 7B read past it, back to rustc's own `help:` line three screens earlier.
    let focus = recurring_error_focus(errors, error_history, Some(workspace_root));
    let shown: &str = focus.as_deref().unwrap_or(errors);
    let directive = match &focus {
        Some(_) => format!(
            "{ONE_ERROR_AT_A_TIME_NOTE}\n{}",
            repair_directive_for_model(shown, workspace_root, &note)
        ),
        None => repair_directive_for_model(shown, workspace_root, &note),
    };
    error_history.push(error_signatures(errors, Some(workspace_root)));
    *attempts += 1;
    *pending = true;
    *pending_directive = Some(directive.clone());
    *repair_target = written.map(str::to_string);
    ui_tx
        .send(AgentToUiMessage::StatusUpdate(format!(
            "{} — sending the diagnostics back to the model for repair ({}/{})…",
            gate_failure_label(errors),
            *attempts,
            max
        )))
        .ok();
    message_history.push(ChatMessage {
        role: "user".to_string(),
        content: directive,
        name: None,
        tool_call_id: None,
        tool_calls: None,
    });
    true
}

/// The repair rule for one rustc error code, as a shape rather than a verb.
///
/// `lr7b_v2` (qwen2.5-coder:7b, 49.3 s, 8 turns) is the reason this exists. The
/// directive's old advice — "for a moved value, clone it before reuse; for
/// lifetimes or borrows, restructure to satisfy them" — is a correct summary of
/// Rust for someone who already knows Rust. The model obeyed it literally: it
/// cloned a record to satisfy an `E0308` on a field declared `&'a
/// PersistedUserRecord`, which turned a type error into a dangling-borrow error,
/// and spent the remaining two repairs doing it again with `&` in front. A clone
/// is a new temporary; cloning cannot lengthen a lifetime. That is the fact the
/// generic sentence hides, and the one a 7B has to be told.
fn recipe_for_code(code: &str) -> Option<&'static str> {
    // Each entry is (codes it covers, the recipe). Written as shapes with a
    // call-by-example because that is the form a small model can copy.
    const RECIPES: &[(&[&str], &str)] = &[
        (
            &["E0515", "E0597"],
            "the value is OWNED BY THIS FUNCTION, so it dies at the return and a `&` to it \
             cannot be handed out. A clone does NOT fix this — a clone is a fresh temporary. \
             Two shapes work: (1) put the value where it outlives the call — a field of `self`, \
             or `self.map.insert(key, value)` — and borrow it back from there, doing every \
             `self.save()` / `self.flush()` call BEFORE you take that borrow so it is the last \
             thing still live; (2) make the receiver own it: return `T` instead of `&T`, or \
             declare the field `T` instead of `&'a T`; (3) the borrow came from a temporary you \
             passed as an argument (`unwrap_or(&mut Foo { .. })`, `ok_or(&x)`) — that dies at the \
             statement's end no matter what you wrap it in, so drop the argument and read the \
             value out of the collection instead: `self.map.entry(k).or_insert_with(|| Foo { .. \
             })` and then borrow from `self.map`.",
        ),
        (
            &["E0502", "E0499"],
            "two borrows of the same data overlap. Copy out what the first borrow needs into a \
             plain local (`let n = x.len();`), let that borrow END — its block closes, or the \
             statement that uses it finishes — and only then start the second. A `&mut` may not \
             be taken while a `&` is still live.",
        ),
        (
            &["E0382"],
            "the owner was moved and used again. Clone AT THE MOVE, not at the use: `let kept \
             = x.clone();` then move `x`. If the receiver is a `&'a` field or return, cloning \
             does not help at all — follow the E0515 rule instead.",
        ),
        (
            &["E0308"],
            "expected and found types differ. Read the declaration of the receiver in the block \
             above and match it exactly: `&'a T` wants a borrow of something that outlives this \
             call (see E0515); `T` wants an owned value — `x.clone()`, `y.to_string()`; \
             `Option<T>` wants `Some(v)`; `&str` wants `s.as_str()`; `String` wants \
             `s.to_string()`; `Vec<T>` wants `v`, not `&v`. rustc's own `help: consider \
             borrowing here` only works if the name it points at OUTLIVES this call: when it is \
             a `let` local and the value is returned or stored, adding `&` trades `E0308` for \
             `E0515` — take the borrow from `self` instead (`self.map.get(&k)`) or keep the \
             field owned.",
        ),
        (
            &["E0599", "E0592", "E0609"],
            "that method or field is not on that type. Use only names that appear in the \
             declaration block above, or the standard conversions: `&x` for a borrow, \
             `x.clone()` for an owned copy, `x.to_string()` for `String`, `x.as_str()` for \
             `&str`, `x.get(k)` on a `HashMap`, `x.iter().next()` for the first element. Never \
             invent a method to bridge two types.",
        ),
        (
            &["E0277", "E0369", "E0282"],
            "a trait bound or a type is missing: `E0277` needs the derive on the type where it \
             is DECLARED (`#[derive(Clone, Debug)]`) or the call changed to avoid needing it; \
             `E0369` needs the operator applied to the same type on both sides (add `*`, `.get()`, \
             or `.clone()` to one side); `E0282` needs an annotation on the binding — `let v: \
             Vec<String> =`.",
        ),
    ];
    RECIPES
        .iter()
        .find(|(codes, _)| codes.contains(&code))
        .map(|(_, recipe)| *recipe)
}

/// The recipes for the codes actually present in this report, in the order the
/// compiler raised them.
fn error_code_recipes(errors: &str) -> String {
    // A rule the model never reads cannot guide it. With `lr7b_v5` the report
    // carried `E0308` (646 B) and `E0515` (856 B) — two bytes over the old cap,
    // so the harder of the two errors got silence while the easier one got an
    // essay. Clip the last one to a clause instead of dropping it.
    const MAX_RECIPE_CHARS: usize = 2_400;
    let mut codes: Vec<String> = Vec::new();
    let bytes = errors.as_bytes();
    let mut at = 0usize;
    while at + 9 < bytes.len() {
        if errors[at..].starts_with("error[E0") {
            let rest = &errors[at + 6..];
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric()))
                .unwrap_or(rest.len());
            let code = &rest[..end];
            if code.len() == 5 && !codes.iter().any(|c| c == code) {
                codes.push(code.to_string());
            }
            at += 6 + end;
            continue;
        }
        at += 1;
    }

    let mut out = String::new();
    let mut used = 0usize;
    for code in &codes {
        let Some(recipe) = recipe_for_code(code) else {
            continue;
        };
        let room = MAX_RECIPE_CHARS.saturating_sub(used);
        if room < 80 {
            break;
        }
        let Some(shown) = clip_to_clause(recipe, room) else {
            continue;
        };
        used += shown.len();
        out.push_str(&format!("`{code}`: {shown}\n"));
    }
    if out.is_empty() {
        return String::new();
    }
    format!(
        "The rule for each error code above — pick the shape it names, then make the SMALLEST \
         edit that fits it:\n{out}"
    )
}

/// The longest prefix of `text` fitting `room` bytes that ends after a clause
/// separator. `None` when no separator fits: a rule cut mid-instruction reads as
/// a complete instruction, which is worse than not pasting it.
fn clip_to_clause(text: &str, room: usize) -> Option<String> {
    if text.len() <= room {
        return Some(text.to_string());
    }
    let mut end = room;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let head = &text[..end];
    let cut = head
        .rfind(". ")
        .map(|i| i + 1)
        .or_else(|| head.rfind("; ").map(|i| i + 1))?;
    Some(head[..cut].to_string())
}

/// Directive for a turn that follows a write the gates rejected.
///
/// Carries the toolchain's own block and, beside it, the verbatim bytes on disk
/// at every line the block names, so the model can issue a targeted `edit_file`
/// patch without a `read_file` — which this turn does not offer. `write_file`
/// stays available for errors that force a whole-file restructure.
///
/// The two rejection kinds get different instructions because they need
/// opposite instincts. A compile error is satisfied by changing the code until
/// rustc accepts it. A failing test is satisfied by making the code do what the
/// test claims — and the cheap way out (delete the assertion, `#[ignore]` it,
/// flip the expected number) produces exactly the artifact that passes the loop
/// and fails the human.
fn compile_repair_directive(errors: &str, workspace_root: &std::path::Path) -> String {
    // A report the gate wrote itself — the deleted-items floor below — is not
    // compiler output, and dressing it up as `Real rustc output:` would hand the
    // model a text it cannot check against anything. Its own wording stands.
    if !is_rustc_diagnostic_report(errors) && !is_test_failure_report(errors) {
        return errors.to_string();
    }
    let is_test = is_test_failure_report(errors);
    // A failed assertion is corrected against the code the assertion computes
    // with, not against the line it sits on, so a test repair gets the whole test
    // and its helpers; a build repair keeps the tight window around each error.
    let excerpt = {
        let widened = if is_test {
            test_failure_context(workspace_root, errors)
        } else {
            String::new()
        };
        if widened.is_empty() {
            repair_error_excerpt(workspace_root, errors)
        } else {
            widened
        }
    };
    let deps = dependency_repair_rule(workspace_root);
    // The pasted diagnostics must not contradict `deps`, or the model is handed
    // two instructions and picks the one that cannot work.
    let errors =
        correct_missing_crate_help(errors, declared_dependencies(workspace_root).as_deref());
    let errors = errors.as_str();
    // What the error sites do not carry: the item each error is inside and the
    // types the diagnostics name, both read from disk by the loop.
    let api_context = repair_api_context(workspace_root, errors);
    let (headline, fix_rule) = if is_test {
        (
            "[System notice] The project BUILDS, but its own tests FAIL after your last write. \
             Real `cargo test` output:",
            "Each failure below is an assertion that did not hold. Fix the CODE so the behaviour \
             the test describes is what happens. Do NOT delete, weaken, `#[ignore]` or retype an \
             assertion to make the run green: a green run over a removed test is a failed task, \
             and the caller grades the tests, not the exit code. The one exception is an \
             assertion that contradicts a stub or mock defined in the same test module — if it \
             expects a real service's number while the module's own helper computes something \
             simpler, correct the assertion to what that helper produces and change no library \
             code. NOTE: {deps}."
                .to_string(),
        )
    } else {
        (
            "[System notice] The project compiled with ERRORS after your last write. Real rustc \
             output:",
            format!(
                "Fix every error, but NOTE: {deps}. Follow the per-code rule pasted below to the \
                 shape it names: a small edit that fits the rule beats an invented method that \
                 sounds like it would."
            ),
        )
    };
    let recipes = error_code_recipes(errors);
    format!(
        "{headline}\n\
         {errors}\n\
         {fix_rule}\n\
         {recipes}\n\
         Most of these are LOCAL, so fix them with edit_file instead of re-sending the \
         file: pass relativeFilePath plus edits, a list of {{oldString, newString}} pairs where \
         each oldString is copied VERBATIM from the file (indentation included) and widened until \
         it matches exactly ONE place. One small edit per problem is usually enough. Use write_file \
         only when a failure forces a whole-file restructure \u{2014} it replaces the entire file, \
         so every line must be present and correct.\n\
         In that output, a line shaped `259 | code` is its own RENDERING of \
         the file: the number, the `|` and the gutter spacing are not file text, so \
         never copy them. Use the verbatim blocks below as the source for oldString.\n\
         {excerpt}\n\
         {api_context}\n\
         Call edit_file (or write_file) in THIS turn \u{2014} do not describe the fix in prose. \
         Neither read_file nor list_dir exists this turn, so do not ask to look at anything. Keep \
         the public API and behavior intact and the #[cfg(test)] tests real."
    )
}

/// The exact bytes on disk around each error rustc reported, as a block the
/// repair directive can paste in.
///
/// A repair turn advertises neither `read_file` nor `list_dir` — withholding
/// exploration is what makes a small model fire the call instead of narrating
/// it — yet `edit_file` is only satisfiable with an `oldString` copied verbatim
/// from the file. Asked to reproduce the indentation of a 31 KB port it emitted
/// a dozen minutes earlier, the model guesses, the uniqueness match fails, and
/// the all-or-nothing batch rule refuses the whole turn, which leaves targeted
/// editing doing strictly less than the whole-file rewrite it was added to
/// replace. rustc already names the line of every error, so the loop reads
/// those lines itself and hands back text the model can copy directly.
///
/// Deliberately unnumbered: a `42 | ` gutter invites the model to copy the
/// prefix into `oldString`, which matches nothing. Each block is headed by its
/// own `path:lines`, so the location is still known.
fn repair_error_excerpt(workspace_root: &std::path::Path, errors: &str) -> String {
    // rustc prints locations relative to the directory `cargo check` ran from,
    // which is the manifest dir `run_compilation_check` resolved.
    let base = crate::automation::build_runner::cargo_manifest_dir(workspace_root)
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    // rustc names the offending line with `-->` but reports the *other* half of
    // a two-site error — `mutable borrow later used here`, `called from here`,
    // `defined here` — only as a caret inside its code block, which the filter
    // drops with the rest of the block. Reach further down than up so the line
    // the model has to change is inside the window, not just the one it was
    // addressed at.
    const BEFORE: usize = 2;
    const AFTER: usize = 5;
    const MAX_BLOCKS: usize = 6;
    const MAX_CHARS: usize = 1_800;

    // When the crate sits under the workspace root — the common case, e.g.
    // `rust_port/` — rustc's location is relative to the crate while the
    // `relativeFilePath` the agent must hand back is relative to the workspace.
    // Show the workspace-relative form or the patch is refused as "not found".
    let crate_rel = crate_display_prefix(&base, workspace_root);

    let hits = diagnostic_error_sites(errors);

    // Pass two: group each file's locations into windows. rustc reports a
    // borrow conflict as a PAIR of locations several lines apart (`immutable
    // borrow occurs here` and `mutable borrow later used here`), and a window
    // centred on the first one hides the line the model actually has to change
    // — so nearby locations widen a single block instead of being dropped or
    // duplicated.
    let mut order: Vec<String> = Vec::new();
    let mut per_file: std::collections::HashMap<String, Vec<usize>> =
        std::collections::HashMap::new();
    for (path, at) in hits {
        if !per_file.contains_key(&path) {
            order.push(path.clone());
        }
        per_file.entry(path).or_default().push(at);
    }

    let mut out = String::new();
    let mut blocks = 0usize;
    let mut cache: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();

    for path in &order {
        let mut lines_iter = per_file[path].clone();
        lines_iter.sort_unstable();
        let mut clusters: Vec<(usize, usize)> = Vec::new();
        for at in lines_iter {
            match clusters.last_mut() {
                Some(last) if at.saturating_sub(last.1) <= BEFORE + AFTER => last.1 = at,
                _ => clusters.push((at, at)),
            }
        }
        let display = if crate_rel.is_empty() {
            path.clone()
        } else {
            format!("{}/{}", crate_rel.trim_end_matches('/'), path)
        };
        for (first, last) in clusters {
            if blocks >= MAX_BLOCKS || out.len() >= MAX_CHARS {
                break;
            }
            let lines = cache
                .entry(path.clone())
                .or_insert_with(|| source_lines(&base, workspace_root, path));
            if lines.is_empty() {
                continue;
            }
            // A location past the end is real: rustc quotes the line the model
            // believed it wrote, and a truncated write leaves the file shorter.
            // Clamp to the last line that exists rather than slicing out of
            // range.
            let start = first
                .saturating_sub(1 + BEFORE)
                .min(lines.len().saturating_sub(1));
            let end = (last + AFTER).min(lines.len());
            if start >= end {
                continue;
            }
            let body = lines[start..end].join("\n");
            if out.len() + body.len() > MAX_CHARS {
                break;
            }
            out.push_str(&format!(
                "--- {display}:{}..{end} (verbatim from disk) ---\n```\n{body}\n```\n",
                start + 1
            ));
            blocks += 1;
        }
    }

    if out.is_empty() {
        return String::new();
    }
    format!(
        "Exact current file text at each error location, read from disk. Pass each header path \
         as `relativeFilePath` and build every `oldString` from the lines inside the box under \
         it \u{2014} that text is byte-for-byte what is in the file, indentation included. The \
         header line above a box is a label, not file text, and is never part of an \
         `oldString`:\n{out}"
    )
}

/// Every `(path, line)` rustc reported.
///
/// `-->` names the line the error was ADDRESSED at, while the numbered
/// code-echo lines inside the same block name the other sites of a
/// multi-location error. A two-site borrow conflict therefore needs both:
/// windowing only the `-->` line leaves the line the model has to change
/// outside the excerpt.
fn diagnostic_error_sites(errors: &str) -> Vec<(String, usize)> {
    let mut hits: Vec<(String, usize)> = Vec::new();
    let mut last_file: Option<String> = None;
    for line in errors.lines() {
        let body = line.trim();
        if let Some(loc) = arrow_location(body) {
            hits.push(loc);
            if let Some((path, _)) = hits.last() {
                last_file = Some(path.clone());
            }
            continue;
        }
        // An echo line belongs to the block it sits in, so it inherits the
        // file of the most recent `-->`.
        let Some(path) = last_file.clone() else {
            continue;
        };
        let digits = body.len() - body.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits == 0 || !body[digits..].trim_start().starts_with('|') {
            continue;
        }
        if let Ok(at) = body[..digits].parse::<usize>() {
            hits.push((path, at));
        }
    }
    hits
}

/// The leading identifier of a whitespace-delimited word: `impl<'a>` and
/// `pub(crate)` reduce to `impl` and `pub`.
fn leading_word(word: &str) -> &str {
    word.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .next()
        .unwrap_or_default()
}

/// Modifiers that may precede an item keyword on a declaration head.
fn is_head_modifier(word: &str) -> bool {
    matches!(
        word,
        "pub" | "async" | "unsafe" | "const" | "extern" | "default" | "move" | "dyn"
    )
}

/// The keyword of a declaration head, if this line opens an item.
///
/// Only the FIRST identifier that is not a modifier is consulted, so a `where`
/// continuation or a call site never reads as a head.
fn item_head_keyword(line: &str) -> Option<&'static str> {
    const ITEMS: [&str; 6] = ["fn", "impl", "trait", "struct", "enum", "union"];
    let t = line.trim_start();
    if t.starts_with('#') || t.starts_with("//") || t.starts_with('}') || t.starts_with('*') {
        return None;
    }
    for raw in t.split_whitespace().take(4) {
        let word = leading_word(raw);
        if is_head_modifier(word) {
            continue;
        }
        // The matched keyword is handed back from the static list, not as a
        // slice of `line`, so the caller is not tied to this file's buffer.
        return ITEMS.iter().find(|item| **item == word).copied();
    }
    None
}

/// The name an item head declares. For `impl`, the type the block is FOR, so
/// that a diagnostic naming `PersistedUserRecord` finds `impl PersistedUserRecord`.
fn declared_name(head: &str, kind: &str) -> Option<String> {
    let mut words = head.split_whitespace().peekable();
    loop {
        let word = words.peek().map(|w| leading_word(w))?;
        if word == kind {
            words.next();
            break;
        }
        if word.is_empty() || is_head_modifier(word) {
            words.next();
            continue;
        }
        return None;
    }
    // `impl<'a> Foo<'a>`, `impl Display for Foo`, `trait Foo: Send` — the name is
    // the first identifier after the keyword, unless a `for` follows, which makes
    // the block about the type on ITS right.
    let mut idents: Vec<String> = Vec::new();
    let mut after_for = None;
    for raw in words.take(6) {
        let word = leading_word(raw);
        if word.is_empty() || word == "where" || is_head_modifier(word) {
            continue;
        }
        if word == "for" {
            after_for = Some(idents.len());
            continue;
        }
        idents.push(word.to_string());
    }
    let named = match after_for {
        // `idents.len()` was the count when `for` appeared, so it indexes the
        // first identifier pushed after it.
        Some(at) => idents.get(at),
        None => idents.first(),
    };
    named.cloned()
}

/// The declaration head at `head_at`, folded onto one line through its return
/// type: rustc signatures wrap, and a half-copied `fn` head is a contract the
/// model cannot read.
fn fold_signature(lines: &[String], head_at: usize) -> String {
    let mut sig = lines[head_at].trim().to_string();
    let mut cursor = head_at;
    for _ in 0..8 {
        if sig.contains('{') || sig.ends_with(';') {
            break;
        }
        cursor += 1;
        if cursor >= lines.len() {
            break;
        }
        let cont = lines[cursor].trim();
        if cont.is_empty() {
            continue;
        }
        sig.push(' ');
        sig.push_str(cont);
    }
    sig
}

/// Whether these diagnostics ask for a restructure rather than a substitution.
///
/// A borrow error is fixed by moving a statement, not by changing a token, and
/// the statement that has to move is usually several lines away from the line
/// rustc points at: `lr7b_v3` kept failing at `budget_manager.rs:287` while the
/// `self.user_dict.insert(user, record)` that makes the answer obvious sat three
/// lines ABOVE the excerpt window (`BEFORE = 2`). The model was handed the wrong
/// half of its own function four repairs in a row.
fn needs_whole_item_context(errors: &str) -> bool {
    ["E0515", "E0597", "E0502", "E0499", "E0382", "E0716"]
        .iter()
        .any(|code| errors.contains(code))
}

/// The declarations a repair turn cannot see for itself.
///
/// A repair turn locks `read_file` on purpose — withholding exploration is what
/// makes a small model fire the call instead of narrating it — and
/// `repair_error_excerpt` hands back the bytes at each error site. What neither
/// gives the model is the API it has to conform to. Measured on the first
/// 8 GB-resident probe (`lr7b`, qwen2.5-coder:7b, 66.5 s, 10 turns): rustc said
/// `no method named `as_ref` found for struct `PersistedUserRecord`` at
/// `user_record: result_record.as_ref()`, and the model spent two of its three
/// repair turns inventing a conversion, because the field it was filling is
/// declared near the top of the same file as `pub user_record:
/// &'a PersistedUserRecord` and the answer to that error is literally `&x`.
/// A 7B does not lack the reasoning; it lacked the line.
///
/// So the loop reads the file itself and pastes two things it can name from the
/// diagnostics: the item each error sits in — its whole body when the code is a
/// borrowck one, since a dangling borrow at line 287 is unfixable without seeing
/// the `insert` at line 284 — and the definition or method list of every type the
/// error text mentions. Names with no matching declaration cost nothing but a
/// lookup.
fn repair_api_context(workspace_root: &std::path::Path, errors: &str) -> String {
    // Sized for the worst real case measured here: a 26 KB port with two E0515
    // sites, one of them in a 40-line method. Two whole methods plus the two
    // struct definitions the diagnostics name comes to ~3.4 KB, which is ~850 of
    // the 20 480 tokens the local turn declares.
    const MAX_API_CHARS: usize = 3_600;
    const MAX_ITEM_LINES: usize = 48;
    let base = crate::automation::build_runner::cargo_manifest_dir(workspace_root)
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let crate_rel = crate_display_prefix(&base, workspace_root);
    let mut names = type_names_in_diagnostics(errors);
    let whole_item = needs_whole_item_context(errors);

    let mut out = String::new();
    let mut cache: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let mut seen_blocks: Vec<String> = Vec::new();
    let mut seen_types: Vec<String> = Vec::new();

    // One pass per file: the item contexts come from the error sites, the type
    // definitions from a scan of the whole file, since the field that answers a
    // conversion error is usually nowhere near the line that errored.
    let mut files: Vec<String> = Vec::new();
    for (path, at) in diagnostic_error_sites(errors) {
        if !files.iter().any(|f| f == &path) {
            files.push(path.clone());
        }
        let lines = cache
            .entry(path.clone())
            .or_insert_with(|| source_lines(&base, workspace_root, &path));
        if lines.is_empty() {
            continue;
        }
        let display = if crate_rel.is_empty() {
            path.clone()
        } else {
            format!("{}/{}", crate_rel.trim_end_matches('/'), path)
        };
        // A borrow error is settled by which statement comes first, so the whole
        // function is the unit of evidence — but only when it is short enough to
        // afford, and only for the codes that need it.
        let item = match (whole_item, enclosing_fn_block(lines, at.saturating_sub(1))) {
            (true, Some((first, end))) if end - first <= MAX_ITEM_LINES => Some(format!(
                "--- {display}, the WHOLE function this error is inside, lines {}..{end} ---\n{}\n",
                first + 1,
                lines[first..end].join("\n")
            )),
            _ => enclosing_item_signature(lines, at)
                .map(|sig| format!("--- {display}, the item this error is inside ---\n{sig}\n")),
        };
        if let Some(block) = item {
            // The item head names a type the error text often does not. For
            // `lr7b_v5` the report said only `expected `&PersistedUserRecord``,
            // while the field that expects it belongs to `UpdateCostResult<'_>`,
            // declared thirty lines elsewhere as `pub user_record:
            // &'a PersistedUserRecord`. Without that struct the model can only
            // guess whether to add a `&` or drop one, and rustc's `help:` guesses
            // wrong.
            if let Some(head) = block.lines().nth(1) {
                for head_name in type_names_in_diagnostics(head) {
                    if names.len() < 24 && !names.iter().any(|n| n == &head_name) {
                        names.push(head_name);
                    }
                }
            }
            if !seen_blocks.contains(&block) {
                seen_blocks.push(block.clone());
                if out.len() + block.len() < MAX_API_CHARS {
                    out.push_str(&block);
                }
            }
        }
    }

    for path in &files {
        let Some(lines) = cache.get(path) else {
            continue;
        };
        let display = if crate_rel.is_empty() {
            path.clone()
        } else {
            format!("{}/{}", crate_rel.trim_end_matches('/'), path)
        };
        for (name, text) in named_type_definitions(lines, &names, &mut seen_types) {
            let block = format!("--- {display} declares {name} ---\n{text}\n");
            if out.len() + block.len() < MAX_API_CHARS {
                out.push_str(&block);
            }
        }
    }

    if out.is_empty() {
        return String::new();
    }
    format!(
        "Declarations from the same file, read by the loop because no read tool exists this \
         turn. A conversion, lifetime or field error against one of these is fixed by what it \
         declares — never invent a method to satisfy it. The header above each box labels it; \
         only the lines inside a box are file text:\n{out}"
    )
}

/// The nearest declaration head above `at` whose body spans the error, folded
/// onto one line.
fn enclosing_item_signature(lines: &[String], at: usize) -> Option<String> {
    const MAX_LOOKBACK: usize = 60;
    let idx = at.checked_sub(1)?;
    if idx >= lines.len() {
        return None;
    }
    let start = idx.saturating_sub(MAX_LOOKBACK);
    for back in (start..=idx).rev() {
        if item_head_keyword(&lines[back]).is_some() {
            return Some(fold_signature(lines, back));
        }
    }
    None
}

/// `fn update_cost` — the item `at` sits inside, named rather than quoted.
///
/// The signature's own text is unusable as a repeat key: the model edits heads,
/// and `lr7b_v4`'s pendulum was exactly a `&self`/`&mut self` change in the
/// declaration the error belonged to. A name survives that. When the head is
/// recognised but the name is not, the answer is `None` rather than the next
/// head up: attributing an error to the `impl` around an unparsed `fn` would
/// merge two methods into one identity, which is the opposite of what a repeat
/// key is for.
fn enclosing_item_name(lines: &[String], at: usize) -> Option<String> {
    const MAX_LOOKBACK: usize = 60;
    let idx = at.checked_sub(1)?;
    if idx >= lines.len() {
        return None;
    }
    let start = idx.saturating_sub(MAX_LOOKBACK);
    for back in (start..=idx).rev() {
        let head = lines[back].trim_start();
        let Some(kind) = item_head_keyword(head) else {
            continue;
        };
        return declared_name(head, kind).map(|name| format!("{kind} {name}"));
    }
    None
}

/// Upper-case identifiers the diagnostics mention, as definition lookups.
fn type_names_in_diagnostics(errors: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for line in errors.lines() {
        let body = line.trim();
        if body.starts_with("-->") || body.starts_with('=') || body.starts_with("note:") {
            continue;
        }
        let mut token = String::new();
        for ch in body.chars() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                token.push(ch);
            } else {
                if token.len() >= 3
                    && token.as_bytes()[0].is_ascii_uppercase()
                    && !token.starts_with("E0")
                    && !names.iter().any(|n| n == &token)
                {
                    names.push(token.clone());
                }
                token.clear();
            }
            if names.len() >= 16 {
                return names;
            }
        }
    }
    names
}

/// Definitions (struct/enum/trait) or method lists (impl) for `names`, found in
/// `lines`, skipping any already handed over through `seen_types`.
///
/// Each block comes back as `(name, text)`; the caller adds the path header.
fn named_type_definitions(
    lines: &[String],
    names: &[String],
    seen_types: &mut Vec<String>,
) -> Vec<(String, String)> {
    const MAX_BODY_LINES: usize = 22;
    const MAX_IMPL_FNS: usize = 12;
    let mut blocks = Vec::new();

    let mut head_at = 0usize;
    while head_at < lines.len() {
        let Some(kind) = item_head_keyword(&lines[head_at]) else {
            head_at += 1;
            continue;
        };
        if kind == "fn" {
            head_at += 1;
            continue;
        }
        let Some(name) = declared_name(lines[head_at].trim(), kind) else {
            head_at += 1;
            continue;
        };
        if !names.contains(&name) || seen_types.iter().any(|s| s == &name) {
            head_at += 1;
            continue;
        }
        seen_types.push(name.clone());

        let mut depth: i32 = 0;
        let mut body: Vec<String> = Vec::new();
        let mut sigs: Vec<String> = Vec::new();
        let mut cursor = head_at;
        while cursor < lines.len() {
            let raw = lines[cursor].trim_end();
            let trimmed = raw.trim_start();
            if cursor > head_at {
                if kind == "impl" {
                    // Only methods directly in this block: `depth == 1` is the
                    // impl's own brace level, so a nested closure never reads
                    // as part of the API.
                    if depth == 1
                        && item_head_keyword(trimmed) == Some("fn")
                        && sigs.len() < MAX_IMPL_FNS
                    {
                        sigs.push(fold_signature(lines, cursor));
                    }
                } else if body.len() < MAX_BODY_LINES {
                    body.push(trimmed.to_string());
                }
            }
            depth += raw.matches('{').count() as i32 - raw.matches('}').count() as i32;
            cursor += 1;
            if depth <= 0 || (kind != "impl" && body.len() >= MAX_BODY_LINES) {
                break;
            }
        }

        let mut text = fold_signature(lines, head_at);
        if kind == "impl" {
            if !sigs.is_empty() {
                text.push_str("\n  ");
                text.push_str(&sigs.join("\n  "));
            }
            text.push_str(" { /* bodies withheld */ }");
        } else if !body.is_empty() {
            text.push('\n');
            text.push_str(&body.join("\n"));
        }
        blocks.push((name, text));
        head_at = cursor;
    }
    blocks
}

/// Peel a rustc location out of a `-->` line: `--> src\budget_manager.rs:535:1`
/// becomes `("src/budget_manager.rs", 535)`.
///
/// The `:line:col` is peeled from the RIGHT because a location can be absolute,
/// and on Windows that means a drive letter: splitting left-to-right reads `C`
/// as the whole path and drops the error.
fn arrow_location(body: &str) -> Option<(String, usize)> {
    let rest = body.trim().strip_prefix("-->")?;
    let mut bits = rest.trim().rsplitn(3, ':');
    let (Some(_col), Some(num), Some(raw_path)) = (bits.next(), bits.next(), bits.next()) else {
        return None;
    };
    let at = num.trim().parse::<usize>().ok()?;
    let path = raw_path.trim().replace('\\', "/");
    (!path.is_empty()).then_some((path, at))
}

/// The crate's own directory as the agent sees it, so a `relativeFilePath` built
/// from a rustc location actually resolves. Empty when the crate IS the root.
fn crate_display_prefix(base: &std::path::Path, workspace_root: &std::path::Path) -> String {
    base.strip_prefix(workspace_root)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

/// A source file as CR-stripped lines, tried against the crate dir first (rustc's
/// paths are relative to where cargo ran) and then the workspace root.
fn source_lines(
    base: &std::path::Path,
    workspace_root: &std::path::Path,
    path: &str,
) -> Vec<String> {
    let joined = base.join(path);
    let fallback = workspace_root.join(path);
    let text = std::fs::read_to_string(&joined)
        .or_else(|_| std::fs::read_to_string(&fallback))
        .unwrap_or_default();
    text.lines()
        .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
        .collect()
}

/// The shapes a Rust function header can start with, after leading whitespace.
const FN_HEADS: [&str; 6] = [
    "fn ",
    "pub fn ",
    "async fn ",
    "unsafe fn ",
    "pub(crate) fn ",
    "pub(super) fn ",
];

/// Does this line open a function named `name`?
fn defines_fn(line: &str, name: &str) -> bool {
    let t = line.trim_start();
    FN_HEADS.iter().any(|h| {
        t.strip_prefix(*h)
            .and_then(|rest| rest.strip_prefix(name))
            .is_some_and(|after| after.starts_with('(') || after.starts_with('<'))
    })
}

/// The 0-based `start..end` of the `fn` block containing `at`, with the
/// attributes above it (`#[test]`, `#[cfg(test)]`) included so the model sees why
/// the block exists. Anything that cannot be placed comes back as `None` rather
/// than as a guessed window.
fn enclosing_fn_block(lines: &[String], at: usize) -> Option<(usize, usize)> {
    let indent_of = |l: &str| l.chars().count() - l.trim_start().chars().count();
    let mut start = at.min(lines.len().saturating_sub(1));
    let indent;
    loop {
        let t = lines[start].trim_start();
        if FN_HEADS.iter().any(|p| t.starts_with(p)) {
            indent = indent_of(&lines[start]);
            break;
        }
        if start == 0 {
            return None;
        }
        start -= 1;
    }
    // The attribute lines sit directly above the header, at the same column.
    let first = (0..start)
        .rev()
        .take_while(|&i| indent_of(&lines[i]) == indent && lines[i].trim_start().starts_with('#'))
        .last()
        .unwrap_or(start);
    // The block ends where its brace count returns to zero AND the line is a lone
    // closer. Either test alone is wrong: depth alone walks straight past a
    // one-line `fn main() {}` and claims whatever line follows it, while the first
    // `}` in the header's column is a nested item's closer inside a `mod`. Counting
    // with `brace_delta` rather than by characters is what keeps a `{` in a string
    // or comment from ending the block two lines early.
    let mut depth = 0i32;
    let mut end = start;
    while end < lines.len() {
        depth += crate::registry::system_tools::brace_delta(&lines[end]);
        if end > start && depth == 0 && lines[end].trim() == "}" {
            return Some((first, end + 1));
        }
        end += 1;
    }
    None
}

/// The whole failing test, plus the same-file helpers it calls, for a test
/// repair turn.
///
/// A compile error is local — the reported line and its neighbours carry the
/// fix, which is what `repair_error_excerpt` is for. A failed assertion is not:
/// `assert!((projected - 0.009).abs() < f64::EPSILON)` can only be corrected
/// against the code that computes `projected`, so a seven-line window hands the
/// model the symptom without the fact. testgate3 measured the consequence on
/// qwen3.6:35b-a3b — two consecutive repair turns that fired ZERO calls and asked
/// for `read_file` five times, because the number it needed was not in front of
/// it and the turn had withheld the one tool that could fetch it.
///
/// The loop can read what the model would have read, so it does, and the turn
/// stays locked: the entire `#[test] fn` block and the definitions of the `fn`s it
/// calls, unnumbered and workspace-relative exactly like `repair_error_excerpt`,
/// so the same verbatim-copy rule covers both.
fn test_failure_context(workspace_root: &std::path::Path, errors: &str) -> String {
    const MAX_SITES: usize = 4;
    const MAX_HELPERS: usize = 4;
    const MAX_CHARS: usize = 4_000;

    let base = crate::automation::build_runner::cargo_manifest_dir(workspace_root)
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let prefix = crate_display_prefix(&base, workspace_root);

    let mut sites: Vec<(String, usize)> = Vec::new();
    for line in errors.lines() {
        let Some((path, at)) = arrow_location(line) else {
            continue;
        };
        if !path.ends_with(".rs") || sites.len() >= MAX_SITES {
            continue;
        }
        sites.push((path, at));
    }

    let mut out = String::new();
    let mut shown: Vec<(String, usize, usize)> = Vec::new();
    for (path, at) in sites {
        let lines = source_lines(&base, workspace_root, &path);
        if lines.is_empty() {
            continue;
        }
        let Some((first, end)) = enclosing_fn_block(&lines, at.saturating_sub(1)) else {
            continue;
        };
        let display = if prefix.is_empty() {
            path.clone()
        } else {
            format!("{}/{}", prefix.trim_end_matches('/'), path)
        };
        let body = lines[first..end].join("\n");
        if out.len() + body.len() > MAX_CHARS {
            break;
        }
        out.push_str(&format!(
            "--- the whole failing test, {display}:{}..{end} ---\n{body}\n",
            first + 1
        ));
        shown.push((path.clone(), first, end));

        // The helpers are the point: the assertion's expected value is a fact
        // about the mock or constructor it calls, and a model that has to guess
        // those asks for a read it is not allowed to make. Match only names this
        // file itself defines, so `format!` and `Vec::len` never bloat the block.
        let mut helpers = 0usize;
        for name in called_names(&body) {
            if helpers >= MAX_HELPERS {
                break;
            }
            let Some(def) = (0..lines.len()).find(|&i| defines_fn(&lines[i], &name)) else {
                continue;
            };
            if shown
                .iter()
                .any(|(p, s, e)| p == &path && *s <= def && def < *e)
            {
                continue;
            }
            let Some((hs, he)) = enclosing_fn_block(&lines, def) else {
                continue;
            };
            shown.push((path.clone(), hs, he));
            helpers += 1;
            let block = lines[hs..he].join("\n");
            if out.len() + block.len() > MAX_CHARS {
                break;
            }
            out.push_str(&format!(
                "--- what it calls, {display}:{}..{he} ---\n{block}\n",
                hs + 1
            ));
        }
    }

    if out.is_empty() {
        return String::new();
    }
    format!(
        "The failing test in full, and the definitions it depends on, read off the file as it \
         stands on disk. Nothing above the `---` markers needs to be re-read: pass each header \
         path as `relativeFilePath` and build every `oldString` from the lines under it — \
         indentation included, the header itself is not file text. Correct the assertion to the \
         value this code actually produces; changing these functions is not the fix.\n```\n{out}```"
    )
}

/// Identifiers the block appears to call: the word in front of every `(`. Cheap
/// on purpose — it only proposes candidates, and one the file does not define is
/// dropped by the caller, so a std method or a macro (`assert_eq!`, `format!`)
/// costs a lookup and never a paragraph of the directive.
fn called_names(body: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for (i, c) in body.char_indices() {
        if c != '(' {
            continue;
        }
        let head = &body[..i];
        let name = head
            .rsplit(|c: char| !(c.is_alphanumeric() || c == '_'))
            .next()
            .unwrap_or("");
        if name.len() >= 3
            && !name.starts_with(|c: char| c.is_ascii_digit())
            && !names.iter().any(|n| n == name)
        {
            names.push(name.to_string());
        }
    }
    names
}

/// The imperative close-out used on the reconstructed write-only turn. Unlike
/// `artifact_delivery_directive` (a general "you owe us files" notice), this is
/// issued only after exploration is locked and the model keeps narrating intent
/// without firing the call. Measured against the live qwen3.6:35b-a3b the
/// generic notice still let it dump the markdown as prose; this wording — stop
/// exploring, incomplete is fine, emit the call in THIS turn — was what flipped
/// the model from narration to a real `write_file` tool call in the probe.
///
/// It names the tools the turn actually advertises, because the locked set is
/// not always `write_file` alone: a task that repairs a file already on disk
/// offers `edit_file` too, and telling the model that only a whole-file write
/// counts would push it straight back into the output-budget failure this tool
/// was added to avoid. The fenced-code-block ban is measured behaviour, not
/// speculation — on a first-write turn there is no fence recovery, so a dumped
/// file is lost and the nudge is burned.
fn force_write_directive(write_names: &str) -> String {
    format!(
        "[System instruction] STOP exploring and STOP describing the file in prose or as a fenced \
         code block. You already have enough context to produce the deliverable. In THIS turn, \
         emit a {write_names} tool call — not sentences — that writes the file at the exact path \
         the task names, using only the directory contents you have already seen. A short or \
         partially-complete file is acceptable and far better than no file at all. The ONLY \
         correct response now is that tool call."
    )
}

/// Remove unparsed tool-call markup from a no-tool-call turn's content.
/// A stream cut mid-call can leave a lone function-close tag (the inline
/// parser consumed the opener and name), a bare call-opener with no body,
/// or a half-emitted function-name tag. Such residue is never prose: if it
/// sits at the end of the turn it defeats the announcement detector's
/// last-line check, and if it is the whole turn the model's intended tool
/// call silently degrades into a "final answer". Returns the cleaned text
/// (with markup lines dropped) plus whether residue was present. Tag
/// literals are assembled at runtime so this source stays copy-paste safe.
/// Character-level scanner for the wrapperless native-fragment dialect
/// token-plan gateways emit into delta content: fn-open markers with the
/// tool name, parameter-open markers with keys and value segments, and
/// fn-close or call-close terminators. Unlike a line-based reader it
/// also catches markup embedded mid-line inside prose, which live run 3
/// showed when the gateway interleaved fragments with sentences.
/// Returns the prose (markup spans removed), complete (name, arguments)
/// calls, and whether a dialect marker was seen. An EOF-truncated call
/// yields no calls at all and flags residue, so the loop asks the model
/// to re-issue it rather than executing a half-parseable tool call.
/// Marker literals are assembled from escapes at runtime: raw control
/// sequences in source are hazardous to copy through editor channels.
fn parse_function_fragments(
    content: &str,
) -> (String, Vec<(String, serde_json::Map<String, Value>)>, bool) {
    let lt = '\u{3c}';
    let fn_open: String = format!("{}function=", lt);
    let param_open: String = format!("{}parameter=", lt);
    let param_close: String = format!("{}{}parameter{}", lt, '/', '>');
    let fn_close: String = format!("{}{}function{}", lt, '/', '>');
    let call_close: String = format!("{}{}tool_call{}", lt, '/', '>');
    // The bracket-plus-slash prefix: catches a closer truncated inside its
    // own name, which never matches the complete close literals.
    let near_close: String = format!("{}{}", lt, '/');
    fn find_lt(content: &str, from: usize, part: &str) -> Option<usize> {
        // Index of the next angle bracket immediately followed by `part`
        // (case-insensitive). Close markers never match: their slash sits
        // between the bracket and the name.
        let lowered = content.to_ascii_lowercase();
        let mut rel = from + lowered[from..].find(part)?;
        loop {
            // A match at index 0 has no preceding bracket: keep searching.
            if rel > 0 && content.as_bytes()[rel - 1] == b'<' {
                return Some(rel - 1);
            }
            rel += 1 + lowered[rel + 1..].find(part)?;
        }
    }
    let mut calls: Vec<(String, serde_json::Map<String, Value>)> = Vec::new();
    let mut prose = String::new();
    let mut i = 0usize;
    let mut saw = false;
    while let Some(s) = find_lt(content, i, "function=") {
        saw = true;
        // `s` is the bracket index; fn_open (bracket + name) spans s..s+len.
        let mut j = s + fn_open.len();
        // Earliest real terminator wins: '>' closes the marker normally, '<'
        // is the `name<` spelling, a newline means the bracket never landed.
        let name_end = [
            content[j..].find('>'),
            content[j..].find('\n'),
            content[j..].find('<'),
        ]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(content.len() - j);
        // Which character terminated the name decides whether the call was
        // well-formed: '>' or '<' are real terminators, a newline or EOF
        // means the stream was cut between the marker and its bracket.
        // Registered tool names are single tokens, so a newline-terminated
        // name is untrustable — bail to the residue path rather than
        // executing a guessed call.
        let term_ok = matches!(
            content[j + name_end..].chars().next(),
            Some('>') | Some('<')
        );
        let name = content[j..j + name_end]
            .trim()
            .trim_matches(|c| c == '<' || c == '>')
            .to_string();
        if !term_ok || name.is_empty() || name.contains('<') || name.contains('\n') {
            return (content.to_string(), Vec::new(), true);
        }
        let piece = content[i..s].trim();
        if !piece.is_empty() {
            prose.push_str(piece);
            prose.push('\n');
        }
        j += name_end + 1;
        let mut args = serde_json::Map::new();
        loop {
            let stop = [
                content[j..].find(&fn_close).map(|p| j + p),
                content[j..].find(&call_close).map(|p| j + p),
                find_lt(content, j, "function="),
            ]
            .into_iter()
            .flatten()
            .min();
            let ps = find_lt(content, j, "parameter=");
            if let Some(p) = ps {
                if stop.is_none_or(|st| p < st) {
                    // p is the bracket index; param_open spans p..p+len.
                    let mut k = p + param_open.len();
                    // The key ends at the first real terminator: its closing
                    // bracket, or a newline when the bracket never arrived.
                    // Taking the earliest (not the first found) matters: a
                    // bracket far past the newline belongs to a later tag.
                    let key_end = [
                        content[k..].find('>'),
                        content[k..].find('\n'),
                        content[k..].find('\r'),
                    ]
                    .into_iter()
                    .flatten()
                    .min()
                    .unwrap_or(content.len() - k);
                    // A bracket-only key means the name was truncated at EOF.
                    let raw_key = &content[k..k + key_end];
                    if raw_key.trim().is_empty() {
                        return (content.to_string(), Vec::new(), true);
                    }
                    let key = content[k..k + key_end]
                        .trim()
                        .trim_matches(|c| c == '"' || c == '\'' || c == '>' || c == '<')
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_string();
                    k += key_end;
                    if content[k..].starts_with('>') {
                        k += 1;
                    }
                    let val_end = [
                        content[k..].find(&param_close).map(|q| k + q),
                        content[k..].find(&fn_close).map(|q| k + q),
                        content[k..].find(&call_close).map(|q| k + q),
                        // Any closer, complete or truncated inside its own
                        // name: the value never runs past a slash-lead tag.
                        content[k..].find(&near_close).map(|q| k + q),
                        find_lt(content, k, "parameter="),
                        find_lt(content, k, "function="),
                    ]
                    .into_iter()
                    .flatten()
                    .min()
                    .unwrap_or(content.len());
                    let val = content[k..val_end].trim().to_string();
                    if !key.is_empty() {
                        args.insert(key, Value::String(val));
                    }
                    j = if content[val_end..].starts_with(&param_close) {
                        val_end + param_close.len()
                    } else {
                        val_end
                    };
                    continue;
                }
            }
            match stop {
                Some(c) => {
                    // Consume the close marker whichever flavor it is.
                    j = if content[c..].starts_with(&fn_close) {
                        c + fn_close.len()
                    } else if content[c..].starts_with(&call_close) {
                        c + call_close.len()
                    } else {
                        c
                    };
                }
                None => {
                    // Call never closed: consumed to EOF, still complete.
                    j = content.len();
                }
            }
            break;
        }
        calls.push((name, args));
        i = j;
    }
    if !saw {
        return (content.to_string(), Vec::new(), false);
    }
    let piece = content[i..].trim();
    if !piece.is_empty() {
        prose.push_str(piece);
    }
    (prose.trim().to_string(), calls, true)
}

fn strip_tool_markup_residue(content: &str) -> (std::borrow::Cow<'_, str>, bool) {
    fn tag(name: &str) -> String {
        format!("<{}>", name)
    }
    fn tag_open(name: &str) -> String {
        format!("<{}", name)
    }
    let close_fn = tag("/function");
    let close_call = tag("/tool_call");
    let open_call = tag("tool_call");
    let open_fn = tag_open("function=");
    let open_param = tag_open("parameter=");
    let close_param = tag("/parameter");
    // Wider residue spellings from live providers: the Claude-family inline
    // tool dialect wraps invoke/parameter tags in an "antml" prefix, and
    // streams cut mid-call leave open-only or close-only fragments. The tag
    // fragments are built from escapes/helper fns on purpose: complete
    // control sequences in source are hazardous to copy through editors,
    // and open-fragment matching catches every partial truncation.
    const LT: char = '\u{3c}';
    const GT: char = '\u{3e}';
    let open_invoke = format!("{}invoke", LT);
    let open_antml = format!("{}antml", LT);
    // The close spelling is  LT + "/" + "antml..." — a distinct prefix.
    let close_antml = format!("{}/antml", LT);
    let close_invoke = tag("/invoke");
    let close_calls = format!("{}/tool_calls{}", LT, GT);
    // Batch wrappers seen from the same provider: the call list is closed
    // with a function-plural tag no earlier marker matched, and the models
    // reasoning block arrives inside the content channel.
    let open_fn_calls = tag_open("function_calls");
    let close_fn_calls = tag("/function_calls");
    let open_thinking = tag_open("thinking");
    let close_thinking = tag("/thinking");
    let dsml = String::from("\u{ff5c}DSML\u{ff5c}");

    let lower = content.to_lowercase();
    let has_residue = content.contains(&close_fn)
        || content.contains(&open_call)
        || content.contains(&open_fn)
        || content.contains(&open_param)
        || lower.contains(&open_invoke)
        || content.contains(&open_antml)
        || content.contains(&close_antml)
        || content.contains(&close_invoke)
        || content.contains(&close_calls)
        || content.contains(&open_fn_calls)
        || content.contains(&close_fn_calls)
        || content.contains(&open_thinking)
        || content.contains(&close_thinking)
        || lower.contains(&dsml)
        || content.contains("[Calling tool ");
    if !has_residue {
        return (std::borrow::Cow::Borrowed(content), false);
    }
    // Markup is not always on its own line: live run 3 showed the gateway
    // embedding a call opener at the end of a prose sentence. Cut every
    // marker span wherever it sits, then let the whole-line filter below
    // clean up what is left of standalone fragment lines.
    let mut cut = String::with_capacity(content.len());
    let mut pos = 0usize;
    while pos < content.len() {
        let rest = &content[pos..];
        let rest_lower = rest.to_ascii_lowercase();
        let complete = [
            close_calls.as_str(),
            close_fn_calls.as_str(),
            close_thinking.as_str(),
            close_invoke.as_str(),
            close_call.as_str(),
            close_fn.as_str(),
            open_call.as_str(),
        ];
        if let Some(m) = complete.iter().find(|m| rest_lower.starts_with(**m)) {
            pos += m.len();
            continue;
        }
        let opener = [
            open_fn.as_str(),
            open_param.as_str(),
            open_antml.as_str(),
            open_invoke.as_str(),
            open_fn_calls.as_str(),
            open_thinking.as_str(),
        ];
        if let Some(m) = opener.iter().find(|m| rest_lower.starts_with(**m)) {
            // Consume the marker through its own closing bracket; a stream
            // cut mid-marker leaves none, so consume to the end.
            let _ = m;
            let span = rest[1..].find(GT).map(|p| p + 2).unwrap_or(rest.len());
            pos += span;
            continue;
        }
        if rest_lower.starts_with("[calling tool ") {
            let end = rest
                .find("']")
                .map(|p| p + 2)
                .or_else(|| rest.find('\n'))
                .unwrap_or(rest.len());
            pos += end;
            continue;
        }
        if let Some(ch) = rest.chars().next() {
            cut.push(ch);
            pos += ch.len_utf8();
        } else {
            pos += 1;
        }
    }
    let kept: Vec<&str> = cut
        .lines()
        .filter(|l| {
            let t = l.trim();
            !(t == close_fn
                || t == close_call
                || t == open_call
                || t == close_invoke
                || t == close_fn_calls
                || t == close_thinking
                || t.starts_with(&open_fn)
                || t.starts_with(&open_param)
                || t.starts_with(&open_invoke)
                || t.starts_with(&open_antml)
                || t.starts_with(&close_antml)
                || t.starts_with(&open_fn_calls)
                || t.starts_with(&open_thinking)
                || (t.starts_with('>') && t.ends_with(&close_param))
                || (t.starts_with("[Calling tool ") && !t.ends_with("']")))
        })
        .collect();
    (
        std::borrow::Cow::Owned(kept.join("\n").trim().to_string()),
        true,
    )
}

/// Returns true when a no-tool-call assistant turn looks unfinished rather
/// than like a final summary: an empty response (stream returned nothing), a
/// trailing colon/ellipsis ("Let me fix the test:"), or a last line that is
/// itself an action announcement. The WASIX POC run ended twice this way —
/// "Agent finished" while the mission was half-done.
fn unfinished_announced_turn(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed.ends_with(':')
        || trimmed.ends_with('：')
        || trimmed.ends_with("...")
        || trimmed.ends_with('…')
    {
        return true;
    }
    if let Some(last_line) = trimmed.lines().rev().find(|l| !l.trim().is_empty()) {
        let lower = last_line.trim_start().to_lowercase();
        // "Let me know …" closings are normal final-summary prose, not intent.
        if lower.starts_with("let me know") || lower.starts_with("let us know") {
            return false;
        }
        const ANNOUNCEMENTS: [&str; 8] = [
            "let me ",
            "let's ",
            "i'll ",
            "i will ",
            "i am going to ",
            "i'm going to ",
            "now i ",
            "next, i ",
        ];
        if ANNOUNCEMENTS.iter().any(|p| lower.starts_with(p)) {
            return true;
        }
    }
    false
}

/// Bound the size of directory/search listing tool results before they enter
/// the model's history. A raw `list_dir` of a large package is ~1.5KB of
/// filenames; for a small reasoning model on an artifact task that bloats the
/// context until it loses the thread of the instruction — observed on the
/// LiteLLM crucible, where qwen3.6:35b-a3b started "reviewing the files you
/// shared" instead of writing the component map. Only listing/search tools are
/// bounded, and only for artifact dispatches on thinking models: `read_file`
/// (needed in full to port code) and every other provider/task shape are left
/// untouched.
fn bound_listing_output(
    tool_name: &str,
    result: String,
    requires_artifact: bool,
    supports_thinking: bool,
) -> String {
    if !requires_artifact || !supports_thinking {
        return result;
    }
    if !matches!(tool_name, "list_dir" | "glob" | "grep_search") {
        return result;
    }
    const CAP: usize = 1200;
    if result.len() <= CAP {
        return result;
    }
    let mut cut = CAP;
    while cut > 0 && !result.is_char_boundary(cut) {
        cut -= 1;
    }
    format!(
        "{}\n\u{2026} (listing truncated for brevity \u{2014} the deliverable needs the top-level entries, not every file)",
        &result[..cut]
    )
}

/// What the loop should do when a turn returned no executable tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoToolCallEnding {
    /// Re-prompt the model so it issues the call it clearly meant to make.
    Nudge,
    /// The re-prompt budget is spent and the turn is still not an answer.
    Blocked,
    /// A genuine final summary; the mission is done.
    FinalAnswer,
    /// Turns that announced an action without ever making a tool call have
    /// consumed too much of the run's wall clock to keep trying.
    Stalled,
}

/// Pure form of the "no tool call" exit decision, so the honest-termination
/// rule is testable without a live provider.
///
/// A turn is unfinished when it carried tool-call markup no parser could
/// execute (`markup_residue`), streamed nothing into the content channel, or
/// read as an announcement rather than a summary. Unfinished turns are
/// re-prompted up to `MAX_AUTO_CONTINUATIONS` times and then reported as
/// blocked — the failure mode this exists to close is a broken-markup turn
/// being relabelled "final answer, no tool calls" and written to the handover
/// as completed.
///
/// `wasted_secs` is the time already spent on such turns and `budget_secs` the
/// run's wall-clock budget (0 when it has none). The count bound alone is not
/// enough on a local model: a measured convert spent its entire 1,497s budget
/// on four planning turns that emitted no tool call and wrote no file, because
/// each retry cost five minutes. Once a share of the budget has gone into
/// nothing, stopping is worth more than another try — the caller can spend what
/// is left on the next model in the chain instead.
fn decide_no_tool_call_ending(
    markup_residue: bool,
    content_was_empty: bool,
    content: &str,
    auto_continuations: u32,
    wasted_secs: u64,
    budget_secs: u64,
) -> NoToolCallEnding {
    let unfinished = markup_residue || content_was_empty || unfinished_announced_turn(content);
    if !unfinished {
        return NoToolCallEnding::FinalAnswer;
    }
    if wasted_time_ends_the_run(wasted_secs, budget_secs) {
        return NoToolCallEnding::Stalled;
    }
    if auto_continuations >= MAX_AUTO_CONTINUATIONS {
        return NoToolCallEnding::Blocked;
    }
    NoToolCallEnding::Nudge
}

/// A run with no wall-clock bound (an operator at the keyboard) is never
/// stopped for spending time; a bounded one gives up on no-progress turns once
/// they account for 40% of it.
fn wasted_time_ends_the_run(wasted_secs: u64, budget_secs: u64) -> bool {
    budget_secs > 0 && wasted_secs * 5 >= budget_secs * 2
}

/// Validate a model's tool call against the tool's declared `inputSchema`
/// BEFORE it runs. Models emit syntactically-valid calls whose arguments
/// violate the contract: the observed convert run sent `write_file` with a
/// blank `content` (a truncated generation) and a call missing a required
/// parameter. Executing either writes a broken artifact that only the compiler
/// catches — one or more turns later, after the loop has already moved on.
/// This short-circuits the call with a precise, per-parameter message so the
/// model corrects on the very next turn instead.
///
/// Returns `None` when the call is acceptable, or `Some(reason)` to reject it.
/// The reason keeps the `"Error executing tool:"` prefix so the existing
/// failure classification (any_error penalisation, event outcomes, in-band
/// refusal detection) treats it like any other tool failure. Deliberately
/// conservative: it only flags a declared `required` parameter that is
/// absent/null (or, for a string-typed one, blank), a present parameter whose
/// JSON type contradicts the declared `type`, and a present parameter whose
/// value falls outside a declared `enum`. Anything the schema does not pin
/// down is left for the tool itself to judge.
fn validate_tool_call(name: &str, args: &Value) -> Option<String> {
    let schema = registry::tool_definitions::get_tools()
        .into_iter()
        .find(|t| t.name == name)
        .map(|t| t.input_schema)?;
    let props = schema.get("properties").and_then(|p| p.as_object());
    let declared_type = |key: &str| -> Option<&str> {
        props?
            .get(key)
            .and_then(|s| s.get("type"))
            .and_then(|t| t.as_str())
    };
    let mut problems: Vec<String> = Vec::new();

    // 1. Every `required` parameter must be present, non-null and (for a
    //    string) non-blank — this is what catches the empty-`content` write.
    if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
        for key in required.iter().filter_map(|k| k.as_str()) {
            match args.get(key) {
                None | Some(Value::Null) => {
                    let ty = declared_type(key).unwrap_or("a value");
                    problems.push(format!(
                        "required parameter '{key}' is missing — supply {ty} '{key}'"
                    ));
                }
                Some(Value::String(s)) if s.trim().is_empty() => {
                    problems.push(format!(
                        "required parameter '{key}' is blank — write the real content into \
                         this call instead of leaving it empty"
                    ));
                }
                _ => {}
            }
        }
    }

    // 2. A present, non-null parameter whose JSON type contradicts the schema.
    let mut type_failed: Vec<&str> = Vec::new();
    if let (Some(obj), Some(map)) = (args.as_object(), props) {
        for (key, val) in obj {
            if val.is_null() {
                continue;
            }
            let want = match map
                .get(key)
                .and_then(|s| s.get("type"))
                .and_then(|t| t.as_str())
            {
                Some(w) => w,
                None => continue,
            };
            let ok = match want {
                "string" => val.is_string(),
                "number" => val.is_number(),
                "integer" => val.is_i64() || val.is_u64(),
                "boolean" => val.is_boolean(),
                "object" => val.is_object(),
                "array" => val.is_array(),
                // Unknown/compound types (oneOf, enum of mixed, …): leave to the tool.
                _ => true,
            };
            if !ok {
                let got = match val {
                    Value::String(_) => "a string",
                    Value::Number(_) => "a number",
                    Value::Bool(_) => "a boolean",
                    Value::Object(_) => "an object",
                    Value::Array(_) => "an array",
                    Value::Null => "null",
                };
                problems.push(format!(
                    "parameter '{key}' must be {want}, but you sent {got}"
                ));
                type_failed.push(key.as_str());
            }
        }
    }

    // 3. An `enum`-constrained parameter must carry one of its declared values —
    //    including each element of an array whose `items` declares a choice set
    //    (`modifiers: ["ctrl","shift"]`), where a near-miss silently presses the
    //    wrong key combination instead of failing loudly.
    //    Sixty-seven registered schemas pin a choice set (`sortDirection`,
    //    `mode`, `capability`, `action`) and a small model reliably drifts to a
    //    plausible synonym — "ascending" where the schema says ["asc","desc"].
    //    The type check above waves it through, the tool then rejects it deep in
    //    its own body, and the returned error names no alternatives, so the next
    //    turn guesses again. Strings compare case-insensitively: that can only
    //    accept a call, never reject one a tool would have taken.
    if let (Some(obj), Some(map)) = (args.as_object(), props) {
        for (key, val) in obj {
            if val.is_null() || type_failed.contains(&key.as_str()) {
                continue;
            }
            let Some(prop) = map.get(key) else { continue };
            if let Some(allowed) = prop.get("enum").and_then(|e| e.as_array()) {
                // An array here is already reported as a type violation above.
                if !val.is_array() && !enum_fits(allowed, val) {
                    problems.push(format!(
                        "parameter '{key}' must be one of {} — you sent {}",
                        enum_choices(allowed),
                        show_value(val)
                    ));
                }
                continue;
            }
            let Some(elems) = val.as_array() else {
                continue;
            };
            let Some(items) = prop.get("items") else {
                continue;
            };
            if let Some(allowed) = items.get("enum").and_then(|e| e.as_array()) {
                let bad: Vec<String> = elems
                    .iter()
                    .filter(|e| !enum_fits(allowed, e))
                    .map(show_value)
                    .collect();
                if !bad.is_empty() {
                    problems.push(format!(
                        "every element of '{key}' must be one of {} — you sent {}",
                        enum_choices(allowed),
                        bad.join(", ")
                    ));
                }
            } else if let Some(step_props) = items.get("properties").and_then(|p| p.as_object()) {
                for (i, elem) in elems.iter().enumerate() {
                    let Some(step) = elem.as_object() else {
                        continue;
                    };
                    let inner = enum_problems_in_object(step_props, step);
                    if !inner.is_empty() {
                        problems.push(format!("'{key}' step {} has {}", i + 1, inner.join("; ")));
                    }
                }
            }
        }
    }

    if problems.is_empty() {
        return None;
    }
    Some(format!(
        "Error executing tool: your call to '{name}' violates its parameter schema:\n{}\n\
         Re-issue the call with every required parameter filled in, correctly typed and \
         drawn from the allowed values listed above — do not describe the fix in prose.",
        problems
            .iter()
            .map(|p| format!("  - {p}"))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

/// Does this argument value satisfy a declared choice set? Strings match
/// case-insensitively so the gate can never reject a call the tool itself would
/// have taken; every other JSON type compares exactly.
fn enum_fits(allowed: &[Value], val: &Value) -> bool {
    allowed.iter().any(|a| match (a.as_str(), val.as_str()) {
        (Some(want), Some(got)) => want.eq_ignore_ascii_case(got),
        _ => a == val,
    })
}

/// The allowed values rendered for the refusal, so the next call can pick one
/// instead of guessing another synonym.
fn enum_choices(allowed: &[Value]) -> String {
    allowed
        .iter()
        .map(|a| match a.as_str() {
            Some(s) => format!("\"{s}\""),
            None => a.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

/// How to show an offending argument back to the model: quoted for a string,
/// literal for anything else.
fn show_value(val: &Value) -> String {
    match val {
        Value::String(s) => format!("\"{s}\""),
        other => other.to_string(),
    }
}

/// Enum violations inside one object checked against a declared `properties`
/// map. Exists because the desktop-automation choice sets live one level below
/// the argument: `wa_input_sequence` declares `op` and `modifiers` per step, so
/// "rightclick" or "control" would otherwise reach the injector and silently
/// press the wrong keys.
fn enum_problems_in_object(
    props: &serde_json::Map<String, Value>,
    args: &serde_json::Map<String, Value>,
) -> Vec<String> {
    let mut out = Vec::new();
    for (key, val) in args {
        let Some(schema) = props.get(key) else {
            continue;
        };
        if let Some(allowed) = schema.get("enum").and_then(|e| e.as_array()) {
            if !val.is_array() && !enum_fits(allowed, val) {
                out.push(format!(
                    "'{key}' must be one of {} — you sent {}",
                    enum_choices(allowed),
                    show_value(val)
                ));
            }
            continue;
        }
        let Some(list) = val.as_array() else { continue };
        let Some(allowed) = schema
            .get("items")
            .and_then(|i| i.get("enum"))
            .and_then(|e| e.as_array())
        else {
            continue;
        };
        let bad: Vec<String> = list
            .iter()
            .filter(|e| !enum_fits(allowed, e))
            .map(show_value)
            .collect();
        if !bad.is_empty() {
            out.push(format!(
                "every '{key}' element must be one of {} — you sent {}",
                enum_choices(allowed),
                bad.join(", ")
            ));
        }
    }
    out
}

/// Extract a complete fenced code block from model prose.
///
/// A local MoE asked to repair a file often does the work and then hands it back
/// as prose: the probe replay of a real failed convert turn streamed 10,558
/// characters of corrected Rust with `done_reason=stop`, fenced as ```rust, and
/// emitted **no** native `tool_calls` at all. The structured-call parsers above
/// (DSML, Claude-inline, bare-JSON) none match a plain code fence, so the loop
/// saw "no tool call", recompiled the unchanged file, and burned a repair
/// attempt. Returning the largest balanced block lets that turn still land.
///
/// Deliberately conservative, because a code fence is also how a model illustrates
/// a snippet: an unbalanced block (a truncated stream, or an excerpt with an elided
/// body) and anything under `min_len` come back as `None`.
///
/// Both scans compare `bytes` slices, never `&content[i..i + 3]`: the index steps
/// one BYTE at a time, so it lands inside multi-byte characters constantly, and a
/// model's own prose is full of them. An em dash used to end the dispatch with
/// "end byte index N is not a char boundary". Matching an ASCII fence on bytes is
/// also what keeps every later `content[i..]` legal — a match means the index sits
/// on a backtick, which is always a boundary.
fn extract_fenced_code_block(content: &str, min_len: usize) -> Option<String> {
    let bytes = content.as_bytes();
    const FENCE: &[u8] = b"```";
    let mut best: Option<String> = None;
    let mut i = 0usize;
    while i + FENCE.len() <= bytes.len() {
        if &bytes[i..i + FENCE.len()] != FENCE {
            i += 1;
            continue;
        }
        // Opening fence runs to end of its line; the language tag lives there.
        let open_end = content[i..]
            .find('\n')
            .map(|o| i + o + 1)
            .unwrap_or(content.len());
        // Closing fence: a ``` at column 0 of some line after the opener.
        let mut close = None;
        let mut j = open_end;
        while j + FENCE.len() <= bytes.len() {
            if &bytes[j..j + FENCE.len()] == FENCE
                && (j == 0 || bytes[j - 1] == b'\n')
                && !content[..j].ends_with("```")
            {
                close = Some(j);
                break;
            }
            j += 1;
        }
        let Some(close_at) = close else {
            // Unterminated fence: the model ran out of stream mid-block. Nothing
            // safe to write — skip this opener entirely rather than emit a
            // truncated file.
            break;
        };
        let body = &content[open_end..close_at];
        // "Looks like a whole file" is what the gate is for, and a truncated
        // stream is what it must catch. Counting `{` naively would reject a
        // legitimate port whose strings carry braces — a template renderer, a
        // lexer port holding `"{"` — which is the model's one correct answer
        // thrown away, so the balance half asks the Rust-aware counter.
        let balanced = body.contains('{') && crate::registry::system_tools::brace_delta(body) == 0;
        let longer = match &best {
            None => true,
            Some(b) => body.len() > b.len(),
        };
        if body.trim().len() >= min_len && balanced && longer {
            best = Some(body.to_string());
        }
        i = close_at + FENCE.len();
    }
    best
}

/// Render an unknown-tool failure for the model. Keeps the "Error executing
/// tool" prefix so failure classification (any_error penalisation, event
/// outcomes, in-band refusal detection) is unchanged; appends the closest
/// registered tool names ranked by shared `_`-tokens and substring overlap.
fn unknown_tool_message(name: &str, all_names: &[String]) -> String {
    let lower = name.to_lowercase();
    let tokens: Vec<&str> = lower
        .split(['_', ' ', '-'])
        .filter(|t| !t.is_empty())
        .collect();
    let mut scored: Vec<(u32, &String)> = all_names
        .iter()
        .map(|n| {
            let ln = n.to_lowercase();
            let mut score = 0u32;
            if ln == lower {
                score += 10;
            }
            for t in ln.split(['_', ' ', '-']).filter(|t| !t.is_empty()) {
                if tokens.contains(&t) {
                    score += 3;
                }
            }
            if ln.contains(&lower) || lower.contains(&ln) {
                score += 2;
            }
            (score, n)
        })
        .filter(|(s, _)| *s > 0)
        .collect();
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            // Live runs showed the alphabetical tiebreak burying the best
            // substitutes: for `edit_file`, generic short names like
            // write_file lost out to create_skill_file and gui_* tools.
            // Prefer shorter (more general) names, then alphabetical.
            .then_with(|| a.1.len().cmp(&b.1.len()))
            .then_with(|| a.1.cmp(b.1))
    });
    let mut suggestion = scored
        .iter()
        .take(6)
        .map(|(_, n)| n.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    if suggestion.is_empty() {
        suggestion = "(no similar tools; re-check the registered tool list)".to_string();
    }
    format!(
        "Error executing tool: ToolNotFound(\"{}\"). No such tool is registered. \
         Closest available tools: [{}]. Use one of these registered tools instead.",
        name, suggestion
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        arm_compile_repair, artifact_delivery_directive, call_identity, compile_repair_directive,
        correct_missing_crate_help, counts_as_artifact, decide_no_tool_call_ending,
        declared_dependencies, declared_name, dependency_repair_rule, enclosing_fn_block,
        error_code_recipes, existing_file_named_by_task, extract_fenced_code_block,
        force_write_directive, gate_failure_label, grade_write, gutting_rewrite_refusal,
        handover_for_outcome, hides_generation_tool, is_refusal, is_rustc_diagnostic_report,
        is_test_failure_report, item_head_keyword, items_a_rewrite_would_delete,
        missing_crate_help, note_refusal_repeat, off_target_refusal, parse_function_fragments,
        read_of_named_target, remember_run_write, repair_api_context, repair_budget,
        repair_error_excerpt, repeated_refusal_note, replay_advice, rust_item_names,
        salvage_complete_calls, should_arm_write_repair, should_close_validate,
        should_grade_tests_now, should_lock_exploration, should_note_unarmed_repair,
        should_nudge_missing_artifact, should_nudge_write_pressure, should_rebuild_write_context,
        should_refund_repair_attempt, should_refuse_gutting_rewrite, should_steer_wrong_path,
        strip_tool_markup_residue, type_names_in_diagnostics, unfinished_announced_turn,
        unknown_tool_message, validate_tool_call, wasted_time_ends_the_run,
        write_context_directive, write_misses_named_target, write_pressure_directive,
        writes_miss_named_target, wrong_path_directive, AgentLoopOutcome, AgentToUiMessage,
        ArtifactSurface, ChatMessage, NoToolCallEnding, RunEnding, TestVerdict,
        ToolCallAccumulator, WriteGrade, FILE_EDITING_TOOLS, MAX_ARTIFACT_NUDGES,
        MAX_AUTO_CONTINUATIONS, MAX_COMPILE_REPAIR_TURNS, MAX_OFF_TARGET_REFUSALS,
        MAX_REFUNDED_REPAIR_ATTEMPTS, MAX_SURFACE_REFUSALS, MAX_TEST_REPAIR_TURNS,
        MAX_TRACKED_RUN_WRITES, MAX_UNARMED_REPAIR_NOTES, MAX_WRITE_PRESSURE_NUDGES,
        MEDIA_GENERATION_TOOLS, ONE_ERROR_AT_A_TIME_NOTE, WRITE_PRESSURE_STREAK,
    };

    // The file-free forms of the repeat detector. Most of these tests feed
    // synthetic reports whose `-->` path does not exist, so only the line-text
    // half of a signature can ever be produced — which is exactly what
    // `error_signatures` does with no workspace. The item-keyed half is covered
    // by tests that pass a real temp dir to `super::`.
    fn error_signatures(errors: &str) -> Vec<String> {
        super::error_signatures(errors, None)
    }

    fn prior_encounters(errors: &str, history: &[Vec<String>]) -> usize {
        super::prior_encounters(errors, history, None)
    }

    fn recurring_error_focus(errors: &str, history: &[Vec<String>]) -> Option<String> {
        super::recurring_error_focus(errors, history, None)
    }

    fn recurring_error_note(errors: &str, history: &[Vec<String>]) -> String {
        super::recurring_error_note(errors, history, None)
    }

    fn tag(name: &str) -> String {
        format!("<{}>", name)
    }

    fn tag_open(name: &str) -> String {
        format!("<{}", name)
    }

    fn lt() -> String {
        String::from('\u{3c}')
    }

    #[test]
    fn write_pressure_nudge_fires_past_streak_within_budget() {
        assert!(!should_nudge_write_pressure(
            WRITE_PRESSURE_STREAK - 1,
            0,
            true
        ));
        assert!(should_nudge_write_pressure(WRITE_PRESSURE_STREAK, 0, true));
        assert!(should_nudge_write_pressure(
            WRITE_PRESSURE_STREAK + 3,
            MAX_WRITE_PRESSURE_NUDGES - 1,
            true
        ));
    }

    #[test]
    fn exploration_lock_follows_a_read_streak_as_well_as_a_pause() {
        // Nothing has been said to the model yet: it may explore.
        assert!(!should_lock_exploration(
            true, false, false, false, false, false
        ));
        // A model that never pauses cannot be reached by a pause-only lock, so
        // the read streak has to bind on its own. This is the repair17a shape.
        assert!(should_lock_exploration(
            true, false, false, true, false, false
        ));
        // The steers that were already enough stay enough.
        assert!(should_lock_exploration(
            true, false, true, false, false, false
        ));
        assert!(should_lock_exploration(
            true, false, false, false, true, false
        ));
        // A non-artifact run has no deliverable to protect: auditing is its job.
        assert!(!should_lock_exploration(
            false, false, true, true, false, false
        ));
    }

    #[test]
    fn exploration_lock_releases_on_the_artifact_a_repair_ignores_it() {
        // Once the file is written and BUILDS the pressure is off; the model may
        // read again to find out what else the task needs.
        assert!(!should_lock_exploration(
            true, true, true, false, false, false
        ));
        assert!(!should_lock_exploration(
            true, true, false, true, false, false
        ));
        // A compile-repair turn is the exception on both counts: the artifact
        // exists but is broken, and the run may not even be an artifact run.
        assert!(should_lock_exploration(
            true, true, false, false, true, false
        ));
        assert!(should_lock_exploration(
            false, true, false, false, true, false
        ));
    }

    #[test]
    fn a_rejected_write_keeps_the_lock_the_artifact_was_supposed_to_release() {
        // repair20: turn 9 wrote a file rustc rejected, the repair budget had
        // already read 3/3 so no repair turn armed, and turns 10-16 explored the
        // file the compiler had already refused. A known-red artifact is not a
        // finished artifact, so it cannot buy back exploration — for either steer.
        assert!(should_lock_exploration(
            true, true, false, false, false, true
        ));
        assert!(should_lock_exploration(true, true, true, true, false, true));
        // Only an artifact run is locked this way: an interactive agent that broke
        // a build still has to be able to go look at why, on the user's question.
        assert!(!should_lock_exploration(
            false, true, false, false, false, true
        ));
    }

    #[test]
    fn a_scratch_file_is_not_the_rust_artifact_the_task_owed() {
        // A port task's deliverable is Rust. A script beside it is work, not the
        // file the caller asked for, so it must not unlock exploration nor stand
        // as evidence that the artifact exists.
        assert!(!counts_as_artifact(Some("run_check.sh"), true));
        assert!(!counts_as_artifact(Some("notes\\todo.md"), true));
        // The deliverable itself, in either separator dialect.
        assert!(counts_as_artifact(
            Some("rust_port/src/budget_manager.rs"),
            true
        ));
        assert!(counts_as_artifact(
            Some("rust_port\\src\\budget_manager.rs"),
            true
        ));
        // A task that never named a language: the loop has no way to call a file
        // junk, so any write stands as the artifact.
        assert!(counts_as_artifact(Some("README.md"), false));
        // And a write with no reported path is not proof of nothing.
        assert!(counts_as_artifact(None, true));
    }

    #[test]
    fn a_write_at_another_path_is_not_the_file_the_task_named() {
        // convert22 named 'rust_port/src/budget_manager.rs' and wrote
        // 'litellm_ws/rust_port/src/budget_manager.rs'. Both are legal
        // workspace-relative paths; only one of them is the deliverable.
        let target = "rust_port/src/budget_manager.rs";
        assert!(write_misses_named_target(
            Some("litellm_ws/rust_port/src/budget_manager.rs"),
            Some(target)
        ));
        // The same path in the other separator dialect is the same file.
        assert!(!write_misses_named_target(
            Some("rust_port\\src\\budget_manager.rs"),
            Some(target)
        ));
        // No rule without a named target, and no rule about a write whose path
        // the changelog could not report.
        assert!(!write_misses_named_target(Some("anything.rs"), None));
        assert!(!write_misses_named_target(None, Some(target)));
    }

    #[test]
    fn a_green_crate_does_not_validate_a_deliverable_that_was_never_written() {
        let target = "rust_port/src/budget_manager.rs";
        let nested = vec!["litellm_ws/rust_port/src/budget_manager.rs".to_string()];
        assert!(writes_miss_named_target(Some(target), &nested));
        // One write at the named path is enough: a port that also drops a helper
        // module beside the deliverable still delivered.
        let both = vec![nested[0].clone(), target.to_string()];
        assert!(!writes_miss_named_target(Some(target), &both));
        // Nothing written at all belongs to the no-files-written nudge.
        assert!(!writes_miss_named_target(Some(target), &[]));
        // A task that names no file has no path to hold the run to.
        assert!(!writes_miss_named_target(None, &nested));
        // The refusal quotes both paths, because the orchestrator reading it has
        // to know which file to move, not just that something went wrong.
        let reason = off_target_refusal(Some(target), &nested)
            .expect("an off-target run has a reason to report");
        assert!(
            reason.contains("litellm_ws/rust_port/src/budget_manager.rs"),
            "{reason}"
        );
        assert!(reason.contains(&format!("'{target}'")), "{reason}");
        assert!(off_target_refusal(Some(target), &both).is_none());
    }

    #[test]
    fn the_off_target_directive_gives_the_model_the_path_to_use() {
        let target = "rust_port/src/budget_manager.rs";
        let directive = wrong_path_directive(
            target,
            &["litellm_ws/rust_port/src/budget_manager.rs".to_string()],
            &["write_file", "edit_file"],
        );
        // The argument name, not prose: this is the field the model got wrong.
        assert!(directive.contains("relativeFilePath"), "{directive}");
        assert!(
            directive.contains("litellm_ws/rust_port/src/budget_manager.rs"),
            "the model must be shown where its write actually went: {directive}"
        );
        assert!(
            directive.contains(&format!("exactly '{target}'")),
            "{directive}"
        );
        assert!(directive.contains("write_file or edit_file"), "{directive}");
    }

    /// Read back the handover a run wrote. `write_handover_nda` seals it with
    /// the workspace key, which the same workspace can open — and an assert on
    /// the sealed bytes would measure nothing about what the record says.
    fn open_handover(root: &std::path::Path) -> String {
        let raw = std::fs::read(root.join(".velocity").join("handover.nda")).unwrap();
        String::from_utf8(crate::agent::crypto::open(root, b"handover", &raw)).unwrap()
    }

    #[test]
    fn the_same_file_written_both_ways_is_listed_once() {
        // A model that types `a\b.rs` after having written `a/b.rs` wrote one
        // file. The list quotes itself back to the model as evidence of where
        // its writes went, so a duplicate reads as two mistakes.
        let mut writes = Vec::new();
        remember_run_write(&mut writes, "rust_port/src/budget_manager.rs");
        remember_run_write(&mut writes, "rust_port\\src\\budget_manager.rs");
        assert_eq!(writes.len(), 1, "{writes:?}");
        remember_run_write(&mut writes, "rust_port/src/scheduler.rs");
        assert_eq!(writes.len(), 2, "{writes:?}");
        // Bounded however many files a run rewrites: the list is also the gate's
        // test scope.
        for i in 0..(MAX_TRACKED_RUN_WRITES * 2) {
            remember_run_write(&mut writes, &format!("m{i}.rs"));
        }
        assert_eq!(writes.len(), MAX_TRACKED_RUN_WRITES, "{writes:?}");
    }

    #[test]
    fn handover_fields_cannot_disagree_with_the_outcome() {
        // Every terminal exit used to choose its own state string and flag
        // beside the outcome it returned, and they drifted: one recorded
        // `idle`/`completed` while returning `Blocked`. Deriving both from the
        // outcome is what this pins.
        let cases = [
            (AgentLoopOutcome::Completed, "idle", false),
            (
                AgentLoopOutcome::Blocked {
                    reason: "no file written".to_string(),
                },
                "blocked",
                true,
            ),
            (
                AgentLoopOutcome::ProviderUnavailable {
                    reason: "no key".to_string(),
                },
                "blocked",
                true,
            ),
            (
                AgentLoopOutcome::TimedOut {
                    elapsed_secs: 900,
                    budget_secs: 900,
                    turns: 4,
                },
                "blocked",
                true,
            ),
            (AgentLoopOutcome::Cancelled, "cancelled", true),
        ];
        for (outcome, state, interrupted) in cases {
            assert_eq!(
                handover_for_outcome(&outcome),
                (state, interrupted),
                "{outcome:?}"
            );
        }
    }

    #[test]
    fn a_terminal_exit_records_its_own_kind_of_ending() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path().to_path_buf();
        let mut ending = RunEnding {
            workspace_root: &root,
            recorded: false,
        };
        assert!(!ending.recorded, "a run that has not ended is on no record");

        let outcome = ending.stopped(
            4,
            "wall-clock budget exhausted",
            AgentLoopOutcome::TimedOut {
                elapsed_secs: 900,
                budget_secs: 900,
                turns: 4,
            },
        );
        assert!(outcome.is_timed_out());
        let handover = open_handover(&root);
        assert!(handover.contains("field\tstate\tblocked"), "{handover}");
        assert!(handover.contains("field\tinterrupted\ttrue"), "{handover}");
        assert!(handover.contains("field\tturn\t4"), "{handover}");

        let outcome = ending.completed(9, "compiler validated; 68 passed");
        assert!(outcome.is_completed());
        let handover = open_handover(&root);
        assert!(handover.contains("field\tstate\tidle"), "{handover}");
        assert!(
            handover.contains("field\tinterrupted\tfalse"),
            "a finished run is not a stop: {handover}"
        );
        assert!(handover.contains("compiler validated"), "{handover}");
        assert!(
            ending.recorded,
            "the closing record has to know an ending is already on file"
        );
    }

    #[test]
    fn the_wrong_path_steering_has_its_own_cap() {
        assert!(should_steer_wrong_path(0));
        assert!(should_steer_wrong_path(MAX_OFF_TARGET_REFUSALS - 1));
        assert!(!should_steer_wrong_path(MAX_OFF_TARGET_REFUSALS));
        // The statement that matters for a run: a model that has already spent
        // the no-files-written budget is still steerable on *where* it wrote.
        assert!(!should_nudge_missing_artifact(
            true,
            false,
            MAX_ARTIFACT_NUDGES,
            true
        ));
        assert!(should_steer_wrong_path(0));
    }

    #[test]
    fn a_rejection_shown_without_a_repair_turn_is_shown_once_per_distinct_report() {
        assert!(should_note_unarmed_repair(true, 0));
        // Byte-identical rustc output is output the model has already been handed.
        assert!(!should_note_unarmed_repair(false, 0));
        // And a fresh report every turn still cannot fill the context with
        // directives: the cap is the whole point of the counter.
        assert!(should_note_unarmed_repair(
            true,
            MAX_UNARMED_REPAIR_NOTES - 1
        ));
        assert!(!should_note_unarmed_repair(true, MAX_UNARMED_REPAIR_NOTES));
    }

    #[test]
    fn a_repair_turn_that_wrote_nothing_gives_its_attempt_back() {
        // The lr7b_v11 shape: the directive went out and the turn's only call was
        // an edit_file refused because its `oldString` quoted text the model's own
        // earlier write had already replaced. No file moved, so no repair was
        // graded — but the attempt had been charged when the directive was armed.
        assert!(should_refund_repair_attempt(true, false, true, 0));
        // A turn that moved the file spent its attempt, whatever else it hit.
        assert!(!should_refund_repair_attempt(true, true, true, 0));
        // A turn that refused nothing has no answer of the loop's own to blame:
        // the refund is for a repair the tooling would not let land.
        assert!(!should_refund_repair_attempt(true, false, false, 0));
        // A turn that was never armed was never charged.
        assert!(!should_refund_repair_attempt(false, false, true, 0));
        // Bounded, so a model that refuses every directive cannot be re-armed for
        // ever — the turn budget is the only thing that would stop it.
        assert!(should_refund_repair_attempt(
            true,
            false,
            true,
            MAX_REFUNDED_REPAIR_ATTEMPTS - 1
        ));
        assert!(!should_refund_repair_attempt(
            true,
            false,
            true,
            MAX_REFUNDED_REPAIR_ATTEMPTS
        ));
    }

    #[test]
    fn an_identical_refused_call_is_named_as_a_replay_from_the_second_time() {
        // The lr7b_v12 shape: one `edit_file` payload refused on four turns
        // running, each with the same two-site explanation, because its
        // `oldString` was a bare identifier that matched two places. Nothing was
        // compiled on any of those turns, so no error-signature check could see
        // the circle — the payload itself is the only thing that repeats.
        let call = call_identity(
            "edit_file",
            &json!({
                "relativeFilePath": "rust_port/src/budget_manager.rs",
                "edits": [{ "oldString": "result_record", "newString": "&result_record" }]
            }),
        );
        let mut counts = std::collections::HashMap::new();

        // The first refusal is the tool's own answer and nothing else: nothing
        // has been repeated yet, and saying it had been would be a lie.
        assert_eq!(note_refusal_repeat(&mut counts, &call, true), None);
        assert_eq!(note_refusal_repeat(&mut counts, &call, true), Some(2));
        assert_eq!(note_refusal_repeat(&mut counts, &call, true), Some(3));

        // Counted per payload, not per run: a different call in the same batch is
        // still being answered for the first time.
        let other = call_identity("edit_file", &json!({"relativeFilePath": "other.rs"}));
        assert_eq!(note_refusal_repeat(&mut counts, &other, true), None);
    }

    #[test]
    fn a_refused_call_that_later_lands_stops_being_a_replay() {
        let call = call_identity("edit_file", &json!({"relativeFilePath": "a.rs"}));
        let mut counts = std::collections::HashMap::new();
        assert_eq!(note_refusal_repeat(&mut counts, &call, true), None);
        assert_eq!(note_refusal_repeat(&mut counts, &call, true), Some(2));

        // The same bytes can land once the file underneath them changes — an
        // `oldString` that matched two places matches one after the other site is
        // edited. A count kept past that point would accuse the next refusal of
        // replaying a call that has since succeeded.
        assert_eq!(note_refusal_repeat(&mut counts, &call, false), None);
        assert_eq!(note_refusal_repeat(&mut counts, &call, true), None);
        assert_eq!(note_refusal_repeat(&mut counts, &call, true), Some(2));
    }

    #[test]
    fn a_call_is_the_same_call_whatever_order_its_keys_come_in() {
        // lr7b_v12's transcript spells one payload both ways across turns.
        // serde_json's `Value` object is a `BTreeMap` unless the crate is built
        // with `preserve_order`; if that ever changes, `Display` follows insertion
        // order and these two stop matching — silently switching the replay
        // detector off, which is what this assertion is here to catch.
        let a = call_identity(
            "edit_file",
            &json!({"relativeFilePath": "x.rs", "edits": [{ "oldString": "a" }]}),
        );
        let b = call_identity(
            "edit_file",
            &json!({"edits": [{ "oldString": "a" }], "relativeFilePath": "x.rs"}),
        );
        assert_eq!(a, b);

        // And a different payload, or a different tool, is still a different call.
        assert_ne!(
            a,
            call_identity(
                "edit_file",
                &json!({"relativeFilePath": "x.rs", "edits": [{ "oldString": "b" }]})
            )
        );
        assert_ne!(
            a,
            call_identity(
                "write_file",
                &json!({"relativeFilePath": "x.rs", "edits": [{ "oldString": "a" }]})
            )
        );
    }

    #[test]
    fn only_the_loops_own_refusal_shapes_count_as_a_refusal() {
        // The two shapes the results loop charges to `any_error`. A replay note
        // that counted anything else would fire on a turn the loop considers
        // clean, and one that missed these would never fire at all.
        assert!(is_refusal(
            "Error executing tool: edit 1: its `oldString` matches 2 places in a.rs."
        ));
        assert!(is_refusal("BLOCKED: write outside the workspace"));
        // A human turning a call away is not the tool refusing it.
        assert!(!is_refusal("Tool call rejected by the user"));
        assert!(!is_refusal("Applied 1 edit to a.rs"));
    }

    #[test]
    fn the_replay_note_is_a_suffix_that_carries_the_count() {
        let note = repeated_refusal_note(
            4,
            "Error executing tool: edit 1: its `oldString` is not in a.rs.",
        );

        // It has to carry the number: that count is the one fact no tool result
        // can know, and it is what makes this answer differ from the last three.
        assert!(note.contains("4 times"), "{note}");
        // It is appended to the tool's own refusal, never substituted for it, so
        // it must start by separating itself from whatever precedes it.
        assert!(note.starts_with("\n\n"), "{note:?}");
    }

    /// Each refusal the loop actually emits, keyed by the class the note has to
    /// recognise. Wordings are copied from `system_tools.rs` so a change there
    /// that breaks the classifier breaks this test rather than a probe run.
    #[test]
    fn the_replay_advice_agrees_with_the_refusal_it_follows() {
        // The `lr7b_v17` case: the change landed two turns ago, so the only
        // useful instruction is to go and look at the line the compiler names.
        // The old fixed note told the model to widen its `oldString` here, and
        // the model complied four times with a payload that could never apply.
        let applied = replay_advice(
            "Error executing tool: no edits were applied to a.rs; the file is unchanged:\n\
             edit 1: this change is already on disk — its `newString` is in a.rs verbatim at \
             line 285, and its `oldString` is not in the file at all.",
        );
        assert!(applied.contains("already"), "{applied}");
        assert!(!applied.contains("Widen"), "{applied}");

        // The `lr7b_v19` case: the refusal quoted the file's own line, so that
        // line is the lever. Telling the model to go and read the region again
        // would send it after the one thing it has already been handed.
        let near = replay_advice(
            "Error executing tool: no edits were applied to a.rs; the file is unchanged:\n\
             edit 1: its `oldString` is not in a.rs, but it is close to text that is: read from \
             line 287, 10 of its 11 non-blank lines are in the file in that order and the next \
             one is not. Its line 11 is where they part — you wrote `});`, the file says `})`.",
        );
        assert!(near.contains("the file says"), "{near}");
        assert!(
            !near.contains("read_file"),
            "a read returns the same line: {near}"
        );
        assert!(!near.contains("Widen"), "{near}");

        // The brace-drift refusal says "not in place of it" in ordinary prose.
        // It is not an ambiguous-site refusal, so it must not draw the
        // widen-it lever, which cannot fix an unclosed block.
        let drift = replay_advice(
            "Error executing tool: no edits were applied to a.rs; the file is unchanged: this \
             batch of 1 edit(s) would leave the file with 1 more block left open than it has \
             now. Put the new text around it, not in place of it.",
        );
        assert!(!drift.contains("Widen"), "{drift}");
        assert!(drift.contains("substance"), "{drift}");

        // v12's case: two sites match, so the lever really is the `oldString`.
        let ambiguous = replay_advice(
            "Error executing tool: edit 1: its `oldString` matches 2 places in a.rs — \
             widen it until only the site you mean still matches.",
        );
        assert!(ambiguous.contains("Widen"), "{ambiguous}");
        assert!(!ambiguous.contains("already"), "{ambiguous}");

        // v15's case: the edit changes no characters, so no re-aiming helps.
        let noop = replay_advice(
            "Error executing tool: edit 1: its `newString` is byte-identical to its `oldString`, \
             so this edit would change no characters.",
        );
        assert!(noop.contains("changes no characters"), "{noop}");

        // A miss with nothing else diagnosed: the text has to come off the disk.
        let missing = replay_advice(
            "Error executing tool: edit 1: its `oldString` is not in a.rs — copy the text \
             verbatim from the file.",
        );
        assert!(missing.contains("read_file"), "{missing}");

        // Anything unrecognised still gets a lever, just not a wrong one.
        let other = replay_advice("BLOCKED: write outside the workspace");
        assert!(other.contains("substance"), "{other}");
    }

    #[test]
    fn an_already_applied_refusal_is_never_answered_with_widen_the_oldstring() {
        // The regression this whole classifier exists to prevent, stated as the
        // model sees it: the note is the last text in the message, so if it
        // contradicts the refusal the model follows the note.
        let refusal = "Error executing tool: no edits were applied to a.rs; the file is \
                       unchanged:\nedit 1: this change is already on disk — its `newString` is \
                       in a.rs verbatim at line 285, and its `oldString` is not in the file at \
                       all."
            .to_string();
        let outgoing = refusal.clone() + &repeated_refusal_note(2, &refusal);
        assert!(outgoing.contains("already on disk"), "{outgoing}");
        assert!(outgoing.contains("2 times"), "{outgoing}");
        assert!(
            !outgoing.contains("long enough to match the one place"),
            "the note overruled the refusal: {outgoing}"
        );
    }

    #[test]
    fn a_clean_build_over_a_preexisting_file_is_graded_by_its_tests() {
        // The repair19 shape: the file arrived complete but broken, and the build
        // just went clean under this run's own repair.
        assert!(should_grade_tests_now(true, false, true));
        // A port still being authored is not test-graded before the test gate has
        // had a say — a half-written module would be judged as a finished one.
        assert!(!should_grade_tests_now(true, false, false));
        // Once the gate has rejected the artifact, every later clean build is
        // graded on the same terms, whatever kind of task this is.
        assert!(should_grade_tests_now(true, true, false));
        // A broken build outranks all of it: cargo test certifies nothing.
        assert!(!should_grade_tests_now(false, true, true));
    }

    #[test]
    fn only_a_real_rustc_report_seeds_the_first_turn() {
        assert!(is_rustc_diagnostic_report(
            "error[E0308]: mismatched types\n  --> src/budget_manager.rs:287:9\n   |\n"
        ));
        assert!(is_rustc_diagnostic_report(
            "error: expected item, found `}`"
        ));
        // A toolchain that will not start, and a workspace with no crate in it,
        // say nothing about the agent's code — and seeding either one would arm
        // the write lock on a file the model must not touch.
        assert!(!is_rustc_diagnostic_report(
            "Failed to execute cargo check: Os { code: 2, kind: NotFound }"
        ));
        assert!(!is_rustc_diagnostic_report(
            "No Cargo.toml found in this workspace root"
        ));
    }

    #[test]
    fn a_read_streak_locks_tools_without_erasing_the_reads() {
        // The lock and the lean rebuild are separate steers: a streak of
        // productive reads gets the tools withheld but keeps its transcript,
        // because the file text it read is what an edit_file's oldString needs.
        assert!(!should_rebuild_write_context(true, false, false));
        // A pause, whose history is full of no-action narration, and a compile
        // repair, whose directive carries fresh rustc diagnostics, both rebuild.
        assert!(should_rebuild_write_context(true, true, false));
        assert!(should_rebuild_write_context(true, false, true));
        // An unlocked turn never rebuilds, whatever the steers say.
        assert!(!should_rebuild_write_context(false, true, true));
    }

    #[test]
    fn write_pressure_nudge_respects_budget_and_final_turn() {
        // Nudge budget exhausted.
        assert!(!should_nudge_write_pressure(
            WRITE_PRESSURE_STREAK,
            MAX_WRITE_PRESSURE_NUDGES,
            true
        ));
        // Last turn: shouting at the model with no turns left to act is noise.
        assert!(!should_nudge_write_pressure(
            WRITE_PRESSURE_STREAK,
            0,
            false
        ));
    }

    #[test]
    fn write_pressure_directive_covers_both_mission_shapes() {
        let directive = write_pressure_directive(&FILE_EDITING_TOOLS);
        assert!(directive.contains("write_file"));
        assert!(directive.contains("read-only"));
        assert!(directive.starts_with("[System notice]"));
    }

    #[test]
    fn write_pressure_directive_stops_demanding_a_whole_file_it_can_patch() {
        // The bug this closes: shouted at after five read-only turns, a model
        // repairing an existing port was told to "write the file's current full
        // contents" — a ~25 KB call that cannot fit the output budget, so the
        // instruction itself was unsatisfiable and the exploring continued.
        let patchable = write_pressure_directive(&FILE_EDITING_TOOLS);
        assert!(
            patchable.contains("edit_file"),
            "an editor that is offered must be named: {patchable}"
        );
        assert!(
            patchable.contains("VERBATIM"),
            "targeted editing only works if oldString is copied exactly: {patchable}"
        );
        assert!(
            !patchable.contains("write the file's current full contents"),
            "no whole-file demand when a patch is available: {patchable}"
        );

        // And never a tool the turn doesn't offer.
        let scratch = write_pressure_directive(&["write_file"]);
        assert!(
            !scratch.contains("edit_file"),
            "a locked scratch turn must not be pointed at a withheld tool: {scratch}"
        );
    }

    #[test]
    fn artifact_directive_only_names_editors_that_can_create_the_file() {
        // Nothing exists yet, so `edit_file` would refuse the call it is told to
        // make; the missing-artifact nudge has to stay on write_file.
        let scratch = artifact_delivery_directive(&["write_file"]);
        assert!(scratch.contains("write_file"));
        assert!(scratch.contains("no writes is a failed task"));
        assert!(
            !scratch.contains("edit_file"),
            "edit_file cannot create: {scratch}"
        );
        assert!(scratch.starts_with("[System notice]"));
    }

    #[test]
    fn artifact_nudge_only_fires_for_unmet_file_deliverables() {
        // A report / read-only task never trips the gate, so its text answer
        // stays a valid completion.
        assert!(!should_nudge_missing_artifact(false, false, 0, true));
        // An artifact task that produced nothing, with budget + turns, is nudged.
        assert!(should_nudge_missing_artifact(true, false, 0, true));
        // Once a file has been written the gate is satisfied.
        assert!(!should_nudge_missing_artifact(true, true, 0, true));
    }

    #[test]
    fn artifact_nudge_respects_budget_and_final_turn() {
        // Nudge budget exhausted: stop honestly instead of shouting forever.
        assert!(!should_nudge_missing_artifact(
            true,
            false,
            MAX_ARTIFACT_NUDGES,
            true
        ));
        // No turns left to act on the directive.
        assert!(!should_nudge_missing_artifact(true, false, 0, false));
    }

    #[test]
    fn force_write_directive_demands_the_call_not_prose() {
        // The reconstructed write turn must forbid prose and permit an
        // incomplete file — the two levers that flipped the probe model from
        // narration to an executed write_file call. "map" was the component-map
        // artifact's wording; a port is not a map, so the directive now says
        // file and the test says file.
        let d = force_write_directive("write_file");
        assert!(d.contains("write_file"));
        assert!(d.contains("STOP describing the file in prose"));
        assert!(d.contains("partially-complete file is acceptable"));
        assert!(!d.contains("edit_file"), "not offered on this turn: {d}");
    }

    #[test]
    fn force_write_directive_names_only_the_tools_the_turn_offers() {
        // A locked turn that also advertises the targeted editor must not be
        // told that only a whole-file write counts: that demand is what sends a
        // repair task re-emitting a 25 KB file it could have patched in 200
        // tokens, and the model follows the instruction over the schema.
        let d = force_write_directive("write_file or edit_file");
        assert!(d.contains("write_file or edit_file"), "{d}");
        assert!(
            !d.contains("ONLY correct response now is the write_file tool call"),
            "must not single out write_file: {d}"
        );
    }

    #[test]
    fn an_artifact_run_hides_the_generation_toolset() {
        // The observed drift: a code sub-agent grabbing media-generation tools. An
        // artifact dispatch must not even see them offered or recognised.
        for t in MEDIA_GENERATION_TOOLS {
            assert!(hides_generation_tool(true, t), "artifact run must hide {t}");
        }
        // A software task's real tools stay visible.
        assert!(!hides_generation_tool(true, "write_file"));
        assert!(!hides_generation_tool(true, "read_file"));
        // A report task or IDE turn keeps the whole catalog (media is legitimate there).
        assert!(!hides_generation_tool(false, "generate_image"));
        assert!(!hides_generation_tool(false, "register_generation_model"));
    }

    #[test]
    fn empty_response_is_unfinished() {
        assert!(unfinished_announced_turn(""));
        assert!(unfinished_announced_turn("   \n "));
    }

    #[test]
    fn trailing_colon_or_ellipsis_is_unfinished() {
        assert!(unfinished_announced_turn(
            "I see the issue. Let me fix the test to use unique data:"
        ));
        assert!(unfinished_announced_turn("Next steps…"));
        assert!(unfinished_announced_turn("Running the tests now..."));
        assert!(unfinished_announced_turn("修改说明："));
    }

    #[test]
    fn announcement_last_line_is_unfinished() {
        assert!(unfinished_announced_turn(
            "Some analysis here.\n\nLet me write the benchmarks now."
        ));
        assert!(unfinished_announced_turn("I'll add the tests next."));
        assert!(unfinished_announced_turn("Now I run cargo test."));
    }

    #[test]
    fn genuine_final_summaries_are_finished() {
        assert!(!unfinished_announced_turn(
            "All 14 tests pass and the benchmarks show 3.2 µs per read. The implementation is complete."
        ));
        assert!(!unfinished_announced_turn(
            "Summary: fork shares the offset, close is safe, reads serialize.\n\nLet me know if you want a follow-up."
        ));
        assert!(!unfinished_announced_turn(
            "Done. The crate builds and all tests pass."
        ));
    }

    #[test]
    fn residue_bare_close_tag_is_stripped_and_flagged() {
        // The exact live failure (LiteLLM phase-1, glm-5.2): the inline parser
        // ate the opener; only the function-close tag survived. Raw content
        // "\n</function>" used to be accepted as a "final answer".
        let raw = format!("\n{}", tag("/function"));
        let (cleaned, had) = strip_tool_markup_residue(&raw);
        assert!(had, "a bare close tag must flag residue");
        assert!(cleaned.trim().is_empty());
        // And once stripped, the empty turn is unfinished — never a summary.
        assert!(unfinished_announced_turn(&cleaned));
    }

    #[test]
    fn residue_after_prose_exposes_the_announcement_line() {
        let raw = format!(
            "Let me start by listing the directory.\n{}list_dir{}\n{}",
            tag_open("parameter="),
            tag("/parameter"),
            tag("/tool_call")
        );
        let (cleaned, had) = strip_tool_markup_residue(&raw);
        assert!(had);
        assert_eq!(cleaned, "Let me start by listing the directory.");
        assert!(unfinished_announced_turn(&cleaned));
    }

    #[test]
    fn clean_final_answer_has_no_residue() {
        let plain = "Component map complete:\n| package | crate |\n|---|---|\n| litellm.proxy | litellm-proxy |";
        let (cleaned, had) = strip_tool_markup_residue(plain);
        assert!(!had);
        assert_eq!(cleaned, plain);
        assert!(!unfinished_announced_turn(cleaned.as_ref()));
    }

    #[test]
    fn residue_antml_close_tag_matches_the_second_live_variant() {
        // Run 2 of the same LiteLLM phase-1 mission: the turn's content
        // collapsed to a lone Claude-family invoke-close tag.
        let raw = format!("\n{}antml:invoke>", chr_lt_slash());
        let (cleaned, had) = strip_tool_markup_residue(&raw);
        assert!(had, "antml close tag must flag residue");
        assert!(cleaned.trim().is_empty());
        assert!(unfinished_announced_turn(&cleaned));
    }

    fn chr_lt_slash() -> String {
        // Assembles the  LT SLASH  pair without a literal sequence in source.
        let lt: char = '\u{3c}';
        let mut s = String::new();
        s.push(lt);
        s.push('/');
        s
    }

    fn tool_names() -> Vec<String> {
        [
            "write_file",
            "read_file",
            "delete_file",
            "run_command",
            "grep_search",
            "edit_file",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    #[test]
    fn unknown_tool_error_names_closest_real_tools() {
        // The observed hallucination class: a near-miss name for a file editor.
        let msg = unknown_tool_message("update_file", &tool_names());
        assert!(msg.contains("write_file"), "must suggest file tools: {msg}");
        assert!(msg.contains("read_file"));
        assert!(msg.contains("update_file"), "must echo the bad name");
    }

    #[test]
    fn unknown_tool_message_keeps_failure_prefix() {
        // Failure classification matches on this prefix (any_error, events).
        let msg = unknown_tool_message("frobnicate", &tool_names());
        assert!(msg.starts_with("Error executing tool"));
    }

    #[test]
    fn unknown_tool_with_no_similar_names_still_recovers() {
        let msg = unknown_tool_message("zzz_qxy", &tool_names());
        assert!(msg.contains("no similar tools"), "{msg}");
    }

    #[test]
    fn validate_accepts_a_complete_write_file() {
        let ok = validate_tool_call(
            "write_file",
            &json!({ "relativeFilePath": "src/lib.rs", "content": "fn main(){}" }),
        );
        assert_eq!(ok, None, "well-formed call must pass: {ok:?}");
    }

    #[test]
    fn validate_accepts_a_complete_edit_file() {
        let ok = validate_tool_call(
            "edit_file",
            &json!({
                "relativeFilePath": "src/lib.rs",
                "edits": [{ "oldString": "fn a() {}", "newString": "fn a() { /* ok */ }" }]
            }),
        );
        assert_eq!(ok, None, "well-formed targeted edit must pass: {ok:?}");
    }

    #[test]
    fn validate_rejects_an_edit_file_that_carries_no_edits() {
        // A repair turn that streams the argument object but is cut off before
        // the list arrives must be refused here, not after the file is read.
        let msg = validate_tool_call("edit_file", &json!({ "relativeFilePath": "src/lib.rs" }))
            .expect("a call without `edits` violates the schema");
        assert!(msg.contains("edits"), "{msg}");
    }

    #[test]
    fn validate_rejects_a_blank_required_string() {
        // The live convert failure: write_file emitted with empty `content`.
        let msg = validate_tool_call(
            "write_file",
            &json!({ "relativeFilePath": "src/lib.rs", "content": "   " }),
        )
        .expect("blank content must be rejected");
        assert!(msg.starts_with("Error executing tool"), "{msg}");
        assert!(msg.contains("content"), "{msg}");
    }

    #[test]
    fn validate_rejects_a_missing_required_parameter() {
        let msg = validate_tool_call("write_file", &json!({ "relativeFilePath": "src/lib.rs" }))
            .expect("missing content must be rejected");
        assert!(msg.contains("content"), "{msg}");
    }

    #[test]
    fn validate_rejects_a_wrongly_typed_parameter() {
        let msg = validate_tool_call(
            "write_file",
            &json!({ "relativeFilePath": "src/lib.rs", "content": 42 }),
        )
        .expect("number where a string is declared must be rejected");
        assert!(msg.contains("content") && msg.contains("string"), "{msg}");
    }

    #[test]
    fn validate_ignores_extra_optional_parameters() {
        // `context` is an optional write_file field; a well-formed value must pass.
        let ok = validate_tool_call(
            "write_file",
            &json!({
                "relativeFilePath": "src/lib.rs",
                "content": "x",
                "context": "porting budget_manager"
            }),
        );
        assert_eq!(ok, None, "optional params must not trip the gate: {ok:?}");
    }

    #[test]
    fn validate_passes_through_unregistered_tools() {
        // Unknown names are handled by ToolNotFound, not the schema gate.
        assert_eq!(
            validate_tool_call("frobnicate_the_widget", &json!({ "x": 1 })),
            None,
            "a tool with no declared schema must not be rejected here"
        );
    }

    #[test]
    fn validate_rejects_a_value_outside_the_declared_enum() {
        // The drift this closes: a plausible synonym for ["asc","desc"].
        let msg = validate_tool_call(
            "browser_list_snapshots",
            &json!({ "sortDirection": "ascending" }),
        )
        .expect("a value outside the enum must be rejected");
        assert!(msg.contains("sortDirection"), "{msg}");
        assert!(msg.contains("\"asc\"") && msg.contains("\"desc\""), "{msg}");
    }

    #[test]
    fn validate_accepts_a_declared_enum_value() {
        let ok = validate_tool_call(
            "browser_list_snapshots",
            &json!({ "sortDirection": "desc" }),
        );
        assert_eq!(ok, None, "an allowed enum value must pass: {ok:?}");
    }

    #[test]
    fn validate_enum_comparison_ignores_case() {
        // Case-tolerant so the gate can only ever accept more than the tool
        // itself would, never reject a call that would have worked.
        let ok = validate_tool_call(
            "browser_list_snapshots",
            &json!({ "sortDirection": "DESC" }),
        );
        assert_eq!(
            ok, None,
            "case-only difference must not be rejected: {ok:?}"
        );
    }

    #[test]
    fn validate_rejects_a_capability_outside_its_choice_set() {
        // Longer choice set, straight from the governance schemas.
        let msg = validate_tool_call("set_approval_policy", &json!({ "capability": "reads" }))
            .expect("\"reads\" is not a declared capability");
        assert!(msg.contains("capability"), "{msg}");
        assert!(
            msg.contains("\"read\"") && msg.contains("\"approve\""),
            "{msg}"
        );
    }

    #[test]
    fn validate_rejects_an_out_of_enum_field_inside_an_array_step() {
        // The nested case that actually bites desktop automation: `op` lives on
        // each step of wa_input_sequence, one level below the argument.
        let msg = validate_tool_call(
            "wa_input_sequence",
            &json!({ "steps": [{ "op": "rightclick" }, { "op": "scroll" }] }),
        )
        .expect("a step with an undeclared op must be rejected");
        assert!(msg.contains("step 1"), "{msg}");
        assert!(msg.contains("right_click"), "{msg}");
        assert!(
            !msg.contains("step 2"),
            "the valid step must not be flagged: {msg}"
        );
    }

    #[test]
    fn validate_rejects_a_modifier_outside_the_choice_set() {
        // "control" is the classic drift for ["ctrl", …]; injecting it would
        // silently press nothing rather than fail loudly.
        let msg = validate_tool_call(
            "wa_input_sequence",
            &json!({ "steps": [{ "op": "key_combo", "modifiers": ["control", "shift"] }] }),
        )
        .expect("an unknown modifier must be rejected");
        assert!(msg.contains("modifiers"), "{msg}");
        assert!(msg.contains("\"control\""), "{msg}");
        assert!(msg.contains("\"ctrl\""), "must name the real value: {msg}");
    }

    #[test]
    fn validate_accepts_a_well_formed_input_sequence() {
        let ok = validate_tool_call(
            "wa_input_sequence",
            &json!({ "steps": [{ "op": "key_combo", "modifiers": ["ctrl", "shift"] }] }),
        );
        assert_eq!(ok, None, "a valid sequence must pass: {ok:?}");
    }

    #[test]
    fn validate_reports_several_problems_in_one_refusal() {
        // One turn should teach the model everything wrong with the call,
        // otherwise each repair burns another turn.
        let msg = validate_tool_call(
            "set_approval_policy",
            &json!({ "id": "p1", "capability": "reads", "quorum": "three" }),
        )
        .expect("an out-of-enum value and a wrongly-typed one must both be reported");
        assert!(msg.contains("capability"), "{msg}");
        assert!(msg.contains("quorum"), "{msg}");
    }

    #[test]
    fn validate_does_not_double_flag_a_wrongly_typed_enum_parameter() {
        // `capability` is a string enum; sending a number is one mistake, so
        // the refusal must name it once rather than as type + choice.
        let msg = validate_tool_call(
            "set_approval_policy",
            &json!({ "id": "p1", "capability": 3 }),
        )
        .expect("a number where a string enum is declared must be rejected");
        let hits = msg.matches("capability").count();
        assert_eq!(
            hits, 1,
            "must complain once, not as type AND choice:\n{msg}"
        );
    }

    fn real_rust_body(needle: &str) -> String {
        // Long enough to clear the 600-byte floor, with balanced braces.
        let mut s = format!(
            "use std::collections::HashMap;\n\npub struct A {{\n    pub m: HashMap<String, u32>,\n}}\n\nimpl A {{\n    pub fn f(&mut self) {{\n        self.m.insert(\"{needle}\".into(), 1);\n    }}\n}}\n"
        );
        while s.len() < 700 {
            s.push_str("\n// pad to make the block look like a whole file\n");
        }
        s
    }

    #[test]
    fn fenced_block_recovery_matches_the_probe_response_shape() {
        // Exactly what the real failed repair turn emitted: prose, then a
        // ```rust fence holding the whole corrected file, then prose.
        let body = real_rust_body("budget");
        let prose = format!(
            "Here is the corrected implementation:\n```rust\n{body}```\nThat fixes the cast errors."
        );
        let got = extract_fenced_code_block(&prose, 600).expect("must recover the block");
        assert!(got.starts_with("use std::collections::HashMap;"), "{got}");
        assert!(!got.contains("```"), "fence markers must not leak: {got}");
        assert!(!got.contains("That fixes"), "trailing prose must not leak");
        assert!(got.contains("budget"));
    }

    #[test]
    fn fenced_recovery_skips_snippets_below_the_size_floor() {
        let prose = "You would change it like this:\n```rust\nlet x = 1;\n```\ndone.";
        assert_eq!(extract_fenced_code_block(prose, 600), None);
    }

    #[test]
    fn fenced_recovery_rejects_unbalanced_braces() {
        // A truncated stream or an elided excerpt: writing it would corrupt the
        // file, which is worse than burning the attempt.
        let body = real_rust_body("trunc");
        let cut = body.len() / 2;
        let prose = format!("```rust\n{}\n```\n", &body[..cut]);
        assert_eq!(extract_fenced_code_block(&prose, 600), None);
    }

    #[test]
    fn fenced_recovery_rejects_an_unclosed_fence() {
        let body = real_rust_body("open");
        let prose = format!("```rust\n{body}");
        assert_eq!(extract_fenced_code_block(&prose, 600), None);
    }

    /// The scan advances one BYTE at a time, and a model's prose is full of
    /// multi-byte characters — em dashes, curly quotes, accented identifiers, the
    /// `─` the harness itself puts in a Decision Trail. Slicing at those byte
    /// indices used to abort the dispatch with "end byte index N is not a char
    /// boundary", which is the worst possible failure for a recovery path: the
    /// panic killed the run that existed to save it.
    #[test]
    fn fenced_recovery_scans_through_multibyte_prose_without_panicking() {
        let body = real_rust_body("café");
        let prose = format!(
            "Voici la correction — l’implémentation « complète » est ici :\n```rust\n{body}```\n\
             J’espère que ça aide ── dites‑moi sinon."
        );
        let got = extract_fenced_code_block(&prose, 600).expect("multi-byte text must not stop it");
        assert!(got.starts_with("use std::"), "language tag stripped: {got}");
        assert!(got.contains("café"), "the body must survive intact");
        assert!(!got.contains("```"), "fence must not leak: {got}");
    }

    /// A truncated stream is what the balance check is for, but a naive count
    /// also fires on a *complete* file that mentions a brace in prose: a lexer
    /// port, a template renderer, a comment explaining a `{`. That is the model's
    /// one correct answer thrown away, and the attempt burned with it.
    #[test]
    fn fenced_recovery_accepts_a_block_that_only_looks_unbalanced() {
        let mut body = real_rust_body("fence");
        body.push_str(
            "\n// the table opens with { and the block closes again\n\
             // this line quotes a lone brace: {\n",
        );
        let prose = format!("```rust\n{body}```");
        let got = extract_fenced_code_block(&prose, 600)
            .expect("a brace inside a comment is not an unclosed block");
        assert!(got.contains("lone brace"), "{got}");
        assert!(got.contains("fence"));
    }

    #[test]
    fn fenced_recovery_prefers_the_largest_block() {
        let body = real_rust_body("full");
        let prose = format!(
            "First a small note:\n```rust\npub fn tiny() {{}}\n```\nThen the file:\n```rust\n{body}```"
        );
        let got = extract_fenced_code_block(&prose, 600).unwrap();
        assert!(got.contains("full") && got.len() > 600, "{got}");
    }

    /// The fence scanner steps through the string BYTES, so an index landing
    /// inside a multi-byte character sliced past a char boundary. A live crucible
    /// dispatch died on the model's own prose — `end byte index 33 is not a char
    /// boundary; it is inside '—' (bytes 32..35)` — taking the whole run down.
    #[test]
    fn a_multibyte_character_before_a_fence_does_not_panic_the_scanner() {
        let body = real_rust_body("dash");
        let prose = format!(
            "The crate BUILDS \u{2014} but its own tests do NOT \u{2014} pass:\n```rust\n{body}\n```\n\
             Fix the assertions \u{2014} not the library."
        );
        let got = extract_fenced_code_block(&prose, 600).expect("the block is still recovered");
        assert!(got.contains("dash") && got.len() > 600, "{got}");
        assert!(
            !got.contains('\u{2014}'),
            "prose must not leak into the file"
        );
    }

    #[test]
    #[ignore = "requires the live-MoE probe dump at this absolute path"]
    fn fenced_recovery_on_real_moe_repair_output_is_well_formed_rust() {
        let dir = r"C:\Users\visse\OneDrive\Documents\Velocity-IDE\.velocity_testing";
        let raw = std::fs::read_to_string(format!("{dir}/probe_clean_content.txt"))
            .expect("probe output present");
        let body = extract_fenced_code_block(&raw, 600).expect("must recover the real repair");
        assert!(body.starts_with("use std::"), "lang tag stripped: {body}");
        assert!(!body.contains("```"), "fence must not leak");
        std::fs::write(
            format!("{dir}/rust_port_extracted_budget_manager.rs"),
            &body,
        )
        .unwrap();
        println!("extracted {} bytes", body.len());
    }

    #[test]
    fn exact_substring_match_outranks_token_noise() {
        // "read_file_content" hallucinated from read_file: read_file ranks first.
        let msg = unknown_tool_message("read_file_content", &tool_names());
        let first = msg
            .split('[')
            .nth(1)
            .unwrap_or("")
            .split(']')
            .next()
            .unwrap_or("");
        assert!(first.starts_with("read_file"), "ranked list: {first}");
    }

    #[test]
    fn real_registry_has_edit_file_and_ranks_file_tools_for_a_near_miss() {
        // `edit_file` exists now — a model asking for it gets a real targeted
        // patch instead of a ToolNotFound. The ranking regression it replaced is
        // still worth locking: the alphabetical tiebreak used to cut write_file
        // out of the top six real candidates behind create_skill_file and gui_*.
        let all: Vec<String> = crate::registry::tool_definitions::get_tools()
            .iter()
            .map(|t| t.name.clone())
            .collect();
        assert!(
            all.iter().any(|n| n == "edit_file"),
            "edit_file must be registered"
        );
        let msg = unknown_tool_message("edit_file_content", &all);
        let list = msg
            .split('[')
            .nth(1)
            .unwrap_or("")
            .split(']')
            .next()
            .unwrap_or("");
        assert!(list.contains("write_file"), "top candidates: {list}");
        assert!(list.contains("read_file"), "top candidates: {list}");
    }

    #[test]
    fn fragment_scan_recovers_calls_embedded_mid_line() {
        // Live run-3 shape: the gateway interleaved native-call fragments
        // with prose, one opener embedded at the end of a sentence.
        let lt = lt();
        let raw = format!(
            "Let me explore the repo.\n{lt}function=list_dir>\n\
             {lt}parameter=relativePath\nlitellm\n{lt}/parameter>\n\
             {lt}/function>\nNow the reader: {lt}function=read_file>\n\
             {lt}parameter=file_path\nlitellm/auth.py{lt}/parameter>\n\
             {lt}/function>",
        );
        let (prose, calls, residue) = parse_function_fragments(&raw);
        assert!(residue);
        assert_eq!(
            calls.len(),
            2,
            "both interleaved calls recovered: {calls:?}"
        );
        assert_eq!(calls[0].0, "list_dir");
        assert_eq!(calls[0].1["relativePath"].as_str().unwrap(), "litellm");
        assert_eq!(calls[1].0, "read_file");
        assert_eq!(calls[1].1["file_path"].as_str().unwrap(), "litellm/auth.py");
        assert!(prose.contains("Let me explore the repo."));
        assert!(prose.contains("Now the reader:"));
        assert!(!prose.contains("function="));
    }

    #[test]
    fn fragment_scan_truncated_call_yields_no_executable_partial() {
        // An fn-open marker cut off at EOF must never surface a partial
        // call: flag residue and hand the content to the stripper instead.
        let lt = lt();
        let raw = format!("Working on it.\n{lt}function=run_");
        let (prose, calls, residue) = parse_function_fragments(&raw);
        assert!(residue);
        assert!(calls.is_empty());
        // The residue stripper then removes the marker even though it sat
        // mid-line, and the flagged turn drives the re-issue notice rather
        // than a "final answer" (markup_residue short-circuits the accept).
        let (cleaned, had) = strip_tool_markup_residue(&prose);
        assert!(had);
        assert!(!cleaned.contains("function="), "markup survived: {cleaned}");
        assert!(cleaned.contains("Working on it."));
    }

    #[test]
    fn fragment_scan_clean_prose_passes_through() {
        let (prose, calls, residue) =
            parse_function_fragments("Component map written to phase1.json.");
        assert!(!residue);
        assert!(calls.is_empty());
        assert_eq!(prose, "Component map written to phase1.json.");
    }

    #[test]
    fn fragment_scan_attribute_style_key_quotes_parse() {
        // Attribute spellings of the parameter key: quoted, and bracket-
        // terminated with the value riding inline after the close bracket.
        let lt = lt();
        let quoted = format!(
            "{lt}function=grep_search>\n{lt}parameter=\"searchText\">query{lt}/parameter>\n{lt}/function>",
        );
        let (_p1, c1, r1) = parse_function_fragments(&quoted);
        assert!(r1);
        assert_eq!(c1.len(), 1);
        assert_eq!(c1[0].0, "grep_search");
        assert_eq!(c1[0].1["searchText"].as_str().unwrap(), "query");

        let inline = format!(
            "{lt}function=list_dir>{lt}parameter=relativePath>litellm{lt}/parameter{lt}/function>",
        );
        let (_p2, c2, r2) = parse_function_fragments(&inline);
        assert!(r2);
        assert_eq!(c2.len(), 1);
        assert_eq!(c2[0].1["relativePath"].as_str().unwrap(), "litellm");
    }

    // ── honest termination of a no-tool-call turn ──────────────────────

    #[test]
    fn residue_turn_is_never_accepted_as_final_answer() {
        // The b6e defect: glm-5.2 streamed a call whose markup arrived
        // broken, the parsers recovered nothing, and the loop recorded
        // "(final answer, no tool calls)" + handover "completed" one turn
        // into the mission. Markup residue must route to nudge, then blocked.
        let residue_only = "\n</antml:invoke>";
        assert_eq!(
            decide_no_tool_call_ending(true, false, residue_only, 0, 0, 0),
            NoToolCallEnding::Nudge
        );
        assert_eq!(
            decide_no_tool_call_ending(true, false, residue_only, MAX_AUTO_CONTINUATIONS - 1, 0, 0),
            NoToolCallEnding::Nudge
        );
        assert_eq!(
            decide_no_tool_call_ending(true, false, residue_only, MAX_AUTO_CONTINUATIONS, 0, 0),
            NoToolCallEnding::Blocked,
            "budget exhausted must report blocked, not completed"
        );
    }

    #[test]
    fn empty_content_channel_is_nudged_then_blocked() {
        assert_eq!(
            decide_no_tool_call_ending(false, true, "", 0, 0, 0),
            NoToolCallEnding::Nudge
        );
        assert_eq!(
            decide_no_tool_call_ending(false, true, "", MAX_AUTO_CONTINUATIONS, 0, 0),
            NoToolCallEnding::Blocked
        );
    }

    #[test]
    fn real_summaries_finish_and_announcements_do_not() {
        assert_eq!(
            decide_no_tool_call_ending(
                false,
                false,
                "Component map complete: litellm.proxy ports to litellm-proxy.",
                0,
                0,
                0
            ),
            NoToolCallEnding::FinalAnswer
        );
        assert_eq!(
            decide_no_tool_call_ending(false, false, "Let me list the package now.", 0, 0, 0),
            NoToolCallEnding::Nudge
        );
    }

    #[test]
    fn clean_final_answer_is_not_hijacked_by_the_blocked_path() {
        // Guard against the widening: a summary that merely mentions a tag
        // name in prose must still terminate as completed, since reporting
        // every such turn blocked would be its own false alarm.
        let summary = "The inline dialect wraps calls in a tool_calls block; \
                       each parameter closes before the next one opens.";
        assert_eq!(
            decide_no_tool_call_ending(false, false, summary, 0, 0, 0),
            NoToolCallEnding::FinalAnswer
        );
        assert!(!strip_tool_markup_residue(summary).1);
    }

    /// `MAX_AUTO_CONTINUATIONS` bounds how many times a model may be re-asked,
    /// which on a local MoE is not the same as bounding what that costs it: a
    /// measured convert spent its whole 1,497s budget on four planning turns that
    /// emitted no tool call and wrote no file. The cost bound has to exist
    /// alongside the count bound.
    #[test]
    fn planning_turns_are_stopped_by_cost_as_well_as_by_count() {
        let announcement = "Let me design the Rust implementation now.";
        // Two fifths of the budget gone on nothing — stop, whatever the count.
        assert_eq!(
            decide_no_tool_call_ending(false, false, announcement, 1, 600, 1500),
            NoToolCallEnding::Stalled
        );
        // Still cheap enough to try again.
        assert_eq!(
            decide_no_tool_call_ending(false, false, announcement, 1, 300, 1500),
            NoToolCallEnding::Nudge
        );
        // A deliberate summary is never a stall, however long it took.
        assert_eq!(
            decide_no_tool_call_ending(false, false, summary_done(), 1, 1400, 1500),
            NoToolCallEnding::FinalAnswer
        );
    }

    /// A run with no wall-clock bound — an operator at the keyboard — must not
    /// be stopped for spending time, or the interactive loop would inherit the
    /// unattended policy.
    #[test]
    fn an_unbounded_run_is_never_stalled_for_cost() {
        assert!(
            !wasted_time_ends_the_run(9999, 0),
            "budget_secs 0 means no deadline, not an instantly-expired one"
        );
        // The bound is 40% of the budget, inclusive at the threshold.
        assert!(wasted_time_ends_the_run(600, 1500));
        assert!(!wasted_time_ends_the_run(599, 1500));
    }

    fn summary_done() -> &'static str {
        "Port complete: budget_manager.rs written with 23 passing tests."
    }

    #[test]
    fn compile_repair_directive_carries_verbatim_diagnostics_and_forces_a_write() {
        // The repair turn only helps if the model sees the exact rustc output and
        // is told to re-issue the file, so the directive must embed the errors and
        // name both file-editing tools — not let the model answer the failure in
        // prose. edit_file leads because a ~200-token patch fits the output budget
        // that a whole-file rewrite of a 30 KB port blows through.
        let temp = tempfile::TempDir::new().unwrap();
        let errors = "error[E0382]: borrow of moved value: `user`";
        let d = compile_repair_directive(errors, temp.path());
        assert!(
            d.contains(errors),
            "directive must pass rustc output verbatim"
        );
        assert!(
            d.contains("write_file"),
            "directive must keep whole-file rewrite available"
        );
        assert!(
            d.contains("edit_file"),
            "directive must offer targeted edits"
        );
        // Diagnostics without a readable location must not fabricate a block.
        assert!(
            !d.contains("verbatim from disk"),
            "no location, no excerpt: {d}"
        );
    }

    /// The `convert11` regression. rustc's own advice for an unresolved crate
    /// tells the model to run `cargo add`, in the same message that says
    /// Cargo.toml is off-limits. A 35B model given both does neither, and the
    /// port kept its `serde_json` through all three build repairs.
    #[test]
    fn a_zero_dependency_crate_is_never_advised_to_run_cargo_add() {
        let errors = "error[E0433]: cannot find module or crate `serde_json` in this scope\n   \
                      = help: if you wanted to use a crate named `serde_json`, use `cargo add \
                      serde_json` to add it to your `Cargo.toml`\n";
        let fixed = correct_missing_crate_help(errors, Some(&[]));
        assert!(
            !fixed.contains("use `cargo add"),
            "the directive must not tell the model to add a crate it may not add:\n{fixed}"
        );
        assert!(!fixed.contains("to add it to your `Cargo.toml`"), "{fixed}");
        assert!(fixed.contains("ZERO dependencies"), "{fixed}");
        assert!(
            fixed.contains("delete every use of `serde_json`"),
            "{fixed}"
        );
        // The error header itself is untouched: the model still needs to see
        // what rustc actually said.
        assert!(fixed.contains("error[E0433]"), "{fixed}");
        // rustc indents the help line inside its block; so does the note.
        let indented = fixed
            .lines()
            .find(|l| l.contains("= note:"))
            .unwrap_or_default();
        assert!(indented.starts_with("   ="), "{indented:?}");
    }

    /// A crate the manifest does declare is failing for some other reason —
    /// a feature gate, a renamed import — and rustc's hint might be the useful
    /// one. Rewriting it would replace true advice with a guess.
    #[test]
    fn a_declared_crate_keeps_rustcs_own_hint() {
        let declared = vec!["serde_json".to_string()];
        let line = "   = help: if you wanted to use a crate named `serde_json`, use `cargo add \
                    serde_json` to add it to your `Cargo.toml`";
        assert!(
            missing_crate_help(line, Some(&declared)).is_none(),
            "declared crate: rustc's advice stands"
        );
        // Hyphens in Cargo.toml and underscores in `use` are the same crate.
        assert!(
            missing_crate_help(line, Some(&["serde-json".to_string()])).is_none(),
            "name punctuation must not fake a missing crate"
        );
        assert!(missing_crate_help(line, Some(&[])).is_some());
        // No manifest means no knowledge, so say nothing rather than forbid.
        assert!(missing_crate_help(line, None).is_none());
        // Any other line is left alone, byte for byte.
        assert!(missing_crate_help("error[E0308]: mismatched types", Some(&[])).is_none());
    }

    /// The rule has to survive the round trip through the real directive, or the
    /// model reads the contradiction and the fix is cosmetic.
    #[test]
    fn the_repair_directive_hands_over_no_cargo_add_when_the_manifest_forbids_it() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            temp.path().join("Cargo.toml"),
            "[package]\nname = \"port\"\nversion = \"0.1.0\"\n\n[dependencies]\n",
        )
        .unwrap();
        let d = compile_repair_directive(
            "error[E0433]: cannot find module or crate `serde_json` in this scope\n   \
             = help: if you wanted to use a crate named `serde_json`, use `cargo add serde_json` \
             to add it to your `Cargo.toml`\n",
            temp.path(),
        );
        assert!(!d.contains("use `cargo add"), "{d}");
        // Once, from the rule; not twice, counting a rewritten help line.
        assert_eq!(
            d.matches("ZERO dependencies").count(),
            2,
            "rule plus corrected note: {d}"
        );
    }

    /// A test failure is a different complaint from a build failure, and the
    /// model has to hear it as one: the cheapest way to green a test run is to
    /// delete the assertion, which is precisely what the caller will grade as a
    /// failed port.
    #[test]
    fn a_failing_test_is_repaired_as_a_test_not_as_a_compile_error() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(
            temp.path().join("src").join("budget_manager.rs"),
            "fn main() {}\nassert!((projected - 0.009).abs() < f64::EPSILON);\n",
        )
        .unwrap();
        let report = format!(
            "{} Real `cargo test` output (test result: FAILED. 68 passed; 2 failed), one block \
             per failing test:\nerror: test `budget_manager::tests::test_projected_cost` failed\n  \
             --> src/budget_manager.rs:2:1\n2 | assert!((projected - 0.009).abs() < f64::EPSILON);\n  \
             | assertion failed: (projected - 0.009).abs() < f64::EPSILON\n",
            crate::agent::executor::thread::TEST_FAILURE_PREAMBLE
        );

        assert_eq!(gate_failure_label(&report), "The crate's tests failed");
        assert_eq!(
            gate_failure_label("error[E0308]: mismatched types\n  --> src/lib.rs:3:9"),
            "Compilation failed"
        );

        let d = compile_repair_directive(&report, temp.path());
        assert!(d.contains("cargo test"), "{d}");
        assert!(d.contains("Do NOT delete"), "{d}");
        assert!(
            !d.contains("compiled with ERRORS"),
            "the build is green; telling the model it failed sends it hunting \
             for a type error: {d}"
        );
        assert!(d.contains("edit_file"), "a test fix is still a patch");
        assert!(
            d.contains("verbatim from disk"),
            "the assertion line must be pasted, not guessed: {d}"
        );
    }

    /// A failed assertion is only fixable against the code that computes the
    /// asserted value, and a repair turn has no `read_file`: testgate3's model
    /// answered that by asking for the withheld tool five times and firing zero
    /// calls. The loop has to paste what the read would have returned — the whole
    /// test plus the helpers it calls — or the turn cannot be satisfied at all.
    #[test]
    fn a_test_repair_gets_the_whole_failing_test_and_the_helpers_it_calls() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        let src = "fn compute_cost(tokens: usize) -> f64 {\n    (tokens as f64) * 0.001\n}\n\n\
                   #[cfg(test)]\nmod tests {\n    use super::compute_cost;\n\n    \
                   fn make_manager() -> Vec<usize> {\n        vec![2, 2]\n    }\n\n    \
                   #[test]\n    fn test_projected_cost() {\n        let usage = make_manager();\n        \
                   let projected = compute_cost(usage[0]) + compute_cost(usage[1]);\n        \
                   assert!((projected - 0.009).abs() < f64::EPSILON);\n    }\n}\n";
        std::fs::write(temp.path().join("src").join("budget_manager.rs"), src).unwrap();
        let at = src
            .lines()
            .position(|l| l.contains("assert!((projected"))
            .unwrap()
            + 1;
        let report = format!(
            "{} Real `cargo test` output (test result: FAILED. 69 passed; 2 failed), one block \
             per failing test:\nerror: test `tests::test_projected_cost` failed\n  \
             --> src/budget_manager.rs:{at}:1\n   | assertion failed\n",
            crate::agent::executor::thread::TEST_FAILURE_PREAMBLE
        );

        let d = compile_repair_directive(&report, temp.path());
        assert!(
            d.contains("let projected = compute_cost(usage[0])"),
            "the whole test body, not a window around the panic: {d}"
        );
        assert!(
            d.contains("fn make_manager() -> Vec<usize>"),
            "the helper that supplies the fixture: {d}"
        );
        assert!(
            d.contains("(tokens as f64) * 0.001"),
            "and the function the asserted number comes from: {d}"
        );
        assert!(
            d.contains("#[test]"),
            "so the model knows which block is the test: {d}"
        );
        assert!(
            !d.contains("fn compute_cost(tokens: usize) -> f64 {\n    (tokens as f64) * 0.001\n}\n\n#[cfg(test)]"),
            "a library block must be pasted as the function, not as the file after it"
        );
        assert!(
            d.contains("Correct the assertion to the value this code actually produces"),
            "and say which side of the mismatch to move: {d}"
        );
    }

    /// The block finder decides what "the failing test" means, and a guessed
    /// range is worse than none: it would paste unrelated lines as if they were
    /// the assertion's context.
    #[test]
    fn enclosing_fn_block_stops_at_the_functions_own_closer_and_keeps_its_attributes() {
        let lines: Vec<String> = "fn compute(tokens: usize) -> f64 {\n    tokens as f64\n}\n\n\
                                  #[test]\nfn t() {\n    let open = \"{\";\n    assert!(true);\n}\n"
            .lines()
            .map(str::to_string)
            .collect();
        // The panic is on line index 7; the block runs from its `#[test]` (4) to
        // its own closer (8) — and the `{` inside the string on index 6 must not
        // be read as an opening brace.
        assert_eq!(enclosing_fn_block(&lines, 7), Some((4, 9)));
        // A line no `fn` owns comes back as nothing at all, so the caller falls
        // back to the tight window instead of inventing a block.
        let orphan: Vec<String> = "fn main() {}\nassert!(false);\n"
            .lines()
            .map(str::to_string)
            .collect();
        assert_eq!(enclosing_fn_block(&orphan, 1), None);
    }

    /// The compile branch keeps its own wording — the test-failure headline is a
    /// branch, not a replacement.
    #[test]
    fn a_compile_error_directive_does_not_talk_about_tests() {
        let temp = tempfile::TempDir::new().unwrap();
        let d = compile_repair_directive("error[E0308]: mismatched types", temp.path());
        assert!(d.contains("compiled with ERRORS"), "{d}");
        assert!(!d.contains("Do NOT delete"), "{d}");
    }

    /// Repeat detection runs on rustc-shaped blocks, so the test report has to
    /// wear the same shape or the loop cannot see a model re-failing the same
    /// assertion three turns in a row.
    #[test]
    fn a_test_report_signatures_like_a_compile_error_so_repeats_are_seen() {
        let report = "The crate BUILDS, but its own tests do NOT pass.\n\
             error: test `budget_manager::tests::test_projected_cost` failed\n  \
             --> src/budget_manager.rs:535:1\n\
             535 | assert!((projected - 0.009).abs() < f64::EPSILON);\n";
        let sigs = error_signatures(report);
        assert_eq!(sigs.len(), 1, "{sigs:?}");
        let history = vec![sigs];
        assert_eq!(prior_encounters(report, &history), 1);
    }

    /// The completion gate only runs on the no-tool-call branch, so a run that
    /// burns its budget mid-repair needs a second look at the files — and only
    /// such a run gets one.
    #[test]
    fn a_run_that_never_reached_its_final_answer_still_validates_its_artifact() {
        // Artifact task, Rust on disk from this run, no validated completion.
        assert!(should_close_validate(true, true, false));
        // The completion gate already ran and passed: no second cargo invocation.
        assert!(!should_close_validate(true, true, true));
        // A report-only task is judged by its answer, not by files it never owed.
        assert!(!should_close_validate(false, true, false));
        // Nothing written means no crate to build or test.
        assert!(!should_close_validate(true, false, false));
    }

    /// Write a manifest into a fresh workspace and hand back the temp dir, which
    /// must outlive the assertions.
    fn workspace_with_manifest(manifest: &str) -> tempfile::TempDir {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::write(temp.path().join("Cargo.toml"), manifest).unwrap();
        temp
    }

    #[test]
    fn a_dependency_free_manifest_keeps_the_std_only_repair_rule() {
        // The offline porting fixture: an empty `[dependencies]` table. Here the
        // std-only instruction is correct, and reimplementation-by-hand is the
        // only available repair, so the rule must say so.
        let temp = workspace_with_manifest(
            "[package]\nname = \"litellm-port\"\nedition = \"2021\"\n\n[dependencies]\n",
        );
        assert_eq!(
            declared_dependencies(temp.path()),
            Some(Vec::new()),
            "an empty table is \"no dependencies\", not \"unknown\""
        );
        let rule = dependency_repair_rule(temp.path());
        assert!(rule.contains("ZERO dependencies"), "{rule}");
        assert!(rule.contains("REIMPLEMENT"), "{rule}");
        assert!(
            compile_repair_directive("error[E0432]: unresolved import", temp.path())
                .contains("std"),
            "the directive must still carry the rule"
        );
    }

    #[test]
    fn a_manifest_that_declares_crates_stops_the_claim_that_there_are_none() {
        // Three declaration shapes cargo accepts, all found: the `key = value`
        // form inside a table, the `[dependencies.name]` table-header form, and
        // `[dev-dependencies]`. Package metadata must not be misread as crates.
        let temp = workspace_with_manifest(
            "[package]\nname = \"engine\"\nversion = \"1.0.0\"\n\n\
             [dependencies]\nserde = { version = \"1\", features = [\"derive\"] }\n\
             # a comment\nrand = \"0.8\"\n\n[dependencies.tokio]\nfeatures = [\"full\"]\n\n\
             [dev-dependencies]\ntempfile = \"3\"\n\n[features]\ndefault = []\n",
        );
        assert_eq!(
            declared_dependencies(temp.path()).unwrap().join(","),
            "rand,serde,tempfile,tokio",
            "sorted, deduped, and free of package/feature keys"
        );
        let rule = dependency_repair_rule(temp.path());
        assert!(rule.contains("serde") && rule.contains("tokio"), "{rule}");
        assert!(
            !rule.contains("ZERO dependencies"),
            "the directive must not lie about a crate that has dependencies: {rule}"
        );
        assert!(rule.contains("do NOT add another"), "{rule}");
    }

    #[test]
    fn an_unreadable_manifest_leaves_the_repair_rule_agnostic() {
        // No manifest at all is a different fact from an empty one, so the rule
        // must not assert either.
        let temp = tempfile::TempDir::new().unwrap();
        assert_eq!(declared_dependencies(temp.path()), None);
        let rule = dependency_repair_rule(temp.path());
        assert!(rule.contains("Do not assume"), "{rule}");
        assert!(
            !rule.contains("ZERO dependencies") && !rule.contains("declares dependencies"),
            "no manifest, no claim about dependencies: {rule}"
        );
    }

    #[test]
    fn the_write_gate_arms_only_on_progress_it_can_ask_for() {
        // All three bounds, and only the combination: no attempts left means
        // stop, byte-identical diagnostics mean the model was already handed this
        // error, and a third sighting of the same error means the repair is
        // circling rather than converging.
        assert!(should_arm_write_repair(true, true, true));
        assert!(
            !should_arm_write_repair(true, false, true),
            "the same diagnostics again must not consume an attempt"
        );
        assert!(
            !should_arm_write_repair(false, true, true),
            "a spent repair budget must not be overspent"
        );
        assert!(
            !should_arm_write_repair(true, true, false),
            "a loop the model is inside must not be paid for again"
        );
    }

    /// A repair that swaps one error for another and back produces different
    /// text every time, so only the error's identity — code, file, flagged
    /// source line — can show the circle. Line numbers cannot: the file grows.
    #[test]
    fn an_error_is_identified_by_its_code_file_and_flagged_line_not_its_line_number() {
        let first = "These failures are in `#[cfg(test)]` code.\n\
             error[E0053]: method `save_data` has an incompatible type for trait\n\
             --> src\\budget_manager.rs:387:13\n\
             |\n\
             387 |             &mut self,\n\
             |             ^^^^^^^^^ types differ in mutability\n";
        // Same error after the file gained four lines above it.
        let shifted = "error[E0053]: method `save_data` has an incompatible type for trait\n\
             --> src\\budget_manager.rs:391:13\n\
             |\n\
             391 |             &mut self,\n\
             |             ^^^^^^^^^ types differ in mutability\n";
        // The other face of the same contradiction, at the same place.
        let flipped = "error[E0594]: cannot assign to `self.data` through `&self`\n\
             --> src\\budget_manager.rs:391:13\n\
             |\n\
             391 |             self.data = user_dict.clone();\n";

        let a = error_signatures(first);
        let b = error_signatures(shifted);
        let c = error_signatures(flipped);
        assert_eq!(
            a, b,
            "a shifted line number must still read as the same error: {a:?} vs {b:?}"
        );
        assert_eq!(a[0], "E0053|budget_manager.rs|&mut self,");
        assert_ne!(a, c, "a different error is not the same one");
        assert_eq!(
            prior_encounters(flipped, std::slice::from_ref(&a)),
            0,
            "the first sighting of E0594 is not a repeat"
        );
        assert_eq!(
            prior_encounters(first, &[a.clone(), c.clone()]),
            1,
            "seeing E0053 again after one earlier message is a repeat"
        );
        assert_eq!(
            prior_encounters(first, &[a.clone(), c.clone(), a.clone()]),
            2,
            "a third sighting is the loop"
        );
    }

    /// A one-file crate in a temp dir, because the item half of a signature needs
    /// the source rustc quoted to actually be on disk at the reported line. The
    /// `TempDir` is handed back so it outlives the assertions.
    fn crate_with_source(lines: &[&str]) -> (tempfile::TempDir, std::path::PathBuf) {
        let temp = tempfile::TempDir::new().unwrap();
        let src = temp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let file = src.join("budget_manager.rs");
        std::fs::write(&file, format!("{}\n", lines.join("\n"))).unwrap();
        (temp, file)
    }

    /// The fixture the 8 GB probes actually left behind: the same bad field fill
    /// in two methods, so an error's function — not its line — is what tells them
    /// apart. Line 10 is inside `create_budget`, 17 inside `update_cost`, 22
    /// inside `settle`.
    fn budget_crate_lines() -> [&'static str; 25] {
        [
            "use std::collections::HashMap;",
            "",
            "pub struct PersistedUserRecord { pub user_dict: HashMap<String, f64> }",
            "pub struct UpdateCostResult<'a> { pub user_record: &'a PersistedUserRecord }",
            "",
            "impl Manager {",
            "    pub fn create_budget(&mut self, user: &str) -> UpdateCostResult<'_> {",
            "        let result_record = PersistedUserRecord { user_dict: HashMap::new() };",
            "        UpdateCostResult {",
            "            user_record: result_record,",
            "        }",
            "    }",
            "",
            "    pub fn update_cost(&mut self, user: &str, cost: f64) -> UpdateCostResult<'_> {",
            "        let result_record = PersistedUserRecord { user_dict: HashMap::new() };",
            "        UpdateCostResult {",
            "            user_record: result_record,",
            "        }",
            "    }",
            "",
            "    pub fn settle(&self, user: &str) -> f64 {",
            "        let total = self.totals[user];",
            "        total",
            "    }",
            "}",
        ]
    }

    /// Round one against that fixture: the same fill failing in both methods.
    const TWO_FIELD_ERRORS: &str = "error[E0308]: mismatched types\n \
        --> src/budget_manager.rs:10:13\n10 |             user_record: result_record,\n\
        error[E0308]: mismatched types\n --> src/budget_manager.rs:17:13\n\
        17 |             user_record: result_record,\n";

    /// Round two: `update_cost`'s fill rewritten where it stands, plus a complaint
    /// out of `settle` that the model has never been shown.
    const REWRITTEN_AND_NEW: &str = "error[E0308]: mismatched types\n \
        --> src/budget_manager.rs:17:13\n17 |             user_record: &result_record,\n\
        error[E0609]: no field `totals` on type `&Manager`\n\
        --> src/budget_manager.rs:22:21\n22 |         let total = self.totals[user];\n";

    /// `lr7b_v10` is what this covers: `E0308` in `budget_manager.rs` came back
    /// five gate reports in a row and the loop never saw a repeat, because the
    /// model rewrote the offending expression every round —
    /// `user_record: result_record`, then `user_record: &result_record` — and the
    /// file grew a statement above it. New text, new line number, same error in
    /// the same function. The item half of the signature is what notices.
    #[test]
    fn an_error_rewritten_in_place_is_still_the_error_the_model_was_shown() {
        let before = [
            "use std::collections::HashMap;",
            "",
            "pub struct PersistedUserRecord { pub user_dict: HashMap<String, f64> }",
            "pub struct UpdateCostResult<'a> { pub user_record: &'a PersistedUserRecord }",
            "",
            "impl Manager {",
            "    pub fn update_cost(&self, user: &str, cost: f64) -> UpdateCostResult<'_> {",
            "        let result_record = PersistedUserRecord { user_dict: HashMap::new() };",
            "        UpdateCostResult {",
            "            user_record: result_record,",
            "        }",
            "    }",
            "}",
        ];
        let (temp, file) = crate_with_source(&before);
        let root = temp.path();
        let first = "error[E0308]: mismatched types\n --> src/budget_manager.rs:10:13\n   |\n\
             10 |             user_record: result_record,\n   |             ^^^^^^^^^^^ \
             expected `&PersistedUserRecord`, found `PersistedUserRecord`\n";
        let round1 = super::error_signatures(first, Some(root));
        assert_eq!(
            round1,
            vec![
                "E0308|budget_manager.rs|user_record: result_record,",
                "E0308|budget_manager.rs|inside fn update_cost",
            ],
            "the line it was quoted at, and the item it sits in: {round1:?}"
        );

        // The model takes a reference and the function gains a statement, so the
        // error is one line lower with different text.
        let after = {
            let mut lines: Vec<&str> = before[..8].to_vec();
            lines.push("        let _ = user;");
            lines.push(before[8]);
            lines.push("            user_record: &result_record,");
            lines.extend_from_slice(&before[10..]);
            lines
        };
        std::fs::write(&file, format!("{}\n", after.join("\n"))).unwrap();
        let second = "error[E0308]: mismatched types\n --> src/budget_manager.rs:11:13\n   |\n\
             11 |             user_record: &result_record,\n   |             ^^^^^^^^^^^ \
             expected `&PersistedUserRecord`, found `PersistedUserRecord`\n";
        let round2 = super::error_signatures(second, Some(root));
        assert_eq!(round2[1], round1[1], "the item did not move");
        assert_ne!(round2[0], round1[0], "and the line text did not survive");
        assert_eq!(
            super::prior_encounters(second, std::slice::from_ref(&round1), Some(root)),
            1,
            "so the item key is the only thing that can call it a repeat"
        );
        assert_eq!(
            super::prior_encounters(second, std::slice::from_ref(&round1), None),
            0,
            "which is exactly what the line-text key missed"
        );

        // And the model is told WHERE it is going in circles, not just that the
        // text came back — the text did not come back.
        let note = super::recurring_error_note(second, std::slice::from_ref(&round1), Some(root));
        assert!(note.contains("re-reporting"), "{note}");
        assert!(
            note.contains("inside `fn update_cost`"),
            "name the place that is still wrong: {note}"
        );
    }

    /// The other half, and the reason the key names an item rather than loosening
    /// to `(code, file)`: the place is what tells two complaints in one file apart.
    /// A new one in a place the model has never been shown must stay new work — a
    /// loop note there is a lie about progress, and it costs the run its repairs.
    #[test]
    fn an_item_key_tells_a_new_complaint_apart_from_an_old_one() {
        let (temp, _) = crate_with_source(&budget_crate_lines());
        let root = temp.path();
        let in_create = "error[E0308]: mismatched types\n --> src/budget_manager.rs:10:13\n\
             10 |             user_record: result_record,\n";
        let round1 = super::error_signatures(in_create, Some(root));
        assert!(
            round1
                .iter()
                .any(|s| s.ends_with("|inside fn create_budget")),
            "{round1:?}"
        );

        // The same code, the same file, a DIFFERENT function, and the expression
        // rewritten on the way there: the model moved, so this is not the old
        // error coming back.
        let moved = "error[E0308]: mismatched types\n --> src/budget_manager.rs:17:13\n\
             17 |             user_record: self.record(user),\n";
        assert_eq!(
            super::prior_encounters(moved, std::slice::from_ref(&round1), Some(root)),
            0,
            "an error that moved to another item with new text is progress"
        );
        // Carrying the identical expression into that other function is a
        // different story: nothing about the complaint is new, so it counts.
        let carried = "error[E0308]: mismatched types\n --> src/budget_manager.rs:17:13\n\
             17 |             user_record: result_record,\n";
        assert_eq!(
            super::prior_encounters(carried, std::slice::from_ref(&round1), Some(root)),
            1,
            "the same line text is the same complaint wherever it sits"
        );
    }

    /// The payoff, measured against `lr7b_v10`'s shape: with two errors standing,
    /// a 7B edits one of them badly over and over. Once the report has come back
    /// and one of its errors is recognisably the old one, the loop shows ONLY
    /// that one — which it could not do when nothing ever registered as a repeat.
    #[test]
    fn a_rewritten_repeat_is_the_only_error_shown_again() {
        let (temp, _) = crate_with_source(&budget_crate_lines());
        let root = temp.path();
        let round1 = super::error_signatures(TWO_FIELD_ERRORS, Some(root));
        // The model leaves `create_budget` alone and rewrites the `update_cost`
        // field into a new expression; the report that follows carries that
        // rewrite plus a complaint from a function it has never been shown.
        let focus = super::recurring_error_focus(
            REWRITTEN_AND_NEW,
            std::slice::from_ref(&round1),
            Some(root),
        )
        .expect("one error is provably the old one, so the other can wait");
        assert!(
            focus.contains("user_record: &result_record"),
            "the error that did not move: {focus}"
        );
        assert!(
            !focus.contains("self.totals[user]"),
            "and not the new one it has never been shown: {focus}"
        );
        assert!(is_rustc_diagnostic_report(&focus), "{focus}");
        assert_eq!(
            super::recurring_error_focus(REWRITTEN_AND_NEW, std::slice::from_ref(&round1), None),
            None,
            "without the item key nothing repeats, so the full report is shown again"
        );
    }

    /// The same pair of rounds through the arming path the loop actually takes,
    /// with the history seeded by the first call rather than by hand. Only the
    /// item key proves the repeat here, so if `arm_compile_repair` ever stops
    /// handing the workspace down, this second turn silently gets the whole
    /// report back — which is the `lr7b_v10` behaviour, and it cost 14 turns.
    #[test]
    fn a_rearmed_repair_narrows_on_the_item_key_alone() {
        let (tx, _rx) = crossbeam_channel::unbounded::<AgentToUiMessage>();
        let (temp, _) = crate_with_source(&budget_crate_lines());
        let mut attempts = 0u32;
        let mut pending = false;
        let mut armed = None;
        let mut target = None;
        let mut seen: Vec<Vec<String>> = Vec::new();
        let mut messages: Vec<ChatMessage> = Vec::new();
        assert!(arm_compile_repair(
            TWO_FIELD_ERRORS,
            temp.path(),
            Some("src/budget_manager.rs"),
            &mut attempts,
            MAX_COMPILE_REPAIR_TURNS,
            &mut pending,
            &mut armed,
            &mut target,
            &mut seen,
            &mut messages,
            &tx,
        ));
        let first_directive = armed.take().expect("the first repair is armed");
        assert!(
            !first_directive.starts_with("Fix ONLY the error below"),
            "a first encounter shows the model everything: {first_directive}"
        );

        assert!(arm_compile_repair(
            REWRITTEN_AND_NEW,
            temp.path(),
            Some("src/budget_manager.rs"),
            &mut attempts,
            MAX_COMPILE_REPAIR_TURNS,
            &mut pending,
            &mut armed,
            &mut target,
            &mut seen,
            &mut messages,
            &tx,
        ));
        let directive = armed.expect("the second repair is armed");
        assert!(
            directive.starts_with("Fix ONLY the error below"),
            "the rewritten line still reads as the error in `fn update_cost`: \
             {directive}"
        );
        assert!(
            directive.contains("user_record: &result_record,"),
            "so this turn is that one error: {directive}"
        );
        assert!(
            !directive.contains("no field `totals`"),
            "and the brand-new complaint waits a turn: {directive}"
        );
    }

    #[test]
    fn a_repeated_error_is_named_in_the_directive_as_a_loop_not_new_work() {
        let e0053 = error_signatures(
            "error[E0053]: incompatible type for trait\n--> src\\m.rs:387:13\n\
             387 | &mut self,\n",
        );
        let e0594 = error_signatures(
            "error[E0594]: cannot assign through `&self`\n--> src\\m.rs:391:13\n\
             391 | self.data = user_dict.clone();\n",
        );
        let history = vec![e0053.clone(), e0594.clone()];
        let note = recurring_error_note(
            "error[E0053]: incompatible type for trait\n--> src\\m.rs:395:13\n395 | &mut self,\n",
            &history,
        );
        assert!(note.contains("re-reporting"), "{note}");
        assert!(
            note.contains("E0053") && note.contains("&mut self,"),
            "name the error the model keeps producing: {note}"
        );
        assert!(
            note.contains("loop, not progress"),
            "and say what the pattern is: {note}"
        );
        // The pair this run actually produced is the mutability contradiction,
        // which has exactly one resolution — and the directive has to name it or
        // the next flip is the only move the model can see.
        assert!(
            note.contains("RefCell") && note.contains("each flip just moves the error"),
            "{note}"
        );

        // A first sighting gets no lecture.
        assert_eq!(
            recurring_error_note(
                "error[E0053]: incompatible type for trait\n--> src\\m.rs:387:13\n\
                 387 | &mut self,\n",
                &[],
            ),
            ""
        );
        // And an unrelated new error is not framed as a repeat.
        let unrelated = recurring_error_note(
            "error[E0308]: mismatched types\n--> src\\m.rs:12:5\n12 | let s: String = 1u8;\n",
            &history,
        );
        assert!(unrelated.is_empty(), "{unrelated}");
    }

    /// gateprobe5's model did not re-report `E0053` — it answered the trait
    /// complaint by flipping `&mut self` to `&self`, which DELETED that
    /// signature and created `E0594` one line over. Nothing repeated, so exact
    /// repeat detection sees a clean new error, while the model is doing
    /// precisely the thing it should not do again. The pair has to be readable
    /// across rounds, not only within one.
    #[test]
    fn an_error_traded_for_its_mutability_opposite_is_named_even_without_repeating() {
        let e0053 = error_signatures(
            "error[E0053]: incompatible type for trait\n--> src\\m.rs:387:13\n387 | &mut self,\n",
        );
        let note = recurring_error_note(
            "error[E0594]: cannot assign to `self.data`, which is behind a `&` reference\n\
             --> src\\m.rs:391:13\n391 | self.data = user_dict.clone();\n",
            std::slice::from_ref(&e0053),
        );
        assert!(
            note.contains("RefCell") && note.contains("each flip just moves the error"),
            "the resolution the model cannot see from either error alone: {note}"
        );
        assert!(
            !note.contains("re-reporting"),
            "nothing repeated, so the note must not claim it did: {note}"
        );

        // The pair must be LIVE, not merely historical. An unrelated type error
        // after an old borrow fight is new work, and a lecture there spends
        // excerpt budget to say nothing the model can act on.
        assert_eq!(
            recurring_error_note(
                "error[E0308]: mismatched types\n--> src\\m.rs:12:5\n12 | let s: String = 1u8;\n",
                &[e0053],
            ),
            ""
        );
    }

    /// The second face of the same mistake, measured on the 8 GB probe: `x` into
    /// a `&'a T` field is `E0308`, `&x` is `E0515`, and the model alternated
    /// between them for its last two repairs. Neither edit is wrong in isolation,
    /// so the note has to name what is missing — an owner, not an operator.
    #[test]
    fn an_e0308_e0515_pendulum_is_told_the_value_needs_an_owner() {
        let e0308 = error_signatures(
            "error[E0308]: mismatched types\n--> src\\budget_manager.rs:358:26\n\
             358 | user_record: result_record,\n",
        );
        let current = "error[E0515]: cannot return value referencing local variable \
             `result_record`\n--> src\\budget_manager.rs:358:26\n358 | user_record: \
             &result_record,\n";
        let note = recurring_error_note(current, std::slice::from_ref(&e0308));
        assert!(
            note.contains("Move the VALUE into something that outlives the call"),
            "the owner is the requirement neither error states: {note}"
        );
        assert!(
            note.contains("get_mut") || note.contains("get(&k)"),
            "and hand it the call that does it: {note}"
        );
        assert!(
            !note.contains("re-reporting"),
            "the signature changed, so no repeat is claimed: {note}"
        );

        // Both faces in ONE round is the same contradiction, seen at once.
        let both = format!(
            "error[E0308]: mismatched types\n--> src\\b.rs:358:26\n358 | user_record: \
             result_record,\n{}",
            current
        );
        let same_round = recurring_error_note(&both, std::slice::from_ref(&e0308));
        assert!(
            same_round.contains("Stop editing that expression"),
            "{same_round}"
        );

        // And a type error with no borrow partner stays quiet, as before.
        assert_eq!(
            recurring_error_note(
                "error[E0308]: mismatched types\n--> src\\m.rs:12:5\n12 | let s: String = 1u8;\n",
                &[e0308],
            ),
            "",
            "E0308 against E0308 is not a pendulum"
        );
    }

    /// `lr7b_v9` was handed a complete, correct directive containing two errors
    /// and burned four `edit_file` calls — plus all three repair attempts — on the
    /// easier one, never once editing the line the other error named. A repeat is
    /// therefore answered by taking the choice away: show only what did not move.
    #[test]
    fn a_repeated_report_narrows_to_the_error_the_model_did_not_move() {
        let e0308 = "error[E0308]: mismatched types\n   --> src\\budget_manager.rs:358:26\n    \
             |\n358 |             user_record: result_record,\n    |                          \
             ^^^^^^^^^^^^^ expected `&PersistedUserRecord`, found `PersistedUserRecord`\n\n";
        // The fixture's other half: `create_budget` returns `&mut T` while
        // `values_mut().next()` is `Option<&mut T>`.
        let e0308_other = "error[E0308]: mismatched types\n   --> src\\budget_manager.rs:287:9\n  \
             |\n287 |         self.user_dict.values_mut().next()\n    |\n\n";
        // What the model wrote in its place — the exact shape the E0515 recipe
        // forbids — which is a NEW error, not a repeat of the one it replaced.
        let e0515 = "error[E0515]: cannot return value referencing temporary value\n   --> \
             src\\budget_manager.rs:287:9\n    |\n287 | /          self.user_dict\n    | \
             ||_____________-^ returns a value referencing data owned by the current function\n\n";
        let first = format!("{e0308}{e0308_other}");
        let first_round = error_signatures(&first);
        assert_eq!(
            first_round.len(),
            2,
            "the fixture's two sites must both be signature-able"
        );

        // Round two, exactly as v9 reported it: one error answered badly, one left
        // alone. Narrowing keeps the ignored one and withholds the fresh one.
        let second = format!("{e0308}{e0515}");
        let focus = recurring_error_focus(&second, std::slice::from_ref(&first_round))
            .expect("one of these two errors was already shown");
        assert!(
            focus.contains("user_record: result_record"),
            "the ignored error must stay on screen:\n{focus}"
        );
        assert!(
            !focus.contains("self.user_dict"),
            "the error the model just edited is the one to withhold:\n{focus}"
        );

        // A first encounter shows everything: narrowing to nothing would hide the
        // second error before the model ever had a chance to fix both.
        assert_eq!(
            recurring_error_focus(&first, &[]),
            None,
            "no history, no narrowing"
        );
        // Nor when every error repeats — there is nothing to lose and the model
        // still has both to fix.
        let both_round = error_signatures(&second);
        assert_eq!(
            recurring_error_focus(&second, std::slice::from_ref(&both_round)),
            None,
            "all-repeat is not a choice worth removing"
        );
        // One error only: nothing to focus.
        assert_eq!(
            recurring_error_focus(e0308, std::slice::from_ref(&first_round)),
            None
        );

        // The narrowed text must still read as a rustc report, or the directive
        // loses its diagnostics-are-real framing, and the verbatim boxes it builds
        // must shrink with it: a withheld error that still gets its own box is a
        // second job the model is silently handed back.
        assert!(is_rustc_diagnostic_report(&focus), "{focus}");
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        let mut lines = vec!["// filler".to_string(); 400];
        lines[286] = "        self.user_dict.values_mut().next()".to_string();
        lines[357] = "            user_record: result_record,".to_string();
        std::fs::write(
            temp.path().join("src").join("budget_manager.rs"),
            lines.join("\n"),
        )
        .unwrap();
        let both_boxes = repair_error_excerpt(temp.path(), &second);
        assert!(
            both_boxes.contains("values_mut().next()")
                && both_boxes.contains("user_record: result_record,"),
            "the full report gets a box per site:\n{both_boxes}"
        );
        let one_box = repair_error_excerpt(temp.path(), &focus);
        assert!(
            one_box.contains("user_record: result_record,"),
            "the kept site keeps its box:\n{one_box}"
        );
        assert!(
            !one_box.contains("values_mut().next()"),
            "the withheld site must not come back through the excerpt:\n{one_box}"
        );
    }

    /// The narrowed report has to travel with an instruction, not just a shorter
    /// error list: a model that sees one error where it knows there were two reads
    /// the missing one as already fixed and stops.
    #[test]
    fn a_narrowed_report_tells_the_model_the_other_error_is_still_there() {
        let e0308 = "error[E0308]: mismatched types\n--> src\\a.rs:3:1\n3 | let x: u8 = 1i32;\n";
        let e0515 = "error[E0515]: cannot return value referencing temporary value\n--> \
             src\\a.rs:9:1\n9 | &local\n";
        let both = format!("{e0308}\n{e0515}");
        let sig = error_signatures(e0308);
        let focus = recurring_error_focus(&both, std::slice::from_ref(&sig))
            .expect("E0308 was already shown");
        assert!(
            focus.contains("let x: u8 = 1i32;") && !focus.contains("&local"),
            "{focus}"
        );
        assert!(
            ONE_ERROR_AT_A_TIME_NOTE.contains("Fix ONLY the error below")
                && ONE_ERROR_AT_A_TIME_NOTE.contains("will be shown to you again"),
            "the withheld half has to be promised, not silently dropped"
        );
    }

    /// The route, not the helper: `lr7b_v9`'s second repair turn replayed through
    /// `arm_compile_repair`, which is the only place a directive is built for the
    /// model. A narrowing the armed turn does not apply is a narrowing nobody sees.
    #[test]
    fn an_armed_repair_shows_only_the_error_the_last_repair_left_alone() {
        let (tx, _rx) = crossbeam_channel::unbounded::<AgentToUiMessage>();
        let temp = tempfile::TempDir::new().unwrap();
        let e0308 = "error[E0308]: mismatched types\n   --> src\\budget_manager.rs:358:26\n358 | \
             user_record: result_record,\n";
        let e0308_other =
            "error[E0308]: mismatched types\n   --> src\\budget_manager.rs:287:9\n287 \
             |         self.user_dict.values_mut().next()\n";
        let e0515 = "error[E0515]: cannot return value referencing temporary value\n   --> \
             src\\budget_manager.rs:287:9\n287 | /          self.user_dict\n";
        let mut seen = vec![error_signatures(&format!("{e0308}{e0308_other}"))];
        let mut attempts = 0u32;
        let mut pending = false;
        let mut armed = None;
        let mut target = None;
        let mut messages = Vec::new();
        assert!(arm_compile_repair(
            &format!("{e0308}{e0515}"),
            temp.path(),
            Some("rust_port/src/budget_manager.rs"),
            &mut attempts,
            MAX_COMPILE_REPAIR_TURNS,
            &mut pending,
            &mut armed,
            &mut target,
            &mut seen,
            &mut messages,
            &tx,
        ));
        let directive = armed.expect("the repair is armed");
        assert!(
            directive.contains("user_record: result_record,"),
            "the ignored error is the one this turn is for:\n{directive}"
        );
        assert!(
            !directive.contains("self.user_dict"),
            "the error the last repair did chase must be out of the turn:\n{directive}"
        );
        assert!(
            directive.contains("Fix ONLY the error below"),
            "and the turn says so:\n{directive}"
        );
        assert!(
            directive.starts_with("Fix ONLY the error below"),
            "the instruction leads the message — v9's advice trailed 10 KB of it: \
             {directive}"
        );
        assert_eq!(
            messages[0].content, directive,
            "the history copy is what a rebuilt context falls back to"
        );
    }

    /// A repair turn rebuilds its context from scratch — the only shape this MoE
    /// reliably acts on — and that rebuild keeps just the system prompt, the task
    /// and the last two tool exchanges. The directive an armed repair pushed into
    /// history is none of those, so it was deleted before the model ever read it:
    /// the turn told to "fix these errors" lost the sentence explaining that its
    /// two errors are one requirement. The advice therefore travels with the armed
    /// directive, and the rebuilt turn is what this test reads.
    #[test]
    fn a_rebuilt_repair_turn_keeps_the_mutability_flip_advice() {
        let (tx, _rx) = crossbeam_channel::unbounded::<AgentToUiMessage>();
        let temp = tempfile::TempDir::new().unwrap();
        let e0053 = "error[E0053]: method `save_data` has an incompatible type for trait\n\
             --> src\\budget_manager.rs:387:13\n387 |             &mut self,\n";
        let e0594 = "error[E0594]: cannot assign to `self.data` through `&self`\n\
             --> src\\budget_manager.rs:391:13\n391 |             self.data = user_dict.clone();\n";

        let mut seen = vec![error_signatures(e0053)];
        let mut attempts = 0u32;
        let mut pending = false;
        let mut armed = None;
        let mut target = None;
        let mut messages = Vec::new();
        assert!(arm_compile_repair(
            e0594,
            temp.path(),
            Some("rust_port/src/budget_manager.rs"),
            &mut attempts,
            MAX_COMPILE_REPAIR_TURNS,
            &mut pending,
            &mut armed,
            &mut target,
            &mut seen,
            &mut messages,
            &tx,
        ));

        // What the model actually receives next turn: the rebuilt write context.
        let rebuilt = write_context_directive(armed.take(), None, &FILE_EDITING_TOOLS, temp.path());
        assert!(
            rebuilt.contains("RefCell") && rebuilt.contains("each flip just moves the error"),
            "the rebuilt turn lost the one resolution the model cannot see \
             from either error alone: {rebuilt}"
        );
        assert!(
            rebuilt.contains("error[E0594]"),
            "and it still carries the diagnostics verbatim: {rebuilt}"
        );
        // One builder, both routes: the history copy says the same thing.
        assert!(
            messages[0].content.contains("RefCell"),
            "{}",
            messages[0].content
        );

        // The fallbacks stay in order: a standing report with no directive armed
        // is still better than a blind write order, and nothing standing means the
        // plain write order — without any borrow advice bolted on.
        let standing = write_context_directive(None, Some(e0594), &FILE_EDITING_TOOLS, temp.path());
        assert!(standing.contains("error[E0594]"), "{standing}");
        let plain = write_context_directive(None, None, &["write_file"], temp.path());
        assert!(
            plain.contains("STOP exploring") && !plain.contains("RefCell"),
            "{plain}"
        );
    }

    /// A name the scanner invents from prose becomes an item the floor demands
    /// back, so only real declarations may be read out of the text.
    #[test]
    fn rust_item_names_reads_declarations_not_prose() {
        let src = "pub fn total(&self) -> f64 {}\n\
                   pub(crate) fn hidden(&self) {}\n\
                   struct Snapshot { pub saved: bool }\n\
                   trait Persist {}\n\
                   mod pendulum_fixture {}\n\
                   type Alias = u32;\n\
                   unsafe fn raw() {}\n\
                   // fn commented_out() {}\n\
                   let s = \"fn in_a_string()\";\n";
        let names = rust_item_names(src);
        for expected in [
            "total",
            "hidden",
            "Snapshot",
            "Persist",
            "pendulum_fixture",
            "Alias",
            "raw",
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "`{expected}` is a declaration and must be in {names:?}"
            );
        }
        for noise in ["commented_out", "in_a_string", "s"] {
            assert!(
                !names.iter().any(|n| n == noise),
                "`{noise}` is not a declaration but appeared in {names:?}"
            );
        }
    }

    /// The surface `lr7b_v13`'s deliverable arrived with, cut to four items so the
    /// assertions stay readable.
    const SURFACE_SRC: &str = "pub fn token_count(t: &str) -> usize { t.len() }\n\
                               pub fn cost_per_token(m: &str) -> f64 { 0.0 }\n\
                               pub struct CostCalculator { pub rate: f64 }\n\
                               pub trait Budget { fn spend(&mut self, n: u64); }\n";

    /// What turn 5 of that run sent instead: one stub, over a file it had never
    /// been allowed to read.
    const STUB_REWRITE: &str = "fn calculate_cost(&self) {\n    unimplemented!()\n}\n";

    /// A full rewrite that still declares most of the surface.
    const FAITHFUL_REWRITE: &str = "pub fn token_count(t: &str) -> usize { t.chars().count() }\n\
                                    pub fn cost_per_token(m: &str) -> f64 { 0.1 }\n\
                                    pub struct CostCalculator { pub rate: f64 }\n";

    #[test]
    fn a_whole_file_write_that_guts_the_deliverable_is_refused_before_it_lands() {
        let surface = rust_item_names(SURFACE_SRC);
        assert_eq!(surface.len(), 4, "{surface:?}");

        let reason = gutting_rewrite_refusal(
            "write_file",
            &json!({
                "relativeFilePath": "rust_port/src/budget_manager.rs",
                "content": STUB_REWRITE
            }),
            Some("rust_port/src/budget_manager.rs"),
            &surface,
            Some("rust_port/src/budget_manager.rs"),
        )
        .expect("a one-function stub over a four-item port is the write that lost v13");

        // Classified as a refusal by the same test the loop charges `any_error`
        // with, so it is refunded and replay-counted like any other. Losing the
        // prefix would silently make it a success the loop credits.
        assert!(is_refusal(&reason), "{reason}");
        // Everything it would have removed is named, so the model sees the loss.
        for gone in ["token_count", "cost_per_token", "CostCalculator", "Budget"] {
            assert!(
                reason.contains(&format!("`{gone}`")),
                "missing `{gone}`: {reason}"
            );
        }
        // It has to say the file survived, and offer the call that changes one
        // line without moving the rest — the surface guard's "put those items
        // back" has no such exit once the bytes are gone.
        assert!(reason.contains("NOT written"), "{reason}");
        assert!(reason.contains("`edit_file`"), "{reason}");
        // And it has to say where the replacement text comes from, or the list of
        // names above is what gets sent back as the edit payload — v15's 54
        // identical `oldString`/`newString` pairs.
        assert!(reason.contains("not the text to send"), "{reason}");
        assert!(
            reason.contains("relativeFilePath"),
            "the read has to be spelled out as a call: {reason}"
        );
    }

    #[test]
    fn a_rewrite_that_keeps_the_deliverables_items_is_not_refused() {
        // Non-vacuity for the test above: the refusal comes from the missing
        // items, not from `write_file` touching the deliverable. One name of four
        // gone is a refactor, which the post-hoc guard also waves through.
        let surface = rust_item_names(SURFACE_SRC);
        assert_eq!(
            gutting_rewrite_refusal(
                "write_file",
                &json!({
                    "relativeFilePath": "rust_port/src/budget_manager.rs",
                    "content": FAITHFUL_REWRITE
                }),
                Some("rust_port/src/budget_manager.rs"),
                &surface,
                Some("rust_port/src/budget_manager.rs"),
            ),
            None
        );
    }

    #[test]
    fn the_gutting_write_gate_only_watches_the_deliverable() {
        let surface = rust_item_names(SURFACE_SRC);

        // Another file in the same crate may be written from scratch: the surface
        // records one file, not a floor for the workspace.
        assert_eq!(
            gutting_rewrite_refusal(
                "write_file",
                &json!({"relativeFilePath": "rust_port/src/lib.rs", "content": STUB_REWRITE}),
                Some("rust_port/src/budget_manager.rs"),
                &surface,
                Some("rust_port/src/budget_manager.rs"),
            ),
            None
        );
        // Only `write_file` replaces text wholesale. An `edit_file` deletes text
        // the model was handed, and the grade after it still catches the loss.
        assert_eq!(
            gutting_rewrite_refusal(
                "edit_file",
                &json!({
                    "relativeFilePath": "rust_port/src/budget_manager.rs",
                    "content": STUB_REWRITE
                }),
                Some("rust_port/src/budget_manager.rs"),
                &surface,
                Some("rust_port/src/budget_manager.rs"),
            ),
            None
        );
        // A run with no recorded surface — a new artifact written for the first
        // time, or a deliverable that is not Rust — has nothing to protect.
        assert_eq!(
            gutting_rewrite_refusal(
                "write_file",
                &json!({
                    "relativeFilePath": "rust_port/src/budget_manager.rs",
                    "content": STUB_REWRITE
                }),
                Some("rust_port/src/budget_manager.rs"),
                &[],
                Some("rust_port/src/budget_manager.rs"),
            ),
            None
        );
        // Names with no file to compare them against cannot be aimed at anything,
        // so the gate stays inert instead of watching every write in the run.
        assert_eq!(
            gutting_rewrite_refusal(
                "write_file",
                &json!({
                    "relativeFilePath": "rust_port/src/budget_manager.rs",
                    "content": STUB_REWRITE
                }),
                None,
                &surface,
                Some("rust_port/src/budget_manager.rs"),
            ),
            None
        );
    }

    #[test]
    fn a_backslash_path_is_still_the_deliverable() {
        // The task names the target with forward slashes and a Windows checkout
        // answers with backslashes. Comparing the strings raw would let every
        // gutting write through on the host this was measured on.
        let surface = rust_item_names(SURFACE_SRC);
        assert!(gutting_rewrite_refusal(
            "write_file",
            &json!({
                "relativeFilePath": "rust_port\\src\\budget_manager.rs",
                "content": STUB_REWRITE
            }),
            Some("rust_port/src/budget_manager.rs"),
            &surface,
            Some("rust_port/src/budget_manager.rs"),
        )
        .is_some());
    }

    #[test]
    fn the_read_the_gutting_refusal_offers_is_the_read_the_lock_honours() {
        let surface = rust_item_names(SURFACE_SRC);
        let target = "rust_port/src/budget_manager.rs";

        let reason = gutting_rewrite_refusal(
            "write_file",
            &json!({"relativeFilePath": target, "content": STUB_REWRITE}),
            Some(target),
            &surface,
            Some(target),
        )
        .expect("the stub rewrite is refused");
        let offered = format!("\"{target}\"");
        assert!(reason.contains(&offered), "{reason}");

        // The promise has to be kept by the gate that enforces the lock, not just
        // printed. A refusal that names a read the lock would refuse sends the
        // model straight into a second refusal.
        let read = json!({"relativeFilePath": target, "startLine": 1, "endLine": 40});
        assert!(
            read_of_named_target("read_file", &read, Some(target)),
            "the refusal offered a read the lock refuses"
        );

        // With nothing named by the task the lock stays whole, so the refusal must
        // not offer a path through it.
        let no_read = gutting_rewrite_refusal(
            "write_file",
            &json!({"relativeFilePath": target, "content": STUB_REWRITE}),
            Some(target),
            &surface,
            None,
        )
        .expect("still refused without a named target");
        assert!(!no_read.contains("not the text to send"), "{no_read}");
        assert!(!read_of_named_target("read_file", &read, None));
    }

    #[test]
    fn half_the_surface_is_where_a_rewrite_becomes_a_deletion() {
        assert!(!should_refuse_gutting_rewrite(0, 4));
        assert!(!should_refuse_gutting_rewrite(1, 4));
        assert!(should_refuse_gutting_rewrite(2, 4));
        assert!(should_refuse_gutting_rewrite(4, 4));
        // `baseline == 0` means "no surface recorded", not "nothing is missing".
        assert!(!should_refuse_gutting_rewrite(0, 0));

        assert!(items_a_rewrite_would_delete(&[], STUB_REWRITE).is_empty());
        assert!(
            items_a_rewrite_would_delete(&rust_item_names(SURFACE_SRC), SURFACE_SRC).is_empty()
        );
        assert_eq!(
            items_a_rewrite_would_delete(&rust_item_names(SURFACE_SRC), FAITHFUL_REWRITE),
            vec!["Budget".to_string()]
        );
    }

    #[test]
    fn a_locked_turn_may_still_read_the_file_it_must_edit() {
        let target = "rust_port/src/budget_manager.rs";
        // lr7b_v14 turn 3: the read the dispatch text itself asked for, refused by
        // the lock, after which every payload the model sent was invented.
        assert!(read_of_named_target(
            "read_file",
            &json!({"relativeFilePath": target}),
            Some(target)
        ));
        // A region read is the same read, and the one that fits an output budget.
        assert!(read_of_named_target(
            "read_file",
            &json!({"relativeFilePath": target, "startLine": 340, "endLine": 370}),
            Some(target)
        ));
        // Backslashes name the same file on the checkout this was measured on.
        assert!(read_of_named_target(
            "read_file",
            &json!({"relativeFilePath": "rust_port\\src\\budget_manager.rs"}),
            Some(target)
        ));
    }

    #[test]
    fn the_lock_still_closes_every_other_read() {
        let target = "rust_port/src/budget_manager.rs";
        // Wandering is what the lock exists to stop, so the exception is one path
        // wide rather than "reads are allowed now".
        assert!(!read_of_named_target(
            "read_file",
            &json!({"relativeFilePath": "rust_port/src/lib.rs"}),
            Some(target)
        ));
        // No named target means nothing to aim a read at.
        assert!(!read_of_named_target(
            "read_file",
            &json!({"relativeFilePath": target}),
            None
        ));
        // No other tool rides through on it. write_file and edit_file are offered
        // by the lock's own tool list, not by this exception.
        for tool in ["list_dir", "grep_search", "glob", "write_file", "edit_file"] {
            assert!(
                !read_of_named_target(tool, &json!({"relativeFilePath": target}), Some(target)),
                "`{tool}` is not a read of the deliverable"
            );
        }
        // And a read that names no path is not a read of the deliverable.
        assert!(!read_of_named_target("read_file", &json!({}), Some(target)));
    }

    #[test]
    fn a_clean_grade_is_withheld_when_the_write_deleted_the_error_site() {
        // pendulum1: an `impl ... for Snapshot` that contradicted its own trait
        // was "fixed" by deleting the module, and the run reported success over a
        // file 440 bytes smaller with 70 green tests. rustc and cargo both go
        // quiet for a deleted file, so the item names are the only evidence that
        // the code the error was reported in is gone.
        let temp = tempfile::TempDir::new().unwrap();
        let rel = "rust_port/src/budget_manager.rs";
        let path = temp.path().join(rel.replace('/', "\\"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let with_fixture = "pub fn spend(&mut self) {}\n\
                            mod pendulum_fixture {\n\
                            pub struct Snapshot { pub saved: bool }\n\
                            pub trait Persist { fn flush(&mut self) -> Result<(), String>; }\n\
                            impl Persist for Snapshot {\n\
                            fn flush(&mut self) -> Result<(), String> { Ok(()) }\n\
                            }\n\
                            }\n";
        std::fs::write(&path, with_fixture).unwrap();
        let mut surface = ArtifactSurface {
            target: Some(rel.to_string()),
            names: rust_item_names(with_fixture),
            refusals: 0,
        };
        // Nothing has been touched yet, so a clean grade stands — and standing
        // still must not spend the bounded refusals.
        assert!(
            surface.withheld_report(temp.path()).is_none(),
            "an untouched artifact is not a deletion"
        );
        assert_eq!(surface.refusals, 0);

        // The measured shape: the module is gone and what remains compiles.
        std::fs::write(&path, "pub fn spend(&mut self) {}\n").unwrap();
        let report = surface
            .withheld_report(temp.path())
            .expect("deleting the items the run started with must not be accepted as a fix");
        for gone in ["pendulum_fixture", "Snapshot", "Persist", "flush"] {
            assert!(
                report.contains(&format!("`{gone}`")),
                "the report must name what vanished, missing `{gone}`: {report}"
            );
        }
        assert!(
            report.contains("DELETED") && report.contains(rel),
            "{report}"
        );
        // surfaceguard1: a task that genuinely orders a deletion has no way to
        // say so, so it burned two turns answering with no tool call and then a
        // repair spiral. The directive must offer that one-line exit.
        assert!(
            report.contains("reply with one line saying the task asked for the removal"),
            "a removal the task asked for must be answerable: {report}"
        );
        // and the status line must call it what it is — the crate built.
        assert_eq!(gate_failure_label(&report), "The artifact shrank");
        assert_eq!(surface.refusals, 1);

        // The floor is bounded: a run that keeps a clean, building crate must not
        // be locked into repairs forever by a refactor this guard cannot read.
        assert!(surface.withheld_report(temp.path()).is_some());
        assert_eq!(surface.refusals, MAX_SURFACE_REFUSALS);
        // The bound stops the interrupts, not the disclosure: a run that ends
        // green over a smaller file has to say which items it no longer has.
        let note = surface.shrink_note(temp.path());
        assert!(
            note.contains("WARNING") && note.contains("`pendulum_fixture`") && note.contains(rel),
            "the success line must name the shrink: {note}"
        );
        assert!(
            surface.withheld_report(temp.path()).is_none(),
            "the third clean grade has to stand, or the guard deadlocks the run"
        );

        // A dispatch that names no existing file has nothing to hold a grade to.
        assert!(ArtifactSurface::default()
            .withheld_report(temp.path())
            .is_none());
        assert_eq!(ArtifactSurface::default().shrink_note(temp.path()), "");
    }

    #[test]
    fn arming_a_repair_sets_every_state_the_next_turn_reads() {
        // The armed flags, the target and the directive are consumed in three
        // different places, so one of them drifting out of sync silently turns a
        // forced repair back into the prose answer it replaced.
        let (tx, rx) = crossbeam_channel::unbounded::<AgentToUiMessage>();
        let temp = tempfile::TempDir::new().unwrap();
        let errors = "error[E0594]: cannot assign to `self.data`";
        let mut attempts = 0u32;
        let mut pending = false;
        let mut held: Option<String> = None;
        let mut target: Option<String> = None;
        let mut seen: Vec<Vec<String>> = Vec::new();
        let mut history: Vec<ChatMessage> = vec![ChatMessage {
            role: "user".to_string(),
            content: "port the module".to_string(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }];

        assert!(arm_compile_repair(
            errors,
            temp.path(),
            Some("rust_port/src/budget_manager.rs"),
            &mut attempts,
            MAX_COMPILE_REPAIR_TURNS,
            &mut pending,
            &mut held,
            &mut target,
            &mut seen,
            &mut history,
            &tx,
        ));
        assert_eq!(attempts, 1, "one real model call per arm");
        assert!(pending, "the next turn must be forced write-only");
        assert!(
            held.as_deref()
                .is_some_and(|d| d.contains(errors) && d.contains("edit_file")),
            "the armed state must be the finished directive, not a note to \
             rebuild one from: {held:?}"
        );
        assert_eq!(
            target.as_deref(),
            Some("rust_port/src/budget_manager.rs"),
            "a repair may only rewrite the file that broke"
        );
        let last = history.last().expect("directive appended");
        assert_eq!(last.role, "user");
        assert!(
            last.content.contains(errors),
            "the directive must carry the diagnostics verbatim"
        );
        assert!(last.content.contains("edit_file"), "{:?}", last.content);
        assert_eq!(history.len(), 2, "arming must not rewrite history");
        assert!(
            !seen.is_empty(),
            "arming must record what it was told to fix"
        );
        assert!(
            !last.content.contains("re-reporting"),
            "a first repair must not be accused of looping:\n{}",
            last.content
        );

        // A spent budget returns false and changes nothing at all: the caller
        // has to be able to fall through to its honest failure.
        let mut spent = MAX_COMPILE_REPAIR_TURNS;
        let before = history.len();
        let seen_before = seen.clone();
        assert!(
            !arm_compile_repair(
                errors,
                temp.path(),
                Some("a.rs"),
                &mut spent,
                MAX_COMPILE_REPAIR_TURNS,
                &mut pending,
                &mut held,
                &mut target,
                &mut seen,
                &mut history,
                &tx,
            ),
            "no attempts left means no arm"
        );
        assert_eq!(spent, MAX_COMPILE_REPAIR_TURNS);
        assert_eq!(
            history.len(),
            before,
            "a refused arm must not touch history"
        );
        assert_eq!(seen, seen_before, "a refused arm records nothing");
        rx.try_recv()
            .expect("the first arm reported itself to the UI");
        assert!(rx.try_recv().is_err(), "a refused arm stays silent");
    }

    /// The attempts that got the library to compile are not the attempts that
    /// grade it. convert10's first attempt spent all three build repairs on
    /// `E0433`/`E0277`/a lifetime slip, its `cargo test` then reported 77 passed
    /// / 7 failed, and the shared counter silently refused the repair — the run
    /// ended with seven fixable assertions on disk and ~1400s of wall clock
    /// still unspent, because nothing could tell the model about them.
    #[test]
    fn a_spent_build_budget_leaves_the_test_gate_free_to_ask_for_help() {
        let (tx, rx) = crossbeam_channel::unbounded::<AgentToUiMessage>();
        let temp = tempfile::TempDir::new().unwrap();
        let report = format!(
            "{} Real `cargo test` output (test result: FAILED. 77 passed; 7 failed), one block per failing test:\n",
            crate::agent::executor::thread::TEST_FAILURE_PREAMBLE
        );

        // The two kinds of failure draw on two different depths.
        let (test_max, is_test) = repair_budget(&report);
        assert!(is_test, "a test report is not a build report");
        assert_eq!(test_max, MAX_TEST_REPAIR_TURNS);
        assert_eq!(
            repair_budget("error[E0433]: cannot find module `serde_json`").0,
            MAX_COMPILE_REPAIR_TURNS,
            "and a build report keeps its own budget"
        );

        let mut build_spent = MAX_COMPILE_REPAIR_TURNS;
        let mut test_attempts = 0u32;
        let mut pending = false;
        let mut held: Option<String> = None;
        let mut target: Option<String> = None;
        let mut seen: Vec<Vec<String>> = Vec::new();
        let mut history: Vec<ChatMessage> = Vec::new();
        assert!(
            arm_compile_repair(
                &report,
                temp.path(),
                Some("rust_port/src/budget_manager.rs"),
                &mut test_attempts,
                test_max,
                &mut pending,
                &mut held,
                &mut target,
                &mut seen,
                &mut history,
                &tx,
            ),
            "a red assertion is a different job from a broken build"
        );
        assert_eq!(test_attempts, 1, "drawn from the test counter");
        assert_eq!(
            build_spent, MAX_COMPILE_REPAIR_TURNS,
            "the spent build budget stays spent"
        );
        assert!(pending, "and the next turn is a forced repair turn");
        let shown = match rx.try_recv() {
            Ok(AgentToUiMessage::StatusUpdate(s)) => s,
            other => panic!("the arm must report itself, got {other:?}"),
        };
        assert!(
            shown.contains("(1/2)"),
            "the model-facing count is the budget actually in play: {shown}"
        );

        // The other half of the asymmetry: a *build* repair is still refused
        // once its own three are gone. The split adds room for assertions, it
        // does not hand a run that cannot type-check another free attempt.
        let mut build_pending = false;
        assert!(
            !arm_compile_repair(
                "error[E0433]: cannot find module or crate `serde_json` in this scope",
                temp.path(),
                Some("a.rs"),
                &mut build_spent,
                MAX_COMPILE_REPAIR_TURNS,
                &mut build_pending,
                &mut held,
                &mut target,
                &mut seen,
                &mut history,
                &tx,
            ),
            "the spent build budget stays spent"
        );
        assert!(!build_pending, "a refused arm forces nothing");
        assert_eq!(
            test_attempts, 1,
            "and refusing a build repair does not touch the test counter"
        );
    }

    #[test]
    fn repair_excerpt_hands_the_model_the_error_lines_it_has_to_copy() {
        // The whole reason this exists: a repair turn sees no read_file, and a
        // model cannot reproduce the indentation of a file it wrote a dozen
        // turns ago. If it guesses, edit_file's uniqueness match refuses the
        // batch and the repair attempt is burned for nothing.
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(
            temp.path().join("src").join("lib.rs"),
            "fn main() {\n    let a = 1;\n        let b = a;\n    drop(a);\n    let c = b;\n}\n",
        )
        .unwrap();

        let excerpt = repair_error_excerpt(
            temp.path(),
            "error[E0382]: borrow of moved value: `a`\n  --> src/lib.rs:4:10",
        );
        assert!(
            excerpt.contains("    drop(a);"),
            "must carry the error line with its real indentation:\n{excerpt}"
        );
        assert!(
            excerpt.contains("let b = a;"),
            "context lines make the needle unique:\n{excerpt}"
        );
        assert!(
            excerpt.contains("src/lib.rs"),
            "the model needs the path it must pass:\n{excerpt}"
        );
        // A `4 | ` gutter invites the model to copy the number into oldString,
        // which matches nothing in the file.
        assert!(
            !excerpt.contains("4 | "),
            "no line-number gutter: {excerpt}"
        );
        assert!(
            compile_repair_directive("--> src/lib.rs:4:10", temp.path()).contains("drop(a);"),
            "the directive must actually embed the excerpt"
        );
    }

    #[test]
    fn repair_excerpt_reads_a_windows_location_and_shows_the_passable_path() {
        // Two Windows-only traps at once: rustc prints `--> src\budget.rs:20:1`
        // with a backslash, and the crate sits below the workspace root, so its
        // path is not the `relativeFilePath` edit_file expects. Getting either
        // wrong makes every patch on the platform refuse as "not found".
        let temp = tempfile::TempDir::new().unwrap();
        let crate_dir = temp.path().join("rust_port");
        std::fs::create_dir_all(crate_dir.join("src")).unwrap();
        std::fs::write(
            crate_dir.join("Cargo.toml"),
            "[package]\nname = \"p\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            crate_dir.join("src").join("budget.rs"),
            "// head\n/// doc\npub struct A;\n",
        )
        .unwrap();

        let errors = "error[E0753]: expected outer doc comment\n  --> src\\budget.rs:2:1";
        let excerpt = repair_error_excerpt(temp.path(), errors);
        assert!(
            excerpt.contains("rust_port/src/budget.rs"),
            "header must be workspace-relative so it is a valid \
             `relativeFilePath`:\n{excerpt}"
        );
        assert!(
            excerpt.contains("/// doc"),
            "the error line itself must be copyable:\n{excerpt}"
        );
    }

    #[test]
    #[ignore = "runs cargo check in the real LiteLLM crucible workspace"]
    fn repair_excerpt_carries_the_real_crucible_error_sites() {
        // The two halves only prove anything together: the diagnostics filter
        // has to keep Windows `--> src\file.rs` lines, and the excerpt has to
        // turn one into copyable text whose path the agent can actually pass to
        // edit_file. Run this against the live port the crucible is failing.
        let root = std::path::Path::new(
            r"C:\Users\visse\OneDrive\Documents\Velocity-IDE\.velocity_testing\litellm_ws",
        );
        let errors = match crate::agent::executor::thread::run_compilation_check(root) {
            Ok(()) => panic!("the crucible port is expected to fail cargo check"),
            Err(e) => e,
        };
        assert!(
            errors.contains("-->"),
            "locations were filtered out again:\n{errors}"
        );
        assert!(
            errors.contains("mutable borrow later used here"),
            "the span body carrying rustc's conclusion was filtered out again:\n{errors}"
        );
        let excerpt = repair_error_excerpt(root, &errors);
        assert!(!excerpt.is_empty(), "empty excerpt for:\n{errors}");
        assert!(
            excerpt.contains("rust_port/src/budget_manager.rs"),
            "header must be the workspace-relative path edit_file accepts:\n{excerpt}"
        );
        // E0502 addresses line 275 (`self._save_data();`) but the borrow it
        // conflicts with begins three statements earlier and the later use sits
        // after it. A repair that can only see the addressed line cannot fix a
        // conflict whose resolution lies at one of the other two.
        for site in ["let entry = self", "self._save_data();", "entry.clone()"] {
            assert!(
                excerpt.contains(site),
                "excerpt must cover every site rustc named, missing `{site}`:\n{excerpt}"
            );
        }
        println!("{excerpt}");
    }

    /// The same pairing against the live crucible port, which is the only way to
    /// see whether the declarations being pasted are the ones the probe is
    /// missing. It prints instead of asserting a model's next move, because the
    /// workspace under it is whatever the last probe left behind.
    #[test]
    #[ignore = "runs cargo check in the real LiteLLM crucible workspace"]
    fn the_real_red_port_gets_its_owning_function_and_declarations() {
        let root = std::path::Path::new(
            r"C:\Users\visse\OneDrive\Documents\Velocity-IDE\.velocity_testing\litellm_ws",
        );
        let errors = match crate::agent::executor::thread::run_compilation_check(root) {
            Ok(()) => panic!("the crucible port is expected to fail cargo check"),
            Err(e) => e,
        };
        let api = repair_api_context(root, &errors);
        assert!(!api.is_empty(), "nothing was found to paste for:\n{errors}");
        println!("{api}");
        println!("=== recipes ===");
        println!("{}", error_code_recipes(&errors));
        println!("=== codes ===");
        println!(
            "{}",
            compile_repair_directive(&errors, root)
                .lines()
                .filter(|l| l.contains(": ") && l.starts_with('`'))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    /// A borrow conflict names two lines: the immutable borrow and the later
    /// use. Showing only the first is what the real crucible E0502 did under
    /// the old per-location dedup, and the line the model has to change was the
    /// one that was missing.
    #[test]
    fn repair_excerpt_covers_both_halves_of_a_split_borrow_error() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(
            temp.path().join("src").join("lib.rs"),
            "fn f(&mut self) {\n    let a = 1;\n    let b = 2;\n    self.save();\n    \
             let c = 3;\n    self.use_b(b);\n}\n",
        )
        .unwrap();

        let errors = "error[E0502]: cannot borrow `*self` as immutable\n  \
             --> src/lib.rs:4:5\nhelp: immutable borrow occurs here\n  \
             --> src/lib.rs:6:5";
        let excerpt = repair_error_excerpt(temp.path(), errors);
        assert!(
            excerpt.contains("self.save();"),
            "first location:\n{excerpt}"
        );
        assert!(
            excerpt.contains("self.use_b(b);"),
            "second location must be in the same window, not dropped:\n{excerpt}"
        );
        assert_eq!(
            excerpt.matches("verbatim from disk").count(),
            1,
            "one widened block, not two overlapping ones:\n{excerpt}"
        );
    }

    /// The `lr7b` regression — the first probe on a model that fits 8 GB VRAM.
    /// rustc said `no method named `as_ref` found for struct
    /// `PersistedUserRecord`` and the model spent two of its three build repairs
    /// inventing a conversion, because the answer sat 12 lines earlier in the
    /// same file: the field it was filling is declared `pub user_record: &'a
    /// PersistedUserRecord`, so the fix is `&result_record`. A repair turn has no
    /// read tool, so the loop has to read the file for it.
    #[test]
    fn a_repair_turn_is_handed_the_declaration_that_answers_its_error() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(
            temp.path().join("src").join("lib.rs"),
            "pub struct PersistedUserRecord {\n    pub user_id: String,\n}\n\n\
             pub struct UpdateCostResult<'a> {\n    pub user_record: &'a PersistedUserRecord,\n}\n\n\
             pub fn merge_costs(\n    records: &[PersistedUserRecord],\n) -> \
             Option<UpdateCostResult<'_>> {\n    let result_record = PersistedUserRecord { \
             user_id: \"1\".to_string() };\n    UpdateCostResult { user_record: \
             result_record.as_ref() }\n}\n",
        )
        .unwrap();

        let errors = "error[E0599]: no method named `as_ref` found for struct \
             `PersistedUserRecord` in the current scope\n  --> src/lib.rs:13:24\n  \
             |\n13 |     UpdateCostResult { user_record: result_record.as_ref() }\n  \
             |                                            ^^^^^^^^^^^^^^^^^^^^^";
        let api = repair_api_context(temp.path(), errors);
        assert!(
            api.contains("pub user_record: &'a PersistedUserRecord,"),
            "the field that answers the conversion error must be pasted in:\n{api}"
        );
        // The error is at line 13; the contract it has to satisfy is the `fn`
        // head at line 9, and that head wraps over three lines.
        assert!(
            api.contains("pub fn merge_costs(") && api.contains("-> Option<UpdateCostResult"),
            "the enclosing signature must be folded onto one readable line:\n{api}"
        );
        // Both types the diagnostics name, nothing else.
        assert!(api.contains("declares UpdateCostResult"), "{api}");
        // And it has to reach the model, not just exist in the module: the
        // excerpt window is centred on line 13, so nothing else in the directive
        // can supply line 6.
        let directive = compile_repair_directive(errors, temp.path());
        assert!(
            directive.contains("Declarations from the same file")
                && directive.contains("pub user_record: &'a PersistedUserRecord,"),
            "the declaration block must be pasted into the repair directive:\n{directive}"
        );
        println!("{api}");
    }

    /// The shape `lr7b_v5` died on. rustc named only the FIELD's type
    /// (`expected `&PersistedUserRecord``) and pointed at line 8, whose text is
    /// `user_record: result_record,` — so neither the report nor the error line
    /// mentions `UpdateCostResult`, the struct whose declaration is the entire
    /// answer. The loop already pastes the enclosing function, and that head
    /// carries `-> UpdateCostResult<'_>`; the type it names has to be looked up
    /// too, or the model is left deciding between `x` and `&x` with only one of
    /// the two halves in front of it — which is the pendulum.
    #[test]
    fn the_return_type_in_the_pasted_head_gets_its_own_declaration() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(
            temp.path().join("src").join("lib.rs"),
            "pub struct PersistedUserRecord {\n    pub user_id: String,\n}\n\n\
             pub struct UpdateCostResult<'a> {\n    pub user_record: &'a PersistedUserRecord,\n}\n\n\
             pub fn update_cost(&mut self) -> UpdateCostResult<'_> {\n    let result_record = \
             record.clone();\n    UpdateCostResult {\n        user_record: result_record,\n    \
             }\n}\n",
        )
        .unwrap();

        let errors = "error[E0308]: mismatched types\n  --> src/lib.rs:10:22\n  \
             |\n10 |         user_record: result_record,\n  |                        \
             ^^^^^^^^^^^^^ expected `&PersistedUserRecord`, found `PersistedUserRecord`";
        let api = repair_api_context(temp.path(), errors);
        assert!(
            api.contains("-> UpdateCostResult<'_>"),
            "the head is pasted, so the loop can see the type there:\n{api}"
        );
        assert!(
            api.contains("declares UpdateCostResult")
                && api.contains("pub user_record: &'a PersistedUserRecord,"),
            "and must hand over what that type declares:\n{api}"
        );

        // A head that names nothing worth looking up costs nothing.
        let no_names = type_names_in_diagnostics("let x = 1;");
        assert!(no_names.is_empty(), "{no_names:?}");
    }

    /// `no method named X` is answered by the list of methods that DO exist. The
    /// bodies are withheld because they are the code the model already wrote;
    /// pasting them back costs window tokens and invites a whole-block rewrite.
    #[test]
    fn an_impl_api_block_lists_method_signatures_without_bodies() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(
            temp.path().join("src").join("lib.rs"),
            "impl BudgetManager {\n    pub fn load(&mut self, raw: &str) -> Result<(), \
             String> {\n        self.total = raw.len() as u32;\n        \
             self.recount();\n        Ok(())\n    }\n\n    fn recount(&mut self) {}\n}\n",
        )
        .unwrap();

        let errors = "error[E0599]: no method named `save` found for struct \
             `BudgetManager` in the current scope\n  --> src/lib.rs:1:6";
        let api = repair_api_context(temp.path(), errors);
        assert!(
            api.contains("pub fn load(&mut self, raw: &str) -> Result<(), String>"),
            "the method list must carry the signatures:\n{api}"
        );
        assert!(api.contains("fn recount(&mut self) {}"), "{api}");
        assert!(
            !api.contains("self.total = raw.len()"),
            "bodies are withheld — only the API:\n{api}"
        );
    }

    /// `lr7b_v3` kept failing the same `E0515` four repairs in a row: the
    /// excerpt window is centred on the reported line and reaches two lines
    /// above it, while the statement that owns the answer — the `insert` that
    /// puts the value somewhere it can be borrowed back from — sat five lines
    /// up. A borrow error is settled by statement order, so for the codes that
    /// need a restructure the model gets the whole function.
    #[test]
    fn a_borrow_error_brings_the_whole_function_that_owns_its_answer() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(
            temp.path().join("src").join("lib.rs"),
            "pub struct Manager {\n    dict: HashMap<String, Record>,\n}\n\nimpl Manager {\n    \
             pub fn add_user(&mut self, user: String) -> &mut Record {\n        let record = \
             Record { cost: 0.0 };\n        self.dict.insert(user.clone(), record);\n        \
             self.verbose = true;\n        let _ = self.save_data();\n        let n = \
             self.dict.len();\n        debug_assert!(n > 0);\n        \
             self.dict.values_mut().next().unwrap_or(&mut Record { cost: 0.0 })\n    }\n}\n",
        )
        .unwrap();

        let borrow = "error[E0515]: cannot return value referencing temporary value\n  \
             --> src/lib.rs:13:9\n13 |         self.dict.values_mut().next().unwrap_or(&mut Record \
             { cost: 0.0 })\n   |         ^ returns a value referencing data owned by the current \
             function";
        let api = repair_api_context(temp.path(), borrow);
        assert!(
            api.contains("the WHOLE function"),
            "a restructure code must widen to the item:\n{api}"
        );
        assert!(
            api.contains("self.dict.insert(user.clone(), record);"),
            "the line that owns the fix is five above the error, outside the excerpt window:\n{api}"
        );

        // A plain type mismatch does not buy the body: it is the declaration the
        // model has to match, and the port is 26 KB of code it must not re-read.
        let mismatch = "error[E0308]: mismatched types\n  --> src/lib.rs:13:9\n13 |         \
             self.dict.values_mut().next().unwrap_or(&mut Record { cost: 0.0 })";
        let narrow = repair_api_context(temp.path(), mismatch);
        assert!(
            narrow.contains("pub fn add_user(&mut self, user: String) -> &mut Record"),
            "the signature is still the contract:\n{narrow}"
        );
        assert!(
            !narrow.contains("self.verbose = true;"),
            "no restructure is being asked for, so no body:\n{narrow}"
        );
    }

    /// A declaration head is recognised by the first identifier that is not a
    /// modifier, so that neither a `where` continuation nor a call site reads as
    /// an item the model should be shown.
    #[test]
    fn only_declaration_heads_read_as_items() {
        for (line, want) in [
            ("pub fn merge_costs(", Some("fn")),
            ("pub(crate) async fn save(&self) {", Some("fn")),
            ("unsafe fn raw(&self) -> *const u8 {", Some("fn")),
            ("impl<'a> PersistedUserRecord<'a> {", Some("impl")),
            ("pub struct UpdateCostResult<'a> {", Some("struct")),
            ("enum Mode {", Some("enum")),
            ("trait Saver: Send {", Some("trait")),
            ("where T: Clone,", None),
            (") -> Option<UpdateCostResult<'_>> {", None),
            (
                "    let result_record = PersistedUserRecord { user_id: s };",
                None,
            ),
            ("records: &[PersistedUserRecord],", None),
            ("#[derive(Clone)]", None),
            ("// impl Foo {}", None),
            ("}", None),
        ] {
            assert_eq!(item_head_keyword(line), want, "line: {line}");
        }
    }

    /// An `impl` block is filed under the type it is FOR, so a diagnostic naming
    /// that type finds the block. A generic-laden head must still resolve.
    #[test]
    fn declared_name_resolves_the_type_a_head_is_about() {
        for (head, kind, want) in [
            (
                "impl Display for PersistedUserRecord {",
                "impl",
                "PersistedUserRecord",
            ),
            (
                "impl<'a> PersistedUserRecord<'a> {",
                "impl",
                "PersistedUserRecord",
            ),
            (
                "pub struct UpdateCostResult<'a> {",
                "struct",
                "UpdateCostResult",
            ),
            ("trait Saver: Send {", "trait", "Saver"),
            ("enum Mode {", "enum", "Mode"),
            ("pub(crate) fn load(&self) {", "fn", "load"),
        ] {
            assert_eq!(
                declared_name(head, kind).as_deref(),
                Some(want),
                "head: {head}"
            );
        }
    }

    /// The block only appears when there is a readable location to read from: a
    /// directive with no file text in it must not claim to have pasted one.
    #[test]
    fn no_readable_location_means_no_declaration_block() {
        let temp = tempfile::TempDir::new().unwrap();
        let errors = "error[E0382]: borrow of moved value: `user`";
        assert!(
            repair_api_context(temp.path(), errors).is_empty(),
            "nothing to read, nothing to paste"
        );
        let directive = compile_repair_directive(errors, temp.path());
        assert!(
            !directive.contains("Declarations from the same file"),
            "{directive}"
        );
    }

    /// The `lr7b_v2` regression. Obeying "for a moved value, clone it before
    /// reuse" turned an `E0308` on a `&'a PersistedUserRecord` field into an
    /// `E0515`, and the model had already spent its second repair doing exactly
    /// that. The rule it needs is the one that says a clone is a new temporary —
    /// which the generic sentence cannot say, because it is advice for four
    /// different errors at once.
    #[test]
    fn an_e0515_repair_is_told_that_cloning_cannot_lengthen_a_lifetime() {
        let temp = tempfile::TempDir::new().unwrap();
        let errors = "error[E0515]: cannot return value referencing temporary value\n  \
             --> src/lib.rs:287:9\n287 |         self.user_dict.values_mut().next().unwrap_or(&mut \
             PersistedUserRecord {})\n    |         ^ returns a value referencing data owned by \
             the current function";
        let directive = compile_repair_directive(errors, temp.path());
        assert!(
            directive.contains("OWNED BY THIS FUNCTION"),
            "the E0515 rule must name the real constraint:\n{directive}"
        );
        assert!(
            directive.contains("A clone does NOT fix this"),
            "the model's instinct is to clone — the rule has to refuse it:\n{directive}"
        );
        assert!(
            !directive.contains("clone it before reuse"),
            "the vague advice that manufactured this error must be gone:\n{directive}"
        );
    }

    /// Codes the loop has no rule for must not drag a rule in, and a report with
    /// several codes must stay inside its budget — the repair turn shares a
    /// 20 480-token window with the port itself.
    #[test]
    fn recipes_are_offered_only_for_the_codes_present() {
        let both = "error[E0382]: use of moved value: `x`\nerror[E0308]: mismatched types\n";
        let out = error_code_recipes(both);
        assert!(out.contains("`E0382`"), "{out}");
        assert!(out.contains("`E0308`"), "{out}");
        assert!(
            !out.contains("`E0515`: "),
            "a code that was not raised gets no rule:\n{out}"
        );
        // E0382's rule points at E0515's, because the two are confused constantly.
        assert!(
            out.contains("see E0515 rule") || out.contains("E0515 rule"),
            "{out}"
        );

        let unknown = "error[E0999]: something exotic\n";
        assert!(
            error_code_recipes(unknown).is_empty(),
            "no known code, no recipe"
        );

        let many = [
            "error[E0515]: a\n",
            "error[E0502]: b\n",
            "error[E0382]: c\n",
            "error[E0308]: d\n",
            "error[E0599]: e\n",
            "error[E0277]: f\n",
        ]
        .concat();
        let crowded = error_code_recipes(&many);
        assert!(
            crowded.len() <= 2_400 + 400,
            "the recipe block must stay bounded, got {} chars",
            crowded.len()
        );
        assert_eq!(
            crowded.matches("The rule for each error code").count(),
            1,
            "{crowded}"
        );
        // Every rule is clipped to a clause, never cut mid-word, so the model
        // cannot read half an instruction as the whole instruction.
        for line in crowded.lines().filter(|l| l.starts_with('`')) {
            let body = line.split_once(": ").map_or("", |(_, rest)| rest);
            assert!(
                body.ends_with('.') || body.ends_with(';'),
                "clip must land on a clause end, got {:?}",
                &body[body.len().saturating_sub(30)..]
            );
        }
    }

    /// The pair `lr7b_v5` actually reported: `E0308` (646 bytes of rule) and
    /// `E0515` (856 bytes of rule). Under the old 1500-byte cap that sum was two
    /// bytes over budget, so `error_code_recipes` kept the first and answered the
    /// second with nothing — the harder error of the two lost its rule to a
    /// rounding margin while the model read the silence as "no rule applies".
    #[test]
    fn a_second_long_rule_is_clipped_rather_than_dropped_by_the_budget() {
        let pair = "error[E0308]: mismatched types\n\
             error[E0515]: cannot return value referencing temporary value\n";
        let out = error_code_recipes(pair);
        assert!(out.contains("`E0308`:"), "{out}");
        assert!(
            out.contains("`E0515`:"),
            "the budget must not delete the second rule: {out}"
        );
        assert!(out.len() <= 2_400 + 400, "got {} chars", out.len());
        let e0515 = out
            .lines()
            .find(|l| l.starts_with("`E0515`:"))
            .unwrap_or_default();
        assert!(
            e0515.contains("OWNED BY THIS FUNCTION"),
            "the clip keeps the rule's actual instruction: {e0515}"
        );
    }

    /// `lr7b_v5` spent three repair turns on the same two errors and gave up,
    /// and the reason is inside the diagnostic the loop pastes: for
    /// `user_record: result_record` against a field declared `&'a
    /// PersistedUserRecord`, rustc prints `help: consider borrowing here` with
    /// the literal replacement `user_record: &result_record`. That help is
    /// statement-local — `result_record` is a `let` in the same function whose
    /// value is returned — so taking it trades `E0308` for `E0515`, and a model
    /// that then sees `E0515` removes the `&` again. Following the compiler's
    /// own suggestion is the *most* reasonable-looking move available, which is
    /// exactly why the pendulum survives every other warning.
    #[test]
    fn a_borrow_help_pointed_at_a_local_is_marked_as_the_trap_it_is() {
        let out = error_code_recipes("error[E0308]: mismatched types\n");
        assert!(
            out.contains("help: consider borrowing here"),
            "the loop pastes that suggestion, so the rule has to name it: {out}"
        );
        assert!(
            out.contains("trades `E0308` for `E0515`"),
            "and say what taking it costs: {out}"
        );
        assert!(
            out.contains("take the borrow from `self`"),
            "then give the move that actually works: {out}"
        );

        // The other face of the same run: `values_mut().next().unwrap_or(&mut
        // PersistedUserRecord { .. })` returns a borrow of a literal that dies
        // with the statement. No wrapper rescues it, so `E0515`'s rule has to
        // name the shape and the replacement.
        let escaped =
            error_code_recipes("error[E0515]: cannot return value referencing temporary value\n");
        assert!(
            escaped.contains("unwrap_or(&mut"),
            "recognise the argument the model wrapped the literal in: {escaped}"
        );
        assert!(
            escaped.contains("or_insert_with"),
            "and point at the collection that can hold the value: {escaped}"
        );
    }

    /// A repair dispatch on a port that already exists must still be allowed to
    /// patch. Locking it to write_file re-creates the failure edit_file was
    /// added for: a 25 KB file pushed back through the output budget, which the
    /// model truncates instead of finishing.
    #[test]
    fn a_task_naming_an_existing_file_keeps_the_targeted_editor() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("rust_port").join("src")).unwrap();
        std::fs::write(
            temp.path()
                .join("rust_port")
                .join("src")
                .join("budget_manager.rs"),
            "fn main() {}\n",
        )
        .unwrap();

        assert_eq!(
            existing_file_named_by_task(
                temp.path(),
                "Fix the ONE error in 'rust_port/src/budget_manager.rs'. `cargo check` reports:"
            )
            .as_deref(),
            Some("rust_port/src/budget_manager.rs"),
            "a quoted crate-relative path must be recognised, and the path the task wrote is \
             the one the run has to read"
        );
        // A from-scratch port names only files that do not exist yet.
        assert!(
            existing_file_named_by_task(
                temp.path(),
                "Convert budget_manager.py into rust_port/src/brand_new.rs"
            )
            .is_none(),
            "no existing target, so the write-only lock stays"
        );
        // An absolute token must not probe outside the workspace: joining it
        // onto the root replaces the root entirely.
        assert!(
            existing_file_named_by_task(temp.path(), "read /etc/hosts and C:\\Windows\\win.ini")
                .is_none(),
            "absolute paths are never workspace files"
        );
        assert!(
            existing_file_named_by_task(temp.path(), "the run passed all 52 tests ok").is_none(),
            "prose is not a path"
        );
        // Dot-prefixed directories are ordinary workspace paths: only trailing
        // punctuation may be stripped.
        std::fs::create_dir_all(temp.path().join(".velocity")).unwrap();
        std::fs::write(temp.path().join(".velocity").join("map.nda"), "x").unwrap();
        assert!(
            existing_file_named_by_task(temp.path(), "see .velocity/map.nda,").is_some(),
            "a leading dot is part of the path"
        );
    }

    /// A model patching a borrow conflict has to see where the borrow BEGAN,
    /// and rustc names that line only inside its code echo (`259 |  let entry =
    /// self`), never as a second `-->`. Before the diagnostics filter kept the
    /// block body, those lines were discarded, so the excerpt windowed one site
    /// and the origin sat outside the text the model was handed.
    #[test]
    fn repair_excerpt_widens_to_a_site_named_only_in_the_code_echo() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        let mut source = String::new();
        for i in 1..=12 {
            source.push_str(&format!("    let line_{i} = {i};\n"));
        }
        std::fs::write(temp.path().join("src").join("lib.rs"), source).unwrap();

        let errors = "error[E0502]: cannot borrow `*self` as immutable\n   \
             --> src/lib.rs:9:9\n    |\n2 |           let line_2 = 2;\n    |  \
             _____________________-\n    | |______________________- mutable borrow occurs here\n\
             ...\n9 |           self._save_data();\n    |           ^^^^ immutable borrow here";
        let excerpt = repair_error_excerpt(temp.path(), errors);
        assert!(
            excerpt.contains("let line_2 = 2;"),
            "the origin site must be inside the excerpt:\n{excerpt}"
        );
        assert!(
            excerpt.contains("let line_9 = 9;"),
            "and so must the addressed one:\n{excerpt}"
        );
        assert_eq!(
            excerpt.matches("verbatim from disk").count(),
            1,
            "one widened block covering both sites:\n{excerpt}"
        );
    }

    /// The crucible's actual shape: E0502 prints ONE `-->` line, and the line
    /// that has to change (`mutable borrow later used here`) is several lines
    /// further down, marked only by a caret inside rustc's code block. The
    /// window therefore reaches further down than up.
    #[test]
    fn repair_excerpt_reaches_the_line_rustc_only_marks_with_a_caret() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        let mut source = String::new();
        for i in 1..=12 {
            source.push_str(&format!("    let line_{i} = {i};\n"));
        }
        std::fs::write(temp.path().join("src").join("lib.rs"), source).unwrap();

        let excerpt = repair_error_excerpt(temp.path(), "--> src/lib.rs:4:9");
        assert!(
            excerpt.contains("let line_4 = 4;"),
            "the addressed line:\n{excerpt}"
        );
        assert!(
            excerpt.contains("let line_9 = 9;"),
            "must reach past the caret-only annotation:\n{excerpt}"
        );
        assert!(
            !excerpt.contains("let line_11 = 11;"),
            "and stay small enough to leave context:\n{excerpt}"
        );
    }

    #[test]
    fn repair_excerpt_survives_a_location_that_is_not_in_the_file() {
        // rustc quotes the line the model last saw, and a model that never
        // landed its write leaves the file short. A missing file or an out-of-
        // range line has to produce no excerpt rather than panic mid-dispatch.
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join("src").join("lib.rs"), "fn a() {}\n").unwrap();

        assert_eq!(
            repair_error_excerpt(temp.path(), "  --> src/missing.rs:1:1"),
            "",
            "an unreadable file yields nothing"
        );
        let beyond = repair_error_excerpt(temp.path(), "  --> src/lib.rs:900:5");
        assert!(
            beyond.contains("fn a() {}"),
            "a line past the end still shows the file it does have:\n{beyond}"
        );
        assert_eq!(
            repair_error_excerpt(temp.path(), "error: aborting due to 1 previous error"),
            "",
            "diagnostics without a location yield nothing"
        );
    }

    /// A repair that trades one red assertion for another has to be told so on
    /// the turn it lands. testgate4 proved what happens otherwise: the last
    /// repair changed `0.004` into `0.005`, nothing reported it, and the run
    /// spent its final ~200s on a `grep_search` turn before the wall clock died
    /// with the failure still standing.
    #[test]
    fn a_repair_write_is_graded_on_its_tests_once_the_test_gate_has_engaged() {
        let report = format!(
            "{} Real `cargo test` output (test result: FAILED. 71 passed; 1 failed), one block per failing test:\n",
            crate::agent::executor::thread::TEST_FAILURE_PREAMBLE
        );
        assert!(matches!(
            grade_write(Ok(()), Some(Err(report.clone()))),
            WriteGrade::Repair(m) if m == report
        ));
        // The label the model sees distinguishes this from a build break, which
        // is the whole reason the run was guessing.
        assert!(is_test_failure_report(&report));
        assert_eq!(gate_failure_label(&report), "The crate's tests failed");
    }

    /// The other half of the same turn: a repair that actually fixed the
    /// assertion ends the run instead of earning another chance to break it.
    #[test]
    fn a_green_repair_ends_the_run_on_the_turn_it_lands() {
        assert_eq!(
            grade_write(
                Ok(()),
                Some(Ok(TestVerdict::Passed("72 passed".to_string())))
            ),
            WriteGrade::Validated("72 passed".to_string())
        );
    }

    /// An unmeasurable test run is not a pass, and a completion claim built on
    /// it would be the false "Compiler validation succeeded!" bug again.
    #[test]
    fn a_repair_whose_tests_could_not_be_measured_claims_nothing() {
        assert_eq!(
            grade_write(
                Ok(()),
                Some(Ok(TestVerdict::NotJudged(
                    "`cargo test` overran its 240s deadline".to_string()
                )))
            ),
            WriteGrade::NotJudged("`cargo test` overran its 240s deadline".to_string())
        );
    }

    /// `cargo test` on a crate that does not compile certifies nothing, so the
    /// build verdict has to win even against a green-looking test verdict.
    #[test]
    fn a_broken_build_outranks_any_test_verdict_it_arrived_with() {
        let grade = grade_write(
            Err("error[E0308]: mismatched types".to_string()),
            Some(Ok(TestVerdict::Passed("72 passed".to_string()))),
        );
        assert_eq!(
            grade,
            WriteGrade::Repair("error[E0308]: mismatched types".to_string())
        );
    }

    /// Before the test gate has rejected the artifact the crate may still be
    /// half-written, so the build-only verdict must stay silent rather than
    /// grade a file the model is still composing.
    #[test]
    fn a_write_before_the_test_gate_engages_is_not_graded_on_tests() {
        assert_eq!(grade_write(Ok(()), None), WriteGrade::BuildClean);
    }

    fn call(name: &str, arguments: &str) -> ToolCallAccumulator {
        ToolCallAccumulator {
            id: "native_0".to_string(),
            name: name.to_string(),
            arguments: arguments.to_string(),
        }
    }

    /// A turn cut off at the wall-clock deadline is mid-argument-list by
    /// definition: executing the call would write the head of a file and stop.
    #[test]
    fn a_tool_call_truncated_by_the_deadline_is_never_executed() {
        let truncated = call(
            "write_file",
            r#"{"relativeFilePath":"rust_port/src/budget_manager.rs","content":"pub struct Budget"#,
        );
        assert!(salvage_complete_calls(vec![truncated]).is_empty());
        // Empty deltas stream the same way an abandoned one does.
        assert!(salvage_complete_calls(vec![call("write_file", "")]).is_empty());
    }

    /// Arguments that close can only have been emitted in full, so a call that
    /// landed before the cut is finished work and must not be thrown away with
    /// the unfinished sibling beside it.
    #[test]
    fn a_tool_call_that_arrived_in_full_before_the_cut_still_lands() {
        let args = r#"{"relativeFilePath":"a.rs","edits":[{"oldString":"x","newString":"y"}]}"#;
        let kept = salvage_complete_calls(vec![
            call("edit_file", args),
            call("write_file", r#"{"relativeFilePath":"b.rs","content":"fn "#),
        ]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].name, "edit_file");
        assert_eq!(kept[0].arguments, args);
        // A tool that genuinely takes no arguments is complete, not truncated.
        assert_eq!(
            salvage_complete_calls(vec![call("index_workspace", "{}")]).len(),
            1
        );
    }
}
