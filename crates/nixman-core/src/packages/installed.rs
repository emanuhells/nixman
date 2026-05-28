//! Installed package listing from the NixOS configuration.

use std::path::Path;

use crate::packages::manage;
use crate::packages::types::{Package, PackageError, PackageSource};

// Public API

/// Return the list of packages declared in the NixOS configuration at
/// `workspace_path`.
///
/// Reads `environment.systemPackages` from the NixOS configuration file(s)
/// under `workspace_path` by delegating to [`manage::list_installed`] and
/// converting the results to [`Package`] structs.
pub async fn list(workspace_path: &Path) -> Result<Vec<Package>, PackageError> {
    let names = manage::list_installed(workspace_path)?;

    let packages: Vec<Package> = names
        .into_iter()
        .map(|name| Package {
            name,
            version: String::new(),
            description: String::new(),
            homepage: None,
            license: None,
            source: PackageSource::System,
        })
        .collect();

    Ok(packages)
}
