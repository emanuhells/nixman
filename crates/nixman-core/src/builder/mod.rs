//! NixOS rebuild orchestration with streaming output.
//!
//! # Overview
//!
//! This module wraps `nixos-rebuild` (via `pkexec`) and exposes a streaming
//! interface so callers receive build output in real time.  The full result
//! is also returned when the build completes and optionally persisted to a
//! JSON history file.
//!
//! | Sub-module   | Responsibility                                          |
//! |--------------|---------------------------------------------------------|
//! | [`rebuild`]  | Spawn `pkexec nixos-rebuild` and orchestrate streaming  |
//! | [`hm`]       | Spawn `home-manager` and orchestrate streaming          |
//! | [`stream`]   | Line-by-line reader that forwards events through mpsc   |
//! | [`phases`]   | Detect build phases from output-line patterns           |
//! | [`history`]  | Persist and load build history from disk                |
//! | [`types`]    | Shared data types and error enum                        |
//!
//! # Quick start
//!
//! ```ignore
//! use std::path::Path;
//! use tokio::sync::mpsc;
//! use nixman_core::builder::{rebuild, types::{BuildMode, BuildEvent}};
//!
//! let (tx, mut rx) = mpsc::channel(64);
//! let flake = Path::new("/etc/nixos");
//!
//! tokio::spawn(async move {
//!     while let Some(event) = rx.recv().await {
//!         match event {
//!             BuildEvent::Output(line) => println!("{line}"),
//!             BuildEvent::PhaseChanged(phase) => println!("phase: {phase:?}"),
//!             BuildEvent::Complete(result) => {
//!                 println!("done: success={}", result.success);
//!             }
//!         }
//!     }
//! });
//!
//! let result = rebuild::run(BuildMode::Switch, flake, tx).await?;
//! ```

pub mod hm;
pub mod history;
pub mod phases;
pub mod rebuild;
pub mod stream;
pub mod types;

// Re-export the most-used items so callers can write `builder::BuildMode`
// instead of `builder::types::BuildMode`.
pub use types::{
    BuildError, BuildEvent, BuildHistoryEntry, BuildMode, BuildPhase, BuildResult,
};

#[cfg(test)]
mod tests;
