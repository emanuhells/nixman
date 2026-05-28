use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::RecvTimeoutError,
    Arc,
};
use std::time::{Duration, Instant};

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;

use crate::watcher::debounce::Debouncer;
use crate::watcher::types::{FileEvent, WatcherConfig};

/// Opaque handle returned by [`start`].  Call [`WatchHandle::stop`] to
/// tear down the watcher and its event-loop thread.
pub struct WatchHandle {
    stop: Arc<AtomicBool>,
}

impl WatchHandle {
    /// Signal the background thread to exit.  In-flight events that already
    /// passed through the debounce window will still be delivered before the
    /// channel is drained.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

// ── path filtering ────────────────────────────────────────────────────────────

/// Returns `true` when `path` is a `.nix` file not inside an ignored
/// directory (`.git`, `node_modules`, `target`).
fn is_relevant(path: &Path) -> bool {
    if path.extension().and_then(|e| e.to_str()) != Some("nix") {
        return false;
    }
    for component in path.components() {
        if let std::path::Component::Normal(name) = component {
            match name.to_str().unwrap_or("") {
                ".git" | "node_modules" | "target" => return false,
                _ => {}
            }
        }
    }
    true
}

/// Map a raw `notify::Event` to a [`FileEvent`], discarding irrelevant paths
/// and unrecognised event kinds.
fn to_file_event(event: Event) -> Option<FileEvent> {
    // Take the first path in the event that passes the filter (usually exactly one).
    let path = event.paths.into_iter().find(|p| is_relevant(p))?;
    match event.kind {
        EventKind::Create(_) => Some(FileEvent::Created(path)),
        EventKind::Modify(_) => Some(FileEvent::Modified(path)),
        EventKind::Remove(_) => Some(FileEvent::Deleted(path)),
        _ => None,
    }
}

// ── public API ────────────────────────────────────────────────────────────────

/// Start watching `dir` recursively.  Filtered, debounced [`FileEvent`]s are
/// forwarded through `tx`.
///
/// The watcher runs on a dedicated `std::thread` so this function is
/// synchronous and can be called from both sync and async code.  It requires
/// an active Tokio runtime to send events through the async channel.
///
/// # Errors
///
/// Returns a [`notify::Error`] if the platform watcher cannot be initialised
/// or `dir` cannot be registered for watching.
pub fn start(
    dir: PathBuf,
    tx: mpsc::Sender<FileEvent>,
    config: WatcherConfig,
) -> Result<WatchHandle, notify::Error> {
    // Raw notify events travel on a std channel; the callback is kept simple
    // and `Send`.
    let (raw_tx, raw_rx) = std::sync::mpsc::channel::<notify::Result<Event>>();

    let mut watcher = RecommendedWatcher::new(
        move |res| {
            let _ = raw_tx.send(res);
        },
        Config::default(),
    )?;
    watcher.watch(&dir, RecursiveMode::Recursive)?;

    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();

    // Capture the Tokio runtime handle from the calling context so the
    // background thread can drive the async `Sender::send` future.
    let rt_handle = tokio::runtime::Handle::try_current().ok();

    std::thread::spawn(move || {
        // Keep the watcher alive for the duration of the event loop.
        let _watcher = watcher;

        let mut debouncer = Debouncer::new(config.debounce_ms);
        // Poll interval for the std channel; also the granularity at which we
        // check the stop flag and drain the debounce buffer.
        let poll = Duration::from_millis(50);

        // Send `event` over the mpsc channel.
        //
        // Uses `Handle::block_on` when a Tokio runtime is available
        // (the normal case in Tauri), otherwise falls back to `try_send` so
        // the background thread never blocks indefinitely.
        //
        // Returns `false` when the receiver has been dropped (channel closed).
        let emit = |event: FileEvent| -> bool {
            if let Some(ref handle) = rt_handle {
                // Drive the async send to completion on this OS thread.
                // Safe here because we are NOT inside a Tokio async task.
                handle.block_on(tx.send(event)).is_ok()
            } else {
                tx.try_send(event).is_ok()
            }
        };

        loop {
            if stop_clone.load(Ordering::Relaxed) {
                break;
            }

            match raw_rx.recv_timeout(poll) {
                Ok(Ok(event)) => {
                    if let Some(fe) = to_file_event(event) {
                        // push() returns events from *other* paths that
                        // just expired while this new event came in.
                        for expired in debouncer.push(fe) {
                            if !emit(expired) {
                                return;
                            }
                        }
                    }
                    // After any new event, drain the debounce buffer so
                    // events are not held back longer than necessary.
                    for expired in debouncer.drain_expired(Instant::now()) {
                        if !emit(expired) {
                            return;
                        }
                    }
                }
                Ok(Err(e)) => {
                    eprintln!("[watcher] notify error: {e}");
                }
                Err(RecvTimeoutError::Timeout) => {
                    // Periodic drain — fires every `poll` ms when the
                    // directory is quiet.
                    for expired in debouncer.drain_expired(Instant::now()) {
                        if !emit(expired) {
                            return;
                        }
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }

        // Flush any events still inside the debounce window on shutdown.
        for event in debouncer.flush() {
            emit(event);
        }
    });

    Ok(WatchHandle { stop })
}

/// Stop the watcher represented by `handle`.
///
/// This is a convenience wrapper around [`WatchHandle::stop`].
pub fn stop(handle: WatchHandle) {
    handle.stop();
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_relevant_accepts_nix_files() {
        assert!(is_relevant(Path::new("/etc/nixos/config.nix")));
        assert!(is_relevant(Path::new("modules/services.nix")));
    }

    #[test]
    fn is_relevant_rejects_non_nix() {
        assert!(!is_relevant(Path::new("/etc/nixos/README.md")));
        assert!(!is_relevant(Path::new("flake.lock")));
        assert!(!is_relevant(Path::new("build.rs")));
    }

    #[test]
    fn is_relevant_rejects_ignored_dirs() {
        assert!(!is_relevant(Path::new(".git/HEAD")));
        assert!(!is_relevant(Path::new("node_modules/pkg/index.nix")));
        assert!(!is_relevant(Path::new("target/debug/config.nix")));
    }
}
