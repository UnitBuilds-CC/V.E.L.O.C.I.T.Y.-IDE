pub mod build_runner;
pub mod tester;
pub mod watcher;
pub mod mediator;
pub mod coordinator;
pub mod instruction_registry;
pub mod model_quality;
pub mod task_router;
pub mod site_map_support;

pub use build_runner::{
    diagnostics_path, read_latest_diagnostics, run_cargo_check, run_self_check,
    spawn_build_watcher, write_diagnostics, BuildDiagnostics,
};
pub use tester::{run_tests_on_demand, run_jit_tests_in_sandbox, TestReport};
pub use watcher::spawn_ast_watcher;
pub use mediator::MediatorArena;
pub use coordinator::WorkspaceCoordinator;
pub use instruction_registry::{AgentTaskKind, InstructionRegistry, InstructionTemplate};
pub use model_quality::{ModelCandidate, ModelQualityIndex, ProviderCapability, TaskRequirements};
pub use task_router::{partition_files_by_coupling, ProviderModelCatalog, RoutedModelRoute, RoutedSubAgentTask, SiteMapTaskRouter};
pub use site_map_support::{open_workspace_site_map, resolve_weight_root};
