//! Meta-agent control plane: decomposes large projects into parallel tasks,
//! dispatches workers, reconciles collisions, and validates per-task outputs.

use std::fmt;

pub mod blueprint;
pub mod reconcile;
pub mod registry;
pub mod scheduler;
pub mod validator;
pub mod worker;

/// Stable identifier for a task node in the project graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct TaskId(pub u64);

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}
