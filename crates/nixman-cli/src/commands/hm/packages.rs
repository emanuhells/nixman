use clap::Subcommand;
use std::path::Path;

use nixman_core::packages::types::PackageError;

/// Options for `home.packages` list operations.
const HM_OPTION_PATH: &str = "home.packages";
/// Entry file for HM configuration.
const HM_ENTRY_FILE: &str = "home.nix";

#[derive(Subcommand)]
pub enum HmPackagesCmd {
    /// List packages declared in home.packages
    List,
    /// Search nixpkgs for packages matching a query
    Search { query: String },
    /// Add a package to home.packages
    Add {
        name: String,
        /// Skip package name verification against nixpkgs
        #[arg(long)]
        no_verify: bool,
        /// Target file for the package (overrides auto-detection)
        #[arg(long)]
        file: Option<String>,
        /// Show what would change without writing
        #[arg(long)]
        dry_run: bool,
        /// Stage change instead of applying immediately
        #[arg(long)]
        stage: bool,
    },
    /// Remove a package from home.packages
    Remove {
        name: String,
        /// Target file for the package (overrides auto-detection)
        #[arg(long)]
        file: Option<String>,
        /// Show what would change without writing
        #[arg(long)]
        dry_run: bool,
        /// Stage change instead of applying immediately
        #[arg(long)]
        stage: bool,
    },
}

pub async fn run(cmd: HmPackagesCmd) -> Result<String, Box<dyn std::error::Error>> {
    let ws = nixman_core::workspace::hm::detect_hm()?;
    let workspace = &ws.path;

    match cmd {
        HmPackagesCmd::List => {
            let packages = nixman_core::packages::manage::list_installed_in_list(
                workspace,
                HM_OPTION_PATH,
                HM_ENTRY_FILE,
            )?;
            if packages.is_empty() {
                Ok("No packages found in home.packages.".to_string())
            } else {
                Ok(serde_json::to_string_pretty(&packages)?)
            }
        }
        HmPackagesCmd::Search { query } => {
            let results = nixman_core::packages::search::query(workspace, &query).await?;
            Ok(serde_json::to_string_pretty(&serde_json::json!({
                "query": results.query,
                "total": results.total,
                "packages": results.packages,
            }))?)
        }
        HmPackagesCmd::Add { name, no_verify, file, dry_run, stage } => {
            if !no_verify {
                match nixman_core::packages::manage::verify_package(&name) {
                    Ok(()) => {}
                    Err(PackageError::NixNotAvailable) => {
                        eprintln!("Warning: cannot verify package name (nix not available)");
                    }
                    Err(e) => return Err(e.into()),
                }
            }

            if stage {
                let mut staged = crate::pending_store::StagedChanges::load(workspace);
                staged.add_package_add(name.clone(), file.clone());
                staged.save(workspace).map_err(|e| format!("Failed to save staging: {}", e))?;
                let file_msg = file.as_ref().map_or(String::new(), |f| format!(" (file: {})", f));
                return Ok(format!("Staged: add package '{name}'{}", file_msg));
            }

            let target = file.as_ref().map(|f| Path::new(f.as_str()));

            if dry_run {
                return dry_run_add(workspace, &name, target);
            }

            let added = nixman_core::packages::manage::add_in_list(
                workspace, &name, target, HM_OPTION_PATH, HM_ENTRY_FILE,
            )
            .map_err(|e| format!("Failed to add package '{name}': {e}"))?;
            if added {
                Ok(format!("Package '{name}' added to home.packages."))
            } else {
                Err(crate::CliError::Noop(format!("Package '{name}' is already in home.packages.")).into())
            }
        }
        HmPackagesCmd::Remove { name, file, dry_run, stage } => {
            if stage {
                let mut staged = crate::pending_store::StagedChanges::load(workspace);
                staged.add_package_remove(name.clone(), file.clone());
                staged.save(workspace).map_err(|e| format!("Failed to save staging: {}", e))?;
                let file_msg = file.as_ref().map_or(String::new(), |f| format!(" (file: {})", f));
                return Ok(format!("Staged: remove package '{name}'{}", file_msg));
            }

            let target = file.as_ref().map(|f| Path::new(f.as_str()));

            if dry_run {
                return dry_run_remove(workspace, &name, target);
            }

            nixman_core::packages::manage::remove_from_list(
                workspace, &name, target, HM_OPTION_PATH, HM_ENTRY_FILE,
            )
            .map_err(|e| format!("Failed to remove package '{name}': {e}"))?;
            Ok(format!("Package '{name}' removed from home.packages."))
        }
    }
}

// Dry-run helpers with RestoreGuard

fn dry_run_add(
    workspace: &Path,
    name: &str,
    target: Option<&Path>,
) -> Result<String, Box<dyn std::error::Error>> {
    let target_file = nixman_core::packages::manage::resolve_package_file_for_list(
        workspace, target, HM_OPTION_PATH, HM_ENTRY_FILE,
    )?;

    let original = std::fs::read_to_string(&target_file)
        .map_err(|e| format!("failed to read {}: {e}", target_file.display()))?;
    let permissions = std::fs::metadata(&target_file)
        .map_err(|e| format!("failed to stat {}: {e}", target_file.display()))?
        .permissions();

    // Guard will restore the file on drop — even on panic.
    let _guard = crate::commands::packages::RestoreGuard {
        path: target_file.clone(),
        original: original.clone(),
        permissions,
    };

    let added = nixman_core::packages::manage::add_in_list(
        workspace, name, target, HM_OPTION_PATH, HM_ENTRY_FILE,
    )
    .map_err(|e| format!("Failed to add package '{name}': {e}"))?;

    if !added {
        return Ok(format!(
            "Package '{}' is already in home.packages.",
            name
        ));
    }

    let modified = std::fs::read_to_string(&target_file)
        .map_err(|e| format!("failed to read {}: {e}", target_file.display()))?;

    let output = crate::diff_util::simple_diff(
        &original,
        &modified,
        &target_file.display().to_string(),
    );
    Ok(output)
}

fn dry_run_remove(
    workspace: &Path,
    name: &str,
    target: Option<&Path>,
) -> Result<String, Box<dyn std::error::Error>> {
    let target_file = nixman_core::packages::manage::resolve_package_file_for_list(
        workspace, target, HM_OPTION_PATH, HM_ENTRY_FILE,
    )?;

    let original = std::fs::read_to_string(&target_file)
        .map_err(|e| format!("failed to read {}: {e}", target_file.display()))?;
    let permissions = std::fs::metadata(&target_file)
        .map_err(|e| format!("failed to stat {}: {e}", target_file.display()))?
        .permissions();

    // Guard will restore the file on drop — even on panic.
    let _guard = crate::commands::packages::RestoreGuard {
        path: target_file.clone(),
        original: original.clone(),
        permissions,
    };

    nixman_core::packages::manage::remove_from_list(
        workspace, name, target, HM_OPTION_PATH, HM_ENTRY_FILE,
    )
    .map_err(|e| format!("Failed to remove package '{name}': {e}"))?;

    let modified = std::fs::read_to_string(&target_file)
        .map_err(|e| format!("failed to read {}: {e}", target_file.display()))?;

    let output = crate::diff_util::simple_diff(
        &original,
        &modified,
        &target_file.display().to_string(),
    );
    Ok(output)
}
