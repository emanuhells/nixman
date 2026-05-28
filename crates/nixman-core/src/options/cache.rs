//! Cache storage and hash-based invalidation for the option index.
//!
//! The cache lives at `~/.cache/nixman/options-<hash>.json` where
//! `<hash>` is the SHA-256 hex digest of the flake's `flake.lock`.  A cache
//! hit is guaranteed to be consistent with the current nixpkgs pin.

use std::io;
use std::path::{Path, PathBuf};
use sha2::{Digest, Sha256};

use crate::options::types::OptionIndex;

// CacheError

/// Errors that can occur during cache read/write operations.
#[derive(Debug)]
pub enum CacheError {
    /// An I/O error (file not found, permissions, hash utility missing, …).
    IoError(io::Error),
    /// The hash stored in the cache file does not match the expected value.
    HashMismatch,
}

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheError::IoError(e) => write!(f, "cache I/O error: {}", e),
            CacheError::HashMismatch => write!(f, "cache hash mismatch"),
        }
    }
}

impl std::error::Error for CacheError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CacheError::IoError(e) => Some(e),
            CacheError::HashMismatch => None,
        }
    }
}

impl From<io::Error> for CacheError {
    fn from(e: io::Error) -> Self {
        CacheError::IoError(e)
    }
}


/// Compute the SHA-256 hex digest of `flake_path/flake.lock`.
///
/// Computed in-process using the `sha2` crate.
pub fn hash_flake_lock(flake_path: &Path) -> Result<String, CacheError> {
    let lock_path = flake_path.join("flake.lock");
    let bytes = std::fs::read(&lock_path)?;
    let digest = Sha256::digest(&bytes);
    let hash = digest.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(hash)
}

/// Return the default cache directory: `~/.cache/nixman`.
pub fn default_cache_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".cache").join("nixman")
}

/// Persist `index` to `<cache_dir>/options-<hash>.json`.
///
/// Creates `cache_dir` (and all parents) if it does not already exist.
pub fn save(index: &OptionIndex, cache_dir: &Path) -> Result<(), CacheError> {
    std::fs::create_dir_all(cache_dir).map_err(CacheError::IoError)?;

    let filename = format!("options-{}.json", index.flake_lock_hash);
    let path = cache_dir.join(filename);

    let json = serde_json::to_string(index).map_err(|e| {
        CacheError::IoError(io::Error::new(io::ErrorKind::Other, e.to_string()))
    })?;

    std::fs::write(&path, json).map_err(CacheError::IoError)?;

    Ok(())
}

/// Load a cached index from `<cache_dir>/options-<expected_hash>.json`.
///
/// Returns `None` if the file does not exist, cannot be deserialised, or
/// its embedded `flake_lock_hash` does not match `expected_hash`.
pub fn load(cache_dir: &Path, expected_hash: &str) -> Option<OptionIndex> {
    let filename = format!("options-{}.json", expected_hash);
    let path = cache_dir.join(filename);

    let data = std::fs::read_to_string(&path).ok()?;
    let index: OptionIndex = serde_json::from_str(&data).ok()?;

    // Guard against a corrupted or recycled file.
    if index.flake_lock_hash != expected_hash {
        return None;
    }

    Some(index)
}

/// Return `true` if a valid cached index exists for the current `flake.lock`.
pub fn is_valid(cache_dir: &Path, flake_path: &Path) -> bool {
    match hash_flake_lock(flake_path) {
        Ok(hash) => load(cache_dir, &hash).is_some(),
        Err(_) => false,
    }
}

/// Persist an HM option index to `<cache_dir>/hm-options-<hash>.json`.
pub fn save_hm(index: &OptionIndex, cache_dir: &Path) -> Result<(), CacheError> {
    std::fs::create_dir_all(cache_dir).map_err(CacheError::IoError)?;

    let filename = format!("hm-options-{}.json", index.flake_lock_hash);
    let path = cache_dir.join(filename);

    let json = serde_json::to_string(index).map_err(|e| {
        CacheError::IoError(io::Error::new(io::ErrorKind::Other, e.to_string()))
    })?;

    std::fs::write(&path, json).map_err(CacheError::IoError)?;

    Ok(())
}

/// Load a cached HM option index from `<cache_dir>/hm-options-<expected_hash>.json`.
pub fn load_hm(cache_dir: &Path, expected_hash: &str) -> Option<OptionIndex> {
    let filename = format!("hm-options-{}.json", expected_hash);
    let path = cache_dir.join(filename);

    let data = std::fs::read_to_string(&path).ok()?;
    let index: OptionIndex = serde_json::from_str(&data).ok()?;

    if index.flake_lock_hash != expected_hash {
        return None;
    }

    Some(index)
}



#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tempfile::TempDir;

    fn make_index(hash: &str) -> OptionIndex {
        OptionIndex {
            options: vec![],
            flake_lock_hash: hash.to_string(),
            built_at: Utc::now(),
            nixpkgs_rev: "abc123".to_string(),
        }
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let index = make_index("deadbeef");
        save(&index, dir.path()).unwrap();

        let loaded = load(dir.path(), "deadbeef").expect("should load");
        assert_eq!(loaded.flake_lock_hash, "deadbeef");
        assert_eq!(loaded.nixpkgs_rev, "abc123");
    }

    #[test]
    fn load_returns_none_for_missing_file() {
        let dir = TempDir::new().unwrap();
        assert!(load(dir.path(), "doesnotexist").is_none());
    }

    #[test]
    fn load_returns_none_when_filename_hash_differs() {
        let dir = TempDir::new().unwrap();
        let index = make_index("aaa");
        save(&index, dir.path()).unwrap();
        // "bbb" → different filename → file not found → None
        assert!(load(dir.path(), "bbb").is_none());
    }

    #[test]
    fn is_valid_returns_false_when_no_flake_lock() {
        let cache_dir = TempDir::new().unwrap();
        let flake_dir = TempDir::new().unwrap();
        // No flake.lock present → hash_flake_lock fails → false
        assert!(!is_valid(cache_dir.path(), flake_dir.path()));
    }

    #[test]
    fn is_valid_returns_false_when_no_cache_file() {
        let cache_dir = TempDir::new().unwrap();
        let flake_dir = TempDir::new().unwrap();
        std::fs::write(flake_dir.path().join("flake.lock"), b"{}").unwrap();
        assert!(!is_valid(cache_dir.path(), flake_dir.path()));
    }

    #[test]
    fn save_creates_cache_dir_if_absent() {
        let base = TempDir::new().unwrap();
        let nested = base.path().join("a").join("b").join("c");
        let index = make_index("xyz");
        save(&index, &nested).expect("should create dirs and save");
        assert!(load(&nested, "xyz").is_some());
    }
}
