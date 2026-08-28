//! V.E.L.O.C.I.T.Y. IDE GUI — Library root
//!
//! The GUI is primarily a binary (see main.rs). This lib root
//! exists so integration tests and tooling can reference the crate.
//! The editor module lives in velocity_mcp; this crate re-exports
//! nothing extra — the binary uses velocity_mcp::editor directly.

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
