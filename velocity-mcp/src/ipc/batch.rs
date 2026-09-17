use serde::{Deserialize, Serialize};
use std::time::Instant;

use super::telemetry_share::{TelemetryRequest, TelemetryResponse};

// ─── Batch Request ─────────────────────────────────────────────────────────────

/// Batch request enum for batching multiple IPC operations together.
/// Enables zero-copy message passing by grouping operations into a single
/// shared memory transaction, reducing synchronization overhead.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum BatchRequest {
    /// Batch update for AST triples across multiple files
    AstBatchUpdate {
        updates: Vec<(String, Vec<(u64, u16, u64)>)>,
    },
    /// Batch delete for AST entries across multiple files
    AstBatchDelete { files: Vec<String> },
    /// Mixed batch of different operation types
    Mixed { operations: Vec<BatchOperation> },
}

// ─── Batch Operation ───────────────────────────────────────────────────────────

/// Individual operation within a batch. Mirrors TelemetryRequest variants
/// but is optimized for batch processing.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum BatchOperation {
    AstUpdate {
        file_path: String,
        triples: Vec<(u64, u16, u64)>,
    },
    AstDelete {
        file_path: String,
    },
    PresenceUpdate {
        cursor_line: usize,
        cursor_col: usize,
    },
}

impl From<BatchOperation> for TelemetryRequest {
    fn from(op: BatchOperation) -> Self {
        match op {
            BatchOperation::AstUpdate { file_path, triples } => {
                TelemetryRequest::AstUpdate { file_path, triples }
            }
            BatchOperation::AstDelete { file_path } => TelemetryRequest::AstDelete { file_path },
            BatchOperation::PresenceUpdate {
                cursor_line,
                cursor_col,
            } => TelemetryRequest::PresenceUpdate {
                cursor_line,
                cursor_col,
            },
        }
    }
}

impl From<TelemetryRequest> for BatchOperation {
    fn from(req: TelemetryRequest) -> Self {
        match req {
            TelemetryRequest::AstUpdate { file_path, triples } => {
                BatchOperation::AstUpdate { file_path, triples }
            }
            TelemetryRequest::AstDelete { file_path } => BatchOperation::AstDelete { file_path },
            TelemetryRequest::PresenceUpdate {
                cursor_line,
                cursor_col,
            } => BatchOperation::PresenceUpdate {
                cursor_line,
                cursor_col,
            },
        }
    }
}

// ─── Batch Response ────────────────────────────────────────────────────────────

/// Response from processing a batch of operations.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BatchResponse {
    /// Individual results for each operation in the batch
    pub results: Vec<BatchResult>,
    /// Total latency for processing the entire batch in microseconds
    pub total_latency_us: u64,
    /// Number of operations processed
    pub operations_count: usize,
}

impl BatchResponse {
    /// Create an empty batch response
    pub fn empty() -> Self {
        Self {
            results: Vec::new(),
            total_latency_us: 0,
            operations_count: 0,
        }
    }

    /// Check if all operations in the batch succeeded
    pub fn all_success(&self) -> bool {
        self.results.iter().all(|r| r.success)
    }

    /// Get count of successful operations
    pub fn success_count(&self) -> usize {
        self.results.iter().filter(|r| r.success).count()
    }

    /// Get count of failed operations
    pub fn failure_count(&self) -> usize {
        self.results.iter().filter(|r| !r.success).count()
    }
}

// ─── Batch Result ──────────────────────────────────────────────────────────────

/// Result of a single operation within a batch.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BatchResult {
    /// Whether the operation succeeded
    pub success: bool,
    /// Optional warning message
    pub warning: Option<String>,
    /// Index of this operation within the original batch
    pub operation_index: usize,
}

impl From<TelemetryResponse> for BatchResult {
    fn from(resp: TelemetryResponse) -> Self {
        Self {
            success: resp.success,
            warning: resp.warning,
            operation_index: 0,
        }
    }
}

impl From<BatchResult> for TelemetryResponse {
    fn from(result: BatchResult) -> Self {
        TelemetryResponse {
            success: result.success,
            warning: result.warning,
        }
    }
}

// ─── Batch Processor ───────────────────────────────────────────────────────────

/// Default maximum batch size
const DEFAULT_MAX_BATCH_SIZE: usize = 64;

/// Default flush threshold - auto-flush when queue reaches this size
const DEFAULT_FLUSH_THRESHOLD: usize = 16;

