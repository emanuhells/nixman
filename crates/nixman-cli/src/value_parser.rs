//! Shared Nix value parsing for CLI inputs.

use nixman_core::nix_parser::NixValue;

/// Parse a CLI string into a `NixValue`.
///
/// Heuristics (in order):
/// - `"true"` / `"false"` → `Bool`
/// - `"null"` → `Null`
/// - All-digit string (optionally leading `-`) → `Int`
/// - Starts with `[` or `{` → parse as Nix expression via rnix
/// - Starts and ends with `"` → `String` (strip quotes, unescape basics)
/// - Contains `.`, space, `(`, or `$` → `Expression` (raw Nix code)
/// - Everything else → `String`
pub fn parse_nix_value(s: &str) -> NixValue {
    match s {
        "true" => NixValue::Bool(true),
        "false" => NixValue::Bool(false),
        "null" => NixValue::Null,
        _ => {
            if let Ok(n) = s.parse::<i64>() {
                return NixValue::Int(n);
            }

            // Try to parse as a Nix expression (for lists, attrsets, etc.)
            if s.starts_with('[') || s.starts_with('{') {
                if let Some(value) = try_parse_nix_expr(s) {
                    return value;
                }
            }

            // Quoted string: strip quotes and unescape basic sequences.
            if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
                let inner = &s[1..s.len() - 1];
                let unescaped = inner.replace("\\\"", "\"").replace("\\\\", "\\");
                return NixValue::String(unescaped);
            }

            // If it looks like a Nix expression, keep it as-is.
            if s.contains('.') || s.contains(' ') || s.contains('(') || s.contains('$') {
                NixValue::Expression(s.to_string())
            } else {
                NixValue::String(s.to_string())
            }
        }
    }
}

/// Try to parse `s` as a Nix expression and convert to `NixValue`.
fn try_parse_nix_expr(s: &str) -> Option<NixValue> {
    let nix_file = nixman_core::nix_parser::reader::parse_string(s).ok()?;
    let expr = nix_file.root.expr()?;
    Some(nixman_core::nix_parser::expr_to_value(&expr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bool() {
        assert_eq!(parse_nix_value("true"), NixValue::Bool(true));
        assert_eq!(parse_nix_value("false"), NixValue::Bool(false));
    }

    #[test]
    fn parse_null() {
        assert_eq!(parse_nix_value("null"), NixValue::Null);
    }

    #[test]
    fn parse_int() {
        assert_eq!(parse_nix_value("42"), NixValue::Int(42));
        assert_eq!(parse_nix_value("-7"), NixValue::Int(-7));
    }

    #[test]
    fn parse_list_of_strings() {
        let v = parse_nix_value(r#"[ "nfs" ]"#);
        match v {
            NixValue::List(items) => {
                assert_eq!(items.len(), 1);
                assert!(matches!(&items[0], NixValue::String(s) if s == "nfs"));
            }
            _ => panic!("expected list, got {:?}", v),
        }
    }

    #[test]
    fn parse_list_multiple() {
        let v = parse_nix_value(r#"[ "wheel" "docker" ]"#);
        match v {
            NixValue::List(items) => assert_eq!(items.len(), 2),
            _ => panic!("expected list"),
        }
    }

    #[test]
    fn parse_expression() {
        let v = parse_nix_value("pkgs.linuxPackages_latest");
        assert!(matches!(v, NixValue::Expression(_)));
    }

    #[test]
    fn parse_simple_string() {
        // Simple word without dots/spaces = string
        assert_eq!(parse_nix_value("hello"), NixValue::String("hello".into()));
    }

    #[test]
    fn parse_quoted_string() {
        assert_eq!(
            parse_nix_value(r#""hello world""#),
            NixValue::String("hello world".into())
        );
    }
}
