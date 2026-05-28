use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::watcher::types::FileEvent;

/// Coalesces rapid file-system events for the same path.
///
/// An event is held back until `debounce_ms` milliseconds have elapsed with
/// no further event for that path.  If a new event arrives before the window
/// expires the timer is reset and the latest event type replaces the previous
/// one.
pub struct Debouncer {
    debounce: Duration,
    /// Map from path → (most-recent event, timestamp of most-recent arrival).
    pending: HashMap<PathBuf, (FileEvent, Instant)>,
}

impl Debouncer {
    pub fn new(debounce_ms: u64) -> Self {
        Self {
            debounce: Duration::from_millis(debounce_ms),
            pending: HashMap::new(),
        }
    }

    /// Record an incoming event and return any events whose debounce window
    /// has already expired (possibly from other paths).
    ///
    /// If an event for the same path is already pending its timer is reset and
    /// its event kind is updated to the latest one.
    pub fn push(&mut self, event: FileEvent) -> Vec<FileEvent> {
        let now = Instant::now();
        // Collect events from other paths that have already passed their window.
        let expired = self.drain_expired(now);
        // Insert or overwrite: resetting the debounce timer for this path.
        self.pending.insert(event.path().clone(), (event, now));
        expired
    }

    /// Return and remove all events whose debounce window has elapsed.
    pub fn drain_expired(&mut self, now: Instant) -> Vec<FileEvent> {
        let debounce = self.debounce;
        let mut expired = Vec::new();
        self.pending.retain(|_, (event, ts)| {
            if now.duration_since(*ts) >= debounce {
                expired.push(event.clone());
                false
            } else {
                true
            }
        });
        expired
    }

    /// Flush *all* remaining events regardless of timing (e.g. on shutdown).
    pub fn flush(&mut self) -> Vec<FileEvent> {
        self.pending.drain().map(|(_, (ev, _))| ev).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn modified(path: &str) -> FileEvent {
        FileEvent::Modified(PathBuf::from(path))
    }

    #[test]
    fn rapid_changes_coalesced() {
        let mut d = Debouncer::new(200);

        // Three rapid events for the same path — none should be emitted yet.
        assert!(d.push(modified("/etc/nixos/config.nix")).is_empty());
        assert!(d.push(modified("/etc/nixos/config.nix")).is_empty());
        let out = d.push(modified("/etc/nixos/config.nix"));
        assert!(out.is_empty(), "should still be within debounce window");

        // Wait for the window to expire, then drain.
        thread::sleep(Duration::from_millis(250));
        let expired = d.drain_expired(Instant::now());
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].path(), &PathBuf::from("/etc/nixos/config.nix"));
    }

    #[test]
    fn different_paths_emitted_separately() {
        let mut d = Debouncer::new(50);

        d.push(modified("/a.nix"));
        d.push(modified("/b.nix"));

        thread::sleep(Duration::from_millis(100));
        let expired = d.drain_expired(Instant::now());
        assert_eq!(expired.len(), 2);
    }

    #[test]
    fn flush_returns_all_pending() {
        let mut d = Debouncer::new(5000); // Very long window.
        d.push(modified("/a.nix"));
        d.push(modified("/b.nix"));

        let all = d.flush();
        assert_eq!(all.len(), 2);
        assert!(d.flush().is_empty(), "flush empties the map");
    }
}
