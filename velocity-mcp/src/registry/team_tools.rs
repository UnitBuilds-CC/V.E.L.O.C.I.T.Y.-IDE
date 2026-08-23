use serde_json::{json, Value};
use std::error::Error;
use std::path::Path;

use crate::agent::AiProvider;
use crate::editor::expert_team::{
    detect_scope_overlaps, load_expert_teams, save_expert_teams, slugify, validate_team_composition,
    ExpertMember, ExpertTeam, MemberUpdate, ValidationSeverity,
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
        "update_expert_team" => update_expert_team(root, arguments)?,
        "update_team_member" => update_team_member(root, arguments)?,
        "add_team_member" => add_team_member(root, arguments)?,
        "remove_team_member" => remove_team_member(root, arguments)?,
        "validate_team" => validate_team(root, arguments)?,
        "check_scope_overlaps" => check_scope_overlaps(root, arguments)?,
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

    if let Some(new_name) = arguments["name"].as_str().map(str::trim).filter(|s| !s.is_empty()) {
        if let Some((old_slug, new_slug)) = team.update_name(new_name) {
            changes.push(format!("name → \"{}\" (slug: {} → {})", new_name, old_slug, new_slug));
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
        skills: arguments["skills"].as_array().map(|_| string_array(&arguments["skills"])),
        scope_patterns: arguments["scope_patterns"]
            .as_array()
            .map(|_| string_array(&arguments["scope_patterns"])),
        tools: arguments["tools"].as_array().map(|_| string_array(&arguments["tools"])),
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
        member.name,
        member.id,
        team_name,
        member_count
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
        removed.name,
        removed.id,
        team_name,
        member_count
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
        .find(|t| t.id.to_lowercase() == lower || t.slug() == slug || t.name.to_lowercase() == lower)
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
        .find(|t| t.id.to_lowercase() == lower || t.slug() == slug || t.name.to_lowercase() == lower)
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
