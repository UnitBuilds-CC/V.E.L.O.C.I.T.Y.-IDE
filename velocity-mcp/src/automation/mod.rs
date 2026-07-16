pub mod build_runner;
pub mod tester;

pub use build_runner::{
    diagnostics_path, read_latest_diagnostics, run_cargo_check, run_self_check,
    spawn_build_watcher, write_diagnostics, BuildDiagnostics,
};
pub use tester::{run_tests_on_demand, TestReport};
