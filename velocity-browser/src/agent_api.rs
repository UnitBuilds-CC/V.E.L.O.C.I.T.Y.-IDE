//! Unified agent action & observation API.
//!
//! Agents don't want a screenshot and a pixel-hunt; they want to *act* on a
//! page by semantic target and immediately learn *what changed* as readable
//! facts. This module provides two things:
//!
//! 1. [`NdaDelta`] + [`diff`]: a minimal, lossless diff between two
//!    [`NdaDocument`] snapshots (added / removed / changed facts). Because
//!    [`NdaDocument`] preserves literals, every entry in a delta is a real
//!    string an agent can read - not a hash.
//! 2. [`AgentActionResult`]: the status of a semantic action plus the NDA
//!    delta it produced. Session action methods (in `session.rs`) snapshot
//!    state before/after and return one of these, so an action is inseparable
//!    from its observation.

use crate::nda::NdaDocument;
use std::collections::HashMap;

/// A single fact that changed value for the same `(subject, predicate)` key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactChange {
    pub subject: String,
    pub predicate: u16,
    pub old: String,
    pub new: String,
}

/// The difference between two [`NdaDocument`] snapshots, expressed as readable
/// facts. `added`/`removed` hold `(subject, predicate, object)` triples that
/// appeared or disappeared outright; `changed` holds facts whose object value
/// changed for an unchanged `(subject, predicate)` key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NdaDelta {
    pub added: Vec<(String, u16, String)>,
    pub removed: Vec<(String, u16, String)>,
    pub changed: Vec<FactChange>,
}

impl NdaDelta {
    /// True when nothing changed between the two snapshots.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }

    /// Total number of individual fact changes represented.
    pub fn len(&self) -> usize {
        self.added.len() + self.removed.len() + self.changed.len()
    }
}

/// Compute the minimal delta transforming `before` into `after`.
///
/// The algorithm works on the readable `(subject, predicate, object)` view of
/// each document using multiset semantics, so duplicate facts are handled
/// correctly. Raw additions/removals that share the same `(subject, predicate)`
/// key are paired into [`FactChange`]s, which keeps a value edit (e.g. an
/// input's `value` going from "" to "hi") reported as one change rather than a
/// spurious remove + add.
pub fn diff(before: &NdaDocument, after: &NdaDocument) -> NdaDelta {
    let before_facts = before.readable_facts();
    let after_facts = after.readable_facts();

    // Multiset counts keyed by the full triple.
    let mut counts: HashMap<(String, u16, String), i64> = HashMap::new();
    for f in &before_facts {
        *counts.entry(f.clone()).or_insert(0) -= 1;
    }
    for f in &after_facts {
        *counts.entry(f.clone()).or_insert(0) += 1;
    }

    // Preserve document order for stable, glanceable deltas.
    let mut raw_removed: Vec<(String, u16, String)> = Vec::new();
    for f in &before_facts {
        if let Some(c) = counts.get_mut(f) {
            if *c < 0 {
                raw_removed.push(f.clone());
                *c += 1;
            }
        }
    }
    let mut raw_added: Vec<(String, u16, String)> = Vec::new();
    for f in &after_facts {
        if let Some(c) = counts.get_mut(f) {
            if *c > 0 {
                raw_added.push(f.clone());
                *c -= 1;
            }
        }
    }

    // Pair removals and additions that share a (subject, predicate) key into
    // value changes. Index added facts by key so we can consume matches.
    let mut added_by_key: HashMap<(String, u16), Vec<usize>> = HashMap::new();
    for (i, (s, p, _)) in raw_added.iter().enumerate() {
        added_by_key.entry((s.clone(), *p)).or_default().push(i);
    }

    let mut changed = Vec::new();
    let mut consumed_added = vec![false; raw_added.len()];
    let mut leftover_removed = Vec::new();

    for (s, p, old_obj) in raw_removed.into_iter() {
        let key = (s.clone(), p);
        let matched = added_by_key
            .get_mut(&key)
            .and_then(|idxs| idxs.iter().position(|&i| !consumed_added[i]).map(|pos| idxs[pos]));
        match matched {
            Some(i) => {
                consumed_added[i] = true;
                changed.push(FactChange {
                    subject: s,
                    predicate: p,
                    old: old_obj,
                    new: raw_added[i].2.clone(),
                });
            }
            None => leftover_removed.push((s, p, old_obj)),
        }
    }

    let added = raw_added
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !consumed_added[*i])
        .map(|(_, f)| f)
        .collect();

    NdaDelta {
        added,
        removed: leftover_removed,
        changed,
    }
}

