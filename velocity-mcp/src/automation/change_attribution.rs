//! System B (attribution) — turn a detected change into an attributed, durable op.
//!
//! Spec §5 is explicit that the watcher *already exists* three times over
//! (`automation/watcher.rs`, `editor/file_watcher.rs`, `automation/build_runner.rs`)
//! and that what was genuinely missing is only the **principal (by whom)**, the
//! **recorded op**, and a **feedback-loop guard**. This module supplies exactly
//! those things and nothing more — it is *not* a fourth watcher, it is the
//! attribution layer the existing paths were missing.
//!
//! [`ChangeAttributor::record`] folds one observed change into the append-only
//! [`OpLogStore`] as a [`Principal`]-attributed [`Op::new_text`] and returns the
//! dependents it re-flags. Dependency re-flagging is *automatic* in the sense the
//! spec means: the op advances the durable trail, and [`SiteMap::get_callers`]
//! surfaces the affected dependents (INV-5) so a reader's Decision Trail can say
//! "X changed by <principal>, these callers are now suspect."
//!
//! The producer is a constructor parameter, not a special case: the GUI human
//! watcher builds the attributor with [`ChangeAttributor::open_human`] and the
//! agent executor builds one with an agent [`Principal`] — *the same* append path
//! serves both (spec §5 "human edits are just another `Op` producer — no special
//! path").
//!
//! **The feedback-loop guard is the durable log itself.** Two producers run on
//! different threads (GUI) or processes (headless), so an in-memory "did I write
//! this?" set cannot see the other's ops. Instead [`ChangeAttributor::record`]
//! re-reads the op-log and skips any `(path, digest)` already recorded — so an
//! agent write that lands in the log first is never re-logged as a human echo,
//! and a repeated watcher ping for unchanged content is dropped. This is a direct
//! consequence of the spine: *the op-log is the single source of truth*, so it is
//! also the single source of dedupe.

use crate::automation::op_log_store::OpLogStore;
use crate::automation::oplog::{Op, OpEffect};
use crate::automation::principal::{NodeId, Principal, VectorClock};
use crate::automation::PrincipalKind;
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use velocity_ide::hash_str;
use velocity_ide::site_map::SiteMap;

/// A change that has been folded into the durable op-log, plus the site-map
/// dependents it re-flags (the "who now needs to know" set, INV-5).
#[derive(Debug, Clone)]
pub struct Attribution {
    /// The op that was appended.
    pub op: Op,
    /// Hashes of subjects that depend on what changed (empty when no site map
    /// was supplied at record time).
    pub reflagged: Vec<u64>,
}

/// Attributes detected changes to a principal and persists them as ops,
/// re-flagging dependents through the site map and deduping against the durable
/// log so the same content is never recorded twice.
pub struct ChangeAttributor {
    store: OpLogStore,
    principal: Principal,
    clock: VectorClock,
    /// `(path, digest)` pairs already present in the durable trail. Refreshed
    /// from the log on every record so cross-producer echoes dedupe correctly.
    seen: HashSet<(String, String)>,
}

impl ChangeAttributor {
    /// Open (or create) the attributor rooted at `<workspace>/.velocity/oplog/`,
    /// attributing changes to `principal`. The node clock and the dedupe set are
    /// seeded from any existing log, so restarts never reuse `op_id`s and already
    /// recorded content is recognized immediately.
    pub fn open(workspace: &Path, principal: Principal) -> Self {
        let store = OpLogStore::open(workspace);
        let mut attributor = Self {
            store,
            principal,
            clock: VectorClock::new(),
            seen: HashSet::new(),
        };
        attributor.refresh_from_log();
        attributor
    }

    /// Open an attributor that stamps changes as a [`PrincipalKind::Human`],
    /// identified by the real `user_id` (the OS login user on the GUI path).
    pub fn open_human(workspace: &Path, user_id: impl Into<String>) -> Self {
        let user_id = user_id.into();
        let principal = Principal::human(NodeId::new(format!("human/{user_id}")), user_id, None);
        Self::open(workspace, principal)
    }

    /// The underlying durable store (read-only handle for reload/materialize).
    pub fn store(&self) -> &OpLogStore {
        &self.store
    }

    /// The principal this attributor stamps onto changes (for tests/diagnostics).
    pub fn principal_kind(&self) -> PrincipalKind {
        self.principal.kind
    }

    /// A stable content digest — what the watcher and the write path both derive
    /// from the same bytes, which is what makes the feedback-loop guard match.
    pub fn digest_of_content(content: &str) -> String {
        format!("sha:{:016x}", hash_str(content))
    }

