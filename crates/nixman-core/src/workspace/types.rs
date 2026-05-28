use std::fmt;
use std::io;
use std::path::PathBuf;

/// Whether the workspace uses the modern flake-based layout or the legacy
/// channel-pinned `configuration.nix` layout.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkspaceKind {
    /// Directory contains a `flake.nix`.
    Flake,
    /// Directory contains a `configuration.nix` but no `flake.nix`.
    Legacy,
}

/// POSIX ownership information for the workspace directory.
#[derive(Debug, Clone)]
pub struct OwnershipInfo {
    /// `true` when the directory's uid matches the running process's uid.
    pub is_user_owned: bool,
    /// Raw POSIX uid of the directory owner.
    pub uid: u32,
}

/// A located NixOS configuration directory together with its metadata.
#[derive(Debug, Clone)]
pub struct Workspace {
    /// Canonical (symlink-resolved) path to the configuration directory.
    pub path: PathBuf,
    /// Flake vs. legacy classification.
    pub kind: WorkspaceKind,
    /// Ownership details for the directory.
    pub owner: OwnershipInfo,
    /// System hostname, sourced from `/etc/hostname` or the `hostname` binary.
    pub hostname: String,
}

/// All errors that workspace operations can produce.
#[derive(Debug)]
pub enum WorkspaceError {
    /// No valid NixOS configuration directory was found anywhere in the
    /// detection chain.
    NotFound,
    /// The current process lacks the permissions required to read or create the
    /// workspace path.
    PermissionDenied,
    /// A path was found but its contents are not a recognisable NixOS config.
    InvalidConfig(String),
    /// An underlying I/O error that doesn't fit a more specific variant.
    IoError(io::Error),
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkspaceError::NotFound => {
                write!(f, "no NixOS configuration directory found")
            }
            WorkspaceError::PermissionDenied => {
                write!(f, "permission denied accessing workspace path")
            }
            WorkspaceError::InvalidConfig(msg) => {
                write!(f, "invalid configuration: {}", msg)
            }
            WorkspaceError::IoError(e) => {
                write!(f, "I/O error: {}", e)
            }
        }
    }
}

impl std::error::Error for WorkspaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            WorkspaceError::IoError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for WorkspaceError {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::PermissionDenied => WorkspaceError::PermissionDenied,
            _ => WorkspaceError::IoError(e),
        }
    }
}
