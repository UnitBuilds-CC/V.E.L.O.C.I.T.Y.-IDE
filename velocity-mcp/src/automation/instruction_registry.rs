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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionTemplate {
    pub id: String,
    pub label: String,
    pub task_kind: AgentTaskKind,
    pub system_prompt: String,
    pub checklist: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstructionRegistryFile {
    templates: Vec<InstructionTemplate>,
}

#[derive(Debug, Clone)]
pub struct InstructionRegistry {
    storage_path: PathBuf,
    templates: Vec<InstructionTemplate>,
}

impl InstructionRegistry {
    pub fn open(workspace_root: &Path) -> Self {
        let storage_path = workspace_root
            .join(".velocity")
            .join("agentic")
            .join("instructions.json");
        let templates = Self::load_templates(&storage_path).unwrap_or_else(|_| Self::default_templates());
        let registry = Self { storage_path, templates };
        let _ = registry.ensure_persisted();
        registry
    }

    pub fn templates(&self) -> &[InstructionTemplate] {
        &self.templates
    }

    pub fn get(&self, id: &str) -> Option<&InstructionTemplate> {
        self.templates.iter().find(|template| template.id == id)
    }

    pub fn for_kind(&self, kind: AgentTaskKind) -> Option<&InstructionTemplate> {
        self.templates.iter().find(|template| template.task_kind == kind)
    }

    pub fn upsert(&mut self, template: InstructionTemplate) {
        if let Some(existing) = self.templates.iter_mut().find(|existing| existing.id == template.id) {
            *existing = template;
        } else {
            self.templates.push(template);
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
        };
        let json = serde_json::to_string_pretty(&payload)
            .map_err(|e| format!("Failed to serialize instruction registry: {e}"))?;
        fs::write(&self.storage_path, json)
            .map_err(|e| format!("Failed to write instruction registry: {e}"))
    }

    fn load_templates(path: &Path) -> Result<Vec<InstructionTemplate>, String> {
        let raw = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read instruction registry: {e}"))?;
        let file = serde_json::from_str::<InstructionRegistryFile>(&raw)
            .map_err(|e| format!("Failed to parse instruction registry: {e}"))?;
        Ok(file.templates)
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_with_default_templates() {
        let dir = tempfile::tempdir().unwrap();
        let registry = InstructionRegistry::open(dir.path());
        assert!(registry.for_kind(AgentTaskKind::Refactor).is_some());
        assert!(dir.path().join(".velocity").join("agentic").join("instructions.json").exists());
    }
}
