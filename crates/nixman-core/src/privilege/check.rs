use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// Returns `true` if the given path is owned by root (uid 0), meaning a write
/// operation against it will require privilege escalation via polkit.
///
/// Returns `false` if the metadata cannot be read (e.g. the path does not exist
/// yet) — in that case the operation should be attempted normally and will fail
/// with a permission error if elevation was actually needed.
pub fn needs_elevation(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(meta) => meta.uid() == 0,
        Err(_) => false,
    }
}
