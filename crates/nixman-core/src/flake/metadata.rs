//! Parse `flake.lock` to extract direct flake input metadata.

use std::path::Path;

use chrono::{DateTime, Utc};

use super::types::{FlakeError, FlakeInput};

/// Read `flake.lock` at `workspace_path` and return metadata for every direct
/// input (i.e. inputs listed under the root node — transitive inputs are
/// excluded).
///
/// # Errors
///
/// - [`FlakeError::LockNotFound`] — no `flake.lock` exists at `workspace_path`.
/// - [`FlakeError::ParseError`] — the file is not valid JSON or is missing
///   required fields.
/// - [`FlakeError::IoError`] — a file-system error occurred while reading.
pub fn list_inputs(workspace_path: &Path) -> Result<Vec<FlakeInput>, FlakeError> {
    let lock_path = workspace_path.join("flake.lock");
    if !lock_path.exists() {
        return Err(FlakeError::LockNotFound);
    }

    let content = std::fs::read_to_string(&lock_path)?;
    let lock: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| FlakeError::ParseError(e.to_string()))?;

    let nodes = lock
        .get("nodes")
        .and_then(|n| n.as_object())
        .ok_or_else(|| FlakeError::ParseError("missing \"nodes\" field".into()))?;

    // Identify the root node key (defaults to "root" per the Nix lockfile spec).
    let root_key = lock
        .get("root")
        .and_then(|r| r.as_str())
        .unwrap_or("root");

    let root_node = nodes
        .get(root_key)
        .and_then(|n| n.as_object())
        .ok_or_else(|| FlakeError::ParseError("missing root node".into()))?;

    // If the root node has no inputs (unlikely but valid), return empty.
    let root_inputs = match root_node.get("inputs").and_then(|i| i.as_object()) {
        Some(inputs) => inputs,
        None => return Ok(Vec::new()),
    };

    let mut inputs = Vec::new();
    for (name, target) in root_inputs {
        // Targets can be a string (input name alias) or an array (path-style).
        // Ignore non-string targets — they are unusual and have no "locked" node.
        let target_key = match target.as_str() {
            Some(k) => k,
            None => continue,
        };

        if let Some(node) = nodes.get(target_key).and_then(|n| n.as_object()) {
            if let Some(locked) = node.get("locked").and_then(|l| l.as_object()) {
                inputs.push(parse_locked_input(name, locked));
            }
        }
    }

    Ok(inputs)
}

/// Build a [`FlakeInput`] from the `locked` object of a flake node.
fn parse_locked_input(
    name: &str,
    locked: &serde_json::Map<String, serde_json::Value>,
) -> FlakeInput {
    let input_type = locked.get("type").and_then(|v| v.as_str()).unwrap_or("unknown");
    let owner = locked.get("owner").and_then(|v| v.as_str()).unwrap_or("");
    let repo = locked.get("repo").and_then(|v| v.as_str()).unwrap_or("");
    let url_ref = locked.get("ref").and_then(|v| v.as_str()).unwrap_or("");

    let url = match input_type {
        "github" => {
            if url_ref.is_empty() {
                format!("github:{owner}/{repo}")
            } else {
                format!("github:{owner}/{repo}/{url_ref}")
            }
        }
        "git" => locked
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        _ => format!("{input_type}:{owner}/{repo}"),
    };

    let rev = locked
        .get("rev")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let nar_hash = locked
        .get("narHash")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let last_modified = locked
        .get("lastModified")
        .and_then(|v| v.as_i64())
        .and_then(|ts| DateTime::from_timestamp(ts, 0))
        .unwrap_or_else(Utc::now);

    FlakeInput {
        name: name.to_string(),
        url,
        rev,
        last_modified,
        nar_hash,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    const SAMPLE_LOCK: &str = r#"{
  "nodes": {
    "nixpkgs": {
      "locked": {
        "lastModified": 1715000000,
        "narHash": "sha256-abc123",
        "owner": "NixOS",
        "repo": "nixpkgs",
        "rev": "deadbeef1234",
        "type": "github"
      },
      "original": {
        "owner": "NixOS",
        "ref": "nixos-unstable",
        "repo": "nixpkgs",
        "type": "github"
      }
    },
    "home-manager": {
      "locked": {
        "lastModified": 1714000000,
        "narHash": "sha256-def456",
        "owner": "nix-community",
        "repo": "home-manager",
        "rev": "cafebabe5678",
        "type": "github"
      },
      "original": {
        "owner": "nix-community",
        "repo": "home-manager",
        "type": "github"
      }
    },
    "root": {
      "inputs": {
        "nixpkgs": "nixpkgs",
        "home-manager": "home-manager"
      }
    }
  },
  "root": "root",
  "version": 7
}"#;

    fn write_lock(dir: &TempDir, content: &str) {
        fs::write(dir.path().join("flake.lock"), content).unwrap();
    }

    #[test]
    fn test_list_inputs_parses_direct_inputs() {
        let dir = TempDir::new().unwrap();
        write_lock(&dir, SAMPLE_LOCK);

        let inputs = list_inputs(dir.path()).unwrap();
        assert_eq!(inputs.len(), 2);

        let nixpkgs = inputs.iter().find(|i| i.name == "nixpkgs").unwrap();
        assert_eq!(nixpkgs.rev, "deadbeef1234");
        assert_eq!(nixpkgs.url, "github:NixOS/nixpkgs");
        assert_eq!(nixpkgs.nar_hash, "sha256-abc123");
        assert_eq!(nixpkgs.last_modified.timestamp(), 1715000000);

        let hm = inputs.iter().find(|i| i.name == "home-manager").unwrap();
        assert_eq!(hm.rev, "cafebabe5678");
        assert_eq!(hm.url, "github:nix-community/home-manager");
    }

    #[test]
    fn test_list_inputs_missing_lock() {
        let dir = TempDir::new().unwrap();
        let result = list_inputs(dir.path());
        assert!(matches!(result, Err(FlakeError::LockNotFound)));
    }

    #[test]
    fn test_list_inputs_empty_root_inputs() {
        let dir = TempDir::new().unwrap();
        let lock = r#"{
  "nodes": {
    "root": {}
  },
  "root": "root",
  "version": 7
}"#;
        write_lock(&dir, lock);
        let inputs = list_inputs(dir.path()).unwrap();
        assert!(inputs.is_empty());
    }

    #[test]
    fn test_list_inputs_url_with_ref() {
        let dir = TempDir::new().unwrap();
        let lock = r#"{
  "nodes": {
    "nixpkgs": {
      "locked": {
        "lastModified": 1715000000,
        "narHash": "sha256-xyz",
        "owner": "NixOS",
        "ref": "nixos-24.05",
        "repo": "nixpkgs",
        "rev": "aabbcc",
        "type": "github"
      },
      "original": {
        "owner": "NixOS",
        "ref": "nixos-24.05",
        "repo": "nixpkgs",
        "type": "github"
      }
    },
    "root": {
      "inputs": { "nixpkgs": "nixpkgs" }
    }
  },
  "root": "root",
  "version": 7
}"#;
        write_lock(&dir, lock);
        let inputs = list_inputs(dir.path()).unwrap();
        assert_eq!(inputs[0].url, "github:NixOS/nixpkgs/nixos-24.05");
    }
}
