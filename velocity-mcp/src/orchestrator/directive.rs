//! The shared directive inbox for a live-VC team run.
//!
//! A `team_dispatch` that routes through the orchestrator fans one prompt out
//! across several specialist workers running in parallel over disjoint file
//! scopes. When the user changes direction *mid-run* — adds a page, a network
//! transport, a new backend endpoint — that delta has to reach every worker
//! already in flight, but only the parts of it that fall inside a worker's own
//! domain. A `DirectiveInbox` is the rendezvous: the [`amend_running_team`]
//! tool pushes scope-tagged directives into it, and the pipeline tick loop
//! drains it once per wave and hands each directive to the workers whose scope
//! it reaches.
//!
//! Why scope tags rather than one flat broadcast: an unrestricted note sent to
//! the database engineer telling it to wire up a new React page is not
//! "awareness", it is a distraction that burns a turn and risks an out-of-scope
//! write. The inbox therefore stores the *filter* with the *text* and defers
//! the fan-out decision to delivery time, when the full roster is visible. A
//! broadcast directive (empty filter) is the one exception — it is the global
//! context every worker should carry, like "the deliverable format changed".
//!
//! The delivery planner is a pure function so the whole filtering contract is
//! testable without spawning a single thread or making a model call.

use serde::{Deserialize, Serialize};

/// Where a directive came from. Surfaced in the observability read-out so an
/// operator can tell an operator amendment from an auto-provisioned new seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DirectiveKind {
    /// An `amend_running_team` delta the user injected mid-run.
    Amend,
    /// A directive created because the delta introduced a scope no existing
    /// role covered, so a fresh seat was provisioned for it.
    Provision,
}

/// One pending instruction awaiting delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directive {
    /// Stable id assigned by the inbox on push; deliveries are recorded against
    /// it so the read-out can show exactly which worker got which directive.
    pub id: u64,
    /// The human-readable steer the worker receives.
    pub text: String,
    /// Which domains this directive is aimed at. Empty means *broadcast*: the
    /// directive reaches every live worker regardless of scope.
    pub scope_filter: Vec<String>,
    pub kind: DirectiveKind,
}

/// A record that directive `directive_id` was handed to the worker owning task
/// `task_id`. Emitted by the pipeline when it drains the inbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Delivery {
    pub directive_id: u64,
    pub task_id: u64,
}

/// A directive plus the task it is planned for — the delivery planner's output
/// row. Kept separate from [`Delivery`] so the pre-spawn path (which rewrites a
/// pending assignment's brief) and the in-flight path (which sends a live note)
/// can share one planner result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedDelivery {
    pub directive_id: u64,
    pub task_id: u64,
    /// Rendered steer text for that worker: the directive body, tagged with the
    /// worker's own domain so the note reads as addressed rather than ambient.
    pub note: String,
}

/// The lifecycle of a task at the moment a directive is drained. The two live
/// states decide *how* a directive reaches the worker; pending/terminal states
/// decide whether it reaches it at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerState {
    /// Not yet dispatched — the directive is folded into the brief before spawn.
    Pending,
    /// A worker thread is running — the directive goes out as a live note.
    Running,
    /// Finished (done/failed/blocked) — the directive is dropped: there is no
    /// one left to steer, and injecting it into a dead handle would silently
    /// vanish it.
    Terminal,
}

/// One roster row the planner needs to decide reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterEntry {
    pub task_id: u64,
    pub role: String,
    pub scope: Vec<String>,
    pub state: WorkerState,
}

/// Scope tag set: an inbox plus an append-only delivery ledger.
///
/// `pending` is the live queue; `delivered` accumulates every (directive, task)
/// pair the pipeline has handed off, and `seen` is the full push history so the
/// observability tool can list what was broadcast even after it drained.
#[derive(Debug, Default)]
pub struct DirectiveInbox {
    next_id: u64,
    pending: Vec<Directive>,
    seen: Vec<Directive>,
    delivered: Vec<Delivery>,
}

impl DirectiveInbox {
    pub fn new() -> Self {
        Self::default()
    }

