use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTaskKind {
    Refactor,
    BugFix,
    Test,
    Documentation,
    Analysis,
    Planning,
    Merge,
}

impl AgentTaskKind {
    pub const ALL: [AgentTaskKind; 7] = [
        AgentTaskKind::Refactor,
        AgentTaskKind::BugFix,
        AgentTaskKind::Test,
        AgentTaskKind::Documentation,
        AgentTaskKind::Analysis,
        AgentTaskKind::Planning,
        AgentTaskKind::Merge,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            AgentTaskKind::Refactor => "refactor",
            AgentTaskKind::BugFix => "bug_fix",
            AgentTaskKind::Test => "test",
            AgentTaskKind::Documentation => "documentation",
            AgentTaskKind::Analysis => "analysis",
            AgentTaskKind::Planning => "planning",
            AgentTaskKind::Merge => "merge",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecompositionStyle {
    IsolatedFiles,
    CoupledComponents,
    SequentialPipeline,
}

impl DecompositionStyle {
    pub const ALL: [DecompositionStyle; 3] = [
        DecompositionStyle::IsolatedFiles,
        DecompositionStyle::CoupledComponents,
        DecompositionStyle::SequentialPipeline,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            DecompositionStyle::IsolatedFiles => "isolated_files",
            DecompositionStyle::CoupledComponents => "coupled_components",
            DecompositionStyle::SequentialPipeline => "sequential_pipeline",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionTemplate {
    pub id: String,
    pub label: String,
    pub task_kind: AgentTaskKind,
    pub system_prompt: String,
    pub checklist: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecompositionPolicy {
    pub id: String,
    pub label: String,
    pub task_kind: AgentTaskKind,
    pub instruction_template_id: String,
    pub decomposition_style: DecompositionStyle,
    pub shared_expectations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreferredPolicy {
    pub task_kind: AgentTaskKind,
    pub policy_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstructionRegistryFile {
    #[serde(default)]
    templates: Vec<InstructionTemplate>,
    #[serde(default)]
    policies: Vec<DecompositionPolicy>,
    #[serde(default)]
    preferred_policies: Vec<PreferredPolicy>,
}

#[derive(Debug, Clone)]
pub struct InstructionRegistry {
    storage_path: PathBuf,
    templates: Vec<InstructionTemplate>,
    policies: Vec<DecompositionPolicy>,
    preferred_policies: Vec<PreferredPolicy>,
}

impl InstructionRegistry {
    pub fn open(workspace_root: &Path) -> Self {
        let storage_path = workspace_root
            .join(".velocity")
            .join("agentic")
            .join("instructions.json");
        let (templates, policies, preferred_policies) = Self::load_registry(&storage_path)
            .map(|(templates, policies, preferred_policies)| {
                let templates = if templates.is_empty() { Self::default_templates() } else { templates };
                let policies = if policies.is_empty() { Self::default_policies() } else { policies };
                (templates, policies, preferred_policies)
            })
            .unwrap_or_else(|_| (Self::default_templates(), Self::default_policies(), Vec::new()));
        let registry = Self {
            storage_path,
            templates,
            policies,
            preferred_policies,
        };
        let _ = registry.ensure_persisted();
        registry
    }

    pub fn templates(&self) -> &[InstructionTemplate] {
        &self.templates
    }

    pub fn templates_for_kind(&self, kind: AgentTaskKind) -> Vec<&InstructionTemplate> {
        self.templates.iter().filter(|template| template.task_kind == kind).collect()
    }

    pub fn policies(&self) -> &[DecompositionPolicy] {
        &self.policies
    }

    pub fn policies_for_kind(&self, kind: AgentTaskKind) -> Vec<&DecompositionPolicy> {
        self.policies.iter().filter(|policy| policy.task_kind == kind).collect()
    }

    pub fn get(&self, id: &str) -> Option<&InstructionTemplate> {
        self.templates.iter().find(|template| template.id == id)
    }

    pub fn for_kind(&self, kind: AgentTaskKind) -> Option<&InstructionTemplate> {
        self.templates.iter().find(|template| template.task_kind == kind)
    }

    pub fn get_policy(&self, id: &str) -> Option<&DecompositionPolicy> {
        self.policies.iter().find(|policy| policy.id == id)
    }

    pub fn preferred_policy_id_for_kind(&self, kind: AgentTaskKind) -> Option<&str> {
        self.preferred_policies
            .iter()
            .find(|preferred| preferred.task_kind == kind)
            .map(|preferred| preferred.policy_id.as_str())
    }

    pub fn policy_for_kind(&self, kind: AgentTaskKind) -> Option<&DecompositionPolicy> {
        self.preferred_policy_id_for_kind(kind)
            .and_then(|policy_id| self.get_policy(policy_id))
            .filter(|policy| policy.task_kind == kind)
            .or_else(|| self.policies.iter().find(|policy| policy.task_kind == kind))
    }

    pub fn set_preferred_policy(&mut self, kind: AgentTaskKind, policy_id: impl Into<String>) {
        let policy_id = policy_id.into();
        if let Some(existing) = self.preferred_policies.iter_mut().find(|preferred| preferred.task_kind == kind) {
            existing.policy_id = policy_id;
        } else {
            self.preferred_policies.push(PreferredPolicy { task_kind: kind, policy_id });
        }
    }

    pub fn upsert(&mut self, template: InstructionTemplate) {
        if let Some(existing) = self.templates.iter_mut().find(|existing| existing.id == template.id) {
            *existing = template;
        } else {
            self.templates.push(template);
        }
    }

    pub fn upsert_policy(&mut self, policy: DecompositionPolicy) {
        if let Some(existing) = self.policies.iter_mut().find(|existing| existing.id == policy.id) {
            *existing = policy;
        } else {
            self.policies.push(policy);
        }
    }

    pub fn persist(&self) -> Result<(), String> {
        self.ensure_persisted()
    }

    fn ensure_persisted(&self) -> Result<(), String> {
        if let Some(parent) = self.storage_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Failed to create instruction registry directory: {e}"))?;
        }
        let payload = InstructionRegistryFile {
            templates: self.templates.clone(),
            policies: self.policies.clone(),
            preferred_policies: self.preferred_policies.clone(),
        };
        let json = serde_json::to_string_pretty(&payload)
            .map_err(|e| format!("Failed to serialize instruction registry: {e}"))?;
        fs::write(&self.storage_path, json)
            .map_err(|e| format!("Failed to write instruction registry: {e}"))
    }

    fn load_registry(path: &Path) -> Result<(Vec<InstructionTemplate>, Vec<DecompositionPolicy>, Vec<PreferredPolicy>), String> {
        let raw = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read instruction registry: {e}"))?;
        let file = serde_json::from_str::<InstructionRegistryFile>(&raw)
            .map_err(|e| format!("Failed to parse instruction registry: {e}"))?;
        Ok((file.templates, file.policies, file.preferred_policies))
    }

    fn default_templates() -> Vec<InstructionTemplate> {
        vec![
            InstructionTemplate {
                id: "planning-architect".to_string(),
                label: "Planning architect".to_string(),
                task_kind: AgentTaskKind::Planning,
                system_prompt: "You are a planning specialist. Break the request into dependency-aware implementation steps, identify risk, and preserve architectural constraints from the surrounding workspace.".to_string(),
                checklist: vec![
                    "State the intended outcome clearly.".to_string(),
                    "Split work into dependency-ordered tasks.".to_string(),
                    "Call out risky files and validation steps.".to_string(),
                ],
            },
            InstructionTemplate {
                id: "refactor-guardian".to_string(),
                label: "Refactor guardian".to_string(),
                task_kind: AgentTaskKind::Refactor,
                system_prompt: "You are a refactoring specialist. Improve structure without changing behavior, keep edits scoped, and respect existing architectural patterns and performance constraints.".to_string(),
                checklist: vec![
                    "Preserve observable behavior.".to_string(),
                    "Keep interfaces stable unless explicitly authorized.".to_string(),
                    "Leave code easier to reason about than before.".to_string(),
                ],
            },
            InstructionTemplate {
                id: "bugfix-responder".to_string(),
                label: "Bugfix responder".to_string(),
                task_kind: AgentTaskKind::BugFix,
                system_prompt: "You are a debugging specialist. Identify the smallest correct fix, explain the root cause, and avoid unrelated changes.".to_string(),
                checklist: vec![
                    "Find the root cause before editing.".to_string(),
                    "Patch only the required surface area.".to_string(),
                    "Validate the fix against the reported failure mode.".to_string(),
                ],
            },
            InstructionTemplate {
                id: "test-hardener".to_string(),
                label: "Test hardener".to_string(),
                task_kind: AgentTaskKind::Test,
                system_prompt: "You are a testing specialist. Strengthen confidence in the target change using the smallest effective validation available in the existing toolchain.".to_string(),
                checklist: vec![
                    "Prefer targeted validation first.".to_string(),
                    "Cover the modified behavior directly.".to_string(),
                    "Avoid introducing flaky or redundant checks.".to_string(),
                ],
            },
            InstructionTemplate {
                id: "analysis-cartographer".to_string(),
                label: "Analysis cartographer".to_string(),
                task_kind: AgentTaskKind::Analysis,
                system_prompt: "You are an analysis specialist. Build a precise map of the relevant code paths, dependencies, and constraints before recommending changes.".to_string(),
                checklist: vec![
                    "Trace the real execution path.".to_string(),
                    "Document important dependencies.".to_string(),
                    "Summarize actionable findings concisely.".to_string(),
                ],
            },
            InstructionTemplate {
                id: "docs-curator".to_string(),
                label: "Docs curator".to_string(),
                task_kind: AgentTaskKind::Documentation,
                system_prompt: "You are a documentation specialist. Explain behavior and architecture accurately, matching the project’s terminology and keeping docs synchronized with code.".to_string(),
                checklist: vec![
                    "Match code reality exactly.".to_string(),
                    "Prefer concise, high-signal wording.".to_string(),
                    "Update only directly related documentation.".to_string(),
                ],
            },
            InstructionTemplate {
                id: "merge-mediator".to_string(),
                label: "Merge mediator".to_string(),
                task_kind: AgentTaskKind::Merge,
                system_prompt: "You are a merge and compatibility specialist. Reconcile concurrent changes, preserve contracts, and produce structurally safe integration steps.".to_string(),
                checklist: vec![
                    "List the interfaces at risk.".to_string(),
                    "Prefer backward-compatible adapters when possible.".to_string(),
                    "State what must be revalidated after integration.".to_string(),
                ],
            },
        ]
    }

    fn default_policies() -> Vec<DecompositionPolicy> {
        vec![
            DecompositionPolicy {
                id: "planning-phased".to_string(),
                label: "Planning phased".to_string(),
                task_kind: AgentTaskKind::Planning,
                instruction_template_id: "planning-architect".to_string(),
                decomposition_style: DecompositionStyle::SequentialPipeline,
                shared_expectations: vec![
                    "Order tasks by dependency and architectural risk.".to_string(),
                    "Prefer producing a narrow set of high-confidence executable steps.".to_string(),
                ],
            },
            DecompositionPolicy {
                id: "refactor-coupled".to_string(),
                label: "Refactor coupled".to_string(),
                task_kind: AgentTaskKind::Refactor,
                instruction_template_id: "refactor-guardian".to_string(),
                decomposition_style: DecompositionStyle::CoupledComponents,
                shared_expectations: vec![
                    "Group files that share structural coupling or contracts.".to_string(),
                    "Minimize cross-agent interface churn during refactors.".to_string(),
                ],
            },
            DecompositionPolicy {
                id: "bugfix-focused".to_string(),
                label: "Bugfix focused".to_string(),
                task_kind: AgentTaskKind::BugFix,
                instruction_template_id: "bugfix-responder".to_string(),
                decomposition_style: DecompositionStyle::CoupledComponents,
                shared_expectations: vec![
                    "Keep root-cause analysis and the repair path together when files are tightly related.".to_string(),
                    "Avoid splitting a bug fix into isolated edits that hide causality.".to_string(),
                ],
            },
            DecompositionPolicy {
                id: "test-isolated".to_string(),
                label: "Test isolated".to_string(),
                task_kind: AgentTaskKind::Test,
                instruction_template_id: "test-hardener".to_string(),
                decomposition_style: DecompositionStyle::IsolatedFiles,
                shared_expectations: vec![
                    "Prefer targeted validation bundles per touched surface.".to_string(),
                    "Keep test additions close to the behavior they cover.".to_string(),
                ],
            },
            DecompositionPolicy {
                id: "analysis-coupled".to_string(),
                label: "Analysis coupled".to_string(),
                task_kind: AgentTaskKind::Analysis,
                instruction_template_id: "analysis-cartographer".to_string(),
                decomposition_style: DecompositionStyle::CoupledComponents,
                shared_expectations: vec![
                    "Trace connected execution paths together.".to_string(),
                    "Preserve enough context for downstream planning or fixes.".to_string(),
                ],
            },
            DecompositionPolicy {
                id: "docs-isolated".to_string(),
                label: "Docs isolated".to_string(),
                task_kind: AgentTaskKind::Documentation,
                instruction_template_id: "docs-curator".to_string(),
                decomposition_style: DecompositionStyle::IsolatedFiles,
                shared_expectations: vec![
                    "Prefer localized documentation edits unless broader concepts are shared.".to_string(),
                    "Keep terminology synchronized with the relevant code surface.".to_string(),
                ],
            },
            DecompositionPolicy {
                id: "merge-phased".to_string(),
                label: "Merge phased".to_string(),
                task_kind: AgentTaskKind::Merge,
                instruction_template_id: "merge-mediator".to_string(),
                decomposition_style: DecompositionStyle::SequentialPipeline,
                shared_expectations: vec![
                    "Sequence compatibility work before final integration.".to_string(),
                    "Prefer adapter steps when direct reconciliation would collide.".to_string(),
                ],
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_with_default_templates() {
        let dir = tempfile::tempdir().unwrap();
        let registry = InstructionRegistry::open(dir.path());
        assert!(registry.for_kind(AgentTaskKind::Refactor).is_some());
        assert!(registry.policy_for_kind(AgentTaskKind::Refactor).is_some());
        assert!(dir.path().join(".velocity").join("agentic").join("instructions.json").exists());
    }

    #[test]
    fn backfills_default_policies_for_legacy_registry_files() {
        let dir = tempfile::tempdir().unwrap();
        let storage_path = dir.path().join(".velocity").join("agentic").join("instructions.json");
        fs::create_dir_all(storage_path.parent().unwrap()).unwrap();
        fs::write(
            &storage_path,
            r#"{
  "templates": [
    {
      "id": "refactor-guardian",
      "label": "Refactor guardian",
      "task_kind": "refactor",
      "system_prompt": "legacy",
      "checklist": ["Preserve behavior"]
    }
  ]
}"#,
        )
        .unwrap();

        let registry = InstructionRegistry::open(dir.path());
        assert_eq!(registry.get("refactor-guardian").unwrap().system_prompt, "legacy");
        assert!(registry.policy_for_kind(AgentTaskKind::Refactor).is_some());
    }

    #[test]
    fn persists_preferred_policy_override() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = InstructionRegistry::open(dir.path());
        registry.upsert_policy(DecompositionPolicy {
            id: "refactor-isolated".to_string(),
            label: "Refactor isolated".to_string(),
            task_kind: AgentTaskKind::Refactor,
            instruction_template_id: "refactor-guardian".to_string(),
            decomposition_style: DecompositionStyle::IsolatedFiles,
            shared_expectations: vec!["Split refactor work per file when coupling is low.".to_string()],
        });
        registry.set_preferred_policy(AgentTaskKind::Refactor, "refactor-isolated");
        registry.persist().unwrap();

        let reopened = InstructionRegistry::open(dir.path());
        assert_eq!(
            reopened.preferred_policy_id_for_kind(AgentTaskKind::Refactor),
            Some("refactor-isolated")
        );
        assert_eq!(
            reopened.policy_for_kind(AgentTaskKind::Refactor).unwrap().id,
            "refactor-isolated"
        );
    }
}
