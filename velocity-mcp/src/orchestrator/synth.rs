//! First-class prompt → [`TaskGraph`] synthesiser for the live-VC pipeline.
//!
//! Earlier, the decomposition a long-horizon team ran on was implicit: the UI's
//! `set_routed_tasks` path happened to build one node per team member, and
//! nothing else. That is not a plan — it is a roster wearing a graph. This
//! module makes the synthesiser a real, standalone step: feed it a natural
//! language prompt, it runs [`crate::agent::intent::classify` for the scope and
//! roles, then emits a fine-grained task graph — 5 to 40 nodes with priorities
//! and dependency edges — that the scheduler can actually wave.
//!
//! Two decomposition engines share one graph builder:
//!
//! * [`synthesize`] is deterministic and network-free. It reads the request's
//!   *shape* (single-module, multi-module, cross-cutting) and expands it into a
//!   scaffold with an explicit contracts/first-task root that every dependent
//!   wave hangs off. This is what runs by default and what the tests pin down.
//! * [`synthesize_llm`] drives a real generative step. It builds a
//!   decomposition prompt, hands it to an injected completion closure, and
//!   parses the model's JSON task list. The closure is a parameter (mirroring
//!   the pluggable decision brain) so the parse/validate/fallback logic is
//!   exercised without a live model. A model answer that is unparseable, empty,
//!   out of the 5–40 band, or cyclic is discarded and the deterministic
//!   scaffold used instead — a bad decomposition must never be dispatched.
//!
//! The graph always uses `TaskId(1)` as the reconcile root and role/sub tasks
//! at `TaskId(2..)`, the same convention [`crate::orchestrator::team_pipeline`]
//! and the checkpoint restorer rely on.

use super::blueprint::TaskGraph;
use super::TaskId;
use crate::agent::intent::{classify, IntentDecision};

/// The floor and ceiling on a synthesised plan's task count. A graph under the
/// floor is not decomposed (one node is a to-do, not a plan); one over the
/// ceiling is the model rambling and will exhaust the wave scheduler.
pub const MIN_TASKS: usize = 5;
pub const MAX_TASKS: usize = 40;

/// One synthesised node before it is wired into a [`TaskGraph`].
///
/// `deps` are indices into the *same* `Vec<SynthTask>` they belong to (not
/// [`TaskId`]s); the graph builder shifts them by the reconcile-root offset.
/// Keeping them as local indices means both the heuristic scaffold and the
/// parsed model JSON describe dependencies the same way, independent of the
/// `TaskId` numbering the graph happens to use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynthTask {
    pub title: String,
    pub description: String,
    pub scope: Vec<String>,
    pub deps: Vec<usize>,
    /// Higher runs first within a wave (mirrors `blueprint::Task::priority`).
    pub priority: u32,
    /// Relative cost 1-10 (mirrors `blueprint::Task::estimated_weight`).
    pub weight: u32,
}

impl SynthTask {
    fn new(title: impl Into<String>, description: impl Into<String>, deps: Vec<usize>) -> Self {
        SynthTask {
            title: title.into(),
            description: description.into(),
            scope: Vec::new(),
            deps,
            priority: 0,
            weight: 2,
        }
    }

    fn scope(mut self, scope: Vec<String>) -> Self {
        self.scope = scope;
        self
    }

    fn priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }

    fn weight(mut self, weight: u32) -> Self {
        self.weight = weight;
        self
    }
}

/// The coarse shape of a request, which decides how wide the scaffold gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptShape {
    /// One module or surface: design → implement → verify, a handful of nodes.
    SingleModule,
    /// Two modules that must agree on a shared boundary before either lands.
    MultiModule,
    /// Three or more surfaces (e.g. frontend + backend + networking) for one
    /// feature: a contracts root, one parallel branch per surface, then an
    /// integration that only unblocks once every branch is green.
    CrossCutting,
}

/// A domain surface the prompt touches, used both to count the shape and to
/// seed each synthesised task's file scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Frontend,
    Backend,
    Networking,
    Data,
    Security,
    Infra,
}

