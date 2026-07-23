use std::fs;
use std::path::Path;
use serde::{Deserialize, Serialize};
use crate::agent::AiProvider;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExpertMember {
    pub id: String,
    pub name: String,
    pub role: String,
    pub provider: AiProvider,
    pub model_id: String,
    pub skills: Vec<String>,
    pub scope_patterns: Vec<String>,
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
            workflow_instructions: workflow_instructions.to_string(),
        }
    }

    pub fn matches_scope(&self, path: &str) -> bool {
        if self.scope_patterns.is_empty() {
            return false;
        }
        let lower_path = path.to_lowercase();
        self.scope_patterns.iter().any(|pattern| {
            let lower_pat = pattern.to_lowercase();
            lower_path.contains(&lower_pat) || lower_pat.contains(&lower_path)
        })
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

pub fn load_expert_teams(workspace_root: &Path) -> Vec<ExpertTeam> {
    let file_path = workspace_root.join(".velocity/expert_teams.json");
    if file_path.exists() {
        if let Ok(content) = fs::read_to_string(&file_path) {
            if let Ok(user_teams) = serde_json::from_str::<Vec<ExpertTeam>>(&content) {
                if !user_teams.is_empty() {
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
    let file_path = dir.join("expert_teams.json");
    if let Ok(json) = serde_json::to_string_pretty(teams) {
        fs::write(file_path, json).is_ok()
    } else {
        false
    }
}
