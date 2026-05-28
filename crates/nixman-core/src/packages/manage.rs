//! List manipulation for NixOS package lists.
//!
//! Provides [`add`] / [`add_in_list`], [`remove`] / [`remove_from_list`],
//! and [`list_installed`] / [`list_installed_in_list`] — all operating on
//! the actual Nix source file rather than shelling out to a `nix` command.
//!
//! The base functions (`add`, `remove`, `list_installed`) target
//! `environment.systemPackages` via `configuration.nix`.
//! The `_in_list` variants accept an arbitrary option path and entry file,
//! allowing reuse for Home Manager's `home.packages` in `home.nix`.
//!
//! # Supported patterns
//!
//! ```nix
//! environment.systemPackages = with pkgs; [ vim git ];   # bare names
//! environment.systemPackages = [ pkgs.vim pkgs.git ];    # qualified names
//! ```
//!
//! Both multi-line and single-line lists are handled. Whitespace, indentation,
//! and comments outside the list are preserved byte-for-byte.

use std::path::{Path, PathBuf};

use rnix::{ast, SyntaxKind};
use rowan::ast::AstNode;

use crate::nix_parser::{
    find_option,
    modules::build_graph,
    reader::parse_string,
    resolver::locate_all,
    types::{ParsedNode, ResolvedCandidate},
};
use crate::packages::types::PackageError;

// ── Public API (backward-compatible NixOS defaults) ───────────────────────────

/// Add a package to `environment.systemPackages` in the workspace config.
///
/// Delegates to [`add_in_list`] with `"environment.systemPackages"` and
/// `"configuration.nix"`.
pub fn add(workspace_path: &Path, pkg_name: &str, target_file: Option<&Path>) -> Result<bool, PackageError> {
    add_in_list(workspace_path, pkg_name, target_file, "environment.systemPackages", "configuration.nix")
}

/// Remove a package from `environment.systemPackages`.
///
/// Delegates to [`remove_from_list`] with `"environment.systemPackages"` and
/// `"configuration.nix"`.
pub fn remove(workspace_path: &Path, pkg_name: &str, target_file: Option<&Path>) -> Result<bool, PackageError> {
    remove_from_list(workspace_path, pkg_name, target_file, "environment.systemPackages", "configuration.nix")
}

/// Return the package names currently in `environment.systemPackages`.
///
/// Delegates to [`list_installed_in_list`] with `"environment.systemPackages"`
/// and `"configuration.nix"`.
pub fn list_installed(workspace_path: &Path) -> Result<Vec<String>, PackageError> {
    list_installed_in_list(workspace_path, "environment.systemPackages", "configuration.nix")
}

/// Resolve the file that would be modified by an add/remove operation
/// targeting `environment.systemPackages`.
pub fn resolve_package_file(
    workspace_path: &Path,
    target_file: Option<&Path>,
) -> Result<PathBuf, PackageError> {
    resolve_package_file_for_list(workspace_path, target_file, "environment.systemPackages", "configuration.nix")
}

// ── Public API (parameterised — use with any list option) ─────────────────────

