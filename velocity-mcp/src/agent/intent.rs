//! Intent classification — the decision layer that routes a request to a
//! single agent or to a specialized team.
//!
//! Everything downstream of this module already exists and is proven: expert
//! teams ([`crate::editor::expert_team`]), per-member routing and headless
//! execution ([`crate::agent::executor::team_routing`]), and the meta-agent
//! orchestrator ([`crate::orchestrator`]). What was missing is the *brain*: a
//! request like "clone liteLLM and rewrite it in rust" used to fall through to
//! one general agent with a turn budget, because the only team path was an
//! explicit `@team` / `/team` directive.
//!
//! [`classify`] fills that gap. It is a pure, deterministic scorer over the
//! request text — no model call, no I/O — so it is trivially unit-testable and
//! can never silently degrade to a 0.5B hallucination. It reads the *shape* of
//! a task (does it span a whole repo? migrate a language? touch several
//! domains? enumerate several deliverables?) and emits an [`IntentDecision`]
//! naming the scope, the specializations to staff, and whether a repository
//! must be pulled first. [`provision_team`] then turns those specializations
//! into a real, persisted [`crate::editor::expert_team::ExpertTeam`] that the
//! existing routing/execution path can dispatch to unchanged.
//!
//! [`classify`] *is* that deterministic scorer and never goes stale. But the
//! routing decision is also the system's first **decision matrix**, and
//! [`decide_intent`] runs it through a swappable System-1 brain
//! ([`crate::agent::decision`]) — on-device Laya by default, hosted Jev with a
//! config flip — then falls back to [`classify`] whenever no model is wired up,
//! the call fails, or the model is unsure. The bounded questions (single-vs-team
//! scope, does-a-repo-need-pulling, target language, complexity, short-vs-long
//! horizon) live in [`intent_questions`]; adding one there extends every backend
//! at once.
//!
//! Horizon ([`TaskHorizon`]) is what lets a request be *run* rather than merely
//! routed: a long-horizon task cannot be babysat, so it goes to the resumable
//! mission runner with checkpoints and gates instead of to one agent with a turn
//! budget. Authorization to actually run unattended stays with the user's text
//! ([`IntentDecision::wants_unattended`]) and is never inferred by a model.

use crate::agent::decision::{self, Answer, DecisionState, Question};
use serde::{Deserialize, Serialize};

/// Whether a request should be handled by one agent or a team of specialists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskScope {
    /// A focused, single-surface task: one agent is the right tool.
    SingleAgent,
    /// A sprawling task that spans modules, domains, or a whole repository:
    /// staff a team of specialized sub-agents.
    Team,
}

/// How long the work will run for, which decides whether a human can stay in
/// the loop.
///
/// This is a different axis from [`TaskScope`]. Scope asks *how wide* the task
/// is (one surface or many); horizon asks *how long* it takes. They diverge in
/// both directions: implementing a red-black tree is narrow and hard and still
/// finishes in one session, while porting 3,000 straightforward files is wide,
/// individually trivial, and runs for a day and a half. Conflating the two
/// would send the first to an unattended mission runner and keep the second
/// interactive, which is exactly backwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskHorizon {
    /// Completes inside an interactive session. Someone can watch it happen and
    /// correct it mid-flight.
    Short,
    /// Runs for many dispatches over hours. Cannot be babysat: it needs
    /// checkpoints, gates and a resumable executor, because nobody will be
    /// present at hour six when a dispatch fails.
    Long,
}

impl TaskHorizon {
    pub fn is_long(self) -> bool {
        matches!(self, TaskHorizon::Long)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            TaskHorizon::Short => "short",
            TaskHorizon::Long => "long",
        }
    }
}

/// One specialization the classifier wants on the team.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleSpec {
    /// Human role title, e.g. "Rust Port Engineer".
    pub role: String,
    /// Skill tags (free-form, mirrors `ExpertMember::skills`).
    pub skills: Vec<String>,
    /// Path globs this role owns, for scope-based routing (mirrors
    /// `ExpertMember::scope_patterns`).
    pub scope_patterns: Vec<String>,
}

/// The full verdict of [`classify`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntentDecision {
    pub scope: TaskScope,
    /// True when the request names a repository to pull before working.
    pub requires_repo_ingest: bool,
    /// The git URL to clone, if one could be extracted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_url: Option<String>,
    /// Target language for a port/rewrite, e.g. "rust".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_language: Option<String>,
    /// Specializations to staff (empty for [`TaskScope::SingleAgent`]).
    pub roles: Vec<RoleSpec>,
    /// One-line explanation of the verdict, for the transcript.
    pub rationale: String,
    /// Confidence in the scope call, 0.0..=1.0.
    pub confidence: f32,
    /// Which signals fired (debuggable audit trail).
    pub signals: Vec<String>,
    /// Whether the work outlives an interactive session.
    pub horizon: TaskHorizon,
    /// True when the *request text* asked for no human in the loop.
    ///
    /// Kept separate from `horizon` on purpose. A long task the user wants to
    /// watch is still long, and a short task nobody authorized for automation
    /// must never be started unattended — running hundreds of dispatches
    /// without a human is a spend decision, and only the user can make it. The
    /// decision model is therefore never allowed to set this field; see
    /// [`intent_from_answers`].
    pub wants_unattended: bool,
}

/// A team-scoped request must reach at least this score to auto-route to a
/// team. Deliberately conservative: an ordinary chat message scores ~0-1 and
/// keeps the single-agent path it always had.
pub const TEAM_THRESHOLD: i32 = 3;

/// Horizon score at which a task is classified [`TaskHorizon::Long`].
///
/// Three, so that no single phrase can commit the system to an unattended
/// multi-hour run: it takes either a repository port (worth three on its own,
/// because "clone X and rewrite it in Y" has no short version) or two
/// independent kinds of evidence.
pub const LONG_HORIZON_THRESHOLD: i32 = 3;

/// Phrases that scope the work to a whole codebase rather than to a part of one.
const LONG_HORIZON_WORDS: &[&str] = &[
    "long-horizon",
    "long horizon",
    "whole repo",
    "entire repo",
    "whole repository",
    "entire repository",
    "whole codebase",
    "entire codebase",
    "full rewrite",
    "complete rewrite",
    "every file",
    "every module",
    "all modules",
    "the scale of",
    "at scale",
    "large codebase",
    "production ready",
    "production-ready",
];

/// Phrases that ask for the work to be completed without a human in the loop.
const UNATTENDED_WORDS: &[&str] = &[
    "end to end",
    "end-to-end",
    "in one pass",
    "in 1 pass",
    "in a single pass",
    "without human intervention",
    "no human intervention",
    "without intervention",
    "unattended",
    "autonomously",
    "autonomous",
    "hands off",
    "hands-off",
    "fully automatic",
    "without asking",
    "to completion",
    "keep going until",
];

