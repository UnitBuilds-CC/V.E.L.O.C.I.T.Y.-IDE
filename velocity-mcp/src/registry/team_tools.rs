use serde_json::{json, Value};
use std::error::Error;
use std::path::Path;

use crate::agent::AiProvider;
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
        "generate_wiki" => generate_wiki(root, arguments)?,
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

/// Pick the most agentic-capable model currently pulled on the local Ollama
/// server, preferring coder-tuned ones. The static default
/// (`qwen2.5-coder:0.5b`) reliably produces confident garbage on agent
/// tasks, so it must not be the automatic choice when something bigger is
/// installed.
fn best_local_ollama_model() -> Option<String> {
    let models = crate::agent::provider::fetch_local_ollama_models(&[]).ok()?;
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

/// Returns alternative models to try for a given provider when the primary model fails.
/// Ordered by likelihood of success (free/cheap first, then larger models).
fn fallback_models_for(provider: AiProvider, primary: &str) -> Vec<String> {
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

/// Check if an error means the provider itself is unavailable (no accounts/keys)
/// vs a model-specific failure (quota, rate limit, model unavailable).
pub(crate) fn is_provider_unavailable(status_updates: &[String], transcript: &str) -> bool {
    transcript.contains("No Cloudflare accounts")
        || transcript.contains("No OpenRouter accounts")
        || transcript.contains("No Alibaba")
        || status_updates
            .iter()
            .any(|s| s.contains("No ") && s.contains("accounts configured"))
        || status_updates.iter().any(|s| s.contains("missing"))
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
        let (_, model) = member.resolve_effective_provider_and_model(
            default_configured_provider(root),
            &default_model,
        );
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
            _ => best_local_ollama_model().unwrap_or_else(|| {
                crate::agent::provider::default_provider_model(AiProvider::LocalOllama)
            }),
        };
        chain.push((AiProvider::LocalOllama, model));
    }

    chain
}

/// Dispatch a task to the best-matching team member, executing it via a
/// headless sub-agent using the member's configured provider and model.
///
/// This is the bridge between the team organizational layer and the
/// execution layer: it routes the task, builds a persona prompt, and
/// runs the agent autonomously. If the primary provider fails, it
/// falls back through the member's fallback_provider and other
/// configured workspace providers.
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

    // Route to the best member
    let routed =
        route_member(team, task, &files, None).ok_or("routing failed: team has no members")?;

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
    let mut file_contexts = Vec::new();
    for file in &files {
        let full_path = root.join(file);
        match std::fs::read_to_string(&full_path) {
            Ok(contents) => {
                // Truncate very large files to keep prompt manageable
                let truncated = if contents.len() > 12_000 {
                    format!(
                        "{}\n... [truncated, {} bytes total]",
                        &contents[..12_000],
                        contents.len()
                    )
                } else {
                    contents
                };
                file_contexts.push(format!("### {}\n```\n{}\n```", file, truncated));
            }
            Err(e) => {
                file_contexts.push(format!("### {}\n[Could not read: {}]", file, e));
            }
        }
    }

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
    prompt.push_str("\n## Required Output Format\n");
    prompt.push_str("Produce a structured report:\n");
    prompt.push_str("1. **Summary** — 1-2 sentence overview of what you found/did\n");
    prompt.push_str("2. **Findings** — Specific, actionable items with file:line references\n");
    prompt.push_str("3. **Recommendations** — Prioritized next steps\n");
    prompt.push_str("\nBe concise. No preamble, no exploration narrative, just results.\n");

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

    // Build fallback chain and execute with failover
    let fallback_chain = build_fallback_chain(member, root);
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
    // Set when the winning attempt came from a sub-7B model: the result is
    // returned, but flagged — never reported as a silent clean success.
    let mut degraded_note: Option<String> = None;
    let dispatch_start = std::time::Instant::now();
    const MAX_DISPATCH_TIME: std::time::Duration = std::time::Duration::from_secs(90);

    for (provider, model) in &fallback_chain {
        // Check overall timeout
        if dispatch_start.elapsed() > MAX_DISPATCH_TIME {
            log::warn!("team_dispatch: overall timeout (90s) exceeded, stopping fallback attempts");
            final_transcript
                .push_str("\n\n[Dispatch timeout: no model succeeded within 90 seconds]");
            break;
        }

        // Try the primary model first, then fallback models for this provider (max 3 attempts per provider)
        let mut models_to_try = vec![model.clone()];
        let fallback = fallback_models_for(*provider, model);
        models_to_try.extend(fallback.into_iter().take(2)); // Limit to 3 total attempts per provider

        let mut provider_succeeded = false;

        for try_model in &models_to_try {
            let result =
                crate::agent::run_headless_subagent(crate::agent::HeadlessSubAgentRequest {
                    workspace_root: root.to_path_buf(),
                    provider: *provider,
                    model: try_model.clone(),
                    thinking: false,
                    prompt: prompt.clone(),
                    cancel_rx: None,
                    progress: None,
                    scoped_files: scoped_files.clone(),
                    max_turns: Some(8),
                });

            all_status_updates.extend(result.status_updates.clone());
            used_provider = *provider;
            used_model = try_model.clone();

            // Check if the attempt succeeded
            let has_error = result.transcript.contains("Error:")
                || result.transcript.contains("exhausted or failed")
                || is_provider_unavailable(&result.status_updates, &result.transcript);

            let degraded_small = !has_error && is_small_model(try_model);
            attempt_log.push(json!({
                "provider": provider.slug(),
                "model": try_model,
                "succeeded": !has_error,
                "quality": if degraded_small { "degraded" } else { "ok" },
                "status_count": result.status_updates.len(),
            }));

            if !has_error {
                final_transcript = result.transcript;
                provider_succeeded = true;
                if degraded_small {
                    degraded_note = Some(format!(
                        "output was produced by small model '{}' (<7B parameters) \u{2014} treat as unreliable; configure a cloud provider or pull a larger local model",
                        try_model
                    ));
                }
                break;
            }

            final_transcript = result.transcript;

            // If the provider itself is unavailable (no keys/accounts),
            // don't try other models — move to next provider immediately.
            if is_provider_unavailable(&result.status_updates, &final_transcript) {
                log::warn!(
                    "team_dispatch: {} unavailable (no keys/accounts), skipping to next provider",
                    provider.slug()
                );
                break;
            }

            // Model-specific failure (403/400/quota) — try next model on same provider
            log::warn!(
                "team_dispatch: {} / {} failed, trying next model on same provider",
                provider.slug(),
                try_model
            );
        }

        if provider_succeeded {
            break;
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
        "degraded": degraded_note.is_some(),
        "degraded_reason": degraded_note,
        "fallback_attempts": attempt_log,
        "status_updates": all_status_updates,
        "transcript": final_transcript,
    });

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
    let sm = SiteMap::open(&site_map_dir, 0).unwrap_or_else(|_| {
        // If opening fails, still try to build wiki from workspace index alone
        SiteMap::open(&site_map_dir, 0).expect("failed to open site map")
    });

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
}
