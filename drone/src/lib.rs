//! Velocity Drone — Lightweight portable agent endpoint.
//!
//! A minimal implementation of the V.E.L.O.C.I.T.Y. peer protocol that can be
//! deployed on any machine as a single binary without the full IDE.
//!
//! # Modules
//! - [`core`] — identity, file transfers, task execution, deployment.
//! - [`safety`] — poisoning-tolerant mutex primitives.
//! - [`server`] — HTTP/JSON API server.
//! - [`scheduler`] — priority-based task scheduler with concurrency control.
//! - [`health`] — health monitoring and status reporting.
//! - [`system`] — screen capture, input simulation, network monitoring.
//!
//! # Protocol
//! See `DRONE_PROTOCOL.md` for the full specification.

pub mod core;
pub mod health;
pub mod safety;
pub mod scheduler;
pub mod server;
pub mod system;
