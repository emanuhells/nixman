//! High-level get / set / apply operations over NixOS configuration files.
//!
//! This module ties together the parser, resolver, writer, and validator to
//! provide a clean three-step editing workflow:
//!
//! 1. **Read** the current value with [`get_value`].
//! 2. **Queue** a change with [`set_value`] (nothing is written to disk yet).
//! 3. **Flush** all queued changes with [`apply_pending`], which validates
//!    every modified file before writing.

use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::nix_parser::{
    find_option,
    modules::build_graph,
    reader::parse_file,
    resolver::locate,
    NixValue,
};

use crate::config::{
    pending::{apply_changes_to_source, group_by_file, PendingChanges},
    types::{ConfigError, PendingChange, ValidationResult},
    validate,
};


/// Read the current value of `option_path` from the workspace configuration.
///
/// Builds the module graph rooted at `<workspace_path>/configuration.nix`,
/// searches every reachable file for `option_path`, parses the file that
/// contains it, and returns the value.
///
/// Returns `Ok(None)` when the option is not set in any file.
///
/// # Errors
///
/// * [`ConfigError::ResolveError`] — the module graph cannot be built (e.g.
///   entry file missing, cyclic imports, ambiguous definition).
/// * [`ConfigError::ParseError`] — the file containing the option has a
///   syntax error.
/// Return the HM entry file path for a workspace, preferring `home.nix`
/// over `flake.nix`.
fn hm_entry_path(workspace_path: &Path) -> PathBuf {
    let home_nix = workspace_path.join("home.nix");
    if home_nix.exists() {
        return home_nix;
    }
    workspace_path.join("flake.nix")
}

/// Read the current value of `option_path` from the workspace configuration.
pub fn get_value(
    workspace_path: &Path,
    option_path: &str,
) -> Result<Option<NixValue>, ConfigError> {
    get_value_internal(workspace_path, "configuration.nix", option_path)
}

/// Internal: read an option value using a custom entry file name.
fn get_value_internal(
    workspace_path: &Path,
    entry_file: &str,
    option_path: &str,
) -> Result<Option<NixValue>, ConfigError> {
    let entry = workspace_path.join(entry_file);
    let graph = build_graph(&entry)?;
    let resolved = locate(&graph, workspace_path, option_path)?;

    if !resolved.exists {
        return Ok(None);
    }

    let nix_file = parse_file(&resolved.file)
        .map_err(|e| ConfigError::ParseError(e.to_string()))?;

    let node = find_option(&nix_file, option_path).ok_or_else(|| {
        ConfigError::ParseError(format!(
            "option '{}' unexpectedly missing after locate",
            option_path
        ))
    })?;

    Ok(Some(node.to_nix_value()))
}

/// Read the current value of `option_path` from the Home Manager configuration.
pub fn hm_get_value(
    workspace_path: &Path,
    option_path: &str,
) -> Result<Option<NixValue>, ConfigError> {
    let entry = hm_entry_path(workspace_path);
    let file_name = entry.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("home.nix");
    get_value_internal(workspace_path, file_name, option_path)
}


/// Queue a change that sets `option_path` to `value`.
///
/// Resolves the option location, captures the old value (if the option
/// already exists), and records a [`PendingChange`] in `pending`.
///
/// **No file is written.**  Call [`apply_pending`] to flush the buffer.
///
/// If a change for the same path is already buffered, it is replaced.
///
/// # Errors
///
/// * [`ConfigError::ResolveError`] — the module graph cannot be built.
/// * [`ConfigError::ParseError`] — the file cannot be parsed while reading
///   the old value.
pub fn set_value(
    pending: &mut PendingChanges,
    workspace_path: &Path,
    option_path: &str,
    value: NixValue,
) -> Result<(), ConfigError> {
    set_value_internal(pending, workspace_path, "configuration.nix", option_path, value)
}

