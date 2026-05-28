/// Represents an action that may require privilege escalation.
#[derive(Debug, Clone)]
pub enum PrivilegeAction {
    /// Writing to a file at the given path (if root-owned, elevation is needed).
    WriteFile(std::path::PathBuf),
    /// Running `nixos-rebuild` always requires elevation.
    NixosRebuild,
}

/// The captured output of an elevated command.
#[derive(Debug, Clone)]
pub struct ElevationResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Errors that can occur during privilege escalation.
#[derive(Debug)]
pub enum PrivilegeError {
    /// The user cancelled the polkit authentication dialog (pkexec exit code 126).
    Cancelled,
    /// The user is not authorized to perform the action (pkexec exit code 127).
    NotAuthorized,
    /// `pkexec` binary was not found on the system.
    PkexecNotFound,
    /// The elevated command ran but exited with a non-zero status.
    CommandFailed { exit_code: i32, stderr: String },
}

impl std::fmt::Display for PrivilegeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(f, "Privilege escalation was cancelled by the user"),
            Self::NotAuthorized => write!(f, "User is not authorized to perform this action"),
            Self::PkexecNotFound => write!(
                f,
                "pkexec not found — polkit is required to run elevated commands"
            ),
            Self::CommandFailed { exit_code, stderr } => write!(
                f,
                "Elevated command failed with exit code {exit_code}: {stderr}"
            ),
        }
    }
}

impl std::error::Error for PrivilegeError {}