/// Languages we recognize as a port/rewrite target.
const LANGUAGES: &[&str] = &[
    "rust",
    "python",
    "typescript",
    "javascript",
    "golang",
    "go",
    "java",
    "kotlin",
    "swift",
    "c++",
    "cpp",
    "c#",
    "csharp",
    "ruby",
    "php",
    "zig",
    "elixir",
    "haskell",
    "scala",
    "dart",
];

/// Domain buckets whose co-occurrence signals a multi-surface (team) task.
const DOMAINS: &[(&str, &[&str])] = &[
    (
        "frontend",
        &["frontend", "ui", "react", "vue", "svelte", "css", "web"],
    ),
    (
        "backend",
        &["backend", "server", "api", "endpoint", "service"],
    ),
    (
        "database",
        &[
            "database",
            "db",
            "sql",
            "schema",
            "migration",
            "postgres",
            "sqlite",
        ],
    ),
    (
        "auth",
        &["auth", "login", "oauth", "permission", "security"],
    ),
    (
        "testing",
        &[
            "test",
            "tests",
            "qa",
            "coverage",
            "unit test",
            "integration test",
        ],
    ),
    ("docs", &["docs", "documentation", "readme", "guide"]),
    (
        "deployment",
        &[
            "deploy",
            "devops",
            "ci",
            "cd",
            "infra",
            "docker",
            "kubernetes",
        ],
    ),
];

/// Action verbs; many distinct verbs => many deliverables => team.
const VERBS: &[&str] = &[
    "implement",
    "add",
    "create",
    "write",
    "build",
    "refactor",
    "test",
    "document",
    "deploy",
    "review",
    "design",
    "migrate",
    "optimize",
    "port",
    "rewrite",
    "convert",
    "translate",
    "analyze",
    "plan",
    "fix",
    "clone",
    "benchmark",
    "audit",
    "harden",
    "sweep",
];

/// Whole-repo / scale intensifiers.
const SCALE_WORDS: &[&str] = &[
    "entire",
    "whole",
    "full",
    "complete",
    "end-to-end",
    "from scratch",
    "top to bottom",
    "every module",
    "all modules",
    "all of",
    "across the",
    "codebase",
    "repository",
    "repo",
];

/// Multi-step narrative connectors.
const SEQUENCE_WORDS: &[&str] = &[
    "and then",
    "then ",
    "after that",
    "first",
    "next",
    "finally",
    "step 1",
    "step one",
    "once that",
];

/// Words that explicitly ask for a team / parallel work.
const TEAM_WORDS: &[&str] = &[
    "team",
    "swarm",
    "orchestrate",
    "parallel",
    "subagent",
    "sub-agent",
    "multi-agent",
    "multiagent",
    "multiple engineers",
    "specialists",
    "coordinate",
];

/// Words that explicitly ask to keep it to one agent / small scope.
const SINGLE_WORDS: &[&str] = &[
    "single agent",
    "one agent",
    "just ",
    "only ",
    "one file",
    "quick ",
    "tiny",
    "small change",
];

/// Whole-word containment on a lowercased haystack (word boundaries so "go"
/// does not match "google" and "ui" does not match "build").
fn has_word(lower: &str, word: &str) -> bool {
    let w = word.trim();
    if w.is_empty() {
        return false;
    }
    let bytes = lower.as_bytes();
    let wb = w.as_bytes();
    let mut start = 0usize;
    while let Some(pos) = lower[start..].find(w) {
        let abs = start + pos;
        let before_ok = abs == 0 || !bytes[abs - 1].is_ascii_alphanumeric();
        let after_idx = abs + wb.len();
        let after_ok = after_idx >= bytes.len() || !bytes[after_idx].is_ascii_alphanumeric();
        // Treat a trailing/leading space in the phrase as a soft boundary.
        if word.ends_with(' ') || word.starts_with(' ') {
            return true;
        }
        if before_ok && after_ok {
            return true;
        }
        start = abs + 1;
        if start >= lower.len() {
            break;
        }
    }
    false
}

/// Detect a port/rewrite target language from proximity cues.
fn detect_target_language(lower: &str) -> Option<&'static str> {
    for lang in LANGUAGES {
        // "in rust", "to rust", "using rust", "with rust"
        let proximate = [
            format!("in {lang}"),
            format!("to {lang}"),
            format!("using {lang}"),
            format!("with {lang}"),
            format!("{lang} rewrite"),
            format!("{lang} port"),
            format!("{lang} version"),
            format!("rewrite it in {lang}"),
            format!("rewrite {lang}"),
        ];
        // "go" is too generic to match on bare containment of the word alone;
        // require a proximate phrase for it.
        for p in &proximate {
            if lower.contains(p.as_str()) {
                return Some(lang);
            }
        }
    }
    None
}

/// Trim surrounding quote/comma/period punctuation from a token.
fn trim_token(s: &str) -> &str {
    s.trim_matches(|c| c == '"' || c == '\'' || c == ',' || c == '.')
}

/// Extract a git URL, or synthesize one from a bare `owner/repo` after "clone".
/// Takes the *original* prompt so a real URL keeps its casing (GitHub paths are
/// case-preserving); matching is done against a lowercased projection.
fn detect_repo(prompt: &str) -> Option<String> {
    let lower = prompt.to_lowercase();
    // Explicit http(s)/git URLs containing a known host or a .git suffix.
    for (orig, low) in prompt.split_whitespace().zip(lower.split_whitespace()) {
        let (o, l) = (trim_token(orig), trim_token(low));
        if (l.starts_with("http://") || l.starts_with("https://") || l.starts_with("git@"))
            && (l.contains("github")
                || l.contains("gitlab")
                || l.contains("bitbucket")
                || l.ends_with(".git"))
        {
            return Some(o.to_string());
        }
        if l.ends_with(".git") && l.contains('/') {
            return Some(o.to_string());
        }
    }
    // Bare owner/repo following "clone" (case preserved from the source text).
    if let Some(idx) = lower.find("clone") {
        let rest_orig = &prompt[idx + "clone".len()..];
        let rest_low = &lower[idx + "clone".len()..];
        for (o, l) in rest_orig
            .split_whitespace()
            .zip(rest_low.split_whitespace())
        {
            let t = trim_token(l);
            if is_owner_repo(t) {
                return Some(format!("https://github.com/{}.git", trim_token(o)));
            }
        }
    }
    None
}

fn is_owner_repo(s: &str) -> bool {
    let mut parts = s.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(o), Some(r), None) => {
            !o.is_empty()
                && !r.is_empty()
                && o.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
                && r.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        }
        _ => false,
    }
}

