//! Integration-style unit tests for the nix_parser module.
//!
//! These tests exercise the public API using the fixture files in
//! `tests/fixtures/` and cover parsing, traversal, writing, insertion,
//! removal, comment preservation, and indentation detection.

use crate::nix_parser::{
    find_option,
    format::detect_indent,
    insert::add_option,
    parse_string,
    writer::{remove_option, set_value},
    NixValue,
};

// Fixture sources embedded at compile time

const MINIMAL_NIX: &str = include_str!("../../tests/fixtures/minimal.nix");
const COMPLEX_NIX: &str = include_str!("../../tests/fixtures/complex.nix");
const WITH_COMMENTS_NIX: &str = include_str!("../../tests/fixtures/with-comments.nix");

// Parsing

/// Parse `minimal.nix` and verify it succeeds and source is preserved.
#[test]
fn test_parse_simple_file() {
    let nix = parse_string(MINIMAL_NIX).expect("minimal.nix should parse cleanly");
    // Source is stored verbatim.
    assert_eq!(nix.source, MINIMAL_NIX);
    // Root expression should exist.
    assert!(nix.root.expr().is_some());
}

/// Parse `complex.nix` — nested attribute sets, string-keyed virtual hosts.
#[test]
fn test_parse_complex_file() {
    let nix = parse_string(COMPLEX_NIX).expect("complex.nix should parse cleanly");
    assert_eq!(nix.source, COMPLEX_NIX);
    assert!(nix.root.expr().is_some());
}

/// Invalid Nix source should produce a SyntaxError, not a panic.
#[test]
fn test_parse_invalid_returns_error() {
    let bad = "{ foo = ; }"; // missing value
    parse_string(bad).expect_err("invalid source should fail to parse");
}

// Finding options

/// Find `services.nginx.enable` in `complex.nix`, which uses a *nested*
/// attribute-set layout (`services.nginx = { enable = …; }`).
#[test]
fn test_find_option_nested() {
    let nix = parse_string(COMPLEX_NIX).expect("should parse");
    let node = find_option(&nix, "services.nginx.enable")
        .expect("services.nginx.enable should be found in complex.nix");
    assert_eq!(node.to_nix_value(), NixValue::Bool(true));
}

/// Find `networking.hostname` in `minimal.nix`, which uses a *flat dotted*
/// key notation (`networking.hostname = "testbox";`).
#[test]
fn test_find_option_dotted() {
    let nix = parse_string(MINIMAL_NIX).expect("should parse");
    let node = find_option(&nix, "networking.hostname")
        .expect("networking.hostname should be found in minimal.nix");
    assert_eq!(node.to_nix_value(), NixValue::String("testbox".into()));
}

/// Querying a path that does not exist should return `None`.
#[test]
fn test_find_option_not_found() {
    let nix = parse_string(MINIMAL_NIX).expect("should parse");
    let result = find_option(&nix, "services.postgresql.enable");
    assert!(result.is_none(), "non-existent option should return None");
}

/// An empty path should return `None`.
#[test]
fn test_find_option_empty_path() {
    let nix = parse_string(MINIMAL_NIX).expect("should parse");
    assert!(find_option(&nix, "").is_none());
}

/// Complex nested option: `networking.firewall.allowedTCPPorts` is a list.
#[test]
fn test_find_option_deep_nested() {
    let nix = parse_string(COMPLEX_NIX).expect("should parse");
    let node = find_option(&nix, "networking.firewall.allowedTCPPorts")
        .expect("networking.firewall.allowedTCPPorts should be found");
    match node.to_nix_value() {
        NixValue::List(items) => {
            assert_eq!(items.len(), 3);
            assert_eq!(items[0], NixValue::Int(22));
            assert_eq!(items[1], NixValue::Int(80));
            assert_eq!(items[2], NixValue::Int(443));
        }
        other => panic!("expected List, got {:?}", other),
    }
}

// Round-trip preservation

/// Parsing a source and accessing its `.source` field must produce the
/// identical bytes as the input (the parser never mutates the source).
#[test]
fn test_round_trip_preservation() {
    let nix = parse_string(MINIMAL_NIX).expect("should parse");
    // The source field is stored verbatim.
    assert_eq!(nix.source, MINIMAL_NIX);

    // Replacing a value with its current value must produce identical bytes.
    let node = find_option(&nix, "services.openssh.enable").expect("should find");
    let range = node.text_range();
    let result = set_value(MINIMAL_NIX, range, &NixValue::Bool(true))
        .expect("set_value should succeed");
    assert_eq!(
        result, MINIMAL_NIX,
        "replacing a value with itself should produce identical bytes"
    );
}

// set_value (writer)

