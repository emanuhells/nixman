//! Types shared across the config editing module.

use std::io;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use rnix::TextRange;
use serde::{Deserialize, Serialize};

use crate::nix_parser::{NixValue, ResolveError, WriteError};

// PendingChange

/// A pending (not-yet-applied) change to a NixOS configuration option.
///
/// Changes accumulate in a [`super::pending::PendingChanges`] buffer and are
/// flushed to disk all at once by [`super::editor::apply_pending`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingChange {
    /// Dotted option path, e.g. `"services.nginx.enable"`.
    pub option_path: String,
    /// The file in which this change will be applied.
    pub file: PathBuf,
    /// The value before the edit, when the option already existed.
    pub old_value: Option<NixValue>,
    /// The value to write.
    pub new_value: NixValue,
    /// Byte range of the existing value node inside `file`.
    ///
    /// `None` when the option did not previously exist and will be inserted
    /// rather than replaced.
    ///
    /// `TextRange` does not implement `Serialize` / `Deserialize` so this
    /// field is skipped during serialization.
    #[serde(skip)]
    pub range: Option<TextRange>,
    /// When this change was enqueued (UTC).
    pub timestamp: DateTime<Utc>,
}

// EditOperation

/// A high-level description of a desired configuration edit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum EditOperation {
    /// Replace the value of an existing option.
    Set { path: String, value: NixValue },
    /// Remove an option entirely.
    Remove { path: String },
    /// Insert a new option that does not yet exist.
    Insert { path: String, value: NixValue },
}

// ValidationResult

/// The result of running a Nix syntax check on a source file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum ValidationResult {
    /// The source is syntactically valid Nix.
    Valid,
    /// The source contains one or more syntax errors.
    Invalid {
        /// Human-readable error lines from `nix-instantiate`.
        errors: Vec<String>,
    },
}

// FileDiff

/// Before-and-after source text for a single file produced by
/// [`super::pending::PendingChanges::generate_diffs`].
#[derive(Debug, Clone)]
pub struct FileDiff {
    /// The file this diff describes.
    pub file: PathBuf,
    /// Original file content (current on disk, before any pending changes).
    pub original: String,
    /// Modified content (with all pending changes applied, not yet on disk).
    pub modified: String,
}

// ConfigError

/// All errors that can occur in the config editing module.
#[derive(Debug)]
pub enum ConfigError {
    /// An error from the module-graph resolver.
    ResolveError(ResolveError),
    /// A Nix parse error (represented as a string message).
    ParseError(String),
    /// An error from the AST writer.
    WriteError(WriteError),
    /// Validation via `nix-instantiate` reported one or more errors.
    ValidationFailed(Vec<String>),
    /// An underlying I/O error.
    IoError(io::Error),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::ResolveError(e) => write!(f, "resolve error: {}", e),
            ConfigError::ParseError(msg) => write!(f, "parse error: {}", msg),
            ConfigError::WriteError(e) => write!(f, "write error: {}", e),
            ConfigError::ValidationFailed(errors) => {
                write!(f, "validation failed: {}", errors.join("; "))
            }
            ConfigError::IoError(e) => write!(f, "I/O error: {}", e),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::IoError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<ResolveError> for ConfigError {
    fn from(e: ResolveError) -> Self {
        ConfigError::ResolveError(e)
    }
}

impl From<WriteError> for ConfigError {
    fn from(e: WriteError) -> Self {
        ConfigError::WriteError(e)
    }
}

impl From<io::Error> for ConfigError {
    fn from(e: io::Error) -> Self {
        ConfigError::IoError(e)
    }
}
