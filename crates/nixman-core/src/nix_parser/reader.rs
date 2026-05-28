//! File reading and parsing functions.
//!
//! The two public entry points are:
//!
//! * [`parse_file`] — read a `.nix` file from disk and parse it.
//! * [`parse_string`] — parse a Nix source string directly.

use std::path::Path;

use rnix::{Root, parser::ParseError};

use super::types::{NixFile, NixParseError};

// Public API

/// Read a `.nix` file from `path` and parse it into a [`NixFile`].
///
/// Returns `Err(NixParseError::IoError)` if the file cannot be read and
/// `Err(NixParseError::SyntaxError)` if the source contains syntax errors.
pub fn parse_file(path: &Path) -> Result<NixFile, NixParseError> {
    let source = std::fs::read_to_string(path)?;
    parse_string(&source)
}

/// Parse a Nix source string into a [`NixFile`].
///
/// Returns `Err(NixParseError::SyntaxError)` when the source contains syntax
/// errors, including the 1-based line and column of the first error.
pub fn parse_string(source: &str) -> Result<NixFile, NixParseError> {
    let parse = Root::parse(source);

    if let Some(error) = parse.errors().first() {
        let (line, column) = parse_error_position(error, source);
        return Err(NixParseError::SyntaxError {
            line,
            column,
            message: error.to_string(),
        });
    }

    Ok(NixFile {
        source: source.to_string(),
        root: parse.tree(),
    })
}

// Internal helpers

/// Convert a [`ParseError`] to a (line, column) pair using `source` to
/// resolve byte offsets.
fn parse_error_position(error: &ParseError, source: &str) -> (usize, usize) {
    let range = extract_text_range(error);

    match range {
        Some(r) => {
            let offset = u32::from(r.start()) as usize;
            byte_offset_to_line_col(source, offset)
        }
        None => {
            // EOF errors: point one past the last character.
            let total_lines = source.lines().count().max(1);
            let last_col = source.lines().last().map_or(1, |l| l.len() + 1);
            (total_lines, last_col)
        }
    }
}

/// Extract the [`rnix::TextRange`] from a [`ParseError`], if available.
///
/// `ParseError` is `#[non_exhaustive]` so a wildcard arm is required.
fn extract_text_range(error: &ParseError) -> Option<rnix::TextRange> {
    match error {
        ParseError::Unexpected(r) => Some(*r),
        ParseError::UnexpectedExtra(r) => Some(*r),
        ParseError::UnexpectedWanted(_, r, _) => Some(*r),
        ParseError::UnexpectedDoubleBind(r) => Some(*r),
        ParseError::DuplicatedArgs(r, _) => Some(*r),
        // UnexpectedEOF, UnexpectedEOFWanted, RecursionLimitExceeded, and any
        // future variants do not carry a position.
        _ => None,
    }
}

/// Convert a byte offset into the source string to a 1-based (line, column).
pub(crate) fn byte_offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    // Clamp offset to the valid range.
    let offset = offset.min(source.len());
    let prefix = &source[..offset];
    let line = prefix.chars().filter(|&c| c == '\n').count() + 1;
    let column = match prefix.rfind('\n') {
        Some(nl_pos) => {
            // Count bytes (chars) after the last newline.
            prefix[nl_pos + 1..].chars().count() + 1
        }
        None => prefix.chars().count() + 1,
    };
    (line, column)
}

// Tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_attr_set() {
        let src = "{ enable = true; }";
        let nix_file = parse_string(src).expect("should parse");
        assert_eq!(nix_file.source, src);
    }

    #[test]
    fn parse_valid_nix_module() {
        let src = "{ config, pkgs, ... }:\n{\n  services.nginx.enable = true;\n}";
        let nix_file = parse_string(src).expect("should parse");
        assert_eq!(nix_file.source, src);
    }

    #[test]
    fn parse_syntax_error_reports_position() {
        let src = "{ enable = ; }"; // missing value
        let err = parse_string(src).expect_err("should fail");
        match err {
            NixParseError::SyntaxError { line, column, .. } => {
                // Line 1, somewhere past the `=`
                assert_eq!(line, 1);
                assert!(column > 1);
            }
            other => panic!("unexpected error kind: {:?}", other),
        }
    }

    #[test]
    fn byte_offset_first_line() {
        // Offset 0 → line 1, col 1
        assert_eq!(byte_offset_to_line_col("hello", 0), (1, 1));
    }

    #[test]
    fn byte_offset_second_line() {
        let src = "line1\nline2";
        // Offset 6 is the start of "line2"
        assert_eq!(byte_offset_to_line_col(src, 6), (2, 1));
    }
}
