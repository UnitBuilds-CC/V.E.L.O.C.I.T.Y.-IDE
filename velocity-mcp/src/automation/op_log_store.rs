//! Spine-2 (physical) — the durable, append-only operation log.
//!
//! The [`OpSet`](crate::automation::oplog::OpSet) is the *logical* source of
//! truth; this module is its *physical* home on disk, so the site map's
//! converged state survives restarts and can be replayed/replicated. The spec
//! (`docs/MULTINODE_COLLAB_GOVERNANCE_SPEC.md` §2) names a "single append-only
//! op-log as source of truth" — [`OpLogStore`] is that artifact.
//!
//! It deliberately mirrors the discipline already proven in
//! [`crate::registry::event_store`]: one JSON object per line under
//! `.velocity/oplog/ops.jsonl`, the serialized line + newline written in a
//! single `write_all` syscall (so concurrent appenders in one process cannot
//! interleave half-records — the historical `}{` collision bug), and a tolerant
//! [`OpLogStore::load`] that skips unreadable lines rather than poisoning the
//! whole log.
//!
//! Unlike the event store there is **no sequence race to guard**: an [`Op`]
//! carries its own `(node, counter)` [`OpId`](crate::automation::principal::OpId)
//! and [`materialize`](crate::automation::oplog::materialize) is order- and
//! duplicate-independent (INV-2/INV-3), so two ops appended in either order — or
//! the same op appended twice — converge to the same state. That is exactly why
//! the log can be append-only and gossip-safe.

use crate::automation::oplog::{materialize, MaterializedState, Op, OpSet};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

/// A durable, append-only op-log persisted as JSONL under
/// `<workspace>/.velocity/oplog/ops.jsonl`.
pub struct OpLogStore {
    dir: PathBuf,
}

impl OpLogStore {
    /// Open (or create) the op-log store rooted at `<workspace>/.velocity/oplog/`.
    pub fn open(workspace: &Path) -> Self {
        let dir = workspace.join(".velocity").join("oplog");
        let _ = fs::create_dir_all(&dir);
        Self { dir }
    }

    /// Path to the JSONL op-log file.
    pub fn ops_path(&self) -> PathBuf {
        self.dir.join("ops.jsonl")
    }

    /// Append a single op atomically: the serialized object and its newline go
    /// down as one `write_all`, so an interrupt or a concurrent appender cannot
    /// leave a half-line or splice two records together.
    pub fn append(&self, op: &Op) -> Result<(), String> {
        self.append_many(std::slice::from_ref(op))
    }

