//! Security subsystem: encrypted secret storage and (Pillar 5b) policy/approval
//! governance for agent tool execution.
//!
//! Secrets never touch disk in the clear — they are sealed with the workspace
//! master key via `agent::crypto` (Windows DPAPI-backed, AES-256-GCM `NDA1`
//! envelope). Connectors and providers reference secrets by *handle* (name)
//! rather than embedding raw credentials in their configs.
//!
//! `dead_code` is allowed at the module level while the governance UI and
//! connector wiring (later pillars) grow into the full secret-handle surface.
#![allow(dead_code)]

pub mod secrets;
