//! Pre-flight checks for nixman runtime requirements.

use std::process::Command;

/// Check whether the Nix installation has flakes enabled.
///
/// Runs `nix flake --help` and checks the exit code. Returns `Ok(())` if
/// flakes are available, or `Err` with a user-friendly message if not.
pub fn check_flakes_enabled() -> Result<(), String> {
    let output = Command::new("nix")
        .args(["flake", "--help"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output();

    match output {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            if stderr.contains("experimental") || stderr.contains("disabled") {
                Err(format!(
                    "Nix flakes are not enabled on this system.\n\n\
                     To enable them, add to /etc/nixos/configuration.nix:\n\n\
                     \x20 nix.settings.experimental-features = [ \"nix-command\" \"flakes\" ];\n\n\
                     Then run: sudo nixos-rebuild switch\n\n\
                     Or for immediate (non-persistent) use:\n\n\
                     \x20 echo 'experimental-features = nix-command flakes' | sudo tee -a /etc/nix/nix.conf\n\
                     \x20 sudo systemctl restart nix-daemon"
                ))
            } else {
                Err(format!("nix flake check failed: {}", stderr.trim()))
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err("nix is not installed or not in PATH.".to_string())
        }
        Err(e) => Err(format!("failed to run nix: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_flakes_returns_result() {
        // Just ensure it doesn't panic — actual result depends on environment.
        let _ = check_flakes_enabled();
    }
}
