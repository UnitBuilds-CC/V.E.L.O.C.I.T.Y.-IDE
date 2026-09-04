//! Velocity Drone — Lightweight portable agent endpoint.
//!
//! A minimal implementation of the V.E.L.O.C.I.T.Y. peer protocol that can be
//! deployed on any machine as a single binary without the full IDE.
//!
//! # Protocol
//! See `DRONE_PROTOCOL.md` for the full specification.

pub mod core;
pub mod safety;
pub mod server;
