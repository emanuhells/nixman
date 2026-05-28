//! Systemd service management module.
//!
//! Provides a typed interface for reading service status, fetching journal
//! logs, and performing runtime actions (start / stop / restart) against
//! systemd units via `systemctl` and `journalctl`.
//!
//! # Overview
//!
//! ```ignore
//! use nixman_core::services::{actions, config_map, logs, status};
//!
//! // Resolve a NixOS services attribute name to its unit name.
//! let unit = config_map::resolve("openssh"); // -> "sshd.service"
//!
//! // List every loaded service unit.
//! let all = status::list_all().await?;
//!
//! // Get detailed info for a specific unit.
//! let info = status::get(&unit).await?;
//!
//! // Tail the last 50 log lines.
//! let entries = logs::get(&unit, 50).await?;
//!
//! // Lifecycle actions.
//! actions::start(&unit).await?;
//! actions::stop(&unit).await?;
//! actions::restart(&unit).await?;
//! ```

pub mod actions;
pub mod config_map;
pub mod logs;
pub mod status;
pub mod types;

// Re-export the public types so callers can use `services::` as the prefix.
pub use types::{LogEntry, LogPriority, ServiceError, ServiceInfo, ServiceStatus};
