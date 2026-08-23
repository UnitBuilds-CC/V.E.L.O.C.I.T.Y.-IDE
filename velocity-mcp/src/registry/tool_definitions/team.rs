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
    ]
}
