//! Logic for inserting new options into a Nix source file.
//!
//! [`add_option`] finds the best attribute set to receive the new binding,
//! computes the insertion byte offset, and splices the new line into the
//! source string while preserving all surrounding content.

use rnix::{ast, SyntaxKind};
use rnix::ast::HasEntry;
use rowan::ast::AstNode;

use crate::nix_parser::format::value_to_nix;
use crate::nix_parser::reader::parse_string;
use crate::nix_parser::types::NixValue;
use crate::nix_parser::writer::WriteError;

// Public API

/// Insert a new option binding into `source`.
///
/// The function searches for the deepest existing attribute set whose key
/// matches a prefix of `option_path`.  The new binding is appended to that
/// set just before its closing `}`.  If no prefix match is found the binding
/// is inserted in the root attribute set.
///
/// `indent` is the complete leading whitespace string for the new line (e.g.
/// `"  "` for 2-space indentation, `"\t"` for tab indentation).
///
/// # Example
///
/// ```rust,ignore
/// let src = "{\n  services.nginx.enable = true;\n}";
/// let result = add_option(src, "services.nginx.port", &NixValue::Int(8080), "  ")?;
/// // result ≈ "{\n  services.nginx.enable = true;\n  services.nginx.port = 8080;\n}"
/// ```
///
/// # Errors
///
/// * [`WriteError::InsertionPointNotFound`] — no root attribute set found.
/// * [`WriteError::ValidationFailed`] — the result fails to re-parse.
pub fn add_option(
    source: &str,
    option_path: &str,
    value: &NixValue,
    indent: &str,
) -> Result<String, WriteError> {
    let path_parts: Vec<&str> = option_path
        .split('.')
        .filter(|s| !s.is_empty())
        .collect();

    if path_parts.is_empty() {
        return Err(WriteError::SerializationFailed(
            "empty option path".to_string(),
        ));
    }

    let nix_file =
        parse_string(source).map_err(|e| WriteError::ValidationFailed(e.to_string()))?;

    let root_expr = nix_file
        .root
        .expr()
        .ok_or(WriteError::InsertionPointNotFound)?;

    let root_attrset = peel_to_attrset(&root_expr).ok_or(WriteError::InsertionPointNotFound)?;

    // Find the deepest existing AttrSet that matches a prefix of the path.
    // `consumed` is the number of path parts absorbed by the matched AttrSet;
    // the remaining parts form the key for the new binding.
    let (target_attrset, consumed) = find_best_attrset(&root_attrset, &path_parts);

    // The remaining path becomes the key in the new binding.
    let remaining_path = path_parts[consumed..].join(".");

    // Find the byte offset of the closing `}` of the target AttrSet.
    let r_brace_pos =
        find_r_brace_position(&target_attrset).ok_or(WriteError::InsertionPointNotFound)?;

    // Build the new line: "{indent}{key} = {value};\n"
    let new_line = format!("{}{} = {};\n", indent, remaining_path, value_to_nix(value));

    // Splice into the source.
    let result = format!(
        "{}{}{}",
        &source[..r_brace_pos],
        new_line,
        &source[r_brace_pos..]
    );

    // Verify.
    parse_string(&result).map_err(|e| WriteError::ValidationFailed(e.to_string()))?;

    Ok(result)
}

// Internal helpers

/// Peel top-level lambdas and parentheses to reach the underlying `AttrSet`.
fn peel_to_attrset(expr: &ast::Expr) -> Option<ast::AttrSet> {
    match expr {
        ast::Expr::AttrSet(a) => Some(a.clone()),
        ast::Expr::Lambda(l) => peel_to_attrset(&l.body()?),
        ast::Expr::Paren(p) => peel_to_attrset(&p.expr()?),
        _ => None,
    }
}

/// Find the deepest `AttrSet` whose key path matches the longest prefix of
/// `path_parts`.
///
/// Returns `(attrset, consumed_prefix_length)`.  When no prefix match is
/// found, returns the root `AttrSet` with `consumed = 0`.
fn find_best_attrset<'a>(
    root: &'a ast::AttrSet,
    path_parts: &[&str],
) -> (ast::AttrSet, usize) {
    // Try from the longest possible prefix down to 1.
    for prefix_len in (1..path_parts.len()).rev() {
        let prefix = &path_parts[..prefix_len];
        if let Some(attrset) = find_attrset_at_prefix(root, prefix) {
            return (attrset, prefix_len);
        }
    }

    // Fall back to the root AttrSet with no prefix consumed.
    (root.clone(), 0)
}

/// Recursively search `attrset` for a nested `AttrSet` reachable via `prefix`.
///
/// Returns `None` if the path does not exist or the value at the path is not
/// an `AttrSet`.
fn find_attrset_at_prefix(attrset: &ast::AttrSet, prefix: &[&str]) -> Option<ast::AttrSet> {
    if prefix.is_empty() {
        return Some(attrset.clone());
    }

    for entry in attrset.attrpath_values() {
        let attrpath = entry.attrpath()?;
        let value = entry.value()?;
        let entry_keys = collect_attr_names(&attrpath);

        if entry_keys.is_empty() {
            continue;
        }

        // The entry keys must be a prefix of (or equal to) the remaining
        // prefix we are searching for.
        if prefix.len() >= entry_keys.len() && keys_prefix_or_eq(&entry_keys, prefix) {
            // The value must be an AttrSet for us to recurse into it.
            if let ast::Expr::AttrSet(nested) = &value {
                 return find_attrset_at_prefix(&nested, &prefix[entry_keys.len()..]);
            }
        }
    }

    None
}

