pub mod defaults;
pub mod nda_format;
pub mod registry;
pub mod types;

pub use registry::*;
pub use types::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn opens_with_default_templates() {
        let dir = tempfile::tempdir().unwrap();
        let registry = InstructionRegistry::open(dir.path());
        assert!(registry.for_kind(AgentTaskKind::Refactor).is_some());
        assert!(registry.policy_for_kind(AgentTaskKind::Refactor).is_some());
        assert!(registry.for_kind(AgentTaskKind::DesktopAutomation).is_some());
        assert!(registry.policy_for_kind(AgentTaskKind::DesktopAutomation).is_some());
        assert!(dir
            .path()
            .join(".velocity")
            .join("agentic")
            .join("instructions.nda")
            .exists());
        assert!(dir
            .path()
            .join(".velocity")
            .join("agentic")
            .join("instructions.json")
            .exists());
    }

    #[test]
    fn backfills_default_policies_for_legacy_registry_files() {
        let dir = tempfile::tempdir().unwrap();
        let storage_path = dir
            .path()
            .join(".velocity")
            .join("agentic")
            .join("instructions.json");
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
        assert_eq!(
            registry.get("refactor-guardian").unwrap().system_prompt,
            "legacy"
        );
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
            shared_expectations: vec![
                "Split refactor work per file when coupling is low.".to_string()
            ],
        });
        registry.set_preferred_policy(AgentTaskKind::Refactor, "refactor-isolated");
        registry.persist().unwrap();

        let reopened = InstructionRegistry::open(dir.path());
        assert_eq!(
            reopened.preferred_policy_id_for_kind(AgentTaskKind::Refactor),
            Some("refactor-isolated")
        );
        assert_eq!(
            reopened
                .policy_for_kind(AgentTaskKind::Refactor)
                .unwrap()
                .id,
            "refactor-isolated"
        );
    }

    #[test]
    fn prefers_nda_registry_over_json_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let agentic_dir = dir.path().join(".velocity").join("agentic");
        fs::create_dir_all(&agentic_dir).unwrap();
        fs::write(
            agentic_dir.join("instructions.nda"),
            "registry version 2\ntemplate_count 1\npolicy_count 0\npreferred_policy_count 0\ntemplate\trefactor-guardian\ntemplate_field\trefactor-guardian\tlabel\tNative\ntemplate_field\trefactor-guardian\ttask_kind\trefactor\ntemplate_field\trefactor-guardian\tsystem_prompt\tnative\ntemplate_checklist_count\trefactor-guardian\t0\n",
        )
        .unwrap();
        fs::write(
            agentic_dir.join("instructions.json"),
            r#"{
  "templates": [
    {
      "id": "refactor-guardian",
      "label": "Json",
      "task_kind": "refactor",
      "system_prompt": "json",
      "checklist": []
    }
  ]
}"#,
        )
        .unwrap();

        let registry = InstructionRegistry::open(dir.path());
        assert_eq!(registry.get("refactor-guardian").unwrap().label, "Native");
        assert_eq!(
            registry.get("refactor-guardian").unwrap().system_prompt,
            "native"
        );
    }
}
