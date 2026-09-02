//! Strongly-typed identifiers for workflows, steps, and runs.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkflowId(pub String);
impl WorkflowId {
    pub fn new() -> Self { Self(Uuid::new_v4().to_string()) }
    pub fn from_str(s: impl Into<String>) -> Self { Self(s.into()) }
    pub fn as_str(&self) -> &str { &self.0 }
}
impl Default for WorkflowId { fn default() -> Self { Self::new() } }
impl std::fmt::Display for WorkflowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}", self.0) }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StepId(pub String);
impl StepId {
    pub fn new() -> Self { Self(Uuid::new_v4().to_string()) }
    pub fn from_str(s: impl Into<String>) -> Self { Self(s.into()) }
    pub fn as_str(&self) -> &str { &self.0 }
}
impl Default for StepId { fn default() -> Self { Self::new() } }
impl std::fmt::Display for StepId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}", self.0) }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RunId(pub String);
impl RunId {
    pub fn new() -> Self { Self(Uuid::new_v4().to_string()) }
    pub fn from_str(s: impl Into<String>) -> Self { Self(s.into()) }
    pub fn as_str(&self) -> &str { &self.0 }
}
impl Default for RunId { fn default() -> Self { Self::new() } }
impl std::fmt::Display for RunId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}", self.0) }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VirtualObjectId(pub String);
impl VirtualObjectId {
    pub fn new() -> Self { Self(Uuid::new_v4().to_string()) }
    pub fn from_str(s: impl Into<String>) -> Self { Self(s.into()) }
    pub fn as_str(&self) -> &str { &self.0 }
}
impl Default for VirtualObjectId { fn default() -> Self { Self::new() } }
impl std::fmt::Display for VirtualObjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}", self.0) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workflow_id_new_is_unique() { assert_ne!(WorkflowId::new(), WorkflowId::new()); }
    #[test]
    fn identifiers_roundtrip_serde() {
        let id = RunId::new();
        let json = serde_json::to_string(&id).unwrap();
        let parsed: RunId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, parsed);
    }
}
