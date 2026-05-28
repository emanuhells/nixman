use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::watcher::types::ConflictInfo;

/// Check whether `path` was externally modified while the application still
/// holds unsaved edits for it.
///
/// Returns `Some(ConflictInfo)` when `path` is found in `pending_changes`,
/// `None` when there is no conflict.
///
/// # Arguments
///
/// * `path`            – The file that changed on disk.
/// * `pending_changes` – Set of paths the editor currently has unsaved edits for.
pub fn check(path: &Path, pending_changes: &HashSet<PathBuf>) -> Option<ConflictInfo> {
    if !pending_changes.contains(path) {
        return None;
    }

    // Best-effort: read the mtime from the filesystem.  Fall back to "now"
    // if the file has been deleted or the metadata cannot be read.
    let external_modified_at = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or_else(|_| SystemTime::now());

    Some(ConflictInfo {
        path: path.to_path_buf(),
        external_modified_at,
        has_pending_edits: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_conflict_when_path_not_pending() {
        let pending: HashSet<PathBuf> = HashSet::new();
        assert!(check(Path::new("/etc/nixos/config.nix"), &pending).is_none());
    }

    #[test]
    fn conflict_detected_for_pending_path() {
        let path = PathBuf::from("/etc/nixos/config.nix");
        let mut pending = HashSet::new();
        pending.insert(path.clone());

        let info = check(&path, &pending).expect("should detect conflict");
        assert!(info.has_pending_edits);
        assert_eq!(info.path, path);
    }

    #[test]
    fn no_conflict_for_different_path() {
        let mut pending = HashSet::new();
        pending.insert(PathBuf::from("/etc/nixos/hardware.nix"));

        let result = check(Path::new("/etc/nixos/config.nix"), &pending);
        assert!(result.is_none());
    }
}
