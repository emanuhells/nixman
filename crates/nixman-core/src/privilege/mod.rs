//! Privilege escalation via polkit (`pkexec`).
//!
//! # Overview
//! Only two operations ever need elevation:
//! 1. Writing to root-owned paths (checked with [`check::needs_elevation`]).
//! 2. Running `nixos-rebuild` (always elevated).
//!
//! Read-only operations are never elevated.
//!
//! # Example
//! ```ignore
//! use std::path::Path;
//! use nixman_core::privilege::{check, polkit};
//!
//! if check::needs_elevation(Path::new("/etc/nixos/configuration.nix")) {
//!     let result = polkit::run_elevated("tee", &["/etc/nixos/configuration.nix"])
//!         .await?;
//! }
//! ```

pub mod check;
pub mod polkit;
pub mod types;

// Re-export the public API so callers can use `privilege::` as the prefix.
pub use check::needs_elevation;
pub use polkit::run_elevated;
pub use types::{ElevationResult, PrivilegeAction, PrivilegeError};
