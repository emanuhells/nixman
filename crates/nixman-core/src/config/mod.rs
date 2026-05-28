//! Configuration editing module.
//!
//! Ties together the Nix parser, AST writer, module-graph resolver, and
//! `nix-instantiate` validator to provide a clean API for reading and
//! modifying NixOS configuration options.
//!
//! # Workflow
//!
//! ```rust,ignore
//! use std::path::Path;
//! use nixman_core::config::{editor, PendingChanges};
//! use nixman_core::nix_parser::NixValue;
//!
//! let workspace = Path::new("/etc/nixos");
//! let mut pending = PendingChanges::new();
//!
//! // 1. Read the current value (optional).
//! let current = editor::get_value(workspace, "services.nginx.enable")?;
//!
//! // 2. Queue a change — nothing is written to disk yet.
//! editor::set_value(
//!     &mut pending,
//!     workspace,
//!     "services.nginx.enable",
//!     NixValue::Bool(true),
//! )?;
//!
//! // 3. Apply — validates syntax, then writes to disk.
//! editor::apply_pending(&mut pending, workspace)?;
//! # Ok::<(), nixman_core::config::ConfigError>(())
//! ```

pub mod editor;
pub mod pending;
pub mod types;
pub mod validate;

// Convenience re-exports so callers can write
// `use nixman_core::config::{PendingChanges, ConfigError, …}`.
pub use pending::PendingChanges;
pub use types::{ConfigError, EditOperation, FileDiff, PendingChange, ValidationResult};

#[cfg(test)]
mod tests;
