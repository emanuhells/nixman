//! Nix AST reader module.
//!
//! Provides a read-only API for parsing `.nix` files and traversing their
//! abstract syntax trees.
//!
//! # Quick start
//!
//! ```rust,ignore
//! use nixman_core::nix_parser::{parse_file, parse_string, find_option};
//!
//! // Parse a file from disk.
//! let nix = parse_file(std::path::Path::new("/etc/nixos/configuration.nix"))?;
//!
//! // Or parse a string directly.
//! let nix = parse_string("{ services.nginx.enable = true; }")?;
//!
//! // Find an option by its dotted path.
//! if let Some(node) = find_option(&nix, "services.nginx.enable") {
//!     println!("{:?}", node.to_nix_value()); // Bool(true)
//! }
//! ```

pub mod format;
pub mod insert;
pub mod modules;
pub mod reader;
pub mod resolver;
pub mod traversal;
pub mod types;
pub mod writer;

// Re-export the most commonly used items at the module root so callers
// can write `use nix_parser::{parse_file, find_option, …}` instead of
// reaching into sub-modules.
pub use reader::{parse_file, parse_string};
pub use traversal::{expr_to_value, find_option, iterate_attr_set};
pub use writer::WriteError;
// Re-export TextRange so callers can construct ranges without depending on
// rnix/rowan directly.
pub use rnix::TextRange;
pub use types::{
    ModuleGraph, NixFile, NixParseError, NixValue, ParsedNode, ResolveError, ResolvedCandidate,
    ResolvedOption,
};

#[cfg(test)]
mod tests;
