//! Formatting utilities for the Nix writer.
//!
//! Provides:
//! * [`detect_indent`] — infer the indentation unit from a source file.
//! * [`value_to_nix`] — serialize a [`NixValue`] to valid Nix syntax.

use crate::nix_parser::types::NixValue;

// Indentation detection

/// Detect the indentation unit used in `source`.
///
/// Examines all non-empty, indented lines and computes the GCD of their
/// leading-whitespace counts.  Returns `"\t"` if any line starts with a tab,
/// otherwise `" ".repeat(unit)` (defaulting to two spaces when no indented
/// lines are found).
///
/// # Examples
///
/// ```
/// # use nixman_core::nix_parser::format::detect_indent;
/// let src = "{\n  enable = true;\n}";
/// assert_eq!(detect_indent(src), "  ");
///
/// let src4 = "{\n    enable = true;\n}";
/// assert_eq!(detect_indent(src4), "    ");
///
/// let src_tab = "{\n\tenable = true;\n}";
/// assert_eq!(detect_indent(src_tab), "\t");
/// ```
pub fn detect_indent(source: &str) -> String {
    // If any non-empty line starts with a tab, call it tab-indented.
    let has_tabs = source
        .lines()
        .any(|l| !l.trim().is_empty() && l.starts_with('\t'));

    if has_tabs {
        return "\t".to_string();
    }

    // Collect leading-space counts from all non-empty, space-indented lines.
    let indents: Vec<usize> = source
        .lines()
        .filter(|l| !l.trim().is_empty() && l.starts_with(' '))
        .map(|l| l.len() - l.trim_start_matches(' ').len())
        .filter(|&n| n > 0)
        .collect();

    if indents.is_empty() {
        return "  ".to_string(); // default: two spaces
    }

    // The GCD of all indentation levels is the atomic indentation unit.
    let unit = indents.iter().copied().fold(0usize, gcd);

    if unit <= 1 {
        // GCD of 1 means inconsistent or 1-space indentation.  Fall back to
        // the minimum observed indent clamped to at least 2.
        let min = *indents.iter().min().unwrap_or(&2);
        return " ".repeat(min.max(2));
    }

    " ".repeat(unit)
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

// Value serialization

/// Serialize a [`NixValue`] to valid Nix source syntax.
///
/// The output can be pasted directly into a Nix file as a value expression.
///
/// # Examples
///
/// ```
/// # use nixman_core::nix_parser::{NixValue, format::value_to_nix};
/// assert_eq!(value_to_nix(&NixValue::Bool(true)), "true");
/// assert_eq!(value_to_nix(&NixValue::Int(8080)), "8080");
/// assert_eq!(value_to_nix(&NixValue::String("hi".into())), r#""hi""#);
/// assert_eq!(value_to_nix(&NixValue::Null), "null");
/// ```
pub fn value_to_nix(value: &NixValue) -> String {
    match value {
        NixValue::Bool(true) => "true".to_string(),
        NixValue::Bool(false) => "false".to_string(),

        NixValue::String(s) => {
            // Escape backslash, double-quote, and common control characters.
            let escaped = s
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\r', "\\r")
                .replace('\t', "\\t");
            format!("\"{}\"", escaped)
        }

        NixValue::Int(n) => n.to_string(),

        NixValue::Null => "null".to_string(),

        // Paths are emitted verbatim (they are already valid Nix path literals).
        NixValue::Path(p) => p.clone(),

        NixValue::List(items) => {
            if items.is_empty() {
                "[ ]".to_string()
            } else {
                let parts: Vec<String> = items.iter().map(value_to_nix).collect();
                format!("[ {} ]", parts.join(" "))
            }
        }

        NixValue::AttrSet(entries) => {
            if entries.is_empty() {
                "{ }".to_string()
            } else {
                let parts: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| format!("{} = {};", k, value_to_nix(v)))
                    .collect();
                // Use double-brace escaping in format! for literal braces.
                format!("{{ {} }}", parts.join(" "))
            }
        }

        // Complex expressions are emitted verbatim.
        NixValue::Expression(e) => e.clone(),
    }
}

