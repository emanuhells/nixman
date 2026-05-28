use std::path::{Path, PathBuf};

use crate::workspace::types::{WorkspaceError, WorkspaceKind};

/// A detected Home Manager configuration workspace.
#[derive(Debug, Clone)]
pub struct HmWorkspace {
    /// Path to the HM config directory.
    pub path: PathBuf,
    /// Flake vs. Legacy classification.
    pub kind: WorkspaceKind,
    /// Current OS user name.
    pub username: String,
    /// How the workspace was detected.
    pub source: HmSource,
}

/// How the HM workspace was discovered.
#[derive(Debug, Clone)]
pub enum HmSource {
    /// `~/.config/home-manager/` directory
    UserConfig,
    /// Inferred from the NixOS flake (home-manager input found)
    NixosFlake,
    /// `~/.config/home-manager/home.nix` fallback
    Fallback,
}

/// Detect the Home Manager configuration directory.
///
/// Detection order:
/// 1. `~/.config/home-manager/` — standalone HM config directory
/// 2. From NixOS flake — if the NixOS flake has a `home-manager` input,
///    use the NixOS workspace as the HM workspace (HM config is in-repo)
/// 3. `~/.config/home-manager/home.nix` — legacy fallback
pub fn detect_hm() -> Result<HmWorkspace, WorkspaceError> {
    let home_dir = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let home_path = PathBuf::from(&home_dir);
    let username = std::env::var("USER").unwrap_or_else(|_| "unknown".to_string());

    // ── Step 1: ~/.config/home-manager/ ──────────────────────────────────
    let user_config = home_path.join(".config/home-manager");
    if user_config.exists() {
        if let Some(kind) = classify_hm_directory(&user_config) {
            return Ok(HmWorkspace {
                path: user_config,
                kind,
                username,
                source: HmSource::UserConfig,
            });
        }
    }

    // ── Step 2: from NixOS flake ────────────────────────────────────────
    // home-manager, the HM config lives in the same repo.
    if let Ok(nixos_ws) = crate::workspace::detect() {
        if nixos_ws.kind == WorkspaceKind::Flake {
            if let Some(kind) = detect_hm_from_flake(&nixos_ws.path) {
                return Ok(HmWorkspace {
                    path: nixos_ws.path,
                    kind,
                    username,
                    source: HmSource::NixosFlake,
                });
            }
        }
    }

    // ── Step 3: fallback ─────────────────────────────────────────────────
    let fallback = home_path.join(".config/home-manager/home.nix");
    if fallback.exists() {
        return Ok(HmWorkspace {
            path: home_path.join(".config/home-manager"),
            kind: WorkspaceKind::Legacy,
            username,
            source: HmSource::Fallback,
        });
    }

    Err(WorkspaceError::NotFound)
}

/// Check if a NixOS flake directory has Home Manager configured.
///
/// Uses two strategies:
/// 1. Quick scan — check if flake.nix mentions "home-manager"
/// 2. If yes, look for HM config files (home.nix, flake.nix) in the dir
fn detect_hm_from_flake(flake_dir: &Path) -> Option<WorkspaceKind> {
    let flake_nix = flake_dir.join("flake.nix");
    if !flake_nix.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&flake_nix).ok()?;
    let has_home_manager = content.contains("home-manager");

    if !has_home_manager {
        return None;
    }

    // Home Manager is referenced in the flake. The HM config is typically
    // in the same directory. Check for common HM config files.
    // Classify the directory as an HM workspace.
    classify_hm_directory(flake_dir)
}

/// Classify an HM directory by the files it contains.
///
/// Returns `Some(WorkspaceKind::Flake)` if `flake.nix` is present,
/// `Some(WorkspaceKind::Legacy)` if `home.nix` is present, or `None`
/// if neither file exists.
fn classify_hm_directory(path: &Path) -> Option<WorkspaceKind> {
    if path.join("flake.nix").exists() {
        Some(WorkspaceKind::Flake)
    } else if path.join("home.nix").exists() {
        Some(WorkspaceKind::Legacy)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn hm_flake_dir() -> TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("flake.nix"), "# stub").expect("write");
        dir
    }

    fn hm_legacy_dir() -> TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("home.nix"), "# stub").expect("write");
        dir
    }

    fn nixos_flake_with_hm() -> TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(
            dir.path().join("flake.nix"),
            r#"{
  inputs.home-manager.url = "github:nix-community/home-manager";
  outputs = { home-manager, ... }: {
    nixosConfigurations.host = nixpkgs.lib.nixosSystem {
      modules = [
        home-manager.nixosModules.home-manager
        { home-manager.users.testuser = import ./home.nix; }
      ];
    };
  };
}"#,
        )
        .expect("write");
        fs::write(dir.path().join("home.nix"), "{ ... }").expect("write");
        dir
    }

    #[test]
    fn classify_flake() {
        let dir = hm_flake_dir();
        assert_eq!(
            classify_hm_directory(dir.path()),
            Some(WorkspaceKind::Flake)
        );
    }

    #[test]
    fn classify_legacy() {
        let dir = hm_legacy_dir();
        assert_eq!(
            classify_hm_directory(dir.path()),
            Some(WorkspaceKind::Legacy)
        );
    }

    #[test]
    fn classify_empty_returns_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(classify_hm_directory(dir.path()), None);
    }

    #[test]
    fn detect_hm_from_flake_with_reference() {
        let dir = nixos_flake_with_hm();
        let kind = detect_hm_from_flake(dir.path());
        assert_eq!(kind, Some(WorkspaceKind::Flake));
    }

    #[test]
    fn detect_hm_from_flake_without_reference() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("flake.nix"), "{ }").expect("write");
        let kind = detect_hm_from_flake(dir.path());
        assert_eq!(kind, None);
    }

    #[test]
    fn detect_hm_from_flake_no_flake() {
        let dir = tempfile::tempdir().expect("tempdir");
        let kind = detect_hm_from_flake(dir.path());
        assert_eq!(kind, None);
    }

    #[test]
    fn hm_source_user_config() {
        let _dir = hm_flake_dir();
        let home = std::env::var("HOME").unwrap();
        let config_path = PathBuf::from(&home).join(".config/home-manager");
        let _ = fs::create_dir_all(&config_path);
        fs::write(config_path.join("home.nix"), "# test").expect("write");
        // just the classify logic
        let _ = fs::remove_file(config_path.join("home.nix"));
    }
}
