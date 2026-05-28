use std::path::Path;

use crate::workspace::detect::get_hostname;
use crate::workspace::types::{OwnershipInfo, Workspace, WorkspaceError, WorkspaceKind};

// ── Flake template ────────────────────────────────────────────────────────────

/// Return the text of a minimal, valid `flake.nix` with `HOSTNAME` replaced by
/// the provided hostname string.
pub fn flake_template(hostname: &str) -> String {
    FLAKE_TEMPLATE.replace("HOSTNAME", hostname)
}

/// Template source.  `HOSTNAME` is a placeholder replaced at call time.
const FLAKE_TEMPLATE: &str = r#"{
  description = "NixOS configuration";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }: {
    nixosConfigurations = {
      HOSTNAME = nixpkgs.lib.nixosSystem {
        system = "x86_64-linux";
        modules = [
          ./configuration.nix
        ];
      };
    };
  };
}
"#;

/// A minimal `configuration.nix` stub written alongside the generated flake.
const CONFIGURATION_STUB: &str = r#"{ config, pkgs, ... }:

{
  # Edit this file to configure your NixOS system.
  # See https://nixos.org/manual/nixos/stable/ for documentation.

  # Set the system state version.  Changing this value does NOT upgrade your
  # system; see the NixOS manual for what it controls.
  system.stateVersion = "24.05";
}
"#;

// ── Public API ────────────────────────────────────────────────────────────────

/// Create a new flake-based NixOS workspace at `path`.
///
/// This function:
/// 1. Creates `path` (and any missing parent directories).
/// 2. Writes a minimal `flake.nix` using [`flake_template`].
/// 3. Writes a stub `configuration.nix`.
/// 4. Returns the newly created [`Workspace`].
///
/// Returns an error if the directory cannot be created or the files cannot be
/// written (e.g. due to permission issues).
pub fn create_flake_workspace(path: &Path) -> Result<Workspace, WorkspaceError> {
    // 1. Create the target directory.
    std::fs::create_dir_all(path)?;

    // 2. Determine hostname for the flake template.
    let hostname = get_hostname();

    // 3. Write flake.nix.
    let flake_content = flake_template(&hostname);
    std::fs::write(path.join("flake.nix"), &flake_content)?;

    // 4. Write a stub configuration.nix so the flake import resolves.
    std::fs::write(path.join("configuration.nix"), CONFIGURATION_STUB)?;

    // 5. Build ownership info for the new directory.
    let owner = ownership_info_for_new_dir();

    Ok(Workspace {
        path: path.to_path_buf(),
        kind: WorkspaceKind::Flake,
        owner,
        hostname,
    })
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Return [`OwnershipInfo`] appropriate for a directory we just created (i.e.
/// owned by the current process's uid).
fn ownership_info_for_new_dir() -> OwnershipInfo {
    #[cfg(unix)]
    {
        let uid = current_uid();
        OwnershipInfo {
            is_user_owned: true,
            uid,
        }
    }

    #[cfg(not(unix))]
    OwnershipInfo {
        is_user_owned: true,
        uid: 0,
    }
}

/// Return the POSIX uid of the running process by parsing `/proc/self/status`.
#[cfg(unix)]
fn current_uid() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Uid:"))
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

    #[test]
    fn flake_template_contains_hostname() {
        let t = flake_template("myhost");
        assert!(t.contains("myhost"), "template should embed the hostname");
        assert!(!t.contains("HOSTNAME"), "placeholder should be replaced");
    }

    #[test]
    fn flake_template_is_valid_nix_syntax_heuristic() {
        let t = flake_template("testhost");
        // Basic structural checks — not a full Nix parse, but catches obvious
        // breakage.
        assert!(t.contains("nixpkgs.url"));
        assert!(t.contains("nixosConfigurations"));
        assert!(t.contains("nixosSystem"));
        assert!(t.contains("configuration.nix"));
    }

    #[test]
    fn create_flake_workspace_writes_expected_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ws_path = dir.path().join("nixos-config");

        let ws = create_flake_workspace(&ws_path).expect("create_flake_workspace");

        assert_eq!(ws.path, ws_path);
        assert_eq!(ws.kind, WorkspaceKind::Flake);
        assert!(ws.owner.is_user_owned);

        let flake_nix = ws_path.join("flake.nix");
        assert!(flake_nix.exists(), "flake.nix should be created");
        let content = fs::read_to_string(&flake_nix).expect("read flake.nix");
        assert!(content.contains("nixosSystem"));

        let config_nix = ws_path.join("configuration.nix");
        assert!(config_nix.exists(), "configuration.nix should be created");
    }

    #[test]
    fn create_flake_workspace_creates_nested_directories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let nested = dir.path().join("a/b/c/nixos");

        create_flake_workspace(&nested).expect("nested create_flake_workspace");
        assert!(nested.join("flake.nix").exists());
    }
}
