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
}

// ─── Event store ────────────────────────────────────────────────────────────

/// Persistent event store backed by a JSONL file.
///
/// Events are append-only; updates (outcome changes) rewrite the full file
/// because the expected volume is low (hundreds to thousands of events, not
/// millions).
pub struct EventStore {
    dir: PathBuf,
}

impl EventStore {
    /// Open (or create) the event store at `<workspace>/.velocity/events/`.
    pub fn open(workspace: &Path) -> Self {
        let dir = workspace.join(".velocity").join("events");
        let _ = fs::create_dir_all(&dir);
        Self { dir }
    }

    /// Path to the JSONL events file.
    fn events_path(&self) -> PathBuf {
        self.dir.join("events.jsonl")
    }

    /// Append a new event.  Returns the assigned sequence number.
    pub fn record(
        &self,
        tool_name: &str,
        description: &str,
        merkle_root_before: Option<String>,
        merkle_root_after: Option<String>,
        context: Option<String>,
        affected_files: Vec<String>,
    ) -> Result<u64, String> {
        let events = self.load_all()?;
        let next_seq = events.last().map(|e| e.sequence + 1).unwrap_or(1);

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
            outcome: EventOutcome::Pending,
            failure_reason: None,
            affected_files,
            metadata: None,
        };

        self.append_event(&event)?;
        Ok(next_seq)
    }

    /// Update the outcome (and optional failure reason) of an existing event.
    pub fn mark_outcome(
        &self,
        sequence: u64,
        outcome: EventOutcome,
        failure_reason: Option<String>,
    ) -> Result<bool, String> {
        let mut events = self.load_all()?;
        let mut found = false;
        for event in events.iter_mut() {
            if event.sequence == sequence {
                event.outcome = outcome.clone();
                event.failure_reason = failure_reason.clone();
                found = true;
                break;
            }
        }
        if !found {
            return Ok(false);
        }
        // Rewrite the full file with the updated event.
        self.save_all(&events)?;
        Ok(true)
    }

    /// Attach or replace the agent context (the *why*) on an existing event.
    pub fn update_context(&self, sequence: u64, context: &str) -> Result<bool, String> {
        let mut events = self.load_all()?;
        let mut found = false;
        for event in events.iter_mut() {
            if event.sequence == sequence {
                event.context = Some(context.to_string());
                found = true;
                break;
            }
        }
        if !found {
            return Ok(false);
        }
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

    fn append_event(&self, event: &CodebaseEvent) -> Result<(), String> {
        let path = self.events_path();
        let line = serde_json::to_string(event)
            .map_err(|e| format!("Failed to serialize event: {}", e))?;
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
        let mut payload = String::with_capacity(line.len() + 1);
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

/// Enrich a `read_file` response with a decision trail from the event store.
/// Returns the original content with a `── Decision Trail ──` section appended
/// (only if events exist for the file).
pub fn enrich_read_response(root: &Path, relative_path: &str, content: &str) -> String {
    let store = EventStore::open(root);
    let events = match store.query(Some(relative_path), None, 5) {
        Ok(e) if !e.is_empty() => e,
        _ => return content.to_string(),
    };

    let mut enriched = content.to_string();
    enriched.push_str("\n\n── Decision Trail ──\n");
    for event in events.iter().rev() {
        let outcome_tag = match event.outcome {
            EventOutcome::Success => "OK",
            EventOutcome::Failure => "FAIL",
            EventOutcome::Revert => "REVERT",
            EventOutcome::Pending => "?",
        };
        enriched.push_str(&format!(
            "  [#{}] {} [{}] {}",
            event.sequence, outcome_tag, event.tool_name, event.description
        ));
        if let Some(ctx) = &event.context {
            enriched.push_str(&format!(" — {}", ctx));
        }
        if let Some(reason) = &event.failure_reason {
            enriched.push_str(&format!(" (failed: {})", reason));
        }
        enriched.push('\n');
    }
    enriched
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
}
