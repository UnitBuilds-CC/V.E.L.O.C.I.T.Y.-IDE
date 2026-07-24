use std::fs;
use std::path::Path;
use serde::{Deserialize, Serialize};
use crate::agent::AiProvider;
use crate::agent::nda::{decode_nda_text, encode_nda_text};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExpertMember {
    pub id: String,
    pub name: String,
    pub role: String,
    pub provider: AiProvider,
    pub model_id: String,
    pub skills: Vec<String>,
    pub scope_patterns: Vec<String>,
    /// Optional allow-list of registry tool names this member may use.
    /// Empty means the member may use every registered tool.
    #[serde(default)]
    pub tools: Vec<String>,
    pub workflow_instructions: String,
}

impl ExpertMember {
    pub fn new(
        id: &str,
        name: &str,
        role: &str,
        provider: AiProvider,
        model_id: &str,
        skills: Vec<&str>,
        scope_patterns: Vec<&str>,
        workflow_instructions: &str,
    ) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            role: role.to_string(),
            provider,
            model_id: model_id.to_string(),
            skills: skills.into_iter().map(String::from).collect(),
            scope_patterns: scope_patterns.into_iter().map(String::from).collect(),
            tools: Vec::new(),
            workflow_instructions: workflow_instructions.to_string(),
        }
    }

    pub fn matches_scope(&self, path: &str) -> bool {
        self.scope_match_len(path).is_some()
    }

    /// Length of the most specific (longest) scope pattern that matches `path`.
    /// Returns `None` when no pattern matches. Used to let narrower scopes
    /// (e.g. `src/net/`) win over broader ones (e.g. `src/`).
    pub fn scope_match_len(&self, path: &str) -> Option<usize> {
        if self.scope_patterns.is_empty() {
            return None;
        }
        let lower_path = path.to_lowercase();
        self.scope_patterns
            .iter()
            .filter_map(|pattern| {
                let lower_pat = pattern.to_lowercase();
                if lower_path.contains(&lower_pat) || lower_pat.contains(&lower_path) {
                    Some(lower_pat.len())
                } else {
                    None
                }
            })
            .max()
    }

    pub fn resolve_effective_provider_and_model(&self, default_provider: AiProvider, default_model: &str) -> (AiProvider, String) {
        if self.model_id.trim().is_empty() {
            (default_provider, default_model.to_string())
        } else {
            (self.provider, self.model_id.clone())
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExpertTeam {
    pub id: String,
    pub name: String,
    pub description: String,
    pub members: Vec<ExpertMember>,
    pub is_preset: bool,
}

impl ExpertTeam {
    pub fn new(id: &str, name: &str, description: &str, members: Vec<ExpertMember>, is_preset: bool) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            members,
            is_preset,
        }
    }

    /// Lower-case, dash-joined slug of the team name for `@team` addressing.
    pub fn slug(&self) -> String {
        slugify(&self.name)
    }

    pub fn find_expert_for_task(&self, goal: &str, files: &[String]) -> Option<&ExpertMember> {
        // 1. Check file scope matching
        for file in files {
            if let Some(member) = self.members.iter().find(|m| m.matches_scope(file)) {
                return Some(member);
            }
        }
        // 2. Check keyword/role matching in goal text
        let goal_lower = goal.to_lowercase();
        if let Some(member) = self.members.iter().find(|m| {
            goal_lower.contains(&m.role.to_lowercase())
                || m.scope_patterns.iter().any(|p| goal_lower.contains(&p.to_lowercase()))
        }) {
            return Some(member);
        }
        // 3. Fall back to lead member (first member)
        self.members.first()
    }
}

