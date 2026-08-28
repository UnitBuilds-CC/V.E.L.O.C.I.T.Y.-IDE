//! Provider failover and routing contract tests.
//!
//! These tests verify the contracts between:
//! - AiProvider enum (slug/label roundtrips, alias resolution)
//! - ExpertMember fallback_provider field (serialization, effective resolution)
//! - Team routing (file-scope match, keyword match, fallback to lead)
//! - NDA serialization of provider and fallback fields

use crate::agent::AiProvider;
use crate::editor::expert_team::{ExpertMember, ExpertTeam};
use crate::editor::team_router::{
    debug_routing, parse_team_directive, resolve_team, route_member,
};
use serde_json::json;
use std::fs;

fn setup_root() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();
    (temp, root)
}

fn sample_team() -> ExpertTeam {
    ExpertTeam::new(
        "team_test",
        "Test Team",
        "Test team for routing",
        vec![
            ExpertMember::new(
                "lead",
                "Team Lead",
                "Architecture",
                AiProvider::OpenRouter,
                "gpt-4",
                vec!["architecture", "design"],
                vec!["src/"],
                "Oversee all tasks",
            ),
            ExpertMember::new(
                "frontend",
                "Frontend Dev",
                "UI Framework",
                AiProvider::Anthropic,
                "claude-3",
                vec!["ui", "frontend", "css"],
                vec!["src/ui/", "src/components/"],
                "Handle UI tasks",
            ),
            ExpertMember::new(
                "backend",
                "Backend Dev",
                "API Database",
                AiProvider::CloudflareWorkersAi,
                "cf-model",
                vec!["api", "database", "sql"],
                vec!["src/api/", "src/db/"],
                "Handle backend tasks",
            ),
        ],
        false,
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// Provider Fallback Serialization
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn member_without_fallback_serializes_cleanly() {
    let member = ExpertMember::new(
        "dev1",
        "Developer",
        "Coding",
        AiProvider::OpenAI,
        "gpt-4",
        vec!["rust"],
        vec!["src/"],
        "Write code",
    );
    let json = serde_json::to_string(&member).unwrap();
    // fallback_provider should be skipped when None
    assert!(!json.contains("fallback_provider"));
}

#[test]
fn member_with_fallback_includes_field() {
    let mut member = ExpertMember::new(
        "dev1",
        "Developer",
        "Coding",
        AiProvider::OpenRouter,
        "gpt-4",
        vec!["rust"],
        vec!["src/"],
        "Write code",
    );
    member.fallback_provider = Some(AiProvider::CloudflareWorkersAi);

    let json = serde_json::to_string(&member).unwrap();
    assert!(json.contains("fallback_provider"));
    assert!(json.contains("CloudflareWorkersAi"), "expected CloudflareWorkersAi in: {}", json);
}

#[test]
fn member_fallback_deserializes_from_slug() {
    let json = r#"{
        "id": "dev1",
        "name": "Developer",
        "role": "Coding",
        "provider": "OpenRouter",
        "model_id": "gpt-4",
        "skills": ["rust"],
        "scope_patterns": ["src/"],
        "tools": [],
        "workflow_instructions": "Write code",
        "fallback_provider": "Anthropic"
    }"#;
    let member: ExpertMember = serde_json::from_str(json).unwrap();
    assert_eq!(member.fallback_provider, Some(AiProvider::Anthropic));
}

#[test]
fn member_fallback_null_deserializes_as_none() {
    let json = r#"{
        "id": "dev1",
        "name": "Developer",
        "role": "Coding",
        "provider": "OpenRouter",
        "model_id": "gpt-4",
        "skills": [],
        "scope_patterns": [],
        "tools": [],
        "workflow_instructions": "",
        "fallback_provider": null
    }"#;
    let member: ExpertMember = serde_json::from_str(json).unwrap();
    assert_eq!(member.fallback_provider, None);
}

#[test]
fn member_fallback_missing_deserializes_as_none() {
    let json = r#"{
        "id": "dev1",
        "name": "Developer",
        "role": "Coding",
        "provider": "OpenRouter",
        "model_id": "gpt-4",
        "skills": [],
        "scope_patterns": [],
        "tools": [],
        "workflow_instructions": ""
    }"#;
    let member: ExpertMember = serde_json::from_str(json).unwrap();
    assert_eq!(member.fallback_provider, None);
}

// ═══════════════════════════════════════════════════════════════════════════
// Effective Provider Resolution
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn effective_provider_uses_member_when_model_set() {
    let member = ExpertMember::new(
        "dev",
        "Dev",
        "Coding",
        AiProvider::Anthropic,
        "claude-3-opus",
        vec![],
        vec![],
        "",
    );
    let (provider, model) = member.resolve_effective_provider_and_model(
        AiProvider::OpenAI,
        "gpt-4-default",
    );
    assert_eq!(provider, AiProvider::Anthropic);
    assert_eq!(model, "claude-3-opus");
}

