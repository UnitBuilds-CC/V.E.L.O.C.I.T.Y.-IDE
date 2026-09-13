//! Velocity Router client — thin HTTP bridge to the MoA orchestration service.
//!
//! The IDE sends tasks to the router; the router handles decomposition,
//! domain-based model selection, parallel dispatch, and result assembly.
//! This module knows nothing about routing internals — that's the router's job.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Default router URL (localhost for development).
pub const DEFAULT_ROUTER_URL: &str = "http://localhost:8787";

/// Health check timeout — short to fail fast if router is down.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);

/// Assignment request timeout — allows for complex multi-model orchestration.
const ASSIGNMENT_TIMEOUT: Duration = Duration::from_secs(120);

// ─── API Types ────────────────────────────────────────────────────────────────

/// Execution tier for task routing.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionTier {
    Lite,
    Standard,
    Pro,
    #[default]
    Ultimate,
}

/// How the assignment should be executed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionMode {
    /// Block until all sub-tasks complete and return assembled results.
    #[default]
    Sync,
    /// Return an assignment ID immediately; poll for status.
    Async,
}

/// POST /v1/assignments — submit a task for orchestration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignmentRequest {
    /// The natural-language task description from the user.
    pub task: String,

    /// Execution tier: lite, standard, pro, ultimate.
    pub tier: ExecutionTier,

    /// Optional project context (file tree, recent edits, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,

    /// Optional file paths involved in the task (helps with domain inference).
    #[serde(default)]
    pub file_paths: Vec<String>,

    /// Execution mode: "sync" blocks until complete, "async" returns an ID.
    #[serde(default)]
    pub mode: ExecutionMode,

    /// Optional maximum cost the user is willing to pay (in USD).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_cost_usd: Option<f64>,
}

/// Response for a submitted assignment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignmentResponse {
    /// Unique assignment ID.
    pub id: String,

    /// Current status.
    pub status: AssignmentStatus,

    /// The decomposition plan — shows how the task was broken up.
    #[serde(default)]
    pub plan: Option<AssignmentPlan>,

    /// Routing decisions — shows WHY each model was chosen.
    #[serde(default)]
    pub routing_decisions: Option<Vec<RoutingDecisionEntry>>,

    /// Completed results (present when status is Completed).
    #[serde(default)]
    pub results: Option<Vec<SubTaskResultResponse>>,

    /// Assembled final output (present when status is Completed).
    #[serde(default)]
    pub assembled_output: Option<String>,

    /// Cost summary.
    #[serde(default)]
    pub cost: Option<CostSummary>,

    /// If max_cost was set and estimated cost exceeds it.
    #[serde(default)]
    pub cost_estimate: Option<CostSummary>,

    /// Error message if something went wrong.
    #[serde(default)]
    pub error: Option<String>,

    /// When the assignment was created.
    pub created_at: String,

    /// When the assignment was completed.
    #[serde(default)]
    pub completed_at: Option<String>,
}

/// Status of an assignment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AssignmentStatus {
    Planning,
    Executing,
    Assembling,
    Completed,
    Failed,
    AwaitingApproval,
}

/// The decomposition plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignmentPlan {
    pub subtasks: Vec<SubTaskPlanEntry>,
}

/// A single entry in the decomposition plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubTaskPlanEntry {
    pub id: String,
    pub description: String,
    pub domain: String,
    pub model: String,
    pub provider: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

/// A routing decision entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingDecisionEntry {
    pub domain: String,
    pub model_id: String,
    pub model_label: String,
    pub provider: String,
    pub rationale: String,
    pub cost_per_mtok: f64,
}

/// Result of a single sub-task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubTaskResultResponse {
    pub id: String,
    pub description: String,
    pub domain: String,
    pub model: String,
    pub provider: String,
    pub output: String,
    pub status: String,
    pub tokens_used: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub duration_ms: u64,
    #[serde(default)]
    pub routing_rationale: Option<String>,
}

