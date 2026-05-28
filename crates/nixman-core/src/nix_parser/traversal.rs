//! AST traversal utilities.
//!
//! Provides:
//!
//! * [`find_option`] — locate a value node by a dotted option path.
//! * [`expr_to_value`] — convert an AST expression to a [`NixValue`].
//! * [`iterate_attr_set`] — iterate the key/value pairs of an attribute set
//!   expression.

use rnix::ast::{self, AstToken, HasEntry};
use rowan::ast::AstNode;

use super::types::{NixFile, NixValue, ParsedNode};

// Public API

/// Find the value node for `path` inside `nix_file`.
///
/// `path` is a dotted attribute path such as `"services.nginx.enable"`.
///
/// The function handles both:
/// - *Flat dotted keys*: `services.nginx.enable = true;`
/// - *Nested attribute sets*: `services = { nginx = { enable = true; }; };`
///
/// It also unwraps top-level lambdas (the common `{ config, pkgs, ... }: { … }`
/// form used in NixOS modules) before searching.
///
/// Returns `None` if the path cannot be found or the file does not start
/// with an attribute set (after unwrapping lambdas).
pub fn find_option(nix_file: &NixFile, path: &str) -> Option<ParsedNode> {
    if path.is_empty() {
        return None;
    }

    let path_parts: Vec<&str> = path.split('.').filter(|s| !s.is_empty()).collect();
    if path_parts.is_empty() {
        return None;
    }

    let root_expr = nix_file.root.expr()?;
    let attrset = unwrap_to_attrset(&root_expr)?;
    find_in_attrset(&attrset, &path_parts)
}

/// Convert an [`ast::Expr`] to a [`NixValue`].
///
/// This is a best-effort conversion.  Expressions that cannot be represented
/// as a primitive value fall back to `NixValue::Expression(raw_text)`.
pub fn expr_to_value(expr: &ast::Expr) -> NixValue {
    match expr {
        ast::Expr::Ident(ident) => extract_ident_value(ident),
        ast::Expr::Literal(lit) => extract_literal_value(lit),
        ast::Expr::Str(s) => extract_string_value(s),
        ast::Expr::Path(p) => NixValue::Path(p.syntax().to_string()),
        ast::Expr::List(list) => {
            let items = list.items().map(|item| expr_to_value(&item)).collect();
            NixValue::List(items)
        }
        ast::Expr::AttrSet(attrset) => extract_attrset_value(attrset),
        // Wrap everything else (BinOp, IfElse, Lambda, Apply, …) as Expression.
        other => NixValue::Expression(other.syntax().to_string()),
    }
}

/// Iterate the key-value pairs of the attribute set at the root of
/// `nix_file` (after unwrapping any top-level lambda).
///
/// Keys use dotted notation for flat entries, e.g.
/// `services.nginx.enable = true` produces key `"services.nginx.enable"`.
///
/// Returns an empty iterator when the root expression is not an attribute set.
pub fn iterate_attr_set(nix_file: &NixFile) -> Vec<(String, ParsedNode)> {
    let Some(root_expr) = nix_file.root.expr() else {
        return Vec::new();
    };
    let Some(attrset) = unwrap_to_attrset(&root_expr) else {
        return Vec::new();
    };

    attrset
        .attrpath_values()
        .filter_map(|entry| {
            let key = collect_attr_names(&entry.attrpath()?).join(".");
            let value_node = ParsedNode::new(entry.value()?.syntax().clone());
            Some((key, value_node))
        })
        .collect()
}

// Traversal helpers

/// Peel off top-level lambdas until we reach an attribute set.
///
/// NixOS configuration files are commonly written as
/// `{ config, pkgs, ... }: { … }`.  We need to look through the lambda
/// to reach the returned attribute set.
fn unwrap_to_attrset(expr: &ast::Expr) -> Option<ast::AttrSet> {
    match expr {
        ast::Expr::AttrSet(attrset) => Some(attrset.clone()),
        ast::Expr::Lambda(lambda) => {
            let body = lambda.body()?;
            unwrap_to_attrset(&body)
        }
        // Parenthesised expressions: `( { … } )`
        ast::Expr::Paren(paren) => {
            let inner = paren.expr()?;
            unwrap_to_attrset(&inner)
        }
        _ => None,
    }
}