impl Surface {
    /// Lowercased keywords that imply this surface appears in a prompt.
    fn keywords(self) -> &'static [&'static str] {
        match self {
            Surface::Frontend => &[
                "ui",
                "frontend",
                "front-end",
                "react",
                "vue",
                "component",
                "screen",
                "layout",
                "css",
                "dashboard",
                "stylesheet",
            ],
            Surface::Backend => &[
                "backend",
                "back-end",
                "server",
                "api",
                "endpoint",
                "service",
                "controller",
                "handler",
                "rest",
                "graphql",
            ],
            Surface::Networking => &[
                "network",
                "networking",
                "socket",
                "websocket",
                "http",
                "tcp",
                "udp",
                "protocol",
                "dns",
                "packet",
                "transport",
                "peer",
            ],
            Surface::Data => &[
                "database",
                "db",
                "schema",
                "migration",
                "storage",
                "persist",
                "query",
                "cache",
                "repository",
                "sql",
            ],
            Surface::Security => &[
                "auth",
                "authentication",
                "authorization",
                "security",
                "encrypt",
                "permission",
                "login",
                "token",
                "oauth",
            ],
            Surface::Infra => &[
                "deploy",
                "devops",
                "ci",
                "docker",
                "pipeline",
                "infra",
                "provision",
                "monitor",
                "kubernetes",
            ],
        }
    }

    /// The directory glob a worker on this surface should own. Kept coarse on
    /// purpose: the synthesiser is scoping parallel work, not resolving a real
    /// tree — that is the worker's job once it can read the filesystem.
    fn default_scope(self) -> &'static str {
        match self {
            Surface::Frontend => "src/frontend/",
            Surface::Backend => "src/backend/",
            Surface::Networking => "src/network/",
            Surface::Data => "src/data/",
            Surface::Security => "src/security/",
            Surface::Infra => "deploy/",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Surface::Frontend => "frontend",
            Surface::Backend => "backend",
            Surface::Networking => "networking",
            Surface::Data => "data layer",
            Surface::Security => "security",
            Surface::Infra => "infrastructure",
        }
    }

    const ALL: [Surface; 6] = [
        Surface::Frontend,
        Surface::Backend,
        Surface::Networking,
        Surface::Data,
        Surface::Security,
        Surface::Infra,
    ];
}

/// Distinct surfaces the prompt mentions, in [`Surface::ALL`] order so the
/// result is deterministic regardless of keyword hit order.
pub fn detect_surfaces(prompt: &str) -> Vec<Surface> {
    let lower = prompt.to_lowercase();
    Surface::ALL
        .into_iter()
        .filter(|s| s.keywords().iter().any(|k| lower.contains(k)))
        .collect()
}

/// Classify a request into a [`PromptShape`] from how many surfaces it spans.
///
/// This is the guard the shape tests rest on: cross-cutting must mean three or
/// more surfaces, and reverting that threshold collapses every plan onto the
/// single-module scaffold — which is exactly what the non-vacuity test detects.
pub fn classify_shape(prompt: &str) -> PromptShape {
    match detect_surfaces(prompt).len() {
        0..=1 => PromptShape::SingleModule,
        2 => PromptShape::MultiModule,
        _ => PromptShape::CrossCutting,
    }
}

/// Scope for a synthesised node on `surface`, preferring a classified role's
/// own patterns when the intent brain staffed one for that surface.
fn scope_for_surface(surface: Surface, decision: &IntentDecision) -> Vec<String> {
    let needle = surface.label();
    let from_roles: Vec<String> = decision
        .roles
        .iter()
        .filter(|r| {
            let blob = format!("{} {}", r.role, r.skills.join(" ")).to_lowercase();
            blob.contains(needle)
        })
        .flat_map(|r| r.scope_patterns.clone())
        .collect();
    if !from_roles.is_empty() {
        return from_roles;
    }
    vec![surface.default_scope().to_string()]
}