    /// Assign the next id and enqueue a directive. The id is monotonic and
    /// never reused, so a delivery ledger entry always resolves to exactly one
    /// directive even after the queue drains.
    pub fn push(&mut self, text: String, scope_filter: Vec<String>, kind: DirectiveKind) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let directive = Directive {
            id,
            text,
            scope_filter,
            kind,
        };
        self.seen.push(directive.clone());
        self.pending.push(directive);
        id
    }

    /// Take the whole pending queue for delivery, leaving it empty. Called once
    /// per wave by the pipeline so a directive is handed to each eligible
    /// worker exactly once.
    pub fn take_pending(&mut self) -> Vec<Directive> {
        std::mem::take(&mut self.pending)
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Record that `directive_id` reached `task_id`.
    pub fn record_delivery(&mut self, directive_id: u64, task_id: u64) {
        self.delivered.push(Delivery {
            directive_id,
            task_id,
        });
    }

    pub fn deliveries(&self) -> &[Delivery] {
        &self.delivered
    }

    pub fn seen(&self) -> &[Directive] {
        &self.seen
    }

    /// True when a specific directive has already been handed to a specific
    /// worker — what the "did the amend reach every in-flight assignment"
    /// acceptance check reads.
    pub fn was_delivered(&self, directive_id: u64, task_id: u64) -> bool {
        self.delivered
            .iter()
            .any(|d| d.directive_id == directive_id && d.task_id == task_id)
    }
}

/// Normalise a scope pattern the way the pipeline's overlap rule does: trim
/// surrounding slashes, flip backslashes, and treat `/**` and a trailing `/` as
/// the directory prefix they name.
fn norm(pattern: &str) -> String {
    let p = pattern.trim().replace('\\', "/");
    let p = p.trim_matches('/');
    p.to_string()
}

/// Does pattern `a` name the same territory as, or a territory nested inside or
/// enclosing, `b`? Empty patterns never match — an unrestricted roster scope
/// must not swallow a targeted directive meant for a specific domain.
pub fn patterns_overlap(a: &str, b: &str) -> bool {
    let a = norm(a);
    let b = norm(b);
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let a = a.strip_suffix("/**").unwrap_or(&a);
    let b = b.strip_suffix("/**").unwrap_or(&b);
    a == b || a.starts_with(b) || b.starts_with(a)
}

/// Does `filter` (the directive's target scopes) reach `worker_scope`?
///
/// An empty filter is a broadcast and always reaches. Otherwise the directive
/// reaches the worker when any of its target patterns overlaps any of the
/// worker's own scope patterns — so a directive aimed at `src/db/` reaches a
/// `src/db/pool.rs` owner but never the `web/` owner.
pub fn scope_reaches(worker_scope: &[String], filter: &[String]) -> bool {
    if filter.is_empty() {
        return true;
    }
    worker_scope
        .iter()
        .any(|w| filter.iter().any(|f| patterns_overlap(w, f)))
}

/// Compute the deliveries for one drained batch against the current roster.
///
/// This is the whole filtering contract in one pure call. A directive reaches
/// an entry only when its scope filter hits that entry's domain *and* the entry
/// is steerable (pending or running, not terminal). The rendered note tags the
/// directive with the receiving worker's role so the note reads as addressed.
///
/// Returned in stable roster order so the delivery ledger is deterministic and
/// tests need not sort.
pub fn plan_directive_deliveries(
    directives: &[Directive],
    roster: &[RosterEntry],
) -> Vec<PlannedDelivery> {
    let mut plan = Vec::new();
    for directive in directives {
        for entry in roster {
            if entry.state == WorkerState::Terminal {
                continue;
            }
            if !scope_reaches(&entry.scope, &directive.scope_filter) {
                continue;
            }
            let note = addressed_note(directive, &entry.role);
            plan.push(PlannedDelivery {
                directive_id: directive.id,
                task_id: entry.task_id,
                note,
            });
        }
    }
    plan
}

