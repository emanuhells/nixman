//! NixOS generation management.
//!
//! # Overview
//!
//! This module provides a complete interface for inspecting and managing NixOS
//! system generations.  Generations are numbered snapshots of the system
//! configuration stored as symlinks under `/nix/var/nix/profiles/`.
//!
//! | Sub-module              | Responsibility                                     |
//! |-------------------------|----------------------------------------------------|
//! | [`list`]                | Enumerate all generations with metadata            |
//! | [`diff`]                | Compare package sets between two generations       |
//! | [`rollback`]            | Activate an older generation                       |
//! | [`gc`]                  | Delete old generations and collect garbage         |
//! | [`types`]               | Shared data types and error enum                   |
//!
//! # Quick start
//!
//! ```ignore
//! use nixman_core::generations;
//!
//! // List all generations (most-recent first).
//! let gens = generations::list::all().await?;
//!
//! // Find what changed between generations 10 and 11.
//! let diff = generations::diff::compare(10, 11).await?;
//!
//! // Roll back to generation 10.
//! generations::rollback::to(10).await?;
//!
//! // Collect garbage, keeping the last 5 generations.
//! let result = generations::gc::collect(Some(5)).await?;
//! println!("freed {} bytes", result.freed_bytes);
//! ```

pub mod diff;
pub mod gc;
pub mod list;
pub mod rollback;
pub mod types;

pub use types::{GcResult, Generation, GenerationDiff, GenerationError};

#[cfg(test)]
mod tests;
