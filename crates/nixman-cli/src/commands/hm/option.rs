use clap::Subcommand;
use std::io::Read;

use crate::CliError;

#[derive(Subcommand)]
pub enum HmOptionCmd {
    /// Get current value of an HM option
    Get { path: String },
    /// Set an HM option value
    Set {
        path: String,
        /// Value to set (omit if using --stdin)
        value: Option<String>,
        /// Read value from stdin instead of argument
        #[arg(long)]
        stdin: bool,
        /// Show what would change without writing
        #[arg(long)]
        dry_run: bool,
        /// Stage change instead of applying immediately
        #[arg(long)]
        stage: bool,
    },
    /// Remove an HM option from config
    Remove {
        path: String,
        /// Show what would change without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Search available Home Manager options
    Search {
        query: String,
        /// Maximum number of results
        #[arg(long, default_value = "20")]
        limit: usize,
    },
}

pub async fn run(cmd: HmOptionCmd) -> Result<String, Box<dyn std::error::Error>> {
    let ws = nixman_core::workspace::hm::detect_hm()
        .map_err(|e| format!("Home Manager workspace not found: {}", e))?;

    match cmd {
        HmOptionCmd::Get { path } => {
            let value = nixman_core::config::editor::hm_get_value(&ws.path, &path)?;
            match value {
                Some(v) => Ok(serde_json::to_string_pretty(&v)?),
                None => Ok(format!("Option '{}' is not set", path)),
            }
        }
        HmOptionCmd::Set { path, value, stdin, dry_run, stage } => {
            let resolved = match (value, stdin) {
                (Some(v), false) => v,
                (None, true) => {
                    let mut buf = String::new();
                    std::io::stdin()
                        .read_to_string(&mut buf)
                        .map_err(|e| format!("failed to read stdin: {}", e))?;
                    let trimmed = buf.trim_end_matches(|c| c == '\n' || c == '\r').to_string();
                    if trimmed.is_empty() {
                        return Err("no value provided on stdin".into());
                    }
                    trimmed
                }
                (Some(_), true) => {
                    return Err("--stdin flag and positional value cannot be used together".into());
                }
                (None, false) => {
                    return Err("no value provided: use positional argument or --stdin".into());
                }
            };

            if stage {
                let mut staged = crate::pending_store::StagedChanges::load(&ws.path);
                staged.add_option(path.clone(), resolved.clone());
                staged.save(&ws.path).map_err(|e| format!("Failed to save staging: {}", e))?;
                return Ok(format!("Staged: {} = {}", path, resolved));
            }

            let nix_value = crate::value_parser::parse_nix_value(&resolved);

            if let Some(current) = nixman_core::config::editor::hm_get_value(&ws.path, &path)? {
                if current == nix_value {
                    return Err(CliError::Noop(format!(
                        "Option '{}' is already set to '{}'",
                        path, resolved
                    ))
                    .into());
                }
            }

            let mut pending = nixman_core::config::PendingChanges::new();
            nixman_core::config::editor::hm_set_value(
                &mut pending,
                &ws.path,
                &path,
                nix_value,
            )?;
            if dry_run {
                let diffs = pending.generate_diffs()?;
                let mut output = String::new();
                for diff in &diffs {
                    output.push_str(&crate::commands::option::simple_diff(
                        &diff.original,
                        &diff.modified,
                        &diff.file.display().to_string(),
                    ));
                }
                return Ok(output);
            }
            nixman_core::config::editor::hm_apply_pending(&mut pending, &ws.path)?;
            Ok(format!("Set: {} = {}", path, resolved))
        }
        HmOptionCmd::Remove { path, dry_run } => {
            if dry_run {
                use nixman_core::nix_parser::{
                    modules::build_graph,
                    resolver::locate,
                    writer::remove_option,
                };
                let entry = ws.path.join("home.nix");
                let graph = build_graph(&entry)
                    .map_err(|e| format!("failed to build module graph: {e}"))?;
                let resolved = locate(&graph, &ws.path, &path)
                    .map_err(|e| format!("failed to locate option: {e}"))?;
                if !resolved.exists {
                    return Err(format!("option '{}' is not set in the configuration", path).into());
                }
                let source = std::fs::read_to_string(&resolved.file)
                    .map_err(|e| format!("failed to read {}: {e}", resolved.file.display()))?;
                let modified = remove_option(&source, &path)
                    .map_err(|e| format!("failed to remove option: {e}"))?;
                let output = crate::commands::option::simple_diff(
                    &source,
                    &modified,
                    &resolved.file.display().to_string(),
                );
                return Ok(output);
            }
            nixman_core::config::editor::hm_remove_value(&ws.path, &path)?;
            Ok(format!("Option '{}' removed from configuration.", path))
        }
        HmOptionCmd::Search { query, limit } => {
            let (tx, _rx) = std::sync::mpsc::channel();
            match nixman_core::options::build_hm_index(&ws.path, tx) {
                Ok(index) => {
                    let results = nixman_core::options::search::query(&index, &query, limit);
                    Ok(serde_json::to_string_pretty(&results)?)
                }
                Err(nixman_core::options::IndexError::FlakeLockNotFound) => {
                    Err("No flake.lock found in HM workspace. HM option search requires a \
                         flake-based Home Manager configuration with flake.lock."
                        .into())
                }
                Err(e) => Err(format!("Failed to build HM option index: {}", e).into()),
            }
        }
    }
}
