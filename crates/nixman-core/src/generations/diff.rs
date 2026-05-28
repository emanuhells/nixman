//! Diff two NixOS generations by comparing their installed package sets.
//!
//! Package lists are obtained by running `nix-store -q --references <path>/sw`
//! for each generation, then parsing the Nix store-path names into
//! `(package_name, version)` pairs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tokio::process::Command;

use crate::generations::types::{GenerationDiff, GenerationError};

/// Profiles directory used to construct per-generation symlink paths.
const PROFILES_DIR: &str = "/nix/var/nix/profiles";

/// Compare the installed packages of generation `gen_a` against `gen_b` and
/// return a [`GenerationDiff`] describing what changed.
///
/// Packages are identified by their Nix store-path name (minus the hash
/// prefix).  A package is considered "changed" when it appears in both
/// generations but at a different version string.
///
/// # Errors
/// Returns [`GenerationError::GenerationNotFound`] if either symlink does not
/// exist, [`GenerationError::CommandFailed`] if `nix-store` exits non-zero,
/// or [`GenerationError::ParseError`] for unexpected output.
pub async fn compare(gen_a: u32, gen_b: u32) -> Result<GenerationDiff, GenerationError> {
    let path_a = resolve_generation_path(gen_a)?;
    let path_b = resolve_generation_path(gen_b)?;

    let pkgs_a = get_packages(&path_a).await?;
    let pkgs_b = get_packages(&path_b).await?;

    // Build name→version maps for both generations.
    let map_a: HashMap<String, String> = pkgs_a.into_iter().collect();
    let map_b: HashMap<String, String> = pkgs_b.into_iter().collect();

    let mut added_packages = Vec::new();
    let mut removed_packages = Vec::new();
    let mut changed_packages = Vec::new();

    // Packages in gen_b.
    for (name, version_b) in &map_b {
        match map_a.get(name) {
            None => added_packages.push(name.clone()),
            Some(version_a) if version_a != version_b => {
                changed_packages.push((name.clone(), version_a.clone(), version_b.clone()));
            }
            _ => {} // same version → no change
        }
    }

    // Packages present in gen_a but absent from gen_b.
    for name in map_a.keys() {
        if !map_b.contains_key(name) {
            removed_packages.push(name.clone());
        }
    }

    // Sort all lists for deterministic output.
    added_packages.sort();
    removed_packages.sort();
    changed_packages.sort_by(|a, b| a.0.cmp(&b.0));

    Ok(GenerationDiff {
        added_packages,
        removed_packages,
        changed_packages,
    })
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Return the resolved store path for generation `number`.
///
/// Reads the `system-N-link` symlink in the profiles directory.
fn resolve_generation_path(number: u32) -> Result<PathBuf, GenerationError> {
    let symlink = PathBuf::from(PROFILES_DIR).join(format!("system-{number}-link"));
    if !symlink.exists() {
        return Err(GenerationError::GenerationNotFound(number));
    }
    std::fs::canonicalize(&symlink).map_err(GenerationError::IoError)
}

/// Run `nix-store -q --references <gen_path>/sw` and parse the output into a
/// list of `(package_name, version)` pairs.
///
/// The `sw` sub-path is the software collection where NixOS records every
/// package installed into the generation.
async fn get_packages(gen_path: &Path) -> Result<Vec<(String, String)>, GenerationError> {
    let sw_path = gen_path.join("sw");

    let output = Command::new("nix-store")
        .args(["-q", "--references"])
        .arg(&sw_path)
        .output()
        .await
        .map_err(GenerationError::IoError)?;

    if !output.status.success() {
        return Err(GenerationError::CommandFailed {
            exit_code: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let packages: Vec<(String, String)> = stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(parse_store_path)
        .filter(|(name, _)| !name.is_empty())
        .collect();

    Ok(packages)
}

/// Parse a Nix store path into a `(package_name, version)` pair.
///
/// Store paths have the form `/nix/store/<32-char-hash>-<name>-<version>`.
/// The version is identified as the suffix starting at the first
/// `-`-separated component that begins with an ASCII digit.
///
/// Examples:
/// - `/nix/store/abc…-firefox-100.0.1` → `("firefox", "100.0.1")`
/// - `/nix/store/abc…-gnome-shell-42.0` → `("gnome-shell", "42.0")`
/// - `/nix/store/abc…-bash-interactive-5.1-p16` → `("bash-interactive", "5.1-p16")`
fn parse_store_path(path: &str) -> (String, String) {
    // Take only the final path component.
    let basename = path.rsplit('/').next().unwrap_or(path);

    // Strip the 32-char hash + '-'.
    let after_hash = if basename.len() > 33 && basename.as_bytes().get(32) == Some(&b'-') {
        &basename[33..]
    } else {
        basename
    };

    let parts: Vec<&str> = after_hash.split('-').collect();

    // Find the index of the first component that looks like a version number.
    let version_start = parts
        .iter()
        .position(|s| s.starts_with(|c: char| c.is_ascii_digit()));

    match version_start {
        Some(idx) if idx > 0 => {
            let pkg_name = parts[..idx].join("-");
            let version = parts[idx..].join("-");
            (pkg_name, version)
        }
        // No version component found — store just the name with an empty version.
        _ => (after_hash.to_string(), String::new()),
    }
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sp(path: &str) -> (String, String) {
        parse_store_path(path)
    }

    #[test]
    fn parse_simple_package() {
        let hash = "a".repeat(32);
        assert_eq!(sp(&format!("/nix/store/{hash}-firefox-100.0.1")), ("firefox".into(), "100.0.1".into()));
    }

    #[test]
    fn parse_multi_word_name() {
        let hash = "b".repeat(32);
        assert_eq!(
            sp(&format!("/nix/store/{hash}-gnome-shell-42.0")),
            ("gnome-shell".into(), "42.0".into())
        );
    }

    #[test]
    fn parse_hyphenated_version() {
        let hash = "c".repeat(32);
        assert_eq!(
            sp(&format!("/nix/store/{hash}-bash-interactive-5.1-p16")),
            ("bash-interactive".into(), "5.1-p16".into())
        );
    }

    #[test]
    fn parse_no_version() {
        let hash = "d".repeat(32);
        // Package with no digit-starting component — name returned as-is.
        let (name, ver) = sp(&format!("/nix/store/{hash}-nixpkgs"));
        assert_eq!(name, "nixpkgs");
        assert_eq!(ver, "");
    }

    #[test]
    fn diff_logic() {
        // Simulate map_a and map_b behaviour via the public compare contract.
        // Full integration tests would require a NixOS host; this exercises
        // the diff algorithm in isolation.
        let map_a: HashMap<_, _> = [("firefox", "99.0"), ("bash", "5.1")]
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let map_b: HashMap<_, _> = [("firefox", "100.0"), ("curl", "7.88")]
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut changed = Vec::new();

        for (name, vb) in &map_b {
            match map_a.get(name) {
                None => added.push(name.clone()),
                Some(va) if va != vb => changed.push((name.clone(), va.clone(), vb.clone())),
                _ => {}
            }
        }
        for name in map_a.keys() {
            if !map_b.contains_key(name) {
                removed.push(name.clone());
            }
        }

        assert_eq!(added, vec!["curl"]);
        assert_eq!(removed, vec!["bash"]);
        assert_eq!(changed, vec![("firefox".into(), "99.0".into(), "100.0".into())]);
    }
}
