//! Integration tests for multi-file package resolution.

use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Copy the fixture workspace to a temp dir for safe modification.
fn setup_multifile_workspace() -> TempDir {
    let dir = TempDir::new().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/multifile-workspace");

    copy_dir_recursive(&fixture, dir.path());
    dir
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            fs::create_dir_all(&dst_path).unwrap();
            copy_dir_recursive(&src_path, &dst_path);
        } else {
            fs::copy(&src_path, &dst_path).unwrap();
        }
    }
}

#[test]
fn add_without_file_selects_largest() {
    let dir = setup_multifile_workspace();
    let workspace = dir.path();

    let added = nixman_core::packages::manage::add(workspace, "btop", None).unwrap();
    assert!(added);

    // base.nix has 5 packages; extras.nix has 2 — auto-select picks base.nix.
    let base_content = fs::read_to_string(workspace.join("modules/base.nix")).unwrap();
    assert!(base_content.contains("btop"));

    let extras_content = fs::read_to_string(workspace.join("modules/extras.nix")).unwrap();
    assert!(!extras_content.contains("btop"));
}

#[test]
fn add_with_file_targets_specified() {
    let dir = setup_multifile_workspace();
    let workspace = dir.path();

    let target = workspace.join("modules/extras.nix");
    let added =
        nixman_core::packages::manage::add(workspace, "btop", Some(target.as_path())).unwrap();
    assert!(added);

    // Should land in extras.nix despite base.nix being larger.
    let extras_content = fs::read_to_string(workspace.join("modules/extras.nix")).unwrap();
    assert!(extras_content.contains("btop"));

    let base_content = fs::read_to_string(workspace.join("modules/base.nix")).unwrap();
    assert!(!base_content.contains("btop"));
}

#[test]
fn remove_finds_package_in_smaller_file() {
    let dir = setup_multifile_workspace();
    let workspace = dir.path();

    // firefox lives only in extras.nix
    let removed = nixman_core::packages::manage::remove(workspace, "firefox", None).unwrap();
    assert!(removed);

    let extras_content = fs::read_to_string(workspace.join("modules/extras.nix")).unwrap();
    assert!(!extras_content.contains("firefox"));
}

#[test]
fn remove_package_in_both_files_returns_ambiguous() {
    let dir = setup_multifile_workspace();
    let workspace = dir.path();

    let base_path = workspace.join("modules/base.nix");
    let extras_path = workspace.join("modules/extras.nix");
    nixman_core::packages::manage::add(workspace, "btop", Some(base_path.as_path())).unwrap();
    nixman_core::packages::manage::add(workspace, "btop", Some(extras_path.as_path())).unwrap();

    let result = nixman_core::packages::manage::remove(workspace, "btop", None);
    assert!(result.is_err());
    let err_msg = format!("{}", result.unwrap_err());
    assert!(err_msg.contains("multiple files"));
}

#[test]
fn list_installed_returns_from_all_files() {
    let dir = setup_multifile_workspace();
    let workspace = dir.path();

    let packages = nixman_core::packages::manage::list_installed(workspace).unwrap();

    // Packages from base.nix
    assert!(packages.iter().any(|p| p == "git"), "missing git");
    // Packages from extras.nix
    assert!(packages.iter().any(|p| p == "firefox"), "missing firefox");
}

#[test]
fn add_with_invalid_file_returns_error() {
    let dir = setup_multifile_workspace();
    let workspace = dir.path();

    let nonexistent = workspace.join("modules/nonexistent.nix");
    let result =
        nixman_core::packages::manage::add(workspace, "btop", Some(nonexistent.as_path()));
    assert!(result.is_err());
}
