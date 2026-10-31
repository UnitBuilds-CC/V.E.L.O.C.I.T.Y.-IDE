//! Living codebase event store.
//!
//! Records *why* and *what* changed in the codebase, correlated with the site
//! map Merkle root so that every edit can be traced back to the agent context
//! that produced it.  Events are persisted as newline-delimited JSON (JSONL)
//! under `.velocity/events/events.jsonl`.
//!
//! # Architecture
//!
//! ```text
//! t₀  Site Map: root=aaaa  (initial state)
//!
//! t₁  Agent adds function X to file Y
//!     ├─ Merkle root: aaaa → bbbb
//!     ├─ Context: "Auth needed token validation for OAuth2"
//!     └─ Outcome: SUCCESS
//!
//! t₂  Agent tries function Z to replace X
//!     ├─ Merkle root: bbbb → cccc
//!     ├─ Context: "Z uses caching, should be faster"
//!     └─ Outcome: FAILURE — "Z breaks concurrent auth flows"
//!
//! t₃  Agent reverts Z, restores X
//!     ├─ Merkle root: cccc → bbbb  (same as t₁!)
//!     ├─ Context: "Reverting to X, Z failure due to race condition"
//!     └─ Outcome: REVERT
//! ```
//!
//! An LLM reviewing file Y ten years from now can query the event store to
//! recover the full decision history — including failed attempts — for every
//! function in the codebase.

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

/// Serializes the read-modify-write (`load_all` to pick the next sequence,
/// then `append_event`) so concurrent writers — the orchestrator runs several
/// worker threads in one process, and dispatch itself can overlap — cannot
/// assign the same sequence number to two events.  The append syscall is
/// already atomic (see `append_event`); this closes the *sequence* race, which
/// is the shared-write conflict that matters for a monotonic decision trail.
static APPEND_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// Bytes of the log each append reads to find the sequence number the next
/// event takes.
///
/// A record is one line, so the newest event — and therefore the number the next
/// one takes — is on the last complete line of the file. The window is a fast
/// path, not a guarantee of one: `context` is capped at
/// [`MAX_TOOL_RESULT_FOR_CONTEXT`] but `metadata` and `affected_files` are not,
/// so a record can outgrow it. When the window holds no complete line the append
/// falls back to reading the log, which costs what the old path always cost and
/// numbers the event the same way.
const SEQUENCE_TAIL_WINDOW_BYTES: u64 = 64 * 1024;

/// What `events.jsonl` is allowed to grow to before the oldest half of it is
/// dropped, and the environment variable that raises or removes the bound
/// (`0`/`off`/`none`/`unbounded`/`unlimited`/`infinite` for no bound).
///
/// Eight megabytes is the same ceiling the context-spill ledger stops at, and it
/// is not a guess at tidiness: every consumer of the trail — the Decision Trail
/// appended to a `read_file` result, `event_context`, `history_brief` — loads
/// the whole log, so the bound is also what keeps those reads from growing with
/// the length of a workspace's history.
pub const DEFAULT_EVENT_LOG_MAX_BYTES: u64 = 8 * 1024 * 1024;
pub const EVENT_LOG_MAX_BYTES_ENV: &str = "VELOCITY_EVENT_STORE_MAX_BYTES";

/// The floor of events a prune never takes away, so a bound set (or misset) to
/// something tiny cannot empty the trail the next agent depends on.
const RETENTION_FLOOR_EVENTS: usize = 256;

thread_local! {
    /// Times *this thread* re-read the whole event log.
    ///
    /// Instrumentation rather than product feature: the append path used to load
    /// every event to find the highest sequence number, which is what made
    /// recording the nth event cost n reads. A test can only prove that is gone
    /// by counting the loads, and the counter is per thread because the suite
    /// runs tests in parallel against stores that are nothing like each other.
    static FULL_LOG_SCANS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn note_full_log_scan() {
    FULL_LOG_SCANS.with(|counter| counter.set(counter.get() + 1));
}

/// Full-log reads performed by the calling thread since the process started.
pub fn full_log_scans() -> u64 {
    FULL_LOG_SCANS.with(|counter| counter.get())
}

/// Spell the retention bound the way every other adjustable ceiling in the
/// harness is spelled: a number is that many bytes, `0`/`off`/`none`/
/// `unbounded`/`unlimited`/`infinite` means no bound at all, and anything else —
/// including a typo — falls back to the built-in default rather than launching an
/// unbounded log. `Unbounded` becomes `u64::MAX`, which is *never* exceeded by a
/// file size, rather than a large number that a long run can outrun.
pub fn retention_limit(raw: Option<&str>) -> u64 {
    use crate::agent::executor::utils::{parse_limit_setting, LimitSetting};
    match parse_limit_setting(raw) {
        LimitSetting::Unbounded => u64::MAX,
        LimitSetting::Value(bytes) => bytes,
        LimitSetting::Default => DEFAULT_EVENT_LOG_MAX_BYTES,
    }
}

// ─── Event model ────────────────────────────────────────────────────────────

/// Outcome of a codebase event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventOutcome {
    /// The change succeeded and is still in place.
    Success,
    /// The change was attempted but failed and was reverted or rolled back.
    Failure,
    /// The change was explicitly reverted to a prior state.
    Revert,
    /// Outcome not yet determined (event was just created).
    Pending,
}

impl EventOutcome {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Revert => "revert",
            Self::Pending => "pending",
        }
    }
}

/// A single codebase event linking an agent action to a site map state change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodebaseEvent {
    /// Monotonic sequence number (1-based).
    pub sequence: u64,
    /// Unix timestamp in milliseconds.
    pub timestamp_ms: u64,
    /// Tool that triggered the event (e.g. `index_workspace`, `write_file`).
    pub tool_name: String,
    /// Human-readable description of what changed.
    pub description: String,
    /// Site map Merkle root *before* the change (hex, 16 chars).
    pub merkle_root_before: Option<String>,
    /// Site map Merkle root *after* the change (hex, 16 chars).
    pub merkle_root_after: Option<String>,
    /// Agent context: *why* this change was made.
    pub context: Option<String>,
    /// Outcome — may be updated later via `event_mark_outcome`.
    pub outcome: EventOutcome,
    /// Optional failure reason (if outcome is Failure).
    pub failure_reason: Option<String>,
    /// Files affected (relative paths).
    pub affected_files: Vec<String>,
    /// Free-form metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// Causal identity of who performed the action (Spine-1). Pre-refactor log
    /// lines that predate this field load as an unattributable `Service`
    /// principal via `#[serde(default)]` (spec §7 back-compat, INV-4); real
    /// human/agent principals are populated by the watcher (Phase 2) and the
    /// executor/RBAC path (Phase 5).
    #[serde(default)]
    pub actor: crate::automation::principal::Principal,
}

/// Tools that never mutate the codebase: reads, searches, GUI state polling and
/// browser inspection.  They dominate the raw log and bury the decisions that
/// actually matter, so the trail renderer demotes them below the fold.
fn is_read_only_tool(tool: &str) -> bool {
    const READ_ONLY: &[&str] = &[
        "read_file",
        "grep_search",
        "search_",
        "list_",
        "glob",
        "gui_get_state",
        "gui_",
        "browser_",
        "event_history",
        "event_timeline",
        "event_context",
        "use_skill",
        "sitemap_query",
        "view_",
    ];
    READ_ONLY.iter().any(|prefix| tool.starts_with(prefix))
}

impl CodebaseEvent {
    /// Whether this event carries decision signal worth surfacing at the top
    /// of a file's trail: a state change, a failure, a revert, a recorded
    /// conflict / stale-plan / task-outcome, or any non read-only tool.
    /// Read-only polling with no root transition is *noise*.
    pub fn is_high_signal(&self) -> bool {
        if self.outcome == EventOutcome::Failure || self.outcome == EventOutcome::Revert {
            return true;
        }
        if let Some(md) = &self.metadata {
            if md.get("shared_write_conflict").is_some()
                || md.get("stale_plan").is_some()
                || md.get("worker_task_outcome").is_some()
            {
                return true;
            }
        }
        // A Merkle-root transition means the codebase actually changed.
        if let (Some(before), Some(after)) = (&self.merkle_root_before, &self.merkle_root_after) {
            if before != after {
                return true;
            }
        }
        !is_read_only_tool(&self.tool_name)
    }
}

// ─── Event store ────────────────────────────────────────────────────────────

/// Persistent event store backed by a JSONL file.
///
/// Events are append-only; updates (outcome changes) rewrite the full file
/// because the expected volume is low (hundreds to thousands of events, not
/// millions). An append reads only the log's last line to number itself, and the
/// file is kept under [`DEFAULT_EVENT_LOG_MAX_BYTES`] — tunable with
/// [`EVENT_LOG_MAX_BYTES_ENV`] — by dropping its oldest half when it passes.
pub struct EventStore {
    dir: PathBuf,
    max_bytes: u64,
}

impl EventStore {
    /// Open (or create) the event store at `<workspace>/.velocity/events/`.
    ///
    /// The retention bound is read from the environment once, here, so an append
    /// does not pay a `getenv` to decide whether it needs a `stat`.
    pub fn open(workspace: &Path) -> Self {
        let dir = workspace.join(".velocity").join("events");
        let _ = fs::create_dir_all(&dir);
        Self {
            dir,
            max_bytes: retention_limit(std::env::var(EVENT_LOG_MAX_BYTES_ENV).ok().as_deref()),
        }
    }

    /// Override the retention bound for this handle. `0` means no bound.
    pub fn with_max_bytes(mut self, max_bytes: u64) -> Self {
        self.max_bytes = if max_bytes == 0 { u64::MAX } else { max_bytes };
        self
    }

    /// Path to the JSONL events file.
    fn events_path(&self) -> PathBuf {
        self.dir.join("events.jsonl")
    }

    /// Append a new event with an as-yet-undetermined outcome.  Returns the
    /// assigned sequence number.  Used by the manual `event_record` tool, where
    /// the agent declares the change up front and marks the outcome later.
    pub fn record(
        &self,
        tool_name: &str,
        description: &str,
        merkle_root_before: Option<String>,
        merkle_root_after: Option<String>,
        context: Option<String>,
        affected_files: Vec<String>,
    ) -> Result<u64, String> {
        self.record_with_outcome(
            tool_name,
            description,
            merkle_root_before,
            merkle_root_after,
            context,
            affected_files,
            EventOutcome::Pending,
            None,
        )
    }

    /// Append a new event whose outcome is already known.
    ///
    /// Automatic dispatch recording must use this: the tool call has already
    /// succeeded or failed by the time we record it, so writing `Pending` and
    /// following up with [`mark_outcome`](Self::mark_outcome) would rewrite the
    /// whole log for every call — and, historically, never did the second
    /// step, leaving the entire decision trail unresolvable.
    #[allow(clippy::too_many_arguments)]
    pub fn record_with_outcome(
        &self,
        tool_name: &str,
        description: &str,
        merkle_root_before: Option<String>,
        merkle_root_after: Option<String>,
        context: Option<String>,
        affected_files: Vec<String>,
        outcome: EventOutcome,
        failure_reason: Option<String>,
    ) -> Result<u64, String> {
        self.record_with_metadata(
            tool_name,
            description,
            merkle_root_before,
            merkle_root_after,
            context,
            affected_files,
            outcome,
            failure_reason,
            None,
        )
    }

