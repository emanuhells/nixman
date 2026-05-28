use serde::{Deserialize, Serialize};

/// Operational status of a systemd service unit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServiceStatus {
    /// The unit is active and its main process is running.
    Running,
    /// The unit is inactive (not running).
    Stopped,
    /// The unit entered a failed state.
    Failed,
    /// The unit is active but one or more of its components have failed.
    Degraded,
    /// The status could not be determined.
    Unknown,
}

/// Snapshot of a single systemd service unit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceInfo {
    /// NixOS services attribute name (e.g. `"openssh"`).
    pub name: String,
    /// Resolved systemd unit name (e.g. `"sshd.service"`).
    pub unit_name: String,
    /// Human-readable description from the unit file.
    pub description: String,
    /// Current operational status.
    pub status: ServiceStatus,
    /// Whether the unit is enabled to start at boot.
    pub enabled: bool,
}

/// Syslog priority levels, matching RFC 5424 / journald `PRIORITY` field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LogPriority {
    /// System is unusable (priority 0).
    Emergency,
    /// Action must be taken immediately (priority 1).
    Alert,
    /// Critical conditions (priority 2).
    Critical,
    /// Error conditions (priority 3).
    Error,
    /// Warning conditions (priority 4).
    Warning,
    /// Normal but significant conditions (priority 5).
    Notice,
    /// Informational messages (priority 6).
    Info,
    /// Debug-level messages (priority 7).
    Debug,
}

/// A single journal log entry returned by `journalctl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// UTC timestamp in ISO 8601 format (`YYYY-MM-DDTHH:MM:SS.xxxxxxZ`).
    pub timestamp: String,
    /// Log message body.
    pub message: String,
    /// Syslog priority level.
    pub priority: LogPriority,
}

/// Errors that can occur when interacting with systemd.
#[derive(Debug)]
pub enum ServiceError {
    /// `systemctl` or `journalctl` binary was not found on the system.
    CommandNotFound,
    /// The command exited with a non-zero status code.
    CommandFailed { exit_code: i32, stderr: String },
    /// The command output could not be parsed as expected.
    ParseError(String),
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CommandNotFound => {
                write!(f, "systemctl/journalctl not found — systemd is required")
            }
            Self::CommandFailed { exit_code, stderr } => {
                write!(f, "Command failed with exit code {exit_code}: {stderr}")
            }
            Self::ParseError(msg) => write!(f, "Failed to parse command output: {msg}"),
        }
    }
}

impl std::error::Error for ServiceError {}