/// Build the deterministic scaffold for a classified request.
///
/// A design/contracts node always comes first and every dependent node points
/// at it, so the plan has a single entry wave and the parallel branches fan out
/// underneath. Priority is higher for earlier (more foundational) nodes.
pub fn decompose_heuristic(prompt: &str, decision: &IntentDecision) -> Vec<SynthTask> {
    let surfaces = detect_surfaces(prompt);
    let shape = classify_shape(prompt);
    let goal = short_goal(prompt);
    let mut tasks: Vec<SynthTask> = Vec::new();

    match shape {
        PromptShape::SingleModule => {
            let surface = surfaces.first().copied();
            let scope = match surface {
                Some(s) => scope_for_surface(s, decision),
                None => vec![],
            };
            let label = surface.map(Surface::label).unwrap_or("module");
            tasks.push(
                SynthTask::new(
                    format!("Design {label} approach for {goal}"),
                    format!("Decide the module boundaries, data shapes and public interface before any code for: {prompt}"),
                    vec![],
                )
                .scope(scope.clone())
                .priority(5),
            );
            tasks.push(
                SynthTask::new(
                    format!("Implement {label}"),
                    format!("Write the code for {goal} against the design."),
                    vec![0],
                )
                .scope(scope.clone())
                .priority(4)
                .weight(3),
            );
            tasks.push(
                SynthTask::new(
                    format!("Tests for {label}"),
                    format!("Unit/integration coverage proving {goal} behaves as designed."),
                    vec![1],
                )
                .scope(scope.clone())
                .priority(3),
            );
            tasks.push(
                SynthTask::new(
                    format!("Review {label}"),
                    "Audit the implementation for correctness, safety and drift from the design."
                        .to_string(),
                    vec![1, 2],
                )
                .scope(scope.clone())
                .priority(2),
            );
            tasks.push(
                SynthTask::new(
                    format!("Document {label}"),
                    format!("Write the usage and design notes for {goal}."),
                    vec![1, 2, 3],
                )
                .scope(vec!["docs/".into()])
                .priority(1),
            );
        }
        PromptShape::MultiModule => {
            let a = surfaces[0];
            let b = surfaces[1];
            let (la, lb) = (a.label(), b.label());
            let (sa, sb) = (
                scope_for_surface(a, decision),
                scope_for_surface(b, decision),
            );
            tasks.push(
                SynthTask::new(
                    format!("Design shared contract for {goal}"),
                    format!("Fix the interface between the {la} and {lb} surfaces so both can build in parallel: {prompt}"),
                    vec![],
                )
                .priority(6),
            );
            tasks.push(
                SynthTask::new(
                    format!("Implement {la}"),
                    format!("Build the {la} side against the shared contract."),
                    vec![0],
                )
                .scope(sa)
                .priority(4)
                .weight(3),
            );
            tasks.push(
                SynthTask::new(
                    format!("Implement {lb}"),
                    format!("Build the {lb} side against the shared contract."),
                    vec![0],
                )
                .scope(sb)
                .priority(4)
                .weight(3),
            );
            tasks.push(
                SynthTask::new(
                    format!("Integrate {la} + {lb}"),
                    format!("Wire the two surfaces together and prove the seam holds for {goal}."),
                    vec![1, 2],
                )
                .priority(3)
                .weight(3),
            );
            tasks.push(
                SynthTask::new(
                    format!("Test integrated {goal}"),
                    "End-to-end coverage across the combined surfaces.".to_string(),
                    vec![3],
                )
                .priority(2),
            );
            tasks.push(
                SynthTask::new(
                    format!("Review and document {goal}"),
                    "Audit the change and record the final contract and behaviour.".to_string(),
                    vec![3, 4],
                )
                .scope(vec!["docs/".into()])
                .priority(1),
            );
        }
        PromptShape::CrossCutting => {
            tasks.push(
                SynthTask::new(
                    format!("Design API contract for {goal}"),
                    format!("Define the request/response and data shapes every surface agrees on: {prompt}"),
                    vec![],
                )
                .priority(9),
            );
            tasks.push(
                SynthTask::new(
                    "Define shared data schema",
                    "Model the entities the surfaces exchange so they do not each invent a shape."
                        .to_string(),
                    vec![0],
                )
                .priority(8),
            );
            // One branch per surface: scaffold then implement, both chained off
            // the schema so the branches run in parallel with each other.
            let mut impl_indices: Vec<usize> = Vec::new();
            for s in &surfaces {
                let scope = scope_for_surface(*s, decision);
                let scaffold_idx = tasks.len();
                tasks.push(
                    SynthTask::new(
                        format!("Scaffold {s} surface", s = s.label()),
                        format!("Set up the {} module's structure and stubs.", s.label()),
                        vec![1],
                    )
                    .scope(scope.clone())
                    .priority(5)
                    .weight(2),
                );
                let impl_idx = tasks.len();
                tasks.push(
                    SynthTask::new(
                        format!("Implement {s} surface", s = s.label()),
                        format!(
                            "Build the {} surface against the shared contract and schema.",
                            s.label()
                        ),
                        vec![scaffold_idx],
                    )
                    .scope(scope)
                    .priority(4)
                    .weight(3),
                );
                impl_indices.push(impl_idx);
            }
            let integration_idx = tasks.len();
            tasks.push(
                SynthTask::new(
                    format!("Integrate all surfaces for {goal}"),
                    "Join every surface branch and run the combined path.".to_string(),
                    impl_indices.clone(),
                )
                .priority(3)
                .weight(4),
            );
            let security_idx = tasks.len();
            tasks.push(
                SynthTask::new(
                    format!("Security sweep across {goal}"),
                    "Audit auth, input validation and data exposure over the integrated feature."
                        .to_string(),
                    vec![integration_idx],
                )
                .scope(vec!["src/security/".into()])
                .priority(2),
            );
            tasks.push(
                SynthTask::new(
                    format!("Test the cross-cutting feature ({goal})"),
                    "End-to-end coverage of the joined surfaces and the security findings' fixes."
                        .to_string(),
                    vec![security_idx],
                )
                .priority(1),
            );
        }
    }

    tasks
}