/// Batch processor for queueing and processing multiple IPC operations.
/// Supports auto-flush when the queue reaches the threshold, and enforces
/// a maximum batch size to prevent memory issues.
pub struct BatchProcessor {
    /// Maximum number of operations allowed in a single batch
    pub max_batch_size: usize,
    /// Queue of pending operations
    pub queue: Vec<BatchOperation>,
    /// Auto-flush threshold - flush() is called automatically when queue reaches this size
    pub flush_threshold: usize,
}

impl Default for BatchProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl BatchProcessor {
    /// Create a new BatchProcessor with default settings
    pub fn new() -> Self {
        Self {
            max_batch_size: DEFAULT_MAX_BATCH_SIZE,
            queue: Vec::new(),
            flush_threshold: DEFAULT_FLUSH_THRESHOLD,
        }
    }

    /// Create a BatchProcessor with custom settings
    pub fn with_config(max_batch_size: usize, flush_threshold: usize) -> Self {
        Self {
            max_batch_size,
            queue: Vec::new(),
            flush_threshold,
        }
    }

    /// Queue an operation for batch processing.
    /// Returns Some(BatchResponse) if auto-flush was triggered (queue reached threshold),
    /// or None if the operation was simply added to the queue.
    pub fn queue_operation(&mut self, op: BatchOperation) -> Option<BatchResponse> {
        // Enforce max batch size
        if self.queue.len() >= self.max_batch_size {
            // Force flush before adding new operation
            let response = self.flush();
            self.queue.push(op);
            // Check if we need to flush again after adding
            if self.queue.len() >= self.flush_threshold {
                return Some(self.flush());
            }
            return Some(response);
        }

        self.queue.push(op);

        // Auto-flush if threshold reached
        if self.queue.len() >= self.flush_threshold {
            return Some(self.flush());
        }

        None
    }

    /// Process all queued operations and return a batch response.
    /// This is a local simulation that processes operations without
    /// going through the shared memory channel.
    pub fn flush(&mut self) -> BatchResponse {
        if self.queue.is_empty() {
            return BatchResponse::empty();
        }

        let start = Instant::now();
        let operations_count = self.queue.len();
        let mut results = Vec::with_capacity(operations_count);

        for (index, _op) in self.queue.drain(..).enumerate() {
            // Simulate successful processing of each operation
            // In a real implementation, this would send through shmem
            results.push(BatchResult {
                success: true,
                warning: None,
                operation_index: index,
            });
        }

        let total_latency_us = start.elapsed().as_micros() as u64;

        BatchResponse {
            results,
            total_latency_us,
            operations_count,
        }
    }

    /// Process queued operations using a custom handler function.
    /// This allows integration with the actual telemetry system.
    pub fn flush_with<F>(&mut self, mut handler: F) -> BatchResponse
    where
        F: FnMut(TelemetryRequest) -> TelemetryResponse,
    {
        if self.queue.is_empty() {
            return BatchResponse::empty();
        }

        let start = Instant::now();
        let operations_count = self.queue.len();
        let mut results = Vec::with_capacity(operations_count);

        for (index, op) in self.queue.drain(..).enumerate() {
            let req: TelemetryRequest = op.into();
            let resp = handler(req);
            results.push(BatchResult {
                success: resp.success,
                warning: resp.warning,
                operation_index: index,
            });
        }

        let total_latency_us = start.elapsed().as_micros() as u64;

        BatchResponse {
            results,
            total_latency_us,
            operations_count,
        }
    }

    /// Get the number of pending operations in the queue
    pub fn pending_count(&self) -> usize {
        self.queue.len()
    }

    /// Check if the queue is empty
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Clear all pending operations without processing them
    pub fn clear(&mut self) {
        self.queue.clear();
    }

    /// Create a BatchRequest from the current queue without clearing it
    pub fn to_batch_request(&self) -> BatchRequest {
        BatchRequest::Mixed {
            operations: self.queue.clone(),
        }
    }

    /// Create a BatchRequest from the current queue and clear it
    pub fn take_batch_request(&mut self) -> BatchRequest {
        let ops = std::mem::take(&mut self.queue);
        BatchRequest::Mixed { operations: ops }
    }
}

// ─── Batch Request Constructors ────────────────────────────────────────────────

