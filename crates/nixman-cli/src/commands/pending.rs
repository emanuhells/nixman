use clap::Subcommand;
use std::path::Path;
use crate::pending_store::StagedChanges;

#[derive(Subcommand)]
pub enum PendingCmd {
    /// List all staged changes
    List,
    /// Apply all staged changes to disk
    Apply,
    /// Discard all staged changes
    Discard,
}

pub async fn run(
    cmd: PendingCmd,
    workspace: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    match cmd {
        PendingCmd::List => {
            let staged = StagedChanges::load(workspace);
            if staged.is_empty() {
                return Ok("No pending changes.".to_string());
            }
            Ok(serde_json::to_string_pretty(&staged.changes)?)
        }
        PendingCmd::Apply => {
            let staged = StagedChanges::load(workspace);
            if staged.is_empty() {
                return Ok("No pending changes to apply.".to_string());
            }
            let mut option_count = 0u32;
            let mut package_count = 0u32;
            let mut errors: Vec<String> = Vec::new();

            for change in &staged.changes {
                let result = match change.kind.as_str() {
                    "package_add" => {
                        let file = change.file.as_ref().map(|f| Path::new(f.as_str()));
                        match nixman_core::packages::manage::add(workspace, &change.option_path, file) {
                            Ok(_added) => {
                                package_count += 1;
                                Ok(())
                            }
                            Err(e) => Err(format!("Failed to add package '{}': {}", change.option_path, e)),
                        }
                    }
                    "package_remove" => {
                        let file = change.file.as_ref().map(|f| Path::new(f.as_str()));
                        match nixman_core::packages::manage::remove(workspace, &change.option_path, file) {
                            Ok(_removed) => {
                                package_count += 1;
                                Ok(())
                            }
                            Err(e) => Err(format!("Failed to remove package '{}': {}", change.option_path, e)),
                        }
                    }
                    _ => {
                        // config editor pipeline.
                        let nix_value = crate::value_parser::parse_nix_value(&change.value);
                        let mut pending = nixman_core::config::PendingChanges::new();
                        match nixman_core::config::editor::set_value(
                            &mut pending,
                            workspace,
                            &change.option_path,
                            nix_value,
                        ) {
                            Ok(()) => {
                                match nixman_core::config::editor::apply_pending(&mut pending, workspace) {
                                    Ok(()) => {
                                        option_count += 1;
                                        Ok(())
                                    }
                                    Err(e) => Err(format!("Failed to apply option '{}': {}", change.option_path, e)),
                                }
                            }
                            Err(e) => Err(format!("Failed to stage option '{}': {}", change.option_path, e)),
                        }
                    }
                };
                if let Err(e) = result {
                    errors.push(e);
                }
            }

            if !errors.is_empty() {
                return Err(format!(
                    "Applied {} option change(s) and {} package change(s) with {} error(s):\n{}",
                    option_count,
                    package_count,
                    errors.len(),
                    errors.join("\n"),
                )
                .into());
            }

            StagedChanges::discard(workspace);
            let total = option_count + package_count;
            let mut parts = Vec::new();
            if option_count > 0 {
                parts.push(format!("{} option(s)", option_count));
            }
            if package_count > 0 {
                parts.push(format!("{} package(s)", package_count));
            }
            let detail = if parts.is_empty() {
                "0 changes".to_string()
            } else {
                parts.join(", ")
            };
            Ok(format!(
                "Applied {} change(s) ({}). Run 'nixman rebuild' to activate.",
                total, detail
            ))
        }
        PendingCmd::Discard => {
            let staged = StagedChanges::load(workspace);
            if staged.is_empty() {
                return Ok("No pending changes to discard.".to_string());
            }
            let count = staged.count();
            StagedChanges::discard(workspace);
            Ok(format!("Discarded {} staged change(s).", count))
        }
    }
}