/// Internal: queue a set change using a custom entry file name.
fn set_value_internal(
    pending: &mut PendingChanges,
    workspace_path: &Path,
    entry_file: &str,
    option_path: &str,
    value: NixValue,
) -> Result<(), ConfigError> {
    let entry = workspace_path.join(entry_file);
    let graph = build_graph(&entry)?;
    let resolved = locate(&graph, workspace_path, option_path)?;

    let old_value = if resolved.exists {
        if let Ok(nix_file) = parse_file(&resolved.file) {
            find_option(&nix_file, option_path).map(|n| n.to_nix_value())
        } else {
            None
        }
    } else {
        None
    };

    let change = PendingChange {
        option_path: option_path.to_string(),
        file: resolved.file,
        old_value,
        new_value: value,
        range: resolved.range,
        timestamp: Utc::now(),
    };

    pending.add(change);
    Ok(())
}

/// Queue an HM option set change.  Uses `home.nix` as the entry point.
pub fn hm_set_value(
    pending: &mut PendingChanges,
    workspace_path: &Path,
    option_path: &str,
    value: NixValue,
) -> Result<(), ConfigError> {
    let entry = hm_entry_path(workspace_path);
    let file_name = entry.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("home.nix");
    set_value_internal(pending, workspace_path, file_name, option_path, value)
}


/// Remove the option at `option_path` from the workspace configuration.
///
/// Builds the module graph, locates the option, removes it from the source
/// file, validates the result, then writes back to disk.
///
/// # Errors
///
/// * [`ConfigError::ParseError`] — the option is not set in the configuration.
/// * [`ConfigError::ResolveError`] — the module graph cannot be built.
/// * [`ConfigError::WriteError`] — the AST writer could not remove the option.
/// * [`ConfigError::ValidationFailed`] — the result fails validation.
/// * [`ConfigError::IoError`] — reading or writing the file failed.
pub fn remove_value(
    workspace_path: &Path,
    option_path: &str,
) -> Result<(), ConfigError> {
    remove_value_internal(workspace_path, "configuration.nix", option_path)
}

/// Internal: remove an option using a custom entry file name.
fn remove_value_internal(
    workspace_path: &Path,
    entry_file: &str,
    option_path: &str,
) -> Result<(), ConfigError> {
    // 1. Build module graph from entry file
    let entry = workspace_path.join(entry_file);
    let graph = build_graph(&entry)?;
    let resolved = locate(&graph, workspace_path, option_path)?;

    // 2. If option doesn't exist, return an error
    if !resolved.exists {
        return Err(ConfigError::ParseError(format!(
            "option '{}' is not set in the configuration",
            option_path
        )));
    }

    // 3. Read the source file
    let source = std::fs::read_to_string(&resolved.file)?;

    // 4. Call the writer to produce modified source
    use crate::nix_parser::writer::remove_option;
    let modified = remove_option(&source, option_path)?;

    // 5. Validate (re-use existing validate module)
    use crate::config::validate;
    match validate::check(&resolved.file, &modified)? {
        crate::config::types::ValidationResult::Valid => {}
        crate::config::types::ValidationResult::Invalid { errors } => {
            return Err(ConfigError::ValidationFailed(errors));
        }
    }

    // 6. Write to disk, preserving permissions
    let metadata = std::fs::metadata(&resolved.file)?;
    let permissions = metadata.permissions();
    std::fs::write(&resolved.file, &modified)?;
    std::fs::set_permissions(&resolved.file, permissions)?;

    Ok(())
}

/// Remove an HM option from the configuration.  Uses `home.nix` as the entry point.
pub fn hm_remove_value(
    workspace_path: &Path,
    option_path: &str,
) -> Result<(), ConfigError> {
    let entry = hm_entry_path(workspace_path);
    let file_name = entry.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("home.nix");
    remove_value_internal(workspace_path, file_name, option_path)
}