/// Add a package to the list identified by `option_path` in the config at
/// `workspace_path` (entry file `entry_file`).
///
/// `pkg_name` is the nixpkgs attribute name (e.g. `"btop"`, `"git"`).
/// `target_file` pins the operation to a specific file; `None` auto-selects
/// the file with the most items.
///
/// Returns `Ok(true)` when the package was inserted, `Ok(false)` when it was
/// already present (idempotent — the file is not modified in that case).
pub fn add_in_list(
    workspace_path: &Path,
    pkg_name: &str,
    target_file: Option<&Path>,
    option_path: &str,
    entry_file: &str,
) -> Result<bool, PackageError> {
    let (file, source, node) = find_packages_node(workspace_path, target_file, option_path, entry_file)?;

    let expr = ast::Expr::cast(node.syntax.clone()).ok_or_else(|| {
        PackageError::ParseError("cannot cast value node to Expr".to_string())
    })?;

    let (list, use_bare) = extract_list(&expr)?;

    if is_in_list(&list, pkg_name, use_bare) {
        return Ok(false); // already present — no-op
    }

    let new_source = insert_package(&source, &list, pkg_name, use_bare)?;

    let permissions = std::fs::metadata(&file)
        .map_err(|e| PackageError::ParseError(format!("failed to stat {}: {e}", file.display())))?
        .permissions();
    std::fs::write(&file, new_source).map_err(|e| {
        PackageError::ParseError(format!("failed to write {}: {e}", file.display()))
    })?;
    std::fs::set_permissions(&file, permissions).map_err(|e| {
        PackageError::ParseError(format!("failed to set permissions on {}: {e}", file.display()))
    })?;

    Ok(true)
}

/// Remove a package from the list identified by `option_path`.
///
/// When `target_file` is `None`, all definition sites are scanned. If the
/// package appears in exactly one file it is removed there. If it appears in
/// multiple files [`PackageError::AmbiguousRemove`] is returned. Pass
/// `target_file` to pin the operation to a specific file.
///
/// Returns [`PackageError::NotInConfig`] when `pkg_name` is not found.
pub fn remove_from_list(
    workspace_path: &Path,
    pkg_name: &str,
    target_file: Option<&Path>,
    option_path: &str,
    entry_file: &str,
) -> Result<bool, PackageError> {
    if let Some(path) = target_file {
        let (file, source, node) = find_packages_node(workspace_path, Some(path), option_path, entry_file)?;
        return remove_pkg_from_file(&file, &source, node, pkg_name);
    }

    let entry = workspace_path.join(entry_file);
    let graph = build_graph(&entry).map_err(|e| {
        PackageError::ParseError(format!("failed to build module graph: {e}"))
    })?;
    let candidates = locate_all(&graph, option_path).map_err(|e| {
        PackageError::ParseError(format!("resolver error: {e}"))
    })?;

    struct Hit {
        file: PathBuf,
        source: String,
        node: ParsedNode,
    }

    let mut hits: Vec<Hit> = Vec::new();

    for candidate in candidates {
        let source = std::fs::read_to_string(&candidate.file).map_err(|e| {
            PackageError::ParseError(format!("failed to read {}: {e}", candidate.file.display()))
        })?;
        let nix_file = parse_string(&source).map_err(|e| {
            PackageError::ParseError(format!(
                "failed to parse {}: {e}",
                candidate.file.display()
            ))
        })?;
        let node = match find_option(&nix_file, option_path) {
            Some(n) => n,
            None => continue,
        };
        let expr = match ast::Expr::cast(node.syntax.clone()) {
            Some(e) => e,
            None => continue,
        };
        let Ok((list, use_bare)) = extract_list(&expr) else { continue };
        if is_in_list(&list, pkg_name, use_bare) {
            hits.push(Hit { file: candidate.file, source, node });
        }
    }

    match hits.len() {
        0 => Err(PackageError::NotInConfig(pkg_name.to_string())),
        1 => {
            let hit = hits.remove(0);
            remove_pkg_from_file(&hit.file, &hit.source, hit.node, pkg_name)
        }
        _ => Err(PackageError::AmbiguousRemove(
            hits.into_iter().map(|h| {
                let byte_offset = u32::from(h.node.syntax.text_range().start()) as usize;
                let line_num = h.source[..byte_offset].matches('\n').count() + 1;
                (h.file, line_num)
            }).collect(),
        )),
    }
}

