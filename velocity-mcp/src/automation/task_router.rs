use std::path::{Path, PathBuf};

use velocity_ide::site_map::SiteMap;

use crate::agent::{AiProvider, ModelInfo};
use crate::automation::instruction_registry::{AgentTaskKind, DecompositionPolicy, DecompositionStyle, InstructionRegistry, InstructionTemplate};
use crate::automation::model_quality::{ModelCandidate, ModelQualityIndex};

#[derive(Debug, Clone)]
pub struct ProviderModelCatalog {
    pub provider: AiProvider,
    pub models: Vec<ModelInfo>,
}

#[derive(Debug, Clone)]
pub struct RoutedModelRoute {
    pub provider: AiProvider,
    pub model_id: String,
    pub model_label: String,
    pub thinking: bool,
    pub score: i32,
}

#[derive(Debug, Clone)]
pub struct RoutedSubAgentTask {
    pub task_id: String,
    pub files: Vec<PathBuf>,
    pub task_kind: AgentTaskKind,
    pub provider: AiProvider,
    pub model_id: String,
    pub model_label: String,
    pub thinking: bool,
    pub fallback_chain: Vec<RoutedModelRoute>,
    pub decomposition_policy_id: String,
    pub decomposition_style: DecompositionStyle,
    pub instruction_template_id: String,
    pub execution_contract: String,
    pub summary: String,
    pub rationale: String,
}

#[derive(Debug, Clone)]
pub struct SiteMapTaskRouter {
    instruction_registry: InstructionRegistry,
}

impl SiteMapTaskRouter {
    pub fn open(workspace_root: &Path) -> Self {
        Self {
            instruction_registry: InstructionRegistry::open(workspace_root),
        }
    }

    pub fn route_tasks(
        &self,
        goal: &str,
        kind: AgentTaskKind,
        files: &[PathBuf],
        site_map: &SiteMap,
        catalogs: &[ProviderModelCatalog],
    ) -> Vec<RoutedSubAgentTask> {
        let policy = self
            .instruction_registry
            .policy_for_kind(kind)
            .cloned()
            .unwrap_or_else(|| default_policy(kind));
        let template = self
            .instruction_registry
            .get(&policy.instruction_template_id)
            .or_else(|| self.instruction_registry.for_kind(kind))
            .or_else(|| self.instruction_registry.templates().first());
        let partitions = partition_files_by_policy(files, site_map, policy.decomposition_style);
        let ranked_models = rank_candidates(kind, catalogs);

        partitions
            .into_iter()
            .enumerate()
            .map(|(idx, partition)| {
                let fallback_chain = build_fallback_chain(&ranked_models);
                let selected = fallback_chain.first().cloned().unwrap_or_else(fallback_route);
                let instruction_template_id = template.map(|item| item.id.clone()).unwrap_or_else(|| "default".to_string());
                let task_id = format!("subagent-{:02}", idx + 1);
                let execution_contract = build_execution_contract(
                    &task_id,
                    goal,
                    kind,
                    &partition,
                    site_map,
                    template,
                    &selected,
                    &fallback_chain,
                    &policy,
                );
                let summary = build_summary(goal, kind, &partition, &selected, &policy);
                let rationale = build_rationale(kind, &partition, site_map, &selected, fallback_chain.len(), &policy);
                RoutedSubAgentTask {
                    task_id,
                    files: partition,
                    task_kind: kind,
                    provider: selected.provider,
                    model_id: selected.model_id.clone(),
                    model_label: selected.model_label.clone(),
                    thinking: selected.thinking,
                    fallback_chain,
                    decomposition_policy_id: policy.id.clone(),
                    decomposition_style: policy.decomposition_style,
                    instruction_template_id,
                    execution_contract,
                    summary,
                    rationale,
                }
            })
            .collect()
    }
}

pub fn partition_files_by_policy(files: &[PathBuf], site_map: &SiteMap, style: DecompositionStyle) -> Vec<Vec<PathBuf>> {
    match style {
        DecompositionStyle::IsolatedFiles => files.iter().map(|file| vec![file.clone()]).collect(),
        DecompositionStyle::CoupledComponents => partition_files_by_coupling(files, site_map),
        DecompositionStyle::SequentialPipeline => vec![files.to_vec()],
    }
}

