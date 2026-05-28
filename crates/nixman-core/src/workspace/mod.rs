//! Workspace detection and management for NixOS configuration directories.
//!
//! # Overview
//!
//! This module locates the user's NixOS configuration on disk and exposes it
//! as a [`Workspace`] value.  It also provides a [`wizard`] sub-module that
//! can scaffold a brand-new flake-based workspace for first-time users.
//!
//! # Detection chain
//!
//! [`detect()`] tries each candidate in order and returns the first hit:
//!
//! 1. `/etc/nixos` — symlinks are fully resolved before classification.
//! 2. `$HOME/nix-config`
//! 3. `$HOME/.config/nixos`
//! 4. [`WorkspaceError::NotFound`] if none of the above contained a
//!    recognisable NixOS configuration.

pub mod detect;
pub mod types;
pub mod wizard;
pub mod hm;

// Re-export the most commonly used items at module root so callers can write
// `workspace::detect()` and `workspace::Workspace` without digging into
// sub-modules.
pub use detect::detect;
pub use hm::{detect_hm, HmWorkspace};
pub use types::{OwnershipInfo, Workspace, WorkspaceError, WorkspaceKind};

#[cfg(test)]
mod tests;
