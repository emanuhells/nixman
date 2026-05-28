//! Enumerate NixOS system generations from `/nix/var/nix/profiles/`.
//!
//! Generation symlinks follow the naming scheme `system-N-link` and live
//! alongside the `system` symlink that points to the currently active one.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::generations::types::{Generation, GenerationError};

/// Root directory that holds all generation symlinks.
const PROFILES_DIR: &str = "/nix/var/nix/profiles";

/// Return all NixOS system generations sorted by generation number descending
/// (most-recent first).
///
/// Each entry includes the resolved store path, creation timestamp, NixOS
/// version string, kernel version, and a flag indicating whether it is the
/// currently active generation.
///
/// # Errors
/// Returns [`GenerationError::IoError`] if the profiles directory cannot be
/// read, or [`GenerationError::ParseError`] if a symlink name is malformed.
pub async fn all() -> Result<Vec<Generation>, GenerationError> {
    let profiles = Path::new(PROFILES_DIR);

    // Determine the current generation number by reading the `system` symlink.
    let current_number = read_current_generation_number(profiles)?;

    // Collect entries matching the pattern `system-N-link`.
    let mut generations: Vec<Generation> = fs::read_dir(profiles)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let number = parse_generation_number(&name)?;
            let symlink_path = entry.path();
            build_generation(symlink_path, number, current_number).ok()
        })
        .collect();

    // Most-recent generation first.
    generations.sort_by_key(|g| std::cmp::Reverse(g.number));
    Ok(generations)
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Read `/nix/var/nix/profiles/system` → `system-N-link` and parse N.
fn read_current_generation_number(profiles: &Path) -> Result<u32, GenerationError> {
    let link = fs::read_link(profiles.join("system"))?;
    // The target may be a relative path like `system-42-link`.
    let name = link
        .file_name()
        .ok_or_else(|| {
            GenerationError::ParseError("could not extract filename from system symlink".into())
        })?
        .to_string_lossy()
        .into_owned();

    parse_generation_number(&name).ok_or_else(|| {
        GenerationError::ParseError(format!(
            "could not parse generation number from system symlink target '{name}'"
        ))
    })
}

/// Parse the generation number from a symlink name like `system-42-link`.
/// Returns `None` if the name does not match the expected pattern.
fn parse_generation_number(name: &str) -> Option<u32> {
    name.strip_prefix("system-")
        .and_then(|s| s.strip_suffix("-link"))
        .and_then(|s| s.parse::<u32>().ok())
}

/// Build a [`Generation`] from a symlink path and the known current number.
fn build_generation(
    symlink_path: PathBuf,
    number: u32,
    current_number: u32,
) -> Result<Generation, GenerationError> {
    // Mtime of the symlink itself — this is when the generation was created.
    let meta = fs::symlink_metadata(&symlink_path)?;
    let mtime = meta.modified()?;
    let date: DateTime<Utc> = mtime.into();

    // Resolve the symlink once to get the actual store path.
    let store_path = resolve_store_path(&symlink_path)?;

    let nixos_version = read_nixos_version(&store_path);
    let kernel_version = read_kernel_version(&store_path);

    Ok(Generation {
        number,
        date,
        nixos_version,
        kernel_version,
        path: store_path,
        is_current: number == current_number,
    })
}

/// Resolve a symlink to its canonical store path.
fn resolve_store_path(symlink: &Path) -> Result<PathBuf, GenerationError> {
    // `canonicalize` follows all levels of symlinks and returns the real path.
    fs::canonicalize(symlink).map_err(GenerationError::IoError)
}

/// Read `<store_path>/nixos-version`; returns `"unknown"` on any error.
fn read_nixos_version(store_path: &Path) -> String {
    fs::read_to_string(store_path.join("nixos-version"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}

/// Read the kernel version from the `<store_path>/kernel` symlink target.
///
/// The symlink typically points to something like
/// `/nix/store/<hash>-linux-5.15.74-x86_64-linux/bzImage`.  We climb up to
/// the store-path component and parse the version number from its name.
/// Falls back to `"unknown"` if anything goes wrong.
fn read_kernel_version(store_path: &Path) -> String {
    let kernel_link = store_path.join("kernel");

    let target = match fs::read_link(&kernel_link) {
        Ok(t) => t,
        Err(_) => return "unknown".into(),
    };

    // The symlink may point to a file (bzImage) or a directory. Climb up until
    // we reach a direct child of `/nix/store/`.
    let store_component = find_store_component(&target);
    parse_kernel_version_from_store_name(store_component)
}

/// Walk up from `path` until we find a path whose parent is `/nix/store/` and
/// return that path's file-name string. Returns `None` if not found.
fn find_store_component(path: &Path) -> Option<&str> {
    let mut current = path;
    while let Some(parent) = current.parent() {
        if parent == Path::new("/nix/store") {
            return current.file_name().and_then(|n| n.to_str());
        }
        current = parent;
    }
    None
}

/// Extract the kernel version from a store-path component name.
///
/// Example: `"<hash>-linux-5.15.74-x86_64-linux"` → `"5.15.74"`.
/// The hash is the first 32 hex chars followed by `-`.
fn parse_kernel_version_from_store_name(store_name: Option<&str>) -> String {
    let name = match store_name {
        Some(n) => n,
        None => return "unknown".into(),
    };

    // Strip the 32-char hash prefix + '-'.
    let after_hash = if name.len() > 33 && name.as_bytes().get(32) == Some(&b'-') {
        &name[33..]
    } else {
        name
    };

    // The version is the first `-`-separated component that begins with a digit.
    // e.g. "linux-5.15.74-x86_64-linux" → "5.15.74"
    after_hash
        .split('-')
        .find(|s| s.starts_with(|c: char| c.is_ascii_digit()))
        .unwrap_or("unknown")
        .to_string()
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_generation_number_valid() {
        assert_eq!(parse_generation_number("system-1-link"), Some(1));
        assert_eq!(parse_generation_number("system-42-link"), Some(42));
        assert_eq!(parse_generation_number("system-100-link"), Some(100));
    }

    #[test]
    fn parse_generation_number_invalid() {
        assert_eq!(parse_generation_number("system"), None);
        assert_eq!(parse_generation_number("system-link"), None);
        assert_eq!(parse_generation_number("system-abc-link"), None);
        assert_eq!(parse_generation_number("system-42"), None);
    }

    #[test]
    fn parse_kernel_version_typical() {
        // Simulated store name: hash(32) + '-' + package-name
        let hash = "a".repeat(32);
        let name = format!("{hash}-linux-5.15.74-x86_64-linux");
        assert_eq!(
            parse_kernel_version_from_store_name(Some(&name)),
            "5.15.74"
        );
    }

    #[test]
    fn parse_kernel_version_no_hash() {
        // Fallback: no hash prefix
        assert_eq!(
            parse_kernel_version_from_store_name(Some("linux-6.1.0-x86_64-linux")),
            "6.1.0"
        );
    }

    #[test]
    fn parse_kernel_version_unknown() {
        assert_eq!(parse_kernel_version_from_store_name(None), "unknown");
    }
}