pub fn partition_files_by_coupling(files: &[PathBuf], site_map: &SiteMap) -> Vec<Vec<PathBuf>> {
    let mut partitions: Vec<Vec<PathBuf>> = Vec::new();

    for file in files {
        let file_hash = path_identity_hash(file);
        let callers = site_map.get_callers(file_hash);
        let dependencies = site_map.get_dependencies(file_hash);

        let mut merged = false;
        for partition in &mut partitions {
            for other_file in partition.iter() {
                let other_hash = path_identity_hash(other_file);
                let reverse_callers = site_map.get_callers(other_hash);
                let reverse_dependencies = site_map.get_dependencies(other_hash);
                if callers.contains(&other_hash)
                    || reverse_callers.contains(&file_hash)
                    || dependencies.contains(&other_hash)
                    || reverse_dependencies.contains(&file_hash)
                {
                    partition.push(file.clone());
                    merged = true;
                    break;
                }
            }
            if merged {
                break;
            }
        }

        if !merged {
            partitions.push(vec![file.clone()]);
        }
    }

    partitions
}

fn rank_candidates(kind: AgentTaskKind, catalogs: &[ProviderModelCatalog]) -> Vec<ModelCandidate> {
    let mut ranked = Vec::new();
    for catalog in catalogs {
        ranked.extend(ModelQualityIndex::rank_models(kind, catalog.provider, &catalog.models));
    }
    ranked.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.label.cmp(&b.label)));
    ranked
}

fn build_execution_contract(
    task_id: &str,
    goal: &str,
    kind: AgentTaskKind,
    files: &[PathBuf],
    site_map: &SiteMap,
    template: Option<&InstructionTemplate>,
    model: &RoutedModelRoute,
    fallback_chain: &[RoutedModelRoute],
    policy: &DecompositionPolicy,
) -> String {
    let mut lines = vec![
        "contract version 1".to_string(),
        format!("task {} kind {}", task_id, kind.as_str()),
        format!("task {} goal {}", task_id, escape_contract_value(goal)),
        format!("task {} site_map_root {:016x}", task_id, site_map.root()),
        format!("task {} policy_id {}", task_id, escape_contract_value(&policy.id)),
        format!("task {} policy_label {}", task_id, escape_contract_value(&policy.label)),
        format!("task {} decomposition_style {}", task_id, policy.decomposition_style.as_str()),
        format!("task {} route_provider {}", task_id, escape_contract_value(model.provider.label())),
        format!("task {} route_model {}", task_id, escape_contract_value(&model.model_label)),
        format!("task {} route_model_id {}", task_id, escape_contract_value(&model.model_id)),
        format!("task {} route_thinking {}", task_id, if model.thinking { "true" } else { "false" }),
    ];

    if let Some(template) = template {
        lines.push(format!("task {} template_id {}", task_id, escape_contract_value(&template.id)));
        lines.push(format!("task {} system_prompt {}", task_id, escape_contract_value(&template.system_prompt)));
        for item in &template.checklist {
            lines.push(format!("task {} checklist {}", task_id, escape_contract_value(item)));
        }
    }

    for route in fallback_chain {
        lines.push(format!(
            "task {} fallback {}|{}|{}|{}|{}",
            task_id,
            escape_contract_value(route.provider.label()),
            escape_contract_value(&route.model_label),
            escape_contract_value(&route.model_id),
            route.score,
            if route.thinking { "true" } else { "false" }
        ));
    }

    for file in files {
        lines.push(format!("task {} scope_file {}", task_id, escape_contract_value(&file.display().to_string())));
    }

    lines.push(format!(
        "task {} structural_expectation {}",
        task_id,
        escape_contract_value("Respect the live Merkle SiteMap and avoid edits outside the assigned scope.")
    ));
    lines.push(format!(
        "task {} structural_expectation {}",
        task_id,
        escape_contract_value("Preserve compatibility for any caller/dependency relationships touching this scope.")
    ));
    lines.push(format!(
        "task {} structural_expectation {}",
        task_id,
        escape_contract_value("Return concise notes suitable for reconciliation and validation.")
    ));

    for expectation in &policy.shared_expectations {
        lines.push(format!("task {} policy_expectation {}", task_id, escape_contract_value(expectation)));
    }

    lines.join("\n") + "\n"
}

