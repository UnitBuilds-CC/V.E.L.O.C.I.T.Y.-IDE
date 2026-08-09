use serde_json::{json, Value};
use std::error::Error;
use std::path::Path;

use crate::agent::AiProvider;
use crate::editor::expert_team::{
    load_expert_teams, save_expert_teams, slugify, ExpertMember, ExpertTeam,
};
use crate::editor::skill_file::{list_skill_files, save_skill_file, SkillFile};

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
        _ => return Ok(None),
    };
    Ok(Some(result))
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
