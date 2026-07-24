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
    ]
}
