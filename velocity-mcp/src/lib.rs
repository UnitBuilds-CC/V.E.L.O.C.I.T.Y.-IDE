//! Library interface for V.E.L.O.C.I.T.Y. MCP server.
//!
//! This crate is primarily a binary (the MCP server / IDE), but exposes
//! key modules for integration testing and programmatic access.

// Workspace-level clippy configuration.
// Architectural allows (function signatures, naming conventions, glob structure):
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]
#![allow(clippy::result_large_err)]
#![allow(clippy::ptr_arg)]
#![allow(clippy::enum_variant_names)]
#![allow(clippy::upper_case_acronyms)]
#![allow(clippy::should_implement_trait)]
#![allow(ambiguous_glob_reexports)]
// Code-style allows — remaining instances require broader refactoring:
#![allow(clippy::needless_range_loop)]
#![allow(clippy::manual_strip)]
#![allow(clippy::only_used_in_recursion)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::if_same_then_else)]

pub mod agent;
pub mod automation;
pub mod compiler;
pub mod connectors;
pub mod disk_hygiene;
pub mod editor;
pub mod errors;
pub mod generation;
pub mod health;
pub mod ipc;
pub mod metrics;
pub mod orchestrator;
pub mod protocol;
pub mod registry;
pub mod safety;
pub mod security;
pub mod shutdown;
pub mod telemetry;
pub mod usage;
pub mod wa;
pub mod wasm_runtime;