#[test]
fn effective_provider_falls_back_to_default_when_model_empty() {
    let member = ExpertMember::new(
        "dev",
        "Dev",
        "Coding",
        AiProvider::Anthropic,
        "",  // empty model_id
        vec![],
        vec![],
        "",
    );
    let (provider, model) = member.resolve_effective_provider_and_model(
        AiProvider::OpenAI,
        "gpt-4-default",
    );
    assert_eq!(provider, AiProvider::OpenAI);
    assert_eq!(model, "gpt-4-default");
}

#[test]
fn effective_provider_falls_back_when_model_whitespace() {
    let member = ExpertMember::new(
        "dev",
        "Dev",
        "Coding",
        AiProvider::Anthropic,
        "   ",  // whitespace-only model_id
        vec![],
        vec![],
        "",
    );
    let (provider, model) = member.resolve_effective_provider_and_model(
        AiProvider::Groq,
        "llama-3",
    );
    assert_eq!(provider, AiProvider::Groq);
    assert_eq!(model, "llama-3");
}

// ═══════════════════════════════════════════════════════════════════════════
// Routing Contract Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn route_file_scope_match_is_authoritative() {
    let team = sample_team();
    let routed = route_member(
        &team,
        "do something",
        &["src/ui/button.rs".to_string()],
        None,
    )
    .unwrap();
    assert_eq!(routed.member_id, "frontend");
    assert!(routed.reason.contains("file scope match"));
}

#[test]
fn route_narrow_scope_beats_broad_scope() {
    let team = ExpertTeam::new(
        "team_scope",
        "Scope Team",
        "Test scope specificity",
        vec![
            ExpertMember::new(
                "broad",
                "Broad Dev",
                "General",
                AiProvider::OpenRouter,
                "m1",
                vec![],
                vec!["src/"],
                "",
            ),
            ExpertMember::new(
                "narrow",
                "Narrow Dev",
                "Specialist",
                AiProvider::OpenRouter,
                "m2",
                vec![],
                vec!["src/net/"],
                "",
            ),
        ],
        false,
    );
    let routed = route_member(
        &team,
        "anything",
        &["src/net/tcp.rs".to_string()],
        None,
    )
    .unwrap();
    assert_eq!(routed.member_id, "narrow");
}

#[test]
fn route_keyword_match_beats_fallback() {
    let team = sample_team();
    let routed = route_member(
        &team,
        "build the database layer",
        &[],
        None,
    )
    .unwrap();
    // "database" (>=4 chars) should match backend's skills
    assert_eq!(routed.member_id, "backend");
    assert!(routed.reason.contains("keyword match"));
}

#[test]
fn route_no_match_falls_back_to_lead() {
    let team = sample_team();
    let routed = route_member(
        &team,
        "completely unrelated task xyz",
        &[],
        None,
    )
    .unwrap();
    assert_eq!(routed.member_id, "lead");
    assert!(routed.reason.contains("no match") || routed.reason.contains("routed to team lead"));
}

#[test]
fn route_empty_team_returns_none() {
    let team = ExpertTeam::new("empty", "Empty", "Empty team", vec![], false);
    let result = route_member(&team, "any task", &[], None);
    assert!(result.is_none());
}

#[test]
fn route_with_custom_router() {
    let team = sample_team();
    let router = |_task: &str| Some("backend".to_string());
    let routed = route_member(
        &team,
        "ambiguous task",
        &[],
        Some(&router),
    )
    .unwrap();
    assert_eq!(routed.member_id, "backend");
    assert_eq!(routed.reason, "selected by router model");
}

#[test]
fn route_router_returns_unknown_member_falls_through() {
    let team = sample_team();
    let router = |_task: &str| Some("nonexistent_member".to_string());
    let routed = route_member(
        &team,
        "ambiguous task",
        &[],
        Some(&router),
    )
    .unwrap();
    // Router returned unknown member, should fall back to lead
    assert_eq!(routed.member_id, "lead");
}

// ═══════════════════════════════════════════════════════════════════════════
// Debug Routing Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn debug_routing_shows_file_scope_stage() {
    let team = sample_team();
    let decision = debug_routing(
        &team,
        "do something",
        &["src/ui/button.rs".to_string()],
    );
    assert_eq!(decision.stage, "file_scope_match");
    assert_eq!(decision.member_id, "frontend");
}

#[test]
fn debug_routing_shows_keyword_stage() {
    let team = sample_team();
    let decision = debug_routing(
        &team,
        "build the database schema",
        &[],
    );
    assert_eq!(decision.stage, "keyword_match");
    assert_eq!(decision.member_id, "backend");
}

#[test]
fn debug_routing_shows_fallback_stage() {
    let team = sample_team();
    let decision = debug_routing(
        &team,
        "xyzzy nonsense task",
        &[],
    );
    assert_eq!(decision.stage, "fallback_to_lead");
    assert_eq!(decision.member_id, "lead");
}

#[test]
fn debug_routing_empty_team_returns_error() {
    let team = ExpertTeam::new("empty", "Empty", "Empty", vec![], false);
    let decision = debug_routing(&team, "task", &[]);
    assert_eq!(decision.stage, "error");
}

