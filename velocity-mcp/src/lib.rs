//! Library interface for V.E.L.O.C.I.T.Y. MCP server.
//!
//! This crate is primarily a binary (the MCP server / IDE), but exposes
//! key modules for integration testing and programmatic access.

#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]
#![allow(clippy::result_large_err)]
#![allow(clippy::ptr_arg)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::manual_strip)]
#![allow(clippy::enum_variant_names)]
#![allow(clippy::upper_case_acronyms)]
#![allow(clippy::only_used_in_recursion)]
#![allow(clippy::manual_c_str_literals)]
#![allow(clippy::derivable_impls)]
#![allow(clippy::manual_div_ceil)]
#![allow(clippy::manual_map)]
#![allow(clippy::while_let_loop)]
#![allow(clippy::new_without_default)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::if_same_then_else)]
#![allow(clippy::should_implement_trait)]
#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(ambiguous_glob_reexports)]

pub mod agent;
pub mod automation;
pub mod benchmark;
pub mod compiler;
pub mod connectors;
pub mod editor;
pub mod errors;
pub mod ipc;
pub mod orchestrator;
pub mod protocol;
pub mod registry;
pub mod safety;
pub mod security;
pub mod shutdown;
pub mod usage;
pub mod wa;
