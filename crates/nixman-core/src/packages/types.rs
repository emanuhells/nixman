//! Core data types for the packages module.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// PackageSource

/// Indicates which configuration layer declares a package.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageSource {
    /// Declared in `environment.systemPackages` (NixOS system config).
    System,
    /// Declared via Home Manager.
    HomeManager,
}

// Package

/// A single Nix package with its associated metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    /// Attribute name / package name (e.g. `firefox`).
    pub name: String,
    /// Package version string (e.g. `110.0`).
    pub version: String,
    /// Short human-readable description.
    pub description: String,
    /// Upstream homepage URL, if available.
    pub homepage: Option<String>,
    /// SPDX licence identifier or full licence name, if available.
    pub license: Option<String>,
    /// Which configuration layer this package originates from.
    pub source: PackageSource,
}

// SearchResult

/// The result of a `nix search` query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Packages that matched the query.
    pub packages: Vec<Package>,
    /// The original search query string.
    pub query: String,
    /// Total number of matched packages (equal to `packages.len()`).
    pub total: usize,
}

// PackageError

/// Errors produced by package search and metadata operations.
#[derive(Debug)]
pub enum PackageError {
    /// A `nix` CLI invocation exited with a non-zero status.
    NixCommandFailed(String),
    /// The JSON output from `nix` could not be parsed.
    ParseError(String),
    /// The requested package was not found in nixpkgs.
    NotFound(String),
    /// The package is not present in `environment.systemPackages`.
    NotInConfig(String),
    /// The `nix` binary is not available; verification cannot run.
    NixNotAvailable,
    /// The package appears in more than one file; the caller must specify
    /// which file to remove it from. Carries `(file, line_number)` pairs.
    AmbiguousRemove(Vec<(PathBuf, usize)>),
}

impl std::fmt::Display for PackageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageError::NixCommandFailed(msg) => {
                write!(f, "nix command failed: {}", msg)
            }
            PackageError::ParseError(msg) => {
                write!(f, "parse error: {}", msg)
            }
            PackageError::NotFound(name) => {
                write!(f, "package not found: {}", name)
            }
            PackageError::NotInConfig(name) => {
                write!(f, "package not in environment.systemPackages: {}", name)
            }
            PackageError::NixNotAvailable => {
                write!(f, "nix binary not found; cannot verify package name")
            }
            PackageError::AmbiguousRemove(paths) => {
                write!(f, "package found in multiple files:\n")?;
                for (path, line) in paths {
                    write!(f, "  - {} (line {})\n", path.display(), line)?;
                }
                write!(f, "use --file to specify which to remove")
            }
        }
    }
}

impl std::error::Error for PackageError {}