/// Replace a boolean value in `minimal.nix` and verify the output is valid.
#[test]
fn test_set_value_boolean() {
    let nix = parse_string(MINIMAL_NIX).expect("should parse");
    let node = find_option(&nix, "services.openssh.enable").expect("should find");
    let range = node.text_range();

    let result = set_value(MINIMAL_NIX, range, &NixValue::Bool(false))
        .expect("set_value should succeed");

    assert!(result.contains("services.openssh.enable = false;"));
    // All other content must be unchanged.
    assert!(result.contains(r#"networking.hostname = "testbox""#));
    assert!(result.contains("environment.systemPackages"));
    // Must still parse.
    parse_string(&result).expect("modified source should still be valid Nix");
}

/// Replace a string value and verify the output is valid.
#[test]
fn test_set_value_string() {
    let nix = parse_string(MINIMAL_NIX).expect("should parse");
    let node = find_option(&nix, "networking.hostname").expect("should find");
    let range = node.text_range();

    let result = set_value(MINIMAL_NIX, range, &NixValue::String("newhost".into()))
        .expect("set_value should succeed");

    assert!(result.contains(r#"networking.hostname = "newhost""#));
    assert!(result.contains("services.openssh.enable = true;"));
    parse_string(&result).expect("modified source should still be valid Nix");
}

// add_option (insert)

/// Insert a new option that does not yet exist in the file.
#[test]
fn test_insert_new_option() {
    let result = add_option(
        MINIMAL_NIX,
        "services.nginx.enable",
        &NixValue::Bool(true),
        "  ",
    )
    .expect("add_option should succeed");

    assert!(result.contains("services.nginx.enable = true;"));
    // Existing options must survive.
    assert!(result.contains("services.openssh.enable = true;"));
    assert!(result.contains(r#"networking.hostname = "testbox""#));
    parse_string(&result).expect("result should be valid Nix");
}

/// Insert into a complex file — inserts inside the nearest matching AttrSet.
#[test]
fn test_insert_option_into_complex_file() {
    let result = add_option(
        COMPLEX_NIX,
        "services.nginx.port",
        &NixValue::Int(8080),
        "    ",
    )
    .expect("add_option should succeed");

    assert!(result.contains("port = 8080;"));
    // The existing `enable = true;` entry is inside the services.nginx { … } block.
    assert!(result.contains("enable = true;"));
    parse_string(&result).expect("result should be valid Nix");
}

// remove_option (writer)

/// Remove `services.openssh.enable` from `minimal.nix`; other options stay.
#[test]
fn test_remove_option() {
    let result = remove_option(MINIMAL_NIX, "services.openssh.enable")
        .expect("remove_option should succeed");

    assert!(!result.contains("openssh"), "removed option should be gone");
    // Surviving options.
    assert!(result.contains(r#"networking.hostname = "testbox""#));
    assert!(result.contains("environment.systemPackages"));
    parse_string(&result).expect("result should be valid Nix");
}

/// Attempting to remove an option that does not exist returns an error.
#[test]
fn test_remove_option_missing() {
    let err = remove_option(MINIMAL_NIX, "services.postgresql.enable")
        .expect_err("should fail for missing option");
    // Should be InsertionPointNotFound, not a panic.
    assert!(
        matches!(err, crate::nix_parser::WriteError::InsertionPointNotFound),
        "expected InsertionPointNotFound, got {:?}",
        err
    );
}

// Comment preservation

/// Modify a value in `with-comments.nix`; all comments must survive.
#[test]
fn test_preserves_comments() {
    let nix = parse_string(WITH_COMMENTS_NIX).expect("should parse");
    let node = find_option(&nix, "networking.hostname").expect("should find");
    let range = node.text_range();

    let result = set_value(WITH_COMMENTS_NIX, range, &NixValue::String("changed".into()))
        .expect("set_value should succeed");

    // Block and line comments must survive.
    assert!(result.contains("# Network configuration"));
    assert!(result.contains("# Machine hostname"));
    assert!(result.contains("# Enable SSH for remote access"));
    assert!(result.contains("# TODO: restrict to specific IPs"));
    assert!(result.contains("/* System packages */"));

    // New value is in place.
    assert!(result.contains(r#"networking.hostname = "changed""#));
    parse_string(&result).expect("result should be valid Nix");
}

// Indentation detection

/// `minimal.nix` uses 2-space indentation.
#[test]
fn test_detect_indentation_two_spaces() {
    assert_eq!(detect_indent(MINIMAL_NIX), "  ");
}

/// Explicit 4-space indented source is detected correctly.
#[test]
fn test_detect_indentation_four_spaces() {
    let src = "{\n    enable = true;\n    port = 8080;\n}";
    assert_eq!(detect_indent(src), "    ");
}

/// Tab-indented source is detected correctly.
#[test]
fn test_detect_indentation_tabs() {
    let src = "{\n\tenable = true;\n\tport = 8080;\n}";
    assert_eq!(detect_indent(src), "\t");
}
