use crate::editor::expert_team::{
    detect_scope_overlaps, load_expert_teams, validate_team_composition, ExpertMember, ExpertTeam,
    MemberUpdate, ValidationSeverity,
};
use crate::registry::call_tool_in_workspace;
use serde_json::json;
use std::fs;

fn setup_root() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();
    (temp, root)
}

#[test]
fn create_expert_team_persists_and_is_routable() {
    let (_temp, root) = setup_root();

    let output = call_tool_in_workspace(
        &root,
        "create_expert_team",
        &json!({
            "name": "Rust Backend Team",
            "description": "Owns the async services.",
            "members": [
                {
                    "name": "API Engineer",
                    "role": "Services & Endpoints",
                    "provider": "openrouter",
                    "model_id": "anthropic/claude-3.5-sonnet",
                    "scope_patterns": ["src/api/"],
                    "skills": ["system_tools"],
                    "workflow_instructions": "Build tower services."
                }
            ]
        }),
    )
    .unwrap();

    assert!(output.contains("rust-backend-team"));

    // Reload from disk and confirm the team round-trips with a routable slug.
    let canon = root.canonicalize().unwrap();
    let teams = load_expert_teams(&canon);
    let team = teams
        .iter()
        .find(|t| t.slug() == "rust-backend-team")
        .expect("team persisted");
    assert_eq!(team.id, "team_rust-backend-team");
    assert!(!team.is_preset);
    assert_eq!(team.members.len(), 1);
    assert_eq!(team.members[0].role, "Services & Endpoints");
    assert_eq!(team.members[0].scope_patterns, vec!["src/api/".to_string()]);
}

#[test]
fn create_expert_team_replaces_matching_slug() {
    let (_temp, root) = setup_root();

    for role in ["First", "Second"] {
        call_tool_in_workspace(
            &root,
            "create_expert_team",
            &json!({
                "name": "Docs Team",
                "members": [{ "name": "Writer", "role": role }]
            }),
        )
        .unwrap();
    }

    let canon = root.canonicalize().unwrap();
    let teams = load_expert_teams(&canon);
    let matching: Vec<_> = teams.iter().filter(|t| t.slug() == "docs-team").collect();
    assert_eq!(
        matching.len(),
        1,
        "second create should replace, not append"
    );
    assert_eq!(matching[0].members[0].role, "Second");
}

#[test]
fn create_expert_team_requires_members() {
    let (_temp, root) = setup_root();
    let result = call_tool_in_workspace(
        &root,
        "create_expert_team",
        &json!({ "name": "Empty", "members": [] }),
    );
    assert!(result.is_err());
}

#[test]
fn create_skill_file_writes_nda() {
    let (_temp, root) = setup_root();

    call_tool_in_workspace(
        &root,
        "create_skill_file",
        &json!({
            "id": "Netcode Expert",
            "name": "Netcode",
            "body": "Prefer deterministic lockstep."
        }),
    )
    .unwrap();

    let skill_path = root
        .join(".velocity")
        .join("skills")
        .join("netcode-expert.nda");
    let raw = fs::read(skill_path).expect("skill nda written");
    let plain = crate::agent::crypto::open(&root, b"skill", &raw);
    let content = String::from_utf8(plain).expect("utf8 skill nda");
    assert!(content.starts_with("skill version 1"));
}