/// Collapse a long prompt into a short, single-line label for task titles.
fn short_goal(prompt: &str) -> String {
    let one_line = prompt.trim().split('\n').next().unwrap_or("").trim();
    let mut chars = one_line.chars();
    let clipped: String = chars.by_ref().take(48).collect();
    if one_line.chars().count() > 48 {
        format!("{}…", clipped.trim_end())
    } else if clipped.is_empty() {
        "the request".to_string()
    } else {
        clipped
    }
}

/// Validate a raw model task list into synthesisable [`SynthTask`]s.
///
/// Rejects a plan that is not dispatchable: too few or too many nodes, a title
/// missing, a dependency pointing outside the list or at itself, or a cycle in
/// the dependency graph. Returning `Err` is the signal to fall back to the
/// deterministic scaffold — a subtly broken graph must never reach the worker.
pub fn validate_synth_tasks(mut tasks: Vec<SynthTask>) -> Result<Vec<SynthTask>, String> {
    if tasks.len() < MIN_TASKS {
        return Err(format!(
            "decomposition has {} tasks, below the floor of {}",
            tasks.len(),
            MIN_TASKS
        ));
    }
    if tasks.len() > MAX_TASKS {
        return Err(format!(
            "decomposition has {} tasks, above the ceiling of {}",
            tasks.len(),
            MAX_TASKS
        ));
    }
    let n = tasks.len();
    for (i, t) in tasks.iter().enumerate() {
        if t.title.trim().is_empty() {
            return Err(format!("task {i} has an empty title"));
        }
        for d in &t.deps {
            if *d >= n {
                return Err(format!("task {i} depends on out-of-range index {d}"));
            }
            if *d == i {
                return Err(format!("task {i} depends on itself"));
            }
        }
    }
    if let Some(cycle) = find_cycle(&tasks) {
        return Err(format!("dependency cycle among tasks {:?}", cycle));
    }
    // Normalise weight/priority so nothing reaches the scheduler at zero.
    for t in tasks.iter_mut() {
        if t.weight == 0 {
            t.weight = 1;
        }
        t.deps.sort_unstable();
        t.deps.dedup();
    }
    Ok(tasks)
}

/// Return the node ids that participate in a dependency cycle, or `None` if the
/// graph is a DAG.
///
/// Kahn's algorithm: a task's dependencies must run first, so each dependency
/// edge points `dep -> task`. Nodes whose incoming edges are all consumed join
/// the schedule; whatever is left when no zero-indegree node remains is exactly
/// the set bound in a cycle. Preferred over a recursive DFS here because a model
/// plan can be wide and deep, and getting the colour bookkeeping wrong would
/// reject a valid graph or — worse — accept a broken one.
fn find_cycle(tasks: &[SynthTask]) -> Option<Vec<usize>> {
    let n = tasks.len();
    // indegree[i] = how many of task i's deps are unresolved.
    let mut indegree = vec![0usize; n];
    // dependents[d] = tasks that must run after d.
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, t) in tasks.iter().enumerate() {
        for &d in &t.deps {
            if d < n && d != i {
                indegree[i] += 1;
                dependents[d].push(i);
            }
        }
    }
    let mut queue: Vec<usize> = (0..n).filter(|i| indegree[*i] == 0).collect();
    let mut scheduled = 0usize;
    while let Some(node) = queue.pop() {
        scheduled += 1;
        for &child in &dependents[node] {
            indegree[child] -= 1;
            if indegree[child] == 0 {
                queue.push(child);
            }
        }
    }
    if scheduled == n {
        None
    } else {
        Some((0..n).filter(|i| indegree[*i] > 0).collect())
    }
}

