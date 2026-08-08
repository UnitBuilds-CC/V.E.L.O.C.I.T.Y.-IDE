//! Graceful shutdown handling for production deployments.
//!
//! Provides signal handling for SIGTERM/SIGINT (Ctrl+C) to allow the application
//! to clean up resources, flush buffers, and exit cleanly rather than being killed
//! abruptly by the OS.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Global shutdown flag that can be checked throughout the application.
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Request a graceful shutdown. This is called by signal handlers.
pub fn request_shutdown() {
    SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
}

/// Check if shutdown has been requested.
pub fn is_shutdown_requested() -> bool {
    SHUTDOWN_REQUESTED.load(Ordering::SeqCst)
}

/// Install signal handlers for graceful shutdown.
///
/// This should be called early in `main()` before any critical work begins.
/// Returns an `Arc<AtomicBool>` that can be checked throughout the application.
pub fn install_shutdown_handlers() -> Arc<AtomicBool> {
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let flag_clone = shutdown_flag.clone();

    // Handle Ctrl+C (SIGINT on Unix, CTRL_C_EVENT on Windows)
    ctrlc::set_handler(move || {
        eprintln!("\n[INFO] Received interrupt signal, initiating graceful shutdown...");
        flag_clone.store(true, Ordering::SeqCst);
        SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
    })
    .expect("Failed to install Ctrl+C handler");

    shutdown_flag
}

/// Wait for shutdown to be requested, checking periodically.
///
/// This is useful for long-running loops that should exit cleanly.
/// Returns `true` if shutdown was requested, `false` if timeout elapsed.
pub fn wait_for_shutdown(timeout: std::time::Duration) -> bool {
    let start = std::time::Instant::now();
    while !is_shutdown_requested() {
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shutdown_flag() {
        // Reset flag
        SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
        
        assert!(!is_shutdown_requested());
        request_shutdown();
        assert!(is_shutdown_requested());
        
        // Reset for other tests
        SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
    }

    #[test]
    fn test_wait_for_shutdown_timeout() {
        SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
        
        // Should timeout without shutdown
        let result = wait_for_shutdown(std::time::Duration::from_millis(50));
        assert!(!result);
        
        // Request shutdown and wait
        request_shutdown();
        let result = wait_for_shutdown(std::time::Duration::from_millis(50));
        assert!(result);
        
        // Reset for other tests
        SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
    }
}
