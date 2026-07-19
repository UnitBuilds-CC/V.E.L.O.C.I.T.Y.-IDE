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
    pub instructions: String,
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
                let instructions = build_instruction_payload(goal, kind, &partition, site_map, template, &selected, &fallback_chain, &policy);
                let rationale = build_rationale(kind, &partition, site_map, &selected, fallback_chain.len(), &policy);
                RoutedSubAgentTask {
                    task_id: format!("subagent-{:02}", idx + 1),
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
                    instructions,
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
        let file_name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let file_hash = hash_str(file_name);
        let callers = site_map.get_callers(file_hash);
        let dependencies = site_map.get_dependencies(file_hash);

        let mut merged = false;
        for partition in &mut partitions {
            for other_file in partition.iter() {
                let other_name = other_file.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let other_hash = hash_str(other_name);
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

fn build_instruction_payload(
    goal: &str,
    kind: AgentTaskKind,
    files: &[PathBuf],
    site_map: &SiteMap,
    template: Option<&InstructionTemplate>,
    model: &RoutedModelRoute,
    fallback_chain: &[RoutedModelRoute],
    policy: &DecompositionPolicy,
) -> String {
    let mut out = String::new();
    if let Some(template) = template {
        out.push_str("SYSTEM ROLE:\n");
        out.push_str(&template.system_prompt);
        out.push_str("\n\nCHECKLIST:\n");
        for item in &template.checklist {
            out.push_str("- ");
            out.push_str(item);
            out.push('\n');
        }
    }
    out.push_str("\nTASK KIND: ");
    out.push_str(kind.as_str());
    out.push_str("\nDECOMPOSITION POLICY: ");
    out.push_str(&policy.label);
    out.push_str(" (");
    out.push_str(policy.decomposition_style.as_str());
    out.push_str(")");
    out.push_str("\nGOAL: ");
    out.push_str(goal);
    out.push_str("\nSITE MAP ROOT: ");
    out.push_str(&format!("{:016x}", site_map.root()));
    out.push_str("\nMODEL ROUTE: ");
    out.push_str(model.provider.label());
    out.push_str(" :: ");
    out.push_str(&model.model_label);
    out.push_str("\nFALLBACK CHAIN:\n");
    for route in fallback_chain {
        out.push_str("- ");
        out.push_str(route.provider.label());
        out.push_str(" :: ");
        out.push_str(&route.model_label);
        out.push_str(" (score ");
        out.push_str(&route.score.to_string());
        out.push_str(")\n");
    }
    out.push_str("\nSCOPE FILES:\n");
    for file in files {
        out.push_str("- ");
        out.push_str(&file.display().to_string());
        out.push('\n');
    }
    out.push_str("\nSTRUCTURAL EXPECTATIONS:\n");
    out.push_str("- Respect the live Merkle SiteMap and avoid edits outside the assigned scope.\n");
    out.push_str("- Preserve compatibility for any caller/dependency relationships touching this scope.\n");
    out.push_str("- Return concise notes suitable for reconciliation and validation.\n");
    if !policy.shared_expectations.is_empty() {
        out.push_str("\nPOLICY EXPECTATIONS:\n");
        for expectation in &policy.shared_expectations {
            out.push_str("- ");
            out.push_str(expectation);
            out.push('\n');
        }
    }
    out
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
        let file_name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let file_hash = hash_str(file_name);
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
        let a_hash = hash_str("a.rs");
        let b_hash = hash_str("b.rs");
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
        assert!(routes[0].instructions.contains("DECOMPOSITION POLICY: Refactor coupled"));
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
}
