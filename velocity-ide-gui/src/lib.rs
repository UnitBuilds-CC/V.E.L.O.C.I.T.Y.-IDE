//! V.E.L.O.C.I.T.Y. IDE GUI — Library root
//!
//! The GUI is primarily a binary (see main.rs). This lib root
//! exists so integration tests and tooling can reference the crate.
//! The editor module lives in velocity_mcp; this crate re-exports
//! nothing extra — the binary uses velocity_mcp::editor directly.