    /// Append several ops under one file handle. Each record is still written as
    /// its own atomic line, so a partial batch never corrupts earlier records.
    pub fn append_many(&self, ops: &[Op]) -> Result<(), String> {
        if ops.is_empty() {
            return Ok(());
        }
        let path = self.ops_path();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("Failed to open op-log for append {}: {e}", path.display()))?;
        for op in ops {
            let line = serde_json::to_string(op)
                .map_err(|e| format!("Failed to serialize op {:?}: {e}", op.op_id))?;
            let mut payload = String::with_capacity(line.len() + 1);
            payload.push_str(&line);
            payload.push('\n');
            file.write_all(payload.as_bytes())
                .map_err(|e| format!("Failed to write op: {e}"))?;
        }
        Ok(())
    }

    /// Read the whole log back into an [`OpSet`]. Missing file → empty set
    /// (back-compat: a workspace that predates the op-log simply starts clean).
    /// Blank and un-parseable lines are skipped with a warning, never fatal.
    pub fn load(&self) -> OpSet {
        let mut set = OpSet::new();
        let path = self.ops_path();
        if !path.exists() {
            return set;
        }
        let file = match fs::File::open(&path) {
            Ok(f) => f,
            Err(e) => {
                log::warn!("op_log_store: cannot open {}: {e}", path.display());
                return set;
            }
        };
        let reader = BufReader::new(file);
        for (idx, line) in reader.lines().enumerate() {
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    log::warn!("op_log_store: skipping unreadable line {}: {e}", idx + 1);
                    continue;
                }
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<Op>(trimmed) {
                Ok(op) => set.push(op),
                Err(e) => {
                    log::warn!("op_log_store: skipping malformed line {}: {e}", idx + 1);
                }
            }
        }
        set
    }

    /// Load and fold in one step — the derived view of the durable log.
    pub fn materialize(&self) -> MaterializedState {
        materialize(&self.load())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::oplog::OpEffect;
    use crate::automation::principal::VectorClock;
    use crate::automation::principal::{NodeId, Principal};
    use velocity_ide::site_map::VcTriple;

    fn triple(subj: u64, obj: u64) -> VcTriple {
        VcTriple {
            subject_hash: subj,
            predicate_id: 2,
            object_hash: obj,
        }
    }

    fn agent(node: &str, task: u64) -> Principal {
        Principal::agent(NodeId::new(node), task, None, None)
    }

    // A fresh empty clock: `Op::new_triple` bumps the node counter internally,
    // so every op built here carries `(node, 1)` — enough to exercise append/load.
    fn add_op(node: &str, subj: u64, obj: u64) -> Op {
        Op::new_triple(
            VectorClock::new(),
            agent(node, 1),
            "f.rs",
            vec![triple(subj, obj)],
            vec![],
            "add",
        )
    }

    fn open_store(dir: &Path) -> OpLogStore {
        OpLogStore::open(dir)
    }

    #[test]
    fn append_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(dir.path());
        let op = add_op("n1", 1, 2);
        store.append(&op).unwrap();

        let back = store.load();
        assert_eq!(back.ops().len(), 1);
        assert_eq!(back.ops()[0].op_id, op.op_id);
        // The converged triple from the durable log matches the in-memory fold.
        assert!(store.materialize().live_triples.contains(&(1, 2, 2)));
    }

    #[test]
    fn one_line_per_op_written_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(dir.path());
        for i in 0..20 {
            store.append(&add_op("n1", i, i + 1)).unwrap();
        }
        let raw = fs::read_to_string(store.ops_path()).unwrap();
        assert_eq!(raw.lines().count(), 20);
        for line in raw.lines() {
            assert!(line.starts_with('{') && line.ends_with('}'), "line {line}");
        }
    }

    #[test]
    fn load_skips_malformed_and_blank_lines() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(dir.path());
        store.append(&add_op("n1", 1, 2)).unwrap();
        // Inject a garbage line + a blank line, then a valid op via the normal path.
        let path = store.ops_path();
        let mut raw = fs::read_to_string(&path).unwrap();
        raw.push_str("this-is-not-json\n\n");
        fs::write(&path, &raw).unwrap();
        store.append(&add_op("n1", 3, 4)).unwrap();

        let set = store.load();
        // Only the two valid ops survive; the poison lines are skipped.
        assert_eq!(set.ops().len(), 2);
    }

    #[test]
    fn missing_file_loads_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(dir.path());
        // INV-4 back-compat: a fresh workspace has no op-log yet.
        assert!(store.load().is_empty());
        assert!(store.materialize().live_triples.is_empty());
        assert!(!store.ops_path().exists());
    }

    #[test]
    fn duplicate_ops_converge_on_reload() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(dir.path());
        let op = add_op("n1", 5, 6);
        // Same op appended twice (simulating a gossip replay).
        store.append(&op).unwrap();
        store.append(&op).unwrap();

        let set = store.load();
        assert_eq!(set.ops().len(), 2); // log keeps both (append-only, never rejects)
                                        // INV-3: materialize dedupes to a single live edge regardless of the duplicate.
        assert_eq!(store.materialize().live_triples.len(), 1);
        assert!(store.materialize().live_triples.contains(&(5, 2, 6)));
    }

    #[test]
    fn concurrent_appends_do_not_interleave() {
        use std::sync::Arc;
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(open_store(dir.path()));
        let mut handles = Vec::new();
        for t in 0..8 {
            let store = Arc::clone(&store);
            handles.push(std::thread::spawn(move || {
                for i in 0..15 {
                    let subj = (t * 100 + i) as u64;
                    store
                        .append(&add_op(&format!("n{t}"), subj, subj + 1))
                        .unwrap();
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let raw = fs::read_to_string(store.ops_path()).unwrap();
        // 8 threads x 15 ops = 120 records, none spliced together.
        assert_eq!(raw.lines().count(), 120);
        for line in raw.lines() {
            serde_json::from_str::<Op>(line)
                .unwrap_or_else(|e| panic!("interleaved record: {e}\nline {line}"));
        }
        assert_eq!(store.load().ops().len(), 120);
    }

    #[test]
    fn text_op_appends_and_loads() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(dir.path());
        let op = Op::new_text(
            VectorClock::new(),
            Principal::human(NodeId::new("human"), "alice", None),
            PathBuf::from("src/lib.rs"),
            "sha:abcd",
            "manual edit",
        );
        store.append(&op).unwrap();
        let back = store.load();
        assert_eq!(back.ops().len(), 1);
        match &back.ops()[0].effect {
            OpEffect::Text { path, digest } => {
                assert_eq!(path, &PathBuf::from("src/lib.rs"));
                assert_eq!(digest, "sha:abcd");
            }
            other => panic!("expected Text effect, got {other:?}"),
        }
        // A Text op is inert for the triple set (byte merge is D1).
        assert!(store.materialize().live_triples.is_empty());
    }
}
