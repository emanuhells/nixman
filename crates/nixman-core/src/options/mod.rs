//! NixOS option index: build, cache, and search the full option set.
//!
//! # Overview
//!
//! This module evaluates the complete NixOS option set from the user's pinned
//! nixpkgs (via `nix build`) and caches the result as a local JSON file keyed
//! to the SHA-256 hash of `flake.lock`.  When the flake inputs change the
//! cache is automatically invalidated and rebuilt.
//!
//! # Quick start
//!
//! ```rust,ignore
//! use std::sync::mpsc;
//! use nixman_core::options;
//!
//! let (tx, rx) = mpsc::channel();
//! let index = options::build_index(std::path::Path::new("/etc/nixos"), tx)?;
//!
//! // Search for nginx-related options.
//! let results = options::search::query(&index, "nginx", 20);
//! for opt in results {
//!     println!("{}: {}", opt.path, opt.description);
//! }
//! ```

pub mod cache;
pub mod index;
pub mod search;
pub mod types;

pub use index::{build, build_hm, IndexError};
pub use types::{OptionIndex, OptionMeta, OptionType};

// Convenience wrapper

/// Build or load a cached [`OptionIndex`] for the flake at `flake_path`.
///
/// If a valid cache entry exists for the current `flake.lock` it is returned
/// immediately (and `1.0` is sent on `progress_tx`).  Otherwise
/// [`index::build`] is invoked and the result is saved to the default cache
/// directory (`~/.cache/nixman/`).
pub fn build_index(
    flake_path: &std::path::Path,
    progress_tx: std::sync::mpsc::Sender<f32>,
) -> Result<OptionIndex, IndexError> {
    let cache_dir = cache::default_cache_dir();

    // Return the cached index if it is still valid.
    if let Ok(hash) = cache::hash_flake_lock(flake_path) {
        if let Some(idx) = cache::load(&cache_dir, &hash) {
            let _ = progress_tx.send(1.0);
            return Ok(idx);
        }
    }

    // No valid cache — perform a full build.
    let idx = index::build(flake_path, progress_tx)?;
    // Persist to cache; ignore errors (cache is best-effort).
    let _ = cache::save(&idx, &cache_dir);
    Ok(idx)
}

/// Build or load a cached [`OptionIndex`] for the Home Manager flake at `hm_path`.
///
/// Uses a separate cache namespace (`hm-options-`) so it never collides with
/// the NixOS option cache.
pub fn build_hm_index(
    hm_path: &std::path::Path,
    progress_tx: std::sync::mpsc::Sender<f32>,
) -> Result<OptionIndex, IndexError> {
    let cache_dir = cache::default_cache_dir();

    // Return the cached index if it is still valid.
    if let Ok(hash) = cache::hash_flake_lock(hm_path) {
        if let Some(idx) = cache::load_hm(&cache_dir, &hash) {
            let _ = progress_tx.send(1.0);
            return Ok(idx);
        }
    }

    // No valid cache — perform a full build.
    let idx = index::build_hm(hm_path, progress_tx)?;
    // Persist to cache; ignore errors (cache is best-effort).
    let _ = cache::save_hm(&idx, &cache_dir);
    Ok(idx)
}

#[cfg(test)]
mod tests;
