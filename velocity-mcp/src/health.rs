//! Health check endpoints for Velocity MCP server.
//!
//! Provides JSON-RPC health check methods for monitoring and load balancing.
//!
//! # Usage
//!
//! ```json
//! {"jsonrpc":"2.0","method":"health","id":1}
//! ```
//!
//! Response:
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "result": {
//!     "status": "ok",
//!     "version": "1.0.0",
//!     "uptime_seconds": 3600,
//!     "providers_available": 4,
//!     "workspace": "/path/to/workspace"
//!   },
//!   "id": 1
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Health status of the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    /// Server is healthy and operational.
    Ok,
    /// Server is degraded but functional.
    Degraded,
    /// Server is unhealthy and may not be functional.
    Unhealthy,
}

/// Health check response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Overall health status.
    pub status: HealthStatus,
    /// Server version.
    pub version: String,
    /// Uptime in seconds.
    pub uptime_seconds: u64,
    /// Number of available providers.
    pub providers_available: usize,
    /// Workspace path.
    pub workspace: String,
    /// Additional details (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<HealthDetails>,
}

/// Detailed health information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthDetails {
    /// Provider status.
    pub providers: Vec<ProviderHealth>,
    /// System resource usage.
    pub resources: ResourceHealth,
    /// Recent error count (last 5 minutes).
    pub recent_errors: u64,
}

/// Individual provider health.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderHealth {
    /// Provider name.
    pub name: String,
    /// Provider status.
    pub status: HealthStatus,
    /// Response time in milliseconds.
    pub response_time_ms: u64,
    /// Last error message (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// System resource health.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceHealth {
    /// Memory usage percentage (0-100).
    pub memory_percent: f32,
    /// CPU usage percentage (0-100).
    pub cpu_percent: f32,
    /// Disk usage percentage (0-100).
    pub disk_percent: f32,
}

/// Health checker for the MCP server.
pub struct HealthChecker {
    start_time: Instant,
    workspace: String,
}

impl HealthChecker {
    /// Create a new health checker.
    pub fn new(workspace: impl Into<String>) -> Self {
        Self {
            start_time: Instant::now(),
            workspace: workspace.into(),
        }
    }

    /// Perform a basic health check.
    pub fn check(&self) -> HealthResponse {
        HealthResponse {
            status: HealthStatus::Ok,
            version: env!("CARGO_PKG_VERSION").to_string(),
            uptime_seconds: self.start_time.elapsed().as_secs(),
            providers_available: 0, // Will be populated by caller
            workspace: self.workspace.clone(),
            details: None,
        }
    }

    /// Perform a detailed health check.
    pub fn check_detailed(&self, providers: Vec<ProviderHealth>) -> HealthResponse {
        let resources = self.check_resources();
        let recent_errors = self.count_recent_errors();

        let overall_status = if providers.iter().any(|p| p.status == HealthStatus::Unhealthy) {
            HealthStatus::Degraded
        } else if resources.memory_percent > 90.0 || resources.disk_percent > 90.0 {
            HealthStatus::Degraded
        } else {
            HealthStatus::Ok
        };

        HealthResponse {
            status: overall_status,
            version: env!("CARGO_PKG_VERSION").to_string(),
            uptime_seconds: self.start_time.elapsed().as_secs(),
            providers_available: providers
                .iter()
                .filter(|p| p.status == HealthStatus::Ok)
                .count(),
            workspace: self.workspace.clone(),
            details: Some(HealthDetails {
                providers,
                resources,
                recent_errors,
            }),
        }
    }

    /// Check system resource usage.
    fn check_resources(&self) -> ResourceHealth {
        // This is a simplified implementation.
        // In production, use platform-specific APIs or crates like `sysinfo`.
        ResourceHealth {
            memory_percent: 0.0,
            cpu_percent: 0.0,
            disk_percent: 0.0,
        }
    }

    /// Count recent errors (last 5 minutes).
    fn count_recent_errors(&self) -> u64 {
        // This would integrate with the logging system to count errors.
        // Simplified implementation returns 0.
        0
    }
}

/// JSON-RPC request for health check.
#[derive(Debug, Deserialize)]
pub struct HealthRequest {
    /// JSON-RPC method name.
    pub method: String,
    /// Request ID.
    pub id: serde_json::Value,
}

/// JSON-RPC response for health check.
#[derive(Debug, Serialize)]
pub struct HealthJsonResponse {
    /// JSON-RPC version.
    pub jsonrpc: String,
    /// Result (health response).
    pub result: HealthResponse,
    /// Request ID.
    pub id: serde_json::Value,
}

impl HealthJsonResponse {
    /// Create a new health response.
    pub fn new(health: HealthResponse, id: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: health,
            id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_checker_basic() {
        let checker = HealthChecker::new("/tmp/workspace");
        let health = checker.check();
        assert_eq!(health.status, HealthStatus::Ok);
        assert_eq!(health.workspace, "/tmp/workspace");
    }

    #[test]
    fn test_health_checker_detailed() {
        let checker = HealthChecker::new("/tmp/workspace");
        let providers = vec![
            ProviderHealth {
                name: "openai".to_string(),
                status: HealthStatus::Ok,
                response_time_ms: 100,
                last_error: None,
            },
            ProviderHealth {
                name: "anthropic".to_string(),
                status: HealthStatus::Ok,
                response_time_ms: 150,
                last_error: None,
            },
        ];
        let health = checker.check_detailed(providers);
        assert_eq!(health.status, HealthStatus::Ok);
        assert_eq!(health.providers_available, 2);
        assert!(health.details.is_some());
    }

    #[test]
    fn test_health_status_serialization() {
        let status = HealthStatus::Ok;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"ok\"");

        let status: HealthStatus = serde_json::from_str("\"degraded\"").unwrap();
        assert_eq!(status, HealthStatus::Degraded);
    }
}
