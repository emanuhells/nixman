//! Package search via `nix search --json`.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use tokio::process::Command;

use crate::packages::types::{Package, PackageError, PackageSource, SearchResult};

// Internal JSON shape produced by `nix search --json`

/// One entry in the JSON map returned by `nix search --json`.
///
/// Keys in the map are attribute paths such as
/// `legacyPackages.x86_64-linux.firefox`; the values have this shape.
#[derive(Deserialize)]
struct NixSearchEntry {
    /// Package name (may differ from the attribute name).
    pname: Option<String>,
    version: Option<String>,
    description: Option<String>,
}

// Public API

/// Search nixpkgs for packages matching `query`.
///
/// Runs:
/// ```text
/// nix search <flake_path>#nixpkgs <query> --json
/// ```
/// and parses the JSON output into a [`SearchResult`].
pub async fn query(flake_path: &Path, query: &str) -> Result<SearchResult, PackageError> {
    let flake_ref = format!("{}#nixpkgs", flake_path.display());

    let output = Command::new("nix")
        .args(["search", &flake_ref, query, "--json"])
        .output()
        .await
        .map_err(|e| PackageError::NixCommandFailed(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(PackageError::NixCommandFailed(stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let entries: HashMap<String, NixSearchEntry> = serde_json::from_str(&stdout)
        .map_err(|e| PackageError::ParseError(format!("{}: {}", e, &stdout[..stdout.len().min(200)])))?;

    let packages: Vec<Package> = entries
        .into_iter()
        .map(|(attr_path, entry)| {
            // Derive a display name: prefer `pname`, fall back to the last
            // component of the attribute path (e.g. "firefox" from
            // "legacyPackages.x86_64-linux.firefox").
            let name = entry
                .pname
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| {
                    attr_path
                        .split('.')
                        .next_back()
                        .unwrap_or(&attr_path)
                        .to_string()
                });

            Package {
                name,
                version: entry.version.unwrap_or_default(),
                description: entry.description.unwrap_or_default(),
                homepage: None,
                license: None,
                source: PackageSource::System,
            }
        })
        .collect();

    let total = packages.len();

    Ok(SearchResult {
        packages,
        query: query.to_string(),
        total,
    })
}
