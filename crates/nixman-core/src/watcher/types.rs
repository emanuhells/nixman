use std::path::PathBuf;
use std::time::SystemTime;

/// Events emitted when a `.nix` file changes on disk.
#[derive(Debug, Clone)]
pub enum FileEvent {
    Created(PathBuf),
    Modified(PathBuf),
    Deleted(PathBuf),
}

impl FileEvent {
    /// Borrow the path this event refers to.
    pub fn path(&self) -> &PathBuf {
        match self {
            Self::Created(p) | Self::Modified(p) | Self::Deleted(p) => p,
        }
    }
}

/// Details about a file that was externally modified while the user has
/// unsaved (pending) edits to it in the editor.
#[derive(Debug, Clone)]
pub struct ConflictInfo {
    /// The file that triggered the conflict.
    pub path: PathBuf,
    /// When the external modification was detected (from filesystem metadata).
    pub external_modified_at: SystemTime,
    /// Whether the in-app editor holds unsaved edits for this file.
    pub has_pending_edits: bool,
}

/// Configuration for the file watcher.
#[derive(Debug, Clone)]
pub struct WatcherConfig {
    /// Milliseconds to wait after the last change before emitting an event
    /// for a file.  Rapid changes within this window are coalesced into one.
    pub debounce_ms: u64,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self { debounce_ms: 300 }
    }
}
