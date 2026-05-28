use clap::Subcommand;
use std::io::Read;
use std::path::Path;

use crate::CliError;

#[derive(Subcommand)]
pub enum OptionCmd {
    /// Get current value of an option
    Get { path: String },
    /// Set an option value
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
    /// Remove an option from config
    Remove {
        path: String,
        /// Show what would change without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Search available NixOS options
    Search {
        query: String,
        /// Maximum number of results
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Browse options under a prefix
    Browse {
        prefix: Option<String>,
        /// Maximum number of results
        #[arg(long, default_value = "100")]
        limit: usize,
    },
    /// Show full details of a specific option
    Show {
        /// Exact option path
        path: String,
    },
}

pub async fn run(
    cmd: OptionCmd,
    workspace: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    match cmd {
        OptionCmd::Get { path } => {
            let value = nixman_core::config::editor::get_value(workspace, &path)?;
            match value {
                Some(v) => Ok(serde_json::to_string_pretty(&v)?),
                None => Ok(format!("Option '{}' is not set", path)),
            }
        }
        OptionCmd::Set { path, value, stdin, dry_run, stage } => {
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
                let mut staged = crate::pending_store::StagedChanges::load(workspace);
                staged.add_option(path.clone(), resolved.clone());
                staged.save(workspace).map_err(|e| format!("Failed to save staging: {}", e))?;
                return Ok(format!("Staged: {} = {}", path, resolved));
            }

            let nix_value = crate::value_parser::parse_nix_value(&resolved);

            if let Some(current) = nixman_core::config::editor::get_value(workspace, &path)? {
                if current == nix_value {
                    return Err(CliError::Noop(format!("Option '{}' is already set to '{}'", path, resolved)).into());
                }
            }

            let mut pending = nixman_core::config::PendingChanges::new();
            nixman_core::config::editor::set_value(
                &mut pending,
                workspace,
                &path,
                nix_value,
            )?;
            if dry_run {
                let diffs = pending.generate_diffs()?;
                let mut output = String::new();
                for diff in &diffs {
                    output.push_str(&simple_diff(
                        &diff.original,
                        &diff.modified,
                        &diff.file.display().to_string(),
                    ));
                }
                return Ok(output);
            }
            nixman_core::config::editor::apply_pending(&mut pending, workspace)?;
            Ok(format!("Set: {} = {}", path, resolved))
        }
        OptionCmd::Remove { path, dry_run } => {
            if dry_run {
                use nixman_core::nix_parser::{
                    modules::build_graph,
                    resolver::locate,
                    writer::remove_option,
                };
                let entry = workspace.join("configuration.nix");
                let graph = build_graph(&entry)
                    .map_err(|e| format!("failed to build module graph: {e}"))?;
                let resolved = locate(&graph, workspace, &path)
                    .map_err(|e| format!("failed to locate option: {e}"))?;
                if !resolved.exists {
                    return Err(format!("option '{}' is not set in the configuration", path).into());
                }
                let source = std::fs::read_to_string(&resolved.file)
                    .map_err(|e| format!("failed to read {}: {e}", resolved.file.display()))?;
                let modified = remove_option(&source, &path)
                    .map_err(|e| format!("failed to remove option: {e}"))?;
                let output = simple_diff(
                    &source,
                    &modified,
                    &resolved.file.display().to_string(),
                );
                return Ok(output);
            }
            nixman_core::config::editor::remove_value(workspace, &path)?;
            Ok(format!("Option '{}' removed from configuration.", path))
        }
        OptionCmd::Search { query, limit } => {
            let (tx, _rx) = std::sync::mpsc::channel();
            match nixman_core::options::build_index(workspace, tx) {
                Ok(index) => {
                    let results = nixman_core::options::search::query(&index, &query, limit);
                    Ok(serde_json::to_string_pretty(&results)?)
                }
                Err(nixman_core::options::IndexError::FlakeLockNotFound) => {
                    Err("No flake.lock found in workspace. Run `nixman init` or switch to \
                         a Nix flake directory to build the option index."
                        .into())
                }
                Err(e) => Err(format!("Failed to build option index: {}", e).into()),
            }
        }
        OptionCmd::Browse { prefix, limit } => {
            let (tx, _rx) = std::sync::mpsc::channel();
            match nixman_core::options::build_index(workspace, tx) {
                Ok(index) => {
                    let prefix_str = prefix.as_deref().unwrap_or("");
                    let results: Vec<&nixman_core::options::OptionMeta> = if prefix_str.is_empty() {
                        let mut seen = std::collections::BTreeSet::new();
                        for opt in &index.options {
                            if let Some(first) = opt.path.split('.').next() {
                                seen.insert(first.to_string());
                            }
                        }
                        let top_segments: Vec<String> = seen.into_iter().take(limit).collect();
                        return Ok(serde_json::to_string_pretty(&top_segments)?);
                    } else {
                        let search_prefix = if prefix_str.ends_with('.') {
                            prefix_str.to_string()
                        } else {
                            format!("{prefix_str}.")
                        };
                        index
                            .options
                            .iter()
                            .filter(|opt| {
                                let p = &opt.path;
                                // or starts-with-boundary match
                                *p == prefix_str || p.starts_with(&search_prefix)
                            })
                            .take(limit)
                            .collect()
                    };
                    Ok(serde_json::to_string_pretty(&results)?)
                }
                Err(nixman_core::options::IndexError::FlakeLockNotFound) => {
                    Err("No flake.lock found in workspace. Run `nixman init` or switch to \
                         a Nix flake directory to build the option index."
                        .into())
                }
                Err(e) => Err(format!("Failed to build option index: {}", e).into()),
            }
        }
        OptionCmd::Show { path } => {
            let (tx, _rx) = std::sync::mpsc::channel();
            match nixman_core::options::build_index(workspace, tx) {
                Ok(index) => {
                    if let Some(meta) = index.options.iter().find(|o| o.path == path) {
                        return Ok(serde_json::to_string_pretty(meta)?);
                    }
                    let suggestions =
                        nixman_core::options::search::query(&index, &path, 3);
                    let mut msg = format!("Option '{}' not found.\n\nDid you mean:", path);
                    for opt in &suggestions {
                        msg.push_str(&format!("\n  {}", opt.path));
                    }
                    Err(msg.into())
                }
                Err(nixman_core::options::IndexError::FlakeLockNotFound) => {
                    Err("No flake.lock found in workspace. Run `nixman init` or switch to \
                         a Nix flake directory to build the option index."
                        .into())
                }
                Err(e) => Err(format!("Failed to build option index: {}", e).into()),
            }
        }
    }
}

pub use crate::diff_util::simple_diff;
