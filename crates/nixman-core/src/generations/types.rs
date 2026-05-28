//! Shared types for the generations module.

use std::io;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Metadata for a single NixOS system generation.
///
/// Generations are stored as numbered symlinks under
/// `/nix/var/nix/profiles/` (`system-N-link → <store-path>`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Generation {
    /// Generation number parsed from the `system-N-link` symlink name.
    pub number: u32,
    /// Creation time taken from the symlink's modification timestamp.
    pub date: DateTime<Utc>,
    /// Contents of `<store-path>/nixos-version`, or `"unknown"` if absent.
    pub nixos_version: String,
    /// Kernel version parsed from the `<store-path>/kernel` symlink target.
    pub kernel_version: String,
    /// Resolved store path that the generation symlink points to.
    pub path: PathBuf,
    /// `true` when this generation is the one currently booted / active.
    pub is_current: bool,
}

/// The result of comparing two generations' package sets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationDiff {
    /// Packages present in the newer generation but not the older one.
    pub added_packages: Vec<String>,
    /// Packages present in the older generation but not the newer one.
    pub removed_packages: Vec<String>,
    /// Packages present in both generations but at different versions.
    /// Tuple: `(package_name, old_version, new_version)`.
    pub changed_packages: Vec<(String, String, String)>,
}

/// Summary of a completed garbage-collection run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcResult {
    /// Approximate number of bytes freed as reported by `nix-collect-garbage`.
    pub freed_bytes: u64,
    /// Generation numbers that were deleted before running GC
    /// (only populated when `keep_last` was supplied).
    pub deleted_generations: Vec<u32>,
}

/// All errors that generation operations can produce.
#[derive(Debug)]
pub enum GenerationError {
    /// Underlying I/O error (file not found, permission denied, etc.).
    IoError(io::Error),
    /// A value could not be parsed from the filesystem or command output.
    ParseError(String),
    /// A nix command was run but exited with a non-zero status.
    CommandFailed { exit_code: i32, stderr: String },
    /// The requested generation number does not exist on this system.
    GenerationNotFound(u32),
}

impl std::fmt::Display for GenerationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError(e) => write!(f, "I/O error: {e}"),
            Self::ParseError(msg) => write!(f, "parse error: {msg}"),
            Self::CommandFailed { exit_code, stderr } => {
                write!(f, "command failed (exit {exit_code}): {stderr}")
            }
            Self::GenerationNotFound(n) => write!(f, "generation {n} not found"),
        }
    }
}

impl std::error::Error for GenerationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::IoError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for GenerationError {
    fn from(e: io::Error) -> Self {
        Self::IoError(e)
    }
}