#[test]
fn debug_routing_includes_scores() {
    let team = sample_team();
    let decision = debug_routing(&team, "build the API", &[]);
    assert!(!decision.scores.is_empty());
    assert_eq!(decision.scores.len(), team.members.len());
}

// ═══════════════════════════════════════════════════════════════════════════
// Team Directive Parsing
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_at_slug_directive() {
    let d = parse_team_directive("@test-team fix the bug").unwrap();
    assert_eq!(d.team_query, "test-team");
    assert_eq!(d.task, "fix the bug");
}

#[test]
fn parse_slash_team_colon_directive() {
    let d = parse_team_directive("/team Test Team: fix the bug").unwrap();
    assert_eq!(d.team_query, "Test Team");
    assert_eq!(d.task, "fix the bug");
}

#[test]
fn parse_slash_team_space_directive() {
    let d = parse_team_directive("/team TestTeam fix the bug").unwrap();
    assert_eq!(d.team_query, "TestTeam");
    assert_eq!(d.task, "fix the bug");
}

#[test]
fn parse_natural_language_with_verb() {
    let d = parse_team_directive("delegate to the test team the bug fix").unwrap();
    assert_eq!(d.team_query, "test");
    assert!(d.task.contains("bug fix"));
}

#[test]
fn parse_rejects_plain_prompt() {
    assert!(parse_team_directive("fix the bug please").is_none());
}

#[test]
fn parse_rejects_empty_input() {
    assert!(parse_team_directive("").is_none());
    assert!(parse_team_directive("   ").is_none());
}

#[test]
fn parse_rejects_team_without_verb() {
    assert!(parse_team_directive("the test team is great").is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// Team Resolution
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn resolve_team_by_exact_slug() {
    let teams = vec![sample_team()];
    assert_eq!(resolve_team(&teams, "team_test"), Some(0));
}

#[test]
fn resolve_team_by_name() {
    let teams = vec![sample_team()];
    assert_eq!(resolve_team(&teams, "Test Team"), Some(0));
}

#[test]
fn resolve_team_by_partial_name() {
    let teams = vec![sample_team()];
    assert_eq!(resolve_team(&teams, "test"), Some(0));
}

#[test]
fn resolve_team_returns_none_for_unknown() {
    let teams = vec![sample_team()];
    assert_eq!(resolve_team(&teams, "nonexistent"), None);
}

#[test]
fn resolve_team_returns_none_for_empty() {
    let teams = vec![sample_team()];
    assert_eq!(resolve_team(&teams, ""), None);
}

// ═══════════════════════════════════════════════════════════════════════════
// Provider Contract Edge Cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn all_provider_labels_are_non_empty() {
    let providers = vec![
        AiProvider::CloudflareWorkersAi,
        AiProvider::OpenRouter,
        AiProvider::AzureOpenAi,
        AiProvider::LocalOllama,
        AiProvider::OpenAI,
        AiProvider::Anthropic,
        AiProvider::GoogleVertex,
        AiProvider::Deepseek,
        AiProvider::AlibabaQwen,
        AiProvider::AwsBedrock,
        AiProvider::Groq,
        AiProvider::Mistral,
        AiProvider::TogetherAi,
        AiProvider::FireworksAi,
        AiProvider::Perplexity,
        AiProvider::Cerebras,
    ];
    for p in &providers {
        assert!(!p.label().is_empty(), "provider {:?} has empty label", p);
        assert!(!p.slug().is_empty(), "provider {:?} has empty slug", p);
    }
}

#[test]
fn from_slug_rejects_whitespace_only() {
    assert_eq!(AiProvider::from_slug("   "), None);
}

#[test]
fn from_slug_rejects_special_chars() {
    assert_eq!(AiProvider::from_slug("open@router!"), None);
}

#[test]
fn member_scope_match_is_case_insensitive() {
    let member = ExpertMember::new(
        "dev",
        "Dev",
        "Coding",
        AiProvider::OpenAI,
        "gpt-4",
        vec![],
        vec!["src/UI/"],
        "",
    );
    assert!(member.matches_scope("SRC/ui/button.rs"));
}

#[test]
fn member_scope_match_empty_patterns_returns_none() {
    let member = ExpertMember::new(
        "dev",
        "Dev",
        "Coding",
        AiProvider::OpenAI,
        "gpt-4",
        vec![],
        vec![],  // no scope patterns
        "",
    );
    assert!(!member.matches_scope("src/anything.rs"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Persistence with Fallback Providers
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn team_with_fallback_roundtrips_through_save_load() {
    let (_temp, root) = setup_root();

    let mut team = sample_team();
    team.members[0].fallback_provider = Some(AiProvider::Anthropic);
    team.members[1].fallback_provider = Some(AiProvider::CloudflareWorkersAi);

    crate::editor::expert_team::save_expert_teams(&root, &[team.clone()]);
    let loaded = crate::editor::expert_team::load_expert_teams(&root);

    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].members[0].fallback_provider, Some(AiProvider::Anthropic));
    assert_eq!(loaded[0].members[1].fallback_provider, Some(AiProvider::CloudflareWorkersAi));
    assert_eq!(loaded[0].members[2].fallback_provider, None);
}