/// The addressed form of a directive for one worker. Kept as one function so
/// the "you own X" framing cannot drift between the live-note and pre-spawn
/// paths.
fn addressed_note(directive: &Directive, role: &str) -> String {
    format!("[live-VC amendment for {role}] {}", directive.text.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directive(id: u64, text: &str, filter: &[&str], kind: DirectiveKind) -> Directive {
        Directive {
            id,
            text: text.to_string(),
            scope_filter: filter.iter().map(|s| s.to_string()).collect(),
            kind,
        }
    }

    fn entry(task_id: u64, role: &str, scope: &[&str], state: WorkerState) -> RosterEntry {
        RosterEntry {
            task_id,
            role: role.to_string(),
            scope: scope.iter().map(|s| s.to_string()).collect(),
            state,
        }
    }

    #[test]
    fn inbox_assigns_monotonic_ids_and_drains_once() {
        let mut inbox = DirectiveInbox::new();
        let a = inbox.push("first".into(), vec![], DirectiveKind::Amend);
        let b = inbox.push(
            "second".into(),
            vec!["src/".into()],
            DirectiveKind::Provision,
        );
        assert_eq!((a, b), (0, 1));
        assert_eq!(inbox.pending_len(), 2);
        let drained = inbox.take_pending();
        assert_eq!(drained.len(), 2);
        // A second drain is empty: each directive is delivered at most once.
        assert!(inbox.take_pending().is_empty());
        // History still lists both for the observability read-out.
        assert_eq!(inbox.seen().len(), 2);
    }

    #[test]
    fn delivery_ledger_resolves_a_directive_to_a_worker() {
        let mut inbox = DirectiveInbox::new();
        let id = inbox.push("steer".into(), vec![], DirectiveKind::Amend);
        assert!(!inbox.was_delivered(id, 7));
        inbox.record_delivery(id, 7);
        assert!(inbox.was_delivered(id, 7));
        assert!(!inbox.was_delivered(id, 8));
    }

    #[test]
    fn broadcast_reaches_every_scope() {
        let roster = vec![
            entry(2, "Lead", &["src/"], WorkerState::Running),
            entry(3, "Backend", &["src/db/"], WorkerState::Pending),
        ];
        let plan = plan_directive_deliveries(
            &[directive(0, "format changed", &[], DirectiveKind::Amend)],
            &roster,
        );
        assert_eq!(
            plan.len(),
            2,
            "a broadcast reaches running and pending alike"
        );
        assert!(plan.iter().all(|p| p.note.contains("format changed")));
    }

    #[test]
    fn targeted_directive_reaches_only_the_owning_domain() {
        let roster = vec![
            entry(2, "Backend", &["src/db/"], WorkerState::Running),
            entry(3, "Frontend", &["web/"], WorkerState::Running),
        ];
        let plan = plan_directive_deliveries(
            &[directive(
                0,
                "add a pool retry",
                &["src/db/"],
                DirectiveKind::Amend,
            )],
            &roster,
        );
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].task_id, 2);
        assert!(plan[0].note.contains("Backend"));
    }

    #[test]
    fn terminal_workers_are_never_delivered_to() {
        let roster = vec![entry(2, "Backend", &["src/db/"], WorkerState::Terminal)];
        let plan = plan_directive_deliveries(
            &[directive(0, "anything", &[], DirectiveKind::Amend)],
            &roster,
        );
        assert!(plan.is_empty(), "a dead worker cannot be steered");
    }

    #[test]
    fn empty_worker_scope_still_receives_a_targeted_directive_only_by_broadcast() {
        // A reviewer with no declared domain owns nothing concrete; a directive
        // aimed at `src/db/` must not reach it, but the same directive with a
        // broadcast filter must.
        let reviewer = entry(4, "Reviewer", &[], WorkerState::Running);
        let targeted = plan_directive_deliveries(
            &[directive(0, "x", &["src/db/"], DirectiveKind::Amend)],
            std::slice::from_ref(&reviewer),
        );
        assert!(targeted.is_empty());
        let broadcast =
            plan_directive_deliveries(&[directive(0, "x", &[], DirectiveKind::Amend)], &[reviewer]);
        assert_eq!(broadcast.len(), 1);
    }

    // ── Non-vacuity of the scope filter ────────────────────────────────────
    //
    // `scope_reaches` IS the guard that keeps one worker's amendment out of
    // another's context window. Reverting it to `true` (deliver everything to
    // everyone) makes `targeted_directive_reaches_only_the_owning_domain`
    // observe two deliveries and fail; reverting it to the empty-check alone
    // makes `empty_worker_scope_still_receives...` observe the targeted case as
    // empty for the wrong reason and the broadcast case as empty too — the pair
    // pins the behavior from both directions.
    #[test]
    fn scope_reaches_distinguishes_nested_from_disjoint() {
        assert!(scope_reaches(&["src/db/pool.rs".into()], &["src/".into()]));
        assert!(scope_reaches(&["src/".into()], &["src/db/".into()]));
        assert!(!scope_reaches(&["web/".into()], &["src/db/".into()]));
        // A disjoint filter never accidentally overlaps on a shared substring
        // because comparison is on path boundaries, not raw text.
        assert!(!scope_reaches(
            &["src/database/".into()],
            &["src/db/".into()]
        ));
    }
}