/// Cost summary for the entire assignment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostSummary {
    pub total_tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_cost_usd: f64,
    pub breakdown: Vec<CostLineItem>,
}

/// Cost for a single sub-task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostLineItem {
    pub subtask_id: String,
    pub model: String,
    pub tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

/// Health check response from the router.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub uptime_seconds: u64,
    pub models_available: usize,
    pub api_keys_active: usize,
}

// ─── Client Functions ─────────────────────────────────────────────────────────

/// Check if the router is available and healthy.
/// Returns `None` if the router is unreachable or unhealthy.
pub fn check_router_health(router_url: &str) -> Option<HealthResponse> {
    let url = format!("{}/health", router_url.trim_end_matches('/'));
    let response = ureq::get(&url)
        .timeout(HEALTH_TIMEOUT)
        .call()
        .ok()?;

    if response.status() != 200 {
        return None;
    }

    response.into_json::<HealthResponse>().ok()
}

/// Submit a task to the router for MoA orchestration.
/// Returns the assignment response with assembled output.
pub fn submit_assignment(
    router_url: &str,
    api_key: &str,
    request: &AssignmentRequest,
) -> Result<AssignmentResponse, RouterError> {
    let url = format!("{}/v1/assignments", router_url.trim_end_matches('/'));

    let response = ureq::post(&url)
        .timeout(ASSIGNMENT_TIMEOUT)
        .set("Authorization", &format!("Bearer {}", api_key))
        .set("Content-Type", "application/json")
        .send_json(request)
        .map_err(RouterError::from_ureq)?;

    if response.status() != 200 && response.status() != 201 {
        return Err(RouterError::HttpStatus(response.status()));
    }

    response
        .into_json::<AssignmentResponse>()
        .map_err(|e| RouterError::ParseError(e.to_string()))
}

/// Submit a task and wait for completion (convenience wrapper).
/// Returns the assembled output string if successful.
pub fn submit_and_wait(
    router_url: &str,
    api_key: &str,
    task: &str,
    tier: ExecutionTier,
    context: Option<&str>,
    file_paths: &[String],
) -> Result<String, RouterError> {
    let request = AssignmentRequest {
        task: task.to_string(),
        tier,
        context: context.map(|s| s.to_string()),
        file_paths: file_paths.to_vec(),
        mode: ExecutionMode::Sync,
        max_cost_usd: None,
    };

    let response = submit_assignment(router_url, api_key, &request)?;

    match response.status {
        AssignmentStatus::Completed => response
            .assembled_output
            .ok_or_else(|| RouterError::NoOutput("Completed but no assembled_output".into())),
        AssignmentStatus::Failed => {
            Err(RouterError::TaskFailed(response.error.unwrap_or_default()))
        }
        AssignmentStatus::AwaitingApproval => {
            Err(RouterError::CostApprovalRequired(response.cost_estimate))
        }
        _ => Err(RouterError::UnexpectedStatus(response.status)),
    }
}

// ─── Error Types ──────────────────────────────────────────────────────────────

/// Errors that can occur when communicating with the router.
#[derive(Debug)]
pub enum RouterError {
    /// Router is unreachable (connection refused, timeout, etc.)
    Unreachable(String),
    /// HTTP error status from the router.
    HttpStatus(u16),
    /// Failed to parse response.
    ParseError(String),
    /// Task completed but produced no output.
    NoOutput(String),
    /// Task failed with an error message.
    TaskFailed(String),
    /// Task requires cost approval before execution.
    CostApprovalRequired(Option<CostSummary>),
    /// Unexpected assignment status.
    UnexpectedStatus(AssignmentStatus),
}

impl RouterError {
    fn from_ureq(err: ureq::Error) -> Self {
        match err {
            ureq::Error::Transport(transport) => {
                RouterError::Unreachable(transport.to_string())
            }
            ureq::Error::Status(code, _response) => RouterError::HttpStatus(code),
        }
    }
}

