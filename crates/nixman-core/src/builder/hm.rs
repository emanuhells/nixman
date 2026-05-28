//! `home-manager` invocation with streaming output.
//!
//! [`rebuild`] is the single entry point.  It constructs the `home-manager`
//! command, spawns it with piped stdout/stderr, streams both through an mpsc
//! channel line-by-line, and returns a JSON result string once the process
//! exits.

use std::io::ErrorKind;
use std::time::Instant;

use tokio::process::Command;
use tokio::sync::mpsc;

use crate::builder::stream;
use crate::builder::types::{BuildEvent, BuildResult};

/// Errors from the Home Manager builder.
#[derive(Debug)]
pub enum BuilderError {
    /// `home-manager` was not found on `$PATH`.
    CommandNotFound,
    /// The child process could not be spawned due to an OS-level I/O error.
    SpawnFailed(std::io::Error),
    /// The build exited with a non-zero status.
    BuildFailed(String),
}

impl std::fmt::Display for BuilderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CommandNotFound => {
                write!(f, "home-manager not found — ensure it is on PATH")
            }
            Self::SpawnFailed(e) => write!(f, "Failed to spawn home-manager: {e}"),
            Self::BuildFailed(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for BuilderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SpawnFailed(e) => Some(e),
            _ => None,
        }
    }
}

/// Run `home-manager <mode>`, streaming output through `event_tx`.
///
/// On success returns a JSON string with `success`, `duration_secs`, and
/// `output` fields.  On failure returns [`BuilderError::BuildFailed`] with
/// the error message.
pub async fn rebuild(
    mode: &str,
    event_tx: mpsc::Sender<BuildEvent>,
) -> Result<String, BuilderError> {
    let start = Instant::now();

    let mut child = Command::new("home-manager")
        .arg(mode)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == ErrorKind::NotFound {
                BuilderError::CommandNotFound
            } else {
                BuilderError::SpawnFailed(e)
            }
        })?;

    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");

    let stdout_task = tokio::spawn(stream::stream_lines(stdout, event_tx.clone()));
    let stderr_task = tokio::spawn(stream::stream_lines(stderr, event_tx.clone()));

    let stdout_lines = stdout_task.await.unwrap_or_default();
    let stderr_lines = stderr_task.await.unwrap_or_default();

    let status = child.wait().await.map_err(BuilderError::SpawnFailed)?;
    let duration_secs = start.elapsed().as_secs_f64();

    let parts: Vec<String> = [stdout_lines, stderr_lines]
        .into_iter()
        .flatten()
        .collect();
    let output = parts.join("\n");

    let success = status.success();
    let exit_code = status.code().unwrap_or(-1);

    let build_result = BuildResult {
        success,
        duration_secs,
        output: output.clone(),
        error: if success {
            None
        } else {
            Some(format!("home-manager exited with code {exit_code}"))
        },
    };

    // Always emit the terminal event so the caller can break its receive loop.
    let _ = event_tx.send(BuildEvent::Complete(build_result)).await;

    if success {
        let json = serde_json::json!({
            "success": true,
            "duration_secs": duration_secs,
        });
        serde_json::to_string(&json)
            .map_err(|e| BuilderError::BuildFailed(format!("JSON serialization failed: {e}")))
    } else {
        Err(BuilderError::BuildFailed(output))
    }
}