impl BatchRequest {
    /// Create an AstBatchUpdate from a list of file/triples pairs
    pub fn ast_batch_update(updates: Vec<(String, Vec<(u64, u16, u64)>)>) -> Self {
        BatchRequest::AstBatchUpdate { updates }
    }

    /// Create an AstBatchDelete from a list of file paths
    pub fn ast_batch_delete(files: Vec<String>) -> Self {
        BatchRequest::AstBatchDelete { files }
    }

    /// Create a Mixed batch from a list of operations
    pub fn mixed(operations: Vec<BatchOperation>) -> Self {
        BatchRequest::Mixed { operations }
    }

    /// Get the number of operations in this batch request
    pub fn len(&self) -> usize {
        match self {
            BatchRequest::AstBatchUpdate { updates } => updates.len(),
            BatchRequest::AstBatchDelete { files } => files.len(),
            BatchRequest::Mixed { operations } => operations.len(),
        }
    }

    /// Check if the batch is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ─── Unit Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_operation_creation() {
        let op = BatchOperation::AstUpdate {
            file_path: "src/main.rs".to_string(),
            triples: vec![(100, 1, 200)],
        };
        match op {
            BatchOperation::AstUpdate { file_path, triples } => {
                assert_eq!(file_path, "src/main.rs");
                assert_eq!(triples.len(), 1);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_batch_request_ast_update() {
        let req = BatchRequest::AstBatchUpdate {
            updates: vec![
                ("src/a.rs".to_string(), vec![(1, 2, 3)]),
                ("src/b.rs".to_string(), vec![(4, 5, 6)]),
            ],
        };
        assert_eq!(req.len(), 2);
        assert!(!req.is_empty());
    }

    #[test]
    fn test_batch_request_ast_delete() {
        let req = BatchRequest::AstBatchDelete {
            files: vec!["src/a.rs".to_string(), "src/b.rs".to_string()],
        };
        assert_eq!(req.len(), 2);
    }

    #[test]
    fn test_batch_request_mixed() {
        let req = BatchRequest::Mixed {
            operations: vec![
                BatchOperation::AstUpdate {
                    file_path: "test.rs".to_string(),
                    triples: vec![],
                },
                BatchOperation::AstDelete {
                    file_path: "old.rs".to_string(),
                },
            ],
        };
        assert_eq!(req.len(), 2);
    }

    #[test]
    fn test_batch_request_empty() {
        let req = BatchRequest::Mixed { operations: vec![] };
        assert_eq!(req.len(), 0);
        assert!(req.is_empty());
    }

    #[test]
    fn test_batch_response_empty() {
        let resp = BatchResponse::empty();
        assert_eq!(resp.operations_count, 0);
        assert_eq!(resp.results.len(), 0);
        assert_eq!(resp.total_latency_us, 0);
        assert!(resp.all_success());
    }

    #[test]
    fn test_batch_response_all_success() {
        let resp = BatchResponse {
            results: vec![
                BatchResult {
                    success: true,
                    warning: None,
                    operation_index: 0,
                },
                BatchResult {
                    success: true,
                    warning: Some("test warning".to_string()),
                    operation_index: 1,
                },
            ],
            total_latency_us: 100,
            operations_count: 2,
        };
        assert!(resp.all_success());
        assert_eq!(resp.success_count(), 2);
        assert_eq!(resp.failure_count(), 0);
    }

    #[test]
    fn test_batch_response_with_failures() {
        let resp = BatchResponse {
            results: vec![
                BatchResult {
                    success: true,
                    warning: None,
                    operation_index: 0,
                },
                BatchResult {
                    success: false,
                    warning: Some("error".to_string()),
                    operation_index: 1,
                },
            ],
            total_latency_us: 50,
            operations_count: 2,
        };
        assert!(!resp.all_success());
        assert_eq!(resp.success_count(), 1);
        assert_eq!(resp.failure_count(), 1);
    }

    #[test]
    fn test_batch_serialization_roundtrip() {
        let req = BatchRequest::Mixed {
            operations: vec![
                BatchOperation::AstUpdate {
                    file_path: "src/main.rs".to_string(),
                    triples: vec![(100, 1, 200), (300, 2, 400)],
                },
                BatchOperation::PresenceUpdate {
                    cursor_line: 42,
                    cursor_col: 10,
                },
            ],
        };

        let json = serde_json::to_string(&req).unwrap();
        let deserialized: BatchRequest = serde_json::from_str(&json).unwrap();

        match deserialized {
            BatchRequest::Mixed { operations } => {
                assert_eq!(operations.len(), 2);
                match &operations[0] {
                    BatchOperation::AstUpdate { file_path, triples } => {
                        assert_eq!(file_path, "src/main.rs");
                        assert_eq!(triples.len(), 2);
                    }
                    _ => panic!("Wrong variant"),
                }
            }
            _ => panic!("Wrong batch request variant"),
        }
    }

    #[test]
    fn test_batch_response_serialization() {
        let resp = BatchResponse {
            results: vec![BatchResult {
                success: true,
                warning: None,
                operation_index: 0,
            }],
            total_latency_us: 42,
            operations_count: 1,
        };

        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: BatchResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.operations_count, 1);
        assert_eq!(deserialized.total_latency_us, 42);
        assert!(deserialized.results[0].success);
    }

    #[test]
    fn test_batch_processor_default() {
        let proc = BatchProcessor::new();
        assert_eq!(proc.max_batch_size, 64);
        assert_eq!(proc.flush_threshold, 16);
        assert_eq!(proc.pending_count(), 0);
        assert!(proc.is_empty());
    }

    #[test]
    fn test_batch_processor_with_config() {
        let proc = BatchProcessor::with_config(128, 32);
        assert_eq!(proc.max_batch_size, 128);
        assert_eq!(proc.flush_threshold, 32);
    }

    #[test]
    fn test_batch_processor_queue_operation() {
        let mut proc = BatchProcessor::new();
        let op = BatchOperation::AstUpdate {
            file_path: "test.rs".to_string(),
            triples: vec![],
        };

        let result = proc.queue_operation(op);
        assert!(result.is_none()); // Below threshold
        assert_eq!(proc.pending_count(), 1);
    }

    #[test]
    fn test_batch_processor_auto_flush_at_threshold() {
        let mut proc = BatchProcessor::with_config(64, 3); // Low threshold for testing

        // Add operations up to threshold
        for i in 0..2 {
            let op = BatchOperation::AstDelete {
                file_path: format!("file{}.rs", i),
            };
            let result = proc.queue_operation(op);
            assert!(result.is_none());
        }

        // Third operation should trigger auto-flush
        let op = BatchOperation::AstDelete {
            file_path: "file2.rs".to_string(),
        };
        let result = proc.queue_operation(op);
        assert!(result.is_some());

        let response = result.unwrap();
        assert_eq!(response.operations_count, 3);
        assert_eq!(proc.pending_count(), 0);
    }

    #[test]
    fn test_batch_processor_manual_flush() {
        let mut proc = BatchProcessor::new();

        for i in 0..5 {
            let op = BatchOperation::PresenceUpdate {
                cursor_line: i,
                cursor_col: 0,
            };
            proc.queue.push(op);
        }

        assert_eq!(proc.pending_count(), 5);

        let response = proc.flush();
        assert_eq!(response.operations_count, 5);
        assert_eq!(response.results.len(), 5);
        assert!(response.all_success());
        assert_eq!(proc.pending_count(), 0);
    }

    #[test]
    fn test_batch_processor_flush_empty() {
        let mut proc = BatchProcessor::new();
        let response = proc.flush();
        assert_eq!(response.operations_count, 0);
        assert!(response.results.is_empty());
    }

    #[test]
    fn test_batch_processor_mixed_operations() {
        let mut proc = BatchProcessor::new();

        proc.queue.push(BatchOperation::AstUpdate {
            file_path: "a.rs".to_string(),
            triples: vec![(1, 2, 3)],
        });
        proc.queue.push(BatchOperation::AstDelete {
            file_path: "b.rs".to_string(),
        });
        proc.queue.push(BatchOperation::PresenceUpdate {
            cursor_line: 10,
            cursor_col: 5,
        });

        let response = proc.flush();
        assert_eq!(response.operations_count, 3);
        assert_eq!(response.results.len(), 3);

        // Verify operation indices
        for (i, result) in response.results.iter().enumerate() {
            assert_eq!(result.operation_index, i);
        }
    }

    #[test]
    fn test_batch_processor_max_batch_size_enforcement() {
        let mut proc = BatchProcessor::with_config(5, 10); // Max 5, threshold 10

        // Fill up to max
        for i in 0..5 {
            proc.queue.push(BatchOperation::AstDelete {
                file_path: format!("{}.rs", i),
            });
        }

        assert_eq!(proc.pending_count(), 5);

        // Adding one more should trigger a flush first
        let op = BatchOperation::AstDelete {
            file_path: "overflow.rs".to_string(),
        };
        let result = proc.queue_operation(op);
        assert!(result.is_some());

        // After flush + add, should have 1 item
        assert_eq!(proc.pending_count(), 1);
    }

    #[test]
    fn test_batch_processor_clear() {
        let mut proc = BatchProcessor::new();

        for i in 0..10 {
            proc.queue.push(BatchOperation::PresenceUpdate {
                cursor_line: i,
                cursor_col: 0,
            });
        }

        assert_eq!(proc.pending_count(), 10);
        proc.clear();
        assert_eq!(proc.pending_count(), 0);
    }

    #[test]
    fn test_batch_processor_to_batch_request() {
        let mut proc = BatchProcessor::new();

        proc.queue.push(BatchOperation::AstUpdate {
            file_path: "test.rs".to_string(),
            triples: vec![],
        });

        let req = proc.to_batch_request();
        assert_eq!(req.len(), 1);
        // Queue should still have the item
        assert_eq!(proc.pending_count(), 1);
    }

    #[test]
    fn test_batch_processor_take_batch_request() {
        let mut proc = BatchProcessor::new();

        proc.queue.push(BatchOperation::AstUpdate {
            file_path: "test.rs".to_string(),
            triples: vec![],
        });

        let req = proc.take_batch_request();
        assert_eq!(req.len(), 1);
        // Queue should be empty now
        assert_eq!(proc.pending_count(), 0);
    }

    #[test]
    fn test_batch_operation_to_telemetry_request() {
        let op = BatchOperation::AstUpdate {
            file_path: "src/lib.rs".to_string(),
            triples: vec![(100, 1, 200)],
        };

        let req: TelemetryRequest = op.into();
        match req {
            TelemetryRequest::AstUpdate { file_path, triples } => {
                assert_eq!(file_path, "src/lib.rs");
                assert_eq!(triples.len(), 1);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_telemetry_request_to_batch_operation() {
        let req = TelemetryRequest::PresenceUpdate {
            cursor_line: 42,
            cursor_col: 10,
        };

        let op: BatchOperation = req.into();
        match op {
            BatchOperation::PresenceUpdate {
                cursor_line,
                cursor_col,
            } => {
                assert_eq!(cursor_line, 42);
                assert_eq!(cursor_col, 10);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_batch_result_to_telemetry_response() {
        let result = BatchResult {
            success: true,
            warning: Some("test".to_string()),
            operation_index: 0,
        };

        let resp: TelemetryResponse = result.into();
        assert!(resp.success);
        assert_eq!(resp.warning, Some("test".to_string()));
    }

    #[test]
    fn test_batch_processor_flush_with_handler() {
        let mut proc = BatchProcessor::new();

        proc.queue.push(BatchOperation::AstUpdate {
            file_path: "test.rs".to_string(),
            triples: vec![(1, 2, 3)],
        });
        proc.queue.push(BatchOperation::AstDelete {
            file_path: "old.rs".to_string(),
        });

        let response = proc.flush_with(|req| match req {
            TelemetryRequest::AstUpdate { .. } => TelemetryResponse {
                success: true,
                warning: None,
            },
            TelemetryRequest::AstDelete { .. } => TelemetryResponse {
                success: true,
                warning: Some("deleted".to_string()),
            },
            _ => TelemetryResponse {
                success: false,
                warning: None,
            },
        });

        assert_eq!(response.operations_count, 2);
        assert!(response.results[0].success);
        assert!(response.results[1].success);
        assert_eq!(response.results[1].warning, Some("deleted".to_string()));
    }

    #[test]
    fn test_batch_request_constructors() {
        let req1 = BatchRequest::ast_batch_update(vec![("a.rs".to_string(), vec![(1, 2, 3)])]);
        assert_eq!(req1.len(), 1);

        let req2 = BatchRequest::ast_batch_delete(vec!["a.rs".to_string()]);
        assert_eq!(req2.len(), 1);

        let req3 = BatchRequest::mixed(vec![BatchOperation::PresenceUpdate {
            cursor_line: 1,
            cursor_col: 2,
        }]);
        assert_eq!(req3.len(), 1);
    }
}
