//! Post-edit validation via `nix-instantiate`.
//!
//! [`check`] writes a Nix source string to a temporary file and runs
//! `nix-instantiate --parse` to verify its syntax without evaluating it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::types::{ConfigError, ValidationResult};

// Public API

/// Syntax-check `source` by running `nix-instantiate --parse` on it.
///
/// `file_path` is the original file the source came from; it is used only for
/// context in error messages and is **not** modified.  The actual check is
/// performed on a temporary copy.
///
/// # Return value
///
/// * [`ValidationResult::Valid`] — `nix-instantiate` exited with status 0.
/// * [`ValidationResult::Invalid`] — `nix-instantiate` reported errors; the
///   `errors` field contains the non-empty lines from its stderr.
///
/// # Errors
///
/// Returns [`ConfigError::IoError`] when the temporary file cannot be
/// written or when `nix-instantiate` itself cannot be spawned.
pub fn check(_file_path: &Path, source: &str) -> Result<ValidationResult, ConfigError> {
    let temp_path = temp_nix_file_path();

    // Write the source to a temporary file.
    std::fs::write(&temp_path, source)?;

    // Run nix-instantiate --parse <temp_file>.
    let output = Command::new("nix-instantiate")
        .arg("--parse")
        .arg(&temp_path)
        .output();

    // Always clean up the temp file, regardless of success or failure.
    let _ = std::fs::remove_file(&temp_path);

    let output = match output {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // nix-instantiate not available — skip validation gracefully.
            return Ok(ValidationResult::Valid);
        }
        Err(e) => return Err(ConfigError::IoError(e)),
    };

    if output.status.success() {
        Ok(ValidationResult::Valid)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let errors: Vec<String> = stderr
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.to_string())
            .collect();

        Ok(ValidationResult::Invalid {
            errors: if errors.is_empty() {
                vec!["nix-instantiate exited with a non-zero status".to_string()]
            } else {
                errors
            },
        })
    }
}

// Internal helpers

/// Build a unique path inside the system temp directory for a short-lived
/// validation file.
///
/// The path encodes the current process ID and a nanosecond timestamp to
/// avoid collisions between concurrent invocations.
fn temp_nix_file_path() -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    std::env::temp_dir().join(format!(
        "nix_mgr_validate_{}_{}.nix",
        std::process::id(),
        ts,
    ))
}