/// Count distinct enumerated list markers ("1." / "1)" / "- " at line start).
fn count_list_markers(text: &str) -> usize {
    let mut n = 0;
    for line in text.lines() {
        let l = line.trim_start();
        let numbered = l
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
            && (l.contains(". ") || l.contains(") "));
        let bulleted = l.starts_with("- ") || l.starts_with("* ");
        if numbered || bulleted {
            n += 1;
        }
    }
    n
}

/// Build the specialization roster from the fired signals.
///
/// `long_horizon` adds the sweep specialists. They are not decoration: a
/// mission that runs for hours produces more code than any single reviewer pass
/// can read, and nobody is present to notice a mistake at hour six. Security,
/// durability and performance are the three axes that fail *silently* — an
/// injection hole, a panic on a rare input, and a quadratic loop all sail
/// through a test suite written by the same pass that wrote the code.
fn derive_roles(
    target_language: Option<&str>,
    is_port: bool,
    domains: &[&'static str],
    long_horizon: bool,
) -> Vec<RoleSpec> {
    let mut roles = Vec::new();
    roles.push(RoleSpec {
        role: "Lead Architect".to_string(),
        skills: vec!["planning".to_string(), "architecture".to_string()],
        scope_patterns: vec![],
    });

    if is_port {
        let lang = target_language.unwrap_or("target");
        roles.push(RoleSpec {
            role: format!("{} Port Engineer", capitalize(lang)),
            skills: vec![lang.to_string(), "porting".to_string()],
            scope_patterns: vec!["src/".to_string()],
        });
    }

    for d in domains {
        let (title, skills, scope) = match *d {
            "frontend" => (
                "Frontend Engineer",
                "ui",
                vec!["web/", "frontend/", "src/ui/"],
            ),
            "backend" => (
                "Backend Engineer",
                "api",
                vec!["server/", "backend/", "src/"],
            ),
            "database" => (
                "Database Engineer",
                "sql",
                vec!["db/", "migrations/", "schema/"],
            ),
            "auth" => ("Security Engineer", "auth", vec!["auth/", "security/"]),
            "testing" => ("Test Engineer", "testing", vec!["tests/", "test/"]),
            "docs" => ("Documentation Engineer", "docs", vec!["docs/", "README"]),
            "deployment" => (
                "DevOps Engineer",
                "ci",
                vec![".github/", "deploy/", "infra/"],
            ),
            _ => continue,
        };
        roles.push(RoleSpec {
            role: title.to_string(),
            skills: vec![skills.to_string()],
            scope_patterns: scope.iter().map(|s| s.to_string()).collect(),
        });
    }

    // Every team gets a reviewer/QA gate.
    if !roles
        .iter()
        .any(|r| r.role.contains("Test") || r.role.contains("QA"))
    {
        roles.push(RoleSpec {
            role: "Test & QA Engineer".to_string(),
            skills: vec!["testing".to_string(), "review".to_string()],
            scope_patterns: vec!["tests/".to_string()],
        });
    }

    if long_horizon {
        // Scope patterns are empty for the sweep roles on purpose: they audit
        // the whole tree rather than owning a corner of it, and a scope pattern
        // would let [`crate::editor::team_router::route_member`] hand them only
        // the files they happened to match.
        push_role_if_absent(
            &mut roles,
            "Security Sweep Auditor",
            &[
                "security",
                "audit",
                "threat-model",
                "dependency-vulnerabilities",
            ],
            &[],
        );
        push_role_if_absent(
            &mut roles,
            "Durability Engineer",
            &[
                "error-handling",
                "reliability",
                "fault-tolerance",
                "resource-cleanup",
            ],
            &[],
        );
        push_role_if_absent(
            &mut roles,
            "Performance Engineer",
            &["benchmarking", "profiling", "optimization", "allocation"],
            &[],
        );
        push_role_if_absent(
            &mut roles,
            "Documentation Engineer",
            &["docs", "api-reference", "architecture-notes"],
            &["docs/", "README"],
        );
    }

    roles.push(RoleSpec {
        role: "Reviewer".to_string(),
        skills: vec!["review".to_string()],
        scope_patterns: vec![],
    });
    roles
}

/// Append a role unless one with that exact title is already on the roster.
///
/// Title equality only. Checking skills instead would drop the sweep auditor
/// whenever the `auth` domain already put a "Security Engineer" on the team,
/// and those are different jobs: one owns the auth code, the other audits
/// everything the mission produced.
fn push_role_if_absent(roles: &mut Vec<RoleSpec>, title: &str, skills: &[&str], scope: &[&str]) {
    if roles.iter().any(|r| r.role == title) {
        return;
    }
    roles.push(RoleSpec {
        role: title.to_string(),
        skills: skills.iter().map(|s| (*s).to_string()).collect(),
        scope_patterns: scope.iter().map(|s| (*s).to_string()).collect(),
    });
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Classify a request into a routing decision. Pure and deterministic.
pub fn classify(prompt: &str) -> IntentDecision {
    let lower = prompt.to_lowercase();
    let mut signals: Vec<String> = Vec::new();
    let mut score: i32 = 0;

    let target_language = detect_target_language(&lower);
    let is_port = [
        "rewrite",
        "port",
        "migrate",
        "convert",
        "reimplement",
        "translate",
        "rebuild",
    ]
    .iter()
    .any(|w| has_word(&lower, w));
    if is_port {
        score += 2;
        signals.push("port/rewrite verb".to_string());
    }
    if let Some(lang) = target_language {
        signals.push(format!("target language: {lang}"));
        if is_port {
            // A language migration of a codebase is inherently multi-file.
            score += 2;
            signals.push("language migration".to_string());
        }
    }

    let repo_url = detect_repo(prompt);
    let wants_clone = has_word(&lower, "clone")
        || lower.contains("pull the repo")
        || lower.contains("download the source")
        || lower.contains("check out the");
    let requires_repo_ingest = repo_url.is_some() || wants_clone;
    if requires_repo_ingest {
        score += 1;
        signals.push("repository ingest".to_string());
    }

    let scale_hits: Vec<&str> = SCALE_WORDS
        .iter()
        .copied()
        .filter(|w| lower.contains(w))
        .collect();
    if !scale_hits.is_empty() {
        score += 2;
        signals.push(format!("whole-repo scope: {}", scale_hits.join(", ")));
    }

    let domain_hits: Vec<&'static str> = DOMAINS
        .iter()
        .filter(|(_, kws)| kws.iter().any(|k| has_word(&lower, k)))
        .map(|(name, _)| *name)
        .collect();
    if domain_hits.len() >= 2 {
        score += (domain_hits.len() as i32) - 1;
        signals.push(format!(
            "{} domains: {}",
            domain_hits.len(),
            domain_hits.join(", ")
        ));
    }

    let verb_hits: Vec<&str> = VERBS
        .iter()
        .copied()
        .filter(|v| has_word(&lower, v))
        .collect();
    match verb_hits.len() {
        0..=1 => {}
        2 => {
            score += 1;
            signals.push(format!("2 deliverable verbs: {}", verb_hits.join(", ")));
        }
        _ => {
            score += 2;
            signals.push(format!("{} deliverable verbs", verb_hits.len()));
        }
    }

    if SEQUENCE_WORDS.iter().any(|w| lower.contains(w)) {
        score += 1;
        signals.push("multi-step narrative".to_string());
    }
    if count_list_markers(prompt) >= 2 {
        score += 1;
        signals.push("enumerated deliverables".to_string());
    }
    if prompt.trim().chars().count() > 240 {
        score += 1;
        signals.push("long request".to_string());
    }
    // A deliberate team ask is treated as a direct instruction: a single hit
    // on team/swarm/orchestrate/etc. is enough to cross the threshold on its
    // own, because the user explicitly asked for a coordinated build-out.
    if TEAM_WORDS.iter().any(|w| has_word(&lower, w)) {
        score += TEAM_THRESHOLD;
        signals.push("explicit team ask".to_string());
    }
    if SINGLE_WORDS.iter().any(|w| lower.contains(w)) {
        score -= 2;
        signals.push("explicit small scope".to_string());
    }

    let scope = if score >= TEAM_THRESHOLD {
        TaskScope::Team
    } else {
        TaskScope::SingleAgent
    };

    // ── Horizon: scored independently of `score`, because width and duration
    // are different axes (see [`TaskHorizon`]).
    let mut horizon_score: i32 = 0;
    if is_port && requires_repo_ingest {
        // "Clone X and rewrite it in Y" has no short version: the tree has to be
        // read, decomposed and ported, and none of that fits one session.
        horizon_score += LONG_HORIZON_THRESHOLD;
        signals.push("porting an external repository".to_string());
    }
    let long_hits: Vec<&str> = LONG_HORIZON_WORDS
        .iter()
        .copied()
        .filter(|w| lower.contains(w))
        .collect();
    if !long_hits.is_empty() {
        horizon_score += 2;
        signals.push(format!("codebase-wide scope: {}", long_hits.join(", ")));
    }
    let unattended_hits: Vec<&str> = UNATTENDED_WORDS
        .iter()
        .copied()
        .filter(|w| lower.contains(w))
        .collect();
    let wants_unattended = !unattended_hits.is_empty();
    if wants_unattended {
        horizon_score += 2;
        signals.push(format!(
            "asked to run unattended: {}",
            unattended_hits.join(", ")
        ));
    }
    if verb_hits.len() >= 4 {
        // Four distinct deliverables in one request is a programme of work, not
        // a task, whatever each of them would cost alone.
        horizon_score += 1;
        signals.push(format!("{} distinct deliverables", verb_hits.len()));
    }
    let horizon = if horizon_score >= LONG_HORIZON_THRESHOLD {
        TaskHorizon::Long
    } else {
        TaskHorizon::Short
    };

    let roles = if scope == TaskScope::Team {
        derive_roles(
            target_language,
            is_port,
            &domain_hits,
            horizon == TaskHorizon::Long,
        )
    } else {
        Vec::new()
    };

    let confidence = match scope {
        TaskScope::Team => (0.55 + score as f32 * 0.06).min(0.95),
        TaskScope::SingleAgent => (0.9 - score.max(0) as f32 * 0.1).clamp(0.55, 0.95),
    };

    let rationale = match scope {
        TaskScope::Team => format!(
            "Sprawling request (score {score}, {} horizon at {horizon_score}): {} — staff {} specialist(s).",
            horizon.as_str(),
            signals.join("; "),
            roles.len()
        ),
        TaskScope::SingleAgent => {
            format!("Focused request (score {score}): handle with a single agent.")
        }
    };

    IntentDecision {
        scope,
        requires_repo_ingest,
        repo_url,
        target_language: target_language.map(|s| s.to_string()),
        roles,
        rationale,
        confidence,
        signals,
        horizon,
        wants_unattended,
    }
}

/// Turn an [`IntentDecision`] into a persisted [`crate::editor::expert_team::ExpertTeam`].
///
/// Member 0 is the lead; every remaining [`RoleSpec`] becomes a specialist with
/// its skills and scope patterns wired for [`crate::editor::team_router::route_member`].
/// Idempotent on the team slug: re-provisioning replaces the roster in place.
/// Providers/models are left at the workspace default so dispatch inherits the
/// active model (the same convention `create_team_quick` uses).
pub fn provision_team(
    root: &std::path::Path,
    decision: &IntentDecision,
    team_name: &str,
) -> Result<String, String> {
    use crate::agent::AiProvider;
    use crate::editor::expert_team::{
        load_expert_teams, save_expert_teams, slugify, ExpertMember, ExpertTeam,
    };

    if decision.roles.is_empty() {
        return Err("decision has no roles to staff (scope is single-agent)".to_string());
    }
    let slug = slugify(team_name);
    if slug.is_empty() {
        return Err("team name must contain alphanumeric characters".to_string());
    }
    let team_id = format!("team_{slug}");

    let members: Vec<ExpertMember> = decision
        .roles
        .iter()
        .enumerate()
        .map(|(i, r)| ExpertMember {
            id: format!("member_{slug}_{}", i + 1),
            name: r.role.clone(),
            role: r.role.clone(),
            provider: AiProvider::CloudflareWorkersAi,
            model_id: String::new(),
            skills: if r.skills.is_empty() {
                vec!["system_tools".to_string()]
            } else {
                r.skills.clone()
            },
            scope_patterns: r.scope_patterns.clone(),
            tools: Vec::new(),
            workflow_instructions: String::new(),
            fallback_provider: None,
        })
        .collect();

    let new_team = ExpertTeam {
        id: team_id.clone(),
        name: team_name.to_string(),
        description: format!("Auto-provisioned from intent: {}", decision.rationale),
        members,
        is_preset: false,
    };

    let mut teams = load_expert_teams(root);
    if let Some(existing) = teams
        .iter_mut()
        .find(|t| t.id == team_id || t.slug() == slug)
    {
        *existing = new_team;
    } else {
        teams.push(new_team);
    }
    if !save_expert_teams(root, &teams) {
        return Err("failed to persist expert_teams.nda".to_string());
    }
    Ok(slug)
}

/// Confidence floor below which a networked decision model's *bounded* answer
/// is not adopted (target language, repo ingest). Deliberately conservative:
/// an uncertain reflex must not silently redirect a request.
pub const MODEL_CONFIDENCE_FLOOR: f32 = 0.60;

/// Adoption bar for the **scope** call, strictly higher than
/// [`MODEL_CONFIDENCE_FLOOR`]. Calibrated against a seven-task battery on the
/// 421M reflex model: its binary team/single discrimination sits in a
/// 0.30-0.63 P(selected) band for *every* task — full-repo rewrites score
/// within noise of "fix a typo" — while its language and ingest answers are
/// sharp. So a scope answer only overrides the deterministic routing when the
/// model is decisively on one side; otherwise the lexical evidence (whole-repo
/// cues, deliverable counts), which is precise on exactly these cases, wins.
pub const SCOPE_ADOPTION_BAR: f32 = 0.70;

/// A noul answer is adopted only outside this dead band around 0.5; inside it
/// the reflex has no opinion and the deterministic signal is kept.
pub const NOUL_DEAD_BAND: (f32, f32) = (0.35, 0.65);

/// The intent routing **decision matrix**: the typed questions every backend
/// (Laya, Jev, or any future System-1 model) is asked to route a request. This
/// is the first matrix in the system; adding another bounded question here (an
/// approval gate, a model-tier choice, an urgency score) is inherited by all
/// configured backends without touching their clients.
pub fn intent_questions() -> Vec<Question> {
    let mut langs: Vec<String> = LANGUAGES.iter().map(|s| (*s).to_string()).collect();
    langs.push("none".to_string());
    vec![
        Question::Choice {
            id: "scope".to_string(),
            prompt: "Handle with one agent, or does it sprawl across modules/domains and need a team of specialists?".to_string(),
            choices: vec!["single_agent".to_string(), "team".to_string()],
            // Descriptions are what lift a small reflex model off a coin flip:
            // llama.cpp scores each label against its annotation.
            descriptions: vec![
                "a focused change on one module or surface; a single agent is enough".to_string(),
                "spans many modules, domains, or an entire repository; needs several specialists working in parallel".to_string(),
            ],
        },
        Question::Noul {
            id: "requires_repo_ingest".to_string(),
            prompt: "Must an external source repository be pulled (git clone) before any work can begin?".to_string(),
        },
        Question::Choice {
            id: "target_language".to_string(),
            prompt: "If this is a port/rewrite to one specific language, which target language? Otherwise 'none'.".to_string(),
            choices: langs,
            descriptions: Vec::new(),
        },
        Question::Score {
            id: "complexity".to_string(),
            prompt: "How large and complex is the overall task?".to_string(),
            levels: vec![
                "trivial".to_string(),
                "small".to_string(),
                "moderate".to_string(),
                "large".to_string(),
                "sprawling".to_string(),
            ],
        },
        // Separate from `complexity` on purpose: complexity measures how hard
        // the work is, horizon measures how long it runs. A hard algorithm is
        // complex and short; porting thousands of easy files is simple and very
        // long. Only the second one needs checkpoints and an unattended runner.
        Question::Choice {
            id: "horizon".to_string(),
            prompt: "Will this finish inside one working session, or run for many hours across hundreds of steps?".to_string(),
            choices: vec!["short".to_string(), "long".to_string()],
            descriptions: vec![
                "a bounded change that completes in one session while someone watches".to_string(),
                "a repository-scale effort spanning many hours and hundreds of steps, needing checkpoints because no human is present".to_string(),
            ],
        },
    ]
}

/// Assemble an [`IntentDecision`] from a model's typed answers, adopting each
/// answer only where that question type has demonstrated discrimination (see
/// [`SCOPE_ADOPTION_BAR`] and [`NOUL_DEAD_BAND`]). The deterministic pass owns
/// the operational roster and keeps repository-URL extraction honest — a
/// reflex model must never invent a clone URL, so `repo_url` and the derived
/// `roles` come from the evidence-backed assembly regardless.
///
/// Returns `None` only when the scope answer is missing or unrecognized —
/// a weak-but-valid scope answer no longer discards the model's sharp
/// language/ingest calls; it just doesn't override the deterministic scope.
fn intent_from_answers(prompt: &str, out: &decision::DecisionOutput) -> Option<IntentDecision> {
    let base = classify(prompt);

    // The probability of the *selected* option from the distribution when
    // available; the `confidence` field is a conservative margin
    // (P(top) - P(runner)) that underestimates binary answers, where P(top)
    // is the meaningful certainty.
    let selected_prob = |selected: &str, probabilities: &[(String, f32)], confidence: f32| {
        probabilities
            .iter()
            .find(|(label, _)| label == selected)
            .map(|(_, p)| *p)
            .unwrap_or(confidence)
    };

    let (model_scope, model_scope_p) = match out.get("scope")? {
        Answer::Choice {
            selected,
            probabilities,
            confidence,
            ..
        } => {
            let scope = match selected.as_str() {
                "team" => TaskScope::Team,
                "single_agent" => TaskScope::SingleAgent,
                _ => return None,
            };
            (scope, selected_prob(selected, probabilities, *confidence))
        }
        _ => return None,
    };

    // Scope: the model only wins when decisively past the adoption bar; the
    // reflex's binary scope discrimination is otherwise noise-band and the
    // lexical evidence is kept.
    let (scope, scope_from_model) = if model_scope_p >= SCOPE_ADOPTION_BAR {
        (model_scope, true)
    } else {
        (base.scope, false)
    };
    let effective_confidence = if scope_from_model {
        model_scope_p
    } else {
        base.confidence
    };

    let (requires_repo_ingest, ingest_from_model) = match out.get("requires_repo_ingest") {
        Some(Answer::Noul {
            yes_probability, ..
        }) => {
            let y = *yes_probability;
            if y >= NOUL_DEAD_BAND.1 {
                (true, true)
            } else if y <= NOUL_DEAD_BAND.0 {
                (false, true)
            } else {
                (base.requires_repo_ingest, false)
            }
        }
        _ => (base.requires_repo_ingest, false),
    };
    let (model_language, language_from_model) = match out.get("target_language") {
        Some(Answer::Choice {
            selected,
            probabilities,
            confidence,
            ..
        }) if selected != "none"
            && selected_prob(selected, probabilities, *confidence) >= MODEL_CONFIDENCE_FLOOR =>
        {
            (Some(selected.clone()), true)
        }
        _ => (base.target_language.clone(), false),
    };
    let complexity = match out.get("complexity") {
        Some(Answer::Score { value, .. }) => Some(*value),
        _ => None,
    };

    // Horizon is adopted from the model on the same bar as scope, because it is
    // the same kind of binary judgement. `wants_unattended` is *not*: that field
    // is the user's authorization to spend without supervision, and a reflex
    // model reading the phrase "autonomous" out of a quoted example must not be
    // able to grant it. It always comes from the deterministic pass.
    let (horizon, horizon_from_model) = match out.get("horizon") {
        Some(Answer::Choice {
            selected,
            probabilities,
            confidence,
            ..
        }) if selected_prob(selected, probabilities, *confidence) >= SCOPE_ADOPTION_BAR => {
            match selected.as_str() {
                "long" => (TaskHorizon::Long, true),
                "short" => (TaskHorizon::Short, true),
                _ => (base.horizon, false),
            }
        }
        _ => (base.horizon, false),
    };

    // A target language from the model implies a port, even if the deterministic
    // verb list missed it; that keeps the Port Engineer on the roster.
    let is_port = base.signals.iter().any(|s| s == "port/rewrite verb") || model_language.is_some();
    let domain_names: Vec<&'static str> = DOMAINS
        .iter()
        .filter(|(name, _)| {
            base.roles
                .iter()
                .any(|r| r.role.starts_with(&domain_title(name)))
        })
        .map(|(name, _)| *name)
        .collect();

    let roles = if scope == TaskScope::Team {
        derive_roles(
            model_language.as_deref(),
            is_port,
            &domain_names,
            horizon.is_long(),
        )
    } else {
        Vec::new()
    };

    let mut signals = vec![format!("decision backend: {}", out.backend)];
    signals.extend(base.signals.iter().cloned());
    signals.push(format!(
        "scope owned by {}: model P(selected)={model_scope_p:.2} vs bar {SCOPE_ADOPTION_BAR:.2}",
        if scope_from_model {
            "model"
        } else {
            "deterministic pass"
        }
    ));
    if ingest_from_model {
        signals.push("repo-ingest adopted from model noul".to_string());
    }
    if language_from_model {
        signals.push("target language adopted from model".to_string());
    }
    if let Some(c) = complexity {
        signals.push(format!("model complexity score: {c:.2}"));
    }
    signals.push(format!(
        "horizon owned by {}: {}",
        if horizon_from_model {
            "model"
        } else {
            "deterministic pass"
        },
        horizon.as_str()
    ));

    let scope_noun = match scope {
        TaskScope::Team => "a team",
        TaskScope::SingleAgent => "a single agent",
    };
    let rationale = if scope_from_model {
        format!(
            "{} decision model routed to {scope_noun} (confidence {effective_confidence:.2}).{}",
            capitalize(&out.backend),
            if scope == TaskScope::Team {
                format!(" Staff {} specialist(s).", roles.len())
            } else {
                String::new()
            }
        )
    } else {
        format!(
            "{} scope signal too weak (P={model_scope_p:.2} < {SCOPE_ADOPTION_BAR:.2}); deterministic routing kept {scope_noun}.",
            capitalize(&out.backend)
        )
    };

    Some(IntentDecision {
        scope,
        requires_repo_ingest,
        repo_url: base.repo_url,
        target_language: model_language,
        roles,
        rationale,
        confidence: effective_confidence,
        signals,
        horizon,
        wants_unattended: base.wants_unattended,
    })
}

fn domain_title(name: &str) -> String {
    match name {
        "frontend" => "Frontend".to_string(),
        "backend" => "Backend".to_string(),
        "database" => "Database".to_string(),
        "auth" => "Security".to_string(),
        "testing" => "Test".to_string(),
        "docs" => "Documentation".to_string(),
        "deployment" => "DevOps".to_string(),
        _ => String::new(),
    }
}

/// Route a request through the *configured* decision brain, degrading to the
/// deterministic [`classify`] whenever the model is absent, unreachable, or
/// unsure. This is the entry point callers (and the chat hook) should use.
pub fn decide_intent(prompt: &str) -> IntentDecision {
    let backend = decision::resolve_backend();
    if let Some(model) = backend.model() {
        let state = DecisionState::text(prompt);
        let questions = intent_questions();
        // A failed reflex call is not an error: fall through to the deterministic
        // scorer so routing never depends on the network.
        if let Ok(out) = model.decide(&state, &questions) {
            if let Some(decision) = intent_from_answers(prompt, &out) {
                return decision;
            }
        }
    }
    classify(prompt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_question_is_single_agent() {
        let d = classify("What does the read_file tool do?");
        assert_eq!(d.scope, TaskScope::SingleAgent);
        assert!(d.roles.is_empty());
    }

    #[test]
    fn small_edit_stays_single_agent() {
        let d = classify("rename the variable count to total in one file");
        assert_eq!(d.scope, TaskScope::SingleAgent);
    }

    #[test]
    fn clone_and_rewrite_in_rust_is_team_with_repo_and_language() {
        let d = classify("clone liteLLM and rewrite it in rust");
        assert_eq!(d.scope, TaskScope::Team, "signals: {:?}", d.signals);
        assert!(d.requires_repo_ingest);
        assert_eq!(d.target_language.as_deref(), Some("rust"));
        // "liteLLM" is a bare name (no owner/repo), so no URL can be honestly
        // synthesized; the request still flags that a repo must be ingested.
        assert!(
            d.repo_url.is_none(),
            "must not fabricate a URL from a bare name, got {:?}",
            d.repo_url
        );
        assert!(!d.roles.is_empty());
        assert!(d.roles.iter().any(|r| r.role.contains("Rust")));
        assert!(d.roles.iter().any(|r| r.role == "Lead Architect"));
        assert!(d.roles.iter().any(|r| r.role == "Reviewer"));
    }

    #[test]
    fn explicit_github_url_is_extracted() {
        let d = classify("please port https://github.com/BerriAI/litellm.git to rust");
        assert_eq!(
            d.repo_url.as_deref(),
            Some("https://github.com/BerriAI/litellm.git")
        );
        assert_eq!(d.target_language.as_deref(), Some("rust"));
    }

    #[test]
    fn multi_domain_full_build_is_team() {
        let d = classify(
            "build the entire app end to end: design the api, implement the frontend ui, \
             set up the database schema, add auth, and write the tests",
        );
        assert_eq!(d.scope, TaskScope::Team);
        assert!(d.roles.len() >= 4);
    }

    #[test]
    fn explicit_team_word_promotes_to_team() {
        let d = classify("orchestrate a team to refactor the billing module");
        assert_eq!(d.scope, TaskScope::Team);
    }

    #[test]
    fn single_word_demotes_a_longish_prompt() {
        // Long-ish but explicitly small: the "just " marker subtracts.
        let d = classify("just fix the typo in the readme, nothing else");
        assert_eq!(d.scope, TaskScope::SingleAgent);
    }

    #[test]
    fn go_word_does_not_false_positive_on_google() {
        // "go" is a language; ensure a prompt about "google" is not read as a
        // Go migration target.
        let d = classify("explain how the google oauth flow works here");
        assert_ne!(d.target_language.as_deref(), Some("go"));
    }

    #[test]
    fn confidence_is_bounded() {
        let team = classify("clone the repo and rewrite the entire codebase in rust end to end");
        assert!((0.0..=1.0).contains(&team.confidence));
        let single = classify("hi");
        assert!((0.0..=1.0).contains(&single.confidence));
    }

    #[test]
    fn provision_team_writes_a_persisted_roster() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let d = classify("clone liteLLM and rewrite it in rust");
        let slug = provision_team(root, &d, "LiteLLM Rust Port").unwrap();
        assert_eq!(slug, "litellm-rust-port");
        let teams = crate::editor::expert_team::load_expert_teams(root);
        let t = teams
            .iter()
            .find(|t| t.slug() == slug)
            .expect("team persisted");
        assert_eq!(t.members.len(), d.roles.len());
        assert_eq!(t.members[0].role, "Lead Architect");
        assert!(t.members.iter().any(|m| m.role.contains("Rust")));
    }

    #[test]
    fn provision_team_is_idempotent_on_slug() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let d = classify("clone liteLLM and rewrite it in rust");
        provision_team(root, &d, "LiteLLM Rust Port").unwrap();
        provision_team(root, &d, "LiteLLM Rust Port").unwrap();
        let teams = crate::editor::expert_team::load_expert_teams(root);
        let matching = teams
            .iter()
            .filter(|t| t.slug() == "litellm-rust-port")
            .count();
        assert_eq!(matching, 1, "re-provisioning must replace, not duplicate");
    }

    #[test]
    fn provision_team_rejects_single_agent_decision() {
        let tmp = tempfile::tempdir().unwrap();
        let d = classify("what is the capital of france");
        assert!(provision_team(tmp.path(), &d, "Nope").is_err());
    }

    // ---- decision-matrix (Laya/Jev) assembly path ----

    use crate::agent::decision::{DecisionOutput, Question as DQuestion};
    use std::collections::HashMap;

    fn scope_out(selected: &str, confidence: f32) -> DecisionOutput {
        // The probability of `selected` is `confidence`; the runner-up gets the remainder.
        let (sa_prob, team_prob) = if selected == "single_agent" {
            (confidence, 1.0 - confidence)
        } else {
            (1.0 - confidence, confidence)
        };
        let mut answers = HashMap::new();
        answers.insert(
            "scope".to_string(),
            Answer::Choice {
                id: "scope".to_string(),
                selected: selected.to_string(),
                probabilities: vec![
                    ("single_agent".to_string(), sa_prob),
                    ("team".to_string(), team_prob),
                ],
                confidence,
            },
        );
        DecisionOutput {
            answers,
            backend: "laya".to_string(),
        }
    }

    #[test]
    fn intent_questions_declares_the_routing_matrix() {
        let qs = intent_questions();
        let ids: Vec<&str> = qs.iter().map(DQuestion::id).collect();
        assert_eq!(
            ids,
            vec![
                "scope",
                "requires_repo_ingest",
                "target_language",
                "complexity",
                "horizon"
            ]
        );
        match &qs[0] {
            DQuestion::Choice { choices, .. } => assert_eq!(
                choices,
                &vec!["single_agent".to_string(), "team".to_string()]
            ),
            other => panic!("scope must be a Choice, got {other:?}"),
        }
        match &qs[4] {
            DQuestion::Choice {
                choices,
                descriptions,
                ..
            } => {
                assert_eq!(choices, &vec!["short".to_string(), "long".to_string()]);
                assert_eq!(
                    descriptions.len(),
                    choices.len(),
                    "a reflex model scores each label against its annotation; an \
                     unannotated choice is a coin flip"
                );
            }
            other => panic!("horizon must be a Choice, got {other:?}"),
        }
    }

    #[test]
    fn model_team_answer_assembles_a_team_decision() {
        let mut out = scope_out("team", 0.9);
        out.answers.insert(
            "requires_repo_ingest".to_string(),
            Answer::Noul {
                id: "requires_repo_ingest".to_string(),
                yes_probability: 0.8,
            },
        );
        out.answers.insert(
            "target_language".to_string(),
            Answer::Choice {
                id: "target_language".to_string(),
                selected: "rust".to_string(),
                probabilities: vec![("rust".to_string(), 0.9)],
                confidence: 0.9,
            },
        );
        out.answers.insert(
            "complexity".to_string(),
            Answer::Score {
                id: "complexity".to_string(),
                value: 0.8,
                level: Some("sprawling".to_string()),
                confidence: 0.7,
            },
        );
        let d = intent_from_answers("port litellm to rust", &out).expect("assembles");
        assert_eq!(d.scope, TaskScope::Team);
        assert!(d.requires_repo_ingest);
        assert_eq!(d.target_language.as_deref(), Some("rust"));
        assert!((d.confidence - 0.9).abs() < 1e-6);
        assert!(d.roles.iter().any(|r| r.role.contains("Rust")));
        assert!(d.roles.iter().any(|r| r.role == "Lead Architect"));
        assert!(d.rationale.contains("Laya"));
        assert!(d.signals.iter().any(|s| s.starts_with("decision backend:")));
    }

    #[test]
    fn weak_scope_answer_keeps_deterministic_routing() {
        // A valid-but-uncertain scope answer (P below the adoption bar) must
        // not hijack routing: the lexical pass keeps the scope, but the
        // assembly still succeeds (other answers stay available for adoption).
        let mut out = scope_out("team", 0.4);
        out.answers.insert(
            "target_language".to_string(),
            Answer::Choice {
                id: "target_language".to_string(),
                selected: "rust".to_string(),
                probabilities: vec![("rust".to_string(), 0.95)],
                confidence: 0.9,
            },
        );
        let d = intent_from_answers(
            "clone litellm and rewrite the entire codebase in rust",
            &out,
        )
        .expect("assembles");
        assert_eq!(
            d.scope,
            TaskScope::Team,
            "lexical pass routes sprawl to a team"
        );
        assert_eq!(d.target_language.as_deref(), Some("rust"));
        assert!(d.rationale.contains("too weak"));
        assert!(d.signals.iter().any(|s| s.starts_with("scope owned by")));
    }

    #[test]
    fn noisy_single_agent_scope_cannot_downgrade_a_whole_repo_task() {
        // The crucible case: Laya answers single_agent at P=0.63 for a
        // full-repo rewrite. That must NOT flip the routing to one agent.
        let out = scope_out("single_agent", 0.63);
        let d = intent_from_answers(
            "clone https://github.com/BerriAI/litellm.git and rewrite the entire codebase in rust",
            &out,
        )
        .expect("assembles");
        assert_eq!(d.scope, TaskScope::Team);
        assert!(!d.roles.is_empty(), "team routing keeps a roster");
    }

    #[test]
    fn unknown_scope_label_is_rejected() {
        let out = scope_out("both", 0.95);
        assert!(intent_from_answers("do a thing", &out).is_none());
    }

    #[test]
    fn single_agent_answer_produces_no_roles() {
        let mut out = scope_out("single_agent", 0.88);
        out.answers.insert(
            "target_language".to_string(),
            Answer::Choice {
                id: "target_language".to_string(),
                selected: "none".to_string(),
                probabilities: vec![],
                confidence: 0.9,
            },
        );
        let d = intent_from_answers("what does read_file do", &out).expect("assembles");
        assert_eq!(d.scope, TaskScope::SingleAgent);
        assert!(d.roles.is_empty());
    }

    #[test]
    fn decide_intent_uses_heuristic_when_no_backend_configured() {
        std::env::remove_var("VELOCITY_DECISION_BACKEND");
        let prompt = "clone liteLLM and rewrite it in rust";
        let d = decide_intent(prompt);
        // With no Laya/Jev wired up this must equal the deterministic scorer.
        assert_eq!(d, classify(prompt));
    }

    // ── Horizon ───────────────────────────────────────────────────────────

    #[test]
    fn a_repository_port_is_long_horizon() {
        let d = classify("clone BerriAI/litellm and rewrite it in rust");
        assert!(d.requires_repo_ingest);
        assert_eq!(d.horizon, TaskHorizon::Long);
        assert!(
            d.signals
                .iter()
                .any(|s| s == "porting an external repository"),
            "{:?}",
            d.signals
        );
    }

    /// The request that motivated the horizon axis, verbatim.
    ///
    /// It names no repository and no target language, so the port rule cannot
    /// fire — it has to be recognised as long-horizon from the shape of the ask
    /// alone (a whole-codebase rewrite, delivered in one pass, with nobody
    /// supervising). If this classifies as short, the mission runner never runs.
    #[test]
    fn the_litellm_scale_rewrite_request_is_long_horizon_and_unattended() {
        let prompt = "see if it can properly handle a job the scale of LiteLLM, split the \
                      rewrite task into sectors, create a team specialized for the task and \
                      execute the full rewrite end to end, including testing, benchmarking \
                      and documentation in 1 pass. if it's a long-horizon task, run it end to \
                      end without human intervention, utilizing multi-agent as much as \
                      applicable and doing security, durability and performance sweeps over \
                      its work";
        let d = classify(prompt);
        assert_eq!(d.horizon, TaskHorizon::Long, "{:?}", d.signals);
        assert!(
            d.wants_unattended,
            "the user said 'without human intervention'"
        );
        assert_eq!(d.scope, TaskScope::Team, "{:?}", d.signals);
        // The three sweeps the user named must actually be on the roster.
        for wanted in [
            "Security Sweep Auditor",
            "Durability Engineer",
            "Performance Engineer",
            "Documentation Engineer",
        ] {
            assert!(
                d.roles.iter().any(|r| r.role == wanted),
                "{wanted} missing from {:?}",
                d.roles.iter().map(|r| &r.role).collect::<Vec<_>>()
            );
        }
    }

    /// Difficulty is not duration. This is a genuinely hard task that one agent
    /// finishes inside a session; sending it to a resumable multi-hour mission
    /// runner would be pure overhead.
    #[test]
    fn a_hard_but_bounded_task_is_short_horizon() {
        let d = classify("implement a lock-free red-black tree with correct memory ordering");
        assert_eq!(d.horizon, TaskHorizon::Short, "{:?}", d.signals);
        assert!(!d.wants_unattended);
        assert!(d.roles.iter().all(|r| r.role != "Security Sweep Auditor"));
    }

    /// "Rewrite" alone must not imply long-horizon, or every small refactor
    /// would be routed to the mission runner.
    #[test]
    fn rewriting_one_function_is_short_horizon() {
        let d = classify("rewrite this helper function in rust");
        assert_eq!(d.target_language.as_deref(), Some("rust"));
        assert_eq!(d.horizon, TaskHorizon::Short, "{:?}", d.signals);
        assert!(!d.wants_unattended);
    }

    #[test]
    fn a_long_horizon_team_staffs_the_sweeps_and_a_short_one_does_not() {
        let long = classify(
            "clone BerriAI/litellm and do a full rewrite of the entire codebase in rust, \
             end to end",
        );
        assert_eq!(long.horizon, TaskHorizon::Long);
        assert!(long
            .roles
            .iter()
            .any(|r| r.role == "Security Sweep Auditor"));
        assert!(long.roles.iter().any(|r| r.role == "Durability Engineer"));
        assert!(long.roles.iter().any(|r| r.role == "Performance Engineer"));

        let short = classify("add a retry helper to the api client and test it");
        assert_eq!(short.horizon, TaskHorizon::Short);
        assert!(
            short
                .roles
                .iter()
                .all(|r| r.role != "Security Sweep Auditor" && r.role != "Durability Engineer"),
            "a bounded task should not staff a sweep roster: {:?}",
            short.roles.iter().map(|r| &r.role).collect::<Vec<_>>()
        );
    }

    /// The sweeps must not duplicate the domain-derived Security Engineer: one
    /// owns the auth code, the other audits everything the mission produced.
    #[test]
    fn the_sweep_auditor_coexists_with_the_domain_security_engineer() {
        let d = classify(
            "clone acme/shop and do a full rewrite of the entire codebase in rust, including \
             the auth and login flows, end to end",
        );
        let titles: Vec<&str> = d.roles.iter().map(|r| r.role.as_str()).collect();
        assert!(titles.contains(&"Security Engineer"), "{titles:?}");
        assert!(titles.contains(&"Security Sweep Auditor"), "{titles:?}");
        let mut sorted = titles.clone();
        sorted.sort_unstable();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(
            before,
            sorted.len(),
            "duplicate role on the roster: {titles:?}"
        );
    }

    /// Authorizing an unattended run is a spend decision that belongs to the
    /// user's text. A model that reads "autonomous" out of a quoted example must
    /// not be able to grant it.
    #[test]
    fn a_model_cannot_authorize_an_unattended_run() {
        let mut out = scope_out("team", 0.9);
        out.answers.insert(
            "horizon".to_string(),
            Answer::Choice {
                id: "horizon".to_string(),
                selected: "long".to_string(),
                probabilities: vec![("long".to_string(), 0.95)],
                confidence: 0.9,
            },
        );
        // No unattended phrasing anywhere in the request.
        let d = intent_from_answers("port the payments module to rust", &out).expect("assembles");
        assert_eq!(d.horizon, TaskHorizon::Long, "horizon is adoptable");
        assert!(
            !d.wants_unattended,
            "the model said long, but only the user can say unattended: {:?}",
            d.signals
        );
        assert!(
            d.signals
                .iter()
                .any(|s| s == "horizon owned by model: long"),
            "{:?}",
            d.signals
        );
    }

    #[test]
    fn a_weak_model_horizon_answer_keeps_the_deterministic_call() {
        let out = scope_out("team", 0.9);
        // A bare `scope_out` carries no horizon answer at all, so the
        // deterministic verdict must survive untouched.
        let d = intent_from_answers(
            "clone BerriAI/litellm and rewrite the entire codebase in rust",
            &out,
        )
        .expect("assembles");
        assert_eq!(d.horizon, TaskHorizon::Long);
        assert!(
            d.signals
                .iter()
                .any(|s| s == "horizon owned by deterministic pass: long"),
            "{:?}",
            d.signals
        );
    }
}