/// Return the package names currently declared in the list at `option_path`.
///
/// Collects names from all definition sites. Items that cannot be statically
/// resolved to a simple name are omitted.
pub fn list_installed_in_list(
    workspace_path: &Path,
    option_path: &str,
    entry_file: &str,
) -> Result<Vec<String>, PackageError> {
    let entry = workspace_path.join(entry_file);
    let graph = build_graph(&entry).map_err(|e| {
        PackageError::ParseError(format!("failed to build module graph: {e}"))
    })?;
    let candidates = locate_all(&graph, option_path).map_err(|e| {
        PackageError::ParseError(format!("resolver error: {e}"))
    })?;

    let mut packages = Vec::new();
    for candidate in candidates {
        let source = std::fs::read_to_string(&candidate.file).map_err(|e| {
            PackageError::ParseError(format!(
                "failed to read {}: {e}",
                candidate.file.display()
            ))
        })?;
        let nix_file = parse_string(&source).map_err(|e| {
            PackageError::ParseError(format!(
                "failed to parse {}: {e}",
                candidate.file.display()
            ))
        })?;
        if let Some(node) = find_option(&nix_file, option_path) {
            if let Some(expr) = ast::Expr::cast(node.syntax.clone()) {
                if let Ok((list, use_bare)) = extract_list(&expr) {
                    packages.extend(collect_names(&list, use_bare));
                }
            }
        }
    }

    Ok(packages)
}

/// Resolve the file that would be modified by an add/remove operation
/// targeting the list at `option_path`.
pub fn resolve_package_file_for_list(
    workspace_path: &Path,
    target_file: Option<&Path>,
    option_path: &str,
    entry_file: &str,
) -> Result<PathBuf, PackageError> {
    let (file, _, _) = find_packages_node(workspace_path, target_file, option_path, entry_file)?;
    Ok(file)
}

