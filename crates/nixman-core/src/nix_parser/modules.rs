//! Module import graph builder.
//!
//! Walks a NixOS configuration tree starting from an entry file, follows
//! every `imports = [ … ]` declaration recursively, and builds a
//! [`ModuleGraph`] that records all reachable `.nix` files together with
//! their direct dependencies.
//!
//! # Example
//!
//! ```rust,ignore
//! use std::path::Path;
//! use nixman_core::nix_parser::modules::build_graph;
//!
//! let graph = build_graph(Path::new("/etc/nixos/configuration.nix"))?;
//! for (file, imports) in &graph.modules {
//!     println!("{} imports {} file(s)", file.display(), imports.len());
//! }
//! ```

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::nix_parser::reader::parse_file;
use crate::nix_parser::traversal::find_option;
use crate::nix_parser::types::{ModuleGraph, NixValue, ResolveError};

// Public API

/// Build the [`ModuleGraph`] rooted at `entry`.
///
/// The function:
/// 1. Canonicalises `entry` (returns [`ResolveError::FileNotFound`] if the
///    file does not exist).
/// 2. Parses the file and extracts its `imports` list.
/// 3. Recursively processes every imported file.
/// 4. Detects and reports circular import chains via
///    [`ResolveError::CyclicImport`].
pub fn build_graph(entry: &Path) -> Result<ModuleGraph, ResolveError> {
    let entry = entry
        .canonicalize()
        .map_err(|_| ResolveError::FileNotFound(entry.to_path_buf()))?;

    let mut modules: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    // `visiting` tracks the call stack for cycle detection.
    let mut visiting: HashSet<PathBuf> = HashSet::new();

    process_file(&entry, &mut modules, &mut visiting)?;

    Ok(ModuleGraph {
        entry_file: entry,
        modules,
    })
}

/// Check that all files in the module graph are readable.
///
/// Returns an error containing the unreadable file path if any file can't
/// be opened.
pub fn check_graph_readable(entry: &Path) -> Result<(), ResolveError> {
    let graph = build_graph(entry)?;
    for file in graph.all_files() {
        if std::fs::File::open(file).is_err() {
            return Err(ResolveError::FileNotFound(file.clone()));
        }
    }
    Ok(())
}

// Internal helpers

/// Recursively parse `file` and its transitive imports, populating `modules`.
fn process_file(
    file: &Path,
    modules: &mut HashMap<PathBuf, Vec<PathBuf>>,
    visiting: &mut HashSet<PathBuf>,
) -> Result<(), ResolveError> {
    // Cycle detection: the file is on the current DFS stack.
    if visiting.contains(file) {
        return Err(ResolveError::CyclicImport(file.to_path_buf()));
    }

    // Already fully processed in a previous branch (diamond imports).
    if modules.contains_key(file) {
        return Ok(());
    }

    visiting.insert(file.to_path_buf());

    // Parse and extract imports.
    let imports = extract_imports(file)?;

    // Record the node before recursing so diamond imports are handled
    // correctly on the second visit.
    modules.insert(file.to_path_buf(), imports.clone());

    for import_path in &imports {
        process_file(import_path, modules, visiting)?;
    }

    visiting.remove(file);

    Ok(())
}

/// Parse `file` and return the list of canonicalized paths in its `imports`
/// attribute.
///
/// Returns an empty `Vec` when the file has no `imports` attribute or when
/// the attribute value cannot be statically resolved to a list of paths.
fn extract_imports(file: &Path) -> Result<Vec<PathBuf>, ResolveError> {
    let parent = file.parent().unwrap_or(Path::new("/"));

    let nix_file = parse_file(file).map_err(|e| {
        ResolveError::ParseError(file.to_path_buf(), e.to_string())
    })?;

    let imports_node = match find_option(&nix_file, "imports") {
        Some(node) => node,
        // No `imports` attribute — perfectly fine.
        None => return Ok(Vec::new()),
    };

    let list_items = match imports_node.to_nix_value() {
        NixValue::List(items) => items,
        // `imports` is set to a non-list expression (e.g. concatenation).
        // We cannot statically resolve it, so treat it as empty.
        _ => return Ok(Vec::new()),
    };

    let mut result = Vec::new();

    for item in list_items {
        match item {
            NixValue::Path(raw) => {
                match resolve_nix_path(raw.trim(), parent) {
                    Ok(canonical) => result.push(canonical),
                    // Skip paths that cannot be resolved on this machine
                    // (e.g. generated hardware-configuration that doesn't
                    // exist yet, or nix search paths like <nixpkgs/…>).
                    Err(_) => {}
                }
            }
            // Non-path items (function applications, attribute references …)
            // cannot be statically resolved.
            _ => {}
        }
    }

    Ok(result)
}

