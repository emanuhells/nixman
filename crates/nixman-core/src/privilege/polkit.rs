use std::io::ErrorKind;

use tokio::process::Command;

use crate::privilege::types::{ElevationResult, PrivilegeError};

/// Spawns `pkexec <command> [args...]` asynchronously, captures stdout/stderr,
/// and translates polkit exit codes into typed errors.
///
/// # Errors
/// - [`PrivilegeError::PkexecNotFound`] — `pkexec` binary is absent.
/// - [`PrivilegeError::Cancelled`] — user dismissed the polkit dialog (exit 126).
/// - [`PrivilegeError::NotAuthorized`] — user is not authorized (exit 127).
/// - [`PrivilegeError::CommandFailed`] — the inner command exited non-zero.
pub async fn run_elevated(command: &str, args: &[&str]) -> Result<ElevationResult, PrivilegeError> {
    let output = Command::new("pkexec")
        .arg(command)
        .args(args)
        .output()
        .await
        .map_err(|e| {
            if e.kind() == ErrorKind::NotFound {
                PrivilegeError::PkexecNotFound
            } else {
                // Treat unexpected I/O errors as a command failure with no output.
                PrivilegeError::CommandFailed {
                    exit_code: -1,
                    stderr: e.to_string(),
                }
            }
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let exit_code = output.status.code().unwrap_or(-1);

    match exit_code {
        0 => Ok(ElevationResult {
            stdout,
            stderr,
            exit_code,
        }),
        126 => Err(PrivilegeError::Cancelled),
        127 => Err(PrivilegeError::NotAuthorized),
        _ => Err(PrivilegeError::CommandFailed { exit_code, stderr }),
    }
}
