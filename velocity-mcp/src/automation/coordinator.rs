use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use velocity_ide::site_map::SiteMap;
use crate::agent::{AiProvider, ModelInfo};
use crate::automation::instruction_registry::AgentTaskKind;
use crate::automation::mediator::MediatorArena;
use crate::automation::task_router::{partition_files_by_coupling, ProviderModelCatalog, RoutedSubAgentTask, SiteMapTaskRouter};

pub struct CoordinatorTask {
    pub task_id: String,
    pub files: Vec<PathBuf>,
    pub instructions: String,
}

pub struct WorkspaceCoordinator {
    mediator: Arc<MediatorArena>,
}

impl WorkspaceCoordinator {
    pub fn new(mediator: Arc<MediatorArena>) -> Self {
        Self { mediator }
    }

    /// partition a complex list of files into decoupled sub-groups based on SiteMap CALLS/DECLARES graph.
    pub fn partition_tasks(
        &self,
        files: &[PathBuf],
        site_map: &SiteMap,
    ) -> Vec<Vec<PathBuf>> {
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
        let mut handles = Vec::new();
        let error_accumulator = Arc::new(Mutex::new(Vec::new()));

        for task in tasks {
            let mediator = self.mediator.clone();
            let base_workspace = base_workspace.to_path_buf();
            let errs = error_accumulator.clone();
            let site_map_path = base_workspace.join(".velocity").join("site_map");
            let weight_root = 0xDEAD;

            let handle = thread::spawn(move || {
                // Setup temporary worktree space
                let worktree_dir = base_workspace.join(".velocity").join("temp_workspaces").join(&task.task_id);
                std::fs::create_dir_all(&worktree_dir).ok();

                // Mock file lock acquisition
                let sm = SiteMap::open(&site_map_path, weight_root).unwrap();
                for file in &task.files {
                    let lock_res = mediator.acquire_lock(
                        file.clone(),
                        (1, 100),
                        task.task_id.clone(),
                        &sm,
                    );
                    if let Err(conflict) = lock_res {
                        let msg = mediator.resolve_conflict(&conflict);
                        let mut errs_guard = errs.lock().unwrap();
                        errs_guard.push(format!("Task {} lock error: {}", task.task_id, msg));
                        return;
                    }
                }

                // Simulate agent work execution time
                thread::sleep(Duration::from_millis(50));

                // Release locks upon completion
                for file in &task.files {
                    mediator.release_lock(file, &task.task_id);
                }
            });

            handles.push(handle);
        }

        for h in handles {
            h.join().map_err(|_| "Failed to join subagent thread".to_string())?;
        }

        let errs_guard = error_accumulator.lock().unwrap();
        if !errs_guard.is_empty() {
            Err(errs_guard.join("\n"))
        } else {
            Ok("All parallel worktree agent tasks executed successfully with zero lock conflicts.".to_string())
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
        let sitemap_dir = temp.path().join("site_map");
        let sm = SiteMap::open(&sitemap_dir, 0xDEAD).unwrap();

        let mediator = Arc::new(MediatorArena::new());
        let coord = WorkspaceCoordinator::new(mediator);

        let files = vec![
            PathBuf::from("src/main.rs"),
            PathBuf::from("src/lib.rs"),
        ];

        let partitions = coord.partition_tasks(&files, &sm);
        assert_eq!(partitions.len(), 2);

        let tasks = vec![
            CoordinatorTask {
                task_id: "Agent_1".to_string(),
                files: vec![PathBuf::from("src/main.rs")],
                instructions: "Compile main".to_string(),
            },
            CoordinatorTask {
                task_id: "Agent_2".to_string(),
                files: vec![PathBuf::from("src/lib.rs")],
                instructions: "Clean lib".to_string(),
            },
        ];

        let run_res = coord.execute_parallel_tasks(tasks, temp.path(), &sm);
        assert!(run_res.is_ok());
    }
}
