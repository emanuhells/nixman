//! Build-history persistence.
//!
//! History is stored as a JSON array in `<cache_dir>/build-history.json`.
//! The full array is rewritten on every [`save`] call, which is acceptable
//! given that builds happen infrequently.

use std::path::Path;

use crate::builder::types::BuildHistoryEntry;

/// File name of the history file inside the cache directory.
const HISTORY_FILE: &str = "build-history.json";

/// Append `entry` to the build-history file at `<cache_dir>/build-history.json`.
///
/// If the cache directory does not exist it is created.  Any I/O or
/// serialisation error is silently ignored so that a history write failure
/// never disrupts the build workflow.
pub fn save(entry: &BuildHistoryEntry, cache_dir: &Path) {
    let history_path = cache_dir.join(HISTORY_FILE);

    // Load existing entries (returns an empty Vec on any error).
    let mut entries = load(cache_dir);
    entries.push(entry.clone());

    if let Ok(json) = serde_json::to_string_pretty(&entries) {
        // Ensure the directory exists before writing.
        if std::fs::create_dir_all(cache_dir).is_ok() {
            let _ = std::fs::write(&history_path, json);
        }
    }
}

/// Load all [`BuildHistoryEntry`] records from `<cache_dir>/build-history.json`.
///
/// Returns an empty [`Vec`] if the file does not exist, cannot be read, or
/// contains invalid JSON.
pub fn load(cache_dir: &Path) -> Vec<BuildHistoryEntry> {
    let history_path = cache_dir.join(HISTORY_FILE);

    std::fs::read_to_string(&history_path)
        .ok()
        .and_then(|contents| serde_json::from_str(&contents).ok())
        .unwrap_or_default()
}
