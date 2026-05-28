//! Full package metadata retrieval via `nix eval --json`.

use std::path::Path;

use serde::Deserialize;
use serde_json::Value;
use tokio::process::Command;

use crate::packages::types::{Package, PackageError, PackageSource};

// Internal JSON shape for `nix eval <flake>#nixpkgs.<name>.meta --json`

#[derive(Deserialize)]
struct NixMeta {
    description: Option<String>,
    homepage: Option<Value>,
    license: Option<Value>,
}

// Helpers

/// Extract a plain string from a `homepage` field that may be a string or a
/// list of strings (some packages provide multiple homepages).
fn extract_homepage(val: &Value) -> Option<String> {
    match val {
        Value::String(s) => Some(s.clone()),
        Value::Array(items) => items
            .first()
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        _ => None,
    }
}

/// Extract a human-readable licence identifier from the polymorphic `license`
/// field in nix package metadata.
///
/// The field can be:
/// - a plain string (`"MIT"`)
/// - an object with `spdxId` / `fullName` keys
/// - a list of either of the above
fn extract_license(val: &Value) -> Option<String> {
    match val {
        Value::String(s) => Some(s.clone()),
        Value::Object(map) => map
            .get("spdxId")
            .or_else(|| map.get("fullName"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        Value::Array(items) => items.first().and_then(extract_license),
        _ => None,
    }
}

// Public API

/// Retrieve full metadata for the package named `name` from nixpkgs.
///
/// Runs:
/// ```text
/// nix eval <flake_path>#nixpkgs.<name>.meta --json
/// ```
/// and parses the output into a [`Package`].
pub async fn get(flake_path: &Path, name: &str) -> Result<Package, PackageError> {
    let eval_expr = format!("{}#nixpkgs.{}.meta", flake_path.display(), name);

    let output = Command::new("nix")
        .args(["eval", &eval_expr, "--json"])
        .output()
        .await
        .map_err(|e| PackageError::NixCommandFailed(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        // Treat "attribute not found" errors as NotFound rather than a
        // generic command failure so callers can distinguish the two cases.
        if stderr.contains("does not provide attribute")
            || stderr.contains("error: attribute '")
        {
            return Err(PackageError::NotFound(name.to_string()));
        }
        return Err(PackageError::NixCommandFailed(stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let meta: NixMeta = serde_json::from_str(&stdout)
        .map_err(|e| PackageError::ParseError(e.to_string()))?;

    Ok(Package {
        name: name.to_string(),
        // `nix eval … .meta` does not include the version; callers that need
        // the version should combine this with the output of `nix search`.
        version: String::new(),
        description: meta.description.unwrap_or_default(),
        homepage: meta.homepage.as_ref().and_then(extract_homepage),
        license: meta.license.as_ref().and_then(extract_license),
        source: PackageSource::System,
    })
}
