use serde_json::{json, Value};
use std::error::Error;
use std::path::Path;

use crate::agent::AiProvider;
use crate::editor::expert_team::{
    detect_scope_overlaps, export_team_to_json, import_team_from_json, load_expert_teams,
    save_expert_teams, slugify, validate_team_composition, ExpertMember, ExpertTeam, MemberUpdate,
    ValidationSeverity,
};
use crate::editor::team_router::debug_routing;
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
        "clone_expert_team" => clone_expert_team(root, arguments)?,
        "export_expert_team" => export_expert_team(root, arguments)?,
        "import_expert_team" => import_expert_team(root, arguments)?,
        "debug_routing" => debug_routing_tool(root, arguments)?,
        "team_analytics" => team_analytics(root, arguments)?,
        "team_health_check" => team_health_check(root, arguments)?,
        "list_providers" => list_providers(root, arguments)?,
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
        .find(|t| t.id.to_lowercase() == lower || t.slug() == slug || t.name.to_lowercase() == lower)
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
        .find(|t| t.id.to_lowercase() == lower || t.slug() == slug || t.name.to_lowercase() == lower)
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
        .find(|t| t.id.to_lowercase() == lower || t.slug() == slug || t.name.to_lowercase() == lower)
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
        .find(|t| t.id.to_lowercase() == lower || t.slug() == slug || t.name.to_lowercase() == lower)
        .ok_or_else(|| format!("team '{}' not found", team_ref))?;

    // Compute basic analytics
    let member_count = team.members.len();
    let total_skills: usize = team.members.iter().map(|m| m.skills.len()).sum();
    let total_scopes: usize = team.members.iter().map(|m| m.scope_patterns.len()).sum();
    let total_tools: usize = team.members.iter().map(|m| m.tools.len()).sum();

    // Provider distribution
    let mut provider_counts = std::collections::HashMap::new();
    for member in &team.members {
        *provider_counts.entry(member.provider.slug().to_string()).or_insert(0) += 1;
    }

    // Scope coverage analysis
    let members_with_scopes = team.members.iter().filter(|m| !m.scope_patterns.is_empty()).count();
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
            "coverage_percent": if member_count > 0 {
                (members_with_scopes * 100) / member_count
            } else {
                0
            },
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
        .find(|t| t.id.to_lowercase() == lower || t.slug() == slug || t.name.to_lowercase() == lower)
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