/// Factory for 3 default common expert teams
pub fn default_preset_teams() -> Vec<ExpertTeam> {
    vec![
        // Team 1: C# Software Team
        ExpertTeam::new(
            "team_csharp",
            "C# Software Team",
            "Enterprise .NET Core backend & desktop suite development team with EF Data & NUnit testing experts.",
            vec![
                ExpertMember::new(
                    "member_csharp_lead",
                    "Lead C# Architect",
                    "Solution Architecture & Design",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["src/Core/", "ARCHITECTURE.md", "*.sln", "*.csproj"],
                    "Enforce clean architecture, dependency injection, async Task paradigms, and robust interface contracts.",
                ),
                ExpertMember::new(
                    "member_csharp_backend",
                    ".NET Core Backend Developer",
                    "ASP.NET Core APIs & Services",
                    AiProvider::CloudflareWorkersAi,
                    "@cf/moonshotai/kimi-k2.7-code",
                    vec!["system_tools"],
                    vec!["src/Services/", "src/Controllers/", "src/API/"],
                    "Implement REST endpoints, DTO mappings, and middleware using modern C# 12 features.",
                ),
                ExpertMember::new(
                    "member_csharp_ef",
                    "Entity Framework Data Specialist",
                    "Database & LINQ Optimization",
                    AiProvider::OpenRouter,
                    "deepseek/deepseek-coder",
                    vec!["system_tools"],
                    vec!["src/Data/", "src/Models/", "Migrations/"],
                    "Optimize DbContext configurations, LINQ queries, migration scripts, and repository abstractions.",
                ),
                ExpertMember::new(
                    "member_csharp_qa",
                    "NUnit QA & Integration Tester",
                    "Unit & Integration Testing",
                    AiProvider::LocalOllama,
                    "llama3.2",
                    vec!["system_tools"],
                    vec!["tests/", "src/Tests/"],
                    "Write thorough NUnit / Moq test suites covering boundary conditions and happy paths.",
                ),
            ],
            true,
        ),

        // Team 2: Android App Development Team
        ExpertTeam::new(
            "team_android",
            "Android App Development Team",
            "Full-stack Android mobile engineering team with Kotlin Compose, Gradle build scripts, and Espresso UI automation.",
            vec![
                ExpertMember::new(
                    "member_android_lead",
                    "Mobile Lead Architect",
                    "Android Architecture & State",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["android-cli", "system_tools"],
                    vec!["app/src/main/", "build.gradle.kts", "AndroidManifest.xml"],
                    "Maintain MVVM / Clean Architecture, Hilt dependency injection, and Android Lifecycle safety.",
                ),
                ExpertMember::new(
                    "member_android_ui",
                    "Kotlin UI & Jetpack Compose Specialist",
                    "Declarative UI & Animations",
                    AiProvider::CloudflareWorkersAi,
                    "@cf/moonshotai/kimi-k2.7-code",
                    vec!["system_tools"],
                    vec!["app/src/main/java/ui/", "components/"],
                    "Build responsive Jetpack Compose screens, Material 3 themes, preview composables, and smooth micro-animations.",
                ),
                ExpertMember::new(
                    "member_android_build",
                    "Android CLI & Build Engineer",
                    "SDK, Gradle & CI Pipeline",
                    AiProvider::OpenRouter,
                    "meta-llama/llama-3.3-70b-instruct",
                    vec!["android-cli"],
                    vec!["gradle/", "settings.gradle.kts", "build.gradle"],
                    "Manage Android SDK dependencies, Gradle build variants, proguard rules, and CLI deployment.",
                ),
                ExpertMember::new(
                    "member_android_qa",
                    "Espresso QA & Device Tester",
                    "UI & E2E Instrumentation",
                    AiProvider::AzureOpenAi,
                    "gpt-4o",
                    vec!["android-cli", "system_tools"],
                    vec!["app/src/androidTest/", "app/src/test/"],
                    "Write Espresso and ComposeTestRule UI instrumentation tests for automated device verification.",
                ),
            ],
            true,
        ),

        // Team 3: Doccit Maintenance Team
        ExpertTeam::new(
            "team_doccit",
            "Doccit Maintenance Team",
            "Dedicated maintenance team for the Doccit browser runtime, NDA indexing engine, and system documentation.",
            vec![
                ExpertMember::new(
                    "member_doccit_lead",
                    "Doccit Maintenance Lead",
                    "Core Runtime & Orchestration",
                    AiProvider::CloudflareWorkersAi,
                    "@cf/moonshotai/kimi-k2.7-code",
                    vec!["system_tools"],
                    vec!["browsing/", "velocity-mcp/src/editor/"],
                    "Coordinate runtime health, session survival, CDP protocol events, and editor panel integrations.",
                ),
                ExpertMember::new(
                    "member_doccit_docs",
                    "Documentation & Spec Specialist",
                    "Docs, Diagrams & Specifications",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["docs/", "*.md", "README.md"],
                    "Maintain clear markdown documentation, Mermaid diagrams, API references, and sitemap indices.",
                ),
                ExpertMember::new(
                    "member_doccit_nda",
                    "NDA Indexing & SiteMap Expert",
                    "Binary NDA & Code Graph",
                    AiProvider::CloudflareWorkersAi,
                    "@cf/moonshotai/kimi-k2.7-code",
                    vec!["system_tools"],
                    vec!["velocity-ide/src/site_map/", "*.nda"],
                    "Optimize RustToNda compilation, site_map indexing, Merkle tree verifiers, and fast symbol lookup.",
                ),
                ExpertMember::new(
                    "member_doccit_audit",
                    "Replay & Evidence Auditor",
                    "Verification & Replay Auditing",
                    AiProvider::AzureOpenAi,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec![".velocity/browser_artifacts/", "tests/"],
                    "Audit browser replay traces, screenshot artifacts, download logs, and checkpoint integrity.",
                ),
            ],
            true,
        ),
    ]
}

