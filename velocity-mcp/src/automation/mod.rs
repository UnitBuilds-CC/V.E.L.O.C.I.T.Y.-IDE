pub mod build_runner;
pub mod coordinator;
pub mod instruction_registry;
pub mod mediator;
pub mod model_quality;
pub mod site_map_support;
pub mod task_router;
pub mod tester;
pub mod watcher;

pub use build_runner::{
    diagnostics_path, read_latest_diagnostics, run_cargo_check, run_self_check,
    spawn_build_watcher, write_diagnostics, BuildDiagnostics,
};
pub use coordinator::WorkspaceCoordinator;
pub use instruction_registry::{
    AgentTaskKind, DecompositionPolicy, DecompositionStyle, InstructionRegistry,
    InstructionTemplate, PreferredPolicy,
};
pub use mediator::MediatorArena;
pub use model_quality::{ModelCandidate, ModelQualityIndex, ProviderCapability, TaskRequirements};
pub use site_map_support::{open_workspace_site_map, resolve_weight_root};
pub use task_router::{
    partition_files_by_coupling, partition_files_by_policy, ProviderModelCatalog, RoutedModelRoute,
    RoutedSubAgentTask, SiteMapTaskRouter,
};
pub use tester::{run_jit_tests_in_sandbox, run_tests_on_demand, TestReport};
pub use watcher::spawn_ast_watcher;