/// The outcome of a semantic agent action: a human/agent-readable status plus
/// the NDA delta the action produced. An action is never reported without its
/// observation.
#[derive(Debug, Clone)]
pub struct AgentActionResult {
    pub status: String,
    pub delta: NdaDelta,
}

impl AgentActionResult {
    pub fn new(status: impl Into<String>, delta: NdaDelta) -> Self {
        Self { status: status.into(), delta }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::predicates::{AOM_NAME, AOM_VALUE};

    #[test]
    fn diff_detects_added_fact() {
        let before = NdaDocument::new();
        let mut after = NdaDocument::new();
        after.push_str("node_1", AOM_NAME, "Submit");
        let delta = diff(&before, &after);
        assert_eq!(delta.added, vec![("node_1".to_string(), AOM_NAME, "Submit".to_string())]);
        assert!(delta.removed.is_empty());
        assert!(delta.changed.is_empty());
    }

    #[test]
    fn diff_detects_removed_fact() {
        let mut before = NdaDocument::new();
        before.push_str("node_1", AOM_NAME, "Submit");
        let after = NdaDocument::new();
        let delta = diff(&before, &after);
        assert_eq!(delta.removed, vec![("node_1".to_string(), AOM_NAME, "Submit".to_string())]);
        assert!(delta.added.is_empty());
    }

    #[test]
    fn value_edit_is_reported_as_single_change() {
        let mut before = NdaDocument::new();
        before.push_str("node_1", AOM_VALUE, "");
        let mut after = NdaDocument::new();
        after.push_str("node_1", AOM_VALUE, "hello");
        let delta = diff(&before, &after);
        assert!(delta.added.is_empty(), "should not be a spurious add");
        assert!(delta.removed.is_empty(), "should not be a spurious remove");
        assert_eq!(
            delta.changed,
            vec![FactChange {
                subject: "node_1".to_string(),
                predicate: AOM_VALUE,
                old: "".to_string(),
                new: "hello".to_string(),
            }]
        );
    }

    #[test]
    fn identical_documents_yield_empty_delta() {
        let mut a = NdaDocument::new();
        a.push_str("node_1", AOM_NAME, "Submit");
        a.push_int("node_1", AOM_VALUE, 3);
        let b = a.clone();
        assert!(diff(&a, &b).is_empty());
    }

    #[test]
    fn delta_len_counts_all_changes() {
        let mut before = NdaDocument::new();
        before.push_str("n1", AOM_NAME, "old");
        before.push_str("n2", AOM_NAME, "removed");
        let mut after = NdaDocument::new();
        after.push_str("n1", AOM_NAME, "new");
        after.push_str("n3", AOM_NAME, "added");
        let delta = diff(&before, &after);
        // n1 changed, n2 removed, n3 added = 3 total
        assert_eq!(delta.len(), 3);
    }

    #[test]
    fn delta_is_empty_for_no_changes() {
        let d = NdaDelta::default();
        assert!(d.is_empty());
        assert_eq!(d.len(), 0);
    }

    #[test]
    fn action_result_carries_status_and_delta() {
        let mut delta = NdaDelta::default();
        delta.added.push(("node".to_string(), AOM_NAME, "Click".to_string()));
        let result = AgentActionResult::new("success", delta.clone());
        assert_eq!(result.status, "success");
        assert_eq!(result.delta.added.len(), 1);
    }

    #[test]
    fn diff_multiple_changes_same_key() {
        let mut before = NdaDocument::new();
        before.push_str("n1", AOM_VALUE, "a");
        before.push_str("n1", AOM_VALUE, "b");
        let mut after = NdaDocument::new();
        after.push_str("n1", AOM_VALUE, "c");
        after.push_str("n1", AOM_VALUE, "d");
        let delta = diff(&before, &after);
        // With multiset semantics, 2 removes and 2 adds for same key
        // should pair into 2 changes
        assert_eq!(delta.changed.len(), 2);
        assert!(delta.added.is_empty());
        assert!(delta.removed.is_empty());
    }
}