/// Lower-case slug: alphanumerics preserved, runs of other chars become a single dash.
pub fn slugify(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

pub fn load_expert_teams(workspace_root: &Path) -> Vec<ExpertTeam> {
    let dir = workspace_root.join(".velocity");
    let nda_path = dir.join("expert_teams.nda");
    if let Ok(bytes) = fs::read(&nda_path) {
        let plain = crate::agent::crypto::open(workspace_root, b"expert_teams", &bytes);
        let content = String::from_utf8_lossy(&plain);
        let teams = parse_expert_teams_nda(&content);
        if !teams.is_empty() {
            return teams;
        }
    }

    // Legacy migration: read the old JSON store once and rewrite it as NDA.
    let json_path = dir.join("expert_teams.json");
    if json_path.exists() {
        if let Ok(content) = fs::read_to_string(&json_path) {
            if let Ok(user_teams) = serde_json::from_str::<Vec<ExpertTeam>>(&content) {
                if !user_teams.is_empty() {
                    save_expert_teams(workspace_root, &user_teams);
                    return user_teams;
                }
            }
        }
    }

    default_preset_teams()
}

pub fn save_expert_teams(workspace_root: &Path, teams: &[ExpertTeam]) -> bool {
    let dir = workspace_root.join(".velocity");
    let _ = fs::create_dir_all(&dir);
    let nda_path = dir.join("expert_teams.nda");
    let serialized = serialize_expert_teams_nda(teams);
    let bytes = crate::agent::crypto::seal(workspace_root, b"expert_teams", serialized.as_bytes())
        .unwrap_or_else(|| serialized.into_bytes());
    fs::write(nda_path, bytes).is_ok()
}

/// Serialize teams into the workspace NDA text convention (versioned, tab-delimited).
pub fn serialize_expert_teams_nda(teams: &[ExpertTeam]) -> String {
    let mut lines = vec![
        "expert-teams version 1".to_string(),
        format!("team_count {}", teams.len()),
    ];
    for (ti, team) in teams.iter().enumerate() {
        lines.push(format!("team\t{}\tid\t{}", ti, encode_nda_text(&team.id)));
        lines.push(format!("team\t{}\tname\t{}", ti, encode_nda_text(&team.name)));
        lines.push(format!(
            "team\t{}\tdescription\t{}",
            ti,
            encode_nda_text(&team.description)
        ));
        lines.push(format!("team\t{}\tis_preset\t{}", ti, team.is_preset));
        lines.push(format!("team\t{}\tmember_count\t{}", ti, team.members.len()));
        for (mi, m) in team.members.iter().enumerate() {
            lines.push(format!("member\t{}\t{}\tid\t{}", ti, mi, encode_nda_text(&m.id)));
            lines.push(format!("member\t{}\t{}\tname\t{}", ti, mi, encode_nda_text(&m.name)));
            lines.push(format!("member\t{}\t{}\trole\t{}", ti, mi, encode_nda_text(&m.role)));
            lines.push(format!("member\t{}\t{}\tprovider\t{}", ti, mi, m.provider.slug()));
            lines.push(format!(
                "member\t{}\t{}\tmodel_id\t{}",
                ti,
                mi,
                encode_nda_text(&m.model_id)
            ));
            lines.push(format!(
                "member\t{}\t{}\tworkflow_instructions\t{}",
                ti,
                mi,
                encode_nda_text(&m.workflow_instructions)
            ));
            for skill in &m.skills {
                lines.push(format!(
                    "member_list\t{}\t{}\tskills\t{}",
                    ti,
                    mi,
                    encode_nda_text(skill)
                ));
            }
            for scope in &m.scope_patterns {
                lines.push(format!(
                    "member_list\t{}\t{}\tscope\t{}",
                    ti,
                    mi,
                    encode_nda_text(scope)
                ));
            }
            for tool in &m.tools {
                lines.push(format!(
                    "member_list\t{}\t{}\ttools\t{}",
                    ti,
                    mi,
                    encode_nda_text(tool)
                ));
            }
        }
    }
    lines.join("\n") + "\n"
}

#[derive(Default)]
struct MemberBuilder {
    id: String,
    name: String,
    role: String,
    provider: Option<AiProvider>,
    model_id: String,
    workflow_instructions: String,
    skills: Vec<String>,
    scope_patterns: Vec<String>,
    tools: Vec<String>,
}

#[derive(Default)]
struct TeamBuilder {
    id: String,
    name: String,
    description: String,
    is_preset: bool,
    members: std::collections::BTreeMap<usize, MemberBuilder>,
}

/// Parse teams from the NDA text convention produced by `serialize_expert_teams_nda`.
pub fn parse_expert_teams_nda(text: &str) -> Vec<ExpertTeam> {
    if !text.trim_start().starts_with("expert-teams version 1") {
        return Vec::new();
    }
    let mut teams: std::collections::BTreeMap<usize, TeamBuilder> = std::collections::BTreeMap::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("team\t") {
            let parts: Vec<&str> = rest.splitn(3, '\t').collect();
            if parts.len() != 3 {
                continue;
            }
            let Ok(ti) = parts[0].parse::<usize>() else { continue };
            let field = parts[1];
            let value = parts[2];
            let team = teams.entry(ti).or_default();
            match field {
                "id" => team.id = decode_nda_text(value),
                "name" => team.name = decode_nda_text(value),
                "description" => team.description = decode_nda_text(value),
                "is_preset" => team.is_preset = value.trim() == "true",
                _ => {}
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("member\t") {
            let parts: Vec<&str> = rest.splitn(4, '\t').collect();
            if parts.len() != 4 {
                continue;
            }
            let Ok(ti) = parts[0].parse::<usize>() else { continue };
            let Ok(mi) = parts[1].parse::<usize>() else { continue };
            let field = parts[2];
            let value = parts[3];
            let member = teams.entry(ti).or_default().members.entry(mi).or_default();
            match field {
                "id" => member.id = decode_nda_text(value),
                "name" => member.name = decode_nda_text(value),
                "role" => member.role = decode_nda_text(value),
                "provider" => member.provider = AiProvider::from_slug(value),
                "model_id" => member.model_id = decode_nda_text(value),
                "workflow_instructions" => member.workflow_instructions = decode_nda_text(value),
                _ => {}
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("member_list\t") {
            let parts: Vec<&str> = rest.splitn(4, '\t').collect();
            if parts.len() != 4 {
                continue;
            }
            let Ok(ti) = parts[0].parse::<usize>() else { continue };
            let Ok(mi) = parts[1].parse::<usize>() else { continue };
            let field = parts[2];
            let value = decode_nda_text(parts[3]);
            let member = teams.entry(ti).or_default().members.entry(mi).or_default();
            match field {
                "skills" => member.skills.push(value),
                "scope" => member.scope_patterns.push(value),
                "tools" => member.tools.push(value),
                _ => {}
            }
        }
    }

    teams
        .into_values()
        .map(|team| ExpertTeam {
            id: team.id,
            name: team.name,
            description: team.description,
            is_preset: team.is_preset,
            members: team
                .members
                .into_values()
                .map(|m| ExpertMember {
                    id: m.id,
                    name: m.name,
                    role: m.role,
                    provider: m.provider.unwrap_or(AiProvider::CloudflareWorkersAi),
                    model_id: m.model_id,
                    skills: m.skills,
                    scope_patterns: m.scope_patterns,
                    tools: m.tools,
                    workflow_instructions: m.workflow_instructions,
                })
                .collect(),
        })
        .collect()
}
