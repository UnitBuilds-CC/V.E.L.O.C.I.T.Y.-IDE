//! Production-safe synchronization primitives with graceful error handling.

use std::sync::{Mutex, MutexGuard};

/// Extension trait for `Mutex<T>` providing poisoning-tolerant locking.
pub trait SafeMutex<T> {
    /// Acquire the lock, recovering from poisoning if necessary.
    fn lock_safe(&self) -> MutexGuard<'_, T>;
}

impl<T> SafeMutex<T> for Mutex<T> {
    fn lock_safe(&self) -> MutexGuard<'_, T> {
        match self.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                eprintln!("[WARN] Mutex poisoning recovered.");
                poisoned.into_inner()
            }
        }
    }
}