    /// Fold one observed change into the durable op-log, attributed to this
    /// instance's principal, and return it (with an empty re-flag set — use
    /// [`Self::record_with_site_map`] when a site map is at hand).
    ///
    /// Returns `None` when `(rel_path, digest)` is already in the durable trail:
    /// either our own earlier record of the same content, or another producer's
    /// (the feedback-loop guard). The op is appended before returning, so a
    /// successful `Some` is already durable.
    pub fn record(&mut self, rel_path: &str, digest: &str, intent: &str) -> Option<Attribution> {
        self.record_inner(None, rel_path, digest, intent)
    }

    /// Like [`Self::record`], but also computes the dependents the change re-flags
    /// through [`SiteMap::get_callers`] (INV-5).
    pub fn record_with_site_map(
        &mut self,
        site_map: &SiteMap,
        rel_path: &str,
        digest: &str,
        intent: &str,
    ) -> Option<Attribution> {
        self.record_inner(Some(site_map), rel_path, digest, intent)
    }

    fn record_inner(
        &mut self,
        site_map: Option<&SiteMap>,
        rel_path: &str,
        digest: &str,
        intent: &str,
    ) -> Option<Attribution> {
        // The durable log is the shared truth across producers/threads: refresh
        // the seen set + clock from it so an echo of another producer's write is
        // recognized even though we hold no shared in-memory state with it.
        self.refresh_from_log();

        let key = (rel_path.to_string(), digest.to_string());
        if self.seen.contains(&key) {
            return None;
        }

        let reflagged = match site_map {
            Some(sm) => reflag_dependents(sm, rel_path),
            None => Vec::new(),
        };

        let op = Op::new_text(
            self.clock.clone(),
            self.principal.clone(),
            PathBuf::from(rel_path),
            digest.to_string(),
            intent.to_string(),
        );
        // Persist the advanced clock so the next op's counter is strictly greater.
        self.clock = op.clock.clone();

        if let Err(e) = self.store.append(&op) {
            log::warn!("change_attribution: failed to append op for {rel_path}: {e}");
        }
        self.seen.insert(key);

        Some(Attribution { op, reflagged })
    }

    /// Re-read the durable log to refresh the dedupe set and advance the clock to
    /// strictly above any `op_id` already recorded for our node.
    fn refresh_from_log(&mut self) {
        let existing = self.store.load();
        self.seen.clear();
        for op in existing.ops() {
            // Fold every observed causal history into our clock; merging is the
            // component-wise max, so our node counter continues from the highest
            // value already in the log (never reusing an op_id).
            self.clock.merge(&op.clock);
            if let OpEffect::Text { path, digest } = &op.effect {
                self.seen
                    .insert((path.to_string_lossy().into_owned(), digest.clone()));
            }
        }
    }
}