/// Verify that a package exists in nixpkgs by running `nix eval`.
///
/// For dotted names like `kdePackages.dolphin`, evaluates `nixpkgs#kdePackages.dolphin`.
/// Returns `Ok(())` if the package exists, `Err(PackageError::NotFound)` if it doesn't,
/// and `Err(PackageError::NixNotAvailable)` when the `nix` binary can't be found.
pub fn verify_package(pkg_name: &str) -> Result<(), PackageError> {
    let result = std::process::Command::new("nix")
        .args([
            "--experimental-features",
            "nix-command flakes",
            "eval",
            &format!("nixpkgs#{}", pkg_name),
            "--apply",
            "x: x.meta.name or x.name or \"ok\"",
            "--raw",
        ])
        .output();

    match result {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(PackageError::NixNotAvailable)
        }
        Err(e) => Err(PackageError::ParseError(format!("failed to run nix eval: {}", e))),
        Ok(output) => {
            if output.status.success() {
                Ok(())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let hint = if stderr.contains("does not provide") {
                    stderr
                        .lines()
                        .find(|l| l.contains("Did you mean"))
                        .unwrap_or("Check the attribute name with 'nix search nixpkgs <name>'.")
                        .to_string()
                } else {
                    "Check the attribute name with 'nix search nixpkgs <name>'.".to_string()
                };
                Err(PackageError::NotFound(format!(
                    "Package '{}' not found in nixpkgs. {}",
                    pkg_name, hint
                )))
            }
        }
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Build the module graph, find which file owns the list at `option_path`,
/// read + parse it, and return `(file_path, source_text, value_node)`.
///
/// When `target_file` is `Some(path)`, only that file is considered.
/// When `target_file` is `None` and multiple files define the option, the
/// file with the highest item count is selected (ties broken by path).
fn find_packages_node(
    workspace_path: &Path,
    target_file: Option<&Path>,
    option_path: &str,
    entry_file: &str,
) -> Result<(PathBuf, String, ParsedNode), PackageError> {
    let entry = workspace_path.join(entry_file);

    let graph = build_graph(&entry).map_err(|e| {
        PackageError::ParseError(format!("failed to build module graph: {e}"))
    })?;

    let mut candidates: Vec<ResolvedCandidate> =
        locate_all(&graph, option_path).map_err(|e| {
            PackageError::ParseError(format!("resolver error: {e}"))
        })?;

    let selected = if let Some(path) = target_file {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        candidates.retain(|c| c.file == canonical || c.file == path);
        candidates.into_iter().next().ok_or_else(|| {
            PackageError::ParseError(format!(
                "{} not found in {}",
                option_path,
                path.display()
            ))
        })?
    } else {
        match candidates.len() {
            0 => {
                return Err(PackageError::ParseError(format!(
                    "{} not found in configuration",
                    option_path,
                )))
            }
            1 => candidates.remove(0),
            _ => {
                candidates.sort_by(|a, b| {
                    b.item_count
                        .cmp(&a.item_count)
                        .then_with(|| a.file.cmp(&b.file))
                });
                let selected = candidates.remove(0);
                eprintln!(
                    "note: using {} ({} packages)",
                    selected.file.display(),
                    selected.item_count
                );
                selected
            }
        }
    };

    let source = std::fs::read_to_string(&selected.file).map_err(|e| {
        PackageError::ParseError(format!(
            "failed to read {}: {e}",
            selected.file.display()
        ))
    })?;

    let nix_file = parse_string(&source).map_err(|e| {
        PackageError::ParseError(format!(
            "failed to parse {}: {e}",
            selected.file.display()
        ))
    })?;

    let node = find_option(&nix_file, option_path).ok_or_else(|| {
        PackageError::ParseError(format!(
            "{} not found after locate",
            option_path,
        ))
    })?;

    Ok((selected.file, source, node))
}


// File-level remove helper

fn remove_pkg_from_file(
    file: &Path,
    source: &str,
    node: ParsedNode,
    pkg_name: &str,
) -> Result<bool, PackageError> {
    let expr = ast::Expr::cast(node.syntax.clone()).ok_or_else(|| {
        PackageError::ParseError("cannot cast value node to Expr".to_string())
    })?;

    let (list, use_bare) = extract_list(&expr)?;

    let item = find_matching_item(&list, pkg_name, use_bare)
        .ok_or_else(|| PackageError::NotInConfig(pkg_name.to_string()))?;

    let new_source = delete_package(source, &list, &item)?;

    let permissions = std::fs::metadata(file)
        .map_err(|e| PackageError::ParseError(format!("failed to stat {}: {e}", file.display())))?
        .permissions();
    std::fs::write(file, new_source).map_err(|e| {
        PackageError::ParseError(format!("failed to write {}: {e}", file.display()))
    })?;
    std::fs::set_permissions(file, permissions).map_err(|e| {
        PackageError::ParseError(format!("failed to set permissions on {}: {e}", file.display()))
    })?;

    Ok(true)
}

// List kind detection

/// Unwrap the expression assigned to the list and return the list AST node
/// together with a flag indicating the naming convention.
///
/// | Expression                  | `use_bare` | Insert as       |
/// |-----------------------------|-----------|-----------------|
/// | `with pkgs; [ vim git ]`    | `true`    | `firefox`       |
/// | `[ pkgs.vim pkgs.git ]`     | `false`   | `pkgs.firefox`  |
fn extract_list(expr: &ast::Expr) -> Result<(ast::List, bool), PackageError> {
    match expr {
        ast::Expr::With(with_expr) => {
            let body = with_expr.body().ok_or_else(|| {
                PackageError::ParseError("with expression has no body".to_string())
            })?;
            match body {
                ast::Expr::List(list) => Ok((list, true)),
                other => Err(PackageError::ParseError(format!(
                    "with expression body is not a list: {}",
                    other.syntax()
                ))),
            }
        }
        ast::Expr::List(list) => Ok((list.clone(), false)),
        other => Err(PackageError::ParseError(format!(
            "option value is not a list or with-expression: {}",
            other.syntax()
        ))),
    }
}

// Item matching

fn is_in_list(list: &ast::List, pkg_name: &str, use_bare: bool) -> bool {
    find_matching_item(list, pkg_name, use_bare).is_some()
}

fn find_matching_item(list: &ast::List, pkg_name: &str, use_bare: bool) -> Option<ast::Expr> {
    for item in list.items() {
        if use_bare {
            if matches_bare_ident(&item, pkg_name) {
                return Some(item);
            }
            // Handle dotted paths like `kdePackages.kdeconnect-kde` which parse
            // as Select expressions, not bare Idents.
            if item.syntax().text().to_string().trim() == pkg_name {
                return Some(item);
            }
        } else if matches_select(&item, pkg_name) {
            return Some(item);
        }
    }
    None
}

/// Returns `true` when `expr` is a bare identifier equal to `name`.
fn matches_bare_ident(expr: &ast::Expr, name: &str) -> bool {
    if let ast::Expr::Ident(ident) = expr {
        ident
            .ident_token()
            .map(|t| t.text() == name)
            .unwrap_or(false)
    } else {
        false
    }
}

/// Returns `true` when `expr` is `pkgs.<name>` — a Select with a single-
/// segment attrpath equal to `name`.
fn matches_select(expr: &ast::Expr, name: &str) -> bool {
    if let ast::Expr::Select(sel) = expr {
        if let Some(ap) = sel.attrpath() {
            let attrs: Vec<_> = ap.attrs().collect();
            if attrs.len() == 1 {
                if let ast::Attr::Ident(ident) = &attrs[0] {
                    return ident
                        .ident_token()
                        .map(|t| t.text() == name)
                        .unwrap_or(false);
                }
            }
        }
    }
    false
}

/// Collect the plain package names from a list.
fn collect_names(list: &ast::List, use_bare: bool) -> Vec<String> {
    list.items()
        .filter_map(|item| {
            if use_bare {
                if let ast::Expr::Ident(ident) = &item {
                    return ident.ident_token().map(|t| t.text().to_string());
                }
                // Handle dotted paths like kdePackages.foo
                if let ast::Expr::Select(_) = &item {
                    return Some(item.syntax().text().to_string());
                }
            } else if let ast::Expr::Select(sel) = &item {
                if let Some(ap) = sel.attrpath() {
                    let attrs: Vec<_> = ap.attrs().collect();
                    if attrs.len() == 1 {
                        if let ast::Attr::Ident(ident) = &attrs[0] {
                            return ident.ident_token().map(|t| t.text().to_string());
                        }
                    }
                }
            }
            None
        })
        .collect()
}

// Source-level add

fn insert_package(
    source: &str,
    list: &ast::List,
    pkg_name: &str,
    use_bare: bool,
) -> Result<String, PackageError> {
    let list_range = list.syntax().text_range();
    let list_start = u32::from(list_range.start()) as usize;
    let list_end = u32::from(list_range.end()) as usize;
    let list_text = &source[list_start..list_end];

    let r_brack_pos = find_r_brack_pos(list)
        .ok_or_else(|| PackageError::ParseError("could not locate `]` in list".to_string()))?;

    let pkg_ref = if use_bare {
        pkg_name.to_string()
    } else {
        format!("pkgs.{}", pkg_name)
    };

    // whitespace that precedes `]`, so the space before `]` is preserved.
    let (insert_pos, insertion) = if list_text.contains('\n') {
        let indent = detect_item_indent(source, list, list_start);
        (r_brack_pos, format!("{}{}\n", indent, pkg_ref))
    } else {
        // so the existing whitespace is kept as the separator before `]`.
        let before = &source[..r_brack_pos];
        let trimmed_len = before
            .trim_end_matches([' ', '\t'])
            .len();
        (trimmed_len, format!(" {}", pkg_ref))
    };

    let result = format!(
        "{}{}{}",
        &source[..insert_pos],
        insertion,
        &source[insert_pos..]
    );

    parse_string(&result).map_err(|e| {
        PackageError::ParseError(format!("validation failed after add: {e}"))
    })?;

    Ok(result)
}

// Source-level remove

fn delete_package(
    source: &str,
    list: &ast::List,
    item: &ast::Expr,
) -> Result<String, PackageError> {
    let list_range = list.syntax().text_range();
    let list_start = u32::from(list_range.start()) as usize;
    let list_end = u32::from(list_range.end()) as usize;
    let is_multiline = source[list_start..list_end].contains('\n');

    let item_range = item.syntax().text_range();
    let item_start = u32::from(item_range.start()) as usize;
    let item_end = u32::from(item_range.end()) as usize;

    let (rm_start, rm_end) = if is_multiline {
        let before_item = &source[list_start..item_start];
        let rm_start = match before_item.rfind('\n') {
            Some(rel_nl) => {
                let indent = &before_item[rel_nl + 1..];
                if indent.chars().all(|c| c == ' ' || c == '\t') {
                    list_start + rel_nl + 1
                } else {
                    item_start
                }
            }
            None => item_start,
        };

        let after_item = &source[item_end..];
        let rm_end = match after_item.find('\n') {
            Some(rel_nl) => item_end + rel_nl + 1,
            None => item_end,
        };

        (rm_start, rm_end)
    } else {
        let is_first = list
            .items()
            .next()
            .map(|first| {
                u32::from(first.syntax().text_range().start())
                    == u32::from(item_range.start())
            })
            .unwrap_or(false);

        if is_first {
            let rm_end = if item_end < source.len() && source.as_bytes()[item_end] == b' ' {
                item_end + 1
            } else {
                item_end
            };
            (item_start, rm_end)
        } else {
            let rm_start =
                if item_start > 0 && source.as_bytes()[item_start - 1] == b' ' {
                    item_start - 1
                } else {
                    item_start
                };
            (rm_start, item_end)
        }
    };

    let result = format!("{}{}", &source[..rm_start], &source[rm_end..]);

    parse_string(&result).map_err(|e| {
        PackageError::ParseError(format!("validation failed after remove: {e}"))
    })?;

    Ok(result)
}


/// Return the byte offset of the `]` token in `list` (absolute, in source).
fn find_r_brack_pos(list: &ast::List) -> Option<usize> {
    list.syntax()
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| t.kind() == SyntaxKind::TOKEN_R_BRACK)
        .map(|t| u32::from(t.text_range().start()) as usize)
}

