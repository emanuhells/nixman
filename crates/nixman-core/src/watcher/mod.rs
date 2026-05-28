//! File-system watcher for the NixOS config directory.
//!
//! Watches a directory recursively for external changes to `.nix` files,
//! debounces rapid changes, and forwards typed [`FileEvent`]s through a
//! Tokio mpsc channel.
//!
//! # Quick start
//!
//! ```rust,no_run
//! use nixman_core::watcher::{self, FileEvent, WatcherConfig};
//! use std::path::PathBuf;
//!
//! #[tokio::main]
//! async fn main() {
//!     let (tx, mut rx) = tokio::sync::mpsc::channel::<FileEvent>(64);
//!     let handle = watcher::start(
//!         PathBuf::from("/etc/nixos"),
//!         tx,
//!         WatcherConfig::default(),
//!     ).expect("failed to start watcher");
//!
//!     while let Some(event) = rx.recv().await {
//!         println!("change: {:?}", event);
//!     }
//!
//!     watcher::stop(handle);
//! }
//! ```

pub mod conflict;
pub mod debounce;
pub mod monitor;
pub mod types;

pub use monitor::{start, stop, WatchHandle};
pub use types::{ConflictInfo, FileEvent, WatcherConfig};
