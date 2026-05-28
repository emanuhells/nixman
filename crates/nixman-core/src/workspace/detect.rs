use std::path::{Path, PathBuf};

use crate::workspace::types::{OwnershipInfo, Workspace, WorkspaceError, WorkspaceKind};

// ── Public entry point ────────────────────────────────────────────────────────

/// Walk the standard detection chain and return the first valid NixOS
/// configuration workspace found.
///
/// Detection order:
/// 1. `/etc/nixos` — resolve any symlink(s) to the real directory.
/// 2. `$HOME/nix-config`
/// 3. `$HOME/.config/nixos`
/// 4. [`WorkspaceError::NotFound`] if none of the above yielded a workspace.
pub fn detect() -> Result<Workspace, WorkspaceError> {
    let hostname = get_hostname();

    // ── Step 1: /etc/nixos (with symlink resolution) ──────────────────────
    let etc_nixos = Path::new("/etc/nixos");
    if etc_nixos.exists() {
        // Resolve the directory itself (common on NixOS where /etc/nixos is a
        // symlink pointing into a user's home directory).
        let real_path = resolve_symlink(etc_nixos);
        if let Some(kind) = classify_directory(&real_path) {
            let owner = ownership_info(&real_path)?;
            return Ok(Workspace {
                path: real_path,
                kind,
                owner,
                hostname,
            });
        }
    }

    // ── Steps 2-3: common user-owned directories ──────────────────────────
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let home_path = PathBuf::from(home);

    let candidates = [
        home_path.join("nix-config"),
        home_path.join(".config/nixos"),
    ];

    for candidate in &candidates {
        if candidate.exists() {
            if let Some(kind) = classify_directory(candidate) {
                let owner = ownership_info(candidate)?;
                return Ok(Workspace {
                    path: candidate.clone(),
                    kind,
                    owner,
                    hostname,
                });
            }
        }
    }

    // ── Step 4: nothing found ─────────────────────────────────────────────
    Err(WorkspaceError::NotFound)
}

// ── Hostname helpers ──────────────────────────────────────────────────────────

/// Return the machine hostname.
///
/// Tries `/etc/hostname` first; falls back to the `hostname` binary; returns
/// `"unknown"` if both fail.
pub fn get_hostname() -> String {
    // Primary: /etc/hostname (always present on NixOS)
    if let Ok(content) = std::fs::read_to_string("/etc/hostname") {
        let trimmed = content.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }

    // Fallback: run the `hostname` binary
    if let Ok(output) = std::process::Command::new("hostname").output() {
        if let Ok(s) = std::str::from_utf8(&output.stdout) {
            let trimmed = s.trim().to_string();
            if !trimmed.is_empty() {
                return trimmed;
            }
        }
    }

    "unknown".to_string()
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Classify a directory by the NixOS files it contains.
///
/// Returns `Some(WorkspaceKind::Flake)` if `flake.nix` is present,
/// `Some(WorkspaceKind::Legacy)` if only `configuration.nix` is present, or
/// `None` if neither file exists.
fn classify_directory(path: &Path) -> Option<WorkspaceKind> {
    if path.join("flake.nix").exists() {
        Some(WorkspaceKind::Flake)
    } else if path.join("configuration.nix").exists() {
        Some(WorkspaceKind::Legacy)
    } else {
        None
    }
}

/// Recursively follow symlinks for `path` until a non-symlink is reached,
/// then return that final path.
///
/// Relative symlink targets are resolved relative to the symlink's parent
/// directory.  A cycle-detection limit of 40 hops (matching the Linux kernel's
/// `MAXSYMLINKS`) prevents infinite loops.
pub fn resolve_symlink(path: &Path) -> PathBuf {
    let mut current = path.to_path_buf();
    const MAX_HOPS: usize = 40;

    for _ in 0..MAX_HOPS {
        match std::fs::read_link(&current) {
            Ok(target) => {
                if target.is_absolute() {
                    current = target;
                } else {
                    // Relative symlink — resolve against the link's parent.
                    let parent = current
                        .parent()
                        .unwrap_or_else(|| Path::new("/"))
                        .to_path_buf();
                    current = parent.join(target);
                }
            }
            // Not a symlink (or an error reading it): we have reached the end.
            Err(_) => break,
        }
    }

    current
}

/// Build an [`OwnershipInfo`] for `path` by inspecting its filesystem metadata.
fn ownership_info(path: &Path) -> Result<OwnershipInfo, WorkspaceError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let meta = std::fs::metadata(path)?;
        let uid = meta.uid();

        Ok(OwnershipInfo {
            is_user_owned: uid == current_uid(),
            uid,
        })
    }

    // Non-Unix platforms: ownership checking is not meaningful here; return a
    // safe default so the module still compiles cross-platform.
    #[cfg(not(unix))]
    {
        let _ = path; // suppress unused-variable warning
        Ok(OwnershipInfo {
            is_user_owned: true,
            uid: 0,
        })
    }
}

/// Return the POSIX uid of the currently running process.
///
/// Reads `/proc/self/status` (always available on Linux) and parses the `Uid:`
/// line.  Falls back to `0` if parsing fails.
#[cfg(unix)]
fn current_uid() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Uid:"))
                // Format: "Uid:\treal\teffective\tsaved\tfilesystem"
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(0)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // Helper: create a temp dir with a flake.nix inside.
    fn flake_dir() -> TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("flake.nix"), "# stub").expect("write");
        dir
    }

    // Helper: create a temp dir with a configuration.nix inside.
    fn legacy_dir() -> TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("configuration.nix"), "# stub").expect("write");
        dir
    }

    #[test]
    fn classify_flake() {
        let dir = flake_dir();
        assert_eq!(classify_directory(dir.path()), Some(WorkspaceKind::Flake));
    }

    #[test]
    fn classify_legacy() {
        let dir = legacy_dir();
        assert_eq!(
            classify_directory(dir.path()),
            Some(WorkspaceKind::Legacy)
        );
    }

    #[test]
    fn classify_empty_returns_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(classify_directory(dir.path()), None);
    }

    #[test]
    fn resolve_non_symlink_is_identity() {
        let dir = tempfile::tempdir().expect("tempdir");
        let resolved = resolve_symlink(dir.path());
        assert_eq!(resolved, dir.path());
    }

    #[cfg(unix)]
    #[test]
    fn resolve_symlink_follows_link() {
        let target_dir = flake_dir();
        let link_dir = tempfile::tempdir().expect("tempdir");
        let link_path = link_dir.path().join("link");
        std::os::unix::fs::symlink(target_dir.path(), &link_path).expect("symlink");

        let resolved = resolve_symlink(&link_path);
        assert_eq!(resolved, target_dir.path());
    }
}
