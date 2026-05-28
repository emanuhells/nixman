use std::io::ErrorKind;

use tokio::process::Command;

use crate::services::types::ServiceError;

// ── Public API ────────────────────────────────────────────────────────────────

/// Starts the given systemd unit: `systemctl start <unit>`.
pub async fn start(unit: &str) -> Result<(), ServiceError> {
    run_action("start", unit).await
}

/// Stops the given systemd unit: `systemctl stop <unit>`.
pub async fn stop(unit: &str) -> Result<(), ServiceError> {
    run_action("stop", unit).await
}

/// Restarts the given systemd unit: `systemctl restart <unit>`.
pub async fn restart(unit: &str) -> Result<(), ServiceError> {
    run_action("restart", unit).await
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Spawns `systemctl <action> <unit>`, captures combined output, and waits for
/// the process to exit.  Returns an error if the command is missing or exits
/// with a non-zero status.
async fn run_action(action: &str, unit: &str) -> Result<(), ServiceError> {
    let output = Command::new("systemctl")
        .args([action, unit])
        .output()
        .await
        .map_err(|e| {
            if e.kind() == ErrorKind::NotFound {
                ServiceError::CommandNotFound
            } else {
                ServiceError::CommandFailed {
                    exit_code: -1,
                    stderr: e.to_string(),
                }
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let exit_code = output.status.code().unwrap_or(-1);
        return Err(ServiceError::CommandFailed { exit_code, stderr });
    }

    Ok(())
}
