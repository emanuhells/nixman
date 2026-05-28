//! Option path resolver.
//!
//! Given a [`ModuleGraph`] and a dotted option path such as
//! `"services.nginx.enable"`, the resolver searches every file in the graph
//! and returns the file (and byte range) where the option is defined.
//!
//! # Example
//!
//! ```rust,ignore
//! use std::path::Path;
//! use nixman_core::nix_parser::{
//!     modules::build_graph,
//!     resolver::{locate, suggest_file},
//! };
//!
//! let graph = build_graph(Path::new("/etc/nixos/configuration.nix"))?;
//!
//! // Find where `services.nginx.enable` is set.
//! let result = locate(&graph, Path::new("/etc/nixos"), "services.nginx.enable")?;
//! if result.exists {
//!     println!("Found in {} at {:?}", result.file.display(), result.range);
//! } else {
//!     println!("Not set — suggested file: {}", result.file.display());
//! }
//! ```

use std::path::{Path, PathBuf};

use rnix::ast;
use rowan::ast::AstNode;

use crate::nix_parser::reader::parse_file;
use crate::nix_parser::traversal::find_option;
use crate::nix_parser::types::{ModuleGraph, ResolveError, ResolvedCandidate, ResolvedOption};

// Public API

/// Search all files in `graph` for `option_path` and return the file and
/// byte-range where the option is set.
///
/// # Behaviour
///
/// | Situation                           | Return value                             |
/// |-------------------------------------|------------------------------------------|
/// | Option found in exactly one file    | `Ok(ResolvedOption { exists: true, … })` |
/// | Option found in multiple files      | `Err(ResolveError::Ambiguous(…))`        |
/// | Option not found in any file        | `Ok(ResolvedOption { exists: false, … })` with the best suggested file |
///
/// `workspace_path` is accepted for API completeness and used as a fallback
/// base when resolving the suggested file; since [`super::modules::build_graph`]
/// canonicalises all paths, it is typically not needed.
pub fn locate(
    graph: &ModuleGraph,
    _workspace_path: &Path,
    option_path: &str,
) -> Result<ResolvedOption, ResolveError> {
    // Collect every (file, range) where the option is found.
    let mut found: Vec<(PathBuf, rnix::TextRange)> = Vec::new();

    for file in graph.all_files() {
        let nix_file = parse_file(file)
            .map_err(|e| ResolveError::ParseError(file.clone(), e.to_string()))?;

        if let Some(node) = find_option(&nix_file, option_path) {
            let range = node.text_range();
            found.push((file.clone(), range));
        }
    }

    match found.len() {
        0 => {
            // Not found — suggest the best insertion target.
            let file = suggest_file(graph, option_path);
            Ok(ResolvedOption {
                file,
                exists: false,
                range: None,
            })
        }
        1 => {
            let (file, range) = found.remove(0);
            Ok(ResolvedOption {
                file,
                exists: true,
                range: Some(range),
            })
        }
        _ => {
            let paths: Vec<PathBuf> = found.into_iter().map(|(p, _)| p).collect();
            Err(ResolveError::Ambiguous(paths))
        }
    }
}

/// Heuristically choose the best file in `graph` to insert `option_path`.
///
/// Strategy (in priority order):
///
/// 1. **Prefix-depth match** — parse each file and check how deep a prefix
///    of `option_path` already exists.  The file with the deepest matching
///    prefix is preferred.
/// 2. **Name heuristics** — if the option starts with `services.`, look for
///    a module whose filename contains "services"; similarly for
///    `networking.`.
/// 3. **Fallback** — the graph's entry file.
pub fn suggest_file(graph: &ModuleGraph, option_path: &str) -> PathBuf {
    let parts: Vec<&str> = option_path.split('.').collect();

    // --- Pass 1: deepest prefix match across all files -------------------
    let mut best_file: Option<PathBuf> = None;
    let mut best_depth: usize = 0;

    for file in graph.all_files() {
        if let Ok(nix_file) = parse_file(file) {
            // Walk from the longest prefix down to 1.
            for depth in (1..parts.len()).rev() {
                let prefix = parts[..depth].join(".");
                if find_option(&nix_file, &prefix).is_some() {
                    if depth > best_depth {
                        best_depth = depth;
                        best_file = Some(file.clone());
                    }
                    break; // No need to check shorter prefixes for this file.
                }
            }
        }
    }

    if let Some(file) = best_file {
        return file;
    }

    // --- Pass 2: filename heuristics -------------------------------------
    let lower = option_path.to_lowercase();

    if lower.starts_with("services.") {
        if let Some(f) = find_module_by_stem(graph, "services") {
            return f;
        }
    } else if lower.starts_with("networking.") {
        if let Some(f) = find_module_by_stem(graph, "networking") {
            return f;
        }
    }

    // --- Fallback: entry file --------------------------------------------
    graph.entry_file.clone()
}

