//! Production-safe synchronization primitives with graceful error handling.
//!
//! Standard `.lock().unwrap()` causes the entire application to crash if any thread
//! panics while holding a lock (mutex poisoning). This module provides alternatives
//! that handle poisoning gracefully, allowing the application to recover.

use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Extension trait for `Mutex<T>` providing poisoning-tolerant locking.
pub trait SafeMutex<T> {
    /// Acquire the lock, recovering from poisoning if necessary.
    ///
    /// If a previous thread panicked while holding this lock, the mutex becomes
    /// "poisoned". Instead of panicking, this method recovers the lock and logs
    /// a warning, allowing the application to continue.
    fn lock_safe(&self) -> MutexGuard<'_, T>;

    /// Try to acquire the lock without blocking.
    ///
    /// Returns `None` if the lock is currently held by another thread.
    fn try_lock_safe(&self) -> Option<MutexGuard<'_, T>>;
}

impl<T> SafeMutex<T> for Mutex<T> {
    fn lock_safe(&self) -> MutexGuard<'_, T> {
        match self.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                eprintln!(
                    "[WARN] Mutex poisoning recovered. This indicates a thread panicked while holding the lock."
                );
                poisoned.into_inner()
            }
        }
    }

    fn try_lock_safe(&self) -> Option<MutexGuard<'_, T>> {
        match self.try_lock() {
            Ok(guard) => Some(guard),
            Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                eprintln!("[WARN] Mutex poisoning recovered (try_lock).");
                Some(poisoned.into_inner())
            }
            Err(std::sync::TryLockError::WouldBlock) => None,
        }
    }
}

/// Implementation for `Arc<Mutex<T>>` — dereferences through the Arc.
impl<T> SafeMutex<T> for Arc<Mutex<T>> {
    fn lock_safe(&self) -> MutexGuard<'_, T> {
        (**self).lock_safe()
    }

    fn try_lock_safe(&self) -> Option<MutexGuard<'_, T>> {
        (**self).try_lock_safe()
    }
}

/// Extension trait for `RwLock<T>` providing poisoning-tolerant locking.
pub trait SafeRwLock<T> {
    /// Acquire read access, recovering from poisoning if necessary.
    fn read_safe(&self) -> RwLockReadGuard<'_, T>;

    /// Acquire write access, recovering from poisoning if necessary.
    fn write_safe(&self) -> RwLockWriteGuard<'_, T>;

    /// Try to acquire read access without blocking.
    fn try_read_safe(&self) -> Option<RwLockReadGuard<'_, T>>;

    /// Try to acquire write access without blocking.
    fn try_write_safe(&self) -> Option<RwLockWriteGuard<'_, T>>;
}

impl<T> SafeRwLock<T> for RwLock<T> {
    fn read_safe(&self) -> RwLockReadGuard<'_, T> {
        match self.read() {
            Ok(guard) => guard,
            Err(poisoned) => {
                eprintln!("[WARN] RwLock poisoning recovered (read).");
                poisoned.into_inner()
            }
        }
    }

    fn write_safe(&self) -> RwLockWriteGuard<'_, T> {
        match self.write() {
            Ok(guard) => guard,
            Err(poisoned) => {
                eprintln!("[WARN] RwLock poisoning recovered (write).");
                poisoned.into_inner()
            }
        }
    }

    fn try_read_safe(&self) -> Option<RwLockReadGuard<'_, T>> {
        match self.try_read() {
            Ok(guard) => Some(guard),
            Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                eprintln!("[WARN] RwLock poisoning recovered (try_read).");
                Some(poisoned.into_inner())
            }
            Err(std::sync::TryLockError::WouldBlock) => None,
        }
    }

    fn try_write_safe(&self) -> Option<RwLockWriteGuard<'_, T>> {
        match self.try_write() {
            Ok(guard) => Some(guard),
            Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                eprintln!("[WARN] RwLock poisoning recovered (try_write).");
                Some(poisoned.into_inner())
            }
            Err(std::sync::TryLockError::WouldBlock) => None,
        }
    }
}

/// Implementation for `Arc<RwLock<T>>` — dereferences through the Arc.
impl<T> SafeRwLock<T> for Arc<RwLock<T>> {
    fn read_safe(&self) -> RwLockReadGuard<'_, T> {
        (**self).read_safe()
    }

    fn write_safe(&self) -> RwLockWriteGuard<'_, T> {
        (**self).write_safe()
    }

    fn try_read_safe(&self) -> Option<RwLockReadGuard<'_, T>> {
        (**self).try_read_safe()
    }

    fn try_write_safe(&self) -> Option<RwLockWriteGuard<'_, T>> {
        (**self).try_write_safe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_safe_mutex_normal_usage() {
        let mutex = Mutex::new(42);
        let guard = mutex.lock_safe();
        assert_eq!(*guard, 42);
    }

    #[test]
    fn test_safe_mutex_poisoning_recovery() {
        let mutex = Arc::new(Mutex::new(42));
        let mutex_clone = mutex.clone();

        // Spawn a thread that will panic while holding the lock
        let handle = thread::spawn(move || {
            let _guard = mutex_clone.lock().unwrap();
            panic!("intentional panic for testing");
        });

        // Wait for the thread to panic
        let _ = handle.join();

        // The mutex is now poisoned, but we can still recover it
        let guard = mutex.lock_safe();
        assert_eq!(*guard, 42);
    }

    #[test]
    fn test_safe_mutex_try_lock() {
        let mutex = Mutex::new(42);
        let guard = mutex.lock_safe();

        // Lock is held, try_lock should return None
        assert!(mutex.try_lock_safe().is_none());

        drop(guard);

        // Lock is free, try_lock should succeed
        let guard2 = mutex.try_lock_safe();
        assert!(guard2.is_some());
    }
}
