use nixman_core::builder::BuildEvent;
use tokio::sync::mpsc;

#[derive(clap::Subcommand)]
pub enum HmRebuildMode {
    /// Build and activate the Home Manager configuration
    Switch,
    /// Build only (do not activate)
    Build,
    /// Build and set as boot default
    Boot,
    /// Build and test (activate in current session only)
    Test,
}

#[derive(clap::Args)]
pub struct HmRebuildArgs {
    #[command(subcommand)]
    pub mode: HmRebuildMode,
    /// Explain errors in plain English when the build fails
    #[arg(long)]
    pub explain: bool,
    /// Roll back to the previous generation if the build fails
    #[arg(long)]
    pub rollback_on_fail: bool,
}

pub async fn run(args: HmRebuildArgs) -> Result<String, Box<dyn std::error::Error>> {
    let mode_str = match args.mode {
        HmRebuildMode::Switch => "switch",
        HmRebuildMode::Build => "build",
        HmRebuildMode::Boot => "boot",
        HmRebuildMode::Test => "test",
    };

    let (tx, mut rx) = mpsc::channel(64);

    let handle = tokio::spawn(async move {
        nixman_core::builder::hm::rebuild(mode_str, tx).await
    });

    while let Some(event) = rx.recv().await {
        match event {
            BuildEvent::Output(line) => eprintln!("{}", line),
            BuildEvent::PhaseChanged(_) => {}
            BuildEvent::Complete(_) => break,
        }
    }

    match handle.await? {
        Ok(json) => {
            let v: serde_json::Value = serde_json::from_str(&json)?;
            let duration = v["duration_secs"].as_f64().unwrap_or(0.0);
            Ok(format!("Build succeeded in {:.1}s", duration))
        }
        Err(err) => {
            let error_msg = err.to_string();

            if args.rollback_on_fail {
                eprintln!("[!] Build failed. Rolling back to previous generation…");
                match rollback_to_previous().await {
                    Ok(msg) => eprintln!("[✓] {}", msg),
                    Err(e) => eprintln!("[✗] Rollback also failed: {}", e),
                }
            }

            if args.explain {
                let explanation = crate::commands::explain::explain_error(&error_msg);
                let explanation_json = serde_json::to_string_pretty(&explanation)
                    .unwrap_or_else(|_| format!("{:?}", explanation));
                Err(format!(
                    "{}\n\n── nixman explain ──\n{}",
                    error_msg, explanation_json
                )
                .into())
            } else {
                Err(error_msg.into())
            }
        }
    }
}

/// Roll back to the previous Home Manager generation.
async fn rollback_to_previous() -> Result<String, Box<dyn std::error::Error>> {
    let output = tokio::process::Command::new("home-manager")
        .args(["switch", "--rollback"])
        .output()
        .await?;

    if output.status.success() {
        Ok("Rolled back to previous Home Manager generation".to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("home-manager switch --rollback failed: {}", stderr).into())
    }
}