// Tests

#[cfg(test)]
mod tests {
    use super::*;

    // detect_indent

    #[test]
    fn detect_two_spaces() {
        let src = "{\n  enable = true;\n  port = 8080;\n}";
        assert_eq!(detect_indent(src), "  ");
    }

    #[test]
    fn detect_four_spaces() {
        let src = "{\n    enable = true;\n    port = 8080;\n}";
        assert_eq!(detect_indent(src), "    ");
    }

    #[test]
    fn detect_tabs() {
        let src = "{\n\tenable = true;\n\tport = 8080;\n}";
        assert_eq!(detect_indent(src), "\t");
    }

    #[test]
    fn detect_defaults_to_two_spaces_for_no_indent() {
        assert_eq!(detect_indent("{ enable = true; }"), "  ");
    }

    #[test]
    fn detect_gcd_of_mixed_levels() {
        // Lines indented 2, 4, 6 → GCD = 2.
        let src = "{\n  a = 1;\n    b = 2;\n      c = 3;\n}";
        assert_eq!(detect_indent(src), "  ");
    }

    // value_to_nix

    #[test]
    fn serialize_bool_true() {
        assert_eq!(value_to_nix(&NixValue::Bool(true)), "true");
    }

    #[test]
    fn serialize_bool_false() {
        assert_eq!(value_to_nix(&NixValue::Bool(false)), "false");
    }

    #[test]
    fn serialize_integer() {
        assert_eq!(value_to_nix(&NixValue::Int(42)), "42");
        assert_eq!(value_to_nix(&NixValue::Int(-7)), "-7");
    }

    #[test]
    fn serialize_string_simple() {
        assert_eq!(
            value_to_nix(&NixValue::String("hello".into())),
            r#""hello""#
        );
    }

    #[test]
    fn serialize_string_escapes() {
        assert_eq!(
            value_to_nix(&NixValue::String("say \"hi\"".into())),
            r#""say \"hi\"""#
        );
        assert_eq!(
            value_to_nix(&NixValue::String("back\\slash".into())),
            r#""back\\slash""#
        );
    }

    #[test]
    fn serialize_null() {
        assert_eq!(value_to_nix(&NixValue::Null), "null");
    }

    #[test]
    fn serialize_path() {
        assert_eq!(
            value_to_nix(&NixValue::Path("/etc/nixos".into())),
            "/etc/nixos"
        );
    }

    #[test]
    fn serialize_empty_list() {
        assert_eq!(value_to_nix(&NixValue::List(vec![])), "[ ]");
    }

    #[test]
    fn serialize_list() {
        let v = NixValue::List(vec![
            NixValue::Int(1),
            NixValue::Int(2),
            NixValue::Int(3),
        ]);
        assert_eq!(value_to_nix(&v), "[ 1 2 3 ]");
    }

    #[test]
    fn serialize_empty_attrset() {
        assert_eq!(value_to_nix(&NixValue::AttrSet(vec![])), "{ }");
    }

    #[test]
    fn serialize_attrset() {
        let v = NixValue::AttrSet(vec![
            ("enable".into(), NixValue::Bool(true)),
            ("port".into(), NixValue::Int(80)),
        ]);
        assert_eq!(value_to_nix(&v), "{ enable = true; port = 80; }");
    }

    #[test]
    fn serialize_expression_passthrough() {
        let expr = "pkgs.lib.mkIf condition value".to_string();
        assert_eq!(value_to_nix(&NixValue::Expression(expr.clone())), expr);
    }

    #[test]
    fn serialize_nested_list() {
        let v = NixValue::List(vec![
            NixValue::String("a".into()),
            NixValue::String("b".into()),
        ]);
        assert_eq!(value_to_nix(&v), r#"[ "a" "b" ]"#);
    }
}
