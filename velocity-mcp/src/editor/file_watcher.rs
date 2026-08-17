//! File watcher using the `notify` crate for instant external change detection.
//!
//! Watches the workspace root for file changes and sends events through a channel
//! so the UI can react to external edits (e.g., from another editor or git operations).

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

/// A file change event from the watcher.
#[derive(Debug, Clone)]
pub struct FileChangeEvent {
    pub path: PathBuf,
    pub kind: FileChangeKind,
    pub timestamp: Instant,
}

/// Kind of file change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChangeKind {
    Created,
    Modified,
    Deleted,
    Renamed,
}

/// File watcher state.
pub struct FileWatcher {
    _watcher: RecommendedWatcher,
    receiver: Receiver<Result<Event, notify::Error>>,
    /// Debounce: last time we processed events per path.
    last_event: std::collections::HashMap<PathBuf, Instant>,
    /// Debounce interval.
    debounce: Duration,
}

impl FileWatcher {
    /// Start watching the given workspace root.
    pub fn new(workspace_root: &Path) -> Option<Self> {
        let (tx, rx) = channel();

        let config = Config::default().with_poll_interval(Duration::from_millis(500));

        let mut watcher = match RecommendedWatcher::new(
            move |res| {
                let _ = tx.send(res);
            },
            config,
        ) {
            Ok(w) => w,
            Err(_) => return None,
        };

        if watcher
            .watch(workspace_root, RecursiveMode::Recursive)
            .is_err()
        {
            return None;
        }

        Some(Self {
            _watcher: watcher,
            receiver: rx,
            last_event: std::collections::HashMap::new(),
            debounce: Duration::from_millis(300),
        })
    }

    /// Poll for new file change events (non-blocking).
    /// Returns a deduplicated, debounced list of events.
    pub fn poll(&mut self) -> Vec<FileChangeEvent> {
        let now = Instant::now();
        let mut events = Vec::new();
        let mut seen_paths = HashSet::new();

        // Drain all pending events
        while let Ok(result) = self.receiver.try_recv() {
            if let Ok(event) = result {
                let kind = match event.kind {
                    EventKind::Create(_) => FileChangeKind::Created,
                    EventKind::Modify(_) => FileChangeKind::Modified,
                    EventKind::Remove(_) => FileChangeKind::Deleted,
                    _ => continue,
                };

                for path in event.paths {
                    // Skip directories
                    if path.is_dir() {
                        continue;
                    }

                    // Skip .velocity internal state files
                    if path.components().any(|c| c.as_os_str() == ".velocity") {
                        continue;
                    }

                    // Debounce: skip if we saw this path recently
                    if let Some(last) = self.last_event.get(&path) {
                        if now.duration_since(*last) < self.debounce {
                            continue;
                        }
                    }

                    // Deduplicate within this poll batch
                    if seen_paths.contains(&path) {
                        continue;
                    }
                    seen_paths.insert(path.clone());

                    self.last_event.insert(path.clone(), now);

                    events.push(FileChangeEvent {
                        path,
                        kind,
                        timestamp: now,
                    });
                }
            }
        }

        events
    }

    /// Clean up old debounce entries (call periodically).
    pub fn cleanup_stale(&mut self) {
        let cutoff = Instant::now() - Duration::from_secs(10);
        self.last_event.retain(|_, t| *t > cutoff);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_change_kind_equality() {
        assert_eq!(FileChangeKind::Created, FileChangeKind::Created);
        assert_ne!(FileChangeKind::Created, FileChangeKind::Deleted);
        assert_ne!(FileChangeKind::Modified, FileChangeKind::Renamed);
    }

    #[test]
    fn cleanup_stale_removes_old_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let mut watcher = FileWatcher::new(tmp.path()).unwrap();
        // Insert a stale entry manually
        watcher.last_event.insert(
            PathBuf::from("old.rs"),
            Instant::now() - Duration::from_secs(30),
        );
        // Insert a fresh entry
        watcher
            .last_event
            .insert(PathBuf::from("new.rs"), Instant::now());
        assert_eq!(watcher.last_event.len(), 2);
        watcher.cleanup_stale();
        assert_eq!(watcher.last_event.len(), 1);
        assert!(watcher
            .last_event
            .contains_key(PathBuf::from("new.rs").as_path()));
    }

    #[test]
    fn poll_returns_empty_when_no_events() {
        let tmp = tempfile::tempdir().unwrap();
        let mut watcher = FileWatcher::new(tmp.path()).unwrap();
        let events = watcher.poll();
        assert_eq!(events.len(), 0);
    }

    #[test]
    fn watcher_created_successfully_on_valid_path() {
        let tmp = tempfile::tempdir().unwrap();
        let watcher = FileWatcher::new(tmp.path());
        assert!(watcher.is_some());
    }
}