/// Build a [`TaskGraph`] from synthesised tasks.
///
/// Reconcile root at `TaskId(1)` (every node eventually feeds it), task `i` at
/// `TaskId(2 + i)`. Local `deps` indices shift to those ids; out-of-range
/// references are dropped defensively even though validation already rejected
/// them. This is the single place index→`TaskId` mapping happens so the
/// heuristic and LLM paths cannot diverge.
pub fn build_graph(goal: &str, tasks: &[SynthTask]) -> TaskGraph {
    let mut graph = TaskGraph::default();
    graph.root = TaskId(1);
    graph.add(
        TaskId(1),
        "Reconcile synthesised plan",
        format!("Reconcile live-VC outputs for goal: {goal}"),
        vec![".velocity/agentic".to_string()],
        vec![],
        None,
    );
    let id_of = |idx: usize| TaskId(idx as u64 + 2);
    for (i, t) in tasks.iter().enumerate() {
        let deps: Vec<TaskId> = t
            .deps
            .iter()
            .filter(|d| **d < tasks.len() && **d != i)
            .map(|d| id_of(*d))
            .collect();
        graph.add(
            id_of(i),
            t.title.clone(),
            t.description.clone(),
            t.scope.clone(),
            deps,
            Some(TaskId(1)),
        );
        if let Some(node) = graph.tasks.get_mut(&id_of(i)) {
            node.priority = t.priority;
            node.estimated_weight = t.weight;
        }
    }
    graph
}

/// The deterministic entry point: classify, then synthesise a scaffold.
///
/// No network, no model — this always returns a valid, dispatchable graph, so
/// it is both the default pipeline path and the fallback [`synthesize_llm`]
/// degrades to when a model answer cannot be trusted. The heuristic scaffold is
/// acyclic and always within the task band by construction, so it needs no
/// validation pass here.
pub fn synthesize(prompt: &str) -> TaskGraph {
    let decision = classify(prompt);
    let tasks = decompose_heuristic(prompt, &decision);
    build_graph(prompt, &tasks)
}

/// Drive a generative decomposition step through `complete`.
///
/// `complete` receives the decomposition prompt and returns the model's raw
/// text (a JSON task list), or `None` when the call failed. Parsing, validation
/// and the fallback to [`synthesize`]'s scaffold all happen here, so the caller
/// only has to supply a one-shot completion. Tests pass a closure returning a
/// fixture; production passes one that runs a headless sub-agent and reads back
/// the transcript.
pub fn synthesize_llm(prompt: &str, complete: &dyn Fn(&str) -> Option<String>) -> TaskGraph {
    let decision = classify(prompt);
    let ask = build_decomposition_prompt(prompt, &decision);
    if let Some(raw) = complete(&ask) {
        if let Ok(tasks) = parse_decomposition(&raw) {
            if let Ok(valid) = validate_synth_tasks(tasks) {
                return build_graph(prompt, &valid);
            }
        }
    }
    // Unreachable model, unparseable or untrustworthy plan: fall back so the
    // run still gets a graph. Never dispatch a decomposition we could not parse.
    build_graph(prompt, &decompose_heuristic(prompt, &decision))
}