/// Resolve a Nix path string relative to `parent` and canonicalize it.
///
/// Handles:
/// - Relative paths starting with `./` or `../`
/// - Absolute paths starting with `/`
/// - Bare names without a leading `./` (treated as relative)
///
/// Nix search paths (`<nixpkgs/…>`) are skipped by returning an error.
fn resolve_nix_path(raw: &str, parent: &Path) -> Result<PathBuf, ResolveError> {
    // Nix angle-bracket search paths — not resolvable as local files.
    if raw.starts_with('<') {
        return Err(ResolveError::FileNotFound(PathBuf::from(raw)));
    }

    let joined = if raw.starts_with('/') {
        PathBuf::from(raw)
    } else {
        parent.join(raw)
    };

    joined
        .canonicalize()
        .map_err(|_| ResolveError::FileNotFound(joined))
}

// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Write a `.nix` file inside `dir`.
    fn write_nix(dir: &TempDir, name: &str, content: &str) -> PathBuf {
        let path = dir.path().join(name);
        fs::write(&path, content).expect("write_nix");
        path
    }

    #[test]
    fn entry_only_no_imports() {
        let dir = TempDir::new().unwrap();
        let entry = write_nix(&dir, "config.nix", "{ services.nginx.enable = true; }");

        let graph = build_graph(&entry).expect("should build graph");

        assert_eq!(graph.entry_file, entry.canonicalize().unwrap());
        assert_eq!(graph.modules.len(), 1);
        assert!(graph.modules[&graph.entry_file].is_empty());
    }

    #[test]
    fn follows_one_import() {
        let dir = TempDir::new().unwrap();
        let hw = write_nix(&dir, "hardware.nix", "{ }");
        let entry = write_nix(
            &dir,
            "config.nix",
            "{ imports = [ ./hardware.nix ]; }",
        );

        let graph = build_graph(&entry).expect("should build graph");

        assert_eq!(graph.modules.len(), 2);
        let entry_canon = entry.canonicalize().unwrap();
        let hw_canon = hw.canonicalize().unwrap();
        assert!(graph.modules[&entry_canon].contains(&hw_canon));
        assert!(graph.modules[&hw_canon].is_empty());
    }

    #[test]
    fn follows_nested_imports() {
        let dir = TempDir::new().unwrap();
        write_nix(&dir, "leaf.nix", "{ }");
        let mid = write_nix(&dir, "mid.nix", "{ imports = [ ./leaf.nix ]; }");
        let entry = write_nix(&dir, "entry.nix", "{ imports = [ ./mid.nix ]; }");

        let graph = build_graph(&entry).expect("should build graph");

        // entry → mid → leaf  ⇒ 3 nodes
        assert_eq!(graph.modules.len(), 3);
        let _ = mid; // mid is reachable via entry
    }

    #[test]
    fn diamond_imports_no_duplicate() {
        let dir = TempDir::new().unwrap();
        write_nix(&dir, "shared.nix", "{ }");
        write_nix(&dir, "left.nix", "{ imports = [ ./shared.nix ]; }");
        write_nix(&dir, "right.nix", "{ imports = [ ./shared.nix ]; }");
        let entry = write_nix(
            &dir,
            "entry.nix",
            "{ imports = [ ./left.nix ./right.nix ]; }",
        );

        let graph = build_graph(&entry).expect("should handle diamond");

        // entry, left, right, shared — no duplicates.
        assert_eq!(graph.modules.len(), 4);
    }

    #[test]
    fn detects_direct_cycle() {
        let dir = TempDir::new().unwrap();
        // Write placeholder first so the other file can reference it.
        let a_path = dir.path().join("a.nix");
        let b_path = dir.path().join("b.nix");
        fs::write(&a_path, "{ imports = [ ./b.nix ]; }").unwrap();
        fs::write(&b_path, "{ imports = [ ./a.nix ]; }").unwrap();

        let err = build_graph(&a_path).expect_err("should detect cycle");
        assert!(matches!(err, ResolveError::CyclicImport(_)));
    }

    #[test]
    fn entry_not_found_returns_error() {
        let path = PathBuf::from("/this/path/does/not/exist.nix");
        let err = build_graph(&path).expect_err("should fail");
        assert!(matches!(err, ResolveError::FileNotFound(_)));
    }

    #[test]
    fn skips_unresolvable_path_items() {
        let dir = TempDir::new().unwrap();
        // `<nixpkgs/nixos>` is a search path — should be silently skipped.
        let entry = write_nix(
            &dir,
            "config.nix",
            "{ imports = [ <nixpkgs/nixos> ]; }",
        );

        let graph = build_graph(&entry).expect("should build graph");
        assert_eq!(graph.modules.len(), 1);
        assert!(graph.modules[&graph.entry_file].is_empty());
    }

    #[test]
    fn lambda_form_imports() {
        let dir = TempDir::new().unwrap();
        write_nix(&dir, "hw.nix", "{ }");
        let entry = write_nix(
            &dir,
            "config.nix",
            "{ config, pkgs, ... }:\n{ imports = [ ./hw.nix ]; }",
        );

        let graph = build_graph(&entry).expect("lambda form should work");
        assert_eq!(graph.modules.len(), 2);
    }
}