#[test]
fn list_expert_teams_includes_created_team() {
    let (_temp, root) = setup_root();

    call_tool_in_workspace(
        &root,
        "create_expert_team",
        &json!({
            "name": "Data Team",
            "members": [{ "name": "DBA", "role": "SQL" }]
        }),
    )
    .unwrap();

    let listing = call_tool_in_workspace(&root, "list_expert_teams", &json!({})).unwrap();
    assert!(listing.contains("data-team"));
    assert!(listing.contains("Data Team"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Batch 1: Edit / Update Workflow Tests
// ═══════════════════════════════════════════════════════════════════════════

/// Helper: create a non-preset team and return its slug.
fn create_test_team(root: &std::path::Path) -> String {
    call_tool_in_workspace(
        root,
        "create_expert_team",
        &json!({
            "name": "Test Team",
            "description": "A test team.",
            "members": [
                {
                    "name": "Lead Dev",
                    "role": "Architecture",
                    "scope_patterns": ["src/"],
                    "skills": ["system_tools"]
                },
                {
                    "name": "API Coder",
                    "role": "REST APIs",
                    "scope_patterns": ["src/api/"],
                    "skills": ["system_tools"]
                }
            ]
        }),
    )
    .unwrap();
    "test-team".to_string()
}

#[test]
fn update_expert_team_renames_and_persists() {
    let (_temp, root) = setup_root();
    create_test_team(&root);

    let output = call_tool_in_workspace(
        &root,
        "update_expert_team",
        &json!({
            "team_id": "test-team",
            "name": "Renamed Team",
            "description": "New description."
        }),
    )
    .unwrap();

    assert!(output.contains("Updated team"));
    assert!(output.contains("Renamed Team"));

    let canon = root.canonicalize().unwrap();
    let teams = load_expert_teams(&canon);
    let team = teams
        .iter()
        .find(|t| t.slug() == "renamed-team")
        .expect("team renamed and persisted");
    assert_eq!(team.name, "Renamed Team");
    assert_eq!(team.description, "New description.");
}

#[test]
fn update_expert_team_rejects_preset() {
    let (_temp, root) = setup_root();
    // Preset teams exist by default
    let result = call_tool_in_workspace(
        &root,
        "update_expert_team",
        &json!({
            "team_id": "c-software-team",
            "name": "New Name"
        }),
    );
    assert!(result.is_err(), "should reject preset team edits");
    assert!(result.unwrap_err().to_string().contains("preset"));
}

#[test]
fn update_team_member_changes_fields() {
    let (_temp, root) = setup_root();
    create_test_team(&root);

    let canon = root.canonicalize().unwrap();
    let teams = load_expert_teams(&canon);
    let team = teams.iter().find(|t| t.slug() == "test-team").unwrap();
    let member_id = &team.members[0].id;

    let output = call_tool_in_workspace(
        &root,
        "update_team_member",
        &json!({
            "team_id": "test-team",
            "member_id": member_id,
            "role": "Updated Role",
            "model_id": "anthropic/claude-3.5-sonnet"
        }),
    )
    .unwrap();

    assert!(output.contains("Updated member"));
    assert!(output.contains("role"));
    assert!(output.contains("model_id"));

    let teams = load_expert_teams(&canon);
    let team = teams.iter().find(|t| t.slug() == "test-team").unwrap();
    assert_eq!(team.members[0].role, "Updated Role");
    assert_eq!(team.members[0].model_id, "anthropic/claude-3.5-sonnet");
    // Name should remain unchanged
    assert_eq!(team.members[0].name, "Lead Dev");
}

#[test]
fn add_team_member_appends_and_persists() {
    let (_temp, root) = setup_root();
    create_test_team(&root);

    let output = call_tool_in_workspace(
        &root,
        "add_team_member",
        &json!({
            "team_id": "test-team",
            "member": {
                "name": "QA Tester",
                "role": "Integration Testing",
                "scope_patterns": ["tests/"],
                "skills": ["system_tools"]
            }
        }),
    )
    .unwrap();

    assert!(output.contains("Added member"));
    assert!(output.contains("QA Tester"));
    assert!(output.contains("3 member(s)"));

    let canon = root.canonicalize().unwrap();
    let teams = load_expert_teams(&canon);
    let team = teams.iter().find(|t| t.slug() == "test-team").unwrap();
    assert_eq!(team.members.len(), 3);
    assert_eq!(team.members[2].name, "QA Tester");
}

#[test]
fn remove_team_member_removes_and_persists() {
    let (_temp, root) = setup_root();
    create_test_team(&root);

    let canon = root.canonicalize().unwrap();
    let teams = load_expert_teams(&canon);
    let team = teams.iter().find(|t| t.slug() == "test-team").unwrap();
    let member_id = team.members[1].id.clone();

    let output = call_tool_in_workspace(
        &root,
        "remove_team_member",
        &json!({
            "team_id": "test-team",
            "member_id": member_id
        }),
    )
    .unwrap();

    assert!(output.contains("Removed member"));
    assert!(output.contains("1 member(s)"));

    let teams = load_expert_teams(&canon);
    let team = teams.iter().find(|t| t.slug() == "test-team").unwrap();
    assert_eq!(team.members.len(), 1);
    assert_eq!(team.members[0].name, "Lead Dev");
}

// ═══════════════════════════════════════════════════════════════════════════
// Batch 1: Validation Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn validate_team_reports_no_issues_for_well_formed_team() {
    let (_temp, root) = setup_root();
    // Use disjoint scopes so no overlap warnings are generated
    call_tool_in_workspace(
        &root,
        "create_expert_team",
        &json!({
            "name": "Valid Team",
            "description": "A well-formed team.",
            "members": [
                {
                    "name": "Lead Dev",
                    "role": "Architecture",
                    "scope_patterns": ["src/"],
                    "skills": ["system_tools"]
                },
                {
                    "name": "UI Coder",
                    "role": "Frontend UI",
                    "scope_patterns": ["web/"],
                    "skills": ["system_tools"]
                }
            ]
        }),
    )
    .unwrap();

    let output = call_tool_in_workspace(
        &root,
        "validate_team",
        &json!({ "team_id": "valid-team" }),
    )
    .unwrap();

    assert!(output.contains("passed all validation"));
}

#[test]
fn validate_team_detects_empty_team() {
    // Build a team with no members directly
    let team = ExpertTeam::new("team_empty", "Empty Team", "desc", vec![], false);
    let issues = validate_team_composition(&team);
    assert!(issues
        .iter()
        .any(|i| i.severity == ValidationSeverity::Error && i.code == "NO_MEMBERS"));
}

#[test]
fn validate_team_detects_duplicate_names() {
    let team = ExpertTeam::new(
        "team_dup",
        "Dup Team",
        "desc",
        vec![
            ExpertMember::new(
                "m1",
                "Same Name",
                "Role A",
                crate::agent::AiProvider::CloudflareWorkersAi,
                "",
                vec![],
                vec!["src/a/"],
                "",
            ),
            ExpertMember::new(
                "m2",
                "Same Name",
                "Role B",
                crate::agent::AiProvider::CloudflareWorkersAi,
                "",
                vec![],
                vec!["src/b/"],
                "",
            ),
        ],
        false,
    );
    let issues = validate_team_composition(&team);
    assert!(issues
        .iter()
        .any(|i| i.severity == ValidationSeverity::Error && i.code == "DUPLICATE_NAME"));
}

#[test]
fn check_scope_overlaps_detects_overlap() {
    let (_temp, root) = setup_root();
    // Create a team with overlapping scopes
    call_tool_in_workspace(
        &root,
        "create_expert_team",
        &json!({
            "name": "Overlap Team",
            "members": [
                {
                    "name": "Broad",
                    "role": "General",
                    "scope_patterns": ["src/"]
                },
                {
                    "name": "Narrow",
                    "role": "Specific",
                    "scope_patterns": ["src/api/"]
                }
            ]
        }),
    )
    .unwrap();

    let output = call_tool_in_workspace(
        &root,
        "check_scope_overlaps",
        &json!({ "team_id": "overlap-team" }),
    )
    .unwrap();

    assert!(output.contains("1 scope overlap"));
    assert!(output.contains("Broad"));
    assert!(output.contains("Narrow"));
}

#[test]
fn check_scope_overlaps_clean_when_disjoint() {
    let (_temp, root) = setup_root();
    call_tool_in_workspace(
        &root,
        "create_expert_team",
        &json!({
            "name": "Clean Team",
            "members": [
                {
                    "name": "Frontend",
                    "role": "UI",
                    "scope_patterns": ["ui/"]
                },
                {
                    "name": "Backend",
                    "role": "API",
                    "scope_patterns": ["server/"]
                }
            ]
        }),
    )
    .unwrap();

    let output = call_tool_in_workspace(
        &root,
        "check_scope_overlaps",
        &json!({ "team_id": "clean-team" }),
    )
    .unwrap();

    assert!(output.contains("No scope overlaps"));
}

#[test]
fn detect_scope_overlaps_unit() {
    let team = ExpertTeam::new(
        "team_test",
        "Test",
        "",
        vec![
            ExpertMember::new(
                "a",
                "A",
                "Role",
                crate::agent::AiProvider::CloudflareWorkersAi,
                "",
                vec![],
                vec!["src/net/"],
                "",
            ),
            ExpertMember::new(
                "b",
                "B",
                "Role",
                crate::agent::AiProvider::CloudflareWorkersAi,
                "",
                vec![],
                vec!["src/"],
                "",
            ),
        ],
        false,
    );
    let overlaps = detect_scope_overlaps(&team);
    assert_eq!(overlaps.len(), 1);
    assert_eq!(overlaps[0].member_a_id, "a");
    assert_eq!(overlaps[0].member_b_id, "b");
}

#[test]
fn member_update_apply_tracks_changed_fields() {
    let mut member = ExpertMember::new(
        "m1",
        "Original",
        "Original Role",
        crate::agent::AiProvider::CloudflareWorkersAi,
        "model-a",
        vec!["skill1"],
        vec!["src/"],
        "instructions",
    );

    let update = MemberUpdate {
        name: Some("Updated".to_string()),
        role: None,
        provider: None,
        model_id: Some("model-b".to_string()),
        skills: Some(vec!["skill2".to_string()]),
        scope_patterns: None,
        tools: None,
        workflow_instructions: None,
    };

    let changed = update.apply(&mut member);
    assert!(changed.contains(&"name"));
    assert!(changed.contains(&"model_id"));
    assert!(changed.contains(&"skills"));
    assert!(!changed.contains(&"role"));
    assert_eq!(member.name, "Updated");
    assert_eq!(member.model_id, "model-b");
    assert_eq!(member.skills, vec!["skill2"]);
    // Unchanged fields
    assert_eq!(member.role, "Original Role");
    assert_eq!(member.scope_patterns, vec!["src/"]);
}

// ═══════════════════════════════════════════════════════════════════════════
// Batch 2: Clone / Import / Export Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn clone_expert_team_creates_editable_copy() {
    let (_temp, root) = setup_root();
    // Clone a preset team
    let output = call_tool_in_workspace(
        &root,
        "clone_expert_team",
        &json!({
            "team_id": "c-software-team",
            "new_name": "My C# Team"
        }),
    )
    .unwrap();

    assert!(output.contains("Cloned team"));
    assert!(output.contains("My C# Team"));
    assert!(output.contains("my-c-team"));

    let canon = root.canonicalize().unwrap();
    let teams = load_expert_teams(&canon);
    let cloned = teams
        .iter()
        .find(|t| t.slug() == "my-c-team")
        .expect("cloned team persisted");
    assert!(!cloned.is_preset, "clone should not be preset");
    assert_eq!(cloned.members.len(), 4, "all members copied");
    assert_eq!(cloned.members[0].name, "Lead C# Architect");
}

#[test]
fn clone_expert_team_rejects_duplicate_slug() {
    let (_temp, root) = setup_root();
    create_test_team(&root);

    // Try to clone with the same name (same slug)
    let result = call_tool_in_workspace(
        &root,
        "clone_expert_team",
        &json!({
            "team_id": "test-team",
            "new_name": "Test Team"
        }),
    );
    assert!(result.is_err(), "should reject duplicate slug");
    assert!(result.unwrap_err().to_string().contains("already exists"));
}

#[test]
fn export_and_import_team_roundtrip() {
    let (_temp, root) = setup_root();
    create_test_team(&root);

    // Export the team
    let export_output = call_tool_in_workspace(
        &root,
        "export_expert_team",
        &json!({ "team_id": "test-team" }),
    )
    .unwrap();

    assert!(export_output.contains("Exported team"));
    assert!(export_output.contains("Test Team"));

    // Extract the JSON portion (after the first newline)
    let json_start = export_output.find('\n').unwrap() + 1;
    let json_str = &export_output[json_start..];

    // Delete the original team by creating a fresh workspace
    let (_temp2, root2) = setup_root();

    // Import into the fresh workspace
    let import_output = call_tool_in_workspace(
        &root2,
        "import_expert_team",
        &json!({ "json": json_str }),
    )
    .unwrap();

    assert!(import_output.contains("Imported team"));
    assert!(import_output.contains("Test Team"));

    let canon2 = root2.canonicalize().unwrap();
    let teams = load_expert_teams(&canon2);
    let imported = teams
        .iter()
        .find(|t| t.slug() == "test-team")
        .expect("imported team persisted");
    assert_eq!(imported.name, "Test Team");
    assert_eq!(imported.members.len(), 2);
    assert_eq!(imported.members[0].name, "Lead Dev");
}

#[test]
fn import_expert_team_replaces_matching_slug() {
    let (_temp, root) = setup_root();
    create_test_team(&root);

    // Create a JSON team with the same slug but different content
    let json = r#"{
        "id": "team_test-team",
        "name": "Test Team",
        "description": "Updated via import",
        "is_preset": false,
        "members": [
            {
                "id": "member_test_team_1",
                "name": "Solo Dev",
                "role": "Everything",
                "provider": "CloudflareWorkersAi",
                "model_id": "",
                "skills": [],
                "scope_patterns": ["src/"],
                "tools": [],
                "workflow_instructions": ""
            }
        ]
    }"#;

    let output = call_tool_in_workspace(
        &root,
        "import_expert_team",
        &json!({ "json": json }),
    )
    .unwrap();

    assert!(output.contains("Replaced team"));

    let canon = root.canonicalize().unwrap();
    let teams = load_expert_teams(&canon);
    let team = teams.iter().find(|t| t.slug() == "test-team").unwrap();
    assert_eq!(team.members.len(), 1, "replaced with imported version");
    assert_eq!(team.members[0].name, "Solo Dev");
}

#[test]
fn clone_with_name_unit() {
    let team = ExpertTeam::new(
        "team_original",
        "Original Team",
        "Original description",
        vec![ExpertMember::new(
            "member_orig_1",
            "Original Member",
            "Original Role",
            crate::agent::AiProvider::OpenRouter,
            "anthropic/claude-3.5-sonnet",
            vec!["skill1"],
            vec!["src/"],
            "original instructions",
        )],
        true,
    );

    let cloned = team.clone_with_name("Cloned Team");
    assert_eq!(cloned.name, "Cloned Team");
    assert_eq!(cloned.id, "team_cloned-team");
    assert_eq!(cloned.slug(), "cloned-team");
    assert!(!cloned.is_preset, "clone should not be preset");
    assert_eq!(cloned.members.len(), 1);
    assert_eq!(cloned.members[0].id, "member_cloned-team_1");
    assert_eq!(cloned.members[0].name, "Original Member");
    assert_eq!(cloned.members[0].role, "Original Role");
    assert_eq!(cloned.description, "Original description");
}
