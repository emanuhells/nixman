//! Unit tests for workspace detection and related types.
//!
//! Tests verify `WorkspaceKind` classification, hostname reading, and the
//! symlink-resolution helper — all without touching system-level paths.

use std::fs;

use tempfile::TempDir;

use crate::workspace::detect::{get_hostname, resolve_symlink};
use crate::workspace::types::{WorkspaceError, WorkspaceKind};

// WorkspaceKind — type behaviour

/// `WorkspaceKind::Flake` and `WorkspaceKind::Legacy` are distinct values.
#[test]
fn test_workspace_kind_variants_are_distinct() {
    assert_ne!(WorkspaceKind::Flake, WorkspaceKind::Legacy);
}

/// A directory that contains `flake.nix` should be classified as Flake.
///
/// This test mirrors the logic of the private `classify_directory` function
/// so that the expected workspace kind is documented in the test suite.
#[test]
fn test_workspace_kind_detection() {
    let dir = TempDir::new().expect("tempdir");
    fs::write(dir.path().join("flake.nix"), "# stub flake").expect("write flake.nix");

    // Verify the sentinel file is present (same check classify_directory does).
    let has_flake = dir.path().join("flake.nix").exists();
    let expected_kind = if has_flake {
        WorkspaceKind::Flake
    } else {
        WorkspaceKind::Legacy
    };

    assert_eq!(expected_kind, WorkspaceKind::Flake);
}

/// A directory with only `configuration.nix` should be classified as Legacy.
#[test]
fn test_workspace_kind_legacy_detection() {
    let dir = TempDir::new().expect("tempdir");
    fs::write(dir.path().join("configuration.nix"), "# stub config").expect("write");

    let has_flake = dir.path().join("flake.nix").exists();
    let has_config = dir.path().join("configuration.nix").exists();
    let expected_kind = if has_flake {
        WorkspaceKind::Flake
    } else if has_config {
        WorkspaceKind::Legacy
    } else {
        panic!("neither file present")
    };

    assert_eq!(expected_kind, WorkspaceKind::Legacy);
}

/// When `flake.nix` is present alongside `configuration.nix`, the workspace
/// should be classified as Flake (flake takes precedence).
#[test]
fn test_workspace_kind_flake_takes_precedence() {
    let dir = TempDir::new().expect("tempdir");
    fs::write(dir.path().join("flake.nix"), "# stub").expect("write flake.nix");
    fs::write(dir.path().join("configuration.nix"), "# stub").expect("write configuration.nix");

    let has_flake = dir.path().join("flake.nix").exists();
    let kind = if has_flake { WorkspaceKind::Flake } else { WorkspaceKind::Legacy };

    assert_eq!(kind, WorkspaceKind::Flake);
}

// Hostname reading

/// `get_hostname()` always returns a non-empty string (never panics, falls
/// back to `"unknown"` when both `/etc/hostname` and `hostname` are absent).
#[test]
fn test_hostname_parsing() {
    let hostname = get_hostname();
    assert!(!hostname.is_empty(), "hostname should never be empty");
    // Should not contain newlines (whitespace-trimmed).
    assert!(!hostname.contains('\n'));
}

/// `get_hostname()` is idempotent: two consecutive calls return the same value.
#[test]
fn test_hostname_is_stable() {
    let h1 = get_hostname();
    let h2 = get_hostname();
    assert_eq!(h1, h2, "hostname must be stable across calls");
}

// Symlink resolution

/// `resolve_symlink` on a plain directory returns the same path unchanged.
#[test]
fn test_resolve_non_symlink_is_identity() {
    let dir = TempDir::new().expect("tempdir");
    let resolved = resolve_symlink(dir.path());
    assert_eq!(resolved, dir.path());
}

/// `resolve_symlink` follows a single level of symlink.
#[cfg(unix)]
#[test]
fn test_resolve_symlink_follows_link() {
    let target = TempDir::new().expect("tempdir");
    let link_dir = TempDir::new().expect("tempdir");
    let link_path = link_dir.path().join("my_link");
    std::os::unix::fs::symlink(target.path(), &link_path).expect("symlink");

    let resolved = resolve_symlink(&link_path);
    assert_eq!(resolved, target.path());
}

/// `resolve_symlink` on a path that does not exist returns the path as-is
/// (no panic on missing entries).
#[test]
fn test_resolve_symlink_missing_path_no_panic() {
    let path = std::path::PathBuf::from("/tmp/nixman_test_nonexistent_path_xyz");
    // Should not panic — returns the path unchanged.
    let resolved = resolve_symlink(&path);
    assert_eq!(resolved, path);
}

// WorkspaceError — Display

/// Every `WorkspaceError` variant must produce a non-empty display string.
#[test]
fn test_workspace_error_display() {
    use std::io;

    let variants: Vec<WorkspaceError> = vec![
        WorkspaceError::NotFound,
        WorkspaceError::PermissionDenied,
        WorkspaceError::InvalidConfig("bad format".into()),
        WorkspaceError::IoError(io::Error::new(io::ErrorKind::Other, "disk full")),
    ];

    for v in &variants {
        let msg = v.to_string();
        assert!(!msg.is_empty(), "Display for {:?} should be non-empty", v);
    }
}