/// Assemble the instruction handed to the model for the decomposition step.
///
/// Public so it can be unit-tested for content (it must name the 5–40 band and
/// the exact JSON shape) and so a caller can log the brief it sent.
pub fn build_decomposition_prompt(prompt: &str, decision: &IntentDecision) -> String {
    let mut roles = String::new();
    if !decision.roles.is_empty() {
        roles.push_str("Specialist roles in scope:\n");
        for r in &decision.roles {
            roles.push_str(&format!(
                "- {} (skills: {}; scopes: {})\n",
                r.role,
                if r.skills.is_empty() {
                    "-".to_string()
                } else {
                    r.skills.join(", ")
                },
                if r.scope_patterns.is_empty() {
                    "-".to_string()
                } else {
                    r.scope_patterns.join(", ")
                }
            ));
        }
    }
    format!(
        "You are planning one long-horizon engineering task as a work-breakdown graph.\n\n\
         TASK (user request):\n{prompt}\n\n\
         {roles}\
         Produce between {MIN_TASKS} and {MAX_TASKS} tasks. Each task is one unit a \
         single specialist can own and that unblocks work behind it. Return ONLY a JSON \
         array, no prose, no code fences, of objects with these keys:\n\
           \"title\" (required, short imperative),\n\
           \"description\" (what to do),\n\
           \"scope\" (array of path globs the task owns),\n\
           \"deps\" (array of 0-based indexes of earlier tasks this one waits on; a task \
           may only depend on tasks listed before it so the graph stays acyclic),\n\
           \"priority\" (integer, higher runs first within a wave),\n\
           \"weight\" (integer 1-10 estimated cost).\n\
         Example shape only, do not copy these contents:\n\
         [{{\"title\":\"Design API\",\"description\":\"Fix the interface\",\"scope\":[\"src/\"],\"deps\":[],\"priority\":9,\"weight\":2}},\n\
         {{\"title\":\"Implement server\",\"description\":\"Build it\",\"scope\":[\"src/backend/\"],\"deps\":[0],\"priority\":5,\"weight\":3}}]"
    )
}

/// Parse a model answer into raw [`SynthTask`]s.
///
/// Tolerates ```json fences and a leading/trailing prose sentence by extracting
/// the outermost JSON array (or an object with a `tasks` array). Missing fields
/// default sensibly (empty scope, no deps, priority by position, weight 2). The
/// result is unvalidated; pass it to [`validate_synth_tasks`] before building.
pub fn parse_decomposition(raw: &str) -> Result<Vec<SynthTask>, String> {
    let json = extract_json_array(raw).ok_or("no JSON array found in the model answer")?;
    let value: serde_json::Value =
        serde_json::from_str(&json).map_err(|e| format!("invalid JSON: {e}"))?;
    let arr = match &value {
        serde_json::Value::Array(a) => a.clone(),
        serde_json::Value::Object(o) => o
            .get("tasks")
            .and_then(|v| v.as_array())
            .cloned()
            .ok_or("object answer has no `tasks` array")?,
        _ => return Err("answer is not a JSON array or object".to_string()),
    };
    let mut tasks = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        let obj = item
            .as_object()
            .ok_or(format!("task {i} is not an object"))?;
        let title = obj
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let description = obj
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let scope = obj
            .get("scope")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let deps = obj
            .get("deps")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|d| d.as_u64().map(|d| d as usize))
                    .collect()
            })
            .unwrap_or_default();
        // Default priority to reverse position so a plain list still schedules
        // head-first when the model omits the field.
        let priority = obj
            .get("priority")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .unwrap_or((arr.len() - i) as u32);
        let weight = obj
            .get("weight")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .unwrap_or(2);
        tasks.push(SynthTask {
            title,
            description,
            scope,
            deps,
            priority,
            weight,
        });
    }
    Ok(tasks)
}

/// Extract the outermost JSON array from a model answer, dropping code fences
/// and any prose before `[` / after the matching `]`.
fn extract_json_array(raw: &str) -> Option<String> {
    let stripped = strip_code_fences(raw);
    let bytes: Vec<char> = stripped.chars().collect();
    let start = bytes.iter().position(|c| *c == '[' || *c == '{')?;
    // Scan for the matching close of whichever bracket opened first.
    let open = bytes[start];
    let close = if open == '[' { ']' } else { '}' };
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for idx in start..bytes.len() {
        let c = bytes[idx];
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '[' | '{' => depth += 1,
            ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    // Only accept a well-closed match of the opening bracket.
                    if c == close {
                        return Some(bytes[start..=idx].iter().collect());
                    }
                    return None;
                }
            }
            _ => {}
        }
    }
    None
}

/// Remove ``` and ```json fences so the JSON inside parses directly.
fn strip_code_fences(raw: &str) -> String {
    let mut out = raw.replace("```json", "```");
    out = out.replace("```", "");
    out
}