/// Recursively search `attrset` for a value at `remaining` path segments.
///
/// Algorithm:
/// 1. For each `AttrpathValue` in the set:
///    a. **Exact match** — the attrpath segments equal `remaining` exactly →
///       return the value node.
///    b. **Prefix match** — the attrpath is a prefix of `remaining` and the
///       value is itself an attribute set → recurse with the tail.
fn find_in_attrset(attrset: &ast::AttrSet, remaining: &[&str]) -> Option<ParsedNode> {
    for entry in attrset.attrpath_values() {
        let attrpath = match entry.attrpath() {
            Some(ap) => ap,
            None => continue,
        };
        let value = match entry.value() {
            Some(v) => v,
            None => continue,
        };

        let entry_keys = collect_attr_names(&attrpath);
        if entry_keys.is_empty() {
            continue;
        }

        // Case 1: exact match — return the value node.
        if keys_eq(&entry_keys, remaining) {
            return Some(ParsedNode::new(value.syntax().clone()));
        }

        // Case 2: the entry's key is a proper prefix of the remaining path.
        // The value must be an attribute set we can recurse into.
        if remaining.len() > entry_keys.len() && keys_prefix(&entry_keys, remaining) {
            if let ast::Expr::AttrSet(nested) = &value {
                return find_in_attrset(nested, &remaining[entry_keys.len()..]);
            }
        }
    }
    None
}

// Attribute key helpers

/// Collect the string names of each segment in an [`ast::Attrpath`].
///
/// Returns an empty `Vec` when any segment is a dynamic expression (`${…}`)
/// since those cannot be matched statically.
fn collect_attr_names(attrpath: &ast::Attrpath) -> Vec<String> {
    attrpath
        .attrs()
        .filter_map(|attr| attr_to_string(&attr))
        .collect()
}

