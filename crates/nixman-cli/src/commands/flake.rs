use std::path::Path;

use clap::Subcommand;
use serde::Serialize;


#[derive(Serialize)]
struct FlakeShowOutput {
    workspace: String,
    kind: String,
    hostname: String,
    nixpkgs_rev: Option<String>,
    lock_hash: Option<String>,
    input_count: Option<usize>,
}

#[derive(Subcommand)]
pub enum FlakeCmd {
    /// List all flake inputs
    List,
    /// Update one or all flake inputs
    Update {
        /// Input name to update (omit to update all)
        input: Option<String>,
    },
    /// Show the current flake metadata
    Show,
}

#[derive(Serialize)]
struct FlakeInputDisplay {
    name: String,
    url: String,
    rev: String,
    last_modified: String,
    age: String,
}

fn human_age(dt: &chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    let duration = now.signed_duration_since(*dt);
    let days = duration.num_days();
    if days == 0 {
        "today".to_string()
    } else if days == 1 {
        "1 day ago".to_string()
    } else if days < 30 {
        format!("{} days ago", days)
    } else if days < 365 {
        format!("{} months ago", days / 30)
    } else {
        format!("{} years ago", days / 365)
    }
}

pub async fn run(
    cmd: FlakeCmd,
    workspace: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    match cmd {
        FlakeCmd::List => {
            match nixman_core::flake::metadata::list_inputs(workspace) {
                Ok(inputs) => {
                    let display: Vec<FlakeInputDisplay> = inputs
                        .into_iter()
                        .map(|i| FlakeInputDisplay {
                            name: i.name,
                            url: i.url,
                            rev: i.rev,
                            last_modified: i.last_modified.to_rfc3339(),
                            age: human_age(&i.last_modified),
                        })
                        .collect();
                    serde_json::to_string_pretty(&display).map_err(|e| e.into())
                }
                Err(nixman_core::flake::FlakeError::LockNotFound) => {
                    Ok("This workspace does not use flakes.".to_string())
                }
                Err(e) => Err(e.to_string().into()),
            }
        }
        FlakeCmd::Show => {
            let workspace_str = workspace.to_string_lossy().to_string();
            let hostname = nixman_core::workspace::detect::get_hostname();

            match nixman_core::flake::metadata::list_inputs(workspace) {
                Ok(inputs) => {
                    let nixpkgs_rev = inputs
                        .iter()
                        .find(|i| i.name == "nixpkgs")
                        .map(|i| i.rev.clone())
                        .filter(|r| !r.is_empty());

                    let lock_hash = nixman_core::options::cache::hash_flake_lock(workspace).ok();
                    let input_count = Some(inputs.len());

                    let output = FlakeShowOutput {
                        workspace: workspace_str,
                        kind: "flake".to_string(),
                        hostname,
                        nixpkgs_rev,
                        lock_hash,
                        input_count,
                    };
                    serde_json::to_string_pretty(&output).map_err(|e| e.into())
                }
                Err(nixman_core::flake::FlakeError::LockNotFound) => {
                    let output = FlakeShowOutput {
                        workspace: workspace_str,
                        kind: "legacy".to_string(),
                        hostname,
                        nixpkgs_rev: None,
                        lock_hash: None,
                        input_count: None,
                    };
                    serde_json::to_string_pretty(&output).map_err(|e| e.into())
                }
                Err(e) => Err(e.to_string().into()),
            }
        }
        FlakeCmd::Update { input } => {
            if !workspace.join("flake.lock").exists() {
                return Err("Not a flake workspace. For legacy NixOS, use: sudo nix-channel --update".into());
            }

            let mut cmd = tokio::process::Command::new("nix");
            cmd.arg("flake").arg("update");
            if let Some(ref name) = input {
                cmd.arg(name);
            }
            cmd.current_dir(workspace);

            let output = cmd.output().await?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("nix flake update failed: {}", stderr).into());
            }

            let cache_dir = nixman_core::options::cache::default_cache_dir();
            if let Ok(entries) = std::fs::read_dir(&cache_dir) {
                for entry in entries.flatten() {
                    if entry.file_name().to_string_lossy().starts_with("options-") {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }

            let target = input.as_deref().unwrap_or("all inputs");
            Ok(format!("Updated {}. Option index cache invalidated.", target))
        }
    }
}