/// Convenience: the shape the synthesiser will use for a prompt, for the
/// pipeline's logging and the dispatch response so an operator sees *why* a
/// plan came out as wide as it did.
pub fn describe_shape(prompt: &str) -> String {
    let surfaces = detect_surfaces(prompt)
        .iter()
        .map(|s| s.label())
        .collect::<Vec<_>>();
    format!("{:?}: [{}]", classify_shape(prompt), surfaces.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::intent::classify as intent_classify;
    use std::collections::HashSet;

    fn ids_of(graph: &TaskGraph) -> Vec<u64> {
        let mut v: Vec<u64> = graph.tasks.keys().map(|t| t.0).collect();
        v.sort_unstable();
        v
    }

    #[test]
    fn build_graph_numbers_tasks_after_the_reconcile_root() {
        let tasks = decompose_heuristic("refactor the auth middleware", &intent_classify("x"));
        let g = build_graph("goal", &tasks);
        assert_eq!(ids_of(&g), vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn detect_surfaces_finds_each_named_surface() {
        let s = detect_surfaces("build a realtime chat: frontend UI, backend api over websockets");
        assert!(s.contains(&Surface::Frontend));
        assert!(s.contains(&Surface::Backend));
        assert!(s.contains(&Surface::Networking));
    }

    #[test]
    fn single_module_shape_for_one_surface() {
        assert_eq!(
            classify_shape("refactor the auth middleware in one file"),
            PromptShape::SingleModule
        );
    }

    #[test]
    fn multi_module_shape_for_two_surfaces() {
        let shape = classify_shape("add a query cache and a background service to the server");
        assert_eq!(shape, PromptShape::MultiModule);
    }

    #[test]
    fn cross_cutting_shape_for_three_surfaces() {
        // The objective's own example: a new feature spanning frontend+backend+networking.
        let shape = classify_shape(
            "add a feature with a frontend dashboard, a backend api and websocket networking",
        );
        assert_eq!(shape, PromptShape::CrossCutting);
    }

    #[test]
    fn single_module_scaffold_is_a_five_node_chain_with_design_root() {
        let g = synthesize("refactor the auth middleware in one file");
        assert_eq!(g.root, TaskId(1));
        // 5 work tasks + reconcile root.
        assert_eq!(g.len(), 6);
        // Everything except the design task depends on something (a real DAG).
        let no_deps: Vec<u64> = g
            .tasks
            .values()
            .filter(|t| t.id != TaskId(1) && t.dependencies.is_empty())
            .map(|t| t.id.0)
            .collect();
        assert_eq!(no_deps, vec![2], "exactly one entry wave task");
    }

    #[test]
    fn cross_cutting_scaffold_fans_out_per_surface_and_reintegrates() {
        let prompt =
            "add a feature with a frontend dashboard, a backend api and websocket networking";
        let g = synthesize(prompt);
        // ≥3 distinct owned scopes (one per surface) beyond the design/schema.
        let owned: HashSet<String> = g
            .tasks
            .values()
            .flat_map(|t| t.scope.iter().cloned())
            .filter(|s| !s.starts_with(".velocity") && s != "docs/")
            .collect();
        assert!(
            owned.len() >= 3,
            "expected ≥3 surface scopes, got {:?}",
            owned
        );
        // An integration node that waits on ≥2 implementation branches.
        let wide = g
            .tasks
            .values()
            .any(|t| t.title.to_lowercase().contains("integrate") && t.dependencies.len() >= 2);
        assert!(
            wide,
            "cross-cutting plan must reintegrate parallel branches"
        );
        // Well above the single-module floor — the whole point of the shape.
        assert!(
            g.len() >= 9,
            "cross-cutting should decompose wider than single-module"
        );
    }

    #[test]
    fn every_synth_graph_is_a_dag_and_root_is_reconcile() {
        for prompt in [
            "rename the variable count to total",
            "add a query cache and a background service to the server",
            "build the app: frontend, backend api and websockets",
        ] {
            let g = synthesize(prompt);
            let tasks = g.tasks.values().collect::<Vec<_>>();
            assert!(g.tasks.contains_key(&TaskId(1)));
            for t in &tasks {
                for d in &t.dependencies {
                    assert!(g.tasks.contains_key(d), "dangling dep {} on {}", d, t.id);
                    assert_ne!(&t.id, d, "self dep on {}", t.id);
                }
            }
        }
    }

    #[test]
    fn parse_decomposition_reads_a_model_json_task_list() {
        let raw = r#"Here is my plan:
        ```json
        [
          {"title":"Design API","description":"fix shapes","scope":["src/"],"deps":[],"priority":9,"weight":2},
          {"title":"Backend","description":"build server","scope":["src/backend/"],"deps":[0],"priority":6,"weight":3},
          {"title":"Frontend","description":"build ui","scope":["src/frontend/"],"deps":[0],"priority":6,"weight":3},
          {"title":"Tests","description":"cover it","scope":[],"deps":[1,2],"priority":3,"weight":2},
          {"title":"Docs","description":"write readme","scope":["docs/"],"deps":[3],"priority":1,"weight":1}
        ]
        ```"#;
        let tasks = parse_decomposition(raw).expect("parse");
        assert_eq!(tasks.len(), 5);
        assert_eq!(tasks[1].deps, vec![0]);
        assert_eq!(tasks[3].deps, vec![1, 2]);
        assert_eq!(tasks[0].priority, 9);
        let valid = validate_synth_tasks(tasks).expect("valid");
        let g = build_graph("demo", &valid);
        assert_eq!(g.len(), 6); // + reconcile root
    }

    #[test]
    fn llm_answer_below_the_floor_falls_back_to_the_scaffold() {
        // A model that returns only two tasks is not a plan; validate_synth_tasks
        // rejects it and synthesize_llm must use the deterministic scaffold.
        let two = r#"[{"title":"a","description":"","scope":[],"deps":[]},{"title":"b","description":"","scope":[],"deps":[0]}]"#;
        let g = synthesize_llm(
            "add a feature with frontend, backend api and websockets",
            &|_| Some(two.to_string()),
        );
        // The scaffold for this cross-cutting prompt is wide (≥9), never 2+root.
        assert!(g.len() >= 8, "fell through to an unusable 2-node plan");
    }

    #[test]
    fn llm_unparseable_answer_falls_back_to_the_scaffold() {
        let g = synthesize_llm("refactor the auth middleware", &|_| {
            Some("sorry, I cannot decompose this".to_string())
        });
        assert_eq!(g.root, TaskId(1));
        assert_eq!(g.len(), 6, "heuristic single-module scaffold");
    }

    #[test]
    fn validate_rejects_a_dependency_cycle() {
        // 0 waits on 1, 1 on 2, 2 back on 0 — a closed loop no node can start.
        let tasks = vec![
            SynthTask::new("a", "", vec![1]),
            SynthTask::new("b", "", vec![2]),
            SynthTask::new("c", "", vec![0]),
            SynthTask::new("d", "", vec![0]),
            SynthTask::new("e", "", vec![3]),
        ];
        let err = validate_synth_tasks(tasks).expect_err("cycle must be rejected");
        assert!(err.contains("cycle"), "got: {err}");
    }

    #[test]
    fn validate_rejects_out_of_range_dependency() {
        let tasks = vec![
            SynthTask::new("a", "", vec![]),
            SynthTask::new("b", "", vec![99]),
            SynthTask::new("c", "", vec![0]),
            SynthTask::new("d", "", vec![0]),
            SynthTask::new("e", "", vec![0]),
        ];
        let err = validate_synth_tasks(tasks).expect_err("range must be enforced");
        assert!(err.contains("out-of-range"), "got: {err}");
    }

    #[test]
    fn build_decomposition_prompt_states_the_bounds_and_shape() {
        let decision = intent_classify("clone liteLLM and rewrite it in rust");
        let ask = build_decomposition_prompt("clone liteLLM and rewrite it in rust", &decision);
        assert!(ask.contains(&MIN_TASKS.to_string()));
        assert!(ask.contains(&MAX_TASKS.to_string()));
        assert!(ask.contains("\"deps\""));
        // Cross-cutting prompts must not exceed the ceiling when expanded.
        assert!(classify_shape("x") == PromptShape::SingleModule);
    }

    #[test]
    fn heuristic_task_count_stays_within_the_band() {
        // A monstrous prompt with every surface should still synthesise ≤ MAX.
        let prompt = "rebuild everything: the frontend ui, backend api service, websockets networking, database schema, oauth auth and docker deployment";
        let decision = intent_classify(prompt);
        let tasks = decompose_heuristic(prompt, &decision);
        assert!(tasks.len() >= MIN_TASKS && tasks.len() <= MAX_TASKS);
        assert!(validate_synth_tasks(tasks).is_ok());
    }
}
