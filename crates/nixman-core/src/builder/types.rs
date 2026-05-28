//! Shared types for the builder module.

use std::io;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The mode passed to `nixos-rebuild`, determining what action is taken after
/// the build completes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BuildMode {
    /// Build and immediately switch the running system.
    Switch,
    /// Build and set the next-boot default, without activating now.
    Boot,
    /// Build and activate in the running system, but do not make it permanent.
    Test,
    /// Build only — do not activate or change the boot default.
    Build,
}

impl BuildMode {
    /// Return the `nixos-rebuild` subcommand string for this mode.
    pub fn as_str(&self) -> &'static str {
        match self {
            BuildMode::Switch => "switch",
            BuildMode::Boot   => "boot",
            BuildMode::Test   => "test",
            BuildMode::Build  => "build",
        }
    }
}

/// A discrete phase of the NixOS rebuild pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BuildPhase {
    /// Nix is evaluating the flake / configuration.
    Evaluating,
    /// Nix is fetching store paths from substituters.
    Fetching,
    /// Nix is building derivations locally.
    Building,
    /// The new configuration is being activated on the running system.
    Activating,
}

/// An event emitted during a build run, sent through an mpsc channel to allow
/// callers to react in real time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BuildEvent {
    /// A single line of raw output from the build process.
    Output(String),
    /// The build entered a new phase (detected from output patterns).
    PhaseChanged(BuildPhase),
    /// The build completed.  This is the final event on the channel.
    Complete(BuildResult),
}

/// Summary produced when a build run finishes (successfully or not).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildResult {
    /// `true` when `nixos-rebuild` exited with status 0.
    pub success: bool,
    /// Wall-clock duration of the entire run in seconds.
    pub duration_secs: f64,
    /// All output lines concatenated (stdout followed by stderr).
    pub output: String,
    /// Human-readable error description when `success` is `false`.
    pub error: Option<String>,
}

/// A persisted record of a past build run, written to the build-history file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildHistoryEntry {
    /// UTC instant at which the build was initiated.
    pub timestamp: DateTime<Utc>,
    /// Which `nixos-rebuild` mode was used.
    pub mode: BuildMode,
    /// The outcome of the build.
    pub result: BuildResult,
}

/// Errors that the builder module can produce.
#[derive(Debug)]
pub enum BuildError {
    /// The polkit authentication dialog was cancelled or the user lacks
    /// authorisation (pkexec exit codes 126 / 127).
    PrivilegeEscalationFailed,
    /// `pkexec` or `nixos-rebuild` was not found on `$PATH`.
    CommandNotFound,
    /// The child process could not be spawned due to an OS-level I/O error.
    SpawnFailed(io::Error),
    /// The supplied flake path could not be converted to a valid UTF-8 string.
    FlakePathInvalid,
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PrivilegeEscalationFailed => {
                write!(f, "Privilege escalation failed or was cancelled")
            }
            Self::CommandNotFound => {
                write!(f, "pkexec or nixos-rebuild not found — ensure both are on PATH")
            }
            Self::SpawnFailed(e) => write!(f, "Failed to spawn nixos-rebuild: {e}"),
            Self::FlakePathInvalid => {
                write!(f, "Flake path contains invalid UTF-8 characters")
            }
        }
    }
}

impl std::error::Error for BuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SpawnFailed(e) => Some(e),
            _ => None,
        }
    }
}
