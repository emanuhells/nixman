//! Git awareness for NixOS configuration workspaces.
//!
//! # Overview
//!
//! This module detects whether the active workspace lives inside a git
//! repository and exposes branch / status information.  It also provides a
//! commit helper that stages a caller-specified set of files and records a new
//! commit — but it never commits automatically.
//!
//! # Sub-modules
//!
//! * [`detect`] — repository detection and status queries.
//! * [`commit`] — file staging, commit creation, and message suggestion.
//! * [`types`]  — shared data structures and the [`GitError`] type.
//!
//! # Quick start
//!
//! ```ignore
//! use std::path::Path;
//! use nixman_core::git;
//!
//! let path = Path::new("/etc/nixos");
//!
//! if git::is_git_repo(path) {
//!     if let Some(status) = git::status(path).unwrap() {
//!         println!("branch: {}", status.branch);
//!     }
//! }
//! ```

pub mod commit;
pub mod detect;
pub mod types;

// Re-export the most commonly used items at the module root so callers can
// write `git::is_git_repo(…)` or `git::GitStatus` without importing sub-modules.
pub use detect::{is_git_repo, status};
pub use types::{CommitRequest, GitBranch, GitError, GitStatus};