    /// Append a new event carrying structured metadata (used for shared-write
    /// conflicts, stale-plan rejections and worker task outcomes so the decision
    /// trail is machine-queryable, not just human-readable). The whole
    /// read-modify-write is serialized under [`APPEND_LOCK`] so concurrent
    /// writers never collide on the sequence number.
    #[allow(clippy::too_many_arguments)]
    pub fn record_with_metadata(
        &self,
        tool_name: &str,
        description: &str,
        merkle_root_before: Option<String>,
        merkle_root_after: Option<String>,
        context: Option<String>,
        affected_files: Vec<String>,
        outcome: EventOutcome,
        failure_reason: Option<String>,
        metadata: Option<serde_json::Value>,
    ) -> Result<u64, String> {
        let _guard = APPEND_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // The next sequence number comes from the log's last line, not from its
        // whole contents: reading every event to number the next one made the
        // nth append cost n reads, and the trail is written once per tool call.
        let next_seq = match self.next_sequence_from_tail() {
            Some(sequence) => sequence,
            None => {
                let events = self.load_all()?;
                events.last().map(|event| event.sequence + 1).unwrap_or(1)
            }
        };

        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let event = CodebaseEvent {
            sequence: next_seq,
            timestamp_ms,
            tool_name: tool_name.to_string(),
            description: description.to_string(),
            merkle_root_before,
            merkle_root_after,
            context,
            outcome,
            failure_reason,
            affected_files,
            metadata,
            // Principals are wired in later phases; every event recorded from
            // the current executor path defaults to the unattributable Service
            // identity so existing callers are unchanged (INV-4).
            actor: crate::automation::principal::Principal::default(),
        };

        self.append_event(&event)?;
        // Runs while `APPEND_LOCK` is held: a prune rewrites the same file every
        // writer in here is appending to, so it cannot be left unlocked.
        self.enforce_retention();
        Ok(next_seq)
    }

    /// Record that a *shared write* was rejected because another agent already
    /// holds a lock over the same file/directory scope.  This is the durable
    /// decision-trail artefact for the mediator's conflict resolution: without
    /// it the history cannot explain why a worker never wrote, and future
    /// agents repeat the same blocked attempt.
    pub fn record_shared_write_conflict(
        &self,
        tool_name: &str,
        description: &str,
        conflict_kind: &str,
        existing_agent: &str,
        requested_agent: &str,
        affected_files: Vec<String>,
        contract: &str,
    ) -> Result<u64, String> {
        let metadata = json!({
            "shared_write_conflict": true,
            "conflict_kind": conflict_kind,
            "existing_agent": existing_agent,
            "requested_agent": requested_agent,
            "mediation_contract": contract,
        });
        self.record_with_metadata(
            tool_name,
            description,
            None,
            None,
            Some(format!(
                "Shared-write conflict ({conflict_kind}): {requested_agent} blocked by {existing_agent}"
            )),
            affected_files,
            EventOutcome::Failure,
            Some(format!("conflict: {conflict_kind}")),
            Some(metadata),
        )
    }

    /// Record that a worker assignment was discarded because the site map had
    /// moved on since the plan was made (the plan's Merkle root no longer
    /// matches the live root).  Surfacing stale plans in history tells future
    /// agents *why* an intended change never landed.
    pub fn record_stale_plan(
        &self,
        description: &str,
        planned_root: &str,
        actual_root: &str,
        affected_files: Vec<String>,
    ) -> Result<u64, String> {
        let metadata = json!({
            "stale_plan": true,
            "planned_site_map_root": planned_root,
            "actual_site_map_root": actual_root,
        });
        self.record_with_metadata(
            "worker_assignment",
            description,
            Some(actual_root.to_string()),
            None,
            Some(format!(
                "Plan built against site-map root {planned_root} but the workspace is now {actual_root}; assignment discarded as stale."
            )),
            affected_files,
            EventOutcome::Failure,
            Some("stale plan".to_string()),
            Some(metadata),
        )
    }

    /// Record the final outcome of an orchestrator worker task so the decision
    /// trail links site-map transitions to the agent that produced them.
    pub fn record_task_outcome(
        &self,
        task_id: u64,
        description: &str,
        outcome: EventOutcome,
        summary: &str,
        affected_files: Vec<String>,
    ) -> Result<u64, String> {
        let metadata = json!({ "worker_task_outcome": true, "task_id": task_id });
        self.record_with_metadata(
            "worker_task",
            description,
            None,
            None,
            Some(summary.to_string()),
            affected_files,
            outcome,
            None,
            Some(metadata),
        )
    }

    /// Update the outcome (and optional failure reason) of an existing event.
    pub fn mark_outcome(
        &self,
        sequence: u64,
        outcome: EventOutcome,
        failure_reason: Option<String>,
    ) -> Result<bool, String> {
        self.rewrite_record(sequence, move |event| {
            event.outcome = outcome;
            event.failure_reason = failure_reason;
        })
    }

    /// Attach or replace the agent context (the *why*) on an existing event.
    pub fn update_context(&self, sequence: u64, context: &str) -> Result<bool, String> {
        let context = context.to_string();
        self.rewrite_record(sequence, move |event| {
            event.context = Some(context);
        })
    }

    /// Edit one record in place: read the log, change the event, write it back.
    ///
    /// Both halves belong inside `APPEND_LOCK`. The rewrite replaces the whole
    /// file, so a version read before another writer appended drops that append
    /// on the way out - and the append path is the one every tool call takes,
    /// while this one runs when an outcome arrives later, which is exactly when
    /// something else has already been written. Held together, the newest state
    /// on disk is the state the rewrite starts from, and "update one record"
    /// cannot cost the trail an event it never saw.
    fn rewrite_record(
        &self,
        sequence: u64,
        edit: impl FnOnce(&mut CodebaseEvent),
    ) -> Result<bool, String> {
        let _guard = APPEND_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut events = self.load_all()?;
        let Some(index) = events.iter().position(|event| event.sequence == sequence) else {
            return Ok(false);
        };
        edit(&mut events[index]);
        self.save_all(&events)?;
        Ok(true)
    }

    /// Load all events (oldest first).
    ///
    /// Malformed lines are skipped with a warning so a single corrupt record
    /// (e.g. two JSON objects concatenated by a concurrent write) cannot lock
    /// the whole event log out of every consumer.  Each line is attempted as
    /// one record; if that fails we additionally try to split concatenated
    /// `}{` sequences and parse each fragment independently.
    pub fn load_all(&self) -> Result<Vec<CodebaseEvent>, String> {
        let path = self.events_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(&path)
            .map_err(|e| format!("Failed to open events file {}: {}", path.display(), e))?;
        note_full_log_scan();
        let reader = BufReader::new(file);
        let mut events = Vec::new();
        for (idx, line) in reader.lines().enumerate() {
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    log::warn!("event_store: skipping unreadable line {}: {}", idx + 1, e);
                    continue;
                }
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<CodebaseEvent>(trimmed) {
                Ok(ev) => events.push(ev),
                Err(primary) => {
                    // Try to recover concatenated objects: split on `}{` and
                    // rewrap the boundaries so each fragment is valid JSON.
                    let recovered = recover_concatenated(trimmed);
                    if recovered.is_empty() {
                        log::warn!(
                            "event_store: skipping malformed event on line {}: {}",
                            idx + 1,
                            primary
                        );
                        continue;
                    }
                    for frag in recovered {
                        match serde_json::from_str::<CodebaseEvent>(&frag) {
                            Ok(ev) => events.push(ev),
                            Err(e) => log::warn!(
                                "event_store: fragment on line {} still unparsable: {}",
                                idx + 1,
                                e
                            ),
                        }
                    }
                }
            }
        }
        Ok(events)
    }

    /// Query events with optional filters.
    pub fn query(
        &self,
        file_filter: Option<&str>,
        outcome_filter: Option<&EventOutcome>,
        limit: usize,
    ) -> Result<Vec<CodebaseEvent>, String> {
        let events = self.load_all()?;
        let filtered: Vec<CodebaseEvent> = events
            .into_iter()
            .rev() // newest first
            .filter(|e| {
                if let Some(file) = file_filter {
                    if !e.affected_files.iter().any(|f| f.contains(file)) {
                        return false;
                    }
                }
                if let Some(outcome) = outcome_filter {
                    if e.outcome != *outcome {
                        return false;
                    }
                }
                true
            })
            .take(limit)
            .collect();
        Ok(filtered)
    }

    /// Get a single event by sequence number.
    pub fn get(&self, sequence: u64) -> Result<Option<CodebaseEvent>, String> {
        let events = self.load_all()?;
        Ok(events.into_iter().find(|e| e.sequence == sequence))
    }

    /// Total number of events.
    pub fn count(&self) -> Result<usize, String> {
        Ok(self.load_all()?.len())
    }

    /// Get events correlated with a specific Merkle root (before or after).
    pub fn by_merkle_root(&self, root_hex: &str) -> Result<Vec<CodebaseEvent>, String> {
        let events = self.load_all()?;
        let filtered: Vec<CodebaseEvent> = events
            .into_iter()
            .filter(|e| {
                e.merkle_root_before.as_deref() == Some(root_hex)
                    || e.merkle_root_after.as_deref() == Some(root_hex)
            })
            .collect();
        Ok(filtered)
    }

    // ── Internal helpers ────────────────────────────────────────────────

    /// The sequence number the next appended event should take, from the last
    /// line of the log alone — or `None` when the tail cannot answer with
    /// certainty and the caller has to read the whole file to say.
    ///
    /// Two things can make the tail unable to answer, and neither is unusual
    /// enough to guess around: the newest bytes are a record a killed writer
    /// left half-written, and the window's first line is itself a fragment of a
    /// longer file. A half-written record is the more dangerous of the two,
    /// because its sequence is unknowable — taking it would number the next
    /// event after a record that may not exist, and the two would then collide.
    fn next_sequence_from_tail(&self) -> Option<u64> {
        use std::io::{Read, Seek, SeekFrom};
        let path = self.events_path();
        let mut file = fs::File::open(&path).ok()?;
        let length = file.metadata().ok()?.len();
        if length == 0 {
            return Some(1);
        }
        let window = length.min(SEQUENCE_TAIL_WINDOW_BYTES);
        let mut bytes = vec![0u8; window as usize];
        file.seek(SeekFrom::Start(length - window)).ok()?;
        file.read_exact(&mut bytes).ok()?;
        // Lossy, because the cut can land inside a multi-byte character in a
        // description. The replacement character is a legal string byte for the
        // parser and no record's `sequence` depends on it.
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let mut lines: Vec<&str> = text.split('\n').collect();
        // Always drop the final element: when the window ends on a newline it is
        // the empty string after it, and when it does not the writer was cut
        // mid-record, so those bytes are a fragment, not the newest event.
        lines.pop();
        if length > window {
            // Bytes before the first newline: the tail of a record that started
            // outside the window. Drop it, and if nothing complete survives the
            // window held exactly one record and it was this fragment.
            if lines.len() <= 1 {
                return None;
            }
            lines.remove(0);
        }
        let last_line = lines.last()?.trim();
        if last_line.is_empty() {
            return None;
        }
        highest_sequence_on_line(last_line).map(|sequence| sequence + 1)
    }

    /// Keep the log inside its byte bound by dropping its oldest half.
    ///
    /// The trigger is a `stat`, so an append that does not cross the bound pays
    /// nothing; the rewrite halves what is there, so the work it does pay
    /// amortises to a constant per event instead of re-reading the trail on every
    /// one. Dropped events are gone for `get`/`mark_outcome`/`update_context`,
    /// which report not-found rather than resurrecting them — that is the cost of
    /// the bound, and the reason the default is generous and the knob exists.
    ///
    /// Callers must hold `APPEND_LOCK`.
    fn enforce_retention(&self) {
        let limit = self.max_bytes;
        let path = self.events_path();
        let Ok(metadata) = fs::metadata(&path) else {
            return;
        };
        if metadata.len() <= limit {
            return;
        }
        let Ok(events) = self.load_all() else {
            return;
        };
        if events.len() <= RETENTION_FLOOR_EVENTS {
            // Already at the floor and still over the bound: every record here is
            // far larger than the cap on a record's context allows, so dropping
            // more would take the trail rather than trim it. Leave it and say so
            // instead of rewriting the same file on every append.
            log::warn!(
                "event_store: {} events use {} bytes, over the {} byte bound, and the store is \
                 already at its {} event floor — nothing is dropped.",
                events.len(),
                metadata.len(),
                limit,
                RETENTION_FLOOR_EVENTS
            );
            return;
        }
        let keep = (events.len() / 2).max(RETENTION_FLOOR_EVENTS);
        let dropped = events.len() - keep;
        match self.save_all(&events[dropped..]) {
            Ok(()) => log::info!(
                "event_store: retention dropped the {} oldest events (sequences through {}) \
                 to hold the log under {} bytes",
                dropped,
                events[dropped - 1].sequence,
                limit
            ),
            Err(e) => log::warn!("event_store: retention rewrite failed, log left as is: {e}"),
        }
    }

    fn append_event(&self, event: &CodebaseEvent) -> Result<(), String> {
        let path = self.events_path();
        let line = serde_json::to_string(event)
            .map_err(|e| format!("Failed to serialize event: {}", e))?;
        let needs_newline = !log_ends_on_a_line_break(&path);
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("Failed to open events file for append: {}", e))?;
        use std::io::Write;
        // Serialize + newline must go down as one syscall so concurrent
        // appenders cannot interleave between the two halves.  `writeln!`
        // internally issues two writes on some platforms which is how we
        // previously ended up with `}{` collisions in events.jsonl.
        let mut payload = String::with_capacity(line.len() + if needs_newline { 2 } else { 1 });
        if needs_newline {
            payload.push('\n');
        }
        payload.push_str(&line);
        payload.push('\n');
        file.write_all(payload.as_bytes())
            .map_err(|e| format!("Failed to write event: {}", e))?;
        file.flush()
            .map_err(|e| format!("Failed to flush event: {}", e))?;
        Ok(())
    }

    fn save_all(&self, events: &[CodebaseEvent]) -> Result<(), String> {
        let path = self.events_path();
        let mut content = String::new();
        for event in events {
            let line = serde_json::to_string(event)
                .map_err(|e| format!("Failed to serialize event: {}", e))?;
            content.push_str(&line);
            content.push('\n');
        }
        fs::write(&path, content)
            .map_err(|e| format!("Failed to write events file {}: {}", path.display(), e))?;
        Ok(())
    }
}