/// Return the byte offset of the closing `}` token of `attrset` in the
/// original source.
fn find_r_brace_position(attrset: &ast::AttrSet) -> Option<usize> {
    attrset
        .syntax()
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| t.kind() == SyntaxKind::TOKEN_R_BRACE)
        .map(|t| u32::from(t.text_range().start()) as usize)
}

// Attribute-key helpers (module-local, mirrors traversal.rs internals)

fn collect_attr_names(attrpath: &ast::Attrpath) -> Vec<String> {
    attrpath
        .attrs()
        .filter_map(|attr| attr_to_string(&attr))
        .collect()
}

fn attr_to_string(attr: &ast::Attr) -> Option<String> {
    match attr {
        ast::Attr::Ident(ident) => ident.ident_token().map(|t| t.text().to_string()),
        ast::Attr::Str(s) => {
            let parts = s.normalized_parts();
            let has_interp = parts
                .iter()
                .any(|p| matches!(p, ast::InterpolPart::Interpolation(_)));
            if has_interp {
                return None;
            }
            let text: String = parts
                .iter()
                .filter_map(|p| {
                    if let ast::InterpolPart::Literal(s) = p {
                        Some(s.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            Some(text)
        }
        ast::Attr::Dynamic(_) => None,
    }
}

/// Return `true` when `keys` equals `parts[..keys.len()]` (prefix or exact).
fn keys_prefix_or_eq(keys: &[String], parts: &[&str]) -> bool {
    keys.len() <= parts.len()
        && keys
            .iter()
            .zip(parts.iter())
            .all(|(k, p)| k.as_str() == *p)
}

// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nix_parser::parse_string;

    // add_option — basic insertion

    #[test]
    fn add_option_to_root_attrset() {
        let src = "{\n  enable = true;\n}";
        let result = add_option(src, "port", &NixValue::Int(8080), "  ").unwrap();
        assert!(result.contains("port = 8080;"));
        assert!(result.contains("enable = true;"));
        parse_string(&result).expect("result should be valid Nix");
    }

    #[test]
    fn add_option_preserves_existing_content() {
        let src = "# My NixOS config\n{\n  services.nginx.enable = true;\n}";
        let result =
            add_option(src, "services.nginx.port", &NixValue::Int(443), "  ").unwrap();
        assert!(result.starts_with("# My NixOS config\n"));
        assert!(result.contains("services.nginx.enable = true;"));
        assert!(result.contains("services.nginx.port = 443;"));
        parse_string(&result).expect("result should be valid Nix");
    }

    #[test]
    fn add_option_string_value() {
        let src = "{\n}";
        let result = add_option(
            src,
            "networking.hostname",
            &NixValue::String("myhost".into()),
            "  ",
        )
        .unwrap();
        assert!(result.contains(r#"networking.hostname = "myhost";"#));
        parse_string(&result).expect("result should be valid Nix");
    }

    #[test]
    fn add_option_bool_value() {
        let src = "{\n}";
        let result = add_option(src, "boot.loader.grub.enable", &NixValue::Bool(true), "  ")
            .unwrap();
        assert!(result.contains("boot.loader.grub.enable = true;"));
        parse_string(&result).expect("result should be valid Nix");
    }

    #[test]
    fn add_option_list_value() {
        let src = "{\n}";
        let result = add_option(
            src,
            "environment.systemPackages",
            &NixValue::List(vec![
                NixValue::Expression("pkgs.vim".into()),
                NixValue::Expression("pkgs.git".into()),
            ]),
            "  ",
        )
        .unwrap();
        assert!(result.contains("environment.systemPackages = [ pkgs.vim pkgs.git ];"));
        parse_string(&result).expect("result should be valid Nix");
    }

    #[test]
    fn add_option_inserts_before_closing_brace() {
        let src = "{\n  enable = true;\n}";
        let result = add_option(src, "port", &NixValue::Int(80), "  ").unwrap();
        // The new line must appear before the closing `}`.
        let brace_pos = result.rfind('}').unwrap();
        let port_pos = result.find("port = 80").unwrap();
        assert!(port_pos < brace_pos);
    }

    #[test]
    fn add_option_finds_nested_attrset() {
        // If an existing nested AttrSet matches a prefix of the path,
        // insert inside that AttrSet using only the remaining key.
        let src = "{\n  services = {\n    enable = true;\n  };\n}";
        let result = add_option(src, "services.port", &NixValue::Int(80), "    ").unwrap();
        // The new entry should be inside `services = { ... }`.
        assert!(result.contains("port = 80;"));
        parse_string(&result).expect("result should be valid Nix");
    }

    #[test]
    fn add_option_through_lambda() {
        let src = "{ config, pkgs, ... }:\n{\n  services.nginx.enable = true;\n}";
        let result = add_option(src, "services.nginx.port", &NixValue::Int(80), "  ").unwrap();
        assert!(result.contains("services.nginx.port = 80;"));
        // Lambda header must be preserved.
        assert!(result.starts_with("{ config, pkgs, ... }:"));
        parse_string(&result).expect("result should be valid Nix");
    }

    #[test]
    fn add_option_with_tab_indent() {
        let src = "{\n\tenable = true;\n}";
        let result = add_option(src, "port", &NixValue::Int(22), "\t").unwrap();
        assert!(result.contains("\tport = 22;"));
        parse_string(&result).expect("result should be valid Nix");
    }
}
