use crate::agent::nda::{decode_nda_text, encode_nda_text};
use crate::agent::AiProvider;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

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
    /// Optional fallback provider used when the primary provider is unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_provider: Option<AiProvider>,
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
            fallback_provider: None,
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

    pub fn resolve_effective_provider_and_model(
        &self,
        default_provider: AiProvider,
        default_model: &str,
    ) -> (AiProvider, String) {
        if self.model_id.trim().is_empty() {
            (default_provider, default_model.to_string())
        } else {
            (self.provider, self.model_id.clone())
        }
    }
}

/// Partial update for an `ExpertMember`. Each `Option` field, when `Some`,
/// replaces the corresponding field on the target member.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemberUpdate {
    pub name: Option<String>,
    pub role: Option<String>,
    pub provider: Option<String>,
    pub model_id: Option<String>,
    pub skills: Option<Vec<String>>,
    pub scope_patterns: Option<Vec<String>>,
    pub tools: Option<Vec<String>>,
    pub workflow_instructions: Option<String>,
}

impl MemberUpdate {
    /// Apply this update to a member, returning the list of fields that changed.
    pub fn apply(&self, member: &mut ExpertMember) -> Vec<&'static str> {
        let mut changed = Vec::new();
        if let Some(ref name) = self.name {
            if !name.trim().is_empty() && name != &member.name {
                member.name = name.trim().to_string();
                changed.push("name");
            }
        }
        if let Some(ref role) = self.role {
            if !role.trim().is_empty() && role != &member.role {
                member.role = role.trim().to_string();
                changed.push("role");
            }
        }
        if let Some(ref provider) = self.provider {
            if let Some(p) = AiProvider::from_slug(provider) {
                member.provider = p;
                changed.push("provider");
            }
        }
        if let Some(ref model_id) = self.model_id {
            let trimmed = model_id.trim().to_string();
            if trimmed != member.model_id {
                member.model_id = trimmed;
                changed.push("model_id");
            }
        }
        if let Some(ref skills) = self.skills {
            member.skills = skills.clone();
            changed.push("skills");
        }
        if let Some(ref scope_patterns) = self.scope_patterns {
            member.scope_patterns = scope_patterns.clone();
            changed.push("scope_patterns");
        }
        if let Some(ref tools) = self.tools {
            member.tools = tools.clone();
            changed.push("tools");
        }
        if let Some(ref instructions) = self.workflow_instructions {
            if instructions != &member.workflow_instructions {
                member.workflow_instructions = instructions.to_string();
                changed.push("workflow_instructions");
            }
        }
        changed
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
    pub fn new(
        id: &str,
        name: &str,
        description: &str,
        members: Vec<ExpertMember>,
        is_preset: bool,
    ) -> Self {
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
                || m.scope_patterns
                    .iter()
                    .any(|p| goal_lower.contains(&p.to_lowercase()))
        }) {
            return Some(member);
        }
        // 3. Fall back to lead member (first member)
        self.members.first()
    }

    /// Update the team name. Returns the old slug and new slug for change tracking.
    pub fn update_name(&mut self, new_name: &str) -> Option<(String, String)> {
        let trimmed = new_name.trim();
        if trimmed.is_empty() || trimmed == self.name {
            return None;
        }
        let old_slug = self.slug();
        self.name = trimmed.to_string();
        // Regenerate the team id from the new name
        self.id = format!("team_{}", self.slug());
        Some((old_slug, self.slug()))
    }

    /// Update the team description.
    pub fn update_description(&mut self, new_description: &str) -> bool {
        let trimmed = new_description.trim();
        if trimmed == self.description {
            return false;
        }
        self.description = trimmed.to_string();
        true
    }

    /// Add a member to the team. Returns `Err` if a member with the same id exists.
    pub fn add_member(&mut self, member: ExpertMember) -> Result<(), String> {
        if self.members.iter().any(|m| m.id == member.id) {
            return Err(format!("member id '{}' already exists", member.id));
        }
        self.members.push(member);
        Ok(())
    }

    /// Remove a member by id. Returns the removed member, or `None` if not found.
    pub fn remove_member(&mut self, member_id: &str) -> Option<ExpertMember> {
        if let Some(pos) = self.members.iter().position(|m| m.id == member_id) {
            Some(self.members.remove(pos))
        } else {
            None
        }
    }

    /// Apply a partial update to a member identified by `member_id`.
    /// Returns the list of fields that changed, or `Err` if member not found.
    pub fn update_member(
        &mut self,
        member_id: &str,
        update: &MemberUpdate,
    ) -> Result<Vec<&'static str>, String> {
        let member = self
            .members
            .iter_mut()
            .find(|m| m.id == member_id)
            .ok_or_else(|| format!("member '{}' not found", member_id))?;
        Ok(update.apply(member))
    }
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Scope Overlap Detection
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

/// A detected overlap between two members' scope patterns.
#[derive(Debug, Clone)]
pub struct ScopeOverlap {
    pub member_a_id: String,
    pub member_a_name: String,
    pub member_b_id: String,
    pub member_b_name: String,
    pub pattern_a: String,
    pub pattern_b: String,
}

/// Detect pairs of members whose scope patterns overlap (one contains the other).
/// Returns an empty vec when no overlaps exist.
pub fn detect_scope_overlaps(team: &ExpertTeam) -> Vec<ScopeOverlap> {
    let mut overlaps = Vec::new();
    for (i, a) in team.members.iter().enumerate() {
        for b in team.members.iter().skip(i + 1) {
            for pa in &a.scope_patterns {
                for pb in &b.scope_patterns {
                    if patterns_overlap(pa, pb) {
                        overlaps.push(ScopeOverlap {
                            member_a_id: a.id.clone(),
                            member_a_name: a.name.clone(),
                            member_b_id: b.id.clone(),
                            member_b_name: b.name.clone(),
                            pattern_a: pa.clone(),
                            pattern_b: pb.clone(),
                        });
                    }
                }
            }
        }
    }
    overlaps
}

/// True when two scope patterns overlap (one contains the other, case-insensitive).
fn patterns_overlap(a: &str, b: &str) -> bool {
    let al = a.to_lowercase();
    let bl = b.to_lowercase();
    al.contains(&bl) || bl.contains(&al)
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Team Composition Validation
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

/// Severity of a validation issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationSeverity {
    Error,
    Warning,
    Info,
}

/// A single validation issue found during team composition checks.
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    pub severity: ValidationSeverity,
    pub code: String,
    pub message: String,
}

