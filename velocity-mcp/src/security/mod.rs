//! Security subsystem: encrypted secret storage and (Pillar 5b) policy/approval
//! governance for agent tool execution.
//!
//! Secrets never touch disk in the clear — they are sealed with the workspace
//! master key via `agent::crypto` (Windows DPAPI-backed, AES-256-GCM `NDA1`
//! envelope). Connectors and providers reference secrets by *handle* (name)
//! rather than embedding raw credentials in their configs.

pub mod secrets;
