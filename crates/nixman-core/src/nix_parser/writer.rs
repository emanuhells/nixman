//! AST modification and source serialization.
//!
//! Works directly on the source string using byte-range replacement so that
//! all formatting, comments, and structure outside the modified node are
//! preserved exactly ("round-trip fidelity").
//!
//! # Operations
//!
//! * [`set_value`] — replace the value of a node at a known [`TextRange`].
//! * [`remove_option`] — remove an option assignment by its dotted path.

use rnix::{ast, SyntaxKind, TextRange};
use rnix::ast::{HasEntry};
use rowan::ast::AstNode;

use crate::nix_parser::format::value_to_nix;
use crate::nix_parser::reader::parse_string;
use crate::nix_parser::types::NixValue;

// WriteError

/// Errors that can occur during an AST write operation.
#[derive(Debug)]
pub enum WriteError {
    /// The supplied [`TextRange`] is out of bounds or misaligned.
    InvalidRange,
    /// The [`NixValue`] could not be serialized to Nix syntax.
    SerializationFailed(String),
    /// The modified source is not valid Nix.
    ValidationFailed(String),
    /// No suitable insertion / removal point was found in the AST.
    InsertionPointNotFound,
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::InvalidRange => write!(f, "invalid text range"),
            WriteError::SerializationFailed(msg) => {
                write!(f, "serialization failed: {}", msg)
            }
            WriteError::ValidationFailed(msg) => {
                write!(f, "validation failed: {}", msg)
            }
            WriteError::InsertionPointNotFound => {
                write!(f, "no suitable insertion or removal point found")
            }
        }
    }
}

impl std::error::Error for WriteError {}


/// Replace the node at `node_range` in `source` with the serialized form of
/// `new_value`.
///
/// The function touches only the bytes covered by `node_range`; everything
/// else in `source` is preserved byte-for-byte.  After the replacement the
/// result is re-parsed to verify it is still valid Nix.
///
/// # Errors
///
/// * [`WriteError::InvalidRange`] — `node_range` is out of bounds or
///   misaligned with UTF-8 boundaries.
/// * [`WriteError::ValidationFailed`] — the resulting source fails to parse.
pub fn set_value(
    source: &str,
    node_range: TextRange,
    new_value: &NixValue,
) -> Result<String, WriteError> {
    let start = u32::from(node_range.start()) as usize;
    let end = u32::from(node_range.end()) as usize;

    if end > source.len() || start > end {
        return Err(WriteError::InvalidRange);
    }
    if !source.is_char_boundary(start) || !source.is_char_boundary(end) {
        return Err(WriteError::InvalidRange);
    }

    let new_text = value_to_nix(new_value);

    let mut result = String::with_capacity(source.len() + new_text.len());
    result.push_str(&source[..start]);
    result.push_str(&new_text);
    result.push_str(&source[end..]);

    // Re-parse to ensure the replacement did not break syntax.
    parse_string(&result).map_err(|e| WriteError::ValidationFailed(e.to_string()))?;

    Ok(result)
}


/// Remove the option at `option_path` from `source`.
///
/// The entire `key = value;` binding is deleted, including its leading
/// indentation and trailing newline.  If removing the binding leaves its
/// parent attribute set empty *and* that attribute set is itself a nested
/// value inside another binding, the parent binding is also removed
/// (cascaded upward until a non-empty set is found).
///
/// # Errors
///
/// * [`WriteError::InsertionPointNotFound`] — the path does not exist in the
///   source.
/// * [`WriteError::ValidationFailed`] — the result fails to re-parse.
pub fn remove_option(source: &str, option_path: &str) -> Result<String, WriteError> {
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

    // Locate the innermost AttrpathValue for the full path.
    let innermost_av =
        find_attrpath_value(&root_attrset, &path_parts).ok_or(WriteError::InsertionPointNotFound)?;

    // Walk upward: if the AttrpathValue is the sole entry in its parent
    // AttrSet and that AttrSet is itself a nested value, bubble up so we
    // remove the now-empty parent binding too.
    let target_av = find_removable_node(innermost_av);

    let (rm_start, rm_end) = compute_removal_range(source, &target_av);

    let result = format!("{}{}", &source[..rm_start], &source[rm_end..]);

    parse_string(&result).map_err(|e| WriteError::ValidationFailed(e.to_string()))?;

    Ok(result)
}

// Internal traversal helpers

/// Peel top-level lambdas and parentheses to reach the underlying AttrSet.
pub(crate) fn peel_to_attrset(expr: &ast::Expr) -> Option<ast::AttrSet> {
    match expr {
        ast::Expr::AttrSet(a) => Some(a.clone()),
        ast::Expr::Lambda(l) => peel_to_attrset(&l.body()?),
        ast::Expr::Paren(p) => peel_to_attrset(&p.expr()?),
        _ => None,
    }
}