/// Validate the composition of a team. Returns a list of issues found.
/// An empty list means the team is well-formed.
pub fn validate_team_composition(team: &ExpertTeam) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    // Rule 1: At least one member
    if team.members.is_empty() {
        issues.push(ValidationIssue {
            severity: ValidationSeverity::Error,
            code: "NO_MEMBERS".to_string(),
            message: "Team must have at least one member".to_string(),
        });
        return issues;
    }

    // Rule 2: First member (team lead) should have a broad scope
    if let Some(lead) = team.members.first() {
        let has_broad_scope = lead.scope_patterns.iter().any(|p| {
            let pl = p.to_lowercase();
            pl == "src/" || pl == "./" || pl == "*" || pl.is_empty()
        });
        if !has_broad_scope && !lead.scope_patterns.is_empty() {
            issues.push(ValidationIssue {
                severity: ValidationSeverity::Warning,
                code: "LEAD_NARROW_SCOPE".to_string(),
                message: format!(
                    "Team lead '{}' has a narrow scope. Consider adding a broad scope (e.g. 'src/') for fallback routing.",
                    lead.name
                ),
            });
        }
        if lead.scope_patterns.is_empty() {
            issues.push(ValidationIssue {
                severity: ValidationSeverity::Warning,
                code: "LEAD_NO_SCOPE".to_string(),
                message: format!(
                    "Team lead '{}' has no scope patterns. Tasks with no file match will still route here, but explicit scopes improve routing accuracy.",
                    lead.name
                ),
            });
        }
    }

    // Rule 3: No duplicate member names
    let mut names = std::collections::HashSet::new();
    for m in &team.members {
        if !names.insert(m.name.to_lowercase()) {
            issues.push(ValidationIssue {
                severity: ValidationSeverity::Error,
                code: "DUPLICATE_NAME".to_string(),
                message: format!("Duplicate member name: '{}'", m.name),
            });
        }
    }

    // Rule 4: No duplicate member ids
    let mut ids = std::collections::HashSet::new();
    for m in &team.members {
        if !ids.insert(m.id.to_lowercase()) {
            issues.push(ValidationIssue {
                severity: ValidationSeverity::Error,
                code: "DUPLICATE_ID".to_string(),
                message: format!("Duplicate member id: '{}'", m.id),
            });
        }
    }

    // Rule 5: All members must have a non-empty name and role
    for m in &team.members {
        if m.name.trim().is_empty() {
            issues.push(ValidationIssue {
                severity: ValidationSeverity::Error,
                code: "EMPTY_NAME".to_string(),
                message: format!("Member '{}' has an empty name", m.id),
            });
        }
        if m.role.trim().is_empty() {
            issues.push(ValidationIssue {
                severity: ValidationSeverity::Error,
                code: "EMPTY_ROLE".to_string(),
                message: format!("Member '{}' has an empty role", m.name),
            });
        }
    }

    // Rule 6: Members with empty model_id will use session defaults (info)
    for m in &team.members {
        if m.model_id.trim().is_empty() {
            issues.push(ValidationIssue {
                severity: ValidationSeverity::Info,
                code: "DEFAULT_MODEL".to_string(),
                message: format!(
                    "Member '{}' will use the session default provider/model",
                    m.name
                ),
            });
        }
    }

    // Rule 7: Scope overlap warnings
    let overlaps = detect_scope_overlaps(team);
    for overlap in &overlaps {
        issues.push(ValidationIssue {
            severity: ValidationSeverity::Warning,
            code: "SCOPE_OVERLAP".to_string(),
            message: format!(
                "Scope overlap between '{}' ({}) and '{}' ({}): '{}' â†” '{}'",
                overlap.member_a_name,
                overlap.member_a_id,
                overlap.member_b_name,
                overlap.member_b_id,
                overlap.pattern_a,
                overlap.pattern_b,
            ),
        });
    }

    issues
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Team Cloning
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

