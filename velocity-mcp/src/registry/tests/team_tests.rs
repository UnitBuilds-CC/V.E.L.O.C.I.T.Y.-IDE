use crate::editor::expert_team::load_expert_teams;
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