/// Detect the indentation used for items in `list`.
///
/// For a non-empty list, returns the leading whitespace before the first item.
/// For an empty multi-line list, returns the indent before `]` plus two spaces.
fn detect_item_indent(source: &str, list: &ast::List, list_start: usize) -> String {
    if let Some(first_item) = list.items().next() {
        let item_start = u32::from(first_item.syntax().text_range().start()) as usize;
        let between = &source[list_start..item_start];
        if let Some(nl_pos) = between.rfind('\n') {
            return between[nl_pos + 1..].to_string();
        }
    }

    if let Some(r_brack_pos) = find_r_brack_pos(list) {
        let before_brack = &source[list_start..r_brack_pos];
        if let Some(nl_pos) = before_brack.rfind('\n') {
            let bracket_indent = &before_brack[nl_pos + 1..];
            return format!("{}  ", bracket_indent);
        }
    }

    "  ".to_string()
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Write `configuration.nix` with `content` and return the temp dir.
    fn setup(content: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("configuration.nix"), content).unwrap();
        dir
    }

    /// Write `home.nix` with `content` and return the temp dir.
    fn setup_hm(content: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("home.nix"), content).unwrap();
        dir
    }

    /// Read a file back from a temp dir.
    fn read_config(dir: &TempDir, name: &str) -> String {
        fs::read_to_string(dir.path().join(name)).unwrap()
    }


    #[test]
    fn add_to_with_pkgs_multiline() {
        let src = concat!(
            "{ config, pkgs, ... }:\n",
            "{\n",
            "  environment.systemPackages = with pkgs; [\n",
            "    vim\n",
            "    git\n",
            "  ];\n",
            "}"
        );
        let dir = setup(src);

        add(dir.path(), "firefox", None).unwrap();

        let result = read_config(&dir, "configuration.nix");
        assert!(result.contains("firefox"), "firefox should be added");
        assert!(result.contains("vim"), "vim should be preserved");
        assert!(result.contains("git"), "git should be preserved");
        parse_string(&result).expect("result should be valid Nix");
    }


    #[test]
    fn add_to_qualified_list() {
        let src = concat!(
            "{ config, pkgs, ... }:\n",
            "{\n",
            "  environment.systemPackages = [ pkgs.vim pkgs.git ];\n",
            "}"
        );
        let dir = setup(src);

        add(dir.path(), "btop", None).unwrap();

        let result = read_config(&dir, "configuration.nix");
        assert!(result.contains("pkgs.btop"), "pkgs.btop should be added");
        assert!(result.contains("pkgs.vim"), "pkgs.vim should be preserved");
        parse_string(&result).expect("result should be valid Nix");
    }


    #[test]
    fn add_to_empty_list() {
        let src = concat!(
            "{ config, pkgs, ... }:\n",
            "{\n",
            "  environment.systemPackages = with pkgs; [ ];\n",
            "}"
        );
        let dir = setup(src);

        add(dir.path(), "vim", None).unwrap();

        let result = read_config(&dir, "configuration.nix");
        assert!(result.contains("vim"), "vim should be added");
        parse_string(&result).expect("result should be valid Nix");
    }


    #[test]
    fn remove_from_with_pkgs_single_line() {
        let src = concat!(
            "{ config, pkgs, ... }:\n",
            "{\n",
            "  environment.systemPackages = with pkgs; [ vim git ];\n",
            "}"
        );
        let dir = setup(src);

        remove(dir.path(), "vim", None).unwrap();

        let result = read_config(&dir, "configuration.nix");
        // "vim" must not appear as a standalone token — but "git" must survive
        assert!(!result.contains("vim"), "vim should be removed");
        assert!(result.contains("git"), "git should be preserved");
        parse_string(&result).expect("result should be valid Nix");
    }

    #[test]
    fn remove_from_with_pkgs_multiline() {
        let src = concat!(
            "{ config, pkgs, ... }:\n",
            "{\n",
            "  environment.systemPackages = with pkgs; [\n",
            "    vim\n",
            "    git\n",
            "    firefox\n",
            "  ];\n",
            "}"
        );
        let dir = setup(src);

        remove(dir.path(), "git", None).unwrap();

        let result = read_config(&dir, "configuration.nix");
        assert!(!result.contains("git"), "git should be removed");
        assert!(result.contains("vim"), "vim should be preserved");
        assert!(result.contains("firefox"), "firefox should be preserved");
        parse_string(&result).expect("result should be valid Nix");
    }


    #[test]
    fn remove_nonexistent_returns_error() {
        let src = concat!(
            "{ config, pkgs, ... }:\n",
            "{\n",
            "  environment.systemPackages = with pkgs; [ vim ];\n",
            "}"
        );
        let dir = setup(src);

        let err = remove(dir.path(), "firefox", None).unwrap_err();
        assert!(
            matches!(err, PackageError::NotInConfig(_)),
            "expected NotInConfig, got {err:?}"
        );
    }


    #[test]
    fn add_duplicate_is_noop() {
        let src = concat!(
            "{ config, pkgs, ... }:\n",
            "{\n",
            "  environment.systemPackages = with pkgs; [ vim git ];\n",
            "}"
        );
        let dir = setup(src);

        add(dir.path(), "vim", None).unwrap();

        let result = read_config(&dir, "configuration.nix");
        // Count standalone "vim" occurrences — should still be exactly one.
        let count = result.split_whitespace().filter(|w| *w == "vim").count();
        assert_eq!(count, 1, "vim should appear exactly once, got: {result}");
    }


    #[test]
    fn preserves_comments_and_surrounding_content() {
        let src = concat!(
            "# My NixOS config\n",
            "{ config, pkgs, ... }:\n",
            "{\n",
            "  # Installed packages\n",
            "  environment.systemPackages = with pkgs; [\n",
            "    vim # text editor\n",
            "    git\n",
            "  ];\n",
            "  # End of config\n",
            "}"
        );
        let dir = setup(src);

        add(dir.path(), "firefox", None).unwrap();

        let result = read_config(&dir, "configuration.nix");
        assert!(result.contains("# My NixOS config"), "file comment preserved");
        assert!(result.contains("# Installed packages"), "block comment preserved");
        assert!(result.contains("vim # text editor"), "inline comment preserved");
        assert!(result.contains("# End of config"), "trailing comment preserved");
        assert!(result.contains("firefox"), "firefox should be present");
        parse_string(&result).expect("result should be valid Nix");
    }


    #[test]
    fn list_installed_returns_package_names() {
        let src = concat!(
            "{ config, pkgs, ... }:\n",
            "{\n",
            "  environment.systemPackages = with pkgs; [ vim git firefox ];\n",
            "}"
        );
        let dir = setup(src);

        let packages = list_installed(dir.path()).unwrap();
        assert!(packages.contains(&"vim".to_string()));
        assert!(packages.contains(&"git".to_string()));
        assert!(packages.contains(&"firefox".to_string()));
    }

    // ── add_in_list / remove_from_list / list_installed_in_list ────────────
    //

    #[test]
    fn add_in_list_hm_home_packages() {
        let src = concat!(
            "{ config, pkgs, ... }:\n",
            "{\n",
            "  home.packages = with pkgs; [ vim git ];\n",
            "}"
        );
        let dir = setup_hm(src);

        add_in_list(dir.path(), "firefox", None, "home.packages", "home.nix").unwrap();

        let result = read_config(&dir, "home.nix");
        assert!(result.contains("firefox"), "firefox should be added");
        assert!(result.contains("vim"), "vim should be preserved");
        parse_string(&result).expect("result should be valid Nix");
    }

    #[test]
    fn add_in_list_home_packages_with_select() {
        let src = concat!(
            "{ config, pkgs, ... }:\n",
            "{\n",
            "  home.packages = [ pkgs.vim pkgs.git ];\n",
            "}"
        );
        let dir = setup_hm(src);

        add_in_list(dir.path(), "btop", None, "home.packages", "home.nix").unwrap();

        let result = read_config(&dir, "home.nix");
        assert!(result.contains("pkgs.btop"), "pkgs.btop should be added");
        parse_string(&result).expect("result should be valid Nix");
    }

    #[test]
    fn remove_from_list_home_packages() {
        let src = concat!(
            "{ config, pkgs, ... }:\n",
            "{\n",
            "  home.packages = with pkgs; [ vim git firefox ];\n",
            "}"
        );
        let dir = setup_hm(src);

        remove_from_list(dir.path(), "git", None, "home.packages", "home.nix").unwrap();

        let result = read_config(&dir, "home.nix");
        assert!(!result.contains("git"), "git should be removed");
        assert!(result.contains("vim"), "vim should be preserved");
        parse_string(&result).expect("result should be valid Nix");
    }

    #[test]
    fn list_installed_in_list_home_packages() {
        let src = concat!(
            "{ config, pkgs, ... }:\n",
            "{\n",
            "  home.packages = with pkgs; [ vim git firefox ];\n",
            "}"
        );
        let dir = setup_hm(src);

        let packages = list_installed_in_list(dir.path(), "home.packages", "home.nix").unwrap();
        assert!(packages.contains(&"vim".to_string()));
        assert!(packages.contains(&"git".to_string()));
        assert!(packages.contains(&"firefox".to_string()));
    }
}
