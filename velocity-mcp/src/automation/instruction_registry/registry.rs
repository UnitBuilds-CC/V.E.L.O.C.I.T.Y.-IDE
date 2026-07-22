use super::defaults::*;
use super::nda_format::*;
use super::types::*;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct InstructionRegistry {
    nda_storage_path: PathBuf,
    json_storage_path: PathBuf,
    templates: Vec<InstructionTemplate>,
    policies: Vec<DecompositionPolicy>,
    preferred_policies: Vec<PreferredPolicy>,
}

impl InstructionRegistry {
    pub fn open(workspace_root: &Path) -> Self {
        let storage_dir = workspace_root.join(".velocity").join("agentic");
        let nda_storage_path = storage_dir.join("instructions.nda");
        let json_storage_path = storage_dir.join("instructions.json");
        let (templates, policies, preferred_policies) =
            Self::load_registry(&nda_storage_path, &json_storage_path)
                .map(|(templates, policies, preferred_policies)| {
                    let templates = if templates.is_empty() {
                        default_templates()
                    } else {
                        templates
                    };
                    let policies = if policies.is_empty() {
                        default_policies()
                    } else {
                        policies
                    };
                    (templates, policies, preferred_policies)
                })
                .unwrap_or_else(|_| (default_templates(), default_policies(), Vec::new()));
        let registry = Self {
            nda_storage_path,
            json_storage_path,
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
        self.templates
            .iter()
            .filter(|template| template.task_kind == kind)
            .collect()
    }

    pub fn policies(&self) -> &[DecompositionPolicy] {
        &self.policies
    }

    pub fn policies_for_kind(&self, kind: AgentTaskKind) -> Vec<&DecompositionPolicy> {
        self.policies
            .iter()
            .filter(|policy| policy.task_kind == kind)
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<&InstructionTemplate> {
        self.templates.iter().find(|template| template.id == id)
    }

    pub fn for_kind(&self, kind: AgentTaskKind) -> Option<&InstructionTemplate> {
        self.templates
            .iter()
            .find(|template| template.task_kind == kind)
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
        if let Some(existing) = self
            .preferred_policies
            .iter_mut()
            .find(|preferred| preferred.task_kind == kind)
        {
            existing.policy_id = policy_id;
        } else {
            self.preferred_policies.push(PreferredPolicy {
                task_kind: kind,
                policy_id,
            });
        }
    }

    pub fn upsert(&mut self, template: InstructionTemplate) {
        if let Some(existing) = self
            .templates
            .iter_mut()
            .find(|existing| existing.id == template.id)
        {
            *existing = template;
        } else {
            self.templates.push(template);
        }
    }

    pub fn upsert_policy(&mut self, policy: DecompositionPolicy) {
        if let Some(existing) = self
            .policies
            .iter_mut()
            .find(|existing| existing.id == policy.id)
        {
            *existing = policy;
        } else {
            self.policies.push(policy);
        }
    }

    pub fn persist(&self) -> Result<(), String> {
        self.ensure_persisted()
    }

    fn ensure_persisted(&self) -> Result<(), String> {
        if let Some(parent) = self.nda_storage_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create instruction registry directory: {e}"))?;
        }
        let nda = to_nda_string(&self.templates, &self.policies, &self.preferred_policies);
        fs::write(&self.nda_storage_path, nda)
            .map_err(|e| format!("Failed to write NDA instruction registry: {e}"))?;

        let payload = InstructionRegistryFile {
            templates: self.templates.clone(),
            policies: self.policies.clone(),
            preferred_policies: self.preferred_policies.clone(),
        };
        let json = serde_json::to_string_pretty(&payload)
            .map_err(|e| format!("Failed to serialize instruction registry JSON export: {e}"))?;
        fs::write(&self.json_storage_path, json)
            .map_err(|e| format!("Failed to write instruction registry JSON export: {e}"))
    }

    fn load_registry(
        nda_path: &Path,
        json_path: &Path,
    ) -> Result<
        (
            Vec<InstructionTemplate>,
            Vec<DecompositionPolicy>,
            Vec<PreferredPolicy>,
        ),
        String,
    > {
        if nda_path.exists() {
            let raw = fs::read_to_string(nda_path)
                .map_err(|e| format!("Failed to read NDA instruction registry: {e}"))?;
            return parse_nda_registry(&raw);
        }

        let raw = fs::read_to_string(json_path)
            .map_err(|e| format!("Failed to read instruction registry JSON fallback: {e}"))?;
        let file = serde_json::from_str::<InstructionRegistryFile>(&raw)
            .map_err(|e| format!("Failed to parse instruction registry JSON fallback: {e}"))?;
        Ok((file.templates, file.policies, file.preferred_policies))
    }
}
