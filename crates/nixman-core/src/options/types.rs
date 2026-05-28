//! Core data types for the options index module.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// OptionType

/// The type of a NixOS option as declared in the module system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OptionType {
    Bool,
    String,
    Int,
    Float,
    Path,
    Package,
    ListOf(Box<OptionType>),
    AttrsOf(Box<OptionType>),
    /// An option that accepts one of a fixed set of string values.
    Enum(Vec<std::string::String>),
    Submodule,
    Unspecified,
}

// OptionMeta

/// Metadata for a single NixOS option.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionMeta {
    /// Dotted option path, e.g. `"services.nginx.enable"`.
    pub path: std::string::String,
    /// The evaluated type of this option.
    pub option_type: OptionType,
    /// Default value serialised to a human-readable string, if any.
    pub default: Option<std::string::String>,
    /// Human-readable description (HTML/DocBook stripped).
    pub description: std::string::String,
    /// Source file that declares this option (first entry in `declarations`).
    pub declared_in: std::string::String,
    /// Example value serialised to a human-readable string, if any.
    pub example: Option<std::string::String>,
}

// OptionIndex

/// A cached, indexed snapshot of the full NixOS option set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionIndex {
    /// All evaluated options, sorted by path.
    pub options: Vec<OptionMeta>,
    /// SHA-256 hex digest of the `flake.lock` at build time.
    pub flake_lock_hash: std::string::String,
    /// Timestamp when this index was built.
    pub built_at: DateTime<Utc>,
    /// nixpkgs git revision pinned by the flake (extracted from `flake.lock`).
    pub nixpkgs_rev: std::string::String,
}
