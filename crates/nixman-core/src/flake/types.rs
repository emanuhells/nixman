//! Shared types for the flake module.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A single direct input declared in a flake, with its pinned metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlakeInput {
    /// Input name as declared in `flake.nix` (e.g. `"nixpkgs"`).
    pub name: String,
    /// Reconstructed flake URL (e.g. `"github:NixOS/nixpkgs"`).
    pub url: String,
    /// Pinned git revision hash.
    pub rev: String,
    /// Timestamp of the pinned commit.
    pub last_modified: DateTime<Utc>,
    /// NAR hash of the fetched source tree.
    pub nar_hash: String,
}

/// Errors returned by flake operations.
#[derive(Debug)]
pub enum FlakeError {
    /// `flake.lock` does not exist at the given workspace path.
    LockNotFound,
    /// The lock file could not be parsed or is missing expected fields.
    ParseError(String),
    /// A file-system I/O error occurred.
    IoError(std::io::Error),
}

impl std::fmt::Display for FlakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FlakeError::LockNotFound => write!(f, "flake.lock not found in workspace"),
            FlakeError::ParseError(msg) => write!(f, "failed to parse flake.lock: {msg}"),
            FlakeError::IoError(e) => write!(f, "I/O error reading flake.lock: {e}"),
        }
    }
}

impl From<std::io::Error> for FlakeError {
    fn from(e: std::io::Error) -> Self {
        FlakeError::IoError(e)
    }
}
