//! Package search, installation listing, and metadata for NixOS.
//!
//! # Overview
//!
//! This module exposes three primary operations backed by the `nix` CLI:
//!
//! - [`search::query`] — search nixpkgs for packages matching a query string
//!   by running `nix search <flake>#nixpkgs <query> --json`.
//! - [`installed::list`] — list packages declared in the NixOS configuration
//!   (currently a placeholder returning an empty vec; will later integrate
//!   with the `nix_parser` module).
//! - [`metadata::get`] — retrieve full package metadata by running
//!   `nix eval <flake>#nixpkgs.<name>.meta --json`.
//!
//! # Error handling
//!
//! All operations return [`types::PackageError`] on failure.  The error
//! distinguishes between CLI failures, JSON parse errors, and packages that
//! simply don't exist in nixpkgs.

pub mod installed;
pub mod manage;
pub mod metadata;
pub mod search;
pub mod types;

// Re-export the most commonly used types at module root.
pub use types::{Package, PackageError, PackageSource, SearchResult};