/// Convert a single [`ast::Attr`] to its string name, if statically known.
fn attr_to_string(attr: &ast::Attr) -> Option<String> {
    match attr {
        ast::Attr::Ident(ident) => ident.ident_token().map(|t| t.text().to_string()),
        ast::Attr::Str(s) => {
            // Only simple non-interpolated string keys can be matched.
            let parts = s.normalized_parts();
            let has_interpolation = parts
                .iter()
                .any(|p| matches!(p, ast::InterpolPart::Interpolation(_)));
            if has_interpolation {
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
        // Dynamic attribute keys `${expr}` cannot be matched statically.
        ast::Attr::Dynamic(_) => None,
    }
}

/// Return `true` when every element of `keys` equals the corresponding
/// element of `path_parts` (same length required).
fn keys_eq(keys: &[String], path_parts: &[&str]) -> bool {
    keys.len() == path_parts.len()
        && keys
            .iter()
            .zip(path_parts.iter())
            .all(|(k, p)| k.as_str() == *p)
}

/// Return `true` when `keys` is a proper prefix of `path_parts`.
fn keys_prefix(keys: &[String], path_parts: &[&str]) -> bool {
    keys.len() < path_parts.len()
        && keys
            .iter()
            .zip(path_parts.iter())
            .all(|(k, p)| k.as_str() == *p)
}

// Value extraction helpers

/// Extract a value from an [`ast::Ident`] node.
///
/// `true`, `false`, and `null` are identifiers in Nix (not keywords).
fn extract_ident_value(ident: &ast::Ident) -> NixValue {
    let text = ident
        .ident_token()
        .map(|t| t.text().to_string())
        .unwrap_or_default();
    match text.as_str() {
        "true" => NixValue::Bool(true),
        "false" => NixValue::Bool(false),
        "null" => NixValue::Null,
        _ => NixValue::Expression(text),
    }
}

/// Extract a value from an [`ast::Literal`] node (integer, float, or URI).
fn extract_literal_value(lit: &ast::Literal) -> NixValue {
    match lit.kind() {
        ast::LiteralKind::Integer(i) => {
            let text = i.syntax().text().to_string();
            match text.parse::<i64>() {
                Ok(n) => NixValue::Int(n),
                Err(_) => NixValue::Expression(text),
            }
        }
        // Floats have no dedicated NixValue variant — represent as Expression.
        ast::LiteralKind::Float(f) => NixValue::Expression(f.syntax().text().to_string()),
        // URI literals (`https://example.com`) behave like strings.
        ast::LiteralKind::Uri(u) => NixValue::String(u.syntax().text().to_string()),
    }
}

/// Extract a value from an [`ast::Str`] (Nix string) node.
///
/// Simple non-interpolated strings become `NixValue::String`.
/// Interpolated strings fall back to `NixValue::Expression`.
fn extract_string_value(s: &ast::Str) -> NixValue {
    let parts = s.normalized_parts();
    let mut result = String::new();
    for part in &parts {
        match part {
            ast::InterpolPart::Literal(text) => result.push_str(text),
            ast::InterpolPart::Interpolation(_) => {
                // Cannot simplify — return raw source text.
                return NixValue::Expression(s.syntax().to_string());
            }
        }
    }
    NixValue::String(result)
}

/// Flatten an [`ast::AttrSet`] into a `NixValue::AttrSet`.
///
/// Dotted key entries (`a.b = 1`) are represented with a dotted key string
/// (`"a.b"`).
fn extract_attrset_value(attrset: &ast::AttrSet) -> NixValue {
    let entries: Vec<(String, NixValue)> = attrset
        .attrpath_values()
        .filter_map(|entry| {
            let keys = collect_attr_names(&entry.attrpath()?);
            if keys.is_empty() {
                return None;
            }
            let key = keys.join(".");
            let val = expr_to_value(&entry.value()?);
            Some((key, val))
        })
        .collect();
    NixValue::AttrSet(entries)
}

// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nix_parser::reader::parse_string;

    // Helper: parse a string, panic on error.
    fn nix(src: &str) -> NixFile {
        parse_string(src).expect("test source should parse")
    }

    // find_option — nested attr sets

    #[test]
    fn find_simple_bool() {
        let f = nix("{ enable = true; }");
        let node = find_option(&f, "enable").expect("should find enable");
        assert_eq!(node.to_nix_value(), NixValue::Bool(true));
    }

    #[test]
    fn find_nested_attr_set() {
        let src = "{ services = { nginx = { enable = true; }; }; }";
        let f = nix(src);
        let node = find_option(&f, "services.nginx.enable").expect("should find path");
        assert_eq!(node.to_nix_value(), NixValue::Bool(true));
    }

    #[test]
    fn find_flat_dotted_key() {
        let src = "{ services.nginx.enable = true; }";
        let f = nix(src);
        let node = find_option(&f, "services.nginx.enable").expect("should find path");
        assert_eq!(node.to_nix_value(), NixValue::Bool(true));
    }

    #[test]
    fn find_through_lambda() {
        let src = "{ config, pkgs, ... }:\n{ services.nginx.enable = true; }";
        let f = nix(src);
        let node = find_option(&f, "services.nginx.enable").expect("should find path");
        assert_eq!(node.to_nix_value(), NixValue::Bool(true));
    }

    #[test]
    fn find_missing_key_returns_none() {
        let f = nix("{ enable = true; }");
        assert!(find_option(&f, "nonexistent").is_none());
    }

    #[test]
    fn find_empty_path_returns_none() {
        let f = nix("{ enable = true; }");
        assert!(find_option(&f, "").is_none());
    }

    // NixValue extraction

    #[test]
    fn value_bool_false() {
        let f = nix("{ enable = false; }");
        let node = find_option(&f, "enable").unwrap();
        assert_eq!(node.to_nix_value(), NixValue::Bool(false));
    }

    #[test]
    fn value_null() {
        let f = nix("{ x = null; }");
        let node = find_option(&f, "x").unwrap();
        assert_eq!(node.to_nix_value(), NixValue::Null);
    }

    #[test]
    fn value_integer() {
        let f = nix("{ port = 8080; }");
        let node = find_option(&f, "port").unwrap();
        assert_eq!(node.to_nix_value(), NixValue::Int(8080));
    }

    #[test]
    fn value_string() {
        let f = nix(r#"{ hostname = "myhost"; }"#);
        let node = find_option(&f, "hostname").unwrap();
        assert_eq!(node.to_nix_value(), NixValue::String("myhost".into()));
    }

    #[test]
    fn value_list() {
        let f = nix("{ items = [ 1 2 3 ]; }");
        let node = find_option(&f, "items").unwrap();
        assert_eq!(
            node.to_nix_value(),
            NixValue::List(vec![
                NixValue::Int(1),
                NixValue::Int(2),
                NixValue::Int(3),
            ])
        );
    }

    #[test]
    fn value_nested_attrset_as_value() {
        let src = "{ services = { nginx = { enable = true; }; }; }";
        let f = nix(src);
        let node = find_option(&f, "services").unwrap();
        // The value of "services" is an AttrSet whose single entry is
        // key "nginx" → another nested AttrSet.
        match node.to_nix_value() {
            NixValue::AttrSet(entries) => {
                assert_eq!(entries.len(), 1);
                let (key, val) = entries.into_iter().next().unwrap();
                assert_eq!(key, "nginx");
                match val {
                    NixValue::AttrSet(inner) => {
                        assert_eq!(inner.len(), 1);
                        assert_eq!(inner[0].0, "enable");
                        assert_eq!(inner[0].1, NixValue::Bool(true));
                    }
                    other => panic!("expected nested AttrSet, got {:?}", other),
                }
            }
            other => panic!("expected AttrSet, got {:?}", other),
        }
    }

    // iterate_attr_set

    #[test]
    fn iterate_returns_all_keys() {
        let f = nix("{ a = 1; b = 2; c = 3; }");
        let pairs = iterate_attr_set(&f);
        let keys: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["a", "b", "c"]);
    }

    #[test]
    fn parsed_node_text_round_trips() {
        let f = nix("{ enable = true; }");
        let node = find_option(&f, "enable").unwrap();
        assert_eq!(node.text(), "true");
    }
}