/// Recursively find the `SyntaxNode` of the innermost `AttrpathValue` that
/// corresponds to `path_parts` inside `attrset`.
fn find_attrpath_value(attrset: &ast::AttrSet, path_parts: &[&str]) -> Option<rnix::SyntaxNode> {
    for entry in attrset.attrpath_values() {
        let attrpath = entry.attrpath()?;
        let value = entry.value()?;
        let entry_keys = collect_attr_names(&attrpath);

        if entry_keys.is_empty() {
            continue;
        }

        // Exact match: this AttrpathValue covers the full remaining path.
        if keys_eq(&entry_keys, path_parts) {
            return Some(entry.syntax().clone());
        }

        // Prefix match: the entry key is a strict prefix of the remaining
        // path and the value is a nested AttrSet we can recurse into.
        if path_parts.len() > entry_keys.len() && keys_prefix(&entry_keys, path_parts) {
            if let ast::Expr::AttrSet(nested) = &value {
                return find_attrpath_value(&nested, &path_parts[entry_keys.len()..]);
            }
        }
    }
    None
}

/// Walk upward from `av` (an `AttrpathValue` node): if the node is the only
/// entry in its parent `AttrSet` and that `AttrSet` is itself nested inside
/// another `AttrpathValue`, return the parent `AttrpathValue` instead
/// (repeating until we reach a non-trivial parent or the root).
fn find_removable_node(av: rnix::SyntaxNode) -> rnix::SyntaxNode {
    let mut current = av;

    loop {
        // Parent of an AttrpathValue is the containing AttrSet.
        let parent_attrset = match current.parent() {
            Some(p) if p.kind() == SyntaxKind::NODE_ATTR_SET => p,
            _ => break,
        };

        // Count how many AttrpathValue children the parent AttrSet has.
        let entry_count = parent_attrset
            .children()
            .filter(|n| n.kind() == SyntaxKind::NODE_ATTRPATH_VALUE)
            .count();

        if entry_count != 1 {
            // The parent has multiple entries; only remove the current one.
            break;
        }

        // The parent AttrSet has exactly one entry.  Check whether that
        // AttrSet is itself the value of a higher-level AttrpathValue.
        let grandparent = match parent_attrset.parent() {
            Some(gp) if gp.kind() == SyntaxKind::NODE_ATTRPATH_VALUE => gp,
            _ => break, // parent is Root, Lambda, etc. — stop here
        };

        // Bubble up to the grandparent AttrpathValue.
        current = grandparent;
    }

    current
}

// Byte-range helpers

/// Compute the byte range `(start, end)` to delete from `source` in order to
/// remove the `AttrpathValue` represented by `av_syntax`.
///
/// The range includes:
/// - The leading whitespace / indentation on the same line (if it is
///   entirely whitespace up to the start of the node).
/// - The `AttrpathValue` itself.
/// - The trailing `;`.
/// - The following newline (if the remainder of the line is only whitespace).
fn compute_removal_range(source: &str, av_syntax: &rnix::SyntaxNode) -> (usize, usize) {
    let av_range = av_syntax.text_range();
    let av_start = u32::from(av_range.start()) as usize;
    let av_end = u32::from(av_range.end()) as usize;

    // Scan forward past optional whitespace to find and consume the `;`.
    let semi_end = {
        let tail = &source[av_end..];
        let mut end = av_end;
        for (i, ch) in tail.char_indices() {
            match ch {
                ';' => {
                    end = av_end + i + 1;
                    break;
                }
                ' ' | '\t' => continue,
                _ => break,
            }
        }
        end
    };

    // Determine the start of the removal region: if everything between the
    // previous newline and `av_start` is pure whitespace, include that
    // leading indentation.
    let rm_start = {
        let prefix = &source[..av_start];
        match prefix.rfind('\n') {
            Some(nl_pos) => {
                let indent = &prefix[nl_pos + 1..];
                if indent.chars().all(|c| c == ' ' || c == '\t') {
                    nl_pos + 1
                } else {
                    av_start
                }
            }
            None => {
                if prefix.chars().all(|c| c == ' ' || c == '\t') {
                    0
                } else {
                    av_start
                }
            }
        }
    };

    // Determine the end of the removal region: if everything between
    // `semi_end` and the next newline is pure whitespace, consume the
    // newline too.
    let rm_end = {
        let tail = &source[semi_end..];
        let mut end = semi_end;
        for (i, ch) in tail.char_indices() {
            match ch {
                '\n' => {
                    end = semi_end + i + 1;
                    break;
                }
                '\r' => continue, // will be consumed with the following \n
                ' ' | '\t' => continue,
                _ => {
                    // Non-whitespace before newline: don't extend past semi_end.
                    end = semi_end;
                    break;
                }
            }
        }
        // If we hit the end of the file, consume everything.
        if end == semi_end && tail.chars().all(|c| c == ' ' || c == '\t' || c == '\r') {
            semi_end + tail.len()
        } else {
            end
        }
    };

    (rm_start, rm_end)
}

// Attribute-key helpers (duplicated from traversal for module-local use)

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

fn keys_eq(keys: &[String], parts: &[&str]) -> bool {
    keys.len() == parts.len()
        && keys
            .iter()
            .zip(parts.iter())
            .all(|(k, p)| k.as_str() == *p)
}