/// Whether the log's last byte is a line break, or there is nothing to break.
///
/// A writer killed mid-record leaves bytes with no newline behind. Appending
/// the next record straight onto them glues the two together, and `load_all`
/// then loses the record that was *not* corrupted — so an unterminated tail has
/// to be closed off before the following line is written. A file that cannot be
/// read is treated as framed: this is a repair, not a precondition.
fn log_ends_on_a_line_break(path: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = fs::File::open(path) else {
        return true;
    };
    if file.seek(SeekFrom::End(-1)).is_err() {
        // Empty file: nothing to terminate.
        return true;
    }
    let mut last = [0u8; 1];
    match file.read(&mut last) {
        Ok(1) => last[0] == b'\n',
        _ => true,
    }
}

/// The newest sequence number written on one physical line of the log.
///
/// Normally that is the line's own record. It is the maximum of several when a
/// concurrent writer glued records together without a newline between them —
/// the `}{` shape `load_all` recovers from — because then the *last* object on
/// the line is the newest event, and taking the first would hand out a sequence
/// that is already in use.
fn highest_sequence_on_line(line: &str) -> Option<u64> {
    if let Ok(event) = serde_json::from_str::<CodebaseEvent>(line) {
        return Some(event.sequence);
    }
    recover_concatenated(line)
        .iter()
        .filter_map(|fragment| serde_json::from_str::<CodebaseEvent>(fragment).ok())
        .map(|event| event.sequence)
        .max()
}

/// Best-effort recovery of two or more JSON objects that were concatenated
/// onto a single line by concurrent writers.  We scan the string tracking
/// brace depth (and skipping over string literals) so that a boundary is only
/// recognised when depth returns to zero *and* the next non-whitespace byte
/// starts another object.  Returns an empty vec when the input has no
/// `}{`-style collisions or when recovery yields no parsable fragments.
fn recover_concatenated(input: &str) -> Vec<String> {
    let bytes = input.as_bytes();
    let mut fragments = Vec::new();
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escaped = false;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    // Emit the fragment up to and including `}` at position i.
                    let frag = &input[start..=i];
                    // Only accept fragments that at least look like an event.
                    if frag.contains("\"sequence\"") && frag.contains("\"tool_name\"") {
                        fragments.push(frag.to_string());
                    }
                    // Skip any whitespace between this object and the next.
                    let mut j = i + 1;
                    while j < bytes.len() && (bytes[j] as char).is_whitespace() {
                        j += 1;
                    }
                    if j >= bytes.len() {
                        break;
                    }
                    start = j;
                    i = j;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    // If we only recovered one fragment identical to the input, it isn't a
    // concatenation — signal no-op so the caller logs the primary error.
    if fragments.len() <= 1 {
        return Vec::new();
    }
    fragments
}

/// Whole-line header of the harness-appended annotation block. Kept as a
/// constant so the enricher and the write-side stripper can never drift.
pub const DECISION_TRAIL_HEADER: &str = "── Decision Trail ──";
/// Prefix of the banner line the enricher places before the header.
pub const DECISION_TRAIL_BANNER: &str = "── End of file content.";

/// The longest a tool result can be and still reach the model's NEXT turn.
///
/// `compress_history` replaces any older tool result above this size with a
/// stub, and a stub is worthless to a model that is about to build an
/// `edit_file` `oldString`: it has to have the file's own characters. The value
/// lives in this module because the harness that *builds* tool results has to
/// budget for it — the Decision Trail used to be appended without measuring, and
/// a capped `read_file` window plus its trail crossed the threshold, so the text
/// the model had just read was elided before it could be used. The compressor
/// imports this instead of repeating the number, so the two cannot drift.
pub const MAX_TOOL_RESULT_FOR_CONTEXT: usize = 4_000;

/// Render one trail entry line. Every entry line begins with a bracketed
/// marker (`[#seq]`, `[verdict]`, `[noise]`) so the write-side stripper can
/// recognise the whole block without cutting legitimate prose.
fn render_trail_entry(seq: u64, outcome_tag: &str, tool: &str, description: &str) -> String {
    format!("  [#{seq}] {outcome_tag} [{tool}] {description}")
}

/// A one-line decisive summary placed at the top of a file's trail so an agent
/// reading the file immediately learns its standing — how many changes landed,
/// whether the latest attempt failed, and whether any shared-write conflicts
/// were recorded — without parsing the whole log.
pub fn file_verdict(events: &[CodebaseEvent]) -> String {
    let signal: Vec<&CodebaseEvent> = events.iter().filter(|e| e.is_high_signal()).collect();
    let count = |pred: &dyn Fn(&CodebaseEvent) -> bool| signal.iter().filter(|e| pred(e)).count();
    let failures = count(&|e| e.outcome == EventOutcome::Failure);
    let reverts = count(&|e| e.outcome == EventOutcome::Revert);
    let successes = count(&|e| e.outcome == EventOutcome::Success);
    let conflicts = count(&|e| {
        e.metadata
            .as_ref()
            .is_some_and(|m| m.get("shared_write_conflict").is_some())
    });

    let mut parts = vec![format!("{successes} change(s) landed")];
    if failures > 0 {
        parts.push(format!("{failures} failure(s)"));
    }
    if conflicts > 0 {
        parts.push(format!("{conflicts} shared-write conflict(s)"));
    }
    if reverts > 0 {
        parts.push(format!("{reverts} revert(s)"));
    }
    let mut verdict = format!("[verdict] {}.", parts.join(", "));
    if let Some(newest) = signal.iter().max_by_key(|e| e.sequence) {
        verdict.push_str(&format!(
            " Latest decision [#{}] {} [{}] {}",
            newest.sequence,
            newest.outcome.label(),
            newest.tool_name,
            newest.description
        ));
        if let Some(reason) = &newest.failure_reason {
            verdict.push_str(&format!(" — {reason}"));
        }
        verdict.push('.');
    }
    verdict
}

