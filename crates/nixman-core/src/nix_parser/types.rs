//! Core data types for the Nix parser module.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

use rnix::{ast, SyntaxNode, TextRange};
use rowan::ast::AstNode;
use serde::{Deserialize, Serialize};

// NixFile

/// A successfully parsed Nix source file.
///
/// Holds both the original source text (needed for error position calculation
/// and round-trip fidelity) and the typed AST root produced by rnix.
#[derive(Debug)]
pub struct NixFile {
    /// The original source text.
    pub source: String,
    /// The typed AST root node.
    pub root: ast::Root,
}

// ParsedNode

/// A single node in the Nix AST together with helper methods to extract its
/// value.
///
/// This wraps an untyped `SyntaxNode` so that callers do not need to depend
/// directly on rowan internals.
#[derive(Clone)]
pub struct ParsedNode {
    pub(crate) syntax: SyntaxNode,
}

impl ParsedNode {
    pub(crate) fn new(syntax: SyntaxNode) -> Self {
        ParsedNode { syntax }
    }

    /// Return the raw source text that this node spans.
    pub fn text(&self) -> String {
        self.syntax.to_string()
    }

    /// Return the byte range of this node within its source file.
    pub fn text_range(&self) -> TextRange {
        self.syntax.text_range()
    }

    /// Attempt to interpret this node as a `NixValue`.
    ///
    /// Complex expressions that cannot be reduced to a simple value are
    /// represented as `NixValue::Expression(raw_text)`.
    pub fn to_nix_value(&self) -> NixValue {
        match ast::Expr::cast(self.syntax.clone()) {
            Some(expr) => crate::nix_parser::traversal::expr_to_value(&expr),
            None => NixValue::Expression(self.syntax.to_string()),
        }
    }
}

impl std::fmt::Debug for ParsedNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ParsedNode({})", self.syntax)
    }
}

// NixValue

/// A Nix value extracted from the AST.
///
/// The `Expression` variant is a catch-all for constructs that cannot be
/// reduced to a primitive (e.g. function calls, let-expressions, interpolated
/// strings).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NixValue {
    Bool(bool),
    String(String),
    Int(i64),
    /// Items of a Nix list `[ a b c ]`.
    List(Vec<NixValue>),
    /// Attribute set entries.  Keys use dotted notation for flat entries
    /// (e.g. `services.nginx.enable = true` → `("services.nginx.enable", …)`).
    AttrSet(Vec<(String, NixValue)>),
    /// A Nix path literal such as `/etc/nixos` or `./hardware.nix`.
    Path(String),
    Null,
    /// A complex expression that could not be simplified.
    Expression(String),
}

// NixParseError

/// Errors that can occur while reading or parsing a Nix file.
#[derive(Debug)]
pub enum NixParseError {
    /// An I/O error that occurred while reading the file from disk.
    IoError(io::Error),
    /// A syntax error in the Nix source with human-readable position info.
    SyntaxError {
        /// 1-based line number.
        line: usize,
        /// 1-based column number (byte offset within the line).
        column: usize,
        /// Human-readable description of the error.
        message: String,
    },
}

impl std::fmt::Display for NixParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NixParseError::IoError(e) => write!(f, "I/O error: {}", e),
            NixParseError::SyntaxError { line, column, message } => {
                write!(f, "syntax error at {}:{}: {}", line, column, message)
            }
        }
    }
}

impl std::error::Error for NixParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            NixParseError::IoError(e) => Some(e),
            NixParseError::SyntaxError { .. } => None,
        }
    }
}

impl From<io::Error> for NixParseError {
    fn from(e: io::Error) -> Self {
        NixParseError::IoError(e)
    }
}

// ModuleGraph

/// The import graph for a NixOS configuration.
///
/// Represents all `.nix` files reachable from the entry file through
/// `imports = [ … ]` declarations.
#[derive(Debug)]
pub struct ModuleGraph {
    /// The top-level entry file (e.g. `configuration.nix`).
    pub entry_file: PathBuf,
    /// Maps each file to the list of files it directly imports.
    pub modules: HashMap<PathBuf, Vec<PathBuf>>,
}

impl ModuleGraph {
    /// Iterate over every file in the graph (including the entry file).
    pub fn all_files(&self) -> impl Iterator<Item = &PathBuf> {
        self.modules.keys()
    }
}

// ResolvedOption

/// The result of looking up an option path across a [`ModuleGraph`].
#[derive(Debug)]
pub struct ResolvedOption {
    /// The file where the option was found (or where it should be inserted
    /// when `exists` is `false`).
    pub file: PathBuf,
    /// `true` if the option is already set in `file`, `false` when this is
    /// a suggested insertion target.
    pub exists: bool,
    /// Byte range of the *value* node within `file`, when the option exists.
    pub range: Option<TextRange>,
}

// ResolvedCandidate

/// A single candidate location where an option was found.
#[derive(Debug, Clone)]
pub struct ResolvedCandidate {
    /// The file containing this definition.
    pub file: PathBuf,
    /// Byte range of the value node in the file.
    pub range: rnix::TextRange,
    /// Number of items in the list node (for list-type options). Zero if not a list.
    pub item_count: usize,
}

// ResolveError

/// Errors that can occur while building the module graph or resolving options.
#[derive(Debug)]
pub enum ResolveError {
    /// A file referenced by an import could not be found on disk.
    FileNotFound(PathBuf),
    /// A file could be read but contains a Nix syntax error.
    ParseError(PathBuf, String),
    /// The option path is set in more than one file.
    Ambiguous(Vec<PathBuf>),
    /// A circular import chain was detected.
    CyclicImport(PathBuf),
    /// An I/O error unrelated to a specific file.
    IoError(io::Error),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::FileNotFound(p) => {
                write!(f, "file not found: {}", p.display())
            }
            ResolveError::ParseError(p, msg) => {
                write!(f, "parse error in {}: {}", p.display(), msg)
            }
            ResolveError::Ambiguous(paths) => {
                write!(f, "option is set in multiple files: ")?;
                for (i, p) in paths.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", p.display())?;
                }
                Ok(())
            }
            ResolveError::CyclicImport(p) => {
                write!(f, "cyclic import detected at: {}", p.display())
            }
            ResolveError::IoError(e) => write!(f, "I/O error: {}", e),
        }
    }
}

impl std::error::Error for ResolveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ResolveError::IoError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for ResolveError {
    fn from(e: io::Error) -> Self {
        ResolveError::IoError(e)
    }
}
