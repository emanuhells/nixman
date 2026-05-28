//! Roll back the running system to a specific NixOS generation.
//!
//! Rollback is a two-step process:
//! 1. Switch the system profile pointer with `nix-env --switch-generation N`.
//! 2. Activate the generation's configuration with
//!    `switch-to-configuration switch`.
//!
//! Both commands are run without privilege elevation here; the caller is
//! responsible for ensuring they have the necessary permissions (typically via
//! `pkexec` or running as root).

use tokio::process::Command;

use crate::generations::types::GenerationError;

/// Profile path managed by NixOS.
const SYSTEM_PROFILE: &str = "/nix/var/nix/profiles/system";

/// Roll back the NixOS system profile to generation `gen_number`.
///
/// Runs:
/// 1. `nix-env --profile /nix/var/nix/profiles/system --switch-generation N`
/// 2. `/nix/var/nix/profiles/system/bin/switch-to-configuration switch`
///
/// # Errors
/// - [`GenerationError::CommandFailed`] if either command exits non-zero.
/// - [`GenerationError::IoError`] if a command cannot be spawned.
pub async fn to(gen_number: u32) -> Result<(), GenerationError> {
    // Step 1 — re-point the profile symlink.
    let switch_out = Command::new("nix-env")
        .args([
            "--profile",
            SYSTEM_PROFILE,
            "--switch-generation",
            &gen_number.to_string(),
        ])
        .output()
        .await
        .map_err(GenerationError::IoError)?;

    if !switch_out.status.success() {
        return Err(GenerationError::CommandFailed {
            exit_code: switch_out.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&switch_out.stderr).into_owned(),
        });
    }

    // Step 2 — activate the generation's init/service configuration.
    let activate_out = Command::new("/nix/var/nix/profiles/system/bin/switch-to-configuration")
        .arg("switch")
        .output()
        .await
        .map_err(GenerationError::IoError)?;

    if !activate_out.status.success() {
        return Err(GenerationError::CommandFailed {
            exit_code: activate_out.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&activate_out.stderr).into_owned(),
        });
    }

    Ok(())
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    /// Verify the argument construction at compile time by ensuring the
    /// function signature is stable.  Full integration tests require a NixOS
    /// host with root access.
    #[test]
    fn rollback_signature_exists() {
        // `to` must be an async fn accepting u32.
        let _: fn(u32) -> _ = super::to;
    }
}