impl ExpertTeam {
    /// Create a deep copy of this team with a new name and regenerated ids.
    /// The `is_preset` flag is set to `false` for the clone.
    pub fn clone_with_name(&self, new_name: &str) -> ExpertTeam {
        let new_slug = slugify(new_name);
        let new_id = format!("team_{}", new_slug);
        let members = self
            .members
            .iter()
            .enumerate()
            .map(|(i, m)| ExpertMember {
                id: format!("member_{}_{}", new_slug, i + 1),
                name: m.name.clone(),
                role: m.role.clone(),
                provider: m.provider,
                model_id: m.model_id.clone(),
                skills: m.skills.clone(),
                scope_patterns: m.scope_patterns.clone(),
                tools: m.tools.clone(),
                workflow_instructions: m.workflow_instructions.clone(),
                fallback_provider: m.fallback_provider,
            })
            .collect();
        ExpertTeam {
            id: new_id,
            name: new_name.to_string(),
            description: self.description.clone(),
            members,
            is_preset: false,
        }
    }
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Import / Export
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

/// Serialize a team to a JSON string for export.
pub fn export_team_to_json(team: &ExpertTeam) -> Result<String, String> {
    serde_json::to_string_pretty(team).map_err(|e| format!("failed to serialize team: {}", e))
}

/// Deserialize a team from a JSON string for import.
/// Validates required fields and regenerates ids if necessary.
pub fn import_team_from_json(json_str: &str) -> Result<ExpertTeam, String> {
    let mut team: ExpertTeam =
        serde_json::from_str(json_str).map_err(|e| format!("invalid team JSON: {}", e))?;

    // Validate the imported team
    if team.name.trim().is_empty() {
        return Err("team name is required".to_string());
    }
    if team.members.is_empty() {
        return Err("team must have at least one member".to_string());
    }

    // Regenerate team id from name
    let slug = slugify(&team.name);
    if slug.is_empty() {
        return Err("team name must contain alphanumeric characters".to_string());
    }
    team.id = format!("team_{}", slug);
    team.is_preset = false;

    // Regenerate member ids if they're empty
    for (i, member) in team.members.iter_mut().enumerate() {
        if member.id.trim().is_empty() {
            member.id = format!("member_{}_{}", slug, i + 1);
        }
        if member.name.trim().is_empty() {
            return Err(format!("member {} has an empty name", i + 1));
        }
        if member.role.trim().is_empty() {
            return Err(format!("member {} has an empty role", i + 1));
        }
    }

    Ok(team)
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

        // Team 4: Rust Systems Programming Team
        ExpertTeam::new(
            "team_rust",
            "Rust Systems Programming Team",
            "High-performance Rust engineering team specializing in systems programming, async runtimes, and memory-safe infrastructure.",
            vec![
                ExpertMember::new(
                    "member_rust_lead",
                    "Lead Rust Systems Architect",
                    "Systems Architecture & Ownership Design",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["src/", "Cargo.toml", "lib.rs", "main.rs"],
                    "Enforce ownership semantics, lifetime safety, trait abstractions, and zero-cost design patterns.",
                ),
                ExpertMember::new(
                    "member_rust_async",
                    "Async Runtime & Concurrency Specialist",
                    "Tokio, Async/Await & Parallelism",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["src/async/", "src/runtime/", "src/concurrent/"],
                    "Design lock-free data structures, async task schedulers, channel-based communication, and safe FFI boundaries.",
                ),
                ExpertMember::new(
                    "member_rust_embedded",
                    "Embedded & No-Std Engineer",
                    "Bare-Metal & Embedded Rust",
                    AiProvider::Groq,
                    "llama-3.3-70b-specdec",
                    vec!["system_tools"],
                    vec!["src/no_std/", "src/embedded/", "src/hal/"],
                    "Build no_std crates, HAL abstractions, memory-mapped I/O drivers, and embedded test harnesses.",
                ),
                ExpertMember::new(
                    "member_rust_test",
                    "Rust QA & Property Testing Engineer",
                    "Testing & Verification",
                    AiProvider::LocalOllama,
                    "llama3.2",
                    vec!["system_tools"],
                    vec!["tests/", "benches/", "src/tests/"],
                    "Write comprehensive unit tests, proptest property-based tests, criterion benchmarks, and integration test suites.",
                ),
            ],
            true,
        ),

        // Team 5: Full-Stack Web Development Team
        ExpertTeam::new(
            "team_fullstack_web",
            "Full-Stack Web Development Team",
            "End-to-end web application team covering React frontends, Node.js APIs, PostgreSQL databases, and cloud deployment.",
            vec![
                ExpertMember::new(
                    "member_fullstack_lead",
                    "Full-Stack Tech Lead",
                    "Architecture & API Design",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["src/", "api/", "server/", "package.json"],
                    "Own system architecture, API contract design, database schema evolution, and cross-stack integration.",
                ),
                ExpertMember::new(
                    "member_fullstack_frontend",
                    "React & TypeScript Frontend Engineer",
                    "SPA & Component Architecture",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["src/components/", "src/pages/", "src/hooks/", "*.tsx", "*.ts"],
                    "Build React components with TypeScript, manage state with Zustand/Redux, and implement responsive layouts.",
                ),
                ExpertMember::new(
                    "member_fullstack_backend",
                    "Node.js & API Backend Developer",
                    "REST/GraphQL APIs & Middleware",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["src/api/", "src/routes/", "src/middleware/", "src/services/"],
                    "Implement Express/Fastify routes, Prisma ORM models, JWT auth middleware, and rate limiting.",
                ),
                ExpertMember::new(
                    "member_fullstack_db",
                    "PostgreSQL & Database Engineer",
                    "Schema Design & Query Optimization",
                    AiProvider::AzureOpenAi,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["migrations/", "src/db/", "prisma/", "*.sql"],
                    "Design normalized schemas, write efficient queries, manage migrations, and optimize connection pooling.",
                ),
            ],
            true,
        ),

        // Team 6: DevOps & Infrastructure Team
        ExpertTeam::new(
            "team_devops",
            "DevOps & Infrastructure Team",
            "Cloud-native infrastructure team specializing in CI/CD pipelines, container orchestration, and infrastructure-as-code.",
            vec![
                ExpertMember::new(
                    "member_devops_lead",
                    "DevOps Platform Lead",
                    "Infrastructure Architecture",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["infra/", "terraform/", "*.tf", "docker-compose.yml"],
                    "Design cloud architecture, manage Terraform state, and enforce infrastructure-as-code best practices.",
                ),
                ExpertMember::new(
                    "member_devops_cicd",
                    "CI/CD Pipeline Engineer",
                    "GitHub Actions & Build Automation",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec![".github/workflows/", "Jenkinsfile", ".gitlab-ci.yml", "Makefile"],
                    "Build and optimize CI/CD pipelines, manage build matrices, caching strategies, and deployment gates.",
                ),
                ExpertMember::new(
                    "member_devops_k8s",
                    "Kubernetes & Container Specialist",
                    "Container Orchestration & Helm",
                    AiProvider::OpenRouter,
                    "meta-llama/llama-3.3-70b-instruct",
                    vec!["system_tools"],
                    vec!["k8s/", "helm/", "docker/", "Dockerfile", "*.yaml"],
                    "Write Kubernetes manifests, Helm charts, Dockerfiles, and manage cluster autoscaling policies.",
                ),
                ExpertMember::new(
                    "member_devops_monitoring",
                    "Observability & Monitoring Engineer",
                    "Metrics, Logging & Alerting",
                    AiProvider::Groq,
                    "llama-3.3-70b-specdec",
                    vec!["system_tools"],
                    vec!["monitoring/", "prometheus/", "grafana/", "alerts/"],
                    "Configure Prometheus metrics, Grafana dashboards, Loki log aggregation, and PagerDuty alert routing.",
                ),
            ],
            true,
        ),

        // Team 7: iOS/Swift Development Team
        ExpertTeam::new(
            "team_ios",
            "iOS/Swift Development Team",
            "Native iOS engineering team building Swift/SwiftUI applications with Combine reactive patterns and Xcode build optimization.",
            vec![
                ExpertMember::new(
                    "member_ios_lead",
                    "iOS Lead Architect",
                    "Swift Architecture & Design Patterns",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["iOS/", "*.swift", "*.xcodeproj", "*.xcworkspace"],
                    "Enforce MVVM-C architecture, Swift concurrency (async/await), Combine reactive patterns, and protocol-oriented design.",
                ),
                ExpertMember::new(
                    "member_ios_ui",
                    "SwiftUI & UIKit Specialist",
                    "Declarative & Imperative UI",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["iOS/UI/", "iOS/Views/", "iOS/Components/"],
                    "Build SwiftUI views with custom modifiers, UIKit bridging, Core Animation transitions, and Dynamic Type support.",
                ),
                ExpertMember::new(
                    "member_ios_networking",
                    "Networking & Data Layer Engineer",
                    "API Integration & Persistence",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["iOS/Networking/", "iOS/Models/", "iOS/Persistence/"],
                    "Implement URLSession APIs, Codable DTOs, Core Data stacks, and Keychain secure storage.",
                ),
                ExpertMember::new(
                    "member_ios_qa",
                    "XCTest & UI Automation Engineer",
                    "Testing & Quality Assurance",
                    AiProvider::LocalOllama,
                    "llama3.2",
                    vec!["system_tools"],
                    vec!["iOS/Tests/", "iOS/UITests/", "iOS/SnapshotTests/"],
                    "Write XCTest unit tests, XCUITest automation, and snapshot tests for visual regression detection.",
                ),
            ],
            true,
        ),

        // Team 8: Security Audit Team
        ExpertTeam::new(
            "team_security",
            "Security Audit Team",
            "Application security team specializing in code auditing, dependency scanning, penetration testing, and compliance verification.",
            vec![
                ExpertMember::new(
                    "member_security_lead",
                    "Security Team Lead",
                    "Threat Modeling & Risk Assessment",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["src/", "Cargo.toml", "package.json", "SECURITY.md"],
                    "Conduct threat modeling, STRIDE analysis, risk scoring, and security architecture reviews.",
                ),
                ExpertMember::new(
                    "member_security_audit",
                    "Static Analysis & Code Auditor",
                    "SAST & Vulnerability Detection",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["src/", "lib/", "tests/"],
                    "Perform static analysis, identify OWASP Top 10 vulnerabilities, audit authentication flows, and review crypto implementations.",
                ),
                ExpertMember::new(
                    "member_security_deps",
                    "Dependency & Supply Chain Analyst",
                    "Vulnerability Scanning & SBOM",
                    AiProvider::OpenRouter,
                    "deepseek/deepseek-coder",
                    vec!["system_tools"],
                    vec!["Cargo.lock", "package-lock.json", "*.toml", "*.json"],
                    "Scan dependencies for CVEs, generate SBOMs, audit transitive dependencies, and manage advisory databases.",
                ),
                ExpertMember::new(
                    "member_security_compliance",
                    "Compliance & Standards Engineer",
                    "SOC2, HIPAA & Regulatory Compliance",
                    AiProvider::AzureOpenAi,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["docs/compliance/", "docs/policies/", "*.md"],
                    "Map controls to SOC2/HIPAA/GDPR requirements, generate compliance reports, and maintain audit trails.",
                ),
            ],
            true,
        ),

        // Team 9: Frontend/React UI Team
        ExpertTeam::new(
            "team_frontend",
            "Frontend/React UI Team",
            "Specialized frontend engineering team focused on React, TypeScript, design systems, accessibility, and performance optimization.",
            vec![
                ExpertMember::new(
                    "member_frontend_lead",
                    "Frontend Tech Lead",
                    "React Architecture & State Management",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["src/", "*.tsx", "*.ts", "*.jsx", "*.js"],
                    "Own React architecture decisions, state management strategy, code splitting, and bundle optimization.",
                ),
                ExpertMember::new(
                    "member_frontend_design",
                    "Design System & Component Library Engineer",
                    "UI Kit & Storybook",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["src/components/", "src/ui/", "src/design-system/", "*.stories.tsx"],
                    "Build reusable component libraries, maintain Storybook documentation, and enforce design token consistency.",
                ),
                ExpertMember::new(
                    "member_frontend_a11y",
                    "Accessibility & Performance Specialist",
                    "WCAG Compliance & Core Web Vitals",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["src/", "*.tsx", "*.css", "*.scss"],
                    "Audit WCAG 2.1 AA compliance, optimize Lighthouse scores, implement lazy loading, and reduce bundle size.",
                ),
                ExpertMember::new(
                    "member_frontend_testing",
                    "Frontend Testing & QA Engineer",
                    "Component & E2E Testing",
                    AiProvider::LocalOllama,
                    "llama3.2",
                    vec!["system_tools"],
                    vec!["src/__tests__/", "src/**/*.test.tsx", "e2e/", "cypress/"],
                    "Write React Testing Library tests, Cypress E2E specs, and visual regression tests with Chromatic.",
                ),
            ],
            true,
        ),

        // Team 10: Data Science & Analytics Team
        ExpertTeam::new(
            "team_data_science",
            "Data Science & Analytics Team",
            "Data science team specializing in statistical analysis, machine learning pipelines, data visualization, and business intelligence.",
            vec![
                ExpertMember::new(
                    "member_data_lead",
                    "Lead Data Scientist",
                    "ML Architecture & Statistical Modeling",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["notebooks/", "src/data/", "models/", "*.ipynb", "*.py"],
                    "Design ML pipelines, statistical experiments, feature engineering strategies, and model validation frameworks.",
                ),
                ExpertMember::new(
                    "member_data_analyst",
                    "Data Analyst & Visualization Specialist",
                    "SQL, Dashboards & Business Intelligence",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["src/analytics/", "dashboards/", "reports/", "*.sql"],
                    "Build SQL queries, create Tableau/PowerBI dashboards, generate business reports, and design KPI frameworks.",
                ),
                ExpertMember::new(
                    "member_data_ml",
                    "Machine Learning Engineer",
                    "Model Training & Deployment",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["src/ml/", "training/", "inference/", "*.py"],
                    "Implement PyTorch/TensorFlow models, optimize training pipelines, manage model versioning, and deploy inference endpoints.",
                ),
                ExpertMember::new(
                    "member_data_engineer",
                    "Data Engineer & Pipeline Architect",
                    "ETL, Data Lakes & Processing",
                    AiProvider::Groq,
                    "llama-3.3-70b-specdec",
                    vec!["system_tools"],
                    vec!["src/etl/", "pipelines/", "dags/", "*.py", "*.sql"],
                    "Build Airflow DAGs, Spark jobs, data lake architectures, and real-time streaming pipelines with Kafka/Flink.",
                ),
            ],
            true,
        ),

        // Team 11: Web Research & Intelligence Team
        ExpertTeam::new(
            "team_web_research",
            "Web Research & Intelligence Team",
            "OSINT and web research team leveraging browser automation for competitive intelligence, market research, and data collection.",
            vec![
                ExpertMember::new(
                    "member_research_lead",
                    "Research Director",
                    "Investigation Strategy & Analysis",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["research/", "reports/", "analysis/"],
                    "Design research methodologies, coordinate multi-source investigations, and synthesize intelligence reports.",
                ),
                ExpertMember::new(
                    "member_research_browser",
                    "Browser Automation & Scraping Specialist",
                    "Web Crawling & Data Extraction",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["scrapers/", "crawlers/", "extractors/"],
                    "Build browser automation workflows, handle CAPTCHAs, manage proxy rotation, and extract structured data from dynamic sites.",
                ),
                ExpertMember::new(
                    "member_research_osint",
                    "OSINT & Competitive Intelligence Analyst",
                    "Open Source Intelligence Gathering",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["osint/", "competitors/", "market/"],
                    "Conduct competitive analysis, monitor industry trends, track competitor moves, and build intelligence databases.",
                ),
                ExpertMember::new(
                    "member_research_verification",
                    "Fact-Checking & Verification Specialist",
                    "Source Validation & Credibility Assessment",
                    AiProvider::AzureOpenAi,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["verification/", "sources/", "factcheck/"],
                    "Verify source credibility, cross-reference claims, detect misinformation, and maintain evidence chains.",
                ),
            ],
            true,
        ),

        // Team 12: Business Process Automation Team
        ExpertTeam::new(
            "team_business_automation",
            "Business Process Automation Team",
            "RPA and workflow automation team using Windows automation to streamline business processes, document handling, and enterprise integrations.",
            vec![
                ExpertMember::new(
                    "member_bpa_lead",
                    "Automation Program Lead",
                    "Process Mining & Automation Strategy",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["processes/", "workflows/", "automation/"],
                    "Identify automation opportunities, design workflow architectures, and manage RPA program governance.",
                ),
                ExpertMember::new(
                    "member_bpa_windows",
                    "Windows Automation & RPA Developer",
                    "Desktop Application Automation",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["rpa/", "automations/", "scripts/"],
                    "Build Windows automation workflows, automate legacy desktop apps, handle file processing, and integrate with enterprise systems.",
                ),
                ExpertMember::new(
                    "member_bpa_integration",
                    "Integration & API Specialist",
                    "System Integration & Data Flow",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["integrations/", "connectors/", "apis/"],
                    "Design API integrations, build middleware connectors, orchestrate data flows between systems, and manage webhooks.",
                ),
                ExpertMember::new(
                    "member_bpa_analyst",
                    "Business Analyst & Process Designer",
                    "Requirements & Workflow Design",
                    AiProvider::AzureOpenAi,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["docs/", "requirements/", "design/"],
                    "Gather business requirements, map current processes, design future-state workflows, and calculate ROI for automation projects.",
                ),
            ],
            true,
        ),

        // Team 13: IT Operations & System Administration Team
        ExpertTeam::new(
            "team_it_ops",
            "IT Operations & System Administration Team",
            "Enterprise IT operations team managing Windows infrastructure, Active Directory, system monitoring, and automated remediation.",
            vec![
                ExpertMember::new(
                    "member_itops_lead",
                    "IT Operations Manager",
                    "Infrastructure Architecture & Governance",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["infra/", "docs/", "policies/"],
                    "Design IT infrastructure, establish operational procedures, manage vendor relationships, and ensure SLA compliance.",
                ),
                ExpertMember::new(
                    "member_itops_windows",
                    "Windows System Administrator",
                    "Active Directory & Windows Infrastructure",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["scripts/", "gpo/", "automation/"],
                    "Manage Active Directory, Group Policies, Windows Server infrastructure, PowerShell automation, and patch management.",
                ),
                ExpertMember::new(
                    "member_itops_monitoring",
                    "Monitoring & Incident Response Engineer",
                    "System Health & Alerting",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["monitoring/", "alerts/", "runbooks/"],
                    "Configure monitoring systems, manage alert routing, write incident runbooks, and lead incident response procedures.",
                ),
                ExpertMember::new(
                    "member_itops_security",
                    "IT Security & Compliance Administrator",
                    "Endpoint Security & Policy Enforcement",
                    AiProvider::AzureOpenAi,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["security/", "compliance/", "policies/"],
                    "Manage endpoint protection, enforce security policies, conduct vulnerability scans, and maintain compliance audits.",
                ),
            ],
            true,
        ),

        // Team 14: Drone Operations & Mission Control Team
        ExpertTeam::new(
            "team_drone_ops",
            "Drone Operations & Mission Control Team",
            "UAV operations team specializing in mission planning, flight control, telemetry analysis, and safety compliance for autonomous drone systems.",
            vec![
                ExpertMember::new(
                    "member_drone_lead",
                    "Drone Operations Director",
                    "Mission Planning & Flight Operations",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["missions/", "flight_plans/", "ops/"],
                    "Plan drone missions, coordinate flight operations, manage airspace permissions, and ensure regulatory compliance.",
                ),
                ExpertMember::new(
                    "member_drone_pilot",
                    "Autonomous Flight Control Engineer",
                    "Flight Dynamics & Control Systems",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["drone/src/", "control/", "navigation/"],
                    "Implement flight control algorithms, navigation systems, obstacle avoidance, and autonomous flight modes.",
                ),
                ExpertMember::new(
                    "member_drone_telemetry",
                    "Telemetry & Data Analysis Specialist",
                    "Flight Data & Performance Analytics",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["telemetry/", "analytics/", "logs/"],
                    "Analyze flight telemetry, monitor battery performance, track flight metrics, and generate operational reports.",
                ),
                ExpertMember::new(
                    "member_drone_safety",
                    "Safety & Compliance Officer",
                    "Risk Assessment & Regulatory Compliance",
                    AiProvider::AzureOpenAi,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["safety/", "compliance/", "docs/"],
                    "Conduct risk assessments, maintain safety protocols, ensure FAA/EASA compliance, and manage incident reports.",
                ),
            ],
            true,
        ),

        // Team 15: Content Creation & Marketing Team
        ExpertTeam::new(
            "team_content_marketing",
            "Content Creation & Marketing Team",
            "Digital marketing and content creation team producing blog posts, social media campaigns, SEO-optimized content, and brand messaging.",
            vec![
                ExpertMember::new(
                    "member_content_lead",
                    "Content Marketing Director",
                    "Content Strategy & Brand Voice",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["content/", "strategy/", "brand/"],
                    "Define content strategy, establish brand voice guidelines, plan editorial calendars, and measure content ROI.",
                ),
                ExpertMember::new(
                    "member_content_writer",
                    "Senior Content Writer & Editor",
                    "Blog Posts, Articles & Copywriting",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["blog/", "articles/", "copy/", "*.md"],
                    "Write engaging blog posts, long-form articles, whitepapers, case studies, and marketing copy aligned with brand voice.",
                ),
                ExpertMember::new(
                    "member_content_seo",
                    "SEO & Content Optimization Specialist",
                    "Search Optimization & Keyword Research",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["seo/", "keywords/", "analytics/"],
                    "Conduct keyword research, optimize content for search engines, analyze SEO metrics, and improve organic rankings.",
                ),
                ExpertMember::new(
                    "member_content_social",
                    "Social Media & Community Manager",
                    "Social Campaigns & Engagement",
                    AiProvider::Groq,
                    "llama-3.3-70b-specdec",
                    vec!["system_tools"],
                    vec!["social/", "campaigns/", "community/"],
                    "Create social media campaigns, manage community engagement, schedule posts, and track social metrics.",
                ),
            ],
            true,
        ),

        // Team 16: Research & Knowledge Management Team
        ExpertTeam::new(
            "team_research_km",
            "Research & Knowledge Management Team",
            "Academic and technical research team focused on literature reviews, knowledge base development, and research documentation.",
            vec![
                ExpertMember::new(
                    "member_research_km_lead",
                    "Research Program Director",
                    "Research Strategy & Knowledge Architecture",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["research/", "knowledge/", "strategy/"],
                    "Define research agendas, design knowledge management systems, and coordinate cross-functional research initiatives.",
                ),
                ExpertMember::new(
                    "member_research_km_analyst",
                    "Research Analyst & Literature Reviewer",
                    "Academic Research & Synthesis",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["papers/", "reviews/", "analysis/"],
                    "Conduct literature reviews, synthesize research findings, analyze academic papers, and produce research summaries.",
                ),
                ExpertMember::new(
                    "member_research_km_writer",
                    "Technical Writer & Documentation Specialist",
                    "Documentation & Knowledge Base Creation",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["docs/", "kb/", "guides/", "*.md"],
                    "Write technical documentation, create knowledge base articles, develop user guides, and maintain documentation standards.",
                ),
                ExpertMember::new(
                    "member_research_km_curator",
                    "Knowledge Base Curator & Taxonomist",
                    "Content Organization & Classification",
                    AiProvider::AzureOpenAi,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["taxonomy/", "classification/", "metadata/"],
                    "Design taxonomies, classify knowledge assets, maintain metadata standards, and ensure content discoverability.",
                ),
            ],
            true,
        ),

        // Team 17: Product Management & UX Research Team
        ExpertTeam::new(
            "team_product_ux",
            "Product Management & UX Research Team",
            "Product and UX team conducting user research, defining product requirements, designing user experiences, and managing product roadmaps.",
            vec![
                ExpertMember::new(
                    "member_product_lead",
                    "Senior Product Manager",
                    "Product Strategy & Roadmap Planning",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["product/", "roadmap/", "strategy/"],
                    "Define product vision, prioritize feature backlogs, manage stakeholder expectations, and drive product-market fit.",
                ),
                ExpertMember::new(
                    "member_product_uxr",
                    "UX Researcher & User Interviewer",
                    "User Research & Usability Testing",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["research/", "interviews/", "usability/"],
                    "Conduct user interviews, run usability tests, synthesize research findings, and create user personas.",
                ),
                ExpertMember::new(
                    "member_product_designer",
                    "UX/UI Designer & Prototyper",
                    "Interaction Design & Wireframing",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["design/", "wireframes/", "prototypes/"],
                    "Create wireframes, design user flows, build interactive prototypes, and establish design systems.",
                ),
                ExpertMember::new(
                    "member_product_analyst",
                    "Product Analyst & Metrics Specialist",
                    "Product Analytics & Data-Driven Decisions",
                    AiProvider::Groq,
                    "llama-3.3-70b-specdec",
                    vec!["system_tools"],
                    vec!["analytics/", "metrics/", "data/"],
                    "Analyze product metrics, track KPIs, conduct A/B tests, and provide data-driven product recommendations.",
                ),
            ],
            true,
        ),

        // Team 18: Quality Assurance & Test Automation Team
        ExpertTeam::new(
            "team_qa_automation",
            "Quality Assurance & Test Automation Team",
            "Comprehensive QA team covering manual testing, test automation, performance testing, and quality process improvement.",
            vec![
                ExpertMember::new(
                    "member_qa_lead",
                    "QA Manager & Test Strategist",
                    "Test Planning & Quality Processes",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["test_plans/", "strategy/", "processes/"],
                    "Define test strategies, establish quality gates, manage test environments, and coordinate release testing.",
                ),
                ExpertMember::new(
                    "member_qa_automation",
                    "Test Automation Engineer",
                    "Automation Frameworks & Scripting",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["tests/automation/", "frameworks/", "scripts/"],
                    "Build test automation frameworks, write automated test scripts, integrate tests into CI/CD, and maintain test infrastructure.",
                ),
                ExpertMember::new(
                    "member_qa_performance",
                    "Performance & Load Testing Specialist",
                    "Load Testing & Performance Analysis",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["tests/performance/", "load/", "benchmarks/"],
                    "Design load tests, conduct performance profiling, identify bottlenecks, and validate scalability requirements.",
                ),
                ExpertMember::new(
                    "member_qa_manual",
                    "Manual QA & Exploratory Tester",
                    "Exploratory Testing & Bug Discovery",
                    AiProvider::LocalOllama,
                    "llama3.2",
                    vec!["system_tools"],
                    vec!["tests/manual/", "exploratory/", "bugs/"],
                    "Perform exploratory testing, write detailed bug reports, validate edge cases, and conduct regression testing.",
                ),
            ],
            true,
        ),

        // Team 19: Financial Analysis & Trading Team
        ExpertTeam::new(
            "team_finance",
            "Financial Analysis & Trading Team",
            "Quantitative finance team specializing in market analysis, algorithmic trading strategies, financial modeling, and risk management.",
            vec![
                ExpertMember::new(
                    "member_finance_lead",
                    "Quantitative Finance Director",
                    "Trading Strategy & Portfolio Management",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["strategies/", "portfolio/", "analysis/"],
                    "Design trading strategies, manage portfolio risk, allocate capital, and oversee quantitative research initiatives.",
                ),
                ExpertMember::new(
                    "member_finance_quant",
                    "Quantitative Analyst & Model Developer",
                    "Algorithmic Trading & Statistical Arbitrage",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["models/", "algorithms/", "backtesting/"],
                    "Develop trading algorithms, build statistical models, conduct backtesting, and optimize execution strategies.",
                ),
                ExpertMember::new(
                    "member_finance_analyst",
                    "Financial Analyst & Market Researcher",
                    "Market Analysis & Fundamental Research",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["research/", "markets/", "reports/"],
                    "Conduct market research, analyze financial statements, track economic indicators, and produce investment reports.",
                ),
                ExpertMember::new(
                    "member_finance_risk",
                    "Risk Management & Compliance Analyst",
                    "Risk Assessment & Regulatory Compliance",
                    AiProvider::AzureOpenAi,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["risk/", "compliance/", "monitoring/"],
                    "Assess financial risks, monitor position limits, ensure regulatory compliance, and manage risk reporting.",
                ),
            ],
            true,
        ),

        // Team 20: Graphic Design & Branding Team
        ExpertTeam::new(
            "team_design_branding",
            "Graphic Design & Branding Team",
            "Creative design team specializing in brand identity, visual design, typography, and marketing collateral creation.",
            vec![
                ExpertMember::new(
                    "member_design_lead",
                    "Creative Director & Brand Strategist",
                    "Brand Identity & Visual Direction",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["brand/", "design/", "assets/"],
                    "Define brand strategy, establish visual identity systems, guide creative direction, and ensure brand consistency.",
                ),
                ExpertMember::new(
                    "member_design_graphic",
                    "Senior Graphic Designer",
                    "Visual Design & Layout",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["designs/", "layouts/", "graphics/"],
                    "Create logos, marketing collateral, social media graphics, print materials, and visual compositions.",
                ),
                ExpertMember::new(
                    "member_design_ui",
                    "UI/UX Visual Designer",
                    "Interface Design & Design Systems",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["ui/", "mockups/", "design_system/"],
                    "Design user interfaces, create design systems, build interactive prototypes, and establish UI patterns.",
                ),
                ExpertMember::new(
                    "member_design_motion",
                    "Motion Graphics & Animation Designer",
                    "Animation & Dynamic Visuals",
                    AiProvider::Groq,
                    "llama-3.3-70b-specdec",
                    vec!["system_tools"],
                    vec!["animations/", "motion/", "video/"],
                    "Create motion graphics, animated logos, video transitions, and dynamic visual content for marketing.",
                ),
            ],
            true,
        ),

        // Team 21: Video Production & Animation Team
        ExpertTeam::new(
            "team_video_production",
            "Video Production & Animation Team",
            "Video production team covering pre-production planning, filming, editing, visual effects, and post-production workflows.",
            vec![
                ExpertMember::new(
                    "member_video_lead",
                    "Video Production Director",
                    "Creative Vision & Project Management",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["projects/", "storyboards/", "scripts/"],
                    "Oversee video projects, manage production schedules, coordinate creative vision, and ensure delivery quality.",
                ),
                ExpertMember::new(
                    "member_video_editor",
                    "Video Editor & Post-Production Specialist",
                    "Editing, Color Grading & Sound Design",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["edits/", "timeline/", "color/", "audio/"],
                    "Edit video content, perform color grading, mix audio tracks, and deliver final cuts in multiple formats.",
                ),
                ExpertMember::new(
                    "member_video_vfx",
                    "Visual Effects & Compositing Artist",
                    "VFX, Green Screen & 3D Integration",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["vfx/", "compositing/", "3d/"],
                    "Create visual effects, composite green screen footage, integrate 3D elements, and enhance visual quality.",
                ),
                ExpertMember::new(
                    "member_video_animator",
                    "2D/3D Animator & Motion Designer",
                    "Character Animation & Motion Graphics",
                    AiProvider::Groq,
                    "llama-3.3-70b-specdec",
                    vec!["system_tools"],
                    vec!["animation/", "characters/", "motion/"],
                    "Animate characters, create explainer videos, build motion graphics, and produce animated content.",
                ),
            ],
            true,
        ),

        // Team 22: Game Development Team
        ExpertTeam::new(
            "team_game_dev",
            "Game Development Team",
            "Game development team building interactive experiences with Unity/Unreal Engine, game mechanics, and real-time rendering.",
            vec![
                ExpertMember::new(
                    "member_game_lead",
                    "Game Director & Lead Designer",
                    "Game Design & Creative Vision",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["design/", "levels/", "mechanics/"],
                    "Define game vision, design core mechanics, balance gameplay systems, and oversee production milestones.",
                ),
                ExpertMember::new(
                    "member_game_programmer",
                    "Gameplay Programmer & Engine Developer",
                    "C++/C# Programming & Engine Integration",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["src/", "engine/", "gameplay/", "*.cpp", "*.cs"],
                    "Implement gameplay systems, optimize engine performance, write shaders, and integrate physics/collision systems.",
                ),
                ExpertMember::new(
                    "member_game_artist",
                    "Game Artist & Environment Designer",
                    "3D Modeling, Texturing & Level Art",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["art/", "models/", "textures/", "environments/"],
                    "Create 3D models, texture assets, design environments, and produce concept art for game worlds.",
                ),
                ExpertMember::new(
                    "member_game_qa",
                    "Game QA & Playtest Coordinator",
                    "Testing, Bug Tracking & Balance Testing",
                    AiProvider::LocalOllama,
                    "llama3.2",
                    vec!["system_tools"],
                    vec!["tests/", "bugs/", "playtests/"],
                    "Conduct playtesting sessions, track bugs, test game balance, and validate player experience quality.",
                ),
            ],
            true,
        ),

        // Team 23: Mechanical Engineering Team
        ExpertTeam::new(
            "team_mechanical",
            "Mechanical Engineering Team",
            "Mechanical engineering team specializing in CAD design, finite element analysis, thermodynamics, and manufacturing processes.",
            vec![
                ExpertMember::new(
                    "member_mech_lead",
                    "Lead Mechanical Engineer",
                    "System Design & Engineering Architecture",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["cad/", "design/", "engineering/"],
                    "Architect mechanical systems, select materials, design mechanisms, and oversee engineering validation.",
                ),
                ExpertMember::new(
                    "member_mech_cad",
                    "CAD Designer & 3D Modeling Specialist",
                    "SolidWorks/AutoCAD & Parametric Design",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["cad/", "models/", "drawings/"],
                    "Create 3D CAD models, produce engineering drawings, design assemblies, and generate manufacturing specs.",
                ),
                ExpertMember::new(
                    "member_mech_fea",
                    "FEA & Simulation Engineer",
                    "Finite Element Analysis & Structural Simulation",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["simulation/", "analysis/", "fea/"],
                    "Run FEA simulations, analyze stress/strain, optimize structural integrity, and validate designs under load.",
                ),
                ExpertMember::new(
                    "member_mech_thermal",
                    "Thermal & Fluid Systems Engineer",
                    "Heat Transfer & Fluid Dynamics",
                    AiProvider::Groq,
                    "llama-3.3-70b-specdec",
                    vec!["system_tools"],
                    vec!["thermal/", "fluid/", "hvac/"],
                    "Design cooling systems, analyze heat transfer, optimize fluid flow, and manage thermal constraints.",
                ),
            ],
            true,
        ),

        // Team 24: Electrical Engineering Team
        ExpertTeam::new(
            "team_electrical",
            "Electrical Engineering Team",
            "Electrical engineering team focused on circuit design, PCB layout, embedded systems, and power electronics.",
            vec![
                ExpertMember::new(
                    "member_elec_lead",
                    "Lead Electrical Engineer",
                    "System Architecture & Circuit Design",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["circuits/", "design/", "schematics/"],
                    "Architect electrical systems, design circuit topologies, select components, and ensure EMC/EMI compliance.",
                ),
                ExpertMember::new(
                    "member_elec_pcb",
                    "PCB Designer & Layout Engineer",
                    "PCB Layout & Signal Integrity",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["pcb/", "layout/", "footprints/"],
                    "Design PCB layouts, route high-speed signals, manage impedance control, and generate Gerber files.",
                ),
                ExpertMember::new(
                    "member_elec_embedded",
                    "Embedded Systems Firmware Engineer",
                    "Microcontroller Programming & Firmware",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["firmware/", "embedded/", "drivers/", "*.c", "*.h"],
                    "Write embedded firmware, develop device drivers, implement communication protocols, and optimize power consumption.",
                ),
                ExpertMember::new(
                    "member_elec_power",
                    "Power Electronics & Battery Systems Engineer",
                    "Power Conversion & Energy Storage",
                    AiProvider::Groq,
                    "llama-3.3-70b-specdec",
                    vec!["system_tools"],
                    vec!["power/", "batteries/", "converters/"],
                    "Design power converters, manage battery systems, optimize efficiency, and implement safety protections.",
                ),
            ],
            true,
        ),

        // Team 25: Healthcare & Medical Research Team
        ExpertTeam::new(
            "team_healthcare",
            "Healthcare & Medical Research Team",
            "Medical research team conducting clinical studies, analyzing healthcare data, and developing evidence-based treatment protocols.",
            vec![
                ExpertMember::new(
                    "member_health_lead",
                    "Clinical Research Director",
                    "Study Design & Medical Strategy",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["studies/", "protocols/", "research/"],
                    "Design clinical studies, establish research protocols, ensure IRB compliance, and oversee medical strategy.",
                ),
                ExpertMember::new(
                    "member_health_analyst",
                    "Biostatistician & Clinical Data Analyst",
                    "Statistical Analysis & Clinical Trials",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["data/", "analysis/", "statistics/"],
                    "Analyze clinical trial data, perform statistical testing, generate survival curves, and validate results.",
                ),
                ExpertMember::new(
                    "member_health_regulatory",
                    "Regulatory Affairs & Compliance Specialist",
                    "FDA/EMA Compliance & Documentation",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["regulatory/", "compliance/", "submissions/"],
                    "Prepare regulatory submissions, ensure FDA/EMA compliance, maintain quality systems, and manage audits.",
                ),
                ExpertMember::new(
                    "member_health_writer",
                    "Medical Writer & Publication Specialist",
                    "Manuscript Preparation & Scientific Communication",
                    AiProvider::AzureOpenAi,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["manuscripts/", "publications/", "docs/"],
                    "Write clinical study reports, prepare journal manuscripts, create patient education materials, and manage publications.",
                ),
            ],
            true,
        ),

        // Team 26: Biotechnology & Life Sciences Team
        ExpertTeam::new(
            "team_biotech",
            "Biotechnology & Life Sciences Team",
            "Biotech research team specializing in genomics, protein engineering, drug discovery, and laboratory automation.",
            vec![
                ExpertMember::new(
                    "member_biotech_lead",
                    "Research Scientist & Lab Director",
                    "Experimental Design & Research Strategy",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["experiments/", "protocols/", "research/"],
                    "Design experiments, oversee lab operations, manage research pipelines, and ensure scientific rigor.",
                ),
                ExpertMember::new(
                    "member_biotech_genomics",
                    "Genomics & Bioinformatics Specialist",
                    "DNA/RNA Sequencing & Genomic Analysis",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["genomics/", "sequences/", "analysis/"],
                    "Analyze genomic data, perform sequence alignment, identify variants, and build bioinformatics pipelines.",
                ),
                ExpertMember::new(
                    "member_biotech_protein",
                    "Protein Engineering & Structural Biology Scientist",
                    "Protein Design & Molecular Modeling",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["proteins/", "modeling/", "structure/"],
                    "Design protein constructs, perform molecular dynamics simulations, analyze protein structures, and optimize expression.",
                ),
                ExpertMember::new(
                    "member_biotech_lab",
                    "Laboratory Automation & High-Throughput Screening Engineer",
                    "Lab Robotics & Assay Development",
                    AiProvider::Groq,
                    "llama-3.3-70b-specdec",
                    vec!["system_tools"],
                    vec!["automation/", "screening/", "assays/"],
                    "Develop automated assays, operate liquid handlers, optimize screening workflows, and manage lab robotics.",
                ),
            ],
            true,
        ),

        // Team 27: Legal Research & Contract Analysis Team
        ExpertTeam::new(
            "team_legal",
            "Legal Research & Contract Analysis Team",
            "Legal team conducting case research, analyzing contracts, reviewing compliance, and preparing legal documentation.",
            vec![
                ExpertMember::new(
                    "member_legal_lead",
                    "Senior Legal Counsel & Strategy Advisor",
                    "Legal Strategy & Risk Assessment",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["cases/", "strategy/", "opinions/"],
                    "Provide legal counsel, assess legal risks, develop case strategies, and oversee legal operations.",
                ),
                ExpertMember::new(
                    "member_legal_research",
                    "Legal Researcher & Case Analyst",
                    "Case Law Research & Legal Analysis",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["research/", "cases/", "precedents/"],
                    "Research case law, analyze legal precedents, prepare legal memos, and summarize court decisions.",
                ),
                ExpertMember::new(
                    "member_legal_contracts",
                    "Contract Analyst & Negotiation Specialist",
                    "Contract Review & Drafting",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["contracts/", "agreements/", "clauses/"],
                    "Review contracts, draft agreements, analyze terms and conditions, and support contract negotiations.",
                ),
                ExpertMember::new(
                    "member_legal_compliance",
                    "Compliance & Regulatory Affairs Analyst",
                    "Regulatory Compliance & Policy Review",
                    AiProvider::AzureOpenAi,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["compliance/", "regulations/", "policies/"],
                    "Ensure regulatory compliance, review policies, conduct compliance audits, and track regulatory changes.",
                ),
            ],
            true,
        ),

        // Team 28: Sales Automation & CRM Team
        ExpertTeam::new(
            "team_sales",
            "Sales Automation & CRM Team",
            "Sales operations team managing CRM systems, automating sales workflows, analyzing pipeline metrics, and optimizing conversion funnels.",
            vec![
                ExpertMember::new(
                    "member_sales_lead",
                    "Sales Operations Director",
                    "Sales Strategy & Revenue Operations",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["strategy/", "pipeline/", "forecasting/"],
                    "Define sales strategy, manage revenue operations, optimize sales processes, and forecast revenue.",
                ),
                ExpertMember::new(
                    "member_sales_crm",
                    "CRM Administrator & Automation Specialist",
                    "Salesforce/HubSpot Configuration & Workflow Automation",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["crm/", "workflows/", "automation/"],
                    "Configure CRM systems, build automation workflows, manage data integrity, and integrate sales tools.",
                ),
                ExpertMember::new(
                    "member_sales_analyst",
                    "Sales Analyst & Pipeline Optimization Specialist",
                    "Data Analysis & Conversion Metrics",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["analytics/", "metrics/", "reports/"],
                    "Analyze sales metrics, track conversion rates, identify pipeline bottlenecks, and optimize sales funnels.",
                ),
                ExpertMember::new(
                    "member_sales_enablement",
                    "Sales Enablement & Training Coordinator",
                    "Sales Training & Content Management",
                    AiProvider::Groq,
                    "llama-3.3-70b-specdec",
                    vec!["system_tools"],
                    vec!["training/", "content/", "enablement/"],
                    "Create sales training materials, manage sales content, coordinate onboarding, and enable sales success.",
                ),
            ],
            true,
        ),

        // Team 29: Customer Success & Support Team
        ExpertTeam::new(
            "team_customer_success",
            "Customer Success & Support Team",
            "Customer-facing team managing onboarding, providing technical support, tracking customer health, and driving retention.",
            vec![
                ExpertMember::new(
                    "member_cs_lead",
                    "Customer Success Director",
                    "Customer Strategy & Retention",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["strategy/", "accounts/", "retention/"],
                    "Define customer success strategy, manage enterprise accounts, reduce churn, and drive customer satisfaction.",
                ),
                ExpertMember::new(
                    "member_cs_manager",
                    "Customer Success Manager",
                    "Onboarding & Account Management",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["onboarding/", "accounts/", "health/"],
                    "Manage customer onboarding, conduct business reviews, monitor account health, and ensure customer success.",
                ),
                ExpertMember::new(
                    "member_cs_support",
                    "Technical Support Engineer",
                    "Troubleshooting & Issue Resolution",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["tickets/", "issues/", "solutions/"],
                    "Resolve technical issues, troubleshoot problems, create knowledge base articles, and escalate critical cases.",
                ),
                ExpertMember::new(
                    "member_cs_analyst",
                    "Customer Analytics & Voice of Customer Specialist",
                    "Customer Metrics & Feedback Analysis",
                    AiProvider::Groq,
                    "llama-3.3-70b-specdec",
                    vec!["system_tools"],
                    vec!["analytics/", "feedback/", "nps/"],
                    "Analyze customer metrics, track NPS/CSAT, synthesize customer feedback, and identify improvement opportunities.",
                ),
            ],
            true,
        ),

        // Team 30: Manufacturing Operations Team
        ExpertTeam::new(
            "team_manufacturing",
            "Manufacturing Operations Team",
            "Manufacturing team managing production planning, quality control, process optimization, and supply chain coordination.",
            vec![
                ExpertMember::new(
                    "member_mfg_lead",
                    "Manufacturing Operations Manager",
                    "Production Planning & Facility Management",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["production/", "planning/", "operations/"],
                    "Manage production schedules, optimize facility layout, coordinate resources, and ensure operational efficiency.",
                ),
                ExpertMember::new(
                    "member_mfg_quality",
                    "Quality Control & Assurance Engineer",
                    "Inspection, Testing & Defect Prevention",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["quality/", "inspection/", "testing/"],
                    "Implement quality systems, conduct inspections, perform root cause analysis, and prevent defects.",
                ),
                ExpertMember::new(
                    "member_mfg_process",
                    "Process Engineer & Continuous Improvement Specialist",
                    "Lean Manufacturing & Six Sigma",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["process/", "improvement/", "lean/"],
                    "Optimize manufacturing processes, implement lean principles, lead Six Sigma projects, and reduce waste.",
                ),
                ExpertMember::new(
                    "member_mfg_supply",
                    "Supply Chain & Procurement Coordinator",
                    "Material Planning & Vendor Management",
                    AiProvider::Groq,
                    "llama-3.3-70b-specdec",
                    vec!["system_tools"],
                    vec!["supply/", "procurement/", "vendors/"],
                    "Manage material requirements, coordinate procurement, negotiate with vendors, and ensure supply continuity.",
                ),
            ],
            true,
        ),

        // Team 31: Supply Chain & Logistics Team
        ExpertTeam::new(
            "team_supply_chain",
            "Supply Chain & Logistics Team",
            "Supply chain team managing inventory, warehouse operations, transportation logistics, and demand forecasting.",
            vec![
                ExpertMember::new(
                    "member_scm_lead",
                    "Supply Chain Director",
                    "End-to-End Supply Chain Strategy",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["strategy/", "network/", "optimization/"],
                    "Design supply chain networks, optimize end-to-end flows, manage risks, and drive supply chain transformation.",
                ),
                ExpertMember::new(
                    "member_scm_inventory",
                    "Inventory Planning & Control Analyst",
                    "Inventory Optimization & Stock Management",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["inventory/", "stock/", "planning/"],
                    "Optimize inventory levels, manage safety stock, conduct ABC analysis, and reduce carrying costs.",
                ),
                ExpertMember::new(
                    "member_scm_warehouse",
                    "Warehouse Operations & Distribution Manager",
                    "Warehouse Management & Order Fulfillment",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["warehouse/", "distribution/", "fulfillment/"],
                    "Manage warehouse operations, optimize picking/packing, coordinate distribution, and ensure on-time delivery.",
                ),
                ExpertMember::new(
                    "member_scm_logistics",
                    "Transportation & Logistics Coordinator",
                    "Freight Management & Route Optimization",
                    AiProvider::Groq,
                    "llama-3.3-70b-specdec",
                    vec!["system_tools"],
                    vec!["transport/", "logistics/", "routes/"],
                    "Coordinate transportation, optimize delivery routes, manage freight carriers, and track shipments.",
                ),
            ],
            true,
        ),

        // Team 32: E-commerce Operations Team
        ExpertTeam::new(
            "team_ecommerce",
            "E-commerce Operations Team",
            "E-commerce team managing online storefronts, product catalogs, order fulfillment, and customer experience optimization.",
            vec![
                ExpertMember::new(
                    "member_ecom_lead",
                    "E-commerce Operations Director",
                    "Online Store Strategy & P&L Management",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["strategy/", "storefront/", "pnl/"],
                    "Manage e-commerce P&L, define storefront strategy, optimize conversion funnels, and drive online revenue.",
                ),
                ExpertMember::new(
                    "member_ecom_catalog",
                    "Product Catalog & Merchandising Manager",
                    "Product Listings & Category Management",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["catalog/", "products/", "merchandising/"],
                    "Manage product catalogs, optimize listings, plan merchandising strategies, and organize product categories.",
                ),
                ExpertMember::new(
                    "member_ecom_marketing",
                    "Digital Marketing & Campaign Manager",
                    "Paid Ads, Email & Conversion Optimization",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["campaigns/", "ads/", "email/"],
                    "Run paid ad campaigns, manage email marketing, optimize landing pages, and drive customer acquisition.",
                ),
                ExpertMember::new(
                    "member_ecom_analytics",
                    "E-commerce Analytics & CRO Specialist",
                    "Data Analysis & Conversion Rate Optimization",
                    AiProvider::Groq,
                    "llama-3.3-70b-specdec",
                    vec!["system_tools"],
                    vec!["analytics/", "cro/", "ab_tests/"],
                    "Analyze e-commerce metrics, run A/B tests, optimize checkout flows, and improve conversion rates.",
                ),
            ],
            true,
        ),

        // Team 33: Aerospace Engineering Team
        ExpertTeam::new(
            "team_aerospace",
            "Aerospace Engineering Team",
            "Aerospace engineering team designing aircraft systems, spacecraft, avionics, and propulsion systems for aviation and space applications.",
            vec![
                ExpertMember::new(
                    "member_aero_lead",
                    "Chief Aerospace Engineer",
                    "System Architecture & Mission Design",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["design/", "missions/", "systems/"],
                    "Architect aerospace systems, define mission requirements, manage system integration, and ensure airworthiness.",
                ),
                ExpertMember::new(
                    "member_aero_aero",
                    "Aerodynamics & Flight Mechanics Engineer",
                    "CFD Analysis & Flight Performance",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["aero/", "cfd/", "performance/"],
                    "Perform CFD analysis, optimize aerodynamic shapes, analyze flight mechanics, and predict performance.",
                ),
                ExpertMember::new(
                    "member_aero_structures",
                    "Structures & Materials Engineer",
                    "Airframe Design & Composite Materials",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["structures/", "materials/", "composites/"],
                    "Design airframe structures, select materials, analyze composites, and ensure structural integrity.",
                ),
                ExpertMember::new(
                    "member_aero_propulsion",
                    "Propulsion & Power Systems Engineer",
                    "Engine Design & Thrust Systems",
                    AiProvider::Groq,
                    "llama-3.3-70b-specdec",
                    vec!["system_tools"],
                    vec!["propulsion/", "engines/", "power/"],
                    "Design propulsion systems, analyze engine performance, optimize thrust, and manage power systems.",
                ),
            ],
            true,
        ),

        // Team 34: Energy & Utilities Management Team
        ExpertTeam::new(
            "team_energy",
            "Energy & Utilities Management Team",
            "Energy sector team managing power generation, grid operations, renewable energy systems, and utility infrastructure.",
            vec![
                ExpertMember::new(
                    "member_energy_lead",
                    "Energy Operations Director",
                    "Power Generation & Grid Strategy",
                    AiProvider::Anthropic,
                    "claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["operations/", "generation/", "grid/"],
                    "Oversee power generation, manage grid operations, coordinate energy distribution, and ensure reliability.",
                ),
                ExpertMember::new(
                    "member_energy_renewable",
                    "Renewable Energy & Solar/Wind Systems Engineer",
                    "Clean Energy & Sustainability",
                    AiProvider::OpenAI,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["renewable/", "solar/", "wind/"],
                    "Design renewable energy systems, optimize solar/wind installations, manage energy storage, and drive sustainability.",
                ),
                ExpertMember::new(
                    "member_energy_grid",
                    "Smart Grid & Power Distribution Engineer",
                    "Grid Modernization & Load Management",
                    AiProvider::OpenRouter,
                    "anthropic/claude-3.5-sonnet",
                    vec!["system_tools"],
                    vec!["grid/", "distribution/", "smart/"],
                    "Design smart grid systems, manage power distribution, optimize load balancing, and implement grid automation.",
                ),
                ExpertMember::new(
                    "member_energy_compliance",
                    "Energy Compliance & Regulatory Affairs Specialist",
                    "Environmental Compliance & Permitting",
                    AiProvider::AzureOpenAi,
                    "gpt-4o",
                    vec!["system_tools"],
                    vec!["compliance/", "regulatory/", "permits/"],
                    "Ensure environmental compliance, manage permitting processes, track regulations, and maintain certifications.",
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
        lines.push(format!(
            "team\t{}\tname\t{}",
            ti,
            encode_nda_text(&team.name)
        ));
        lines.push(format!(
            "team\t{}\tdescription\t{}",
            ti,
            encode_nda_text(&team.description)
        ));
        lines.push(format!("team\t{}\tis_preset\t{}", ti, team.is_preset));
        lines.push(format!(
            "team\t{}\tmember_count\t{}",
            ti,
            team.members.len()
        ));
        for (mi, m) in team.members.iter().enumerate() {
            lines.push(format!(
                "member\t{}\t{}\tid\t{}",
                ti,
                mi,
                encode_nda_text(&m.id)
            ));
            lines.push(format!(
                "member\t{}\t{}\tname\t{}",
                ti,
                mi,
                encode_nda_text(&m.name)
            ));
            lines.push(format!(
                "member\t{}\t{}\trole\t{}",
                ti,
                mi,
                encode_nda_text(&m.role)
            ));
            lines.push(format!(
                "member\t{}\t{}\tprovider\t{}",
                ti,
                mi,
                m.provider.slug()
            ));
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
            if let Some(fallback) = &m.fallback_provider {
                lines.push(format!(
                    "member\t{}\t{}\tfallback_provider\t{}",
                    ti,
                    mi,
                    fallback.slug()
                ));
            }
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
    fallback_provider: Option<AiProvider>,
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
    let mut teams: std::collections::BTreeMap<usize, TeamBuilder> =
        std::collections::BTreeMap::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("team\t") {
            let parts: Vec<&str> = rest.splitn(3, '\t').collect();
            if parts.len() != 3 {
                continue;
            }
            let Ok(ti) = parts[0].parse::<usize>() else {
                continue;
            };
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
            let Ok(ti) = parts[0].parse::<usize>() else {
                continue;
            };
            let Ok(mi) = parts[1].parse::<usize>() else {
                continue;
            };
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
                "fallback_provider" => member.fallback_provider = AiProvider::from_slug(value),
                _ => {}
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("member_list\t") {
            let parts: Vec<&str> = rest.splitn(4, '\t').collect();
            if parts.len() != 4 {
                continue;
            }
            let Ok(ti) = parts[0].parse::<usize>() else {
                continue;
            };
            let Ok(mi) = parts[1].parse::<usize>() else {
                continue;
            };
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
                    fallback_provider: m.fallback_provider,
                })
                .collect(),
        })
        .collect()
}
