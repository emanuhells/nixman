//! `nixos-rebuild` invocation with streaming output.
//!
//! [`run`] is the single entry point.  It constructs the `pkexec nixos-rebuild`
//! command, spawns it with piped stdout/stderr, streams both through an mpsc
//! channel line-by-line (with phase detection), and returns a [`BuildResult`]
//! once the process exits.

use std::io::ErrorKind;
use std::path::Path;
use std::time::Instant;

use tokio::process::Command;
use tokio::sync::mpsc;

use crate::builder::stream;
use crate::builder::types::{BuildError, BuildEvent, BuildMode, BuildResult};

/// Read `/etc/hostname` to obtain the machine's hostname, falling back to
/// `"nixos"` if the file is absent or contains invalid data.
fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| String::from("nixos"))
}

/// Build the NixOS configuration at `flake_path` using `mode`, streaming all
/// output through `event_tx`.
///
/// # Command
/// ```text
/// pkexec nixos-rebuild <mode> --flake <flake_path>#<hostname>
/// ```
///
/// # Events
/// - [`BuildEvent::PhaseChanged`] — emitted whenever a new build phase is
///   detected in the output.
/// - [`BuildEvent::Output`] — emitted for every output line (both stdout and
///   stderr).
/// - [`BuildEvent::Complete`] — emitted exactly once when the process exits.
///   After this event no further events will be sent on the channel.
///
/// # Errors
/// - [`BuildError::FlakePathInvalid`] — `flake_path` is not valid UTF-8.
/// - [`BuildError::CommandNotFound`] — `pkexec` is not on `$PATH`.
/// - [`BuildError::PrivilegeEscalationFailed`] — pkexec returned 126 or 127.
/// - [`BuildError::SpawnFailed`] — another OS-level error prevented spawning.
pub async fn run(
    mode: BuildMode,
    flake_path: &Path,
    event_tx: mpsc::Sender<BuildEvent>,
) -> Result<BuildResult, BuildError> {
    let flake_str = flake_path
        .to_str()
        .ok_or(BuildError::FlakePathInvalid)?;

    let flake_arg = format!("{}#{}", flake_str, hostname());

    let start = Instant::now();

    // Spawn:  pkexec nixos-rebuild <mode> --flake <path>#<hostname>
    let mut child = Command::new("pkexec")
        .arg("nixos-rebuild")
        .arg(mode.as_str())
        .arg("--flake")
        .arg(&flake_arg)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == ErrorKind::NotFound {
                BuildError::CommandNotFound
            } else {
                BuildError::SpawnFailed(e)
            }
        })?;

    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");

    // Read stdout and stderr concurrently in separate tasks so neither pipe
    // can fill up and deadlock the child process.
    let stdout_task = tokio::spawn(stream::stream_lines(stdout, event_tx.clone()));
    let stderr_task = tokio::spawn(stream::stream_lines(stderr, event_tx.clone()));

    // Await both readers — they finish when the child closes the pipes.
    let stdout_lines = stdout_task.await.unwrap_or_default();
    let stderr_lines = stderr_task.await.unwrap_or_default();

    // Collect the exit status.
    let status = child.wait().await.map_err(BuildError::SpawnFailed)?;
    let exit_code = status.code().unwrap_or(-1);

    // Translate pkexec privilege-escalation exit codes.
    if exit_code == 126 || exit_code == 127 {
        return Err(BuildError::PrivilegeEscalationFailed);
    }

    let duration_secs = start.elapsed().as_secs_f64();

    // Assemble the full output string (stdout first, then stderr).
    let parts: Vec<String> = [stdout_lines, stderr_lines]
        .into_iter()
        .flatten()
        .collect();
    let output = parts.join("\n");

    let success = status.success();
    let error = if success {
        None
    } else {
        Some(format!("nixos-rebuild exited with code {exit_code}"))
    };

    let result = BuildResult {
        success,
        duration_secs,
        output,
        error,
    };

    // Emit the terminal event.
    let _ = event_tx.send(BuildEvent::Complete(result.clone())).await;

    Ok(result)
}
