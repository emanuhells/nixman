//! In-memory pending-changes buffer.
//!
//! [`PendingChanges`] holds all edits that have been queued but not yet
//! written to disk.  Changes are keyed by `option_path`; adding a change for
//! a path that is already buffered replaces the earlier change.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::nix_parser::{
    format::detect_indent,
    insert::add_option,
    writer::set_value,
};

use crate::config::types::{ConfigError, FileDiff, PendingChange};

// PendingChanges

/// An in-memory buffer of configuration changes that have not yet been
/// written to disk.
#[derive(Debug, Default)]
pub struct PendingChanges {
    /// The buffered changes, in insertion order.
    pub changes: Vec<PendingChange>,
}

impl PendingChanges {
    /// Create an empty buffer.
    pub fn new() -> Self {
        PendingChanges {
            changes: Vec::new(),
        }
    }

    /// Add `change` to the buffer.
    ///
    /// If a change for `change.option_path` is already buffered it is
    /// replaced rather than appended, so each path appears at most once.
    pub fn add(&mut self, change: PendingChange) {
        if let Some(existing) = self
            .changes
            .iter_mut()
            .find(|c| c.option_path == change.option_path)
        {
            *existing = change;
        } else {
            self.changes.push(change);
        }
    }

    /// Discard the pending change for `option_path`, if any.
    pub fn remove(&mut self, option_path: &str) {
        self.changes.retain(|c| c.option_path != option_path);
    }

    /// Return a slice over all buffered changes.
    pub fn list(&self) -> &[PendingChange] {
        &self.changes
    }

    /// Return the number of buffered changes.
    pub fn count(&self) -> usize {
        self.changes.len()
    }

    /// Discard every buffered change.
    pub fn clear(&mut self) {
        self.changes.clear();
    }

    /// For each file that has pending changes, produce the original on-disk
    /// source and the modified source (after all pending changes are applied
    /// in memory).
    ///
    /// Returns one [`FileDiff`] per affected file.  Files are read from disk
    /// at call time.
    pub fn generate_diffs(&self) -> Result<Vec<FileDiff>, ConfigError> {
        let by_file = group_by_file(&self.changes);
        let mut diffs = Vec::new();

        for (file, file_changes) in &by_file {
            let original = std::fs::read_to_string(file)?;
            let modified = apply_changes_to_source(&original, file_changes)?;
            diffs.push(FileDiff {
                file: file.clone(),
                original,
                modified,
            });
        }

        Ok(diffs)
    }
}

// Internal helpers (pub(crate) so editor.rs can reuse them)

/// Group `changes` by their target file.
///
/// Returns a map from `PathBuf` → the subset of `changes` that target that
/// file (as shared references into the input slice).
pub(crate) fn group_by_file(
    changes: &[PendingChange],
) -> HashMap<PathBuf, Vec<&PendingChange>> {
    let mut map: HashMap<PathBuf, Vec<&PendingChange>> = HashMap::new();
    for change in changes {
        map.entry(change.file.clone()).or_default().push(change);
    }
    map
}

/// Apply a set of pending changes for a single file to `source` and return
/// the resulting string.
///
/// # Application order
///
/// Changes that carry a `range` (updates to existing values) are applied from
/// the **highest** byte offset to the **lowest**.  This preserves the
/// validity of each range: a replacement at offset N cannot invalidate any
/// range that starts before N.
///
/// Changes without a `range` (new options to be inserted) are applied after
/// all range-based changes using [`add_option`], which re-parses the current
/// source for each insertion so positions are always fresh.
pub(crate) fn apply_changes_to_source(
    source: &str,
    changes: &[&PendingChange],
) -> Result<String, ConfigError> {
    // Split into range-based (update existing) and rangeless (insert new).
    let mut updates: Vec<&PendingChange> = changes
        .iter()
        .copied()
        .filter(|c| c.range.is_some())
        .collect();

    let inserts: Vec<&PendingChange> = changes
        .iter()
        .copied()
        .filter(|c| c.range.is_none())
        .collect();

    // Sort range-based changes by start offset descending so earlier byte
    // positions remain valid after each replacement.
    updates.sort_by(|a, b| {
        let a_start = a.range.map(|r| u32::from(r.start())).unwrap_or(0);
        let b_start = b.range.map(|r| u32::from(r.start())).unwrap_or(0);
        b_start.cmp(&a_start)
    });

    let mut current = source.to_string();

    for change in &updates {
        // Safety: filtered above.
        let range = change.range.unwrap();
        current = set_value(&current, range, &change.new_value)?;
    }

    // Detect the indentation style of the (possibly just-modified) source
    // and use it for all insertions in this file.
    let indent = detect_indent(&current);

    for change in &inserts {
        current = add_option(&current, &change.option_path, &change.new_value, &indent)?;
    }

    Ok(current)
}

// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use crate::nix_parser::NixValue;

    fn make_change(path: &str, value: NixValue) -> PendingChange {
        PendingChange {
            option_path: path.to_string(),
            file: PathBuf::from("/tmp/test.nix"),
            old_value: None,
            new_value: value,
            range: None,
            timestamp: Utc::now(),
        }
    }

    // add / remove / list / count / clear

    #[test]
    fn add_and_list() {
        let mut p = PendingChanges::new();
        p.add(make_change("a", NixValue::Bool(true)));
        p.add(make_change("b", NixValue::Int(42)));
        assert_eq!(p.count(), 2);
        let paths: Vec<&str> = p.list().iter().map(|c| c.option_path.as_str()).collect();
        assert!(paths.contains(&"a"));
        assert!(paths.contains(&"b"));
    }

    #[test]
    fn add_same_path_replaces() {
        let mut p = PendingChanges::new();
        p.add(make_change("enable", NixValue::Bool(false)));
        p.add(make_change("enable", NixValue::Bool(true)));
        assert_eq!(p.count(), 1);
        assert_eq!(p.list()[0].new_value, NixValue::Bool(true));
    }

    #[test]
    fn remove_existing() {
        let mut p = PendingChanges::new();
        p.add(make_change("a", NixValue::Bool(true)));
        p.add(make_change("b", NixValue::Bool(false)));
        p.remove("a");
        assert_eq!(p.count(), 1);
        assert_eq!(p.list()[0].option_path, "b");
    }

    #[test]
    fn remove_missing_is_no_op() {
        let mut p = PendingChanges::new();
        p.add(make_change("a", NixValue::Bool(true)));
        p.remove("nonexistent");
        assert_eq!(p.count(), 1);
    }

    #[test]
    fn clear_empties_buffer() {
        let mut p = PendingChanges::new();
        p.add(make_change("a", NixValue::Bool(true)));
        p.add(make_change("b", NixValue::Bool(false)));
        p.clear();
        assert_eq!(p.count(), 0);
        assert!(p.list().is_empty());
    }

    // apply_changes_to_source

    #[test]
    fn apply_insert_new_option() {
        let source = "{\n  enable = true;\n}";
        let change = make_change("port", NixValue::Int(8080));
        let result = apply_changes_to_source(source, &[&change]).unwrap();
        assert!(result.contains("port = 8080;"));
        assert!(result.contains("enable = true;"));
    }

    #[test]
    fn apply_multiple_inserts() {
        let source = "{\n}";
        let c1 = make_change("enable", NixValue::Bool(true));
        let c2 = make_change("port", NixValue::Int(80));
        let result = apply_changes_to_source(source, &[&c1, &c2]).unwrap();
        assert!(result.contains("enable = true;"));
        assert!(result.contains("port = 80;"));
    }
}
