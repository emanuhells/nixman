use std::path::Path;

use nixman_core::builder::{rebuild, BuildEvent, BuildMode};
use tokio::process::Command;
use tokio::sync::mpsc;

pub async fn run(
    mode: &str,
    workspace: &Path,
    explain: bool,
    rollback_on_fail: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    let build_mode = match mode {
        "switch" => BuildMode::Switch,
        "boot" => BuildMode::Boot,
        "test" => BuildMode::Test,
        "build" => BuildMode::Build,
        other => {
            return Err(
                format!("Unknown build mode '{}'. Use switch, boot, test, or build.", other)
                    .into(),
            )
        }
    };

    let (tx, mut rx) = mpsc::channel(64);
    let workspace_owned = workspace.to_path_buf();

    let handle = tokio::spawn(async move { rebuild::run(build_mode, &workspace_owned, tx).await });

    while let Some(event) = rx.recv().await {
        match event {
            BuildEvent::Output(line) => eprintln!("{}", line),
            BuildEvent::PhaseChanged(phase) => eprintln!("[{}]", phase_label(&phase)),
            BuildEvent::Complete(_) => break,
        }
    }

    let result = handle.await??;

    if result.success {
        Ok(format!("Build succeeded in {:.1}s", result.duration_secs))
    } else {
        let error_msg = result
            .error
            .unwrap_or_else(|| "nixos-rebuild failed".to_string());

        if rollback_on_fail {
            eprintln!("[!] Build failed. Rolling back to previous generation…");
            match rollback_to_previous().await {
                Ok(msg) => eprintln!("[✓] {}", msg),
                Err(e) => eprintln!("[✗] Rollback also failed: {}", e),
            }
        }

        if explain {
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

/// Run `sudo nixos-rebuild switch --rollback` to revert to the previous
/// generation.
async fn rollback_to_previous() -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("sudo")
        .args(["nixos-rebuild", "switch", "--rollback"])
        .output()
        .await?;

    if output.status.success() {
        Ok("Rolled back to previous generation".to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("nixos-rebuild switch --rollback failed: {}", stderr).into())
    }
}

fn phase_label(phase: &nixman_core::builder::BuildPhase) -> &'static str {
    match phase {
        nixman_core::builder::BuildPhase::Evaluating => "evaluating",
        nixman_core::builder::BuildPhase::Fetching => "fetching",
        nixman_core::builder::BuildPhase::Building => "building",
        nixman_core::builder::BuildPhase::Activating => "activating",
    }
}
