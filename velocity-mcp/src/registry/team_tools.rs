use serde_json::{json, Value};
use std::error::Error;
use std::path::Path;

use crate::agent::{turn_ceiling_for, AiProvider};
use crate::editor::expert_team::{
    detect_scope_overlaps, export_team_to_json, import_team_from_json, load_expert_teams,
    save_expert_teams, slugify, validate_team_composition, ExpertMember, ExpertTeam, MemberUpdate,
    ValidationSeverity,
};
use crate::editor::skill_file::{list_skill_files, save_skill_file, SkillFile};
use crate::editor::team_router::{debug_routing, route_member};
use crate::usage::load_workspace_provider_settings;

/// Collect a JSON string array into owned `String`s, ignoring non-string entries.
fn string_array(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Build a single `ExpertMember` from a JSON member spec.
fn parse_member(
    spec: &Value,
    team_slug: &str,
    index: usize,
) -> Result<ExpertMember, Box<dyn Error>> {
    let name = spec["name"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("each member requires a non-empty 'name'")?;
    let role = spec["role"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("each member requires a non-empty 'role'")?;

    let provider = spec["provider"]
        .as_str()
        .and_then(AiProvider::from_slug)
        .unwrap_or(AiProvider::CloudflareWorkersAi);
    let model_id = spec["model_id"].as_str().unwrap_or("").trim().to_string();

    let id = spec["id"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(slugify)
        .unwrap_or_else(|| format!("member_{}_{}", team_slug, index + 1));

    let fallback = spec["fallback_provider"]
        .as_str()
        .and_then(AiProvider::from_slug);

    Ok(ExpertMember {
        id,
        name: name.to_string(),
        role: role.to_string(),
        provider,
        model_id,
        skills: string_array(&spec["skills"]),
        scope_patterns: string_array(&spec["scope_patterns"]),
        tools: string_array(&spec["tools"]),
        workflow_instructions: spec["workflow_instructions"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_string(),
        fallback_provider: fallback,
    })
}

/// Dispatch the chat-driven team/skill authoring tools. Returns `Ok(None)` when
/// `name` is not one of these tools so the caller can try other handlers.
pub fn handle_team_tool(
    root: &Path,
    name: &str,
    arguments: &Value,
) -> Result<Option<String>, Box<dyn Error>> {
    let result = match name {
        "create_expert_team" => create_expert_team(root, arguments)?,
        "create_skill_file" => create_skill_file(root, arguments)?,
        "list_expert_teams" => list_expert_teams(root)?,
        "list_skills" => list_skills(root)?,
        "update_expert_team" => update_expert_team(root, arguments)?,
        "update_team_member" => update_team_member(root, arguments)?,
        "add_team_member" => add_team_member(root, arguments)?,
        "remove_team_member" => remove_team_member(root, arguments)?,
        "validate_team" => validate_team(root, arguments)?,
        "check_scope_overlaps" => check_scope_overlaps(root, arguments)?,
        "clone_expert_team" => clone_expert_team(root, arguments)?,
        "export_expert_team" => export_expert_team(root, arguments)?,
        "import_expert_team" => import_expert_team(root, arguments)?,
        "debug_routing" => debug_routing_tool(root, arguments)?,
        "team_analytics" => team_analytics(root, arguments)?,
        "team_health_check" => team_health_check(root, arguments)?,
        "list_providers" => list_providers(root, arguments)?,
        "create_team_quick" => create_team_quick(root, arguments)?,
        "bulk_import_members" => bulk_import_members(root, arguments)?,
        "team_changelog" => team_changelog(root, arguments)?,
        "team_dispatch" => team_dispatch(root, arguments)?,
        "classify_intent" => classify_intent_tool(arguments)?,
        "select_model" => select_model_tool(root, arguments)?,
        "provision_team_from_intent" => provision_team_from_intent(root, arguments)?,
        "repo_rewrite" => repo_rewrite(root, arguments)?,
        "run_rewrite_mission" => run_rewrite_mission(root, arguments)?,
        "generate_wiki" => generate_wiki(root, arguments)?,
        _ => return Ok(None),
    };
    Ok(Some(result))
}

/// Expose the intent decision brain: classify a request and return the routing
/// verdict as JSON. Read-only — it neither provisions a team nor clones a repo.
fn classify_intent_tool(arguments: &Value) -> Result<String, Box<dyn Error>> {
    let prompt = arguments["prompt"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'prompt' is required")?;
    let decision = crate::agent::intent::decide_intent(prompt);
    let mut obj = serde_json::to_value(&decision)?;
    obj["backend"] =
        serde_json::Value::String(crate::agent::decision::resolve_backend().name().to_string());
    Ok(serde_json::to_string_pretty(&obj).unwrap_or_else(|_| "{}".to_string()))
}

/// The live model catalog for one provider, fetched with the keys/APIs the
/// user actually enabled in provider-settings (mirrors the executor's
/// `fetch_models_for_provider`). Account-scoped providers (Cloudflare,
/// OpenRouter, Azure, Ollama) are only consulted when explicitly requested
/// via `provider` — scanning for a usable pool never synthesizes their
/// fallback models. Alibaba routes by the active plan (token plan vs
/// DashScope international), exactly like dispatch does.
fn usable_catalog_for(root: &Path, provider: AiProvider) -> Option<Vec<crate::agent::ModelInfo>> {
    use crate::agent::executor::dispatch::{
        alibaba_base_url, resolve_api_key, ALIBABA_DASHSCOPE_INTL_BASE_URL,
    };
    use crate::agent::provider as p;
    let key = |field: &str, env: &str| resolve_api_key(&root.to_path_buf(), field, env);
    let models = match provider {
        AiProvider::OpenAI => p::fetch_openai_models(&key("openai", "OPENAI_API_KEY")),
        AiProvider::Anthropic => p::fetch_anthropic_models(&key("anthropic", "ANTHROPIC_API_KEY")),
        AiProvider::GoogleVertex => p::fetch_google_models(&key("google", "GOOGLE_API_KEY")),
        AiProvider::Deepseek => p::fetch_deepseek_models(&key("deepseek", "DEEPSEEK_API_KEY")),
        AiProvider::AlibabaQwen => {
            let k = key("alibaba", "DASHSCOPE_API_KEY");
            let base = alibaba_base_url(&root.to_path_buf());
            if base == ALIBABA_DASHSCOPE_INTL_BASE_URL {
                p::fetch_alibaba_models(&k)
            } else {
                p::fetch_alibaba_models_at(&base, &k)
            }
        }
        AiProvider::Groq => p::fetch_groq_models(&key("groq", "GROQ_API_KEY")),
        AiProvider::Mistral => p::fetch_mistral_models(&key("mistral", "MISTRAL_API_KEY")),
        AiProvider::TogetherAi => p::fetch_together_models(&key("together", "TOGETHER_API_KEY")),
        AiProvider::FireworksAi => {
            p::fetch_fireworks_models(&key("fireworks", "FIREWORKS_API_KEY"))
        }
        AiProvider::Perplexity => {
            p::fetch_perplexity_models(&key("perplexity", "PERPLEXITY_API_KEY"))
        }
        AiProvider::Cerebras => p::fetch_cerebras_models(&key("cerebras", "CEREBRAS_API_KEY")),
        AiProvider::CloudflareWorkersAi => {
            let accounts = crate::usage::load_accounts(root);
            p::fetch_model_catalog(&accounts)
        }
        AiProvider::OpenRouter => {
            let accounts = crate::usage::load_openrouter_accounts(root);
            if accounts.is_empty() && crate::agent::provider::openrouter_api_key().is_empty() {
                return None; // not enabled: no account and no env key
            }
            let tracker = crate::usage::UsageTracker::new(root);
            p::fetch_openrouter_models(&accounts, &tracker)
        }
        AiProvider::AzureOpenAi => {
            let accounts = crate::usage::load_azure_accounts(root);
            p::fetch_azure_models(&accounts)
        }
        AiProvider::LocalOllama => {
            let accounts = crate::usage::load_local_ollama_accounts(root);
            p::fetch_local_ollama_models(&accounts)
        }
        // Static/benchmark catalog, no key to gate on: only when explicitly
        // requested (it never enters the enablement scan).
        AiProvider::AwsBedrock => Ok(p::fetch_bedrock_models().unwrap_or_default()),
    };
    models.ok().filter(|m| !m.is_empty())
}

/// Providers worth scanning when the caller didn't name one: the key-gated
/// HTTP catalogs, in display order. Account-scoped providers are excluded by
/// design (see [`usable_catalog_for`]).
const SCANNABLE_PROVIDERS: [AiProvider; 11] = [
    AiProvider::OpenAI,
    AiProvider::Anthropic,
    AiProvider::GoogleVertex,
    AiProvider::Deepseek,
    AiProvider::AlibabaQwen,
    AiProvider::Groq,
    AiProvider::Mistral,
    AiProvider::TogetherAi,
    AiProvider::FireworksAi,
    AiProvider::Perplexity,
    AiProvider::Cerebras,
];

/// Expose the model-selection decision matrix: pick the best model for a task
/// strictly from the catalogs of enabled provider APIs. Read-only — it never
/// changes the active model or dispatches anything.
fn select_model_tool(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let task = arguments["task"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'task' is required")?;
    let kind = match arguments["task_kind"].as_str() {
        Some(k) => Some(
            crate::automation::instruction_registry::AgentTaskKind::parse(k)
                .ok_or_else(|| format!("unknown task_kind {k:?}"))?,
        ),
        None => None,
    };
    let provider = match arguments["provider"].as_str() {
        Some(p) if !p.trim().is_empty() => Some(
            serde_json::from_str::<AiProvider>(&json!(p).to_string())
                .map_err(|e| format!("unknown provider: {e}"))?,
        ),
        _ => None,
    };

    // Assemble the usable pool: only models the provider APIs actually serve
    // for the keys the user enabled — never a fabricated or disabled id.
    let mut pool: Vec<(AiProvider, Vec<crate::agent::ModelInfo>)> = Vec::new();
    if let Some(p) = provider {
        if let Some(models) = usable_catalog_for(root, p) {
            pool.push((p, models));
        }
    } else {
        for p in SCANNABLE_PROVIDERS {
            if let Some(models) = usable_catalog_for(root, p) {
                pool.push((p, models));
            }
        }
    }
    if pool.is_empty() {
        return Err(
            "no usable models: no enabled provider API returned a catalog. \
                    Enable a provider key in Settings (e.g. Alibaba Qwen token plan) \
                    or pass an explicit 'provider'."
                .into(),
        );
    }

    // Deterministic ranking per provider; the overall best-scoring provider is
    // the one the decision runs against.
    let mut ranked: Vec<(AiProvider, crate::automation::model_quality::ModelCandidate)> =
        Vec::new();
    for (p, models) in &pool {
        let kind = match kind {
            Some(k) => k,
            None => crate::editor::app::helpers::infer_task_kind_from_goal(task),
        };
        let r = crate::automation::model_quality::ModelQualityIndex::rank_models(kind, *p, models);
        // rank_models sorts best-first, so the head is the provider's top pick.
        if let Some(top) = r.into_iter().next() {
            ranked.push((*p, top));
        }
    }
    ranked.sort_by(|a, b| {
        b.1.score
            .cmp(&a.1.score)
            .then_with(|| a.1.label.cmp(&b.1.label))
    });
    let (best_provider, _) = ranked
        .first()
        .ok_or("no provider returned a ranked candidate")?;
    let kind = kind.unwrap_or_else(|| crate::editor::app::helpers::infer_task_kind_from_goal(task));

    // Ask the decision brain over the union of usable ids (membership-guarded).
    let usable: Vec<crate::agent::ModelInfo> = pool.iter().flat_map(|(_, m)| m.clone()).collect();
    let selection = crate::automation::model_quality::ModelQualityIndex::decide_model_with(
        kind,
        *best_provider,
        &usable,
        task,
        crate::agent::decision::resolve_backend().model(),
    );

    let runners: Vec<Value> = ranked
        .iter()
        .take(5)
        .map(|(p, c)| {
            json!({
                "provider": p.slug(),
                "provider_label": p.label(),
                "model_id": c.model_id,
                "label": c.label,
                "score": c.score,
                "supports_tools": c.supports_tools,
                "supports_thinking": c.supports_thinking,
            })
        })
        .collect();
    Ok(json!({
        "task_kind": kind.as_str(),
        "selected": {
            "provider": selection.candidate.provider.slug(),
            "provider_label": selection.candidate.provider.label(),
            "model_id": selection.candidate.model_id,
            "label": selection.candidate.label,
            "score": selection.candidate.score,
        },
        "backend": selection.backend,
        "confidence": selection.confidence,
        "rationale": selection.rationale,
        "usable_providers": pool.iter().map(|(p, m)| json!({ "provider": p.label(), "model_count": m.len() })).collect::<Vec<_>>(),
        "candidates": runners,
    })
    .to_string())
}

/// Classify a request and, when it verdicts a team, author + persist the
/// implied ExpertTeam. Also selects the best usable model for the task and
/// stamps it onto every member. Idempotent on the slug.
fn provision_team_from_intent(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let prompt = arguments["prompt"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'prompt' is required")?;
    let team_name = arguments["team_name"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Intent Auto-Team")
        .to_string();
    let decision = crate::agent::intent::decide_intent(prompt);
    if decision.scope != crate::agent::intent::TaskScope::Team {
        return Err(format!(
            "intent verdict is single-agent (confidence {:.2}); nothing to provision. Use classify_intent to inspect, or rephrase as a sprawling multi-part task.",
            decision.confidence
        ).into());
    }
    let slug = crate::agent::intent::provision_team(root, &decision, &team_name)?;

    // Select the best model for this task and stamp it onto team members.
    let model_selection = stamp_selected_model(root, &slug, prompt);

    let report = serde_json::json!({
        "team_slug": slug,
        "team_name": team_name,
        "decision": decision,
        "model_selection": model_selection.map(|(p, id)| json!({"provider": p.slug(), "model_id": id})),
    });
    Ok(serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string()))
}

/// Select the best usable model for `task` (the same decision brain behind the
/// `select_model` tool: live catalog + Laya/heuristic ranking) and stamp it onto
/// every member of the team `slug`. Returns the pick when one was made.
/// Without this, dispatched members fall through to per-provider static
/// defaults, which the LiteLLM crucible proved dangerous: the legacy
/// `qwen-max/plus/turbo` names 404 against current token-plan endpoints, and
/// the whole chain burned itself retrying deterministic failures.
fn stamp_selected_model(root: &Path, slug: &str, task: &str) -> Option<(AiProvider, String)> {
    let model_selection = select_model_tool(root, &json!({"task": task}))
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|v| {
            let id = v["selected"]["model_id"].as_str()?.to_string();
            let provider_slug = v["selected"]["provider"].as_str()?;
            let provider = AiProvider::from_slug(provider_slug)?;
            Some((provider, id))
        })?;
    let mut teams = crate::editor::expert_team::load_expert_teams(root);
    if let Some(team) = teams.iter_mut().find(|t| t.slug() == slug) {
        for m in &mut team.members {
            m.provider = model_selection.0;
            m.model_id = model_selection.1.clone();
        }
        crate::editor::expert_team::save_expert_teams(root, &teams);
    }
    Some(model_selection)
}

/// Map a file extension to the language name used across the agent tooling.
///
/// `pub(crate)` because [`crate::automation::sectors`] decomposes a repo by the
/// same language set; two tables that disagree would put a file in a sector the
/// dependency scanner cannot read.
pub(crate) fn lang_for_ext(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "py" => "python",
        "rs" => "rust",
        "js" | "mjs" | "cjs" => "javascript",
        "ts" | "tsx" | "jsx" => "typescript",
        "go" => "go",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "swift" => "swift",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" => "cpp",
        "cs" => "csharp",
        "rb" => "ruby",
        "php" => "php",
        "zig" => "zig",
        "ex" | "exs" => "elixir",
        "hs" => "haskell",
        "scala" => "scala",
        "dart" => "dart",
        "sql" => "sql",
        "html" | "htm" | "css" => "web",
        _ => return None,
    })
}

/// Deterministically inventory a cloned repository: dominant source languages,
/// a bounded file count, and top-level layout. No LLM, no network — this is the
/// "analyze" leg of [`repo_rewrite`] and stays unit-testable.
pub(crate) fn analyze_repo(root: &Path) -> Value {
    let mut exts: std::collections::HashMap<&'static str, usize> = std::collections::HashMap::new();
    let mut file_count = 0usize;
    let mut capped = false;
    let mut stack: Vec<(std::path::PathBuf, usize)> = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > 8 {
            continue;
        }
        let rd = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for entry in rd.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if matches!(
                    name.as_str(),
                    ".git"
                        | "node_modules"
                        | "target"
                        | "__pycache__"
                        | ".venv"
                        | "venv"
                        | "dist"
                        | "build"
                ) {
                    continue;
                }
                stack.push((path, depth + 1));
            } else {
                file_count += 1;
                if file_count > 20_000 {
                    capped = true;
                    break;
                }
                if let Some(lang) = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase())
                    .and_then(|e| lang_for_ext(&e))
                {
                    *exts.entry(lang).or_default() += 1;
                }
            }
        }
        if capped {
            break;
        }
    }
    let mut langs: Vec<(&str, usize)> = exts.iter().map(|(k, v)| (*k, *v)).collect();
    langs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let top_level: Vec<String> = std::fs::read_dir(root)
        .map(|rd| {
            let mut v: Vec<String> = rd
                .flatten()
                .filter_map(|e| {
                    let n = e.file_name().to_string_lossy().to_string();
                    if n == ".git" {
                        return None;
                    }
                    if e.path().is_dir() {
                        Some(format!("{n}/"))
                    } else {
                        Some(n)
                    }
                })
                .collect();
            v.sort();
            v.truncate(60);
            v
        })
        .unwrap_or_default();
    json!({
        "file_count": file_count,
        "languages": langs
            .iter()
            .take(8)
            .map(|(l, c)| json!({ "language": l, "files": c }))
            .collect::<Vec<_>>(),
        "top_level": top_level,
    })
}

/// Build a deterministic, phased rewrite plan from the repo profile + team
/// roster. A scaffold the lead agent refines — no model call required.
///
/// When a [`SectorPlan`] is supplied the porting phase is grounded in real
/// arithmetic — sector count, wave count, dispatch count, model-hours — instead
/// of the sentence "port remaining modules in dependency order", which on a
/// codebase the size of LiteLLM is not a task but the entire job.
///
/// The security, durability and performance sweeps are emitted whether or not a
/// sector plan exists. They are not optional polish: a mission that runs for
/// hours produces more code than one review pass can read, and those three are
/// exactly the axes that fail silently, so a suite written by the same pass that
/// wrote the code will not catch them.
pub(crate) fn build_rewrite_plan(
    profile: &Value,
    target_language: &str,
    roles: &[String],
    sectors: Option<&crate::automation::sectors::SectorPlan>,
) -> Vec<Value> {
    let find = |pred: &dyn Fn(&&str) -> bool, fallback: &str| -> String {
        roles
            .iter()
            .find(|r| pred(&r.as_str()))
            .cloned()
            .unwrap_or_else(|| fallback.to_string())
    };
    // Prefer an exact title, then fall back to a substring.
    //
    // The substring fallback matters for the security seat: the `auth` domain
    // puts a "Security Engineer" on the roster, and a plan that named a role
    // nobody was hired for could never be dispatched.
    let seat = |exact: &str, fallback: &str| -> String {
        roles
            .iter()
            .find(|r| r.as_str() == exact)
            .or_else(|| roles.iter().find(|r| r.contains(fallback)))
            .cloned()
            .unwrap_or_else(|| exact.to_string())
    };
    let lead = roles
        .first()
        .cloned()
        .unwrap_or_else(|| "Lead Architect".to_string());
    let porter = find(
        &|r| r.contains("Port") || r.to_lowercase().contains(target_language),
        &format!("{target_language} Port Engineer"),
    );
    let qa = find(
        &|r| r.contains("Test") || r.contains("QA"),
        "Test & QA Engineer",
    );
    let reviewer = find(&|r| r.contains("Review"), "Reviewer");
    let security = seat("Security Sweep Auditor", "Security");
    let durability = seat("Durability Engineer", "Durability");
    let performance = seat("Performance Engineer", "Performance");
    let docs = seat("Documentation Engineer", "Documentation");
    let primary = profile["languages"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|f| f["language"].as_str())
        .unwrap_or("source")
        .to_string();
    let file_count = profile["file_count"].as_u64().unwrap_or(0);
    let phase = |idx: u32,
                 title: String,
                 owner: String,
                 goals: Vec<String>,
                 deps: Vec<u32>|
     -> Value {
        json!({ "phase": idx, "title": title, "owner_role": owner, "goals": goals, "depends_on": deps })
    };

    // Porting goals: measured when a decomposition exists, aspirational when not.
    let port_goals: Vec<String> = match sectors {
        Some(p) => {
            let t = &p.totals;
            let mut goals = vec![
                format!(
                    "Port {} sectors across {} dependency waves ({} dispatch units, {} source files, {} lines)",
                    t.sectors, t.waves, t.dispatches, t.files, t.lines
                ),
                format!(
                    "Treat wave order as a suggestion, not a guarantee: {} dependency edges had to be broken to make the graph acyclic, so sectors in one wave may need stubs for each other",
                    t.cycles_broken
                ),
                format!(
                    "{} files are too large for one context window and are pre-chunked into {} line-range units; never hand a dispatch more than one unit",
                    t.files_chunked, t.chunks
                ),
                format!(
                    "Budget {:.1} model-hours ({:.1} wall-hours at {}x parallelism) before counting retries, rework or the sweep phases",
                    t.est_model_secs as f64 / 3600.0,
                    t.est_wall_hours_at_parallelism,
                    t.parallelism
                ),
            ];
            // The scope travels with the budget, for the same reason
            // `SectorTotals::feasibility` prefixes rather than appends it: a
            // plan that says "budget 320.7 model-hours" without saying what
            // those hours cover invites an operator to authorise spend against a
            // number describing 68% of a repository. Both conditions are
            // checked separately because a plan can be deliberately narrowed
            // *and* have had a detected root declined, and either fact alone
            // misrepresents the scope. Neither is emitted when the plan covers
            // everything it found, so a whole-repo decomposition gets no noise.
            if t.root_coverage() < 1.0 {
                goals.push(format!(
                    "SCOPE: this decomposition covers {:.0}% of the source found - {} files ({} KB) sit outside the package root '{}' and will not be ported{}",
                    t.root_coverage() * 100.0,
                    t.outside_root_files,
                    t.outside_root_bytes / 1024,
                    p.source_root,
                    if t.outside_root_examples.is_empty() {
                        String::new()
                    } else {
                        format!("; not scheduled: {}", t.outside_root_examples.join(", "))
                    }
                ));
            }
            if !t.root_declined.is_empty() {
                goals.push(format!("SCOPE: {}", t.root_declined));
            }
            goals
        }
        None => vec![
            "Port remaining modules in dependency order, parallelizing independent domains across specialists".into(),
        ],
    };

    vec![
        phase(1, "Ingest & map the source".into(), lead.clone(), vec![
            format!("Inventory modules, entrypoints, public API surface and config of the {primary} codebase ({file_count} files)"),
            "Produce a component map the team can port against".into(),
            "Flag anything that cannot port 1:1 (FFI, platform-specific code, native deps)".into(),
        ], vec![]),
        phase(2, format!("Design the {target_language} architecture"), lead, vec![
            format!("Target crate/module layout, error-handling and async model for {target_language}"),
            "Define the public-API parity contract".into(),
            "Fix the workspace-wide conventions every sector must follow, so 1,000 independently-ported sectors still read as one codebase".into(),
        ], vec![1]),
        phase(3, "Port core types & interfaces".into(), porter.clone(), vec![
            "Translate foundational data structures, traits and interfaces".into(),
            "Establish the build skeleton and dependency manifest".into(),
        ], vec![2]),
        phase(4, "Port sectors in dependency waves".into(), porter.clone(), port_goals, vec![3]),
        phase(5, "Port entrypoints & CLI/API surface".into(), porter, vec![
            "Reimplement CLI, HTTP API and configuration handling to match source behavior".into(),
        ], vec![4]),
        phase(6, "Re-establish test parity".into(), qa.clone(), vec![
            format!("Port/rewrite the test suite to {target_language}"),
            "Stand up a parity harness comparing source vs rewritten behavior".into(),
            "Make the gate set green — build, fmt, lint, test — before any sweep starts, so a sweep fix that breaks something is caught by a machine rather than by a reader".into(),
        ], vec![3, 4, 5]),
        // The three sweeps run after test parity and in parallel with each
        // other: they read the same artifact and ask different questions, so
        // there is nothing to serialise.
        phase(7, "Security sweep".into(), security, vec![
            "Audit every ported sector for injection, unvalidated input crossing a trust boundary, and secrets in logs or error text".into(),
            "Check the dependency manifest for known-vulnerable and unmaintained crates".into(),
            "Verify no ported code reintroduced an unsafe block, a raw pointer cast or a disabled check that the source language made impossible".into(),
            "File one finding per issue with file:line and a concrete fix; do not restate the code".into(),
        ], vec![6]),
        phase(8, "Durability sweep".into(), durability, vec![
            "Audit error handling: no swallowed errors, no panic-on-input path reachable from outside, every `unwrap`/`expect` justified or removed".into(),
            "Check resource lifetime — files, sockets, locks, spawned tasks — for leaks on the error path as well as the happy path".into(),
            "Verify partial-failure behavior: what state is left behind when a step fails halfway, and can the operation be retried or resumed".into(),
            "Confirm integer overflow, empty-collection and boundary cases are handled rather than assumed away".into(),
        ], vec![6]),
        phase(9, "Performance sweep & benchmark harness".into(), performance, vec![
            "Stand up a reproducible benchmark harness for the hot paths, with numbers recorded before and after each change".into(),
            "Audit for accidental quadratic behavior introduced by the port — repeated linear scans, cloning where a borrow would do, allocation in a loop".into(),
            "Benchmark against the source implementation and report the ratio honestly, including where the port is slower".into(),
        ], vec![6]),
        phase(10, "Remediate sweep findings and re-run the gates".into(), qa.clone(), vec![
            "Fix every finding from the security, durability and performance sweeps, or record why it was rejected".into(),
            "Re-run build, fmt, lint, test and the benchmark harness; a fix that regresses a gate is not a fix".into(),
        ], vec![7, 8, 9]),
        phase(11, "Behavioral parity review".into(), reviewer, vec![
            "Review for behavioral parity and idiomatic target language, not for a line-by-line transliteration of the source".into(),
            "Confirm the public-API parity contract from phase 2 actually holds".into(),
        ], vec![10]),
        phase(12, "Documentation & benchmark report".into(), docs, vec![
            "Document the architecture, the ported public API, and every place the port deliberately diverges from the source".into(),
            "Publish the benchmark results and the known-limitations list, including the sectors that could not be ported 1:1".into(),
        ], vec![11]),
    ]
}

/// End-to-end orchestration for "clone X and rewrite it in Y": clone the repo
/// into the workspace, analyze it deterministically, provision the team the
/// intent brain implies, and emit a phased rewrite plan. Optionally hands the
/// first phase to the team lead for a real planning pass (`dispatch: true`).
///
/// Deliberately does NOT perform the rewrite itself — that stays with the routed
/// agents / Mission Control, so the user keeps control over model spend.
fn repo_rewrite(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let url = arguments["url"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'url' is required")?;
    let target_language = arguments["target_language"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'target_language' is required")?;
    let team_name = arguments["team_name"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Repo Rewrite Team")
        .to_string();
    let dispatch = arguments["dispatch"].as_bool().unwrap_or(false);

    // 1. Clone safely into the workspace (git is invoked directly by git_clone).
    let mut clone_args = json!({ "url": url });
    if let Some(d) = arguments["depth"].as_u64() {
        clone_args["depth"] = json!(d);
    }
    let clone_raw =
        crate::registry::system_tools::git_clone_impl(root, &clone_args).or_else(|e| {
            // Crucible lesson: a re-run of repo_rewrite into the same workspace
            // must not die on the clone from the first run. If the default
            // destination already holds a git work tree, adopt it instead of
            // failing on "already exists and is not empty".
            if !e.to_string().contains("already exists and is not empty") {
                return Err(e);
            }
            let name = url
                .trim_end_matches('/')
                .trim_end_matches(".git")
                .rsplit(['/', ':'])
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or("repo")
                .to_string();
            let rel = format!(".velocity/clones/{name}");
            if !root.join(&rel).join(".git").exists() {
                return Err(e);
            }
            serde_json::to_string_pretty(&json!({
                "success": true,
                "path": rel,
                "repo_name": name,
                "reused_existing_clone": true,
            }))
            .map_err(|je| Box::new(je) as Box<dyn std::error::Error>)
        })?;
    let clone_report: Value = serde_json::from_str(&clone_raw)
        .map_err(|_| format!("unexpected git_clone output: {clone_raw}"))?;
    if clone_report["success"] != json!(true) {
        return Err(format!(
            "git clone failed: {}",
            clone_report["output"].as_str().unwrap_or("")
        )
        .into());
    }
    let rel_path = clone_report["path"]
        .as_str()
        .ok_or("git_clone returned no path")?
        .to_string();
    let repo_name = clone_report["repo_name"]
        .as_str()
        .unwrap_or("repo")
        .to_string();
    let clone_dir = root.join(&rel_path);

    // 2. Deterministic analysis of what we just pulled.
    let profile = analyze_repo(&clone_dir);

    // 2b. Decompose the tree into dispatchable sectors.
    //
    // This is the step that turns a repo-scale rewrite from one impossible
    // prompt into a schedulable job. Pure filesystem work — no model, no
    // network — so it is reproducible, and a resumed mission recognises the
    // sectors it already ported instead of re-doing them under new ids.
    let parallelism = arguments["parallelism"]
        .as_u64()
        .map(|v| v.clamp(1, 32) as usize)
        .unwrap_or(crate::automation::sectors::DEFAULT_PARALLELISM);
    let sector_opts = crate::automation::sectors::SectorOptions {
        include_tests: arguments["include_tests"].as_bool().unwrap_or(false),
        // Deliberate scoping. Auto-detection refuses to adopt a package root
        // that would leave more than a quarter of the collected source outside
        // it, because a monorepo's other directories may be separate
        // sub-projects or may be part of the same library and the byte counts
        // cannot tell which. An operator who knows can say so here.
        source_root: arguments["source_root"]
            .as_str()
            .map(str::trim)
            .unwrap_or("")
            .to_string(),
        ..Default::default()
    };
    let sector_plan = crate::automation::sectors::plan_sectors_with_parallelism(
        &clone_dir,
        &sector_opts,
        parallelism,
    );
    // Persisted, not just returned: a mission that runs for hours will be
    // interrupted, and re-decomposing on resume is only safe if the resumed run
    // reads the *same* plan the sectors were started from.
    let mission = mission_dir(root, &repo_name, target_language);
    std::fs::create_dir_all(&mission)?;
    let sector_plan_rel = mission_rel(&repo_name, target_language).join("sector_plan.json");
    std::fs::write(
        mission.join("sector_plan.json"),
        serde_json::to_string_pretty(&sector_plan.to_json())?,
    )?;
    let st = &sector_plan.totals;

    // 3. Provision the team the intent brain implies for this port/rewrite.
    //    Same brain as classify_intent/provision_team_from_intent — routed
    //    through decide_intent so the model's sharp bounded answers (target
    //    language, repo ingest) are adopted here too, with the deterministic
    //    pass as always-available fallback.
    //
    //    The synthesized prompt has to describe the operation accurately. "port
    //    X and rewrite the entire codebase in Y" scores 2 on the horizon axis —
    //    one short of long — which would staff no sweep specialists and leave
    //    plan phases 7-12 owned by roles nobody was hired for. Naming the clone
    //    makes repo ingest fire, which is what this operation actually does.
    //    It deliberately omits any "end to end" phrasing: `repo_rewrite` plans a
    //    long-horizon job but must not authorise running it unattended, because
    //    that authorization belongs to the user and to `run_rewrite_mission`.
    let synth =
        format!("clone {url} and do a full rewrite of the entire codebase in {target_language}");
    let decision = crate::agent::intent::decide_intent(&synth);
    let role_names: Vec<String> = decision.roles.iter().map(|r| r.role.clone()).collect();
    let team_slug =
        if decision.scope == crate::agent::intent::TaskScope::Team && !decision.roles.is_empty() {
            crate::agent::intent::provision_team(root, &decision, &team_name)?
        } else {
            String::new()
        };
    // Stamp the brain's model pick onto the roster, so dispatched members
    // start on a model that exists in the live catalog instead of the static
    // per-provider defaults (which 404 on token-plan endpoints).
    let model_selection = if team_slug.is_empty() {
        None
    } else {
        stamp_selected_model(root, &team_slug, &synth)
    };

    // 3b. Persist the mission's identity, which is what makes it resumable.
    //
    //     `sector_plan.json` says *what* the work is; `mission.json` says where
    //     the source and output trees are and which team owns them. Keeping the
    //     two apart means a re-run under a tighter budget cannot re-target a
    //     different language, and a resumed run cannot guess `package_root`
    //     differently and scatter its output across two trees.
    //
    //     The output root sits under the mission directory rather than beside
    //     the clone. That is not tidiness: the clone is the reference the parity
    //     review diffs against, so a port able to write into it would destroy the
    //     only means of checking itself.
    let output_root_rel = mission_rel(&repo_name, target_language).join("output");
    let prior = crate::automation::mission_runner::MissionManifest::load(&mission).ok();
    let manifest = crate::automation::mission_runner::MissionManifest {
        repo_name: repo_name.clone(),
        source_url: url.to_string(),
        target_language: target_language.to_string(),
        source_root: rel_path.clone(),
        package_root: sector_plan.source_root.clone(),
        output_root: output_root_rel.to_string_lossy().replace('\\', "/"),
        // A re-run must not silently re-point the mission at no team at all:
        // keep the recorded one when this run provisioned none.
        team_id: if team_slug.is_empty() {
            prior
                .as_ref()
                .map(|m| m.team_id.clone())
                .unwrap_or_default()
        } else {
            team_slug.clone()
        },
        parallelism,
        include_tests: sector_opts.include_tests,
        created_at_ms: prior
            .as_ref()
            .map(|m| m.created_at_ms)
            .unwrap_or_else(crate::automation::build_runner::now_ms),
    };
    manifest.save(&mission)?;

    // 4. Phased plan derived from the roster + profile + measured decomposition.
    let plan = build_rewrite_plan(&profile, target_language, &role_names, Some(&sector_plan));
    // Written next to the manifest so the mission directory describes itself:
    // an operator picking up an interrupted run should not have to re-derive the
    // phase order from the tool's stdout.
    std::fs::write(
        mission.join("plan.json"),
        serde_json::to_string_pretty(&plan)?,
    )?;

    // 5. Optional: give the lead a real first-pass planning task.
    let dispatch_result = if dispatch && !team_slug.is_empty() {
        let task = format!(
            "Phase 1 (Ingest & map) of a {target_language} rewrite of the repository cloned at '{rel_path}'. Produce the component map and porting notes; do not modify the clone."
        );
        let args = json!({ "team_id": team_slug, "task": task, "files": [rel_path.clone()] });
        match handle_team_tool(root, "team_dispatch", &args) {
            Ok(Some(text)) => Some(text.chars().take(8000).collect::<String>()),
            Ok(None) => None,
            Err(e) => Some(format!("dispatch failed: {e}")),
        }
    } else {
        None
    };

    Ok(serde_json::to_string_pretty(&json!({
        "success": true,
        "repo": { "url": url, "name": repo_name, "path": rel_path },
        "target_language": target_language,
        "analysis": profile,
        "team": {
            "slug": team_slug,
            "name": team_name,
            "roles": role_names,
            "rationale": decision.rationale,
            "horizon": decision.horizon.as_str(),
            "wants_unattended": decision.wants_unattended,
        },
        "model_selection": model_selection.map(|(p, id)| json!({"provider": p.slug(), "model_id": id})),
        "sectors": {
            "plan_path": sector_plan_rel.to_string_lossy(),
            "source_root": sector_plan.source_root,
            "totals": json!({
                "files": st.files,
                "bytes": st.bytes,
                "lines": st.lines,
                "sectors": st.sectors,
                "dispatches": st.dispatches,
                "waves": st.waves,
                "largest_wave": st.largest_wave,
                "sectors_over_budget": st.sectors_over_budget,
                "files_chunked": st.files_chunked,
                "chunks": st.chunks,
                "cycles_broken": st.cycles_broken,
                "units_over_ceiling": st.units_over_ceiling,
                "generated_skipped": st.generated_skipped,
                "generated_bytes": st.generated_bytes,
                "generated_examples": st.generated_examples,
                // Scope accounting. Without these the totals above cannot be
                // told apart from a plan of the same shape that covered the
                // whole repository, which is the distinction an operator
                // authorising 569 model-hours of spend needs.
                "root_coverage": st.root_coverage(),
                "outside_root_files": st.outside_root_files,
                "outside_root_bytes": st.outside_root_bytes,
                "outside_root_examples": st.outside_root_examples,
                "root_declined": st.root_declined,
                "est_model_secs": st.est_model_secs,
                "model_hours": st.est_model_secs as f64 / 3600.0,
                "est_wall_hours_at_parallelism": st.est_wall_hours_at_parallelism,
                "parallelism": st.parallelism,
            }),
            "feasibility": st.feasibility(),
        },
        "plan": plan,
        "mission": {
            "dir": mission_rel(&repo_name, target_language).to_string_lossy().replace('\\', "/"),
            "manifest": "mission.json",
            "output_root": manifest.output_root,
            "team_id": manifest.team_id,
            "wants_unattended": decision.wants_unattended,
        },
        "dispatched": dispatch_result.is_some(),
        "dispatch_result": dispatch_result,
        // Named explicitly so the caller is not left guessing why a tool called
        // "repo_rewrite" produced no rewritten code.
        "executed": false,
        "next_step": format!(
            "run_rewrite_mission with mission_dir '{}' to execute the plan; it is resumable and takes a spend cap",
            mission_rel(&repo_name, target_language).to_string_lossy()
        ),
    }))?)
}

/// Where a rewrite mission keeps its state, relative to the workspace root.
///
/// Under `.velocity/missions/<repo>-<language>/` rather than inside the clone:
/// the clone is disposable (a re-run may re-pull it) while the mission's
/// progress record is the only thing that makes resume possible.
fn mission_rel(repo_name: &str, target_language: &str) -> std::path::PathBuf {
    let slug = slugify(&format!("{repo_name}-{target_language}"));
    let slug = if slug.is_empty() {
        "rewrite".to_string()
    } else {
        slug
    };
    std::path::Path::new(".velocity/missions").join(slug)
}

/// [`mission_rel`] resolved against the workspace root.
fn mission_dir(root: &Path, repo_name: &str, target_language: &str) -> std::path::PathBuf {
    root.join(mission_rel(repo_name, target_language))
}

/// Execute (or resume) the rewrite mission that [`repo_rewrite`] planned.
///
/// Dispatches the porting waves in parallel, runs the gates against the output
/// tree, then the security / durability / performance sweeps, remediation,
/// parity review and documentation. Every step is checkpointed, so a cap, a
/// crash or a reboot resumes rather than restarts.
///
/// This is the only tool here that will spend on models without a human reading
/// each result first, so it is the only one that asks for the authorization to be
/// spelled out: `unattended: true`, or `dry_run: true` to see the schedule, the
/// seats and the caps without spending anything.
///
/// That gate is a speed bump and an audit trail, not a security boundary — the
/// caller is itself an agent and can set the flag. What it buys is that the spend
/// decision has to be explicit and appears in the transcript, where it can be
/// traced back to a user request that actually asked for it.
/// [`crate::agent::intent::IntentDecision::wants_unattended`] is what decides
/// whether an agent *should* set it, and no model is permitted to set that.
fn run_rewrite_mission(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let mission_dir = arguments["mission_dir"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'mission_dir' is required; repo_rewrite reports it under mission.dir")?;
    let cfg = crate::automation::mission_runner::MissionConfig::from_args(arguments);
    let unattended = arguments["unattended"].as_bool().unwrap_or(false);
    if !unattended && !cfg.dry_run {
        return Err(
            "refusing to dispatch: pass unattended=true to authorise an unattended run, or \
             dry_run=true to see the schedule, the seats and the caps without spending anything. \
             A mission of this size is thousands of model calls, and authorising that is the \
             user's decision rather than the agent's"
                .into(),
        );
    }
    let report = crate::automation::mission_runner::run_mission(root, mission_dir, &cfg)?;
    Ok(serde_json::to_string_pretty(&report.to_json())?)
}

/// Create (or replace by id/slug) an expert team and persist it as NDA.
fn create_expert_team(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let team_name = arguments["name"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'name' is required")?;
    let description = arguments["description"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();

    let member_specs = arguments["members"]
        .as_array()
        .filter(|arr| !arr.is_empty())
        .ok_or("'members' must be a non-empty array")?;

    let slug = slugify(team_name);
    if slug.is_empty() {
        return Err("'name' must contain alphanumeric characters".into());
    }
    let team_id = format!("team_{}", slug);

    let mut members = Vec::with_capacity(member_specs.len());
    for (idx, spec) in member_specs.iter().enumerate() {
        members.push(parse_member(spec, &slug, idx)?);
    }

    let member_count = members.len();
    let new_team = ExpertTeam {
        id: team_id.clone(),
        name: team_name.to_string(),
        description,
        members,
        is_preset: false,
    };

    // Merge with the teams already on disk: replace when the id or slug matches,
    // otherwise append.
    let mut teams = load_expert_teams(root);
    let replaced = if let Some(existing) = teams
        .iter_mut()
        .find(|t| t.id == team_id || t.slug() == slug)
    {
        *existing = new_team;
        true
    } else {
        teams.push(new_team);
        false
    };

    if !save_expert_teams(root, &teams) {
        return Err("failed to persist expert_teams.nda".into());
    }

    Ok(format!(
        "{} team \"{}\" (id: {}, slug: {}) with {} member(s). Address it with @{} or \"send it to the {} team\".",
        if replaced { "Updated" } else { "Created" },
        team_name,
        team_id,
        slug,
        member_count,
        slug,
        team_name
    ))
}

/// Create (or overwrite) a reusable skill `.nda` file injected into member prompts.
fn create_skill_file(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let raw_id = arguments["id"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'id' is required")?;
    let id = slugify(raw_id);
    if id.is_empty() {
        return Err("'id' must contain alphanumeric characters".into());
    }
    let body = arguments["body"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'body' is required")?;
    let name = arguments["name"].as_str().unwrap_or(&id).trim().to_string();
    let description = arguments["description"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();

    let skill = SkillFile::new(&id, &name, &description, body);
    if !save_skill_file(root, &skill) {
        return Err("failed to persist skill .nda".into());
    }

    Ok(format!(
        "Saved skill \"{}\" (id: {}) to .velocity/skills/{}.nda. Attach it by listing \"{}\" in a member's skills.",
        name, id, id, id
    ))
}

/// Summarize the teams currently persisted for the workspace.
fn list_expert_teams(root: &Path) -> Result<String, Box<dyn Error>> {
    let teams = load_expert_teams(root);
    let summary: Vec<Value> = teams
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "name": t.name,
                "slug": t.slug(),
                "is_preset": t.is_preset,
                "description": t.description,
                "members": t
                    .members
                    .iter()
                    .map(|m| json!({
                        "id": m.id,
                        "name": m.name,
                        "role": m.role,
                        "provider": m.provider.slug(),
                        "model_id": m.model_id,
                        "skills": m.skills,
                        "scope_patterns": m.scope_patterns,
                    }))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(serde_json::to_string_pretty(&summary)?)
}

/// List the reusable skill files persisted under `.velocity/skills`.
fn list_skills(root: &Path) -> Result<String, Box<dyn Error>> {
    let summary: Vec<Value> = list_skill_files(root)
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "name": s.name,
                "description": s.description,
            })
        })
        .collect();
    Ok(serde_json::to_string_pretty(&summary)?)
}

// ═══════════════════════════════════════════════════════════════════════════
// Edit / Update Tools
// ═══════════════════════════════════════════════════════════════════════════

/// Find a mutable reference to a team by id or slug.
fn find_team_mut<'a>(teams: &'a mut [ExpertTeam], team_ref: &str) -> Option<&'a mut ExpertTeam> {
    let lower = team_ref.to_lowercase();
    let slug = slugify(team_ref);
    teams.iter_mut().find(|t| {
        t.id.to_lowercase() == lower || t.slug() == slug || t.name.to_lowercase() == lower
    })
}

/// Update a team's name and/or description.
fn update_expert_team(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let team_ref = arguments["team_id"]
        .as_str()
        .or_else(|| arguments["team"].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'team_id' is required")?;

    let mut teams = load_expert_teams(root);
    let team = find_team_mut(&mut teams, team_ref)
        .ok_or_else(|| format!("team '{}' not found", team_ref))?;

    if team.is_preset {
        return Err("cannot edit preset teams; clone the team first".into());
    }

    let mut changes = Vec::new();

    if let Some(new_name) = arguments["name"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if let Some((old_slug, new_slug)) = team.update_name(new_name) {
            changes.push(format!(
                "name → \"{}\" (slug: {} → {})",
                new_name, old_slug, new_slug
            ));
        }
    }

    if let Some(desc) = arguments["description"].as_str() {
        if team.update_description(desc) {
            changes.push("description updated".to_string());
        }
    }

    if changes.is_empty() {
        return Ok(format!("No changes applied to team '{}'", team.name));
    }

    let team_name = team.name.clone();
    if !save_expert_teams(root, &teams) {
        return Err("failed to persist expert_teams.nda".into());
    }

    Ok(format!(
        "Updated team \"{}\": {}",
        team_name,
        changes.join(", ")
    ))
}

/// Apply a partial update to a team member.
fn update_team_member(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let team_ref = arguments["team_id"]
        .as_str()
        .or_else(|| arguments["team"].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'team_id' is required")?;

    let member_id = arguments["member_id"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'member_id' is required")?;

    let update = MemberUpdate {
        name: arguments["name"].as_str().map(|s| s.trim().to_string()),
        role: arguments["role"].as_str().map(|s| s.trim().to_string()),
        provider: arguments["provider"].as_str().map(|s| s.trim().to_string()),
        model_id: arguments["model_id"].as_str().map(|s| s.trim().to_string()),
        skills: arguments["skills"]
            .as_array()
            .map(|_| string_array(&arguments["skills"])),
        scope_patterns: arguments["scope_patterns"]
            .as_array()
            .map(|_| string_array(&arguments["scope_patterns"])),
        tools: arguments["tools"]
            .as_array()
            .map(|_| string_array(&arguments["tools"])),
        workflow_instructions: arguments["workflow_instructions"]
            .as_str()
            .map(|s| s.to_string()),
    };

    let mut teams = load_expert_teams(root);
    let team = find_team_mut(&mut teams, team_ref)
        .ok_or_else(|| format!("team '{}' not found", team_ref))?;

    if team.is_preset {
        return Err("cannot edit preset team members; clone the team first".into());
    }

    let changed = team.update_member(member_id, &update)?;
    if changed.is_empty() {
        return Ok(format!("No changes applied to member '{}'", member_id));
    }

    let team_name = team.name.clone();
    if !save_expert_teams(root, &teams) {
        return Err("failed to persist expert_teams.nda".into());
    }

    Ok(format!(
        "Updated member '{}' in team \"{}\": {}",
        member_id,
        team_name,
        changed.join(", ")
    ))
}

/// Add a new member to an existing team.
fn add_team_member(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let team_ref = arguments["team_id"]
        .as_str()
        .or_else(|| arguments["team"].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'team_id' is required")?;

    let member_spec = &arguments["member"];
    if member_spec.is_null() {
        return Err("'member' object is required".into());
    }

    let mut teams = load_expert_teams(root);
    let team = find_team_mut(&mut teams, team_ref)
        .ok_or_else(|| format!("team '{}' not found", team_ref))?;

    if team.is_preset {
        return Err("cannot add members to preset teams; clone the team first".into());
    }

    let slug = team.slug();
    let member = parse_member(member_spec, &slug, team.members.len())?;
    team.add_member(member.clone())?;

    let team_name = team.name.clone();
    let member_count = team.members.len();
    if !save_expert_teams(root, &teams) {
        return Err("failed to persist expert_teams.nda".into());
    }

    Ok(format!(
        "Added member \"{}\" (id: {}) to team \"{}\". Team now has {} member(s).",
        member.name, member.id, team_name, member_count
    ))
}

/// Remove a member from a team by member id.
fn remove_team_member(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let team_ref = arguments["team_id"]
        .as_str()
        .or_else(|| arguments["team"].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'team_id' is required")?;

    let member_id = arguments["member_id"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'member_id' is required")?;

    let mut teams = load_expert_teams(root);
    let team = find_team_mut(&mut teams, team_ref)
        .ok_or_else(|| format!("team '{}' not found", team_ref))?;

    if team.is_preset {
        return Err("cannot remove members from preset teams".into());
    }

    let removed = team
        .remove_member(member_id)
        .ok_or_else(|| format!("member '{}' not found in team '{}'", member_id, team.name))?;

    let team_name = team.name.clone();
    let member_count = team.members.len();
    if !save_expert_teams(root, &teams) {
        return Err("failed to persist expert_teams.nda".into());
    }

    Ok(format!(
        "Removed member \"{}\" (id: {}) from team \"{}\". Team now has {} member(s).",
        removed.name, removed.id, team_name, member_count
    ))
}

// ═══════════════════════════════════════════════════════════════════════════
// Validation Tools
// ═══════════════════════════════════════════════════════════════════════════

/// Run team composition validation and return the results.
fn validate_team(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let team_ref = arguments["team_id"]
        .as_str()
        .or_else(|| arguments["team"].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'team_id' is required")?;

    let teams = load_expert_teams(root);
    let lower = team_ref.to_lowercase();
    let slug = slugify(team_ref);
    let team = teams
        .iter()
        .find(|t| {
            t.id.to_lowercase() == lower || t.slug() == slug || t.name.to_lowercase() == lower
        })
        .ok_or_else(|| format!("team '{}' not found", team_ref))?;

    let issues = validate_team_composition(team);

    let errors: Vec<&str> = issues
        .iter()
        .filter(|i| i.severity == ValidationSeverity::Error)
        .map(|i| i.message.as_str())
        .collect();
    let warnings: Vec<&str> = issues
        .iter()
        .filter(|i| i.severity == ValidationSeverity::Warning)
        .map(|i| i.message.as_str())
        .collect();
    let infos: Vec<&str> = issues
        .iter()
        .filter(|i| i.severity == ValidationSeverity::Info)
        .map(|i| i.message.as_str())
        .collect();

    let summary = json!({
        "team": team.name,
        "team_id": team.id,
        "member_count": team.members.len(),
        "errors": errors,
        "warnings": warnings,
        "info": infos,
        "score": 100i32 - (errors.len() as i32 * 20 + warnings.len() as i32 * 5),
    });

    if errors.is_empty() && warnings.is_empty() {
        Ok(format!(
            "Team \"{}\" passed all validation checks.\n{}",
            team.name,
            serde_json::to_string_pretty(&summary)?
        ))
    } else {
        Ok(format!(
            "Team \"{}\" has {} error(s), {} warning(s), {} info:\n{}",
            team.name,
            errors.len(),
            warnings.len(),
            infos.len(),
            serde_json::to_string_pretty(&summary)?
        ))
    }
}

/// Check for scope overlaps between team members.
fn check_scope_overlaps(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let team_ref = arguments["team_id"]
        .as_str()
        .or_else(|| arguments["team"].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'team_id' is required")?;

    let teams = load_expert_teams(root);
    let lower = team_ref.to_lowercase();
    let slug = slugify(team_ref);
    let team = teams
        .iter()
        .find(|t| {
            t.id.to_lowercase() == lower || t.slug() == slug || t.name.to_lowercase() == lower
        })
        .ok_or_else(|| format!("team '{}' not found", team_ref))?;

    let overlaps = detect_scope_overlaps(team);

    let overlap_details: Vec<Value> = overlaps
        .iter()
        .map(|o| {
            json!({
                "member_a": o.member_a_name,
                "member_a_id": o.member_a_id,
                "pattern_a": o.pattern_a,
                "member_b": o.member_b_name,
                "member_b_id": o.member_b_id,
                "pattern_b": o.pattern_b,
            })
        })
        .collect();

    let summary = json!({
        "team": team.name,
        "team_id": team.id,
        "overlap_count": overlaps.len(),
        "overlaps": overlap_details,
    });

    if overlaps.is_empty() {
        Ok(format!(
            "No scope overlaps detected in team \"{}\".\n{}",
            team.name,
            serde_json::to_string_pretty(&summary)?
        ))
    } else {
        Ok(format!(
            "Found {} scope overlap(s) in team \"{}\":\n{}",
            overlaps.len(),
            team.name,
            serde_json::to_string_pretty(&summary)?
        ))
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Clone / Import / Export Tools
// ═══════════════════════════════════════════════════════════════════════════

/// Clone an existing team (including preset teams) with a new name.
fn clone_expert_team(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let team_ref = arguments["team_id"]
        .as_str()
        .or_else(|| arguments["team"].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'team_id' is required")?;

    let new_name = arguments["new_name"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'new_name' is required")?;

    let teams = load_expert_teams(root);
    let lower = team_ref.to_lowercase();
    let slug = slugify(team_ref);
    let source = teams
        .iter()
        .find(|t| {
            t.id.to_lowercase() == lower || t.slug() == slug || t.name.to_lowercase() == lower
        })
        .ok_or_else(|| format!("team '{}' not found", team_ref))?;

    let source_name = source.name.clone();
    let cloned = source.clone_with_name(new_name);
    let member_count = cloned.members.len();
    let cloned_id = cloned.id.clone();

    // Check for slug collision
    let new_slug = cloned.slug();
    if teams.iter().any(|t| t.slug() == new_slug) {
        return Err(format!(
            "a team with slug '{}' already exists; choose a different name",
            new_slug
        )
        .into());
    }

    let mut teams = teams;
    teams.push(cloned);
    if !save_expert_teams(root, &teams) {
        return Err("failed to persist expert_teams.nda".into());
    }

    Ok(format!(
        "Cloned team \"{}\" as \"{}\" (id: {}, slug: {}) with {} member(s). Edit the clone with @{}.",
        source_name, new_name, cloned_id, new_slug, member_count, new_slug
    ))
}

/// Export a team to JSON format for sharing or backup.
fn export_expert_team(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let team_ref = arguments["team_id"]
        .as_str()
        .or_else(|| arguments["team"].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'team_id' is required")?;

    let teams = load_expert_teams(root);
    let lower = team_ref.to_lowercase();
    let slug = slugify(team_ref);
    let team = teams
        .iter()
        .find(|t| {
            t.id.to_lowercase() == lower || t.slug() == slug || t.name.to_lowercase() == lower
        })
        .ok_or_else(|| format!("team '{}' not found", team_ref))?;

    let json = export_team_to_json(team)?;
    Ok(format!(
        "Exported team \"{}\" ({} member(s)):\n{}",
        team.name,
        team.members.len(),
        json
    ))
}

/// Import a team from JSON format.
fn import_expert_team(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let json_str = arguments["json"]
        .as_str()
        .or_else(|| arguments["data"].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'json' is required")?;

    let imported = import_team_from_json(json_str)?;
    let member_count = imported.members.len();
    let team_name = imported.name.clone();
    let team_slug = imported.slug();

    // Merge: replace if slug matches, otherwise append
    let mut teams = load_expert_teams(root);
    let replaced = if let Some(existing) = teams.iter_mut().find(|t| t.slug() == team_slug) {
        *existing = imported;
        true
    } else {
        teams.push(imported);
        false
    };

    if !save_expert_teams(root, &teams) {
        return Err("failed to persist expert_teams.nda".into());
    }

    Ok(format!(
        "{} team \"{}\" (id: team_{}, slug: {}) with {} member(s). Address it with @{}.",
        if replaced { "Replaced" } else { "Imported" },
        team_name,
        team_slug,
        team_slug,
        member_count,
        team_slug
    ))
}

// ═══════════════════════════════════════════════════════════════════════════
// Routing Debug / Analytics Tools
// ═══════════════════════════════════════════════════════════════════════════

/// Debug the routing decision for a task without actually routing it.
fn debug_routing_tool(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let team_ref = arguments["team_id"]
        .as_str()
        .or_else(|| arguments["team"].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'team_id' is required")?;

    let task = arguments["task"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'task' is required")?;

    let files = string_array(&arguments["files"]);

    let teams = load_expert_teams(root);
    let lower = team_ref.to_lowercase();
    let slug = slugify(team_ref);
    let team = teams
        .iter()
        .find(|t| {
            t.id.to_lowercase() == lower || t.slug() == slug || t.name.to_lowercase() == lower
        })
        .ok_or_else(|| format!("team '{}' not found", team_ref))?;

    let decision = debug_routing(team, task, &files);

    let scores_json: Vec<Value> = decision
        .scores
        .iter()
        .map(|s| {
            json!({
                "member_id": s.member_id,
                "member_name": s.member_name,
                "score": s.score,
                "matched_tokens": s.matched_tokens,
            })
        })
        .collect();

    let result = json!({
        "team": team.name,
        "team_id": team.id,
        "task": task,
        "files": files,
        "decision": {
            "stage": decision.stage,
            "member_id": decision.member_id,
            "member_name": decision.member_name,
            "reason": decision.reason,
        },
        "all_scores": scores_json,
    });

    Ok(format!(
        "Routing debug for team \"{}\":\n{}",
        team.name,
        serde_json::to_string_pretty(&result)?
    ))
}

/// Show analytics and statistics for a team.
fn team_analytics(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let team_ref = arguments["team_id"]
        .as_str()
        .or_else(|| arguments["team"].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'team_id' is required")?;

    let teams = load_expert_teams(root);
    let lower = team_ref.to_lowercase();
    let slug = slugify(team_ref);
    let team = teams
        .iter()
        .find(|t| {
            t.id.to_lowercase() == lower || t.slug() == slug || t.name.to_lowercase() == lower
        })
        .ok_or_else(|| format!("team '{}' not found", team_ref))?;

    // Compute basic analytics
    let member_count = team.members.len();
    let total_skills: usize = team.members.iter().map(|m| m.skills.len()).sum();
    let total_scopes: usize = team.members.iter().map(|m| m.scope_patterns.len()).sum();
    let total_tools: usize = team.members.iter().map(|m| m.tools.len()).sum();

    // Provider distribution
    let mut provider_counts = std::collections::HashMap::new();
    for member in &team.members {
        *provider_counts
            .entry(member.provider.slug().to_string())
            .or_insert(0) += 1;
    }

    // Scope coverage analysis
    let members_with_scopes = team
        .members
        .iter()
        .filter(|m| !m.scope_patterns.is_empty())
        .count();
    let members_without_scopes = member_count - members_with_scopes;

    let analytics = json!({
        "team": team.name,
        "team_id": team.id,
        "slug": team.slug(),
        "is_preset": team.is_preset,
        "description": team.description,
        "stats": {
            "member_count": member_count,
            "total_skills": total_skills,
            "total_scope_patterns": total_scopes,
            "total_tool_restrictions": total_tools,
        },
        "provider_distribution": provider_counts,
        "scope_coverage": {
            "members_with_scopes": members_with_scopes,
            "members_without_scopes": members_without_scopes,
            "coverage_percent": (members_with_scopes * 100).checked_div(member_count).unwrap_or(0),
        },
        "members": team.members.iter().map(|m| json!({
            "id": m.id,
            "name": m.name,
            "role": m.role,
            "provider": m.provider.slug(),
            "model_id": m.model_id,
            "skills_count": m.skills.len(),
            "scopes_count": m.scope_patterns.len(),
            "tools_count": m.tools.len(),
        })).collect::<Vec<_>>(),
    });

    Ok(format!(
        "Team analytics for \"{}\":\n{}",
        team.name,
        serde_json::to_string_pretty(&analytics)?
    ))
}

// ═══════════════════════════════════════════════════════════════════════════
// Health Check / Provider Tools
// ═══════════════════════════════════════════════════════════════════════════

/// Comprehensive health check combining validation, overlaps, and analytics.
fn team_health_check(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let team_ref = arguments["team_id"]
        .as_str()
        .or_else(|| arguments["team"].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'team_id' is required")?;

    let teams = load_expert_teams(root);
    let lower = team_ref.to_lowercase();
    let slug = slugify(team_ref);
    let team = teams
        .iter()
        .find(|t| {
            t.id.to_lowercase() == lower || t.slug() == slug || t.name.to_lowercase() == lower
        })
        .ok_or_else(|| format!("team '{}' not found", team_ref))?;

    // Run all checks
    let issues = validate_team_composition(team);
    let overlaps = detect_scope_overlaps(team);

    let errors: Vec<&str> = issues
        .iter()
        .filter(|i| i.severity == ValidationSeverity::Error)
        .map(|i| i.message.as_str())
        .collect();
    let warnings: Vec<&str> = issues
        .iter()
        .filter(|i| i.severity == ValidationSeverity::Warning)
        .map(|i| i.message.as_str())
        .collect();

    // Compute health score (0-100)
    let base_score = 100i32 - (errors.len() as i32 * 25 + warnings.len() as i32 * 10);
    let overlap_penalty = overlaps.len() as i32 * 5;
    let health_score = base_score.saturating_sub(overlap_penalty).max(0);

    // Determine health status
    let status = if health_score >= 90 {
        "excellent"
    } else if health_score >= 70 {
        "good"
    } else if health_score >= 50 {
        "fair"
    } else {
        "poor"
    };

    // Provider diversity check
    let mut providers = std::collections::HashSet::new();
    for m in &team.members {
        providers.insert(m.provider.slug());
    }
    let provider_diversity = providers.len();

    let health = json!({
        "team": team.name,
        "team_id": team.id,
        "health_score": health_score,
        "status": status,
        "summary": {
            "errors": errors.len(),
            "warnings": warnings.len(),
            "scope_overlaps": overlaps.len(),
            "provider_diversity": provider_diversity,
        },
        "error_details": errors,
        "warning_details": warnings,
        "recommendations": {
            "has_errors": !errors.is_empty(),
            "has_warnings": !warnings.is_empty(),
            "has_overlaps": !overlaps.is_empty(),
            "low_diversity": provider_diversity == 1 && team.members.len() > 2,
        },
    });

    Ok(format!(
        "Team health check for \"{}\": {} (score: {}/100)\n{}",
        team.name,
        status.to_uppercase(),
        health_score,
        serde_json::to_string_pretty(&health)?
    ))
}

/// List all available AI providers with their slugs and labels.
fn list_providers(_root: &Path, _arguments: &Value) -> Result<String, Box<dyn Error>> {
    let providers = vec![
        json!({"slug": "cloudflare", "label": "Cloudflare Workers AI", "aliases": ["cf", "workers-ai"]}),
        json!({"slug": "openrouter", "label": "OpenRouter", "aliases": ["or"]}),
        json!({"slug": "azure", "label": "Azure OpenAI", "aliases": ["azure_openai"]}),
        json!({"slug": "ollama", "label": "Local Ollama", "aliases": ["local"]}),
        json!({"slug": "openai", "label": "OpenAI Direct", "aliases": []}),
        json!({"slug": "anthropic", "label": "Anthropic Claude", "aliases": ["claude"]}),
        json!({"slug": "vertex", "label": "Google Vertex AI", "aliases": ["google"]}),
        json!({"slug": "deepseek", "label": "Deepseek", "aliases": []}),
        json!({"slug": "alibaba", "label": "Alibaba Qwen", "aliases": ["qwen", "dashscope"]}),
        json!({"slug": "bedrock", "label": "AWS Bedrock", "aliases": ["aws"]}),
        json!({"slug": "groq", "label": "Groq", "aliases": []}),
        json!({"slug": "mistral", "label": "Mistral AI", "aliases": ["mistralai"]}),
        json!({"slug": "together", "label": "Together AI", "aliases": ["togetherai"]}),
        json!({"slug": "fireworks", "label": "Fireworks AI", "aliases": ["fireworksai"]}),
        json!({"slug": "perplexity", "label": "Perplexity", "aliases": ["pplx"]}),
        json!({"slug": "cerebras", "label": "Cerebras", "aliases": []}),
    ];

    Ok(format!(
        "Available AI providers ({}):\n{}",
        providers.len(),
        serde_json::to_string_pretty(&providers)?
    ))
}

// ═══════════════════════════════════════════════════════════════════════════
// Quick-Create / Bulk Import Tools
// ═══════════════════════════════════════════════════════════════════════════

/// Quick-create a team with minimal required information and sensible defaults.
fn create_team_quick(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let team_name = arguments["name"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'name' is required")?;

    let description = arguments["description"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();

    // Parse member names (comma-separated or array)
    let member_names: Vec<String> = if let Some(arr) = arguments["members"].as_array() {
        arr.iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else if let Some(names_str) = arguments["members"].as_str() {
        names_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        return Err("'members' is required (array or comma-separated string)".into());
    };

    if member_names.is_empty() {
        return Err("at least one member name is required".into());
    }

    let slug = slugify(team_name);
    if slug.is_empty() {
        return Err("'name' must contain alphanumeric characters".into());
    }
    let team_id = format!("team_{}", slug);

    // Create members with defaults
    let members: Vec<ExpertMember> = member_names
        .iter()
        .enumerate()
        .map(|(i, name)| ExpertMember {
            id: format!("member_{}_{}", slug, i + 1),
            name: name.clone(),
            role: if i == 0 {
                "Team Lead".to_string()
            } else {
                "Specialist".to_string()
            },
            provider: AiProvider::CloudflareWorkersAi,
            model_id: String::new(),
            skills: vec!["system_tools".to_string()],
            scope_patterns: Vec::new(),
            tools: Vec::new(),
            workflow_instructions: String::new(),
            fallback_provider: None,
        })
        .collect();

    let member_count = members.len();
    let new_team = ExpertTeam {
        id: team_id.clone(),
        name: team_name.to_string(),
        description,
        members,
        is_preset: false,
    };

    // Merge with existing teams
    let mut teams = load_expert_teams(root);
    let replaced = if let Some(existing) = teams
        .iter_mut()
        .find(|t| t.id == team_id || t.slug() == slug)
    {
        *existing = new_team;
        true
    } else {
        teams.push(new_team);
        false
    };

    if !save_expert_teams(root, &teams) {
        return Err("failed to persist expert_teams.nda".into());
    }

    Ok(format!(
        "{} team \"{}\" (id: {}, slug: {}) with {} member(s) using default settings. Customize with update_team_member.",
        if replaced { "Updated" } else { "Created" },
        team_name,
        team_id,
        slug,
        member_count
    ))
}

/// Bulk import multiple members to an existing team.
fn bulk_import_members(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let team_ref = arguments["team_id"]
        .as_str()
        .or_else(|| arguments["team"].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'team_id' is required")?;

    let members_spec = arguments["members"]
        .as_array()
        .ok_or("'members' must be an array")?;

    if members_spec.is_empty() {
        return Err("'members' array is empty".into());
    }

    let mut teams = load_expert_teams(root);
    let team = find_team_mut(&mut teams, team_ref)
        .ok_or_else(|| format!("team '{}' not found", team_ref))?;

    if team.is_preset {
        return Err("cannot add members to preset teams; clone first".into());
    }

    let team_slug = team.slug();
    let start_index = team.members.len();
    let mut added_count = 0;

    for (i, spec) in members_spec.iter().enumerate() {
        let member = parse_member(spec, &team_slug, start_index + i)?;
        team.add_member(member)?;
        added_count += 1;
    }

    let team_name = team.name.clone();
    let total_members = team.members.len();

    if !save_expert_teams(root, &teams) {
        return Err("failed to persist expert_teams.nda".into());
    }

    Ok(format!(
        "Added {} member(s) to team \"{}\". Team now has {} member(s) total.",
        added_count, team_name, total_members
    ))
}

// ═══════════════════════════════════════════════════════════════════════════
// Version Control Tools
// ═══════════════════════════════════════════════════════════════════════════

/// Generate a versioned snapshot of a team for change tracking.
fn team_changelog(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let team_ref = arguments["team_id"]
        .as_str()
        .or_else(|| arguments["team"].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'team_id' is required")?;

    let teams = load_expert_teams(root);
    let lower = team_ref.to_lowercase();
    let slug = slugify(team_ref);
    let team = teams
        .iter()
        .find(|t| {
            t.id.to_lowercase() == lower || t.slug() == slug || t.name.to_lowercase() == lower
        })
        .ok_or_else(|| format!("team '{}' not found", team_ref))?;

    // Generate a deterministic snapshot hash
    let snapshot =
        serde_json::to_string(team).map_err(|e| format!("failed to serialize team: {}", e))?;
    let hash = format!("{:x}", md5_hash(snapshot.as_bytes()));

    let changelog = json!({
        "team": team.name,
        "team_id": team.id,
        "slug": team.slug(),
        "snapshot_hash": hash,
        "member_count": team.members.len(),
        "is_preset": team.is_preset,
        "members": team.members.iter().map(|m| json!({
            "id": m.id,
            "name": m.name,
            "role": m.role,
            "provider": m.provider.slug(),
            "fallback_provider": m.fallback_provider.map(|p| p.slug()),
        })).collect::<Vec<_>>(),
    });

    Ok(format!(
        "Team snapshot for \"{}\" (hash: {}):\n{}",
        team.name,
        &hash[..8],
        serde_json::to_string_pretty(&changelog)?
    ))
}

/// Simple MD5-like hash for snapshot identification.
fn md5_hash(data: &[u8]) -> u128 {
    let mut hash: u128 = 0;
    for (i, &byte) in data.iter().enumerate() {
        hash = hash.wrapping_mul(31).wrapping_add(byte as u128);
        hash = hash.rotate_left((i % 16) as u32);
    }
    hash
}

/// Determine which providers have credentials configured in the workspace.
/// Returns a list of (provider, is_configured) pairs in priority order.
///
/// The table itself lives on `WorkspaceProviderSettings` so the editor's startup
/// default and the workflow router resolve the same answer from the same data.
pub(crate) fn configured_providers(root: &Path) -> Vec<(AiProvider, bool)> {
    load_workspace_provider_settings(root).credentials()
}

/// Get the first configured provider as the workspace default.
/// Falls back to LocalOllama (local, no API key needed) rather than
/// Cloudflare when nothing is configured.
fn default_configured_provider(root: &Path) -> AiProvider {
    configured_providers(root)
        .into_iter()
        .find(|(_, configured)| *configured)
        .map(|(provider, _)| provider)
        .unwrap_or(AiProvider::LocalOllama)
}

/// The model the user has actively selected in the IDE, read from
/// `.velocity/workspace-preferences.json` (persisted by the chat panel).
/// Dispatched members must inherit this instead of falling through to
/// hardcoded per-provider defaults — a stale default like `qwen-max` 404s
/// on current token-plan endpoints that no longer list the legacy model.
fn workspace_active_model(root: &Path) -> Option<(AiProvider, String)> {
    let contents =
        std::fs::read_to_string(root.join(".velocity").join("workspace-preferences.json")).ok()?;
    let stripped = contents.strip_prefix('\u{feff}').unwrap_or(&contents);
    let prefs: Value = serde_json::from_str(stripped).ok()?;
    let model = prefs["selected_model"].as_str()?.trim().to_string();
    if model.is_empty() {
        return None;
    }
    let provider = AiProvider::from_label(prefs["provider"].as_str()?)?;
    Some((provider, model))
}

/// True when a model id advertises a parameter count below ~7B (e.g.
/// `qwen2.5-coder:0.5b`, `llama3.2:1b`) or carries a known "toy" suffix
/// (`phi3:mini`). Sub-7B models reliably fail agentic tool-use; if one
/// serves a dispatch the result must be flagged, never a silent success.
fn is_small_model(model: &str) -> bool {
    let lower = model.to_lowercase();
    let bytes = lower.as_bytes();
    let mut largest_billions: Option<f64> = None;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            // "7b", ":1.5b", "-70b-instruct" → size mention; reject "4o".
            if i < bytes.len()
                && bytes[i] == b'b'
                && (i + 1 == bytes.len() || !bytes[i + 1].is_ascii_alphabetic())
            {
                if let Ok(v) = lower[start..i].parse::<f64>() {
                    largest_billions = Some(largest_billions.map_or(v, |p| p.max(v)));
                }
            }
        } else {
            i += 1;
        }
    }
    if let Some(v) = largest_billions {
        return v < 7.0;
    }
    lower.contains("mini") || lower.contains("nano") || lower.contains("tiny")
}

/// Pick the most agentic-capable model currently served by any configured
/// Ollama host, preferring coder-tuned ones. The static default
/// (`qwen2.5-coder:0.5b`) reliably produces confident garbage on agent
/// tasks, so it must not be the automatic choice when something bigger is
/// installed. Uses the real discovered catalog across every registered host,
/// not the old empty-accounts stub.
fn best_local_ollama_model(root: &Path) -> Option<String> {
    let accounts = crate::usage::load_local_ollama_accounts(root);
    let models = crate::agent::provider::fetch_local_ollama_models(&accounts).ok()?;
    let mut ids: Vec<String> = models
        .iter()
        .map(|m| m.id.clone())
        .filter(|id| !is_small_model(id))
        .collect();
    if ids.is_empty() {
        return None;
    }
    ids.sort_by_key(|id| if id.contains("coder") { 0 } else { 1 });
    Some(ids.remove(0))
}

/// One entry of the dispatch `fallback_attempts` log.
///
/// Kept as a pure function because the decision it records is the one that
/// decides whether a finished mission gets thrown away: `succeeded` comes from
/// the loop's structured outcome, and a blocked attempt carries its reason so a
/// caller can tell a dead provider from an exhausted turn budget without
/// re-reading status lines.
fn attempt_log_entry(
    provider: AiProvider,
    model: &str,
    outcome: &crate::agent::models::AgentLoopOutcome,
    status_count: usize,
) -> Value {
    let succeeded = outcome.is_completed();
    let mut entry = json!({
        "provider": provider.slug(),
        "model": model,
        "succeeded": succeeded,
        "quality": if succeeded && is_small_model(model) {
            "degraded"
        } else {
            "ok"
        },
        "status_count": status_count,
    });
    if !succeeded {
        entry["blocked_reason"] = json!(outcome.reason());
    }
    entry
}

/// How sound the Rust on disk is, measured with the same `cargo check` the
/// agent loop gates its writes with.
///
/// The chain deliberately does not read an attempt's prose to learn this. The
/// transcript is whatever the model chose to print, and a run that says
/// "Compiler validation succeeded!" over a file that does not build is a run
/// whose report is worth less than the two seconds it takes to check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArtifactSoundness {
    Builds,
    Broken,
    /// The check could not be run at all — no Rust project, or no cargo on
    /// PATH. Nothing may be restored on the strength of this.
    NotMeasured,
}

/// The files one attempt wrote, with their bytes, kept so that a later attempt
/// in the same chain cannot leave the workspace worse than this one did.
///
/// `convert10` is the regression: its first attempt wrote a 24 KB port that
/// built and passed 77 of its 84 tests, then the provider failed over, the
/// next attempt re-ran the task from scratch, and the 8 KB non-compiling port
/// it produced got the last write. The dispatch therefore reported failure over
/// a deliverable that had already been good.
struct AttemptArtifact {
    provider: AiProvider,
    model: String,
    files: Vec<(std::path::PathBuf, Vec<u8>)>,
    soundness: ArtifactSoundness,
}

/// Run the build check the loop would have run, but over the whole project.
fn measure_artifact_soundness(root: &Path) -> ArtifactSoundness {
    if crate::automation::build_runner::cargo_manifest_dir(root).is_err() {
        return ArtifactSoundness::NotMeasured;
    }
    match crate::agent::executor::thread::run_compilation_check(root) {
        Ok(()) => ArtifactSoundness::Builds,
        Err(_) => ArtifactSoundness::Broken,
    }
}

/// Should this failed attempt's files become the guard?
///
/// Pure so the rule can be tested without cargo. Only a build that actually
/// compiles is worth protecting — and once one attempt has left one, later
/// attempts have to beat it, not merely match it, so the first sound artifact
/// wins and the choice never depends on which model happened to be slower.
fn should_guard_artifact(soundness: ArtifactSoundness, held: bool) -> bool {
    soundness == ArtifactSoundness::Builds && !held
}

/// Should the guard be written back over what is on disk now?
///
/// Pure. Restoring is only ever a repair of a worse outcome: the chain must
/// have failed to produce a completed run, the last attempt must have measured
/// itself broken, and the earlier artifact must still be sound enough to be
/// worth having. A completed run owns the workspace — its files are never
/// reverted, however good an older attempt's were.
fn should_restore_artifact(
    chain_succeeded: bool,
    guard_soundness: ArtifactSoundness,
    final_soundness: ArtifactSoundness,
) -> bool {
    !chain_succeeded
        && guard_soundness == ArtifactSoundness::Builds
        && final_soundness == ArtifactSoundness::Broken
}

/// Read the bytes of the files this attempt wrote, skipping any it has since
/// removed so a vanished file is not resurrected as an empty one.
fn snapshot_written_files(files: &[std::path::PathBuf]) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    files
        .iter()
        .filter_map(|p| std::fs::read(p).ok().map(|bytes| (p.clone(), bytes)))
        .collect()
}

/// Whether a write-back actually repaired the workspace, measured after it.
fn restore_builds(soundness: ArtifactSoundness) -> bool {
    soundness == ArtifactSoundness::Builds
}

/// What the dispatch reports after writing an earlier attempt's files back.
///
/// The wording comes from a fresh measurement, not from the revert itself:
/// restoring the guarded paths does not delete a `.rs` file a later attempt
/// wrote under *another* name — a model inventing its own target despite the
/// prompt naming one — so "reverted" on its own would claim a repaired
/// workspace the compiler still rejects.
fn artifact_restore_note(
    guard: &AttemptArtifact,
    restored_len: usize,
    after: ArtifactSoundness,
) -> String {
    match after {
        ArtifactSoundness::Builds => format!(
            "Reverted the workspace to the files {} / {} wrote: the final attempt did not compile, and its predecessor's Rust did.",
            guard.provider.slug(),
            guard.model
        ),
        other => format!(
            "Rewrote {} file(s) from {} / {}, but the project still does not compile ({:?} after the revert): a later attempt left an artifact under a different path that the revert cannot remove.",
            restored_len,
            guard.provider.slug(),
            guard.model,
            other
        ),
    }
}

/// Alternative models to try for a provider when the primary model fails,
/// best first.
///
/// The discovered catalog wins over the static list whenever one is available.
/// Hardcoded names rot: the Alibaba entries below were written against the
/// DashScope-intl plan and 404 on the token-plan endpoint, so a failover that
/// trusted them burned three doomed attempts — each with its own retry cycle —
/// before moving on to the next provider. `usable_catalog_for` is cache-backed,
/// so this costs a network round-trip at most once per TTL per provider, and
/// the static list remains as the last resort when no catalog can be reached.
fn fallback_models_for(
    provider: AiProvider,
    primary: &str,
    root: &Path,
    kind: crate::automation::instruction_registry::AgentTaskKind,
) -> Vec<String> {
    // Honest single-model evaluation: when set, the primary model is the ONLY
    // one tried for its provider, so a weaker fallback (e.g. qwen2.5-coder:32b
    // rescuing qwen3.6:35b-a3b) can't silently mask whether the pinned model
    // actually completed the task. Used by the LiteLLM crucible to prove a
    // model works on its own; default-off so normal dispatch keeps its
    // resilience. Provider-level fallback (other providers in the chain) is
    // unaffected — this only trims the same-provider model ladder.
    if std::env::var("VELOCITY_SINGLE_MODEL_DISPATCH")
        .is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    {
        let _ = (provider, primary, root, kind);
        return Vec::new();
    }
    if let Some(catalog) = usable_catalog_for(root, provider) {
        let ranked = rank_catalog_fallbacks(&catalog, primary, provider, kind);
        if !ranked.is_empty() {
            return ranked;
        }
    }
    let mut models = Vec::new();
    match provider {
        AiProvider::AlibabaQwen => {
            for m in &[
                "qwen-plus",
                "qwen-turbo",
                "qwen-max-latest",
                "qwen3.8-flash",
                "qwen3.8-max-0902",
                "deepseek-v3",
                "deepseek-r1",
                "glm-5.3",
                "kimi-k3",
            ] {
                if *m != primary {
                    models.push(m.to_string());
                }
            }
        }
        AiProvider::OpenRouter => {
            for m in &[
                "tencent/hy3:free",
                "google/gemini-2.0-flash-exp:free",
                "meta-llama/llama-3.3-70b-instruct:free",
            ] {
                if *m != primary {
                    models.push(m.to_string());
                }
            }
        }
        AiProvider::CloudflareWorkersAi => {
            for m in &[
                "@cf/qwen/qwen2.5-coder-7b-instruct",
                "@cf/meta/llama-3.3-70b-instruct-fp8-fast",
            ] {
                if *m != primary {
                    models.push(m.to_string());
                }
            }
        }
        AiProvider::LocalOllama => {
            // Ordered most→least capable: agentic tool-use degrades badly
            // under ~7B, so the largest installed models are tried first.
            for m in &[
                "qwen2.5-coder:7b",
                "llama3.2:3b",
                "phi3:mini",
                "qwen2.5-coder:1.5b",
                "llama3.2:1b",
            ] {
                if *m != primary {
                    models.push(m.to_string());
                }
            }
        }
        _ => {} // Other providers: no model fallback, just move to next provider
    }
    models
}

/// Rank a discovered catalog into failover candidates, best first.
///
/// Takes the catalog as an argument so the ranking is testable without a
/// network. Two rules matter:
///
/// * Scoring is the product's own [`ModelQualityIndex`], not a private
///   heuristic — the same scores that picked the primary model decide what to
///   fall back to, so a dispatch can never degrade to a model the picker would
///   have rejected outright.
/// * Models that cannot carry tool calls are dropped rather than ranked low. A
///   headless dispatch with no working tool channel cannot write a single file,
///   so trying one is not a cheap fallback, it is a wasted attempt.
fn rank_catalog_fallbacks(
    catalog: &[crate::agent::ModelInfo],
    primary: &str,
    provider: AiProvider,
    kind: crate::automation::instruction_registry::AgentTaskKind,
) -> Vec<String> {
    crate::automation::model_quality::ModelQualityIndex::rank_models(kind, provider, catalog)
        .into_iter()
        .filter(|candidate| candidate.supports_tools && candidate.model_id != primary)
        .map(|candidate| candidate.model_id)
        .collect()
}

/// Build a fallback chain for a member, ordered by likelihood of success:
/// 1. Member's provider — only if it's configured in the workspace
/// 2. Workspace default (first configured provider)
/// 3. Member's fallback_provider — if different and configured
/// 4. Other configured workspace providers
/// 5. Member's original provider (even if unconfigured — last resort)
/// 6. LocalOllama as final fallback (might be running locally)
pub(crate) fn build_fallback_chain(
    member: &ExpertMember,
    root: &Path,
) -> Vec<(AiProvider, String)> {
    let mut chain = Vec::new();
    let configured = configured_providers(root);

    // Helper: check if a provider is configured in this workspace
    let is_configured =
        |p: AiProvider| -> bool { configured.iter().any(|(prov, yes)| *prov == p && *yes) };

    // The model the user actually selected in the IDE wins over the static
    // per-provider defaults for every chain entry on that provider — stale
    // hardcoded names (e.g. "qwen-max") 404 on current token-plan endpoints
    // that no longer list the legacy models.
    let active = workspace_active_model(root);
    let model_for = |p: AiProvider| -> String {
        match &active {
            Some((ap, m)) if *ap == p => m.clone(),
            _ => crate::agent::provider::default_provider_model(p),
        }
    };

    let member_provider_configured = is_configured(member.provider);

    // 1. Member's own provider — only first if actually configured
    if member_provider_configured {
        let default_model = model_for(member.provider);
        // Workspace active model takes precedence over member's default when
        // the provider matches — the user's explicit selection in the IDE
        // should override the provisioned team's model assignment.
        let model = match &active {
            Some((ap, m)) if *ap == member.provider => m.clone(),
            _ => {
                let (_, m) = member.resolve_effective_provider_and_model(
                    default_configured_provider(root),
                    &default_model,
                );
                m
            }
        };
        chain.push((member.provider, model));
    }

    // 2. Workspace default configured provider (if different from member's)
    let workspace_default = default_configured_provider(root);
    if workspace_default != member.provider || !member_provider_configured {
        let model = model_for(workspace_default);
        chain.push((workspace_default, model));
    }

    // 3. Member's explicit fallback_provider — if configured and not already in chain
    if let Some(fallback) = member.fallback_provider {
        if fallback != member.provider && fallback != workspace_default && is_configured(fallback) {
            let model = model_for(fallback);
            chain.push((fallback, model));
        }
    }

    // 4. Other configured workspace providers not yet in chain
    for (provider, is_yes) in &configured {
        if *is_yes
            && *provider != member.provider
            && *provider != workspace_default
            && member.fallback_provider != Some(*provider)
            && !chain.iter().any(|(p, _)| *p == *provider)
        {
            let model = model_for(*provider);
            chain.push((*provider, model));
        }
    }

    // 5. Member's original provider as last resort (even if unconfigured)
    if !member_provider_configured && !chain.iter().any(|(p, _)| *p == member.provider) {
        let default_model = model_for(member.provider);
        let (_, model) =
            member.resolve_effective_provider_and_model(workspace_default, &default_model);
        chain.push((member.provider, model));
    }

    // 6. LocalOllama as final fallback (might be running even if not in
    // settings). Prefer the biggest model actually installed locally — the
    // 0.5B static default only ever produced confident garbage.
    if !chain.iter().any(|(p, _)| *p == AiProvider::LocalOllama) {
        let model = match &active {
            Some((AiProvider::LocalOllama, m)) => m.clone(),
            _ => best_local_ollama_model(root).unwrap_or_else(|| {
                crate::agent::provider::default_provider_model(AiProvider::LocalOllama)
            }),
        };
        chain.push((AiProvider::LocalOllama, model));
    }

    // 7. Honor the provider the user actually selected in the IDE as the
    // *primary* attempt. Without this, a member provisioned with a cloud
    // provider keeps that provider first, so switching the workspace to a
    // local model (e.g. after a cloud token plan expired) still burned the
    // whole turn budget crawling through 403'd cloud keys before ever reaching
    // the model the user chose. Move the selected provider to the front and
    // pin it to the selected model; everything else stays as fallback.
    if let Some((ap, am)) = &active {
        if let Some(pos) = chain.iter().position(|(p, _)| p == ap) {
            let entry = chain.remove(pos);
            chain.insert(0, (entry.0, am.clone()));
        }
    }

    // Honest single-model evaluation: when pinned, the selected provider is the
    // ONLY one tried. A credential-less cloud fallback (a 403 Alibaba key) must
    // not take over the verdict or spend the second half of the budget retrying
    // doomed keys — that masks whether the local model actually completed the
    // task. Gated behind the same opt-in env `fallback_models_for` already
    // honors, so default (resilient) dispatch keeps its full provider chain.
    if std::env::var("VELOCITY_SINGLE_MODEL_DISPATCH")
        .is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    {
        chain.truncate(1);
    }

    chain
}

/// The closing "Required Output Format" block for a dispatched task.
///
/// Two deliverables, two formats, and getting them backwards is silent and
/// total. A task that *writes files* — porting, design, documentation, parity,
/// remediation — must be told the files are the deliverable, or the model treats
/// its text answer as the deliverable and never calls the write tool. A task
/// that *audits* — a security, durability or performance sweep — wants the
/// opposite: a structured report and no edits. Ending a porting task with
/// "produce a structured report ... just results" is why the first live LiteLLM
/// run wrote zero `.rs` files, ported zero sectors, and still reported its
/// gates green over an empty crate.
///
/// The default is the report, so every caller that does not name a deliverable
/// — the chat UI and the lead-planning dispatch — behaves exactly as before.
fn output_format_block(deliverable: &str) -> &'static str {
    match deliverable.trim().to_lowercase().as_str() {
        "artifact" | "artifacts" | "file" | "files" | "write" => {
            "\n## Required Output Format\n\
This task produces files. The deliverable is the files themselves, not a description of them: call the write tool to create or update every file the task names, at the exact paths it gives, creating parent directories as needed. A response with no writes is a failed task, not a completed one.\n\
Keep your final text response to one line: the paths you wrote, then anything you could not finish and why. Do not paste file contents back into the response.\n"
        }
        _ => "\n## Required Output Format\n\
Produce a structured report:\n\
1. **Summary** — 1-2 sentence overview of what you found/did\n\
2. **Findings** — Specific, actionable items with file:line references\n\
3. **Recommendations** — Prioritized next steps\n\
\n\
Be concise. No preamble, no exploration narrative, just results.\n",
    }
}

/// Whether the named deliverable is files the agent must write. Kept in sync
/// with `output_format_block`'s artifact arm so the prompt (which tells the
/// model "a response with no writes is a failed task") and the loop's artifact
/// gate use the identical classification and cannot drift apart.
fn deliverable_requires_artifact(deliverable: &str) -> bool {
    matches!(
        deliverable.trim().to_lowercase().as_str(),
        "artifact" | "artifacts" | "file" | "files" | "write"
    )
}

/// The `## Target Files (pre-loaded)` blocks for the paths a dispatch names.
///
/// Callers use this field two ways: as a list of files to work on, and as a
/// list of *scopes* — `"files": ["rust_port"]` meaning "that crate". A scope is
/// a directory, and `read_to_string` on a directory fails on Windows with the
/// opaque `Access is denied. (os error 5)`, which gateprobe5 embedded in the
/// prompt verbatim under the heading `### rust_port`. A model reading that
/// believes its own target is unreadable, and the sentence tells it nothing
/// about what actually exists. Name the entries instead — that is the answer to
/// the question the scope was asking.
fn preload_target_files(root: &Path, files: &[String]) -> Vec<String> {
    // Prompt budget per file, and per directory listing. Both bounds exist to
    // keep a dispatch on a large crate from spending the whole context window
    // before the task text is even reached.
    const MAX_BYTES: usize = 12_000;
    const MAX_LISTED: usize = 25;

    let mut contexts = Vec::new();
    for file in files {
        let full_path = root.join(file);
        if full_path.is_dir() {
            let prefix = file.trim_end_matches(['/', '\\']);
            let mut entries: Vec<String> = std::fs::read_dir(&full_path)
                .map(|rd| {
                    rd.filter_map(|e| e.ok())
                        .filter(|e| e.path().is_file())
                        .map(|e| format!("{prefix}/{}", e.file_name().to_string_lossy()))
                        .collect()
                })
                .unwrap_or_default();
            entries.sort();
            let mut body = entries
                .iter()
                .take(MAX_LISTED)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");
            if entries.len() > MAX_LISTED {
                body.push_str(&format!("\n…and {} more", entries.len() - MAX_LISTED));
            } else if body.is_empty() {
                body = "(no files in it)".to_string();
            }
            contexts.push(format!(
                "### {file} is a directory, not a file, so there is nothing to preload. Its \
                 entries — read one of these paths, or write a new one there:\n{body}\n"
            ));
            continue;
        }
        match std::fs::read_to_string(&full_path) {
            Ok(contents) => {
                // Truncate very large files to keep the prompt manageable — at a
                // character boundary. Slicing at the raw byte index panics the
                // dispatch thread on any file whose 12,000th byte falls inside a
                // multi-byte character, which is every non-ASCII source.
                let truncated = if contents.len() > MAX_BYTES {
                    let mut cut = MAX_BYTES;
                    while cut > 0 && !contents.is_char_boundary(cut) {
                        cut -= 1;
                    }
                    format!(
                        "{}\n... [truncated, {} bytes total]",
                        &contents[..cut],
                        contents.len()
                    )
                } else {
                    contents
                };
                contexts.push(format!("### {file}\n```\n{truncated}\n```"));
            }
            Err(e) => {
                contexts.push(format!("### {file}\n[Could not read {file}: {e}]\n"));
            }
        }
    }
    contexts
}

/// Dispatch a task to the best-matching team member, executing it via a
/// headless sub-agent using the member's configured provider and model.
///
/// This is the bridge between the team organizational layer and the
/// execution layer: it routes the task, builds a persona prompt, and
/// runs the agent autonomously. If the primary provider fails, it
/// falls back through the member's fallback_provider and other
/// configured workspace providers.
/// Decide whether a `team_dispatch` request should run through the
/// headless-VC pipeline rather than the single-member chain, and on which
/// axis.
///
/// Returns `None` (fall through to the legacy path) whenever the caller
/// pinned a member/role — pinning is an explicit human instruction, and
/// silently widening a "send this to X" dispatch behind the operator's back
/// is worse than ignoring the classifier. Otherwise, the classifier's
/// scope/horizon verdict decides.
pub(crate) fn pipeline_branch_for(
    task: &str,
    arguments: &Value,
) -> Option<(
    crate::orchestrator::team_pipeline::PipelineTrigger,
    crate::agent::intent::IntentDecision,
)> {
    let pinned = arguments["member"]
        .as_str()
        .or_else(|| arguments["role"].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if pinned.is_some() {
        return None;
    }
    let decision = crate::agent::intent::classify(task);
    crate::orchestrator::team_pipeline::pipeline_trigger(&decision).map(|t| (t, decision))
}

fn team_dispatch(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let team_ref = arguments["team_id"]
        .as_str()
        .or_else(|| arguments["team"].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'team_id' is required")?;

    let task = arguments["task"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'task' is required")?;

    let files = string_array(&arguments["files"]);

    // Resolve the team
    let teams = load_expert_teams(root);
    let lower = team_ref.to_lowercase();
    let slug = slugify(team_ref);
    let team = teams
        .iter()
        .find(|t| {
            t.id.to_lowercase() == lower || t.slug() == slug || t.name.to_lowercase() == lower
        })
        .ok_or_else(|| format!("team '{}' not found", team_ref))?;

    // System C chokepoint (spec §6.2): the per-dispatch admission, consulting
    // the same `Governor::require_dispatch` seam as `run_mission` — in a
    // governed workspace the role staffing this team must hold `DispatchTo` on
    // its resource, and a denial lands in the durable op-log (INV-6) before
    // the error surfaces. Ungoverned workspaces are inert, so every existing
    // dispatch path is untouched.
    {
        let governor = crate::automation::Governor::open(root);
        if governor.governed() {
            governor.require_dispatch(&team.slug())?;
        }
    }

    // ── P1 branch: route Team/Long dispatches through the orchestrator ──
    //
    // Two axes from `intent::classify` decide whether the single-member
    // headless chain can do the work: how *wide* the request is (scope) and
    // how *long* it will run for (horizon). Either one crossing its
    // threshold makes the pipeline the right tool — one agent, one context
    // window and one wall-clock budget cannot rewrite a repository.
    //
    // Pinning is honoured before the branch: an operator who names a seat
    // ("send this to the security engineer") meant one member, not a
    // parallel plan, and quietly widening that dispatch behind their back
    // is worse than ignoring the classifier. The whole guard lives in
    // `pipeline_branch_for` so it is unit-testable in isolation and
    // non-vacuity is provable by byte-revert of a single call site.
    if let Some((trigger, decision)) = pipeline_branch_for(task, arguments) {
        return dispatch_via_pipeline(root, team, task, &files, arguments, trigger, &decision);
    }

    // Route to the best member — unless the caller named the seat.
    //
    // Keyword routing is the right answer for a chat message and the wrong
    // answer for an unattended mission runner. "Run the security sweep over
    // these 40 files" scoring highest on the Documentation Engineer is not a
    // visible failure: nobody is watching, the dispatch completes, and the
    // finding list comes back empty. Pinning is therefore exact-or-nothing —
    // it errors when the named member does not exist instead of quietly
    // substituting a different specialist.
    let pinned = arguments["member"]
        .as_str()
        .or_else(|| arguments["role"].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let routed = match pinned {
        Some(want) => {
            let lower = want.to_lowercase();
            team.members
                .iter()
                .find(|m| {
                    m.id.to_lowercase() == lower
                        || m.name.to_lowercase() == lower
                        || m.role.to_lowercase() == lower
                })
                .or_else(|| {
                    team.members
                        .iter()
                        .find(|m| m.role.to_lowercase().contains(&lower))
                })
                .map(|m| crate::editor::team_router::RoutedMember {
                    member_id: m.id.clone(),
                    reason: format!("pinned to '{want}' by the caller"),
                })
                .ok_or_else(|| {
                    format!(
                        "member '{}' not found on team '{}'; refusing to substitute — \
                         an unattended dispatch that lands on the wrong specialist \
                         reports success while doing the wrong job",
                        want, team.name
                    )
                })?
        }
        None => {
            route_member(team, task, &files, None).ok_or("routing failed: team has no members")?
        }
    };

    let member = team
        .members
        .iter()
        .find(|m| m.id == routed.member_id)
        .ok_or_else(|| format!("routed member '{}' not found", routed.member_id))?;

    // Build surgical prompt with pre-injected file context
    let mut prompt = String::new();

    // ── Identity ───────────────────────────────────────────────────
    prompt.push_str(&format!(
        "You are {}, {} on the {} team.\n",
        member.name, member.role, team.name
    ));
    if !member.skills.is_empty() {
        prompt.push_str(&format!("Expertise: {}\n", member.skills.join(", ")));
    }

    // ── Pre-injected file context ──────────────────────────────────
    // Read target files now so the agent doesn't waste turns exploring.
    let file_contexts = preload_target_files(root, &files);

    // ── Task framing ───────────────────────────────────────────────
    prompt.push_str(&format!("\n## Task\n{}\n", task));

    if !file_contexts.is_empty() {
        prompt.push_str("\n## Target Files (pre-loaded)\n");
        for ctx in &file_contexts {
            prompt.push_str(ctx);
            prompt.push('\n');
        }
    }

    // ── Scope boundaries ───────────────────────────────────────────
    prompt.push_str("\n## Constraints\n");
    prompt.push_str("- ONLY work on the specified files and task above.\n");
    prompt.push_str("- Do NOT explore unrelated files, run cargo commands, or check workspace structure unless the task explicitly requires it.\n");
    prompt.push_str("- Do NOT create checkpoints, record events, or use meta-tools unless the task explicitly requires it.\n");
    prompt.push_str("- Do NOT go on tangents or explore beyond the stated scope.\n");
    prompt.push_str("- If you need to read a file not provided above, read ONLY the specific file relevant to the task.\n");

    // ── Output format ──────────────────────────────────────────────
    // Named by the caller, never inferred here: the dispatch layer cannot tell
    // an audit from a port by reading the task, and guessing wrong means the
    // agent produces the wrong kind of deliverable while still reporting
    // success. Absent the argument, the report format is the default.
    let deliverable = arguments["deliverable"].as_str().unwrap_or("report");
    prompt.push_str(output_format_block(deliverable));

    // ── Routing metadata ───────────────────────────────────────────
    prompt.push_str(&format!(
        "\n(Routing: {} → {} via: {})\n",
        team.name, member.name, routed.reason
    ));
    if !member.workflow_instructions.is_empty() {
        prompt.push_str(&format!(
            "(Team instructions: {})\n",
            member.workflow_instructions
        ));
    }

    // Build fallback chain and execute with failover. When neither the member
    // nor the user named a model, head the chain with the decision brain's
    // pick: static per-provider defaults are legacy names (qwen-max et al.)
    // that 404 on current token-plan endpoints — the LiteLLM crucible's first
    // live dispatch burned 127s rediscovering exactly that.
    let mut fallback_chain = build_fallback_chain(member, root);
    if member.model_id.trim().is_empty() && workspace_active_model(root).is_none() {
        if let Ok(raw) = select_model_tool(root, &json!({ "task": task })) {
            if let Ok(v) = serde_json::from_str::<Value>(&raw) {
                if let (Some(p), Some(m)) = (
                    v["selected"]["provider"]
                        .as_str()
                        .and_then(AiProvider::from_slug),
                    v["selected"]["model_id"].as_str(),
                ) {
                    let pick = (p, m.to_string());
                    if !fallback_chain.contains(&pick) {
                        fallback_chain.insert(0, pick);
                    }
                }
            }
        }
    }
    let scoped_files = if files.is_empty() {
        None
    } else {
        Some(files.iter().map(|f| root.join(f)).collect())
    };

    let mut all_status_updates = Vec::new();
    let mut final_transcript = String::new();
    let mut used_provider = fallback_chain
        .first()
        .map(|(p, _)| *p)
        .unwrap_or(AiProvider::CloudflareWorkersAi);
    let mut used_model = fallback_chain
        .first()
        .map(|(_, m)| m.clone())
        .unwrap_or_default();
    let mut attempt_log = Vec::new();
    // Set when some model in the chain actually completed the loop. Exposed as
    // a top-level field because a caller — the mission runner, which dispatches
    // thousands of these unattended — must be able to branch on it. Deriving it
    // from `fallback_attempts[].succeeded` works but is an invitation to scan
    // the transcript instead, which is the exact defect this handler already
    // had once.
    let mut succeeded = false;
    // Set when the winning attempt came from a sub-7B model: the result is
    // returned, but flagged — never reported as a silent clean success.
    let mut degraded_note: Option<String> = None;
    // The best sound artifact any failed attempt left on disk, kept so the
    // chain can hand it back rather than hand the caller a workspace the last
    // attempt broke. See `AttemptArtifact`.
    let mut artifact_guard: Option<AttemptArtifact> = None;
    let dispatch_start = std::time::Instant::now();
    let budget = crate::agent::unattended_budget();
    // Inferred once, from the prompt every attempt receives, so all candidates
    // are ranked against each other on the same terms.
    let task_kind = crate::editor::app::helpers::infer_task_kind_from_goal(&prompt);
    // Computed from the same classification that chose the output-format block,
    // so the two cannot drift. Also decides whether an attempt's files are worth
    // guarding: a text answer has nothing on disk to lose.
    let requires_artifact = deliverable_requires_artifact(deliverable);

    'chain: for (provider, model) in &fallback_chain {
        // Try the primary model first, then fallback models for this provider (max 3 attempts per provider)
        let mut models_to_try = vec![model.clone()];
        let fallback = fallback_models_for(*provider, model, root, task_kind);
        models_to_try.extend(fallback.into_iter().take(2)); // Limit to 3 total attempts per provider

        let mut provider_succeeded = false;

        for try_model in &models_to_try {
            // Each attempt is handed what is actually left, not the original
            // budget, so the chain as a whole cannot outlive it. The old 300s
            // ceiling was only consulted *before starting another attempt*,
            // which left an in-flight run unbounded: a LiteLLM port went 21
            // minutes past it and had to be killed by hand.
            let Some(attempt_budget) = crate::agent::budget_left(budget, dispatch_start.elapsed())
            else {
                log::warn!(
                    "team_dispatch: budget of {}s spent, stopping fallback attempts",
                    budget.as_secs()
                );
                final_transcript.push_str(&format!(
                    "\n\n[Dispatch budget exhausted: no model succeeded within {} seconds]",
                    budget.as_secs()
                ));
                break 'chain;
            };
            // The collector is what makes the guard possible: the loop reports
            // every file it wrote through its event channel, and only those
            // files — never the whole workspace — are snapshotted and, if the
            // chain ends badly, written back.
            let progress = std::sync::Arc::new(std::sync::Mutex::new(
                crate::agent::HeadlessSubAgentProgress::default(),
            ));
            let result =
                crate::agent::run_headless_subagent(crate::agent::HeadlessSubAgentRequest {
                    workspace_root: root.to_path_buf(),
                    provider: *provider,
                    model: try_model.clone(),
                    thinking: false,
                    prompt: prompt.clone(),
                    cancel_rx: None,
                    progress: Some(progress.clone()),
                    scoped_files: scoped_files.clone(),
                    // Repo-scale ingest/port tasks legitimately need 10+
                    // list/read turns before they can answer; the previous 8
                    // truncated the LiteLLM phase-1 map mid-flight. The
                    // dispatch budget bounds the wall clock, and the ceiling
                    // now comes from it — see `turn_ceiling_for`.
                    max_turns: Some(turn_ceiling_for(attempt_budget)),
                    budget: Some(attempt_budget),
                    // Honor the prompt's own contract: when the deliverable is
                    // files, the loop must not certify a text-only answer as
                    // success.
                    requires_artifact,
                });

            all_status_updates.extend(result.status_updates.clone());
            used_provider = *provider;
            used_model = try_model.clone();

            // Success is the loop's own structured outcome, never a substring
            // hunt through its prose. The previous check scanned the transcript
            // for `Error:` -- which a *successful* porting run contains whenever
            // it quotes a cargo diagnostic it already fixed. That misreading
            // threw away a completed, compiler-validated LiteLLM port and re-ran
            // the whole task on two models that 404, ending in "no model
            // succeeded within 300 seconds" with the winner's transcript lost.
            let has_error = !result.outcome.is_completed();

            let degraded_small = !has_error && is_small_model(try_model);
            attempt_log.push(attempt_log_entry(
                *provider,
                try_model,
                &result.outcome,
                result.status_updates.len(),
            ));

            if !has_error {
                final_transcript = result.transcript;
                provider_succeeded = true;
                succeeded = true;
                if degraded_small {
                    degraded_note = Some(format!(
                        "output was produced by small model '{}' (<7B parameters) \u{2014} treat as unreliable; configure a cloud provider or pull a larger local model",
                        try_model
                    ));
                }
                break;
            }

            final_transcript = result.transcript;

            // An attempt that failed its own task can still have left the best
            // deliverable the dispatch will produce. Measure it with cargo
            // rather than believing its report, and keep its bytes so the next
            // attempt cannot end the run by overwriting them.
            if has_error && requires_artifact {
                let written: Vec<std::path::PathBuf> = {
                    use crate::safety::SafeMutex;
                    progress
                        .lock_safe()
                        .changed_files
                        .iter()
                        .filter(|f| f.replace('\\', "/").ends_with(".rs"))
                        .map(std::path::PathBuf::from)
                        .collect()
                };
                if !written.is_empty() {
                    let soundness = measure_artifact_soundness(root);
                    if should_guard_artifact(soundness, artifact_guard.is_some()) {
                        artifact_guard = Some(AttemptArtifact {
                            provider: *provider,
                            model: try_model.clone(),
                            files: snapshot_written_files(&written),
                            soundness,
                        });
                        all_status_updates.push(format!(
                            "Guarding the Rust {} / {} wrote: it builds, so no later attempt may be allowed to leave the workspace broken.",
                            provider.slug(),
                            try_model
                        ));
                    }
                }
            }

            // An operator stopped this run. Retrying the same mission on the
            // next model — or the next provider — would be doing work a human
            // just asked to stop, so the chain ends here.
            if result.outcome.is_cancelled() {
                log::warn!(
                    "team_dispatch: cancelled on {} / {}, abandoning fallback chain",
                    provider.slug(),
                    try_model
                );
                break 'chain;
            }

            // A spent budget ends the whole chain rather than just this
            // attempt: the next model would be handed an even smaller slice, and
            // "retry weaker with less time" is how one dispatch ends up
            // reporting a timeout that was really several stacked.
            if result.outcome.is_timed_out() {
                log::warn!(
                    "team_dispatch: {}s budget spent on {} / {}, abandoning fallback chain",
                    budget.as_secs(),
                    provider.slug(),
                    try_model
                );
                break 'chain;
            }

            // If the provider itself is unavailable (no keys/accounts) rather
            // than this one model being rejected, don't try other models on it —
            // move to the next provider. This is a variant check, not the
            // `reason().starts_with("provider unavailable")` prose match it
            // replaces: branching on a formatted string is the same defect class
            // as the transcript scan above, and it breaks the moment somebody
            // improves the wording.
            if result.outcome.is_provider_unavailable() {
                log::warn!(
                    "team_dispatch: {} unavailable (no keys/accounts), skipping to next provider",
                    provider.slug()
                );
                break;
            }

            // Model-specific failure (403/400/quota) — try next model on same provider
            log::warn!(
                "team_dispatch: {} / {} failed ({}), trying next model on same provider",
                provider.slug(),
                try_model,
                result.outcome.reason()
            );
        }

        if provider_succeeded {
            break;
        }
    }

    // The chain is over and nothing completed. If the attempt that got the last
    // write left the project not compiling while an earlier one left it
    // compiling, the caller would otherwise receive a workspace strictly worse
    // than the one this dispatch actually produced. Put the sound files back and
    // say so — the answer is still a failure, but an honest one.
    let mut restored_artifact: Option<Value> = None;
    if let Some(guard) = artifact_guard.filter(|_| !succeeded) {
        let final_soundness = measure_artifact_soundness(root);
        if should_restore_artifact(succeeded, guard.soundness, final_soundness) {
            let mut restored = Vec::new();
            let mut unrestored = Vec::new();
            for (path, bytes) in &guard.files {
                // Only files that the later attempt actually changed get
                // rewritten, so the report names what really moved rather than
                // every path the earlier attempt ever touched.
                if std::fs::read(path)
                    .map(|current| current == *bytes)
                    .unwrap_or(false)
                {
                    continue;
                }
                match std::fs::write(path, bytes) {
                    Ok(()) => restored.push(path.display().to_string()),
                    Err(e) => {
                        // A refused write has to be said out loud: this is the
                        // one way the guard can leave the workspace exactly as
                        // the failed attempt made it and still report that the
                        // earlier artifact was put back.
                        log::warn!(
                            "team_dispatch: artifact restore could not rewrite {}: {e}",
                            path.display()
                        );
                        unrestored.push(path.display().to_string());
                    }
                }
            }
            if !unrestored.is_empty() {
                let note = format!(
                    "team_dispatch: the artifact guard restored {} file(s) but could not rewrite \
                     {} ({}) — the workspace may still be the broken one",
                    restored.len(),
                    unrestored.len(),
                    unrestored.join(", ")
                );
                log::warn!("{}", note);
                all_status_updates.push(note);
                final_transcript.push_str(&format!(
                    "\n\n[ARTIFACT NOT FULLY RESTORED: {} could not be rewritten]",
                    unrestored.join(", ")
                ));
            }
            if !restored.is_empty() {
                // Re-measure: the guarded files are back, but the workspace is
                // only repaired if nothing else the last attempt wrote still
                // breaks the build.
                let after = measure_artifact_soundness(root);
                let note = artifact_restore_note(&guard, restored.len(), after);
                log::warn!("team_dispatch: {}", note);
                all_status_updates.push(note);
                final_transcript.push_str(&format!(
                    "\n\n[ARTIFACT RESTORED: {} — {} file(s) rewritten from the earlier attempt: {}{}]",
                    guard.model,
                    restored.len(),
                    restored.join(", "),
                    if restore_builds(after) {
                        String::new()
                    } else {
                        " — PROJECT STILL DOES NOT COMPILE".to_string()
                    }
                ));
                restored_artifact = Some(json!({
                    "provider": guard.provider.slug(),
                    "model": guard.model,
                    "files": restored,
                    "unrestored": unrestored.clone(),
                    "builds_after_restore": restore_builds(after),
                }));
            }
        }
    }

    // Prefix the transcript so an orchestrating agent reading only the
    // result text still sees the quality warning.
    if let Some(note) = &degraded_note {
        final_transcript = format!("[QUALITY WARNING: {}]\n\n{}", note, final_transcript);
        log::warn!("team_dispatch: degraded result — {}", note);
    }

    // Build response
    let response = json!({
        "success": succeeded,
        "team": team.name,
        "team_id": team.id,
        "routed_to": {
            "member_id": member.id,
            "member_name": member.name,
            "role": member.role,
            "reason": routed.reason,
        },
        "provider": used_provider.slug(),
        "model": used_model,
        "budget_secs": budget.as_secs(),
        "degraded": degraded_note.is_some(),
        "degraded_reason": degraded_note,
        "fallback_attempts": attempt_log,
        "artifact_restored": restored_artifact,
        "status_updates": all_status_updates,
        "transcript": final_transcript,
    });

    Ok(serde_json::to_string_pretty(&response)?)
}

/// Plan (and optionally drive) the live-VC parallel pipeline.
///
/// Split out so a caller (the `team_dispatch` P1 branch, the P5 observability
/// tool and every regression test) can inspect the graph that
/// `intent::classify` + `team.members` would produce WITHOUT actually
/// spawning workers.
///
/// The crucible runs on models that take real wall-clock minutes per
/// dispatch. Proving the guard fired and the DAG shape is correct does not
/// need those minutes spent.
///
/// `dry_run` returns the plan; `execute` runs the tick loop to completion.
pub(crate) fn dispatch_via_pipeline(
    root: &Path,
    team: &crate::editor::expert_team::ExpertTeam,
    task: &str,
    files: &[String],
    arguments: &Value,
    trigger: crate::orchestrator::team_pipeline::PipelineTrigger,
    decision: &crate::agent::intent::IntentDecision,
) -> Result<String, Box<dyn Error>> {
    use crate::orchestrator::team_pipeline as pipeline;
    use velocity_ide::site_map::SiteMap;

    let deliverable = arguments["deliverable"].as_str().unwrap_or("report");
    let dry_run = arguments["dry_run"].as_bool().unwrap_or(false);

    let mut role_tasks = pipeline::build_team_graph(task, team, files);
    let chain_builder = |member: &crate::editor::expert_team::ExpertMember| {
        build_fallback_chain(member, root)
            .into_iter()
            .map(
                |(provider, model_id)| crate::automation::task_router::RoutedModelRoute {
                    provider,
                    model_label: model_id.clone(),
                    model_id,
                    thinking: false,
                    score: 0,
                },
            )
            .collect::<Vec<_>>()
    };
    let preload = |scoped: &[String]| preload_target_files(root, scoped);
    pipeline::fill_assignment_context(
        &mut role_tasks,
        team,
        task,
        files,
        &chain_builder,
        deliverable,
        &preload,
    );
    let mut graph = pipeline::graph_from_role_tasks(task, &role_tasks);
    let weight_root = crate::automation::resolve_weight_root(root);
    let planned_site_map_root =
        SiteMap::open(&root.join(".velocity").join("site_map"), weight_root)
            .map(|sm| sm.root())
            .unwrap_or(0);

    let plan_json = json!({
        "pipeline": pipeline::trigger_label(trigger),
        "team_id": team.id,
        "team_name": team.name,
        "rationale": decision.rationale,
        "signals": decision.signals,
        "scope": format!("{:?}", decision.scope),
        "horizon": decision.horizon.as_str(),
        "task_count": role_tasks.len(),
        "tasks": role_tasks.iter().map(|rt| json!({
            "task_id": rt.task.id.0,
            "member_id": rt.member_id,
            "role": rt.role,
            "title": rt.task.title,
            "scope": rt.task.scope,
            "dependencies": rt.task.dependencies.iter().map(|d| d.0).collect::<Vec<_>>(),
            "priority": rt.task.priority,
            "estimated_weight": rt.task.estimated_weight,
            "task_kind": rt.task_kind.as_str(),
            "provider": rt.provider.slug(),
            "model": rt.model_id,
        })).collect::<Vec<_>>(),
        "reconcile_root": graph.root.0,
        "planned_site_map_root": format!("{:016x}", planned_site_map_root),
    });

    if dry_run {
        return Ok(serde_json::to_string_pretty(&plan_json)?);
    }

    let mut outcome = pipeline::run_pipeline(
        root,
        &mut graph,
        &role_tasks,
        planned_site_map_root,
        weight_root,
        std::time::Duration::ZERO,
    );
    outcome.trigger = trigger;
    outcome.rationale = decision.rationale.clone();
    let mut response = pipeline::outcome_to_json(&outcome);
    if let Some(obj) = response.as_object_mut() {
        obj.insert("team_id".into(), json!(team.id));
        obj.insert("team_name".into(), json!(team.name));
        obj.insert("plan".into(), plan_json);
    }
    Ok(serde_json::to_string_pretty(&response)?)
}

/// Generate a wiki from the workspace site map with optional index/cache enrichment.
fn generate_wiki(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    use velocity_ide::site_map::SiteMap;
    use velocity_ide::wiki;

    let output_dir = arguments["output_dir"]
        .as_str()
        .unwrap_or("wiki")
        .trim()
        .to_string();
    let format = arguments["format"].as_str().unwrap_or("markdown").trim();
    let use_index = arguments["use_index"].as_bool().unwrap_or(true);

    // Resolve output path relative to workspace root
    let out_path = if std::path::Path::new(&output_dir).is_absolute() {
        std::path::PathBuf::from(&output_dir)
    } else {
        root.join(&output_dir)
    };

    // Load or create site map.  `index_workspace` writes to
    // `.velocity/site_map/` while `SiteMap::open` expects the directory that
    // directly contains `index.json`/`kv/`/`nodes/`/`programs/`.  Using the
    // `.velocity/` root here silently produced an empty wiki (0 KV, 0 nodes)
    // even after a successful indexing run — see MCP batch3 regression.
    let velocity_dir = root.join(".velocity");
    std::fs::create_dir_all(&velocity_dir)?;
    let site_map_dir = velocity_dir.join("site_map");
    std::fs::create_dir_all(&site_map_dir)?;
    // `open` creates its own subdirectories and hands back an empty map when
    // there is no index yet, so reaching this error means unwritable storage or
    // a corrupt `index.json` — neither of which a second identical call fixes.
    // It used to retry and then `expect`, i.e. panic inside a tool handler;
    // every other caller propagates it instead.
    let sm = SiteMap::open(&site_map_dir, 0).map_err(|err| {
        format!(
            "failed to open the site map at {}: {err}. A missing index is not the \
             problem (that yields an empty wiki); this is unreadable storage or a \
             corrupt index.json.",
            site_map_dir.display()
        )
    })?;

    // Build wiki model
    let mut result = if use_index {
        wiki::build_wiki_enhanced(&sm, root)
    } else {
        let model = wiki::build_wiki(&sm);
        wiki::EnhancedWikiResult {
            model,
            pagerank: None,
            cache_stats: None,
            files_indexed: 0,
            pages_regenerated: 0,
            pages_from_cache: 0,
            elapsed_us: 0,
        }
    };

    // Enrich with structural details from index
    if use_index {
        let indices = wiki::index_workspace(root);
        wiki::enrich_with_structural_details(&mut result.model, &indices);
    }

    // Export in requested format
    let pages_written = match format {
        "html" => wiki::export_html(&result.model, &out_path)?,
        "github-pages" | "gh-pages" => wiki::export_github_pages(&result.model, &out_path)?,
        _ => wiki::export_markdown(&result.model, &out_path)?,
    };

    let response = serde_json::json!({
        "status": "success",
        "output_dir": out_path.display().to_string(),
        "format": format,
        "pages_written": pages_written,
        "files_indexed": result.files_indexed,
        "file_pages": result.model.file_count(),
        "symbol_pages": result.model.symbol_count(),
        "elapsed_ms": result.elapsed_us / 1000,
        "cache_hits": result.pages_from_cache,
        "cache_misses": result.pages_regenerated,
    });

    Ok(serde_json::to_string_pretty(&response)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// System C chokepoint: a governed workspace stops `team_dispatch` at the
    /// gate — before routing, so nothing is spent — and the denial itself is
    /// retrievable from the durable trail (INV-6).
    #[test]
    fn team_dispatch_refuses_a_governed_workspace_without_a_grant() {
        use crate::editor::expert_team::ExpertTeam;
        let ws = tempfile::tempdir().unwrap();
        crate::editor::expert_team::save_expert_teams(
            ws.path(),
            &[ExpertTeam {
                id: "team_guard-team".into(),
                name: "Guard Team".into(),
                description: String::new(),
                members: vec![],
                is_preset: false,
            }],
        );
        let mut org = crate::automation::governor::test_governed_org();
        org.grants.clear(); // the staffing role holds nothing
        assert!(crate::editor::company_org::save_company_org(
            ws.path(),
            &org
        ));
        let err = handle_team_tool(
            ws.path(),
            "team_dispatch",
            &serde_json::json!({ "team": "Guard Team", "task": "prove the gate" }),
        )
        .unwrap_err();
        assert!(err.to_string().contains("rbac"), "{err}");
        assert!(
            crate::automation::OpLogStore::open(ws.path())
                .load()
                .ops()
                .iter()
                .any(|o| o.intent.contains("rbac-deny: dispatch_to")),
            "the denial must be in the durable trail"
        );
    }

    /// P1 branch: a sprawling port prompt must be routed through the
    /// orchestrator, not the single-member chain. The response carries
    /// `pipeline: "team_scope"` and one task per team member.
    #[test]
    fn team_dispatch_routes_whole_repo_port_through_pipeline_dry_run() {
        std::env::remove_var("VELOCITY_DECISION_BACKEND");
        use crate::editor::expert_team::{ExpertMember, ExpertTeam};
        let ws = tempfile::tempdir().unwrap();
        let members = vec![
            ExpertMember::new(
                "lead",
                "Plan Lead",
                "Planning Architect",
                AiProvider::CloudflareWorkersAi,
                "plan-model",
                vec!["plan", "architect"],
                vec![],
                "coordinate the port",
            ),
            ExpertMember::new(
                "backend",
                "Backend Engineer",
                "Backend Engineer",
                AiProvider::CloudflareWorkersAi,
                "be-model",
                vec!["rust", "backend"],
                vec!["src/backend/"],
                "port backend modules",
            ),
            ExpertMember::new(
                "auditor",
                "Security Auditor",
                "Security Auditor",
                AiProvider::CloudflareWorkersAi,
                "audit-model",
                vec!["audit", "security"],
                vec!["src/backend/"],
                "review the port",
            ),
        ];
        crate::editor::expert_team::save_expert_teams(
            ws.path(),
            &[ExpertTeam {
                id: "team_port-team".into(),
                name: "Port Team".into(),
                description: String::new(),
                members,
                is_preset: false,
            }],
        );
        let out = handle_team_tool(
            ws.path(),
            "team_dispatch",
            &serde_json::json!({
                "team": "Port Team",
                "task": "rewrite the entire repository in rust, run it end to end",
                "files": [],
                "dry_run": true,
            }),
        )
        .unwrap()
        .expect("team_dispatch must return a payload");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            v["pipeline"].as_str(),
            Some("team_scope"),
            "a whole-repo port must go through the pipeline: {out}"
        );
        assert!(
            v["task_count"].as_u64().unwrap_or(0) >= 3,
            "one task per member: {out}"
        );
        assert_eq!(v["team_name"].as_str(), Some("Port Team"));
    }

    /// P1 branch guard: when the caller names a seat, the classifier is
    /// never consulted and the pipeline is never entered. Silently widening
    /// "send this to the security engineer" behind the operator's back would
    /// be worse than ignoring the classifier entirely.
    #[test]
    fn pipeline_branch_for_returns_none_when_a_member_is_pinned() {
        std::env::remove_var("VELOCITY_DECISION_BACKEND");
        let sprawling = "rewrite the entire repository in rust, run it end to end";
        assert!(
            pipeline_branch_for(
                sprawling,
                &serde_json::json!({ "member": "Backend Engineer" })
            )
            .is_none(),
            "a pinned member must short-circuit the pipeline branch"
        );
        assert!(
            pipeline_branch_for(
                sprawling,
                &serde_json::json!({ "role": "Security Auditor" })
            )
            .is_none(),
            "a pinned role must short-circuit the pipeline branch too"
        );
        // Same prompt, no pin → the classifier must fire. This is the
        // non-vacuity half: without it, the pinned assertions above would
        // pass whether the guard checked `pinned` or always returned None.
        let unpinned = pipeline_branch_for(sprawling, &serde_json::json!({}));
        assert!(
            unpinned.is_some(),
            "unpinned sprawling prompt must enter the pipeline branch"
        );
    }

    /// P1 branch: a long-horizon request classified as `SingleAgent` scope
    /// still goes through the pipeline. The two axes are independent — a
    /// 3,000-file mechanical port is narrow but hours long, and one agent
    /// with one context window and one wall-clock budget cannot run it.
    ///
    /// The decision is constructed directly rather than pulled from the
    /// classifier: P1's guarantee is that *either* axis fires the branch,
    /// and asking `classify` to hit a specific axis pair couples this test
    /// to the whole scorer's phrase table. The pinned override is checked
    /// alongside so the guard's precedence order (pin > classifier) is
    /// covered on the horizon axis too.
    #[test]
    fn pipeline_fires_for_long_horizon_even_at_single_agent_scope() {
        let long_single = crate::agent::intent::IntentDecision {
            scope: crate::agent::intent::TaskScope::SingleAgent,
            requires_repo_ingest: false,
            repo_url: None,
            target_language: None,
            roles: vec![],
            rationale: String::new(),
            confidence: 1.0,
            signals: vec![],
            horizon: crate::agent::intent::TaskHorizon::Long,
            wants_unattended: false,
        };
        assert_eq!(
            crate::orchestrator::team_pipeline::pipeline_trigger(&long_single),
            Some(crate::orchestrator::team_pipeline::PipelineTrigger::LongHorizon),
            "long horizon must trigger the pipeline on its own axis"
        );
        // Pinned short-circuits the guard even when the classifier would
        // otherwise fire: pinning wins over both axes.
        std::env::remove_var("VELOCITY_DECISION_BACKEND");
        let sprawling = "rewrite the entire repository in rust, run it end to end";
        assert!(
            pipeline_branch_for(
                sprawling,
                &serde_json::json!({ "role": "Security Auditor" })
            )
            .is_none(),
            "pinning overrides the long-horizon axis too"
        );
    }

    #[test]
    fn is_small_model_flags_sub7b_toy_models() {
        assert!(is_small_model("qwen2.5-coder:0.5b"));
        assert!(is_small_model("qwen2.5-coder:1.5b"));
        assert!(is_small_model("llama3.2:1b"));
        assert!(is_small_model("phi3:mini"));
        assert!(!is_small_model("qwen2.5-coder:7b"));
        assert!(!is_small_model("llama-3.3-70b-versatile"));
        assert!(!is_small_model("gpt-4o"));
        assert!(!is_small_model("tencent/hy3:free"));
        assert!(!is_small_model("deepseek-chat"));
    }

    /// The crucible regression: `convert10` attempt 1 wrote a port that built
    /// and passed 77 of 84 tests; the failover attempt re-ran the task from
    /// scratch and left an 8 KB port that did not compile at all. Guarding is
    /// the chain's only chance to notice that the worse result got the last
    /// write — so only a measured build earns the guard, and only the first one.
    #[test]
    fn fallback_guards_only_a_building_artifact_and_only_the_first_one() {
        use super::ArtifactSoundness::*;
        assert!(super::should_guard_artifact(Builds, false));
        // A broken attempt has nothing worth protecting, and a workspace with
        // no Rust project in it is evidence of neither.
        assert!(!super::should_guard_artifact(Broken, false));
        assert!(!super::should_guard_artifact(NotMeasured, false));
        // An earlier sound attempt already holds the place; ties do not displace
        // it, which keeps the choice independent of which model ran slower.
        assert!(!super::should_guard_artifact(Builds, true));
    }

    #[test]
    fn fallback_restores_only_over_a_measurably_worse_result() {
        use super::ArtifactSoundness::*;
        // The exact convert10 shape: chain failed, disk is broken, guard builds.
        assert!(super::should_restore_artifact(false, Builds, Broken));
        // A completed run means the last attempt *is* the answer. Its files are
        // never overwritten by an older one, however good the older one was.
        assert!(
            !super::should_restore_artifact(true, Builds, Broken),
            "a successful attempt's artifact must not be reverted"
        );
        // Nothing wrong to restore over, and nothing sound to restore *with*.
        assert!(!super::should_restore_artifact(false, Builds, Builds));
        assert!(!super::should_restore_artifact(false, Broken, Broken));
        // An unmeasurable workspace is not a broken one; guessing here would
        // rewrite files the chain never touched.
        assert!(!super::should_restore_artifact(false, Builds, NotMeasured));
    }

    fn guard_artifact(soundness: super::ArtifactSoundness) -> AttemptArtifact {
        AttemptArtifact {
            provider: AiProvider::LocalOllama,
            model: "qwen3.6:35b-a3b".to_string(),
            files: vec![(
                std::path::PathBuf::from("rust_port/src/budget_manager.rs"),
                vec![],
            )],
            soundness,
        }
    }

    /// A revert is only worth reporting as a repair when the project builds
    /// afterwards. The guard restores the paths *it* snapshotted, so a later
    /// attempt that wrote its artifact under a different name leaves the build
    /// broken — and a message claiming "the final attempt did not compile, and
    /// its predecessor's Rust did" would be describing a workspace the compiler
    /// still rejects.
    #[test]
    fn fallback_restore_report_only_claims_a_repair_the_build_confirms() {
        use super::ArtifactSoundness::*;
        let repaired = artifact_restore_note(&guard_artifact(Builds), 1, Builds);
        assert!(
            repaired.contains("Reverted the workspace"),
            "a confirmed repair may be reported as one: {repaired}"
        );

        for still_bad in [Broken, NotMeasured] {
            let note = artifact_restore_note(&guard_artifact(Builds), 1, still_bad);
            assert!(
                !note.contains("Reverted the workspace"),
                "{still_bad:?} must not read as a completed repair: {note}"
            );
            assert!(
                note.contains("still does not compile"),
                "the report must name the remaining breakage: {note}"
            );
            assert!(!super::restore_builds(still_bad));
        }
        assert!(super::restore_builds(Builds));
    }

    /// The crucible regression. A dispatch attempt that ran 13 turns, wrote a
    /// Rust port and then reported "Compiler validation succeeded!" used to be
    /// logged `succeeded: false`, because success was inferred by scanning its
    /// transcript for `Error:` — a word the run's own report contained. The
    /// dispatcher then re-ran the whole task on two models that 404 and lost the
    /// winner's transcript. The log entry now depends only on the loop's
    /// structured outcome, so nothing an attempt prints can fail it.
    #[test]
    fn a_completed_attempt_is_never_failed_by_its_own_prose() {
        use crate::agent::models::AgentLoopOutcome;

        let entry = attempt_log_entry(
            AiProvider::AlibabaQwen,
            "glm-5.2",
            &AgentLoopOutcome::Completed,
            55,
        );
        assert_eq!(entry["succeeded"], json!(true));
        assert_eq!(entry["status_count"], json!(55));
        assert_eq!(entry["quality"], json!("ok"));
        assert!(entry.get("blocked_reason").is_none(), "{entry}");
    }

    /// A blocked attempt has to say why, so whoever reads `fallback_attempts`
    /// can tell a dead provider from an exhausted turn budget. Quality flagging
    /// only applies to output that exists: a blocked run on a small model is
    /// failed, not "degraded".
    #[test]
    fn a_blocked_attempt_carries_its_reason() {
        use crate::agent::models::AgentLoopOutcome;

        let dead_model = attempt_log_entry(
            AiProvider::AlibabaQwen,
            "qwen-plus",
            &AgentLoopOutcome::ProviderUnavailable {
                reason: "Alibaba Qwen rejected the request (404)".to_string(),
            },
            10,
        );
        assert_eq!(dead_model["succeeded"], json!(false));
        assert!(
            dead_model["blocked_reason"]
                .as_str()
                .unwrap()
                .contains("404"),
            "{dead_model}"
        );

        let small = attempt_log_entry(
            AiProvider::LocalOllama,
            "qwen2.5-coder:0.5b",
            &AgentLoopOutcome::Blocked {
                reason: "turn budget of 16 exhausted without a final answer".to_string(),
            },
            3,
        );
        assert_eq!(small["succeeded"], json!(false));
        assert_eq!(small["quality"], json!("ok"));

        // The same small model producing a real answer is the degraded case.
        let degraded = attempt_log_entry(
            AiProvider::LocalOllama,
            "qwen2.5-coder:0.5b",
            &AgentLoopOutcome::Completed,
            9,
        );
        assert_eq!(degraded["succeeded"], json!(true));
        assert_eq!(degraded["quality"], json!("degraded"));
    }

    /// A run that stopped because its wall-clock budget was spent must say so
    /// in the attempt log, with the numbers. "no model succeeded" was the old
    /// verdict and it sent the investigation towards the models instead of the
    /// ceiling that was actually too low.
    #[test]
    fn a_timed_out_attempt_records_the_budget_it_was_given() {
        use crate::agent::models::AgentLoopOutcome;

        let entry = attempt_log_entry(
            AiProvider::AlibabaQwen,
            "qwen3-max",
            &AgentLoopOutcome::TimedOut {
                elapsed_secs: 901,
                budget_secs: 900,
                turns: 11,
            },
            42,
        );
        assert_eq!(entry["succeeded"], json!(false));
        let reason = entry["blocked_reason"].as_str().unwrap();
        assert!(reason.contains("900s"), "{reason}");
        assert!(reason.contains("11 turn"), "{reason}");
        // A timeout is not a quality judgement about the model.
        assert_eq!(entry["quality"], json!("ok"));
    }

    /// Failover candidates come from the catalog the endpoint actually serves,
    /// ranked by the product's own quality index. The static list is a last
    /// resort: its Alibaba entries were written against the DashScope-intl plan
    /// and 404 on a token-plan key, which cost three doomed attempts — each
    /// with its own retry cycle — per provider.
    #[test]
    fn failover_candidates_come_from_the_catalog_and_drop_models_without_tools() {
        use crate::agent::models::{ApiStyle, ModelInfo};
        use crate::automation::instruction_registry::AgentTaskKind;

        let model = |id: &str, tools: bool| ModelInfo {
            id: id.to_string(),
            label: id.to_string(),
            api_style: if tools {
                ApiStyle::OpenAiTools
            } else {
                ApiStyle::OpenAiChat
            },
            supports_tools: tools,
            supports_thinking: false,
        };
        let catalog = vec![
            // The primary: it just failed, so offering it back is a wasted turn.
            model("qwen-plus", true),
            model("qwen3-max", true),
            model("qwen-vl-ocr", false),
            model("text-embedding-helper", false),
        ];

        let ranked = rank_catalog_fallbacks(
            &catalog,
            "qwen-plus",
            AiProvider::AlibabaQwen,
            AgentTaskKind::Refactor,
        );
        assert!(
            !ranked.iter().any(|id| id == "qwen-plus"),
            "the model that just failed is not a fallback: {ranked:?}"
        );
        assert!(
            ranked.iter().any(|id| id == "qwen3-max"),
            "a real, tool-capable catalog entry must be offered: {ranked:?}"
        );
        assert!(
            !ranked
                .iter()
                .any(|id| id.contains("vl-ocr") || id.contains("embedding")),
            "a model with no tool channel cannot do agentic work at all: {ranked:?}"
        );

        // An unreachable catalog yields nothing, which is how the caller knows
        // to fall back to the static list rather than fail over to nothing.
        assert!(rank_catalog_fallbacks(
            &[],
            "qwen-plus",
            AiProvider::AlibabaQwen,
            AgentTaskKind::Refactor
        )
        .is_empty());
    }

    #[test]
    fn analyze_repo_inventories_languages_and_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("main.py"), "print(1)").unwrap();
        std::fs::write(root.join("app.py"), "x=1").unwrap();
        std::fs::write(root.join("lib.rs"), "fn m(){}").unwrap();
        std::fs::write(root.join("src/handler.go"), "package main").unwrap();
        // .git is ignored entirely.
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/config"), "junk").unwrap();

        let prof = analyze_repo(root);
        assert_eq!(prof["file_count"].as_u64(), Some(4));
        let langs = prof["languages"].as_array().unwrap();
        // python (2 files) should rank first.
        assert_eq!(langs[0]["language"].as_str(), Some("python"));
        assert_eq!(langs[0]["files"].as_u64(), Some(2));
        let top = prof["top_level"].as_array().unwrap();
        let names: Vec<&str> = top.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(names.contains(&"src/"));
        assert!(
            !names.iter().any(|n| n.contains(".git")),
            ".git must be excluded: {names:?}"
        );
    }

    #[test]
    fn build_rewrite_plan_is_ordered_dag_with_roles() {
        let profile = serde_json::json!({
            "file_count": 120,
            "languages": [{ "language": "python", "files": 100 }],
        });
        let roles = vec![
            "Lead Architect".to_string(),
            "Rust Port Engineer".to_string(),
            "Test & QA Engineer".to_string(),
            "Reviewer".to_string(),
        ];
        let plan = build_rewrite_plan(&profile, "rust", &roles, None);
        assert_eq!(plan.len(), 12);
        assert_eq!(plan[0]["phase"].as_u64(), Some(1));
        // Owners are drawn from the roster by role semantics.
        assert_eq!(plan[0]["owner_role"].as_str(), Some("Lead Architect"));
        assert_eq!(plan[2]["owner_role"].as_str(), Some("Rust Port Engineer"));
        assert_eq!(plan[5]["owner_role"].as_str(), Some("Test & QA Engineer"));
        assert_eq!(plan[10]["owner_role"].as_str(), Some("Reviewer"));
        assert!(plan[1]["title"].as_str().unwrap().contains("rust"));

        // A real DAG: phases numbered from 1 in order, and every dependency
        // points strictly backwards. A forward or self edge would deadlock any
        // executor that waits on dependencies before starting a phase.
        for (i, p) in plan.iter().enumerate() {
            assert_eq!(p["phase"].as_u64(), Some(i as u64 + 1));
            for dep in p["depends_on"].as_array().unwrap() {
                let d = dep.as_u64().unwrap();
                assert!(
                    d < i as u64 + 1,
                    "phase {} depends on {d}, which is not earlier",
                    i + 1
                );
            }
            assert!(
                !p["goals"].as_array().unwrap().is_empty(),
                "phase {} has no goals",
                i + 1
            );
        }
        // The last phase must be reachable from the first, i.e. the plan is one
        // connected chain rather than several unrelated fragments.
        assert_eq!(plan[11]["depends_on"].as_array().unwrap().len(), 1);
    }

    /// The three sweeps the user asked for must exist as distinct, separately
    /// owned phases — not folded into one "review" phase, which is how they were
    /// effectively invisible before.
    #[test]
    fn the_plan_carries_security_durability_and_performance_sweeps() {
        let profile = serde_json::json!({ "file_count": 10, "languages": [] });
        let roles: Vec<String> = vec![];
        let plan = build_rewrite_plan(&profile, "rust", &roles, None);
        let titles: Vec<&str> = plan.iter().filter_map(|p| p["title"].as_str()).collect();
        assert!(
            titles.iter().any(|t| t.contains("Security sweep")),
            "{titles:?}"
        );
        assert!(
            titles.iter().any(|t| t.contains("Durability sweep")),
            "{titles:?}"
        );
        assert!(
            titles.iter().any(|t| t.contains("Performance sweep")),
            "{titles:?}"
        );
        assert!(
            titles.iter().any(|t| t.contains("Remediate")),
            "a sweep that only reports findings changes nothing: {titles:?}"
        );
        assert!(
            titles.iter().any(|t| t.contains("Documentation")),
            "{titles:?}"
        );
        // With an empty roster every seat falls back to a named role rather than
        // emitting an empty owner, which no router could dispatch to.
        for p in &plan {
            assert!(
                !p["owner_role"].as_str().unwrap_or_default().is_empty(),
                "phase {} has no owner",
                p["phase"]
            );
        }
    }

    /// The sweeps read the same artifact and ask different questions, so they
    /// must be dispatchable concurrently, and remediation must wait for all
    /// three — otherwise a fix lands while an auditor is still reading.
    #[test]
    fn the_sweeps_run_in_parallel_and_remediation_waits_for_all_three() {
        let profile = serde_json::json!({ "file_count": 10, "languages": [] });
        let plan = build_rewrite_plan(&profile, "rust", &[], None);
        let idx = |needle: &str| -> u64 {
            plan.iter()
                .position(|p| p["title"].as_str().unwrap().contains(needle))
                .unwrap_or_else(|| panic!("no phase matching {needle}")) as u64
                + 1
        };
        let test_parity = idx("test parity");
        let sec = idx("Security sweep");
        let dur = idx("Durability sweep");
        let perf = idx("Performance sweep");
        let remediate = idx("Remediate");
        for s in [sec, dur, perf] {
            assert_eq!(
                plan[(s - 1) as usize]["depends_on"],
                serde_json::json!([test_parity]),
                "each sweep depends on test parity and nothing else, so all three are ready at once"
            );
        }
        let mut deps: Vec<u64> = plan[(remediate - 1) as usize]["depends_on"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|d| d.as_u64())
            .collect();
        deps.sort_unstable();
        assert_eq!(deps, vec![sec, dur, perf]);
    }

    /// A sweep seat must resolve to whoever is actually on the roster: the
    /// `auth` domain hires a "Security Engineer", and a plan naming a
    /// "Security Sweep Auditor" that does not exist could never be dispatched.
    #[test]
    fn sweep_owners_fall_back_to_the_roster_they_were_given() {
        let profile = serde_json::json!({ "file_count": 10, "languages": [] });
        let roles = vec![
            "Lead Architect".to_string(),
            "Rust Port Engineer".to_string(),
            "Security Engineer".to_string(),
            "Documentation Engineer".to_string(),
            "Reviewer".to_string(),
        ];
        let plan = build_rewrite_plan(&profile, "rust", &roles, None);
        assert_eq!(
            plan[6]["owner_role"].as_str(),
            Some("Security Engineer"),
            "the security sweep goes to the security seat that exists"
        );
        assert_eq!(
            plan[11]["owner_role"].as_str(),
            Some("Documentation Engineer")
        );
    }

    /// With a measured decomposition the porting phase states real arithmetic.
    ///
    /// Every number here is what the LiteLLM scale check actually produced when
    /// run with `source_root: "litellm"`, so this test doubles as a record of
    /// them — including the scope accounting, because that run covers 68% of
    /// the repository and the phase has to say so next to the budget.
    #[test]
    fn a_sector_plan_puts_real_arithmetic_in_the_porting_phase() {
        use crate::automation::sectors::{SectorPlan, SectorTotals};
        let profile = serde_json::json!({ "file_count": 2976, "languages": [] });
        let sectors = SectorPlan {
            repo_name: "litellm".into(),
            source_root: "litellm".into(),
            sectors: vec![],
            waves: vec![],
            totals: SectorTotals {
                files: 2976,
                bytes: 39_813_145,
                lines: 950_665,
                sectors: 1174,
                dispatches: 3848,
                waves: 259,
                largest_wave: 193,
                sectors_over_budget: 136,
                files_chunked: 346,
                chunks: 1218,
                cycles_broken: 5370,
                units_over_ceiling: 0,
                generated_skipped: 15,
                generated_bytes: 5_005_235,
                generated_examples: vec![],
                outside_root_files: 2793,
                outside_root_bytes: 18_652_165,
                outside_root_examples: vec![
                    "backend/main.py (2 KB)".into(),
                    "backend/routes/__init__.py (0 KB)".into(),
                    "backend/routes/allowlist.py (4 KB)".into(),
                    "db_scripts/backfill_daily_tool_spend.sql (2 KB)".into(),
                    "db_scripts/backfill_key_total_spend.sql (2 KB)".into(),
                ],
                est_model_secs: 1_154_400,
                est_wall_hours_at_parallelism: 40.1,
                parallelism: 8,
                // Struct-update rather than a full literal: this fixture has
                // already had to be edited once because a reporting field was
                // added to `SectorTotals`, and it is not the thing under test.
                ..Default::default()
            },
        };
        let plan = build_rewrite_plan(&profile, "rust", &[], Some(&sectors));
        let goals: Vec<&str> = plan[3]["goals"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|g| g.as_str())
            .collect();
        let joined = goals.join(" | ");
        for expected in [
            "1174 sectors",
            "259 dependency waves",
            "3848 dispatch units",
            "5370 dependency edges",
            "346 files",
            "1218 line-range units",
            "320.7 model-hours",
            "40.1 wall-hours",
        ] {
            assert!(
                joined.contains(expected),
                "{expected} missing from {joined}"
            );
        }
        // The estimate must not present itself as complete.
        assert!(
            joined.contains("before counting retries"),
            "the porting phase must say what the estimate excludes: {joined}"
        );
        // Nor as covering more of the repository than it does. This is the
        // whole point of the scope fields: a plan quoting 320.7 model-hours for
        // 68% of a tree used to be indistinguishable from one quoting it for
        // all of it, and the mission that ran it still reported success.
        for expected in [
            "SCOPE:",
            "covers 68% of the source found",
            "2793 files (18215 KB)",
            "backend/main.py (2 KB)",
        ] {
            assert!(
                joined.contains(expected),
                "{expected} missing from {joined}"
            );
        }
        // Without a decomposition the same phase stays honest but vague.
        let bare = build_rewrite_plan(&profile, "rust", &[], None);
        assert_eq!(bare[3]["goals"].as_array().unwrap().len(), 1);
    }

    /// A plan that covers everything it found says nothing about scope.
    ///
    /// The caveat exists to correct a partial plan; emitting it on a complete
    /// one would train readers to skip the line that matters.
    #[test]
    fn a_full_coverage_plan_carries_no_scope_caveat() {
        use crate::automation::sectors::{SectorPlan, SectorTotals};
        let profile = serde_json::json!({ "file_count": 10, "languages": [] });
        let sectors = SectorPlan {
            repo_name: "small".into(),
            source_root: "src".into(),
            sectors: vec![],
            waves: vec![],
            totals: SectorTotals {
                files: 10,
                bytes: 40_000,
                dispatches: 12,
                est_model_secs: 3_600,
                parallelism: 8,
                ..Default::default()
            },
        };
        assert_eq!(sectors.totals.root_coverage(), 1.0);
        let plan = build_rewrite_plan(&profile, "rust", &[], Some(&sectors));
        let goals: Vec<&str> = plan[3]["goals"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|g| g.as_str())
            .collect();
        assert_eq!(goals.len(), 4, "{goals:?}");
        assert!(!goals.join(" ").contains("SCOPE:"), "{goals:?}");
    }

    #[test]
    fn the_feasibility_verdict_scales_with_the_estimate() {
        use crate::automation::sectors::SectorTotals;
        let mk = |dispatches: usize, wall: f64| SectorTotals {
            dispatches,
            est_model_secs: dispatches as u64 * 300,
            est_wall_hours_at_parallelism: wall,
            parallelism: 8,
            ..Default::default()
        };
        assert!(mk(4, 0.2).feasibility().contains("one sitting"));
        assert!(mk(40, 5.0).feasibility().contains("working day"));
        assert!(mk(400, 40.1).feasibility().contains("multi-day"));
        let huge = mk(4000, 400.0).feasibility();
        assert!(huge.contains("not one mission"), "{huge}");
        // Every band must name the cost it is judging, or the verdict is an
        // opinion with no evidence attached.
        for v in [mk(4, 0.2).feasibility(), mk(4000, 400.0).feasibility()] {
            assert!(v.contains("dispatches"), "{v}");
            assert!(v.contains("model-hours"), "{v}");
            assert!(v.contains("excludes retries"), "{v}");
        }
    }

    /// Mission state lives outside the clone, because the clone is disposable
    /// and the progress record is the only thing that makes resume possible.
    #[test]
    fn mission_paths_are_stable_and_outside_the_clone() {
        let rel = mission_rel("litellm", "rust");
        assert_eq!(
            rel,
            std::path::Path::new(".velocity/missions/litellm-rust"),
            "{rel:?}"
        );
        assert_eq!(rel, mission_rel("litellm", "rust"), "must be deterministic");
        let root = std::path::Path::new("/ws");
        assert_eq!(
            mission_dir(root, "litellm", "rust"),
            root.join(".velocity/missions/litellm-rust")
        );
        // A name that slugifies to nothing must not produce an empty path,
        // which would resolve to the workspace root and write mission state
        // over the user's own files.
        assert_eq!(
            mission_rel("---", "!!!"),
            std::path::Path::new(".velocity/missions/rewrite")
        );
    }

    #[test]
    fn repo_rewrite_requires_url_and_language() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(repo_rewrite(tmp.path(), &serde_json::json!({"target_language": "rust"})).is_err());
        assert!(repo_rewrite(
            tmp.path(),
            &serde_json::json!({"url": "https://github.com/a/b.git"})
        )
        .is_err());
    }

    #[test]
    fn classify_intent_tool_requires_prompt() {
        assert!(classify_intent_tool(&serde_json::json!({})).is_err());
    }

    #[test]
    fn classify_intent_tool_returns_a_team_verdict() {
        std::env::remove_var("VELOCITY_DECISION_BACKEND");
        let out = classify_intent_tool(&serde_json::json!({
            "prompt": "clone https://github.com/BerriAI/litellm.git and rewrite the entire codebase in rust"
        }))
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["scope"], serde_json::json!("team"));
        assert_eq!(v["backend"], serde_json::json!("heuristic"));
        assert_eq!(v["target_language"], serde_json::json!("rust"));
        assert_eq!(
            v["repo_url"],
            serde_json::json!("https://github.com/BerriAI/litellm.git")
        );
        assert!(v["roles"].as_array().unwrap().len() >= 2);
    }

    // ---- select_model (model-selection decision matrix) ----------------- //

    #[test]
    fn select_model_tool_requires_task() {
        std::env::set_var("VELOCITY_DECISION_BACKEND", "none");
        let tmp = tempfile::tempdir().unwrap();
        assert!(select_model_tool(tmp.path(), &serde_json::json!({})).is_err());
    }

    /// What `select_model` owes the caller on whatever provider state the machine
    /// happens to have: it either picks, or it says none are usable.
    ///
    /// The first form of this test scanned all eleven catalogs through
    /// `usable_catalog_for` and branched on that answer before calling the tool —
    /// a second, cache-backed scan of the same providers, so one fetch that timed
    /// out under full-suite load flipped the branch and the test `unwrap_err()`ed
    /// a success (it failed inside `cargo test --lib` and passed alone, twice
    /// misread as a broken build). It also asserted `backend == "heuristic"`,
    /// which `agent/decision`'s tests rewrite from another thread through the
    /// process-global `VELOCITY_DECISION_BACKEND`. The branch now follows the call
    /// it judges, so no timing can make the two disagree.
    #[test]
    fn select_model_tool_either_picks_a_model_or_says_none_are_usable() {
        std::env::set_var("VELOCITY_DECISION_BACKEND", "none");
        let tmp = tempfile::tempdir().unwrap();
        let args = serde_json::json!({ "task": "refactor a coupled module" });
        match select_model_tool(tmp.path(), &args) {
            // Configured machine: a pick must stay in-catalog, never a bare
            // success with an empty choice for the caller to act on.
            Ok(out) => {
                let v: serde_json::Value = serde_json::from_str(&out).unwrap();
                let picked = v["selected"]["model_id"].as_str().unwrap_or_default();
                assert!(!picked.is_empty(), "a success must name a model: {out}");
            }
            // Nothing enabled (CI): refuse, and say why, without a partial pick.
            Err(err) => {
                let msg = err.to_string();
                assert!(
                    msg.contains("no usable models"),
                    "the refusal must say why: {msg}"
                );
            }
        }
    }

    #[test]
    #[ignore] // needs at least one real provider key enabled in Settings
    fn select_model_tool_live_picks_only_from_enabled_catalogs() {
        // Force the deterministic backend only when the caller didn't wire up
        // a System-1 model; with VELOCITY_DECISION_BACKEND=laya (+ endpoint)
        // exported, this exercises the full Laya path against live catalogs.
        if std::env::var("VELOCITY_DECISION_BACKEND")
            .unwrap_or_default()
            .is_empty()
        {
            std::env::set_var("VELOCITY_DECISION_BACKEND", "none");
        }
        let tmp = tempfile::tempdir().unwrap();
        println!(
            "SELECT_MODEL_LIVE resolved_backend={}",
            crate::agent::decision::resolve_backend().name()
        );
        let report: Vec<(AiProvider, usize)> = SCANNABLE_PROVIDERS
            .iter()
            .filter_map(|p| usable_catalog_for(tmp.path(), *p).map(|m| (*p, m.len())))
            .collect();
        if std::env::var("SELECT_MODEL_PROBE").is_ok() {
            if let Some(model) = crate::agent::decision::resolve_backend().model() {
                if let Some((p, ms)) = report
                    .iter()
                    .map(|(p, _)| (*p, usable_catalog_for(tmp.path(), *p).unwrap()))
                    .find(|(_, ms)| ms.len() >= 3)
                {
                    let ids: Vec<String> = ms.iter().map(|m| m.id.clone()).collect();
                    let descs: Vec<String> = ms
                        .iter()
                        .map(|m| {
                            format!(
                                "{}{}",
                                m.label,
                                if m.supports_tools { "; tools" } else { "" }
                            )
                        })
                        .collect();
                    let qs = [crate::agent::decision::Question::Choice {
                        id: "model".to_string(),
                        prompt: "Which model fits the task best?".to_string(),
                        choices: ids,
                        descriptions: descs,
                    }];
                    let st = crate::agent::decision::DecisionState::text(
                        "refactor a tightly coupled module across services",
                    );
                    match model.decide(&st, &qs) {
                        Ok(out) => {
                            for (_, a) in out.answers {
                                println!("SELECT_MODEL_LIVE raw probe ({p:?}): {a:?}");
                            }
                        }
                        Err(e) => println!("SELECT_MODEL_LIVE probe error: {e:?}"),
                    }
                }
            }
        }
        if report.is_empty() {
            println!("SELECT_MODEL_LIVE skipped: no enabled provider keys");
            return;
        }
        let out = select_model_tool(
            tmp.path(),
            &serde_json::json!({ "task": "refactor a tightly coupled module across services" }),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let chosen = v["selected"]["model_id"].as_str().unwrap();
        println!(
            "SELECT_MODEL_LIVE backend={} id={chosen} conf={} rationale={}",
            v["backend"], v["confidence"], v["rationale"]
        );
        // The hard guarantee: the pick must come from a catalog that was
        // actually fetched from an enabled provider API.
        let in_pool = report.iter().any(|(p, _)| {
            usable_catalog_for(tmp.path(), *p)
                .map(|ms| ms.iter().any(|m| m.id == chosen))
                .unwrap_or(false)
        });
        assert!(in_pool, "selected {chosen} is not in any enabled catalog");
    }

    #[test]
    fn provision_team_from_intent_writes_a_persisted_team() {
        std::env::remove_var("VELOCITY_DECISION_BACKEND");
        let tmp = tempfile::tempdir().unwrap();
        let out = provision_team_from_intent(
            tmp.path(),
            &serde_json::json!({"prompt": "orchestrate a team to build the entire app end to end: api, frontend, database, tests"}),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let slug = v["team_slug"].as_str().unwrap();
        assert!(!slug.is_empty());
        let teams = crate::editor::expert_team::load_expert_teams(tmp.path());
        assert!(teams.iter().any(|t| t.slug() == slug));
    }

    #[test]
    fn provision_team_from_intent_rejects_single_agent_prompt() {
        std::env::remove_var("VELOCITY_DECISION_BACKEND");
        let tmp = tempfile::tempdir().unwrap();
        let res = provision_team_from_intent(
            tmp.path(),
            &serde_json::json!({"prompt": "what is the capital of france"}),
        );
        assert!(res.is_err());
    }

    #[test]
    fn workspace_active_model_reads_selected_provider_and_model() {
        let tmp = tempfile::tempdir().unwrap();
        let vel = tmp.path().join(".velocity");
        std::fs::create_dir_all(&vel).unwrap();
        // Windows editors often prepend a UTF-8 BOM — parsing must survive it.
        std::fs::write(
            vel.join("workspace-preferences.json"),
            "\u{feff}{\"selected_model\":\"qwen3-max-preview\",\"provider\":\"Alibaba Qwen\"}",
        )
        .unwrap();
        let (provider, model) = workspace_active_model(tmp.path()).unwrap();
        assert_eq!(provider, AiProvider::AlibabaQwen);
        assert_eq!(model, "qwen3-max-preview");

        // No preferences file → None, so callers fall through to defaults.
        let empty = tempfile::tempdir().unwrap();
        assert!(workspace_active_model(empty.path()).is_none());
    }

    /// E1 regression: the closing output-format block must follow the
    /// deliverable the caller named. A porting/design/docs task that ends with
    /// "produce a structured report" is told its text answer *is* the
    /// deliverable, so it never calls the write tool — the first live LiteLLM
    /// run ported zero sectors for exactly this reason.
    #[test]
    fn the_output_format_follows_the_deliverable_the_caller_named() {
        let artifact = output_format_block("artifact");
        assert!(
            artifact.contains("write tool"),
            "an artifact task must be told to write files: {artifact}"
        );
        assert!(
            !artifact.contains("structured report"),
            "an artifact task must not be told to produce a report: {artifact}"
        );

        let report = output_format_block("report");
        assert!(
            report.contains("structured report"),
            "a report task keeps the audit format: {report}"
        );
        assert!(
            report.contains("Findings"),
            "a report task must still demand file:line findings: {report}"
        );

        // An absent or unknown deliverable defaults to the report, so every
        // existing caller (chat, lead planning) is unchanged.
        assert_eq!(output_format_block(""), report);
        assert_eq!(output_format_block("nonsense"), report);
        // Case and surrounding whitespace are irrelevant.
        assert_eq!(output_format_block("  ARTIFACT "), artifact);
        assert_eq!(output_format_block("files"), artifact);
    }

    /// The loop's artifact gate must fire on exactly the deliverables whose
    /// prompt says "a response with no writes is a failed task". If these two
    /// classifications drift, either the model is promised a hard gate the loop
    /// does not enforce, or a report task is blocked for having no files.
    #[test]
    fn the_artifact_gate_matches_the_prompt_that_promises_it() {
        for d in [
            "artifact",
            "artifacts",
            "file",
            "files",
            "write",
            "  ARTIFACT ",
        ] {
            assert!(
                deliverable_requires_artifact(d),
                "'{d}' writes the no-writes-failed prompt, so the gate must arm"
            );
            assert!(
                output_format_block(d).contains("no writes is a failed task"),
                "'{d}' must carry the no-writes contract: {d}"
            );
        }
        for d in ["", "report", "nonsense", "audit"] {
            assert!(
                !deliverable_requires_artifact(d),
                "'{d}' is a report task and must never trip the artifact gate"
            );
        }
    }

    /// A dispatch that names a crate directory as its scope used to paste
    /// Windows' `Access is denied. (os error 5)` into the prompt as that
    /// "file"'s content (gateprobe5). The model has no way to tell a refused
    /// read from a broken workspace, and the listing it needed was one
    /// `read_dir` away.
    #[test]
    fn a_directory_scope_lists_the_files_it_contains_instead_of_an_access_error() {
        let tmp = tempfile::tempdir().unwrap();
        let crate_dir = tmp.path().join("rust_port");
        std::fs::create_dir_all(&crate_dir).unwrap();
        std::fs::write(crate_dir.join("Cargo.toml"), "[package]\nname = \"p\"\n").unwrap();
        std::fs::create_dir_all(crate_dir.join("src")).unwrap();
        std::fs::write(
            crate_dir.join("src").join("lib.rs"),
            "mod budget_manager;\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join("notes.md"), "# real file\n").unwrap();

        let files = vec!["rust_port".to_string(), "notes.md".to_string()];
        let blocks = preload_target_files(tmp.path(), &files);
        assert_eq!(blocks.len(), 2);
        let dir_block = &blocks[0];
        assert!(
            !dir_block.contains("Access is denied") && !dir_block.contains("Could not read"),
            "a directory is not a failed read: {dir_block}"
        );
        assert!(
            dir_block.contains("rust_port/Cargo.toml"),
            "the listing must name paths the agent can pass straight to read_file: {dir_block}"
        );
        // A nested directory is more scope, not a path the agent can open, so it
        // must not be offered as though it were one.
        assert!(
            !dir_block.contains("rust_port/src"),
            "a sub-directory is not a readable entry: {dir_block}"
        );
        // The file case keeps working, content and all.
        assert!(blocks[1].contains("# real file"), "{}", blocks[1]);
    }

    /// The truncation cut at a fixed BYTE index. A source file whose 12,000th
    /// byte lands inside a multi-byte character (a `—`, an accented identifier,
    /// a CJK comment — all ordinary in real code) panicked the dispatch thread
    /// instead of building a prompt, and the caller got a dead tool rather than
    /// a truncated file.
    #[test]
    fn preloading_a_large_non_ascii_file_cuts_at_a_character_boundary() {
        let tmp = tempfile::tempdir().unwrap();
        // 11,999 ASCII bytes, then a 3-byte '—' straddling the 12,000 boundary.
        let mut body = "a".repeat(11_999);
        body.push('—');
        body.push_str(&"b".repeat(4_000));
        std::fs::write(tmp.path().join("big.rs"), &body).unwrap();

        let blocks = preload_target_files(tmp.path(), &["big.rs".to_string()]);
        let one = &blocks[0];
        assert!(
            one.contains("[truncated, ") && one.contains("bytes total"),
            "a large file must say it was cut: {}",
            &one[..one.len().min(120)]
        );
        assert!(
            !one.contains("\u{fffd}"),
            "and must not contain a split character: {one}"
        );
        // No panic, and the ASCII tail before the cut survived intact.
        assert!(one.contains(&"a".repeat(4_000)));
    }

    #[test]
    fn fallback_chain_inherits_active_workspace_model() {
        let tmp = tempfile::tempdir().unwrap();
        let vel = tmp.path().join(".velocity");
        std::fs::create_dir_all(&vel).unwrap();
        std::fs::write(
            vel.join("workspace-preferences.json"),
            "{\"selected_model\":\"qwen2.5-coder:14b\",\"provider\":\"Local Ollama\"}",
        )
        .unwrap();
        let member: ExpertMember = serde_json::from_str("{}").unwrap();
        let chain = build_fallback_chain(&member, tmp.path());
        // Whatever else the chain contains, the LocalOllama entry must carry
        // the IDE-selected model — not the 0.5B static default that produced
        // silent garbage in the filesystem-proposal mission.
        let ollama = chain
            .iter()
            .find(|(p, _)| *p == AiProvider::LocalOllama)
            .expect("chain always ends with a LocalOllama entry");
        assert_eq!(ollama.1, "qwen2.5-coder:14b");
    }

    #[test]
    fn selected_provider_is_promoted_to_the_front_of_the_chain() {
        // The user picked Local Ollama in the IDE. Whatever provider the member
        // was provisioned with, the dispatch must attempt the selected provider
        // FIRST, so an expired cloud key can never consume the turn budget
        // before the local model is tried.
        let tmp = tempfile::tempdir().unwrap();
        let vel = tmp.path().join(".velocity");
        std::fs::create_dir_all(&vel).unwrap();
        std::fs::write(
            vel.join("workspace-preferences.json"),
            "{\"selected_model\":\"qwen2.5-coder:14b\",\"provider\":\"Local Ollama\"}",
        )
        .unwrap();
        let mut member: ExpertMember = serde_json::from_str("{}").unwrap();
        member.provider = AiProvider::AlibabaQwen;
        let chain = build_fallback_chain(&member, tmp.path());
        assert_eq!(
            chain.first().map(|(p, _)| *p),
            Some(AiProvider::LocalOllama),
            "selected provider must lead the chain, got {:?}",
            chain.iter().map(|(p, _)| p.slug()).collect::<Vec<_>>()
        );
        assert_eq!(chain[0].1, "qwen2.5-coder:14b");
        // The member's original (cloud) provider remains available as fallback.
        assert!(chain.iter().any(|(p, _)| *p == AiProvider::AlibabaQwen));
    }
}
