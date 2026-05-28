use std::path::Path;

use clap::Subcommand;
use nixman_core::intent::types::{ChangePlan, ProposedChange};

#[derive(Subcommand)]
pub enum IntentCmd {
    /// Propose changes and get a validated plan
    Propose {
        /// Options to set (format: path=value)
        #[arg(long = "set", value_name = "PATH=VALUE")]
        sets: Vec<String>,

        /// Packages to add to environment.systemPackages
        #[arg(long = "add-package")]
        add_packages: Vec<String>,

        /// Packages to remove from environment.systemPackages
        #[arg(long = "remove-package")]
        remove_packages: Vec<String>,
    },
    /// Show the last proposed plan
    Show,
    /// Apply the last proposed plan
    Apply,
    /// Discard the current plan
    Discard,
}

pub async fn run(
    cmd: IntentCmd,
    workspace: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    match cmd {
        IntentCmd::Propose {
            sets,
            add_packages,
            remove_packages,
        } => {
            let mut changes = Vec::new();

            // Parse --set path=value arguments
            for set_arg in &sets {
                let (path, value) = set_arg
                    .split_once('=')
                    .ok_or_else(|| format!("Invalid --set format '{}'. Expected path=value", set_arg))?;
                changes.push(ProposedChange {
                    path: path.trim().to_string(),
                    value: value.trim().to_string(),
                    reason: None,
                });
            }

            // Handle --add-package as appending to environment.systemPackages
            for pkg in &add_packages {
                changes.push(ProposedChange {
                    path: "environment.systemPackages".to_string(),
                    value: format!("__append:{}", pkg),
                    reason: Some(format!("Install package: {}", pkg)),
                });
            }

            // Handle --remove-package
            for pkg in &remove_packages {
                changes.push(ProposedChange {
                    path: "environment.systemPackages".to_string(),
                    value: format!("__remove:{}", pkg),
                    reason: Some(format!("Remove package: {}", pkg)),
                });
            }

            if changes.is_empty() {
                return Err(
                    "No changes specified. Use --set, --add-package, or --remove-package".into(),
                );
            }

            let hostname = detect_hostname();

            let plan = nixman_core::intent::propose(workspace, &hostname, changes)
                .await
                .map_err(|e| format!("Intent engine error: {e}"))?;

            save_plan(workspace, &plan)?;

            Ok(serde_json::to_string_pretty(&plan)?)
        }
        IntentCmd::Show => {
            let plan = load_plan(workspace)?;
            Ok(serde_json::to_string_pretty(&plan)?)
        }
        IntentCmd::Apply => {
            let plan = load_plan(workspace)?;
            if !plan.valid {
                return Err("Cannot apply: plan has unresolved conflicts".into());
            }

            let mut pending = nixman_core::config::PendingChanges::new();

            for change in &plan.changes {
                if let Some(pkg) = change.value.strip_prefix("__append:") {
                    let _ = nixman_core::packages::manage::add(workspace, pkg, None)
                        .map_err(|e| format!("Failed to add package {pkg}: {e}"))?;
                    continue;
                }
                if let Some(pkg) = change.value.strip_prefix("__remove:") {
                    nixman_core::packages::manage::remove(workspace, pkg, None)
                        .map_err(|e| format!("Failed to remove package {pkg}: {e}"))?;
                    continue;
                }
                let nix_value = crate::value_parser::parse_nix_value(&change.value);
                nixman_core::config::editor::set_value(
                    &mut pending,
                    workspace,
                    &change.path,
                    nix_value,
                )
                .map_err(|e| format!("Failed to set {}: {e}", change.path))?;
            }

            nixman_core::config::editor::apply_pending(&mut pending, workspace)
                .map_err(|e| format!("Failed to write changes: {e}"))?;

            remove_plan(workspace);
            Ok("Plan applied successfully. Run 'nixman rebuild' to activate."
                .to_string())
        }
        IntentCmd::Discard => {
            remove_plan(workspace);
            Ok("Plan discarded.".to_string())
        }
    }
}

/// Detect hostname from /etc/hostname, falling back to "nixos".
fn detect_hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "nixos".to_string())
}

/// Save the plan to `.nixman-plan.json` in the workspace.
fn save_plan(workspace: &Path, plan: &ChangePlan) -> Result<(), Box<dyn std::error::Error>> {
    let plan_path = workspace.join(".nixman-plan.json");
    let json = serde_json::to_string_pretty(plan)?;
    std::fs::write(&plan_path, json)?;
    Ok(())
}

/// Load the plan from `.nixman-plan.json`.
fn load_plan(workspace: &Path) -> Result<ChangePlan, Box<dyn std::error::Error>> {
    let plan_path = workspace.join(".nixman-plan.json");
    if !plan_path.exists() {
        return Err("No pending plan. Run 'intent propose' first.".into());
    }
    let json = std::fs::read_to_string(&plan_path)?;
    let plan: ChangePlan = serde_json::from_str(&json)?;
    Ok(plan)
}

/// Remove the plan file, ignoring errors (file may not exist).
fn remove_plan(workspace: &Path) {
    let plan_path = workspace.join(".nixman-plan.json");
    let _ = std::fs::remove_file(plan_path);
}
