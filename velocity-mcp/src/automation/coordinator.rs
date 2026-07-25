#![allow(dead_code)]

use crate::agent::{AiProvider, ModelInfo};
use crate::automation::instruction_registry::AgentTaskKind;
use crate::automation::mediator::MediatorArena;
use crate::automation::site_map_support::resolve_weight_root;
use crate::automation::task_router::{
    partition_files_by_coupling, ProviderModelCatalog, RoutedModelRoute, RoutedSubAgentTask,
    SiteMapTaskRouter,
};
use crate::orchestrator::blueprint::Task;
use crate::orchestrator::worker::{spawn_live_worker, WorkerAssignment};
use crate::orchestrator::TaskId;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use velocity_ide::site_map::SiteMap;

pub struct CoordinatorTask {
    pub task_id: String,
    pub task_kind: AgentTaskKind,
    pub files: Vec<PathBuf>,
    pub summary: String,
    pub execution_contract: String,
    pub provider: AiProvider,
    pub model_id: String,
    pub model_label: String,
    pub thinking: bool,
    pub fallback_chain: Vec<RoutedModelRoute>,
}

pub struct WorkspaceCoordinator {
    mediator: Arc<MediatorArena>,
}

impl WorkspaceCoordinator {
    pub fn new(mediator: Arc<MediatorArena>) -> Self {
        Self { mediator }
    }

    /// partition a complex list of files into decoupled sub-groups based on SiteMap CALLS/DECLARES graph.
    pub fn partition_tasks(&self, files: &[PathBuf], site_map: &SiteMap) -> Vec<Vec<PathBuf>> {
        partition_files_by_coupling(files, site_map)
    }

    pub fn plan_routed_tasks(
        &self,
        workspace_root: &Path,
        goal: &str,
        task_kind: AgentTaskKind,
        files: &[PathBuf],
        site_map: &SiteMap,
        model_catalogs: &[(AiProvider, Vec<ModelInfo>)],
    ) -> Vec<RoutedSubAgentTask> {
        let router = SiteMapTaskRouter::open(workspace_root);
        let catalogs: Vec<ProviderModelCatalog> = model_catalogs
            .iter()
            .map(|(provider, models)| ProviderModelCatalog {
                provider: *provider,
                models: models.clone(),
            })
            .collect();
        router.route_tasks(goal, task_kind, files, site_map, &catalogs)
    }

    /// Spawns parallel subagent sessions across distinct worktree directories.
    pub fn execute_parallel_tasks(
        &self,
        tasks: Vec<CoordinatorTask>,
        base_workspace: &Path,
        site_map: &SiteMap,
    ) -> Result<String, String> {
        let weight_root = resolve_weight_root(base_workspace);
        let mut workers = Vec::new();

        for (idx, task) in tasks.into_iter().enumerate() {
            let orchestrator_task = Task {
                id: TaskId(idx as u64 + 1),
                title: task.task_id.clone(),
                description: task.summary.clone(),
                scope: task
                    .files
                    .iter()
                    .map(|file| file.display().to_string())
                    .collect(),
                dependencies: Vec::new(),
                output: None,
            };
            let handle = spawn_live_worker(
                WorkerAssignment {
                    task: orchestrator_task,
                    task_kind: task.task_kind,
                    workspace_root: base_workspace.to_path_buf(),
                    instructions: task.execution_contract.clone(),
                    planned_site_map_root: site_map.root(),
                    provider: task.provider,
                    provider_label: task.provider.label().to_string(),
                    model_id: task.model_id.clone(),
                    model_label: task.model_label.clone(),
                    thinking: task.thinking,
                    fallback_chain: task.fallback_chain.clone(),
                    scoped_files: None,
                },
                self.mediator.clone(),
                weight_root,
            );
            workers.push((task.task_id, handle));
        }

        let mut completed = Vec::new();
        let mut failures = Vec::new();
        while !workers.is_empty() {
            let mut remaining = Vec::new();
            for (task_id, mut handle) in workers.into_iter() {
                if let Some(result) = handle.poll() {
                    if result.success {
                        completed.push(format!("{}: {}", task_id, result.message));
                    } else {
                        failures.push(format!("{}: {}", task_id, result.message));
                    }
                } else {
                    remaining.push((task_id, handle));
                }
            }
            workers = remaining;
            if !workers.is_empty() {
                thread::yield_now();
            }
        }

        if failures.is_empty() {
            Ok(format!(
                "Executed {} parallel task(s) through the live worker runtime.",
                completed.len()
            ))
        } else {
            Err(failures.join("\n"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_partition_and_execution() {
        let temp = tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(temp.path().join("src/lib.rs"), "pub fn lib() {}\n").unwrap();

        let sitemap_dir = temp.path().join("site_map");
        let sm = SiteMap::open(&sitemap_dir, resolve_weight_root(temp.path())).unwrap();

        let mediator = Arc::new(MediatorArena::new());
        let coord = WorkspaceCoordinator::new(mediator);

        let files = vec![PathBuf::from("src/main.rs"), PathBuf::from("src/lib.rs")];

        let partitions = coord.partition_tasks(&files, &sm);
        assert_eq!(partitions.len(), 2);

        let tasks = vec![
            CoordinatorTask {
                task_id: "Agent_1".to_string(),
                task_kind: AgentTaskKind::Test,
                files: vec![PathBuf::from("src/main.rs")],
                summary: "Compile main".to_string(),
                execution_contract: "contract version 1\n".to_string(),
                provider: AiProvider::CloudflareWorkersAi,
                model_id: "@cf/moonshotai/kimi-k2.7-code".to_string(),
                model_label: "kimi-k2.7-code".to_string(),
                thinking: false,
                fallback_chain: Vec::new(),
            },
            CoordinatorTask {
                task_id: "Agent_2".to_string(),
                task_kind: AgentTaskKind::Test,
                files: vec![PathBuf::from("src/lib.rs")],
                summary: "Clean lib".to_string(),
                execution_contract: "contract version 1\n".to_string(),
                provider: AiProvider::CloudflareWorkersAi,
                model_id: "@cf/moonshotai/kimi-k2.7-code".to_string(),
                model_label: "kimi-k2.7-code".to_string(),
                thinking: false,
                fallback_chain: Vec::new(),
            },
        ];

        let run_res = coord.execute_parallel_tasks(tasks, temp.path(), &sm);
        assert!(
            run_res.is_ok()
                || run_res
                    .as_ref()
                    .err()
                    .unwrap()
                    .contains("No scoped file changes")
        );
    }
}
