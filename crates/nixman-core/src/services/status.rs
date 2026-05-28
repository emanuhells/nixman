use std::collections::HashMap;
use std::io::ErrorKind;

use serde::Deserialize;
use tokio::process::Command;

use crate::services::types::{ServiceError, ServiceInfo, ServiceStatus};

// ── Public API ────────────────────────────────────────────────────────────────

/// Runs `systemctl list-units --type=service --output=json --all --no-pager`
/// and returns a [`ServiceInfo`] vec for every loaded service unit.
///
/// The `enabled` field reflects the unit's `load` state; for an accurate
/// autostart flag call [`get`] which queries `UnitFileState` directly.
pub async fn list_all() -> Result<Vec<ServiceInfo>, ServiceError> {
    let output = Command::new("systemctl")
        .args([
            "list-units",
            "--type=service",
            "--output=json",
            "--all",
            "--no-pager",
        ])
        .output()
        .await
        .map_err(|e| {
            if e.kind() == ErrorKind::NotFound {
                ServiceError::CommandNotFound
            } else {
                ServiceError::CommandFailed {
                    exit_code: -1,
                    stderr: e.to_string(),
                }
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let exit_code = output.status.code().unwrap_or(-1);
        return Err(ServiceError::CommandFailed { exit_code, stderr });
    }

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    parse_list_units_json(&stdout)
}

/// Runs `systemctl show <unit> --property=... --no-pager` and returns a
/// detailed [`ServiceInfo`] for that unit.
///
/// Properties queried: `ActiveState`, `SubState`, `Description`,
/// `UnitFileState`, `LoadState`.
pub async fn get(unit: &str) -> Result<ServiceInfo, ServiceError> {
    let output = Command::new("systemctl")
        .args([
            "show",
            unit,
            "--property=ActiveState,SubState,Description,UnitFileState,LoadState",
            "--no-pager",
        ])
        .output()
        .await
        .map_err(|e| {
            if e.kind() == ErrorKind::NotFound {
                ServiceError::CommandNotFound
            } else {
                ServiceError::CommandFailed {
                    exit_code: -1,
                    stderr: e.to_string(),
                }
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let exit_code = output.status.code().unwrap_or(-1);
        return Err(ServiceError::CommandFailed { exit_code, stderr });
    }

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    parse_show_output(unit, &stdout)
}

// ── Parsing helpers ───────────────────────────────────────────────────────────

/// Internal structure that mirrors a single entry in the JSON produced by
/// `systemctl list-units --output=json`.
#[derive(Deserialize)]
struct UnitListEntry {
    unit: String,
    load: String,
    active: String,
    sub: String,
    description: String,
}

fn parse_list_units_json(json: &str) -> Result<Vec<ServiceInfo>, ServiceError> {
    let entries: Vec<UnitListEntry> = serde_json::from_str(json)
        .map_err(|e| ServiceError::ParseError(e.to_string()))?;

    Ok(entries
        .into_iter()
        .map(|e| {
            let name = strip_service_suffix(&e.unit);
            let status = derive_status(&e.active, &e.sub);
            // list-units does not expose UnitFileState; treat a loaded unit as
            // "enabled" for this purpose — use get() for the precise flag.
            let enabled = e.load == "loaded";
            ServiceInfo {
                name,
                unit_name: e.unit,
                description: e.description,
                status,
                enabled,
            }
        })
        .collect())
}

/// Parses the `Key=Value` output of `systemctl show` into a [`ServiceInfo`].
fn parse_show_output(unit: &str, output: &str) -> Result<ServiceInfo, ServiceError> {
    let mut props: HashMap<&str, &str> = HashMap::new();

    for line in output.lines() {
        if let Some((key, value)) = line.split_once('=') {
            props.insert(key.trim(), value.trim());
        }
    }

    let active = props.get("ActiveState").copied().unwrap_or("unknown");
    let sub = props.get("SubState").copied().unwrap_or("unknown");
    let description = props
        .get("Description")
        .copied()
        .unwrap_or("")
        .to_string();
    let unit_file_state = props.get("UnitFileState").copied().unwrap_or("disabled");

    // A unit is considered "enabled" when it will be pulled in at boot.
    let enabled = matches!(
        unit_file_state,
        "enabled" | "enabled-runtime" | "static" | "generated"
    );

    Ok(ServiceInfo {
        name: strip_service_suffix(unit),
        unit_name: unit.to_string(),
        description,
        status: derive_status(active, sub),
        enabled,
    })
}

// ── Utility functions ─────────────────────────────────────────────────────────

/// Strips the `.service` suffix to obtain an approximate NixOS attribute name.
fn strip_service_suffix(unit: &str) -> String {
    unit.strip_suffix(".service").unwrap_or(unit).to_string()
}

/// Maps (`active-state`, `sub-state`) to a [`ServiceStatus`] variant.
fn derive_status(active: &str, sub: &str) -> ServiceStatus {
    match active {
        "active" | "activating" => match sub {
            "degraded" => ServiceStatus::Degraded,
            _ => ServiceStatus::Running,
        },
        "failed" => ServiceStatus::Failed,
        "inactive" | "deactivating" => ServiceStatus::Stopped,
        _ => ServiceStatus::Unknown,
    }
}
