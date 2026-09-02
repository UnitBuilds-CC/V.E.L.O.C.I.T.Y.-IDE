//! Velocity Workflow Engine — WAL-backed, batched, concurrent workflow execution.
//!
//! # Architecture
//!
//! The engine provides three key capabilities over the core workflow types:
//!
//! 1. **Virtual Object Batching** — Mutations to virtual objects are collected
//!    and committed in batches (controlled by sync_steps), reducing fsync
//!    overhead from O(steps) to O(steps/sync_steps).
//!
//! 2. **Concurrent Execution** — Steps with satisfied dependencies execute in
//!    parallel via a tokio task pool, bounded by max_step_parallelism.
//!
//! 3. **WAL-backed Durability** — Every batch commit is written to a WAL
//!    (SQLite-backed) before acknowledgment, enabling crash recovery.
//!
//! # Execution Flow
//!
//! `	ext
//! Workflow submitted
//!     |
//!     v
//! [Dependency Resolver] -> ready steps
//!     |
//!     v
//! [Parallel Executor] -> step outcomes + mutations
//!     |
//!     v
//! [Mutation Collector] -> batch buffer
//!     |
//!     v  (when buffer >= sync_steps)
//! [WAL Writer] -> batch commit (single fsync)
//!     |
//!     v
//! [Virtual Object Store] -> state updated
//! `

pub mod wal;
pub mod executor;
pub mod engine;
pub mod worker_pool;

pub use engine::WorkflowEngine;
pub use wal::WriteAheadLog;
pub use executor::StepExecutor;
pub use worker_pool::WorkerPool;