fn build_summary(
    goal: &str,
    kind: AgentTaskKind,
    files: &[PathBuf],
    model: &RoutedModelRoute,
    policy: &DecompositionPolicy,
) -> String {
    let scope = if files.is_empty() {
        "no scoped files".to_string()
    } else {
        files
            .iter()
            .map(|file| file.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "{} task for '{}' using policy '{}' ({}) on {} via {} / {}.",
        kind.as_str(),
        goal,
        policy.label,
        policy.decomposition_style.as_str(),
        scope,
        model.provider.label(),
        model.model_label,
    )
}

fn build_rationale(
    kind: AgentTaskKind,
    files: &[PathBuf],
    site_map: &SiteMap,
    model: &RoutedModelRoute,
    fallback_count: usize,
    policy: &DecompositionPolicy,
) -> String {
    let mut coupling_edges = 0usize;
    for file in files {
        let file_hash = path_identity_hash(file);
        coupling_edges += site_map.get_callers(file_hash).len();
        coupling_edges += site_map.get_dependencies(file_hash).len();
    }

    format!(
        "Task kind '{}' routed via policy '{}' ({}) to {} / {} with score {} across {} file(s), {} observed coupling edge(s), and {} route candidate(s).",
        kind.as_str(),
        policy.label,
        policy.decomposition_style.as_str(),
        model.provider.label(),
        model.model_label,
        model.score,
        files.len(),
        coupling_edges,
        fallback_count,
    )
}

fn default_policy(kind: AgentTaskKind) -> DecompositionPolicy {
    DecompositionPolicy {
        id: format!("{}-default", kind.as_str()),
        label: format!("{} default", kind.as_str()),
        task_kind: kind,
        instruction_template_id: "default".to_string(),
        decomposition_style: DecompositionStyle::CoupledComponents,
        shared_expectations: vec!["Stay within declared scope and preserve structural integrity.".to_string()],
    }
}

fn build_fallback_chain(ranked_models: &[ModelCandidate]) -> Vec<RoutedModelRoute> {
    let mut chain = Vec::new();
    for candidate in ranked_models {
        if chain.iter().any(|route: &RoutedModelRoute| route.provider == candidate.provider && route.model_id == candidate.model_id) {
            continue;
        }
        chain.push(RoutedModelRoute {
            provider: candidate.provider,
            model_id: candidate.model_id.clone(),
            model_label: candidate.label.clone(),
            thinking: candidate.supports_thinking,
            score: candidate.score,
        });
        if chain.len() >= 3 {
            break;
        }
    }
    if chain.is_empty() {
        chain.push(fallback_route());
    }
    chain
}

fn fallback_route() -> RoutedModelRoute {
    RoutedModelRoute {
        provider: AiProvider::CloudflareWorkersAi,
        model_id: "auto".to_string(),
        model_label: "auto".to_string(),
        thinking: false,
        score: 0,
    }
}

fn escape_contract_value(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn path_identity_hash(path: &Path) -> u64 {
    let canonical = canonicalize_scope_path(path);
    hash_str(&canonical)
}

fn canonicalize_scope_path(path: &Path) -> String {
    let mut normalized = Vec::new();
    for component in path.components() {
        use std::path::Component;
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.push("..".to_string());
            }
            Component::Normal(part) => normalized.push(part.to_string_lossy().replace('\\', "/")),
            Component::RootDir | Component::Prefix(_) => {}
        }
    }
    normalized.join("/")
}

