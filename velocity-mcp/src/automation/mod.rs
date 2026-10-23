pub mod audit;
pub mod build_runner;
pub mod change_attribution;
pub mod coordinator;
pub mod formatter;
pub mod governor;
pub mod instruction_registry;
pub mod mediator;
pub mod merge;
pub mod mission_runner;
pub mod model_quality;
pub mod op_log_store;
pub mod oplog;
pub mod partition;
pub mod principal;
pub mod replication;
pub mod sectors;
pub mod site_map_support;
pub mod task_router;
pub mod tester;
pub mod watcher;
pub mod write_gate;

pub use audit::{caused_by, engagement};
pub use build_runner::{
    read_latest_diagnostics, run_cargo_check, run_self_check, spawn_build_watcher, BuildDiagnostics,
};
pub use change_attribution::{Attribution, ChangeAttributor};
pub use coordinator::WorkspaceCoordinator;
pub use governor::{team_resource, Governor, WORKSPACE_RESOURCE};
pub use instruction_registry::{AgentTaskKind, DecompositionStyle, InstructionRegistry};
pub use mediator::MediatorArena;
pub use merge::{materialize_text, LastWriteWins, MergePolicy, TextHead};
pub use mission_runner::{
    run_mission, MissionConfig, MissionManifest, MissionProgress, MissionReport, MissionStatus,
};
pub use op_log_store::OpLogStore;
pub use oplog::{materialize, EventOutcome, MaterializedState, Op, OpEffect, OpSet, TripleEffect};
pub use partition::{
    default_partition_key, materialize_partitioned, partition_by, union_live_triples, PartitionKey,
};
pub use principal::{CausalOrder, NodeId, OpId, Principal, PrincipalKind, VectorClock};
pub use replication::{
    handle_exchange, merge_remote_ops, ops_for_peer, OpsExchangeRequest, OpsExchangeResponse,
    ReplicationView,
};
pub use sectors::{plan_sectors, SectorOptions, SectorPlan, SectorTotals, SectorUnit};
pub use site_map_support::{open_workspace_site_map, resolve_weight_root};
pub use task_router::RoutedSubAgentTask;
pub use watcher::spawn_ast_watcher;
pub use write_gate::{Conflict, ConflictKind, EditLock, WriteGate, WHOLE_SCOPE};