fn keys_prefix(keys: &[String], parts: &[&str]) -> bool {
    keys.len() < parts.len()
        && keys
            .iter()
            .zip(parts.iter())
            .all(|(k, p)| k.as_str() == *p)
}

// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nix_parser::{find_option, parse_string};

    fn source_range(source: &str, path: &str) -> TextRange {
        let nix = parse_string(source).unwrap();
        let node = find_option(&nix, path).unwrap();
        node.syntax.text_range()
    }


    #[test]
    fn set_value_replaces_bool() {
        let src = "{ enable = true; }";
        let range = source_range(src, "enable");
        let result = set_value(src, range, &NixValue::Bool(false)).unwrap();
        assert_eq!(result, "{ enable = false; }");
    }

    #[test]
    fn set_value_replaces_integer() {
        let src = "{ port = 8080; }";
        let range = source_range(src, "port");
        let result = set_value(src, range, &NixValue::Int(443)).unwrap();
        assert_eq!(result, "{ port = 443; }");
    }

    #[test]
    fn set_value_replaces_string() {
        let src = r#"{ hostname = "old"; }"#;
        let range = source_range(src, "hostname");
        let result = set_value(src, range, &NixValue::String("new".into())).unwrap();
        assert_eq!(result, r#"{ hostname = "new"; }"#);
    }

    #[test]
    fn set_value_preserves_surrounding_text() {
        let src = "# header\n{ enable = true; # inline\n}";
        let range = source_range(src, "enable");
        let result = set_value(src, range, &NixValue::Bool(false)).unwrap();
        assert_eq!(result, "# header\n{ enable = false; # inline\n}");
    }

    #[test]
    fn set_value_invalid_range_out_of_bounds() {
        let src = "{ enable = true; }";
        let range = TextRange::new(100u32.into(), 110u32.into());
        assert!(matches!(
            set_value(src, range, &NixValue::Bool(false)),
            Err(WriteError::InvalidRange)
        ));
    }

    #[test]
    fn set_value_invalid_range_start_equals_end_but_out_of_bounds() {
        let src = "{ enable = true; }";
        // A zero-length range entirely out of bounds.
        let range = TextRange::new(200u32.into(), 200u32.into());
        assert!(matches!(
            set_value(src, range, &NixValue::Bool(false)),
            Err(WriteError::InvalidRange)
        ));
    }

    // round-trip

    #[test]
    fn round_trip_identical() {
        // Parsing and immediately writing back must produce the same bytes.
        // This is a property of set_value (we only touch the target bytes).
        let src = "{ config, pkgs, ... }:\n{\n  services.nginx.enable = true;\n  boot.loader.grub.enable = false;\n}";
        let _nix = parse_string(src).unwrap();
        // Replace a value with its current value — output must equal input.
        let range = source_range(src, "services.nginx.enable");
        let result = set_value(src, range, &NixValue::Bool(true)).unwrap();
        assert_eq!(result, src);
    }


    #[test]
    fn remove_option_simple() {
        let src = "{\n  enable = true;\n  port = 8080;\n}";
        let result = remove_option(src, "enable").unwrap();
        // The enable line is gone; port remains.
        assert!(!result.contains("enable"));
        assert!(result.contains("port = 8080"));
        parse_string(&result).expect("result should parse");
    }

    #[test]
    fn remove_option_last_entry() {
        let src = "{\n  enable = true;\n}";
        let result = remove_option(src, "enable").unwrap();
        assert!(!result.contains("enable"));
        parse_string(&result).expect("result should parse");
    }

    #[test]
    fn remove_option_flat_dotted_key() {
        let src = "{\n  services.nginx.enable = true;\n  services.nginx.port = 8080;\n}";
        let result = remove_option(src, "services.nginx.enable").unwrap();
        assert!(!result.contains("services.nginx.enable"));
        assert!(result.contains("services.nginx.port"));
        parse_string(&result).expect("result should parse");
    }

    #[test]
    fn remove_option_missing_path_returns_error() {
        let src = "{ enable = true; }";
        assert!(matches!(
            remove_option(src, "nonexistent"),
            Err(WriteError::InsertionPointNotFound)
        ));
    }

    #[test]
    fn remove_option_cascade_empty_parent() {
        // Removing the only entry inside a nested AttrSet should also remove
        // the now-empty parent binding.
        let src = "{\n  services = {\n    enable = true;\n  };\n}";
        let result = remove_option(src, "services.enable").unwrap();
        assert!(!result.contains("services"));
        assert!(!result.contains("enable"));
        parse_string(&result).expect("result should parse");
    }

    #[test]
    fn remove_option_no_cascade_when_sibling_exists() {
        let src = "{\n  services = {\n    enable = true;\n    port = 80;\n  };\n}";
        let result = remove_option(src, "services.enable").unwrap();
        // Parent `services` block should survive (it still has `port`).
        assert!(result.contains("services"));
        assert!(result.contains("port"));
        assert!(!result.contains("enable"));
        parse_string(&result).expect("result should parse");
    }
}