fn hash_str(s: &str) -> u64 {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    let d = h.finalize();
    u64::from_le_bytes(d[..8].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{ApiStyle, ModelInfo};
    use velocity_ide::site_map::NdaNode;

    #[test]
    fn routes_coupled_files_together() {
        let temp = tempfile::tempdir().unwrap();
        let mut sm = SiteMap::open(temp.path(), 0).unwrap();
        let a_hash = path_identity_hash(Path::new("src/a.rs"));
        let b_hash = path_identity_hash(Path::new("src/b.rs"));
        sm.put_node(&NdaNode::Triple {
            subject_hash: a_hash,
            predicate_id: 2,
            object_hash: b_hash,
        })
        .unwrap();

        let router = SiteMapTaskRouter::open(temp.path());
        let routes = router.route_tasks(
            "Refactor coupled files",
            AgentTaskKind::Refactor,
            &[PathBuf::from("src/a.rs"), PathBuf::from("src/b.rs")],
            &sm,
            &[ProviderModelCatalog {
                provider: AiProvider::CloudflareWorkersAi,
                models: vec![ModelInfo {
                    id: "cf/kimi-k2".to_string(),
                    label: "kimi-k2".to_string(),
                    api_style: ApiStyle::OpenAiTools,
                    supports_tools: true,
                    supports_thinking: true,
                }],
            }],
        );

        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].files.len(), 2);
        assert_eq!(routes[0].provider, AiProvider::CloudflareWorkersAi);
        assert!(!routes[0].fallback_chain.is_empty());
        assert_eq!(routes[0].fallback_chain[0].model_label, "kimi-k2");
        assert_eq!(routes[0].decomposition_policy_id, "refactor-coupled");
        assert_eq!(routes[0].decomposition_style, DecompositionStyle::CoupledComponents);
        assert!(routes[0].execution_contract.contains("contract version 1"));
        assert!(routes[0].execution_contract.contains("policy_label Refactor coupled"));
        assert!(routes[0].summary.contains("Refactor coupled"));
    }

    #[test]
    fn same_named_files_in_different_directories_do_not_collide() {
        let temp = tempfile::tempdir().unwrap();
        let mut sm = SiteMap::open(temp.path(), 0).unwrap();
        let caller_hash = path_identity_hash(Path::new("src/feature/a.rs"));
        let callee_hash = path_identity_hash(Path::new("src/shared/a.rs"));
        sm.put_node(&NdaNode::Triple {
            subject_hash: caller_hash,
            predicate_id: 2,
            object_hash: callee_hash,
        })
        .unwrap();

        let partitions = partition_files_by_coupling(
            &[
                PathBuf::from("src/feature/a.rs"),
                PathBuf::from("src/shared/a.rs"),
                PathBuf::from("src/other/a.rs"),
            ],
            &sm,
        );

        assert_eq!(partitions.len(), 2);
        assert!(partitions.iter().any(|group| {
            group.len() == 2
                && group.contains(&PathBuf::from("src/feature/a.rs"))
                && group.contains(&PathBuf::from("src/shared/a.rs"))
        }));
        assert!(partitions
            .iter()
            .any(|group| group == &vec![PathBuf::from("src/other/a.rs")]));
    }

    #[test]
    fn test_test_tasks_use_isolated_file_policy() {
        let temp = tempfile::tempdir().unwrap();
        let sm = SiteMap::open(temp.path(), 0).unwrap();
        let router = SiteMapTaskRouter::open(temp.path());
        let routes = router.route_tasks(
            "Add coverage",
            AgentTaskKind::Test,
            &[PathBuf::from("src/a.rs"), PathBuf::from("src/b.rs")],
            &sm,
            &[ProviderModelCatalog {
                provider: AiProvider::CloudflareWorkersAi,
                models: vec![ModelInfo {
                    id: "cf/kimi-k2".to_string(),
                    label: "kimi-k2".to_string(),
                    api_style: ApiStyle::OpenAiTools,
                    supports_tools: true,
                    supports_thinking: true,
                }],
            }],
        );

        assert_eq!(routes.len(), 2);
        assert!(routes.iter().all(|route| route.decomposition_policy_id == "test-isolated"));
        assert!(routes.iter().all(|route| route.decomposition_style == DecompositionStyle::IsolatedFiles));
        assert!(routes.iter().all(|route| route.files.len() == 1));
    }

    #[test]
    fn honors_preferred_policy_override_for_kind() {
        let temp = tempfile::tempdir().unwrap();
        let sm = SiteMap::open(temp.path(), 0).unwrap();
        let mut registry = InstructionRegistry::open(temp.path());
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

        let router = SiteMapTaskRouter::open(temp.path());
        let routes = router.route_tasks(
            "Refactor with isolation",
            AgentTaskKind::Refactor,
            &[PathBuf::from("src/a.rs"), PathBuf::from("src/b.rs")],
            &sm,
            &[ProviderModelCatalog {
                provider: AiProvider::CloudflareWorkersAi,
                models: vec![ModelInfo {
                    id: "cf/kimi-k2".to_string(),
                    label: "kimi-k2".to_string(),
                    api_style: ApiStyle::OpenAiTools,
                    supports_tools: true,
                    supports_thinking: true,
                }],
            }],
        );

        assert_eq!(routes.len(), 2);
        assert!(routes.iter().all(|route| route.decomposition_policy_id == "refactor-isolated"));
        assert!(routes.iter().all(|route| route.decomposition_style == DecompositionStyle::IsolatedFiles));
    }
}
