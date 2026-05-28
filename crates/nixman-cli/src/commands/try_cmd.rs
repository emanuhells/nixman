use clap::Subcommand;
use std::path::Path;

#[derive(Subcommand)]
pub enum TryCmd {
    /// Apply temporary changes with auto-revert timeout
    Apply {
        /// Options to set temporarily (format: path=value)
        #[arg(long = "set", value_name = "PATH=VALUE")]
        sets: Vec<String>,

        /// Timeout in seconds before auto-revert
        #[arg(long, default_value = "120")]
        timeout: u64,
    },
    /// Confirm temporary changes (make them permanent)
    Confirm,
}

pub async fn run(
    cmd: TryCmd,
    workspace: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    match cmd {
        TryCmd::Apply { sets, timeout } => {
            if sets.is_empty() {
                return Err("No changes specified. Use --set path=value".into());
            }

            let mut pending = nixman_core::config::PendingChanges::new();
            for set_arg in &sets {
                let (path, value) = set_arg.split_once('=')
                    .ok_or_else(|| format!("Invalid --set format '{}'. Expected path=value", set_arg))?;
                let nix_value = crate::value_parser::parse_nix_value(value.trim());
                nixman_core::config::editor::set_value(
                    &mut pending,
                    workspace,
                    path.trim(),
                    nix_value,
                )?;
            }
            nixman_core::config::editor::apply_pending(&mut pending, workspace)?;

            eprintln!("Building with 'test' mode (won't change boot default)...");
            let build_output = tokio::process::Command::new("sudo")
                .args(["nixos-rebuild", "test"])
                .current_dir(workspace)
                .output()
                .await?;

            if !build_output.status.success() {
                let stderr = String::from_utf8_lossy(&build_output.stderr);
                let _ = tokio::process::Command::new("git")
                    .args(["checkout", "."])
                    .current_dir(workspace)
                    .output()
                    .await;
                return Err(format!("Build failed. Changes reverted.\n{}", stderr).into());
            }

            let state = TryState {
                workspace: workspace.to_string_lossy().to_string(),
                changes: sets.clone(),
                timeout,
                started_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            };
            save_try_state(&state)?;

            let _ = tokio::process::Command::new("systemd-run")
                .args([
                    "--user",
                    "--on-active", &format!("{}s", timeout),
                    "--unit", "nixman-try-revert",
                    "--description", "nixman try auto-revert",
                    "bash", "-c",
                    &format!("cd {} && git checkout . && sudo nixos-rebuild switch", workspace.display()),
                ])
                .output()
                .await;

            Ok(format!(
                "Changes applied (test mode). Auto-revert in {} seconds.\n\
                 Run 'nixman try confirm' to make permanent, or wait for timeout.",
                timeout
            ))
        }
        TryCmd::Confirm => {
            let state = load_try_state()?;
            let workspace = Path::new(&state.workspace);

            // Cancel the auto-revert timer and transient service.
            let _ = tokio::process::Command::new("systemctl")
                .args(["--user", "stop", "nixman-try-revert.timer"])
                .output()
                .await;
            let _ = tokio::process::Command::new("systemctl")
                .args(["--user", "stop", "nixman-try-revert.service"])
                .output()
                .await;

            // Make the test changes permanent by switching the boot default.
            let output = tokio::process::Command::new("sudo")
                .args(["nixos-rebuild", "switch"])
                .current_dir(workspace)
                .output()
                .await?;

            remove_try_state();

            if output.status.success() {
                Ok("Changes confirmed and made permanent.".to_string())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                Err(format!("Failed to confirm: {}", stderr).into())
            }
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct TryState {
    workspace: String,
    changes: Vec<String>,
    timeout: u64,
    started_at: u64,
}

fn try_state_path() -> std::path::PathBuf {
    let state_dir = std::env::var("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            std::path::PathBuf::from(home).join(".local").join("state")
        });
    state_dir.join("nixman").join("try-state.json")
}

fn save_try_state(state: &TryState) -> Result<(), Box<dyn std::error::Error>> {
    let path = try_state_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(state)?;
    std::fs::write(&path, json)?;
    Ok(())
}

fn load_try_state() -> Result<TryState, Box<dyn std::error::Error>> {
    let path = try_state_path();
    if !path.exists() {
        return Err("No active try session. Run 'nixman try apply' first.".into());
    }
    let content = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&content)?)
}

fn remove_try_state() {
    let _ = std::fs::remove_file(try_state_path());
}
