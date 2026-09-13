//! Security subsystem: encrypted secret storage, audit logging, input
//! sanitization, and (Pillar 5b) policy/approval governance for agent tool
//! execution.
//!
//! Secrets never touch disk in the clear — they are sealed with the workspace
//! master key via `agent::crypto` (Windows DPAPI-backed, AES-256-GCM `NDA1`
//! envelope). Connectors and providers reference secrets by *handle* (name)
//! rather than embedding raw credentials in their configs.

pub mod audit;
pub mod chaos;
pub mod key_rotation;
pub mod policy;
pub mod rate_limit;
pub mod sanitize;
pub mod secrets;
pub mod threat_model;
