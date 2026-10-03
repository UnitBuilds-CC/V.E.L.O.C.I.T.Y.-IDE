use crate::agent::{AiProvider, ModelInfo};
use crate::automation::instruction_registry::AgentTaskKind;
use crate::automation::mediator::MediatorArena;
use crate::automation::task_router::{ProviderModelCatalog, RoutedSubAgentTask, SiteMapTaskRouter};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use velocity_ide::site_map::SiteMap;

/// Turns a goal + file scope into a list of model-routed sub-agent tasks by
/// consulting the SiteMap. The Orchestrator panel owns the actual dispatch and
/// worker threads; this coordinator is purely the planning seam that the app
/// calls before handing the routed tasks to that panel.
pub struct WorkspaceCoordinator {
    mediator: Arc<MediatorArena>,
}

impl WorkspaceCoordinator {
    pub fn new(mediator: Arc<MediatorArena>) -> Self {
        Self { mediator }
    }

    /// The mediator the eventual worker threads will share. Exposed so the
    /// caller can hand the same arena to the Orchestrator dispatch without
    /// threading it through the planning call.
    pub fn mediator(&self) -> &Arc<MediatorArena> {
        &self.mediator
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
}
