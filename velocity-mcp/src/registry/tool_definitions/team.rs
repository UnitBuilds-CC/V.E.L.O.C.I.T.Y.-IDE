use crate::registry::types::Tool;
use serde_json::json;

pub fn get_team_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "create_expert_team".to_string(),
            description: "Create or update a mixture-of-experts team and persist it to .velocity/expert_teams.nda. Each member is assigned an AI provider/model, a role, optional skills (loadable .nda skill files), file scope patterns, and workflow instructions. Tasks can later be routed to the team via '@<slug>' or natural language like 'send it to the <name> team'. A team with a matching id/slug is replaced.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Human-readable team name, e.g. \"Rust Backend Team\". Used to derive the routing slug." },
                    "description": { "type": "string", "description": "Short description of the team's purpose." },
                    "members": {
                        "type": "array",
                        "description": "The team roster. At least one member is required; the first member is treated as the team lead.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string", "description": "Member display name, e.g. \"Backend Engineer\"." },
                                "role": { "type": "string", "description": "Specialty/role title, e.g. \"API & Services\"." },
                                "provider": { "type": "string", "description": "Provider slug: cloudflare, openrouter, azure, ollama, openai, anthropic, or vertex. Defaults to cloudflare." },
                                "model_id": { "type": "string", "description": "Model identifier for the provider (e.g. \"anthropic/claude-3.5-sonnet\"). Empty uses the session default." },
                                "skills": { "type": "array", "items": { "type": "string" }, "description": "Skill ids to inject into this member's prompt (e.g. \"system_tools\" or a custom skill file id)." },
                                "scope_patterns": { "type": "array", "items": { "type": "string" }, "description": "File paths/areas this member owns (e.g. \"src/api/\", \"*.sql\"). Used for scope-based routing." },
                                "tools": { "type": "array", "items": { "type": "string" }, "description": "Optional allow-list of tool names this member may call. Empty means all tools." },
                                "workflow_instructions": { "type": "string", "description": "Operating instructions appended to this member's system persona." }
                            },
                            "required": ["name", "role"]
                        }
                    }
                },
                "required": ["name", "members"]
            }),
        },
        Tool {
            name: "create_skill_file".to_string(),
            description: "Create or overwrite a reusable skill definition saved as .velocity/skills/<id>.nda. A skill body is injected into the system prompt of any team member that lists the skill id, specializing that member's behavior.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Stable skill id/slug, e.g. \"netcode\". Used as the file name and referenced from member skills." },
                    "name": { "type": "string", "description": "Human-readable skill name. Defaults to the id." },
                    "description": { "type": "string", "description": "Short description of what the skill provides." },
                    "body": { "type": "string", "description": "The specialized instructions/knowledge injected into the member's prompt." }
                },
                "required": ["id", "body"]
            }),
        },
        Tool {
            name: "list_expert_teams".to_string(),
            description: "List the expert teams currently persisted for the workspace, including each team's slug and members. Use this before creating a team to avoid duplicating an existing one.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "list_skills".to_string(),
            description: "List the reusable skill files persisted under .velocity/skills (id, name, description). Use this before creating a skill to avoid duplicating one, or to discover skill ids to attach to a team member.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        // ── Edit / Update Tools ──────────────────────────────────────────
        Tool {
            name: "update_expert_team".to_string(),
            description: "Update an existing expert team's name and/or description. Cannot edit preset teams (clone first). The team id is regenerated from the new name.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "team_id": { "type": "string", "description": "Team id, slug, or name to identify the team." },
                    "name": { "type": "string", "description": "New team name. Updates the slug and id." },
                    "description": { "type": "string", "description": "New team description." }
                },
                "required": ["team_id"]
            }),
        },
        Tool {
            name: "update_team_member".to_string(),
            description: "Apply a partial update to a team member. Only the fields you provide are changed; all other fields remain unchanged. Cannot edit preset team members.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "team_id": { "type": "string", "description": "Team id, slug, or name." },
                    "member_id": { "type": "string", "description": "The member's id to update." },
                    "name": { "type": "string", "description": "New display name." },
                    "role": { "type": "string", "description": "New role/specialty." },
                    "provider": { "type": "string", "description": "New provider slug (cloudflare, openrouter, azure, ollama, etc.)." },
                    "model_id": { "type": "string", "description": "New model identifier." },
                    "skills": { "type": "array", "items": { "type": "string" }, "description": "Replace the skills list." },
                    "scope_patterns": { "type": "array", "items": { "type": "string" }, "description": "Replace the scope patterns." },
                    "tools": { "type": "array", "items": { "type": "string" }, "description": "Replace the tool allow-list." },
                    "workflow_instructions": { "type": "string", "description": "Replace the workflow instructions." }
                },
                "required": ["team_id", "member_id"]
            }),
        },
        Tool {
            name: "add_team_member".to_string(),
            description: "Add a new member to an existing expert team. The member object follows the same schema as create_expert_team members. Cannot add to preset teams.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "team_id": { "type": "string", "description": "Team id, slug, or name." },
                    "member": {
                        "type": "object",
                        "description": "The new member specification.",
                        "properties": {
                            "name": { "type": "string" },
                            "role": { "type": "string" },
                            "provider": { "type": "string" },
                            "model_id": { "type": "string" },
                            "skills": { "type": "array", "items": { "type": "string" } },
                            "scope_patterns": { "type": "array", "items": { "type": "string" } },
                            "tools": { "type": "array", "items": { "type": "string" } },
                            "workflow_instructions": { "type": "string" }
                        },
                        "required": ["name", "role"]
                    }
                },
                "required": ["team_id", "member"]
            }),
        },
        Tool {
            name: "remove_team_member".to_string(),
            description: "Remove a member from an expert team by member id. Cannot remove from preset teams.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "team_id": { "type": "string", "description": "Team id, slug, or name." },
                    "member_id": { "type": "string", "description": "The id of the member to remove." }
                },
                "required": ["team_id", "member_id"]
            }),
        },
        // ── Validation Tools ─────────────────────────────────────────────
        Tool {
            name: "validate_team".to_string(),
            description: "Run composition validation on a team. Checks for: empty teams, team lead scope coverage, duplicate names/ids, empty fields, default model usage, and scope overlaps. Returns errors, warnings, info, and a health score (0-100).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "team_id": { "type": "string", "description": "Team id, slug, or name to validate." }
                },
                "required": ["team_id"]
            }),
        },
        Tool {
            name: "check_scope_overlaps".to_string(),
            description: "Detect scope pattern overlaps between members of a team. Overlaps occur when one member's scope pattern contains another's (e.g. 'src/' and 'src/api/'). Returns detailed overlap pairs.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "team_id": { "type": "string", "description": "Team id, slug, or name to check." }
                },
                "required": ["team_id"]
            }),
        },
        // ── Clone / Import / Export Tools ────────────────────────────────
        Tool {
            name: "clone_expert_team".to_string(),
            description: "Clone an existing expert team (including preset teams) with a new name. All member configurations are preserved. The clone is a non-preset team that can be edited. Use this to customize a preset team.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "team_id": { "type": "string", "description": "Team id, slug, or name to clone." },
                    "new_name": { "type": "string", "description": "Name for the cloned team. Must produce a unique slug." }
                },
                "required": ["team_id", "new_name"]
            }),
        },
        Tool {
            name: "export_expert_team".to_string(),
            description: "Export an expert team to JSON format for sharing, backup, or version control. The JSON can be re-imported with import_expert_team.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "team_id": { "type": "string", "description": "Team id, slug, or name to export." }
                },
                "required": ["team_id"]
            }),
        },
        Tool {
            name: "import_expert_team".to_string(),
            description: "Import an expert team from JSON format (as produced by export_expert_team). If a team with the same slug exists, it is replaced; otherwise the team is appended. Member ids are regenerated if empty.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "json": { "type": "string", "description": "The JSON string representing the team to import." }
                },
                "required": ["json"]
            }),
        },
        // ── Routing Debug / Analytics Tools ──────────────────────────────
        Tool {
            name: "debug_routing".to_string(),
            description: "Debug the routing decision for a task without actually routing it. Shows which routing stage matched (file_scope_match, keyword_match, fallback_to_lead), the selected member, and keyword scores for all members. Useful for understanding why a task was routed to a particular member.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "team_id": { "type": "string", "description": "Team id, slug, or name." },
                    "task": { "type": "string", "description": "The task description to route." },
                    "files": { "type": "array", "items": { "type": "string" }, "description": "Optional file paths for scope-based routing." }
                },
                "required": ["team_id", "task"]
            }),
        },
        Tool {
            name: "team_analytics".to_string(),
            description: "Show analytics and statistics for a team: member count, provider distribution, scope coverage, skills/tools per member. Useful for understanding team composition and identifying gaps.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "team_id": { "type": "string", "description": "Team id, slug, or name." }
                },
                "required": ["team_id"]
            }),
        },
        // ── Health Check / Provider Tools ───────────────────────────────
        Tool {
            name: "team_health_check".to_string(),
            description: "Comprehensive health check combining validation, scope overlaps, and analytics. Returns a health score (0-100), status (excellent/good/fair/poor), error/warning details, and recommendations.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "team_id": { "type": "string", "description": "Team id, slug, or name." }
                },
                "required": ["team_id"]
            }),
        },
        Tool {
            name: "list_providers".to_string(),
            description: "List all available AI providers with their slugs, labels, and aliases. Use this to discover valid provider values for team members.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        // ── Quick-Create / Bulk Import Tools ─────────────────────────────
        Tool {
            name: "create_team_quick".to_string(),
            description: "Quick-create a team with minimal input. Provide a team name and member names (array or comma-separated string). Members are created with sensible defaults (Cloudflare provider, Team Lead role for first member, Specialist for others). Customize later with update_team_member.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Team name." },
                    "description": { "type": "string", "description": "Optional team description." },
                    "members": {
                        "oneOf": [
                            { "type": "array", "items": { "type": "string" }, "description": "Array of member names." },
                            { "type": "string", "description": "Comma-separated member names." }
                        ],
                        "description": "Member names (array or comma-separated string)."
                    }
                },
                "required": ["name", "members"]
            }),
        },
        Tool {
            name: "bulk_import_members".to_string(),
            description: "Bulk import multiple members to an existing team at once. Each member object follows the same schema as create_expert_team members. More efficient than calling add_team_member multiple times.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "team_id": { "type": "string", "description": "Team id, slug, or name." },
                    "members": {
                        "type": "array",
                        "description": "Array of member specs to add.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string" },
                                "role": { "type": "string" },
                                "provider": { "type": "string" },
                                "model_id": { "type": "string" },
                                "skills": { "type": "array", "items": { "type": "string" } },
                                "scope_patterns": { "type": "array", "items": { "type": "string" } },
                                "tools": { "type": "array", "items": { "type": "string" } },
                                "workflow_instructions": { "type": "string" }
                            },
                            "required": ["name", "role"]
                        }
                    }
                },
                "required": ["team_id", "members"]
            }),
        },
        // ── Version Control Tools ───────────────────────────────────────
        Tool {
            name: "team_changelog".to_string(),
            description: "Generate a versioned snapshot of a team for change tracking. Returns a snapshot hash, timestamp, and current team state. Compare hashes to detect changes over time.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "team_id": { "type": "string", "description": "Team id, slug, or name." }
                },
                "required": ["team_id"]
            }),
        },
        // ── Team Execution Tools ────────────────────────────────────────
        Tool {
            name: "team_dispatch".to_string(),
            description: "Dispatch a task to the best-matching team member for autonomous execution. Routes the task using file-scope matching, keyword scoring, or fallback to team lead. Builds a persona prompt from the member's role, skills, and workflow instructions, then executes via a headless sub-agent using the member's configured AI provider and model. Returns the routing decision, transcript, and status updates. This is the bridge between team organization and autonomous execution.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "team_id": { "type": "string", "description": "Team id, slug, or name." },
                    "task": { "type": "string", "description": "The task description to execute." },
                    "files": { "type": "array", "items": { "type": "string" }, "description": "Optional file paths for scope-based routing and pre-indexing." },
                    "deliverable": { "type": "string", "enum": ["report", "artifact"], "description": "What the task owes. 'report' (default) ends the prompt with a structured-report format, right for audits and analysis. 'artifact' tells the agent its deliverable is files it must write with the write tool (porting, design, docs, remediation) and that a text answer with no writes is a failed task. Sending the wrong one makes the agent emit prose when it should have written a file, and report success doing it." },
                    "budget_secs": { "type": "integer", "description": "Wall-clock seconds this dispatch may run for. Omit to use the process default (VELOCITY_DISPATCH_BUDGET_SECS, 3600s). Set 0 for no ceiling at all: the run then ends only on the loop's own signals — a provider that stops streaming for the stall window, a repeated/no-action structural loop, or a deliverable that passes the compile and test gates. Use it for a task whose single generation is legitimately long (a from-scratch port write turn measured 1999s on a shared GPU), because a clock that cuts off alive, merely slow work reports success as failure." },
                    "max_wall_secs": { "type": "integer", "description": "Long-horizon team runs only: hard cap on wall-clock seconds the orchestrator spends dispatching this plan. 0 (default) is uncapped, matching run_rewrite_mission. A run that hits the cap checkpoints its progress instead of losing it. `budget_secs` is honoured as the same cap when this is absent." },
                    "resume": { "type": "boolean", "description": "Long-horizon team runs only: when true and a checkpoint for this team+prompt exists at .velocity/orchestrator/checkpoint.json, the orchestrator replays finished work and continues rather than starting the plan fresh. Default false." },
                    "background": { "type": "boolean", "description": "Long-horizon team runs only: when true, the orchestrator run is handed to a driver thread and this call returns immediately with a run_id while the workers keep executing. Use it to start a run you intend to steer mid-flight, because the serial stdio server cannot deliver amend_running_team to a blocking dispatch. Default false (synchronous run, unchanged behaviour)." }
                },
                "required": ["team_id", "task"]
            }),
        },
        Tool {
            name: "amend_running_team".to_string(),
            description: "Steer a live-VC team run that was started with team_dispatch background:true while it is still executing. Runs the mid-run change (the 'delta') through the intent brain, pushes a broadcast awareness directive into the run's shared directive inbox (so every in-flight and pending worker learns the direction changed) plus one scope-tagged directive per classified role (so each worker only sees the part of the change that falls in its own file-scope domain), and — when the delta names a scope no existing seat covers — provisions a fresh team so that domain gets a specialist. The driver thread drains the inbox on its next wave and records every hand-off in a delivery ledger the observability tools read. Requires a run_id or a team slug; a finished run cannot be amended.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "run_id": { "type": "string", "description": "The run_id returned by a team_dispatch background:true call. Takes precedence over 'team'." },
                    "team": { "type": "string", "description": "Team slug, @slug, or id used as a fallback selector when run_id is omitted; resolves to the newest still-running dispatch for that team." },
                    "delta": { "type": "string", "description": "The mid-run change the user added or redirected to, e.g. \"also add a rate-limit retry to the http client and a React dashboard page\"." }
                },
                "required": ["delta"]
            }),
        },
        Tool {
            name: "list_live_runs".to_string(),
            description: "P5 observability: list every live-VC team run the running MCP process currently knows about — the background: true dispatches that are still executing or have just finished. Each row shows the run_id, team, trigger, whether it is still running, the current wave, elapsed seconds and any directives still queued. Use it to find a run_id to feed to inspect_live_run or read_directive_inbox. Read-only.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
            }),
        },
        Tool {
            name: "inspect_live_run".to_string(),
            description: "P5 observability: read one live-VC run's task graph, in-flight assignments and per-worker statuses from the snapshot the driver thread publishes every wave, plus the run lifecycle (running, or finished with succeeded/failed/blocked counts and wall seconds). This is the primary tool for watching the parallel pipeline work end to end. Supply a run_id, or a team slug/id to resolve that team's newest still-running dispatch. Read-only.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "run_id": { "type": "string", "description": "The run_id from team_dispatch background:true or list_live_runs. Takes precedence over 'team'." },
                    "team": { "type": "string", "description": "Team slug, @slug, or id fallback selector when run_id is omitted; resolves to that team's newest running dispatch." }
                }
            }),
        },
        Tool {
            name: "read_directive_inbox".to_string(),
            description: "P5 observability: read a live-VC run's shared directive inbox and delivery ledger. Lists every broadcast and scope-tagged directive pushed by amend_running_team, and the ledger of which directive was handed to which in-flight assignment by the driver thread. The ledger is the evidence that an amendment actually reached every in-flight worker rather than merely queueing a note. Supply a run_id or team selector. Read-only.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "run_id": { "type": "string", "description": "The run_id from team_dispatch background:true or list_live_runs. Takes precedence over 'team'." },
                    "team": { "type": "string", "description": "Team slug, @slug, or id fallback selector when run_id is omitted." }
                }
            }),
        },
        // ── Intent / Decision Routing Tools ────────────────────────────
        Tool {
            name: "classify_intent".to_string(),
            description: "Run the intent decision brain on a request and return the routing verdict as JSON: whether it should be handled by a single agent or a specialized team, whether a repository must be cloned first, the detected target language, the specialist roles implied, a confidence value, and the fired signals. Uses the configured System-1 decision model (on-device Laya or hosted Jev) when one is set up, and falls back to the deterministic scorer otherwise. Read-only: it classifies and explains, and does NOT provision any team or clone any repo.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "prompt": { "type": "string", "description": "The user request to classify, e.g. \"clone liteLLM and rewrite it in rust\"." }
                },
                "required": ["prompt"]
            }),
        },
        Tool {
            name: "select_model".to_string(),
            description: "Pick the best AI model for a task from the models that are ACTUALLY usable: live provider catalogs fetched with the API keys enabled in provider-settings (e.g. the Alibaba Qwen token-plan listing), never a fabricated model id. Optionally scope the candidate pool to one provider slug. Runs the configured System-1 decision model (on-device Laya or hosted Jev) over the candidate set and adopts its pick only when the chosen option is in the usable set and stands clearly above the rest; otherwise falls back to the deterministic capability ranking (task needs vs tool/thinking/context support). The result explains which backend decided, the confidence, the option probability, the rationale, and the ranked runners-up. Read-only: it fetches catalogs and selects, it never switches the active model or dispatches work.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task": { "type": "string", "description": "Description of the task to pick a model for." },
                    "provider": { "type": "string", "description": "Optional provider slug (e.g. \"alibaba\", \"openai\") to scope the pool; all key-enabled providers are scanned when omitted." },
                    "task_kind": { "type": "string", "enum": ["refactor", "bug_fix", "test", "documentation", "analysis", "planning", "merge", "desktop_automation"], "description": "Optional task kind for capability scoring; inferred from the task text when omitted." }
                },
                "required": ["task"]
            }),
        },
        Tool {
            name: "provision_team_from_intent".to_string(),
            description: "Classify a request with the intent brain and, when it verdicts a team, author and persist the implied ExpertTeam (lead + specialists with skills and file-scope patterns) to .velocity/expert_teams.nda. Idempotent on the team slug (re-running replaces the roster). Returns the team slug and the full decision. Does not clone a repository or start execution.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "prompt": { "type": "string", "description": "The user request whose implied team should be provisioned." },
                    "team_name": { "type": "string", "description": "Optional team name; a stable default is used when omitted." }
                },
                "required": ["prompt"]
            }),
        },
        Tool {
            name: "repo_rewrite".to_string(),
            description: "Orchestrate a 'clone X and rewrite it in Y' request end to end: safely git-clone the repository into .velocity/clones, analyze it deterministically (dominant languages, file count, layout), decompose it into dependency-ordered sectors small enough for one dispatch to own (oversized files pre-chunked into line ranges, generated/vendored files excluded and reported), provision the specialist team the intent brain implies for a long-horizon port (including security, durability and performance sweep seats), and emit a 12-phase rewrite plan grounded in the measured arithmetic. Also reports a feasibility verdict in model-hours and wall-hours. It does NOT perform the rewrite itself: use run_rewrite_mission to execute, so the user keeps control over model spend.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "Git repository URL to clone (http/https/git scheme)." },
                    "target_language": { "type": "string", "description": "Language to rewrite the repository into, e.g. \"rust\"." },
                    "team_name": { "type": "string", "description": "Optional team name; defaults to 'Repo Rewrite Team'." },
                    "depth": { "type": "integer", "description": "Optional shallow-clone depth (e.g. 1)." },
                    "dispatch": { "type": "boolean", "description": "When true, dispatch the mapping phase to the team lead for a planning pass. Default false." },
                    "parallelism": { "type": "integer", "description": "Concurrent workers the wall-clock estimate assumes (1-32). Default 8, matching the orchestrator's concurrency gate." },
                    "include_tests": { "type": "boolean", "description": "Include the source test suite in the sector decomposition. Default false; test parity is its own phase." },
                    "source_root": { "type": "string", "description": "Optional package root to scope the decomposition to, relative to the repo root (e.g. \"litellm\", \"src/packaging\"). When omitted it is detected, and detection refuses to adopt a root that would leave more than a quarter of the collected source outside it — such a monorepo is planned whole rather than silently narrowed. The response reports sectors.root_coverage, sectors.outside_root_* and sectors.root_declined so the scope actually in force is always visible; read them before trusting the effort estimate." }
                },
                "required": ["url", "target_language"]
            }),
        },
        Tool {
            name: "run_rewrite_mission".to_string(),
            description: "Execute or resume the long-horizon rewrite mission that repo_rewrite planned. Reads mission.json and sector_plan.json from the mission directory, then runs the plan end to end without a human in the loop: the conventions/design pass, the porting waves in parallel (workers pull from a shared queue, one dispatch per sector unit, oversized files arriving as line-range chunks), the build/fmt/clippy/test gates against the output tree, the security + durability + performance sweeps concurrently over batches of ported code, remediation of every finding with the gates re-run afterwards, a behavioural parity review, and documentation with the benchmark numbers. Progress is checkpointed after every sector, so a cap, crash or reboot resumes instead of restarting. Requires an explicit authorization to spend: unattended=true, or dry_run=true to see the schedule, the seats and the caps for free. The report states what was NOT done - missing seats, truncated sweep coverage, failed gates, degraded small-model dispatches - rather than only what succeeded.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "mission_dir": { "type": "string", "description": "Mission directory from repo_rewrite's mission.dir, e.g. '.velocity/missions/litellm-rust'. Absolute paths are accepted." },
                    "unattended": { "type": "boolean", "description": "REQUIRED to dispatch. Authorises running the mission without a human reviewing each result. Set it only when the user's request actually asked for an unattended run; dry_run=true is the free alternative." },
                    "dry_run": { "type": "boolean", "description": "Plan and validate without dispatching a single model call or writing progress: resolves every seat, lists the caps, and reports what would run. Default false." },
                    "team_id": { "type": "string", "description": "Override the team recorded in mission.json." },
                    "parallelism": { "type": "integer", "description": "Concurrent sector dispatches (1-32). 0 uses the manifest's value, default 8." },
                    "max_dispatches": { "type": "integer", "description": "Hard cap on dispatches THIS RUN may make. 0 (default) is uncapped; a paused run resumes with a fresh cap." },
                    "max_wall_secs": { "type": "integer", "description": "Hard cap on wall-clock seconds spent dispatching this run. 0 (default) is uncapped." },
                    "max_waves": { "type": "integer", "description": "Stop after this many dependency waves. 0 (default) runs all of them." },
                    "max_attempts": { "type": "integer", "description": "Retries per sector across the whole mission, including prior runs. Default 2." },
                    "run_gates": { "type": "boolean", "description": "Run build/fmt/clippy/test/bench against the output tree. Default true." },
                    "stop_on_gate_failure": { "type": "boolean", "description": "Halt at the first failing gate instead of recording it and continuing. Default false, so one sector that will not compile does not strand the rest." },
                    "run_sweeps": { "type": "boolean", "description": "Run the security, durability and performance sweeps. Default true." },
                    "sweep_batch_sectors": { "type": "integer", "description": "Ported sectors audited per sweep dispatch. Default 6; one dispatch per sector would triple the porting cost." },
                    "max_sweep_dispatches": { "type": "integer", "description": "Per-sweep dispatch cap, bounding audit spend separately from porting spend. Default 20. Coverage is reported when this truncates a sweep." },
                    "run_documentation": { "type": "boolean", "description": "Run the documentation + benchmark-report phase. Default true." },
                    "gate_timeout_secs": { "type": "integer", "description": "Per-gate wall-clock ceiling. Default 900; a hung cargo test would otherwise hang the mission forever." }
                },
                "required": ["mission_dir"]
            }),
        },
        // ── Wiki Generation Tools ──────────────────────────────────────
        Tool {
            name: "generate_wiki".to_string(),
            description: "Generate a project wiki from the workspace site map with indexed analysis, PageRank reading order, and content caching. Supports markdown, HTML, and GitHub Pages export formats. Uses deterministic pre-indexing (zero LLM calls) for symbol/import extraction, with optional LLM-powered detail generation. Incremental updates skip unchanged files via content-keyed cache.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "output_dir": { "type": "string", "description": "Output directory for wiki files (relative to workspace root or absolute). Default: 'wiki'." },
                    "format": { "type": "string", "enum": ["markdown", "html", "github-pages"], "description": "Export format. Default: 'markdown'." },
                    "use_index": { "type": "boolean", "description": "Enable pre-indexing phase for symbol/import extraction. Default: true." }
                }
            }),
        },
    ]
}