impl std::fmt::Display for RouterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RouterError::Unreachable(msg) => write!(f, "Router unreachable: {}", msg),
            RouterError::HttpStatus(code) => write!(f, "Router returned HTTP {}", code),
            RouterError::ParseError(msg) => write!(f, "Failed to parse router response: {}", msg),
            RouterError::NoOutput(msg) => write!(f, "No output: {}", msg),
            RouterError::TaskFailed(msg) => write!(f, "Task failed: {}", msg),
            RouterError::CostApprovalRequired(_) => {
                write!(f, "Task requires cost approval")
            }
            RouterError::UnexpectedStatus(status) => {
                write!(f, "Unexpected assignment status: {:?}", status)
            }
        }
    }
}

impl std::error::Error for RouterError {}

// ─── Router Availability Cache ────────────────────────────────────────────────

use std::sync::atomic::{AtomicBool, Ordering};

/// Cached router availability — avoids repeated health checks.
static ROUTER_AVAILABLE: AtomicBool = AtomicBool::new(false);

/// Check if the router was recently available (cached result).
pub fn is_router_available() -> bool {
    ROUTER_AVAILABLE.load(Ordering::Relaxed)
}

/// Refresh the router availability cache by performing a health check.
pub fn refresh_router_availability(router_url: &str) -> bool {
    let available = check_router_health(router_url).is_some();
    ROUTER_AVAILABLE.store(available, Ordering::Relaxed);
    available
}

/// Mark the router as unavailable (e.g., after a failed request).
pub fn mark_router_unavailable() {
    ROUTER_AVAILABLE.store(false, Ordering::Relaxed);
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_tier_default() {
        assert_eq!(ExecutionTier::default(), ExecutionTier::Ultimate);
    }

    #[test]
    fn test_execution_mode_default() {
        assert_eq!(ExecutionMode::default(), ExecutionMode::Sync);
    }

    #[test]
    fn test_assignment_request_serialization() {
        let request = AssignmentRequest {
            task: "Analyze this codebase".to_string(),
            tier: ExecutionTier::Pro,
            context: Some("Rust project".to_string()),
            file_paths: vec!["src/main.rs".to_string()],
            mode: ExecutionMode::Sync,
            max_cost_usd: Some(0.10),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"task\":\"Analyze this codebase\""));
        assert!(json.contains("\"tier\":\"pro\""));
        assert!(json.contains("\"mode\":\"sync\""));
    }

    #[test]
    fn test_assignment_response_deserialization() {
        let json = r#"{
            "id": "test-123",
            "status": "completed",
            "assembled_output": "Hello, world!",
            "created_at": "2026-01-01T00:00:00Z"
        }"#;

        let response: AssignmentResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.id, "test-123");
        assert_eq!(response.status, AssignmentStatus::Completed);
        assert_eq!(response.assembled_output, Some("Hello, world!".to_string()));
    }

    #[test]
    fn test_health_response_deserialization() {
        let json = r#"{
            "status": "healthy",
            "version": "1.0.0",
            "uptime_seconds": 3600,
            "models_available": 93,
            "api_keys_active": 9
        }"#;

        let health: HealthResponse = serde_json::from_str(json).unwrap();
        assert_eq!(health.status, "healthy");
        assert_eq!(health.models_available, 93);
    }

    #[test]
    fn test_router_error_display() {
        let err = RouterError::Unreachable("connection refused".to_string());
        assert_eq!(
            format!("{}", err),
            "Router unreachable: connection refused"
        );
    }

    #[test]
    fn test_router_availability_cache() {
        // Initially false
        mark_router_unavailable();
        assert!(!is_router_available());

        // Can be set
        ROUTER_AVAILABLE.store(true, Ordering::Relaxed);
        assert!(is_router_available());

        // Can be cleared
        mark_router_unavailable();
        assert!(!is_router_available());
    }
}