/// Search all files in `graph` for `option_path` and return all candidates
/// where the option is defined, letting the caller decide which to use.
///
/// Returns an empty `Vec` when the option is not found in any file.
pub fn locate_all(
    graph: &ModuleGraph,
    option_path: &str,
) -> Result<Vec<ResolvedCandidate>, ResolveError> {
    let mut candidates: Vec<ResolvedCandidate> = Vec::new();

    for file in graph.all_files() {
        let nix_file = parse_file(file)
            .map_err(|e| ResolveError::ParseError(file.clone(), e.to_string()))?;

        if let Some(node) = find_option(&nix_file, option_path) {
            let range = node.text_range();
            let item_count = count_list_items(&node);
            candidates.push(ResolvedCandidate { file: file.clone(), range, item_count });
        }
    }

    Ok(candidates)
}

// Internal helpers

/// Count the items in a list value node.
///
/// Handles bare lists `[ a b ]` and with-expressions `with pkgs; [ a b ]`.
/// Returns zero for non-list nodes.
fn count_list_items(node: &crate::nix_parser::types::ParsedNode) -> usize {
    if let Some(list) = ast::List::cast(node.syntax.clone()) {
        return list.items().count();
    }
    if let Some(with_expr) = ast::With::cast(node.syntax.clone()) {
        if let Some(ast::Expr::List(list)) = with_expr.body() {
            return list.items().count();
        }
    }
    0
}

// Internal helpers (file suggestion)

/// Return the first file in `graph` whose stem contains `name` (case-insensitive).
fn find_module_by_stem(graph: &ModuleGraph, name: &str) -> Option<PathBuf> {
    for file in graph.all_files() {
        if let Some(stem) = file.file_stem() {
            if stem.to_string_lossy().to_lowercase().contains(name) {
                return Some(file.clone());
            }
        }
    }
    None
}

// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nix_parser::modules::build_graph;
    use std::fs;
    use tempfile::TempDir;

    fn write_nix(dir: &TempDir, name: &str, content: &str) -> PathBuf {
        let path = dir.path().join(name);
        fs::write(&path, content).expect("write_nix");
        path
    }

    // locate — option found

    #[test]
    fn locate_finds_option_in_entry() {
        let dir = TempDir::new().unwrap();
        let entry = write_nix(
            &dir,
            "config.nix",
            "{ services.nginx.enable = true; }",
        );
        let graph = build_graph(&entry).unwrap();

        let result = locate(&graph, dir.path(), "services.nginx.enable").unwrap();

        assert!(result.exists);
        assert_eq!(result.file, entry.canonicalize().unwrap());
        assert!(result.range.is_some());
    }

    #[test]
    fn locate_returns_correct_range() {
        let dir = TempDir::new().unwrap();
        let src = "{ services.nginx.enable = true; }";
        let entry = write_nix(&dir, "config.nix", src);
        let graph = build_graph(&entry).unwrap();

        let result = locate(&graph, dir.path(), "services.nginx.enable").unwrap();

        let range = result.range.expect("range should be present");
        // The value "true" lives somewhere after the `=`.
        let start = u32::from(range.start()) as usize;
        let end = u32::from(range.end()) as usize;
        assert_eq!(&src[start..end], "true");
    }

    #[test]
    fn locate_finds_option_in_imported_file() {
        let dir = TempDir::new().unwrap();
        let services = write_nix(
            &dir,
            "services.nix",
            "{ services.nginx.enable = true; }",
        );
        let entry = write_nix(
            &dir,
            "config.nix",
            "{ imports = [ ./services.nix ]; }",
        );
        let graph = build_graph(&entry).unwrap();

        let result = locate(&graph, dir.path(), "services.nginx.enable").unwrap();

        assert!(result.exists);
        assert_eq!(result.file, services.canonicalize().unwrap());
    }

    // locate — option not found

    #[test]
    fn locate_not_found_returns_exists_false() {
        let dir = TempDir::new().unwrap();
        let entry = write_nix(&dir, "config.nix", "{ }");
        let graph = build_graph(&entry).unwrap();

        let result = locate(&graph, dir.path(), "services.nginx.enable").unwrap();

        assert!(!result.exists);
        assert!(result.range.is_none());
    }

    // locate — ambiguous

    #[test]
    fn locate_returns_ambiguous_when_set_in_multiple_files() {
        let dir = TempDir::new().unwrap();
        write_nix(&dir, "a.nix", "{ services.nginx.enable = true; }");
        let entry = write_nix(
            &dir,
            "config.nix",
            "{ imports = [ ./a.nix ]; services.nginx.enable = false; }",
        );
        let graph = build_graph(&entry).unwrap();

        let err = locate(&graph, dir.path(), "services.nginx.enable")
            .expect_err("should be ambiguous");

        assert!(matches!(err, ResolveError::Ambiguous(paths) if paths.len() == 2));
    }

    // suggest_file

    #[test]
    fn suggest_falls_back_to_entry_when_no_match() {
        let dir = TempDir::new().unwrap();
        let entry = write_nix(&dir, "config.nix", "{ }");
        let graph = build_graph(&entry).unwrap();

        let suggested = suggest_file(&graph, "some.unknown.option");
        assert_eq!(suggested, entry.canonicalize().unwrap());
    }

    #[test]
    fn suggest_uses_services_module_by_name() {
        let dir = TempDir::new().unwrap();
        let services = write_nix(&dir, "services.nix", "{ }");
        let entry = write_nix(
            &dir,
            "config.nix",
            "{ imports = [ ./services.nix ]; }",
        );
        let graph = build_graph(&entry).unwrap();

        let suggested = suggest_file(&graph, "services.nginx.enable");
        assert_eq!(suggested, services.canonicalize().unwrap());
    }

    #[test]
    fn suggest_uses_networking_module_by_name() {
        let dir = TempDir::new().unwrap();
        let net = write_nix(&dir, "networking.nix", "{ }");
        let entry = write_nix(
            &dir,
            "config.nix",
            "{ imports = [ ./networking.nix ]; }",
        );
        let graph = build_graph(&entry).unwrap();

        let suggested = suggest_file(&graph, "networking.hostName");
        assert_eq!(suggested, net.canonicalize().unwrap());
    }

    #[test]
    fn suggest_prefers_deeper_prefix_match() {
        let dir = TempDir::new().unwrap();
        // services.nix has the `services` attr set — deeper prefix.
        write_nix(&dir, "services.nix", "{ services = {}; }");
        write_nix(&dir, "other.nix", "{ }");
        let entry = write_nix(
            &dir,
            "config.nix",
            "{ imports = [ ./services.nix ./other.nix ]; }",
        );
        let graph = build_graph(&entry).unwrap();

        // The services.nix file has `services` as a prefix match.
        let suggested = suggest_file(&graph, "services.nginx.enable");
        let services_canon = dir.path().join("services.nix").canonicalize().unwrap();
        assert_eq!(suggested, services_canon);
    }

    // locate_all

    #[test]
    fn locate_all_returns_multiple_candidates() {
        let dir = TempDir::new().unwrap();
        write_nix(
            &dir,
            "a.nix",
            "{ environment.systemPackages = with pkgs; [ a b c d e ]; }",
        );
        let entry = write_nix(
            &dir,
            "config.nix",
            "{ imports = [ ./a.nix ]; environment.systemPackages = [ x ]; }",
        );
        let graph = build_graph(&entry).unwrap();

        let candidates = super::locate_all(&graph, "environment.systemPackages").unwrap();

        assert_eq!(candidates.len(), 2);
        let counts: std::collections::HashSet<usize> =
            candidates.iter().map(|c| c.item_count).collect();
        assert!(counts.contains(&5), "expected a candidate with item_count=5");
        assert!(counts.contains(&1), "expected a candidate with item_count=1");
    }

    #[test]
    fn locate_all_returns_empty_when_not_found() {
        let dir = TempDir::new().unwrap();
        let entry = write_nix(&dir, "config.nix", "{ }");
        let graph = build_graph(&entry).unwrap();

        let candidates = super::locate_all(&graph, "environment.systemPackages").unwrap();

        assert!(candidates.is_empty());
    }

    #[test]
    fn locate_all_single_file_returns_one_candidate() {
        let dir = TempDir::new().unwrap();
        let entry = write_nix(
            &dir,
            "config.nix",
            "{ environment.systemPackages = [ vim git ]; }",
        );
        let graph = build_graph(&entry).unwrap();

        let candidates = super::locate_all(&graph, "environment.systemPackages").unwrap();

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].item_count, 2);
        assert_eq!(candidates[0].file, entry.canonicalize().unwrap());
    }
}
