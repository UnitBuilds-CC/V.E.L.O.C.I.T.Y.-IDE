//! Velocity Workflow Core — foundational types and traits.
//!
//! This crate defines the core abstractions for the Velocity Workflow Engine:
//! - [`WorkflowId`], [`StepId`], [`RunId`] — Strongly-typed identifiers
//! - [`Workflow`] — A named, ordered sequence of steps
//! - [`Step`] — A unit of work within a workflow
//! - [`VirtualObject`] — Batchable state mutation target (Restate-style)
//! - [`WorkflowState`] — Execution state machine
//! - [`StepOutcome`] — Result of executing a step
//!
//! # Virtual Object Batching
//!
//! Inspired by Restate, virtual objects allow multiple state mutations to be
//! batched into a single commit. Instead of fsyncing after each step, the engine
//! collects mutations to virtual objects and commits them in batches controlled
//! by the `sync_steps` configuration.

pub mod error;
pub mod identifiers;
pub mod step;
pub mod virtual_object;
pub mod workflow;
pub mod state;
pub mod config;

pub use error::*;
pub use identifiers::*;
pub use step::*;
pub use virtual_object::*;
pub use workflow::*;
pub use state::*;
pub use config::*;