/// Enrich a `read_file` response with a decision trail from the event store.
/// Returns the original content with a `── Decision Trail ──` section appended
/// (only if events exist for the file). The banner is deliberately explicit:
/// fast models copy whole tool results back into file writes, and a live run
/// leaked an unlabelled trail into the file itself.
///
/// The trail is *signal-first*: state changes, failures, conflicts and task
/// outcomes are listed (newest first, capped), and read-only polling is folded
/// into a single `[noise]` count so it cannot bury the decisions.
pub fn enrich_read_response(root: &Path, relative_path: &str, content: &str) -> String {
    let store = EventStore::open(root);
    let events = match store.query(Some(relative_path), None, 40) {
        Ok(e) if !e.is_empty() => e,
        _ => return content.to_string(),
    };

    // `query` yields newest-first; keep that order within each partition.
    let signal: Vec<&CodebaseEvent> = events.iter().filter(|e| e.is_high_signal()).collect();
    let noise = events.iter().filter(|e| !e.is_high_signal()).count();
    if signal.is_empty() && noise == 0 {
        return content.to_string();
    }
    // A result this size is already going to be elided next turn; appending
    // metadata only makes the stub that replaces it longer.
    if content.len() >= MAX_TOOL_RESULT_FOR_CONTEXT {
        return content.to_string();
    }

    let mut enriched = content.to_string();
    let banner = format!(
        "\n\n{DECISION_TRAIL_BANNER} The Decision Trail below is harness metadata appended to \nthis tool result by V.E.L.O.C.I.T.Y.; it is NOT part of the file and must never be copied\ninto a file write. ──\n{DECISION_TRAIL_HEADER}\n  {}\n",
        file_verdict(&events)
    );
    if enriched.len() + banner.len() > MAX_TOOL_RESULT_FOR_CONTEXT {
        return content.to_string();
    }
    enriched.push_str(&banner);

    let mut added = 0usize;
    for event in signal.iter().take(8) {
        let outcome_tag = match event.outcome {
            EventOutcome::Success => "OK",
            EventOutcome::Failure => "FAIL",
            EventOutcome::Revert => "REVERT",
            EventOutcome::Pending => "?",
        };
        let mut entry = render_trail_entry(
            event.sequence,
            outcome_tag,
            &event.tool_name,
            &event.description,
        );
        if let Some(ctx) = &event.context {
            entry.push_str(&format!(" — {}", ctx));
        }
        if let Some(reason) = &event.failure_reason {
            entry.push_str(&format!(" (failed: {})", reason));
        }
        entry.push('\n');
        // Newest entries are already first, so stop at the point where the
        // remaining budget cannot fit one more.
        if enriched.len() + entry.len() > MAX_TOOL_RESULT_FOR_CONTEXT {
            break;
        }
        enriched.push_str(&entry);
        added += 1;
    }
    // Covers both reasons an entry can be absent: the budget ran out, and the
    // newest-first cap of 8.
    let omitted = signal.len().saturating_sub(added);
    if noise > 0 {
        let poll = format!(
            "  [noise] {} read-only poll(s) folded (no codebase change)\n",
            noise
        );
        if enriched.len() + poll.len() <= MAX_TOOL_RESULT_FOR_CONTEXT {
            enriched.push_str(&poll);
        }
    }
    if omitted > 0 {
        let note = format!(
            "  [trail] {} older decision(s) omitted to keep the file text usable\n",
            omitted
        );
        if enriched.len() + note.len() <= MAX_TOOL_RESULT_FOR_CONTEXT {
            enriched.push_str(&note);
        }
    }
    enriched
}

/// Remove a trailing decision-trail block that a model copied back into a
/// `write_file` payload. A live run rewrote a file straight from the enriched
/// `read_file` result and dragged the harness annotation into the file.
///
/// Deliberately conservative: it only strips when the header stands alone on
/// its line, at least one trail entry follows, and nothing but trail entries
/// follows it - so a source file merely mentioning the marker (this module
/// does) or prose that reuses it mid-content is never cut.
pub fn strip_decision_trail(content: &str) -> Option<String> {
    let header_at = content.rfind(DECISION_TRAIL_HEADER)?;
    let line_start = content[..header_at].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = content[header_at..]
        .find('\n')
        .map(|i| header_at + 1 + i)
        .unwrap_or(content.len());
    if content[line_start..line_end].trim() != DECISION_TRAIL_HEADER {
        return None;
    }
    let mut entries = 0;
    for line in content[line_end..].lines() {
        if line.trim().is_empty() {
            continue;
        }
        let t = line.trim_start();
        // Every trail line carries a bracketed marker: an event `[#seq]`, the
        // `[verdict]` summary, or the folded `[noise]` count. Anything else is
        // real content and stops the strip.
        if !(t.starts_with("[#") || t.starts_with("[verdict]") || t.starts_with("[noise]")) {
            return None;
        }
        entries += 1;
    }
    if entries == 0 {
        return None;
    }
    // Extend the cut back over the banner line when the model copied it too.
    let mut cut = line_start;
    if let Some(b) = content[..line_start].rfind(DECISION_TRAIL_BANNER) {
        if content[..b].rfind('\n').is_none_or(|n| n + 1 == b) {
            cut = b;
        }
    }
    let mut cleaned = content[..cut].to_string();
    // The enricher always appends exactly one blank line before the banner;
    // undo it so the original byte-ending of the file is preserved.
    if cleaned.ends_with("\n\n") {
        cleaned.truncate(cleaned.len() - 2);
    }
    Some(cleaned)
}

/// Build a compact *prior decision history* brief for a set of scope files from
/// the durable event store, meant for injection into an agent's first-attempt
/// prompt.  This is the read-back half of the memory loop: the worker inherits
/// past failures, shared-write conflicts and reverts on the exact files it is
/// about to touch, instead of blindly re-running attempts that already failed
/// in a prior session.  Read-only noise is excluded; only high-signal events
/// surface.  Returns an empty string when there is nothing worth spending
/// prompt budget on (so the caller can skip injection cleanly).
pub fn history_brief(root: &Path, scope_files: &[String], max_lines: usize) -> String {
    use std::collections::HashSet;
    let store = EventStore::open(root);
    let mut seen: HashSet<u64> = HashSet::new();
    let mut collected: Vec<(u64, String)> = Vec::new();

    for file in scope_files.iter().take(12) {
        let events = match store.query(Some(file), None, 20) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for ev in events {
            if !ev.is_high_signal() || !seen.insert(ev.sequence) {
                continue;
            }
            let mut line = format!(
                "[#{}] {} [{}] {}",
                ev.sequence,
                ev.outcome.label().to_uppercase(),
                ev.tool_name,
                ev.description
            );
            if let Some(reason) = &ev.failure_reason {
                line.push_str(&format!(" \u{2014} reason: {reason}"));
            }
            if let Some(ctx) = &ev.context {
                line.push_str(&format!(" \u{2014} why: {ctx}"));
            }
            collected.push((ev.sequence, line));
        }
    }

    if collected.is_empty() {
        return String::new();
    }
    collected.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    let body = collected
        .into_iter()
        .take(max_lines)
        .map(|(_, line)| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "PRIOR DECISION HISTORY for this scope (from the workspace event store). Failures and \nconflicts below already happened in past sessions \u{2014} do not repeat an approach that failed \nwithout changing the strategy; if a shared-write conflict is listed, the scope was already held \nby another agent.\n{body}"
    )
}