/// Apply all pending changes: validate every modified file, then write to disk.
///
/// # Process
///
/// 1. For each file that has pending changes, apply them in memory to
///    produce a modified source string.
/// 2. Validate every modified source with `nix-instantiate --parse`.
/// 3. **Only if all files pass validation**: write every modified file to
///    disk and clear the pending buffer.
///
/// If any file fails validation, **no** file is written and the buffer is
/// left intact so the caller can inspect or correct the queued changes.
///
/// Returns immediately with `Ok(())` when the buffer is empty.
///
/// # Errors
///
/// * [`ConfigError::ValidationFailed`] — at least one file has a syntax
///   error after changes are applied.  No files have been written.
/// * [`ConfigError::WriteError`] — an AST write operation failed.
/// * [`ConfigError::IoError`] — reading a source file or writing to disk
///   failed.
pub fn apply_pending(
    pending: &mut PendingChanges,
    _workspace_path: &Path,
) -> Result<(), ConfigError> {
    if pending.count() == 0 {
        return Ok(());
    }

    // before calling `pending.clear()`.
    let changes: Vec<PendingChange> = pending.list().to_vec();
    let by_file = group_by_file(&changes);

    let mut validated: Vec<(std::path::PathBuf, String)> = Vec::new();

    for (file, file_changes) in &by_file {
        let original = std::fs::read_to_string(file)?;
        let modified = apply_changes_to_source(&original, file_changes)?;

        match validate::check(file, &modified)? {
            ValidationResult::Valid => {
                validated.push((file.clone(), modified));
            }
            ValidationResult::Invalid { errors } => {
                return Err(ConfigError::ValidationFailed(errors));
            }
        }
    }

    for (file, modified) in validated {
        let metadata = std::fs::metadata(&file)?;
        let permissions = metadata.permissions();
        std::fs::write(&file, modified)?;
        std::fs::set_permissions(&file, permissions)?;
    }

    pending.clear();
    Ok(())
}

/// Apply pending HM changes.  Delegates to [`apply_pending`] since the
/// changes already reference their target files by path.
pub fn hm_apply_pending(
    pending: &mut PendingChanges,
    workspace_path: &Path,
) -> Result<(), ConfigError> {
    apply_pending(pending, workspace_path)
}




#[cfg(test)]
mod tests {
    use super::*;
    use crate::nix_parser::{NixValue, ResolveError};
    use tempfile::TempDir;

    fn create_config(dir: &TempDir, content: &str) {
        std::fs::write(dir.path().join("configuration.nix"), content).unwrap();
    }

    fn create_home(dir: &TempDir, content: &str) {
        std::fs::write(dir.path().join("home.nix"), content).unwrap();
    }

    fn read_file(dir: &TempDir, name: &str) -> String {
        std::fs::read_to_string(dir.path().join(name)).unwrap()
    }

    fn nixos_module(body: &str) -> String {
        format!("{{ config, pkgs, ... }}: {{\n  {}\n}}", body)
    }

    // -- 1. Reading values --------------------------------------------------

    #[test]
    fn get_value_simple() {
        let dir = TempDir::new().unwrap();
        create_config(&dir, &nixos_module("services.nginx.enable = true;"));
        let result = get_value(dir.path(), "services.nginx.enable").unwrap();
        assert_eq!(result, Some(NixValue::Bool(true)));
    }

    #[test]
    fn get_value_nested() {
        let dir = TempDir::new().unwrap();
        create_config(&dir, &nixos_module("users.users.test.isNormalUser = true;"));
        let result = get_value(dir.path(), "users.users.test.isNormalUser").unwrap();
        assert_eq!(result, Some(NixValue::Bool(true)));
    }