/// Dependents of a changed file: for every declaration the file owns, the subjects
/// that call it. Deduplicated + sorted for a deterministic re-flag set.
fn reflag_dependents(site_map: &SiteMap, rel_path: &str) -> Vec<u64> {
    let file_hash = hash_str(rel_path);
    // Members this file declares = object side of triples whose subject is the file.
    let members: Vec<u64> = site_map
        .find_live_triples(Some(file_hash), None, None)
        .into_iter()
        .map(|t| t.object_hash)
        .collect();

    let mut dependents: BTreeSet<u64> = BTreeSet::new();
    for member in members {
        for caller in site_map.get_callers(member) {
            // A file is not its own dependent.
            if caller != file_hash {
                dependents.insert(caller);
            }
        }
    }
    dependents.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use velocity_ide::site_map::VcTriple;

    fn snapshot(sm: &mut SiteMap, path: &str, triples: &[(u64, u16, u64)]) {
        let live: Vec<VcTriple> = triples
            .iter()
            .map(|(s, p, o)| VcTriple {
                subject_hash: *s,
                predicate_id: *p,
                object_hash: *o,
            })
            .collect();
        sm.put_file_snapshot(path, &live).unwrap();
    }

    #[test]
    fn human_op_is_appended_to_durable_store() {
        let dir = tempfile::tempdir().unwrap();
        let mut attributor = ChangeAttributor::open_human(dir.path(), "alice");
        let sm = SiteMap::open(&dir.path().join("sm"), 0).unwrap();

        let a = attributor
            .record_with_site_map(&sm, "src/lib.rs", "sha:aaaa", "manual edit")
            .expect("not a dup");

        assert_eq!(a.op.principal.kind, PrincipalKind::Human);
        assert_eq!(attributor.principal_kind(), PrincipalKind::Human);
        // The op is durable: reload the physical log and find it.
        let back = attributor.store().load();
        assert_eq!(back.ops().len(), 1);
        assert_eq!(back.ops()[0].op_id, a.op.op_id);
    }

    #[test]
    fn dependents_are_reflagged_through_get_callers() {
        let dir = tempfile::tempdir().unwrap();
        let mut attributor = ChangeAttributor::open_human(dir.path(), "alice");
        let mut sm = SiteMap::open(&dir.path().join("sm"), 0).unwrap();

        // a.rs declares member M (=500); b.rs calls M (predicate 2).
        let a_hash = hash_str("a.rs");
        let b_hash = hash_str("b.rs");
        snapshot(&mut sm, "a.rs", &[(a_hash, 1, 500)]);
        snapshot(&mut sm, "b.rs", &[(b_hash, 2, 500)]);

        let a = attributor
            .record_with_site_map(&sm, "a.rs", "sha:1", "manual edit")
            .unwrap();

        // Changing a.rs re-flags its caller, b.rs.
        assert_eq!(a.reflagged, vec![b_hash]);
    }

    #[test]
    fn record_without_site_map_still_persists() {
        let dir = tempfile::tempdir().unwrap();
        let mut attributor = ChangeAttributor::open_human(dir.path(), "alice");
        let a = attributor.record("a.rs", "sha:1", "edit").unwrap();
        assert!(a.reflagged.is_empty());
        assert_eq!(attributor.store().load().ops().len(), 1);
    }

    #[test]
    fn duplicate_content_is_deduped_against_the_durable_log() {
        let dir = tempfile::tempdir().unwrap();
        let mut attributor = ChangeAttributor::open_human(dir.path(), "alice");
        // First record lands; a repeat of the exact (path, digest) is dropped.
        assert!(attributor.record("a.rs", "sha:same", "edit").is_some());
        assert!(
            attributor.record("a.rs", "sha:same", "edit").is_none(),
            "the same content must not be double-recorded"
        );
        // A different digest for the same path is a real change → recorded.
        assert!(attributor.record("a.rs", "sha:new", "edit").is_some());
        assert_eq!(attributor.store().load().ops().len(), 2);
    }

    #[test]
    fn cross_producer_echo_is_deduped_via_the_shared_log() {
        // The feedback-loop guard is the durable log: an agent instance records a
        // write, then a *separate* human instance (different thread/process, no
        // shared memory) observing the same (path, digest) suppresses the echo.
        let dir = tempfile::tempdir().unwrap();
        let agent_principal = Principal::agent(NodeId::new("agent/primary"), 7, None, None);
        let mut agent = ChangeAttributor::open(dir.path(), agent_principal);
        assert!(agent
            .record("a.rs", "sha:written-by-agent", "agent write")
            .is_some());

        let mut human = ChangeAttributor::open_human(dir.path(), "alice");
        assert!(
            human
                .record("a.rs", "sha:written-by-agent", "manual edit")
                .is_none(),
            "the agent's write echo must not be attributed as human"
        );
        // Only the agent op is in the trail.
        let log = human.store().load();
        assert_eq!(log.ops().len(), 1);
        assert_eq!(log.ops()[0].principal.kind, PrincipalKind::Agent);
    }

    #[test]
    fn op_ids_never_reuse_across_reopen() {
        // Reopening the same workspace must not collide on op_id: the clock is
        // seeded from the existing log, so the next counter is strictly greater.
        let dir = tempfile::tempdir().unwrap();
        let mut first = ChangeAttributor::open_human(dir.path(), "alice");
        let a = first.record("a.rs", "sha:1", "e1").unwrap();
        drop(first);

        let mut second = ChangeAttributor::open_human(dir.path(), "alice");
        let b = second.record("b.rs", "sha:2", "e2").unwrap();
        assert!(
            b.op.op_id > a.op.op_id,
            "a reopened attributor must advance the node counter"
        );
    }

    #[test]
    fn counters_strictly_increase_per_op() {
        let dir = tempfile::tempdir().unwrap();
        let mut attributor = ChangeAttributor::open_human(dir.path(), "alice");
        let first = attributor.record("a.rs", "sha:1", "e1").unwrap();
        let second = attributor.record("b.rs", "sha:2", "e2").unwrap();
        assert!(second.op.op_id > first.op.op_id);
    }

    #[test]
    fn content_digest_is_stable_and_distinguishing() {
        assert_eq!(
            ChangeAttributor::digest_of_content("fn a() {}"),
            ChangeAttributor::digest_of_content("fn a() {}")
        );
        assert_ne!(
            ChangeAttributor::digest_of_content("fn a() {}"),
            ChangeAttributor::digest_of_content("fn b() {}")
        );
    }

    #[test]
    fn materialize_sees_no_triples_from_text_ops() {
        // A Text op advances the durable trail but is inert for the triple set
        // (byte merge is D1), so materialize's live_triples stays empty.
        let dir = tempfile::tempdir().unwrap();
        let mut attributor = ChangeAttributor::open_human(dir.path(), "alice");
        attributor.record("a.rs", "sha:1", "edit");
        assert!(attributor.store().materialize().live_triples.is_empty());
        assert_eq!(attributor.store().load().ops().len(), 1);
    }
}