/// File sets that have actually collided in past shared-write conflicts.
/// Every `shared_write_conflict` event's affected files are unioned into
/// contention groups: files recorded together in one conflict belong to the
/// same group, and merging is transitive across events. The conflict-aware
/// scheduler uses the groups to serialize tasks whose declared scopes merely
/// *touch the same hot area* even when the paths never overlap textually —
/// semantic collisions the runtime mediator used to catch only after a wasted
/// worker start. Paths are normalized (forward slashes, lowercase); groups of
/// a single file are dropped because plain textual overlap already covers
/// them. Returns nothing when the store is missing or holds no conflicts.
pub fn conflict_file_groups(root: &Path) -> Vec<std::collections::HashSet<String>> {
    fn norm(p: &str) -> String {
        p.replace('\\', "/").trim_matches('/').to_lowercase()
    }
    let store = EventStore::open(root);
    let Ok(events) = store.load_all() else {
        return Vec::new();
    };
    let mut groups: Vec<std::collections::HashSet<String>> = Vec::new();
    // Newest first; a bounded scan keeps the per-tick cost flat even in long
    // histories — recent contention patterns are the ones that predict the
    // next session's collisions.
    for ev in events.iter().rev().take(2000) {
        let is_conflict = ev
            .metadata
            .as_ref()
            .and_then(|m| m.get("shared_write_conflict"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !is_conflict {
            continue;
        }
        let mut merged: std::collections::HashSet<String> = ev
            .affected_files
            .iter()
            .map(|f| norm(f))
            .filter(|f| !f.is_empty())
            .collect();
        if merged.is_empty() {
            continue;
        }
        // Union every existing group this event's file set intersects.
        let mut rest = std::mem::take(&mut groups);
        for g in rest.drain(..) {
            if g.iter().any(|f| merged.contains(f)) {
                merged.extend(g);
            } else {
                groups.push(g);
            }
        }
        groups.push(merged);
    }
    groups.retain(|g| g.len() > 1);
    groups
}

// ─── MCP tool handlers ──────────────────────────────────────────────────────

/// Handle `event_record` — create a new codebase event.
pub fn handle_event_record(root: &Path, arguments: &serde_json::Value) -> Result<String, String> {
    let tool_name = arguments["tool_name"].as_str().unwrap_or("manual");
    let description = arguments["description"]
        .as_str()
        .ok_or_else(|| "description is required".to_string())?;
    let context = arguments["context"].as_str().map(|s| s.to_string());
    let affected_files: Vec<String> = arguments["affected_files"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let merkle_before = arguments["merkle_root_before"]
        .as_str()
        .map(|s| s.to_string());
    let merkle_after = arguments["merkle_root_after"]
        .as_str()
        .map(|s| s.to_string());

    let store = EventStore::open(root);
    let seq = store.record(
        tool_name,
        description,
        merkle_before,
        merkle_after,
        context,
        affected_files,
    )?;

    Ok(serde_json::to_string(&json!({
        "success": true,
        "sequence": seq,
        "message": format!("Recorded event #{}", seq)
    }))
    .unwrap())
}

/// Handle `event_history` — query event history.
pub fn handle_event_history(root: &Path, arguments: &serde_json::Value) -> Result<String, String> {
    let store = EventStore::open(root);
    let file_filter = arguments["file"].as_str();
    let limit = arguments["limit"].as_u64().unwrap_or(20).min(200) as usize;
    let outcome_filter = arguments["outcome"].as_str().and_then(|s| match s {
        "success" => Some(EventOutcome::Success),
        "failure" => Some(EventOutcome::Failure),
        "revert" => Some(EventOutcome::Revert),
        "pending" => Some(EventOutcome::Pending),
        _ => None,
    });

    let events = store.query(file_filter, outcome_filter.as_ref(), limit)?;
    let total = store.count()?;

    let event_json: Vec<serde_json::Value> = events
        .iter()
        .map(|e| {
            json!({
                "seq": e.sequence,
                "timestamp_ms": e.timestamp_ms,
                "tool": e.tool_name,
                "description": e.description,
                "outcome": e.outcome.label(),
                "merkle_before": e.merkle_root_before,
                "merkle_after": e.merkle_root_after,
                "context": e.context,
                "failure_reason": e.failure_reason,
                "affected_files": e.affected_files,
            })
        })
        .collect();

    Ok(serde_json::to_string(&json!({
        "totalEvents": total,
        "returned": event_json.len(),
        "events": event_json
    }))
    .unwrap())
}

/// Handle `event_context` — get full detail for a single event.
pub fn handle_event_context(root: &Path, arguments: &serde_json::Value) -> Result<String, String> {
    let seq = arguments["sequence"]
        .as_u64()
        .ok_or_else(|| "sequence is required".to_string())?;
    let store = EventStore::open(root);
    match store.get(seq)? {
        Some(event) => Ok(serde_json::to_string(&json!({
            "found": true,
            "event": {
                "seq": event.sequence,
                "timestamp_ms": event.timestamp_ms,
                "tool": event.tool_name,
                "description": event.description,
                "outcome": event.outcome.label(),
                "merkle_before": event.merkle_root_before,
                "merkle_after": event.merkle_root_after,
                "context": event.context,
                "failure_reason": event.failure_reason,
                "affected_files": event.affected_files,
                "metadata": event.metadata,
            }
        }))
        .unwrap()),
        None => Ok(serde_json::to_string(&json!({
            "found": false,
            "sequence": seq,
            "message": format!("No event with sequence #{}", seq)
        }))
        .unwrap()),
    }
}

/// Handle `event_mark_outcome` — update the outcome of an existing event.
pub fn handle_event_mark_outcome(
    root: &Path,
    arguments: &serde_json::Value,
) -> Result<String, String> {
    let seq = arguments["sequence"]
        .as_u64()
        .ok_or_else(|| "sequence is required".to_string())?;
    let outcome_str = arguments["outcome"]
        .as_str()
        .ok_or_else(|| "outcome is required (success, failure, revert, pending)".to_string())?;
    let outcome = match outcome_str {
        "success" => EventOutcome::Success,
        "failure" => EventOutcome::Failure,
        "revert" => EventOutcome::Revert,
        "pending" => EventOutcome::Pending,
        other => {
            return Err(format!(
                "Unknown outcome '{}'. Expected: success, failure, revert, pending",
                other
            ))
        }
    };
    let failure_reason = arguments["failure_reason"].as_str().map(|s| s.to_string());

    let store = EventStore::open(root);
    let updated = store.mark_outcome(seq, outcome, failure_reason)?;

    Ok(serde_json::to_string(&json!({
        "success": updated,
        "sequence": seq,
        "outcome": outcome_str,
        "message": if updated {
            format!("Event #{} marked as {}", seq, outcome_str)
        } else {
            format!("No event with sequence #{}", seq)
        }
    }))
    .unwrap())
}

/// Handle `event_timeline` — chronological view of all events with state
/// transitions, showing the codebase's journey over time.
pub fn handle_event_timeline(root: &Path, arguments: &serde_json::Value) -> Result<String, String> {
    let store = EventStore::open(root);
    let limit = arguments["limit"].as_u64().unwrap_or(50).min(500) as usize;
    let events = store.load_all()?;

    let total = events.len();
    let timeline: Vec<serde_json::Value> = events
        .iter()
        .rev()
        .take(limit)
        .map(|e| {
            let mut entry = serde_json::Map::new();
            entry.insert("seq".into(), json!(e.sequence));
            entry.insert("timestamp_ms".into(), json!(e.timestamp_ms));
            entry.insert("tool".into(), json!(e.tool_name));
            entry.insert("description".into(), json!(e.description));
            entry.insert("outcome".into(), json!(e.outcome.label()));
            if let Some(before) = &e.merkle_root_before {
                entry.insert("root_before".into(), json!(before));
            }
            if let Some(after) = &e.merkle_root_after {
                entry.insert("root_after".into(), json!(after));
            }
            if let Some(ctx) = &e.context {
                entry.insert("context".into(), json!(ctx));
            }
            if let Some(reason) = &e.failure_reason {
                entry.insert("failure_reason".into(), json!(reason));
            }
            if !e.affected_files.is_empty() {
                entry.insert("files".into(), json!(e.affected_files));
            }
            serde_json::Value::Object(entry)
        })
        .collect();

    Ok(serde_json::to_string(&json!({
        "totalEvents": total,
        "returned": timeline.len(),
        "timeline": timeline
    }))
    .unwrap())
}

/// Handle `event_attach_context` — attach agent reasoning (the *why*) to an
/// existing event after the fact.
pub fn handle_event_attach_context(
    root: &Path,
    arguments: &serde_json::Value,
) -> Result<String, String> {
    let seq = arguments["sequence"]
        .as_u64()
        .ok_or_else(|| "sequence is required".to_string())?;
    let context = arguments["context"]
        .as_str()
        .ok_or_else(|| "context is required".to_string())?;

    let store = EventStore::open(root);
    let updated = store.update_context(seq, context)?;

    Ok(serde_json::to_string(&json!({
        "success": updated,
        "sequence": seq,
        "message": if updated {
            format!("Context attached to event #{}", seq)
        } else {
            format!("No event with sequence #{}", seq)
        }
    }))
    .unwrap())
}

// ─── Auto-event creation from dispatch ──────────────────────────────────────

/// Record a codebase event automatically after a tool call.
/// Called from dispatch.rs after successful tool execution.
/// The `context` parameter carries the agent's stated reason for the change,
/// captured at call time so it cannot be forgotten.
pub fn auto_record_tool_event(
    root: &Path,
    tool_name: &str,
    merkle_before: Option<u64>,
    merkle_after: Option<u64>,
    description: &str,
    context: Option<&str>,
) {
    let store = EventStore::open(root);
    let _ = store.record(
        tool_name,
        description,
        merkle_before.map(|r| format!("{:016x}", r)),
        merkle_after.map(|r| format!("{:016x}", r)),
        context.map(|s| s.to_string()),
        Vec::new(),
    );
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn open_store() -> (TempDir, EventStore) {
        let dir = TempDir::new().unwrap();
        let store = EventStore::open(dir.path());
        (dir, store)
    }

    #[test]
    fn record_and_load_single_event() {
        let (_dir, store) = open_store();
        let seq = store
            .record(
                "write_file",
                "Added auth module",
                Some("aaaa".into()),
                Some("bbbb".into()),
                Some("Needed OAuth2 support".into()),
                vec!["src/auth.rs".into()],
            )
            .unwrap();
        assert_eq!(seq, 1);

        let events = store.load_all().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].tool_name, "write_file");
        assert_eq!(events[0].description, "Added auth module");
        assert_eq!(events[0].merkle_root_before, Some("aaaa".into()));
        assert_eq!(events[0].merkle_root_after, Some("bbbb".into()));
        assert_eq!(events[0].context, Some("Needed OAuth2 support".into()));
        assert_eq!(events[0].outcome, EventOutcome::Pending);
        assert_eq!(events[0].affected_files, vec!["src/auth.rs"]);
    }

    #[test]
    fn sequence_numbers_are_monotonic() {
        let (_dir, store) = open_store();
        let s1 = store.record("t", "a", None, None, None, vec![]).unwrap();
        let s2 = store.record("t", "b", None, None, None, vec![]).unwrap();
        let s3 = store.record("t", "c", None, None, None, vec![]).unwrap();
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
        assert_eq!(s3, 3);
    }

    #[test]
    fn mark_outcome_updates_event() {
        let (_dir, store) = open_store();
        store.record("t", "desc", None, None, None, vec![]).unwrap();
        let updated = store
            .mark_outcome(1, EventOutcome::Failure, Some("broke tests".into()))
            .unwrap();
        assert!(updated);

        let event = store.get(1).unwrap().unwrap();
        assert_eq!(event.outcome, EventOutcome::Failure);
        assert_eq!(event.failure_reason, Some("broke tests".into()));
    }

    #[test]
    fn mark_outcome_nonexistent_returns_false() {
        let (_dir, store) = open_store();
        let updated = store
            .mark_outcome(999, EventOutcome::Success, None)
            .unwrap();
        assert!(!updated);
    }

    #[test]
    fn query_filters_by_file() {
        let (_dir, store) = open_store();
        store
            .record("t", "auth", None, None, None, vec!["src/auth.rs".into()])
            .unwrap();
        store
            .record("t", "db", None, None, None, vec!["src/db.rs".into()])
            .unwrap();

        let auth_events = store.query(Some("auth"), None, 10).unwrap();
        assert_eq!(auth_events.len(), 1);
        assert_eq!(auth_events[0].description, "auth");
    }

    #[test]
    fn query_filters_by_outcome() {
        let (_dir, store) = open_store();
        store.record("t", "a", None, None, None, vec![]).unwrap();
        store.record("t", "b", None, None, None, vec![]).unwrap();
        store.mark_outcome(1, EventOutcome::Success, None).unwrap();
        store
            .mark_outcome(2, EventOutcome::Failure, Some("oops".into()))
            .unwrap();

        let failures = store.query(None, Some(&EventOutcome::Failure), 10).unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].description, "b");
    }

    #[test]
    fn query_respects_limit() {
        let (_dir, store) = open_store();
        for i in 0..10 {
            store
                .record("t", &format!("event_{}", i), None, None, None, vec![])
                .unwrap();
        }
        let limited = store.query(None, None, 3).unwrap();
        assert_eq!(limited.len(), 3);
        // Newest first
        assert_eq!(limited[0].sequence, 10);
        assert_eq!(limited[2].sequence, 8);
    }

    #[test]
    fn by_merkle_root_finds_matching_events() {
        let (_dir, store) = open_store();
        store
            .record(
                "t",
                "a",
                Some("aaaa".into()),
                Some("bbbb".into()),
                None,
                vec![],
            )
            .unwrap();
        store
            .record(
                "t",
                "b",
                Some("bbbb".into()),
                Some("cccc".into()),
                None,
                vec![],
            )
            .unwrap();
        store
            .record(
                "t",
                "c",
                Some("cccc".into()),
                Some("dddd".into()),
                None,
                vec![],
            )
            .unwrap();

        // "bbbb" appears as merkle_after in event 1 and merkle_before in event 2
        let found = store.by_merkle_root("bbbb").unwrap();
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn get_nonexistent_returns_none() {
        let (_dir, store) = open_store();
        assert!(store.get(42).unwrap().is_none());
    }

    #[test]
    fn count_reflects_stored_events() {
        let (_dir, store) = open_store();
        assert_eq!(store.count().unwrap(), 0);
        store.record("t", "a", None, None, None, vec![]).unwrap();
        assert_eq!(store.count().unwrap(), 1);
        store.record("t", "b", None, None, None, vec![]).unwrap();
        assert_eq!(store.count().unwrap(), 2);
    }

    #[test]
    fn empty_store_returns_empty_results() {
        let (_dir, store) = open_store();
        assert_eq!(store.load_all().unwrap().len(), 0);
        assert_eq!(store.query(None, None, 10).unwrap().len(), 0);
        assert_eq!(store.count().unwrap(), 0);
    }

    #[test]
    fn event_outcome_labels() {
        assert_eq!(EventOutcome::Success.label(), "success");
        assert_eq!(EventOutcome::Failure.label(), "failure");
        assert_eq!(EventOutcome::Revert.label(), "revert");
        assert_eq!(EventOutcome::Pending.label(), "pending");
    }

    #[test]
    fn handle_event_record_and_history() {
        let dir = TempDir::new().unwrap();
        let args = json!({
            "description": "Added caching layer",
            "context": "Response times were too slow",
            "tool_name": "write_file",
            "affected_files": ["src/cache.rs"],
            "merkle_root_before": "000000000000aaaa",
            "merkle_root_after": "000000000000bbbb"
        });
        let result = handle_event_record(dir.path(), &args).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["sequence"], 1);

        let history = handle_event_history(dir.path(), &json!({})).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&history).unwrap();
        assert_eq!(parsed["totalEvents"], 1);
        assert_eq!(parsed["returned"], 1);
    }

    #[test]
    fn handle_event_context_found_and_not_found() {
        let dir = TempDir::new().unwrap();
        let args = json!({
            "description": "Test event",
            "context": "Testing"
        });
        handle_event_record(dir.path(), &args).unwrap();

        // Found
        let ctx = handle_event_context(dir.path(), &json!({"sequence": 1})).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&ctx).unwrap();
        assert_eq!(parsed["found"], true);
        assert_eq!(parsed["event"]["description"], "Test event");

        // Not found
        let ctx = handle_event_context(dir.path(), &json!({"sequence": 99})).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&ctx).unwrap();
        assert_eq!(parsed["found"], false);
    }

    #[test]
    fn handle_event_mark_outcome_success_and_failure() {
        let dir = TempDir::new().unwrap();
        handle_event_record(dir.path(), &json!({"description": "Try Z"})).unwrap();

        // Mark as failure
        let result = handle_event_mark_outcome(
            dir.path(),
            &json!({"sequence": 1, "outcome": "failure", "failure_reason": "Race condition"}),
        )
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["success"], true);

        // Verify
        let ctx = handle_event_context(dir.path(), &json!({"sequence": 1})).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&ctx).unwrap();
        assert_eq!(parsed["event"]["outcome"], "failure");
        assert_eq!(parsed["event"]["failure_reason"], "Race condition");
    }

    #[test]
    fn auto_record_tool_event_creates_event() {
        let dir = TempDir::new().unwrap();
        auto_record_tool_event(
            dir.path(),
            "index_workspace",
            Some(0xaaaa),
            Some(0xbbbb),
            "Indexed lib/expect",
            Some("Initial workspace setup"),
        );
        let store = EventStore::open(dir.path());
        assert_eq!(store.count().unwrap(), 1);
        let event = store.get(1).unwrap().unwrap();
        assert_eq!(event.merkle_root_before, Some("000000000000aaaa".into()));
        assert_eq!(event.merkle_root_after, Some("000000000000bbbb".into()));
        assert_eq!(event.context, Some("Initial workspace setup".into()));
    }

    #[test]
    fn update_context_attaches_reasoning() {
        let (_dir, store) = open_store();
        store
            .record(
                "write_file",
                "Added cache",
                None,
                None,
                None,
                vec!["cache.rs".into()],
            )
            .unwrap();
        // Initially no context
        assert!(store.get(1).unwrap().unwrap().context.is_none());
        // Attach context
        let updated = store
            .update_context(1, "Response times were 2s, needed <200ms")
            .unwrap();
        assert!(updated);
        assert_eq!(
            store.get(1).unwrap().unwrap().context,
            Some("Response times were 2s, needed <200ms".into())
        );
    }

    #[test]
    fn update_context_nonexistent_returns_false() {
        let (_dir, store) = open_store();
        assert!(!store.update_context(999, "nope").unwrap());
    }

    #[test]
    fn enrich_read_response_appends_trail() {
        let (dir, store) = open_store();
        store
            .record(
                "write_file",
                "Added auth module",
                None,
                None,
                Some("Needed OAuth2".into()),
                vec!["src/auth.rs".into()],
            )
            .unwrap();
        store.mark_outcome(1, EventOutcome::Success, None).unwrap();

        let content = "fn authenticate() { /* ... */ }";
        let enriched = enrich_read_response(dir.path(), "src/auth.rs", content);
        assert!(enriched.contains("Decision Trail"));
        assert!(enriched.contains("Added auth module"));
        assert!(enriched.contains("OK"));
        assert!(enriched.contains("Needed OAuth2"));
    }

    #[test]
    fn enrich_read_response_no_events_returns_unchanged() {
        let (dir, _store) = open_store();
        let content = "fn main() {}";
        let enriched = enrich_read_response(dir.path(), "main.rs", content);
        assert_eq!(enriched, content);
    }

    #[test]
    fn strip_decision_trail_round_trips_enriched_content() {
        // A model that copies an enriched read result back into write_file
        // must get the pristine bytes back, whatever the file's line ending.
        let (dir, store) = open_store();
        store
            .record(
                "write_file",
                "Touched auth",
                None,
                None,
                None,
                vec!["auth.rs".into()],
            )
            .unwrap();
        store.mark_outcome(1, EventOutcome::Success, None).unwrap();
        for content in ["fn a() {}\n", "no trailing newline", "x"] {
            let enriched = enrich_read_response(dir.path(), "auth.rs", content);
            assert_ne!(
                enriched, content,
                "enrich must change content when events exist"
            );
            assert!(
                enriched.contains("must never be copied"),
                "banner must label metadata"
            );
            assert_eq!(strip_decision_trail(&enriched).as_deref(), Some(content));
        }
    }

    #[test]
    fn strip_decision_trail_leaves_unrelated_content_alone() {
        // In-prose mentions must never be cut - this module's own source
        // contains the marker in doc comments and string literals.
        assert!(strip_decision_trail("see `── Decision Trail ──` in docs").is_none());
        // Header alone, or followed by non-trail prose, is not a copied trail.
        assert!(strip_decision_trail("── Decision Trail ──\n").is_none());
        assert!(strip_decision_trail("── Decision Trail ──\nintentional prose\n").is_none());
        // A trail block that does not end the content is not stripped.
        assert!(
            strip_decision_trail("── Decision Trail ──\n  [#1] OK [read_file] x\nreal tail\n")
                .is_none()
        );
        assert!(strip_decision_trail("").is_none());
    }

    #[test]
    fn strip_decision_trail_handles_copy_without_banner() {
        // Older/oblique copies may include the header and entries but not the
        // banner line; the byte-ending of the original body is preserved.
        let copied = "body text\n\n── Decision Trail ──\n  [#1] OK [read_file] Read x\n";
        assert_eq!(strip_decision_trail(copied).as_deref(), Some("body text"));
    }

    #[test]
    fn handle_event_timeline_returns_chronological_order() {
        let dir = TempDir::new().unwrap();
        handle_event_record(dir.path(), &json!({"description": "First"})).unwrap();
        handle_event_record(dir.path(), &json!({"description": "Second"})).unwrap();
        handle_event_record(dir.path(), &json!({"description": "Third"})).unwrap();

        let result = handle_event_timeline(dir.path(), &json!({})).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["totalEvents"], 3);
        // Newest first
        assert_eq!(parsed["timeline"][0]["description"], "Third");
        assert_eq!(parsed["timeline"][2]["description"], "First");
    }

    #[test]
    fn handle_event_attach_context_works() {
        let dir = TempDir::new().unwrap();
        handle_event_record(dir.path(), &json!({"description": "Try caching"})).unwrap();

        let result = handle_event_attach_context(
            dir.path(),
            &json!({"sequence": 1, "context": "Users complained about slow load times"}),
        )
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["success"], true);

        // Verify context is now present
        let ctx = handle_event_context(dir.path(), &json!({"sequence": 1})).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&ctx).unwrap();
        assert_eq!(
            parsed["event"]["context"],
            "Users complained about slow load times"
        );
    }

    // ── Robustness: corrupt lines ─────────────────────────────────────────

    #[test]
    fn load_all_skips_totally_malformed_line() {
        let (dir, store) = open_store();
        store
            .record("t1", "good first", None, None, None, vec![])
            .unwrap();
        // Manually inject a totally-garbage line, then append another valid
        // event via the normal path.
        let path = dir
            .path()
            .join(".velocity")
            .join("events")
            .join("events.jsonl");
        let mut raw = fs::read_to_string(&path).unwrap();
        raw.push_str("this-is-not-json-at-all\n");
        fs::write(&path, raw).unwrap();
        store
            .record("t2", "good second", None, None, None, vec![])
            .unwrap();

        let events = store.load_all().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].description, "good first");
        assert_eq!(events[1].description, "good second");
    }

    #[test]
    fn legacy_event_line_without_actor_loads_as_service() {
        // INV-4: a pre-refactor events.jsonl line carries no `actor` field, yet
        // must still deserialize, defaulting to an unattributable Service
        // principal so the decision trail keeps rendering unchanged.
        let legacy = r#"{"sequence":1,"timestamp_ms":100,"tool_name":"write_file","description":"old","merkle_root_before":null,"merkle_root_after":null,"context":null,"outcome":"success","failure_reason":null,"affected_files":[]}"#;
        let ev: CodebaseEvent = serde_json::from_str(legacy).unwrap();
        assert_eq!(
            ev.actor.kind,
            crate::automation::principal::PrincipalKind::Service
        );
        assert_eq!(ev.description, "old");
        assert_eq!(ev.outcome, EventOutcome::Success);
    }

    #[test]
    fn load_all_recovers_concated_objects() {
        let (dir, store) = open_store();
        // Simulate a `}{` collision: two serialized records on one physical line.
        let a = serde_json::to_string(&CodebaseEvent {
            sequence: 1,
            timestamp_ms: 100,
            tool_name: "grep_search".into(),
            description: "first half".into(),
            merkle_root_before: None,
            merkle_root_after: None,
            context: None,
            outcome: EventOutcome::Pending,
            failure_reason: None,
            affected_files: vec![],
            metadata: None,
            actor: crate::automation::principal::Principal::default(),
        })
        .unwrap();
        let b = serde_json::to_string(&CodebaseEvent {
            sequence: 2,
            timestamp_ms: 200,
            tool_name: "write_file".into(),
            description: "second half".into(),
            merkle_root_before: None,
            merkle_root_after: None,
            context: None,
            outcome: EventOutcome::Pending,
            failure_reason: None,
            affected_files: vec![],
            metadata: None,
            actor: crate::automation::principal::Principal::default(),
        })
        .unwrap();
        let path = dir
            .path()
            .join(".velocity")
            .join("events")
            .join("events.jsonl");
        fs::write(&path, format!("{a}{b}\n")).unwrap();

        let events = store.load_all().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].description, "first half");
        assert_eq!(events[1].description, "second half");
    }

    #[test]
    fn append_event_writes_line_atomically() {
        // The record path must produce exactly one line per event.  Regression
        // guard for the previous `writeln!` interleaving bug.
        let (dir, store) = open_store();
        for i in 0..20 {
            store
                .record("t", &format!("desc {i}"), None, None, None, vec![])
                .unwrap();
        }
        let path = dir
            .path()
            .join(".velocity")
            .join("events")
            .join("events.jsonl");
        let raw = fs::read_to_string(&path).unwrap();
        assert_eq!(raw.lines().count(), 20);
        for line in raw.lines() {
            assert!(
                line.starts_with('{') && line.ends_with('}'),
                "line shape: {line}"
            );
            // Every line must parse as a single event object.
            let ev: CodebaseEvent = serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("line did not parse cleanly: {e}"));
            assert!(ev.description.starts_with("desc "));
        }
    }

    // ── Decisive history: conflict / stale-plan / outcome recording ────────

    #[test]
    fn record_shared_write_conflict_tags_metadata_and_is_signal() {
        let (_dir, store) = open_store();
        let seq = store
            .record_shared_write_conflict(
                "worker_assignment",
                "worker 'X' blocked",
                "scope_overlap",
                "task-1",
                "task-2",
                vec!["src/lib.rs".into()],
                "MEDIATION CONTRACT: SCOPE OVERLAP",
            )
            .unwrap();
        let ev = store.get(seq).unwrap().unwrap();
        assert_eq!(ev.outcome, EventOutcome::Failure);
        assert!(ev.is_high_signal());
        let md = ev.metadata.expect("conflict must carry metadata");
        assert_eq!(md["shared_write_conflict"], json!(true));
        assert_eq!(md["conflict_kind"], json!("scope_overlap"));
        assert_eq!(md["existing_agent"], json!("task-1"));
        assert_eq!(md["requested_agent"], json!("task-2"));
    }

    #[test]
    fn record_stale_plan_and_task_outcome() {
        let (_dir, store) = open_store();
        let seq = store
            .record_stale_plan(
                "worker 'Y' assignment discarded",
                "000000000000aaaa",
                "000000000000bbbb",
                vec!["src/a.rs".into()],
            )
            .unwrap();
        let ev = store.get(seq).unwrap().unwrap();
        assert_eq!(ev.outcome, EventOutcome::Failure);
        assert!(ev.is_high_signal());
        assert_eq!(
            ev.metadata.unwrap()["planned_site_map_root"],
            json!("000000000000aaaa")
        );

        let seq2 = store
            .record_task_outcome(
                42,
                "worker 'Z' finished",
                EventOutcome::Success,
                "all good",
                vec!["src/z.rs".into()],
            )
            .unwrap();
        let ev2 = store.get(seq2).unwrap().unwrap();
        assert_eq!(ev2.outcome, EventOutcome::Success);
        assert!(ev2.is_high_signal());
        assert_eq!(ev2.metadata.unwrap()["task_id"], json!(42));
    }

    // ── Shared-write concurrency on the history file itself ────────────────

    #[test]
    fn concurrent_appends_are_sequence_unique_and_monotonic() {
        // The literal "shared write" on the durable history: many worker
        // threads append through one process. APPEND_LOCK must serialize the
        // load_all -> append read-modify-write so no two events share a
        // sequence and none is lost to a `}{` collision.
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        let writers = 8;
        let per_writer = 10;
        let mut handles = Vec::new();
        for w in 0..writers {
            let root = root.clone();
            handles.push(std::thread::spawn(move || {
                let store = EventStore::open(&root);
                for i in 0..per_writer {
                    store
                        .record(
                            "write_file",
                            &format!("writer {w} event {i}"),
                            None,
                            None,
                            None,
                            vec![],
                        )
                        .unwrap();
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        let store = EventStore::open(&root);
        let events = store.load_all().unwrap();
        assert_eq!(events.len(), writers * per_writer, "no event lost");
        let mut seqs: Vec<u64> = events.iter().map(|e| e.sequence).collect();
        seqs.sort_unstable();
        let expected: Vec<u64> = (1..=(writers * per_writer) as u64).collect();
        assert_eq!(seqs, expected, "sequences must be unique and dense");

        // Every physical line must be a single, cleanly-parseable object (no
        // interleaved `}{` fragments survived the concurrent writers).
        let path = root.join(".velocity").join("events").join("events.jsonl");
        let raw = fs::read_to_string(&path).unwrap();
        assert_eq!(raw.lines().count(), writers * per_writer);
        for line in raw.lines() {
            serde_json::from_str::<CodebaseEvent>(line)
                .unwrap_or_else(|e| panic!("torn line survived: {e}"));
        }
    }

    // ── Signal-first trail + verdict ───────────────────────────────────────

    #[test]
    fn read_only_events_are_low_signal_and_folded() {
        let (_dir, store) = open_store();
        store
            .record("gui_get_state", "poll ui", None, None, None, vec![])
            .unwrap();
        store.mark_outcome(1, EventOutcome::Success, None).unwrap();
        let read = store.get(1).unwrap().unwrap();
        assert!(!read.is_high_signal(), "gui polling must be noise");

        store
            .record(
                "write_file",
                "real change",
                Some("aaaa".into()),
                Some("bbbb".into()),
                None,
                vec![],
            )
            .unwrap();
        let write = store.get(2).unwrap().unwrap();
        assert!(write.is_high_signal(), "a root transition is signal");
    }

    #[test]
    fn enrich_read_response_leads_with_verdict_and_folds_noise() {
        let (dir, store) = open_store();
        store
            .record(
                "write_file",
                "Added auth module",
                Some("aaaa".into()),
                Some("bbbb".into()),
                Some("Needed OAuth2".into()),
                vec!["src/auth.rs".into()],
            )
            .unwrap();
        store.mark_outcome(1, EventOutcome::Success, None).unwrap();
        // Bury the signal under read-only noise on the same file.
        for i in 0..6 {
            store
                .record(
                    "read_file",
                    &format!("poll {i}"),
                    None,
                    None,
                    None,
                    vec!["src/auth.rs".into()],
                )
                .unwrap();
            store
                .mark_outcome(2 + i, EventOutcome::Success, None)
                .unwrap();
        }

        let enriched = enrich_read_response(dir.path(), "src/auth.rs", "fn authenticate() {}");
        assert!(
            enriched.contains("[verdict]"),
            "trail must lead with verdict"
        );
        assert!(enriched.contains("Added auth module"), "signal surfaced");
        assert!(
            enriched.contains("read-only poll(s) folded"),
            "noise folded into a count"
        );
        // The individual noise entries must NOT be listed verbatim.
        assert!(
            !enriched.contains("[read_file] poll 3"),
            "folded noise should not leak as entries"
        );
        // Round-trip: stripping recovers the pristine body.
        assert_eq!(
            strip_decision_trail(&enriched).as_deref(),
            Some("fn authenticate() {}")
        );
    }

    #[test]
    fn enrich_read_response_adds_nothing_to_a_result_that_is_already_elided() {
        // A payload at or past the threshold is replaced by a stub next turn no
        // matter what else it carries, so a trail there only makes the stub
        // longer and costs the dispatch tokens for metadata nobody reads.
        let (dir, store) = open_store();
        store
            .record(
                "write_file",
                "Added auth module",
                None,
                None,
                None,
                vec!["src/auth.rs".into()],
            )
            .unwrap();
        store.mark_outcome(1, EventOutcome::Success, None).unwrap();

        let whole = "x".repeat(MAX_TOOL_RESULT_FOR_CONTEXT);
        assert_eq!(
            enrich_read_response(dir.path(), "src/auth.rs", &whole),
            whole,
            "an already-elided read must be handed back untouched"
        );

        // Just below it, the same file does get the trail — the rule is a
        // threshold, not a ban on annotating long reads.
        let nearly = "y".repeat(whole.len() - 900);
        let enriched = enrich_read_response(dir.path(), "src/auth.rs", &nearly);
        assert!(
            enriched.contains(DECISION_TRAIL_HEADER),
            "a read with room left must still be annotated"
        );
        assert!(enriched.len() <= MAX_TOOL_RESULT_FOR_CONTEXT);
    }

    #[test]
    fn file_verdict_counts_conflicts_and_flags_latest_failure() {
        let (_dir, store) = open_store();
        store
            .record_shared_write_conflict(
                "worker_assignment",
                "blocked",
                "direct_line",
                "task-1",
                "task-2",
                vec!["src/x.rs".into()],
                "MEDIATION CONTRACT",
            )
            .unwrap();
        store
            .record(
                "write_file",
                "landed change",
                Some("aaaa".into()),
                Some("bbbb".into()),
                None,
                vec!["src/x.rs".into()],
            )
            .unwrap();
        store.mark_outcome(2, EventOutcome::Success, None).unwrap();

        let events = store.load_all().unwrap();
        let verdict = file_verdict(&events);
        assert!(verdict.starts_with("[verdict]"));
        assert!(verdict.contains("1 shared-write conflict(s)"));
        assert!(verdict.contains("1 change(s) landed"));
    }

    #[test]
    fn history_brief_surfaces_failures_and_conflicts_and_skips_noise() {
        // The memory loop's read-back half: a worker's first attempt must
        // inherit past failures and shared-write conflicts for its scope,
        // while read-only polling stays filtered out.
        let (dir, store) = open_store();
        store
            .record(
                "read_file",
                "poll file",
                None,
                None,
                None,
                vec!["src/a.rs".into()],
            )
            .unwrap();
        store.mark_outcome(1, EventOutcome::Success, None).unwrap();
        store
            .record_with_outcome(
                "write_file",
                "Tried caching",
                None,
                None,
                Some("latency win".into()),
                vec!["src/a.rs".into()],
                EventOutcome::Failure,
                Some("broke tests".into()),
            )
            .unwrap();
        store
            .record_shared_write_conflict(
                "worker_assignment",
                "worker blocked",
                "scope_overlap",
                "task-1",
                "task-2",
                vec!["src/a.rs".into()],
                "MEDIATION CONTRACT",
            )
            .unwrap();

        let brief = history_brief(dir.path(), &["src/a.rs".to_string()], 12);
        assert!(brief.contains("PRIOR DECISION HISTORY"));
        assert!(brief.contains("Tried caching"), "failure must surface");
        assert!(brief.contains("broke tests"), "failure reason required");
        assert!(brief.contains("worker blocked"), "conflict must surface");
        assert!(!brief.contains("poll file"), "read-only noise excluded");
        // Newest-first: the conflict (#3) precedes the failure (#2) in body.
        let conflict_at = brief.find("worker blocked").unwrap();
        let failure_at = brief.find("Tried caching").unwrap();
        assert!(conflict_at < failure_at, "brief must be newest-first");

        // A scope with no high-signal history yields an empty brief so the
        // caller skips injection without burning prompt budget.
        assert!(history_brief(dir.path(), &["src/untouched.rs".to_string()], 12).is_empty());
    }

    #[test]
    fn conflict_file_groups_union_transitively_and_drop_singletons() {
        let (dir, store) = open_store();
        // Two multi-file conflicts sharing one file collapse into a single
        // contention group (transitive merge).
        store
            .record_shared_write_conflict(
                "worker_assignment",
                "db collision",
                "scope_overlap",
                "task-1",
                "task-2",
                vec!["src/db/pool.rs".into(), "src/db/schema.rs".into()],
                "c",
            )
            .unwrap();
        store
            .record_shared_write_conflict(
                "worker_assignment",
                "cache collision",
                "semantic",
                "task-3",
                "task-4",
                vec!["src/db/schema.rs".into(), "src/cache.rs".into()],
                "c",
            )
            .unwrap();
        // A single-file conflict adds nothing textual overlap wouldn't catch.
        store
            .record_shared_write_conflict(
                "worker_assignment",
                "file collision",
                "direct_line",
                "task-5",
                "task-6",
                vec!["README.md".into()],
                "c",
            )
            .unwrap();
        // Ordinary events never join a group.
        store
            .record(
                "write_file",
                "plain edit",
                None,
                None,
                None,
                vec!["src/x.rs".into()],
            )
            .unwrap();

        let groups = conflict_file_groups(dir.path());
        assert_eq!(groups.len(), 1, "one merged contention group");
        assert_eq!(groups[0].len(), 3, "pool + schema + cache");
        assert!(groups[0].contains("src/db/pool.rs"));
        assert!(groups[0].contains("src/cache.rs"));
    }

    #[test]
    fn conflict_file_groups_quiet_when_no_conflicts_recorded() {
        let (dir, store) = open_store();
        store
            .record(
                "write_file",
                "edit",
                None,
                None,
                None,
                vec!["src/a.rs".into()],
            )
            .unwrap();
        assert!(conflict_file_groups(dir.path()).is_empty());
        assert!(conflict_file_groups(std::path::Path::new("nonexistent-workspace")).is_empty());
    }

    // ── the append path, and the bound on the log it writes ─────────────

    /// Append `n` events and return the sequence numbers the store handed out.
    fn record_n(store: &EventStore, n: usize) -> Vec<u64> {
        (0..n)
            .map(|i| {
                store
                    .record(
                        "write_file",
                        &format!("change {i}"),
                        None,
                        None,
                        None,
                        vec![format!("src/f{i}.rs")],
                    )
                    .unwrap()
            })
            .collect()
    }

    fn events_path_of(dir: &Path) -> PathBuf {
        dir.join(".velocity").join("events").join("events.jsonl")
    }

    #[test]
    fn recording_an_event_does_not_reread_the_log_to_number_itself() {
        let (_dir, store) = open_store();
        let before = full_log_scans();
        let sequences = record_n(&store, 40);
        assert_eq!(
            full_log_scans() - before,
            0,
            "every append re-read the whole log to find the highest sequence number: \
             40 events cost 40 full reads, and 40,000 would cost a billion records parsed"
        );
        assert_eq!(sequences.first().copied(), Some(1));
        assert_eq!(sequences.last().copied(), Some(40));
        // The counter measures the thing it claims: a read that should scan does.
        store.load_all().unwrap();
        assert_eq!(full_log_scans() - before, 1, "load_all is the scan");
    }

    #[test]
    fn a_half_written_newest_record_does_not_steal_the_next_sequence() {
        let (dir, store) = open_store();
        record_n(&store, 2);
        // What a killed writer leaves behind: bytes of a record with no newline
        // and no end. Its sequence field is readable and it is not a record.
        fs::OpenOptions::new()
            .append(true)
            .open(events_path_of(dir.path()))
            .unwrap()
            .write_all(b"{\"sequence\":9,\"context\":\"half-writt")
            .unwrap();
        let third = store
            .record("write_file", "third", None, None, None, vec![])
            .unwrap();
        assert_eq!(
            third, 3,
            "the store numbered the next event off a fragment that may never have been a record"
        );
        let events = store.load_all().unwrap();
        let sequences: Vec<u64> = events.iter().map(|e| e.sequence).collect();
        assert_eq!(
            sequences,
            vec![1, 2, 3],
            "the append was glued onto the unterminated fragment, so the record that \
             was still intact is the one the log lost"
        );
    }

    #[test]
    fn a_newest_record_larger_than_the_window_falls_back_to_the_full_read() {
        let (dir, store) = open_store();
        record_n(&store, 1);
        // A record too long for the tail window: the window is then one partial
        // line and holds no complete record to read a number off.
        use std::io::Write;
        fs::OpenOptions::new()
            .append(true)
            .open(events_path_of(dir.path()))
            .unwrap()
            .write_all(b"y".repeat(70_000).as_slice())
            .unwrap();
        assert_eq!(
            store.next_sequence_from_tail(),
            None,
            "the tail held no complete record, so guessing from it was not available"
        );
        let before = full_log_scans();
        assert_eq!(
            store
                .record("write_file", "second", None, None, None, vec![])
                .unwrap(),
            2,
            "the fallback has to produce the same number the full read always did"
        );
        assert_eq!(full_log_scans() - before, 1, "and it has to be the scan");
    }

    #[test]
    fn retention_keeps_a_contiguous_suffix_of_newest_events() {
        let (_dir, store) = open_store();
        let store = store.with_max_bytes(4_096);
        let recorded = record_n(&store, 320);
        let events = store.load_all().unwrap();
        let sequences: Vec<u64> = events.iter().map(|e| e.sequence).collect();
        assert!(
            events.len() < recorded.len(),
            "the log never shrank under a 4 KiB bound ({} events kept)",
            events.len()
        );
        assert!(
            events.len() >= RETENTION_FLOOR_EVENTS,
            "pruning went below the floor that is supposed to protect the trail: {}",
            events.len()
        );
        assert_eq!(
            sequences.last().copied(),
            recorded.last().copied(),
            "the newest event is the one an agent asks about; it cannot be the first thing dropped"
        );
        let contiguous = sequences.windows(2).all(|pair| pair[1] == pair[0] + 1);
        assert!(
            contiguous,
            "a retained window with holes in it: {sequences:?}"
        );
        // With a bound this small the floor, not the bound, is what stops the
        // prune: 256 records of this size are already far past 4 KiB, so the
        // store keeps exactly the floor — the newest ones — instead of emptying
        // the trail to obey a number that cannot be honoured.
        assert_eq!(events.len(), RETENTION_FLOOR_EVENTS);
        assert_eq!(
            sequences.first().copied(),
            Some(recorded.len() as u64 - RETENTION_FLOOR_EVENTS as u64 + 1),
            "the retained window must be the newest suffix, not an arbitrary slice"
        );
    }

    #[test]
    fn the_default_bound_leaves_an_ordinary_log_alone() {
        let (dir, store) = open_store();
        record_n(&store, 60);
        assert_eq!(store.count().unwrap(), 60);
        assert!(
            fs::metadata(events_path_of(dir.path())).unwrap().len() < DEFAULT_EVENT_LOG_MAX_BYTES,
            "a 60-event log should not be anywhere near the default bound"
        );
    }

    #[test]
    fn the_bound_is_spelled_like_every_other_ceiling() {
        assert_eq!(retention_limit(Some("2000000")), 2_000_000);
        for off in ["0", "off", "none", "unbounded", "unlimited", "infinite"] {
            assert_eq!(
                retention_limit(Some(off)),
                u64::MAX,
                "{off} must mean no bound at all, never a number a long run can outrun"
            );
        }
        assert_eq!(
            retention_limit(Some("eight megabytes")),
            DEFAULT_EVENT_LOG_MAX_BYTES,
            "a typo falls back to the default rather than silently unbounding the log"
        );
        assert_eq!(retention_limit(None), DEFAULT_EVENT_LOG_MAX_BYTES);
    }

    // ── The whole-file rewrite and the append path share one lock ────────────

    #[test]
    fn an_outcome_rewrite_does_not_drop_an_event_appended_while_it_ran() {
        // The race the retention bound exposed. `mark_outcome` lands after the
        // tool call it describes, so by the time it runs other writers have
        // appended; and it rewrites the whole file, so a snapshot taken before
        // those appends publishes a log without them.
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        let seed = EventStore::open(&root);
        let mut seeded = Vec::new();
        for i in 0..10 {
            seeded.push(
                seed.record("edit_file", &format!("seed {i}"), None, None, None, vec![])
                    .unwrap(),
            );
        }

        let writers = 4;
        let per_writer = 40;
        let seeded_count = seeded.len();
        let mut handles = Vec::new();
        for w in 0..writers {
            let root = root.clone();
            handles.push(std::thread::spawn(move || {
                let store = EventStore::open(&root);
                for i in 0..per_writer {
                    store
                        .record(
                            "write_file",
                            &format!("writer {w} event {i}"),
                            None,
                            None,
                            None,
                            vec![],
                        )
                        .unwrap();
                }
            }));
        }
        let rewriter = {
            let root = root.clone();
            std::thread::spawn(move || {
                let store = EventStore::open(&root);
                for i in 0..150 {
                    let sequence = seeded[i % seeded.len()];
                    store
                        .mark_outcome(sequence, EventOutcome::Success, None)
                        .unwrap();
                    store
                        .update_context(sequence, &format!("late outcome {i}"))
                        .unwrap();
                }
            })
        };
        handles.push(rewriter);
        for handle in handles {
            handle.join().unwrap();
        }

        let store = EventStore::open(&root);
        let events = store.load_all().unwrap();
        let expected = seeded_count + writers * per_writer;
        assert_eq!(
            events.len(),
            expected,
            "a rewrite published a log that had never seen the appends"
        );
        let mut sequences: Vec<u64> = events.iter().map(|event| event.sequence).collect();
        sequences.sort_unstable();
        sequences.dedup();
        assert_eq!(
            sequences.len(),
            expected,
            "duplicate sequence in {sequences:?}"
        );

        // A rewrite that cuts across an append can also tear a record in half.
        let raw = fs::read_to_string(events_path_of(dir.path())).unwrap();
        assert_eq!(raw.lines().count(), expected, "lines glued or lost");
        for line in raw.lines() {
            serde_json::from_str::<CodebaseEvent>(line)
                .unwrap_or_else(|error| panic!("torn line survived: {error}"));
        }
    }

    #[test]
    fn every_whole_file_rewrite_of_the_log_holds_the_append_lock() {
        // Behaviour alone cannot pin this: the race needs two writers and the
        // guard is about which of them is allowed to be mid-rewrite. So the
        // shape is checked directly - a rewrite that stops holding the lock, or
        // a new one that never started, fails here rather than in production.
        let src = include_str!("event_store.rs");
        // The tests in this file quote the very strings being scanned, so the
        // guard reads the production side of the cfg(test) split only.
        let production = &src[..src
            .find("#[cfg(test)]")
            .expect("the test module is cfg(test)-gated")];
        let methods = methods_in(production);
        let mut rewriters: Vec<&str> = methods
            .iter()
            .filter(|(_, body)| body.contains("self.save_all("))
            .map(|(name, _)| *name)
            .collect();
        rewriters.sort_unstable();
        assert_eq!(
            rewriters,
            vec!["enforce_retention", "rewrite_record"],
            "every whole-file rewrite either takes APPEND_LOCK itself or is documented as \
             running under a caller that does"
        );
        let rewrite = methods
            .iter()
            .find(|(name, _)| *name == "rewrite_record")
            .map(|(_, body)| *body)
            .unwrap();
        assert!(
            rewrite.contains("APPEND_LOCK"),
            "the single-record rewrite reads and writes the log; it must hold the lock every \
             appender holds"
        );

        // The retention pass is the one rewrite that borrows its caller's lock,
        // so the caller has to be a method that actually takes it.
        let callers: Vec<&str> = methods
            .iter()
            .filter(|(_, body)| body.contains("self.enforce_retention("))
            .map(|(name, _)| *name)
            .collect();
        assert_eq!(
            callers,
            vec!["record_with_metadata"],
            "a prune from anywhere else would rewrite the log unlocked"
        );
        let recorder = methods
            .iter()
            .find(|(name, _)| *name == "record_with_metadata")
            .map(|(_, body)| *body)
            .unwrap();
        assert!(recorder.contains("APPEND_LOCK"));
    }

    /// Each `    fn name(` / `    pub fn name(` in `src` with the text up to the
    /// next one, so a body check sees the whole method rather than its first
    /// line. Only the lock-discipline guard above uses this.
    fn methods_in(src: &str) -> Vec<(&str, &str)> {
        let mut starts: Vec<(usize, &str)> = Vec::new();
        for marker in ["\n    fn ", "\n    pub fn "] {
            let mut at = 0;
            while let Some(found) = src[at..].find(marker) {
                let name_start = at + found + marker.len();
                let name_end = src[name_start..]
                    .find('(')
                    .map(|offset| name_start + offset)
                    .unwrap_or(name_start);
                starts.push((name_start, &src[name_start..name_end]));
                at = name_start;
            }
        }
        starts.sort_unstable();
        let mut methods = Vec::new();
        for (index, (start, name)) in starts.iter().enumerate() {
            let end = starts
                .get(index + 1)
                .map(|(next, _)| *next)
                .unwrap_or(src.len());
            methods.push((*name, &src[*start..end]));
        }
        methods
    }
}