    #[test]
    fn get_value_missing() {
        let dir = TempDir::new().unwrap();
        create_config(&dir, &nixos_module("services.nginx.enable = true;"));
        let result = get_value(dir.path(), "services.nonexistent").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn get_value_list() {
        let dir = TempDir::new().unwrap();
        create_config(&dir, &nixos_module("environment.systemPackages = [ vim git ];"));
        let result = get_value(dir.path(), "environment.systemPackages").unwrap();
        match result {
            Some(NixValue::List(items)) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0], NixValue::Expression("vim".to_string()));
                assert_eq!(items[1], NixValue::Expression("git".to_string()));
            }
            other => panic!("expected List, got {:?}", other),
        }
    }

    // -- 2. Setting values --------------------------------------------------

    #[test]
    fn set_value_new_option() {
        let dir = TempDir::new().unwrap();
        create_config(&dir, &nixos_module("services.nginx.enable = true;"));
        let mut pending = PendingChanges::new();
        set_value(&mut pending, dir.path(), "services.nginx.port", NixValue::Int(8080)).unwrap();
        assert_eq!(pending.count(), 1);
        let change = &pending.list()[0];
        assert_eq!(change.option_path, "services.nginx.port");
        assert_eq!(change.new_value, NixValue::Int(8080));
        assert!(change.old_value.is_none());
        assert!(change.range.is_none());
        assert!(change.file.ends_with("configuration.nix"));
    }

    #[test]
    fn set_value_existing_option() {
        let dir = TempDir::new().unwrap();
        create_config(&dir, &nixos_module("services.nginx.enable = true;"));
        let mut pending = PendingChanges::new();
        set_value(&mut pending, dir.path(), "services.nginx.enable", NixValue::Bool(false)).unwrap();
        assert_eq!(pending.count(), 1);
        let change = &pending.list()[0];
        assert_eq!(change.option_path, "services.nginx.enable");
        assert_eq!(change.new_value, NixValue::Bool(false));
        assert_eq!(change.old_value, Some(NixValue::Bool(true)));
        assert!(change.range.is_some());
    }

    #[test]
    fn set_value_replaces_previous() {
        let dir = TempDir::new().unwrap();
        create_config(&dir, &nixos_module("services.nginx.enable = true;"));
        let mut pending = PendingChanges::new();
        set_value(&mut pending, dir.path(), "services.nginx.enable", NixValue::Bool(false)).unwrap();
        set_value(&mut pending, dir.path(), "services.nginx.enable", NixValue::Bool(true)).unwrap();
        assert_eq!(pending.count(), 1);
        assert_eq!(pending.list()[0].new_value, NixValue::Bool(true));
    }

    // -- 3. Removing values ------------------------------------------------

    #[test]
    fn remove_value_existing() {
        let dir = TempDir::new().unwrap();
        let src = concat!(
            "{ config, pkgs, ... }:\n",
            "{\n",
            "  services.nginx.enable = true;\n",
            "  services.nginx.port = 8080;\n",
            "}\n",
        );
        create_config(&dir, src);
        remove_value(dir.path(), "services.nginx.enable").unwrap();
        let content = read_file(&dir, "configuration.nix");
        assert!(!content.contains("services.nginx.enable"), "enable removed");
        assert!(content.contains("services.nginx.port"), "port remains");
    }

    #[test]
    fn remove_value_nonexistent_returns_error() {
        let dir = TempDir::new().unwrap();
        create_config(&dir, &nixos_module("services.nginx.enable = true;"));
        let result = remove_value(dir.path(), "services.nonexistent");
        assert!(result.is_err());
        match result {
            Err(ConfigError::ParseError(msg)) => assert!(msg.contains("not set"), "{}", msg),
            other => panic!("expected ParseError, got {:?}", other),
        }
    }

    // -- 4. Applying pending -----------------------------------------------

    #[test]
    fn apply_pending_empty_is_noop() {
        let mut pending = PendingChanges::new();
        let dir = TempDir::new().unwrap();
        assert!(apply_pending(&mut pending, dir.path()).is_ok());
    }

    #[test]
    fn apply_pending_insert_new_option() {
        let dir = TempDir::new().unwrap();
        create_config(&dir, &nixos_module("services.nginx.enable = true;"));
        let mut pending = PendingChanges::new();
        set_value(&mut pending, dir.path(), "services.nginx.port", NixValue::Int(8080)).unwrap();
        apply_pending(&mut pending, dir.path()).unwrap();
        assert_eq!(pending.count(), 0);
        let content = read_file(&dir, "configuration.nix");
        assert!(content.contains("port = 8080"), "port added");
        assert!(content.contains("enable = true"), "enable remains");
    }

    #[test]
    fn apply_pending_replace_existing() {
        let dir = TempDir::new().unwrap();
        create_config(&dir, &nixos_module("services.nginx.enable = true;"));
        let mut pending = PendingChanges::new();
        set_value(&mut pending, dir.path(), "services.nginx.enable", NixValue::Bool(false)).unwrap();
        apply_pending(&mut pending, dir.path()).unwrap();
        assert_eq!(pending.count(), 0);
        let content = read_file(&dir, "configuration.nix");
        assert!(content.contains("= false;") || content.contains("= false\n"));
    }

    // -- 5. Error cases ----------------------------------------------------

    #[test]
    fn get_value_no_entry_file() {
        let dir = TempDir::new().unwrap();
        let result = get_value(dir.path(), "services.nginx.enable");
        assert!(result.is_err());
        assert!(matches!(result, Err(ConfigError::ResolveError(ResolveError::FileNotFound(_)))));
    }

    #[test]
    fn set_value_no_entry_file() {
        let dir = TempDir::new().unwrap();
        let mut pending = PendingChanges::new();
        let result = set_value(&mut pending, dir.path(), "services.nginx.enable", NixValue::Bool(true));
        assert!(result.is_err());
        assert!(matches!(result, Err(ConfigError::ResolveError(ResolveError::FileNotFound(_)))));
    }

    #[test]
    fn remove_value_no_entry_file() {
        let dir = TempDir::new().unwrap();
        let result = remove_value(dir.path(), "services.nginx.enable");
        assert!(result.is_err());
        assert!(matches!(result, Err(ConfigError::ResolveError(ResolveError::FileNotFound(_)))));
    }

    // -- 6. HM-specific functions ------------------------------------------

    #[test]
    fn hm_get_value_reads_home_nix() {
        let dir = TempDir::new().unwrap();
        create_home(&dir, &nixos_module(r#"home.username = "testuser";"#));
        let result = hm_get_value(dir.path(), "home.username").unwrap();
        assert_eq!(result, Some(NixValue::String("testuser".to_string())));
    }

    #[test]
    fn hm_get_value_falls_back_to_flake_nix() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("flake.nix"), &nixos_module(r#"home.username = "from-flake";"#)).unwrap();
        let result = hm_get_value(dir.path(), "home.username").unwrap();
        assert_eq!(result, Some(NixValue::String("from-flake".to_string())));
    }

    #[test]
    fn hm_set_value_queues_change_on_home_nix() {
        let dir = TempDir::new().unwrap();
        create_home(&dir, &nixos_module("home.enable = true;"));
        let mut pending = PendingChanges::new();
        hm_set_value(&mut pending, dir.path(), "home.packages", NixValue::List(vec![])).unwrap();
        assert_eq!(pending.count(), 1);
        assert!(pending.list()[0].file.ends_with("home.nix"));
    }

    #[test]
    fn hm_remove_value_removes_from_home_nix() {
        let dir = TempDir::new().unwrap();
        let src = concat!(
            "{ config, pkgs, ... }:\n",
            "{\n",
            "  home.username = \"test\";\n",
            "  home.enable = true;\n",
            "}\n",
        );
        create_home(&dir, src);
        hm_remove_value(dir.path(), "home.username").unwrap();
        let content = read_file(&dir, "home.nix");
        assert!(!content.contains("home.username"), "home.username removed");
        assert!(content.contains("home.enable"), "home.enable remains");
    }

    #[test]
    fn hm_apply_pending_applies_to_home_nix() {
        let dir = TempDir::new().unwrap();
        create_home(&dir, &nixos_module(r#"home.username = "old";"#));
        let mut pending = PendingChanges::new();
        hm_set_value(&mut pending, dir.path(), "home.username", NixValue::String("new".into())).unwrap();
        hm_apply_pending(&mut pending, dir.path()).unwrap();
        assert_eq!(pending.count(), 0);
        let content = read_file(&dir, "home.nix");
        assert!(content.contains(r#""new""#), "username updated to new");
    }
}
