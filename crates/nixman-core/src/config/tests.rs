//! Unit tests for the config pending-changes buffer and editor utilities.
//!
//! Tests cover adding, updating, discarding, and clearing pending changes as
//! well as the in-memory application of changes to source strings.

use std::path::PathBuf;

use chrono::Utc;

use crate::config::{
    pending::{apply_changes_to_source, PendingChanges},
    types::PendingChange,
};
use crate::nix_parser::NixValue;

// Helpers

/// Build a minimal `PendingChange` with no existing range (new insertion).
fn make_change(path: &str, value: NixValue) -> PendingChange {
    PendingChange {
        option_path: path.to_string(),
        file: PathBuf::from("/tmp/config.nix"),
        old_value: None,
        new_value: value,
        range: None,
        timestamp: Utc::now(),
    }
}

// Add and list

/// Adding a change and listing it back should reflect the buffered change.
#[test]
fn test_pending_add_and_list() {
    let mut pending = PendingChanges::new();

    pending.add(make_change("services.nginx.enable", NixValue::Bool(true)));
    pending.add(make_change("networking.hostname", NixValue::String("mybox".into())));

    assert_eq!(pending.count(), 2);

    let paths: Vec<&str> = pending.list().iter().map(|c| c.option_path.as_str()).collect();
    assert!(paths.contains(&"services.nginx.enable"));
    assert!(paths.contains(&"networking.hostname"));
}

/// A freshly created buffer has zero entries.
#[test]
fn test_pending_empty_on_creation() {
    let pending = PendingChanges::new();
    assert_eq!(pending.count(), 0);
    assert!(pending.list().is_empty());
}

// Discard (remove) a change

/// Discarding a specific change leaves the other changes intact.
#[test]
fn test_pending_discard() {
    let mut pending = PendingChanges::new();
    pending.add(make_change("services.nginx.enable", NixValue::Bool(true)));
    pending.add(make_change("networking.hostname", NixValue::String("x".into())));

    pending.remove("services.nginx.enable");

    assert_eq!(pending.count(), 1);
    assert_eq!(pending.list()[0].option_path, "networking.hostname");
}

/// Discarding a path that was never added is a no-op (no panic).
#[test]
fn test_pending_discard_nonexistent_is_noop() {
    let mut pending = PendingChanges::new();
    pending.add(make_change("a.b.c", NixValue::Bool(true)));

    pending.remove("x.y.z"); // not in buffer

    assert_eq!(pending.count(), 1, "count should be unchanged");
}

// Setting the same option twice replaces the earlier value

/// When the same option path is added twice, only the latest value is kept.
#[test]
fn test_pending_update_same_path() {
    let mut pending = PendingChanges::new();

    pending.add(make_change("services.nginx.enable", NixValue::Bool(false)));
    pending.add(make_change("services.nginx.enable", NixValue::Bool(true)));

    // Still one entry — not two.
    assert_eq!(pending.count(), 1);
    assert_eq!(pending.list()[0].new_value, NixValue::Bool(true));
}

/// Multiple consecutive updates to the same path keep only the last value.
#[test]
fn test_pending_update_same_path_multiple_times() {
    let mut pending = PendingChanges::new();

    for i in 0..5_i64 {
        pending.add(make_change("boot.loader.timeout", NixValue::Int(i)));
    }

    assert_eq!(pending.count(), 1);
    assert_eq!(pending.list()[0].new_value, NixValue::Int(4));
}

// Clear

/// `clear()` removes every buffered change.
#[test]
fn test_pending_clear() {
    let mut pending = PendingChanges::new();
    pending.add(make_change("a", NixValue::Bool(true)));
    pending.add(make_change("b", NixValue::Null));

    pending.clear();

    assert_eq!(pending.count(), 0);
    assert!(pending.list().is_empty());
}

// apply_changes_to_source

/// Inserting a new option that does not exist updates the source correctly.
#[test]
fn test_pending_apply_insert() {
    let source = "{\n  enable = true;\n}";
    let change = make_change("port", NixValue::Int(8080));

    let result = apply_changes_to_source(source, &[&change]).unwrap();

    assert!(result.contains("port = 8080;"));
    assert!(result.contains("enable = true;"));
}

/// Inserting two new options at once produces both in the output.
#[test]
fn test_pending_apply_multiple_inserts() {
    let source = "{\n}";
    let c1 = make_change("services.nginx.enable", NixValue::Bool(true));
    let c2 = make_change("networking.hostname", NixValue::String("box".into()));

    let result = apply_changes_to_source(source, &[&c1, &c2]).unwrap();

    assert!(result.contains("services.nginx.enable = true;"));
    assert!(result.contains(r#"networking.hostname = "box";"#));
}

/// Applying zero changes returns the source unchanged.
#[test]
fn test_pending_apply_no_changes() {
    let source = "{ enable = true; }";
    let result = apply_changes_to_source(source, &[]).unwrap();
    assert_eq!(result, source);
}
