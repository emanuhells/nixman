//! MCP server handler — exposes nixman-core tools over MCP.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nixman_core::config::editor;
use nixman_core::nix_parser::NixValue;
use rmcp::{
    ErrorData as McpError,
    ServerHandler,
    handler::server::router::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::*,
    schemars,
    tool,
    tool_handler,
    tool_router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

/// Parameters for the `option_get` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct OptionGetParams {
    /// Option path, e.g. services.nginx.enable
    pub path: String,
}

/// Parameters for the `packages_search` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PackagesSearchParams {
    /// Search query string
    pub query: String,
}

/// Parameters for the `check` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CheckParams;

/// Parameters for the `doctor` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DoctorParams;

/// Parameters for the `status` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct StatusParams;

/// Parameters for the `pending_list` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PendingListParams;

/// Parameters for the `option_set` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct OptionSetParams {
    /// Option path, e.g. services.nginx.enable
    pub path: String,
    /// Nix value to set, e.g. true or "42" or [ "a" "b" ]
    pub value: String,
    /// If true, stage the change in pending instead of writing immediately (default: true)
    #[serde(default = "default_true")]
    pub stage: bool,
}

/// Parameters for the `packages_add` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PackagesAddParams {
    /// Package attribute name, e.g. btop
    pub package: String,
    /// If true, stage the change in pending instead of writing immediately (default: true)
    #[serde(default = "default_true")]
    pub stage: bool,
    /// Skip nixpkgs verification (useful for packages from other flakes)
    #[serde(default)]
    pub no_verify: bool,
}

/// Parameters for the `packages_remove` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PackagesRemoveParams {
    /// Package attribute name, e.g. btop
    pub package: String,
    /// If true, stage the change in pending instead of writing immediately (default: true)
    #[serde(default = "default_true")]
    pub stage: bool,
}

/// Parameters for the `rebuild` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RebuildParams {
    /// Build mode: switch, boot, test, or build
    pub mode: String,
    /// Pipe errors through nixman's error explainer
    #[serde(default)]
    pub explain: bool,
    /// Automatically rollback to previous generation on failure
    #[serde(default)]
    pub rollback_on_fail: bool,
}

/// Parameters for the `try_apply` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TryApplyParams {
    /// Timeout in seconds before auto-revert (default 120)
    #[serde(default = "default_120")]
    pub timeout: u64,
}

/// Parameters for the `try_confirm` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TryConfirmParams;

/// Parameters for the `pending_apply` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PendingApplyParams;

/// Parameters for the `pending_discard` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PendingDiscardParams;

fn default_120() -> u64 {
    120
}

fn default_true() -> bool {
    true
}

/// A single staged change written to the pending file on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StagedChange {
    /// Kind: "option_set", "package_add", or "package_remove"
    #[serde(default = "default_option_kind")]
    pub kind: String,
    /// Option path (for option_set) or package name (for package_add/remove)
    pub option_path: String,
    /// Raw value string (for option_set; empty for package ops)
    #[serde(default)]
    pub value: String,
    /// Optional target file override
    #[serde(default)]
    pub file: Option<String>,
    /// Unix timestamp when the change was staged
    pub timestamp: String,
}

fn default_option_kind() -> String {
    "option_set".to_string()
}

/// Collection of staged changes persisted to disk.
#[derive(Debug, Default, Serialize, Deserialize)]
struct StagedChanges {
    pub changes: Vec<StagedChange>,
}

fn now_timestamp() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", dur.as_secs())
}

/// Compute the staging file path for a given workspace.
fn staging_path(workspace: &Path) -> PathBuf {
    let state_dir = std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".local").join("state")
        });

    let workspace_str = workspace.to_string_lossy();
    let hash: u64 = workspace_str
        .bytes()
        .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
    let filename = format!("pending-{:016x}.json", hash);

    state_dir.join("nixman").join(filename)
}

/// Load staged changes from disk. Returns empty if no staging file exists.
fn load_staged(workspace: &Path) -> StagedChanges {
    let path = staging_path(workspace);
    if !path.exists() {
        return StagedChanges::default();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Save staged changes to disk.
fn save_staged(staged: &StagedChanges, workspace: &Path) -> Result<(), String> {
    let path = staging_path(workspace);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(staged).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}


/// Persisted state for an active try session.
#[derive(Debug, Serialize, Deserialize)]
struct TryState {
    pub workspace: String,
    pub timeout: u64,
    pub started_at: u64,
}

/// Path to the try-state file.
fn try_state_path() -> PathBuf {
    let state_dir = std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".local").join("state")
        });
    state_dir.join("nixman").join("try-state.json")
}

fn save_try_state(state: &TryState) -> Result<(), String> {
    let path = try_state_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

fn load_try_state() -> Result<TryState, String> {
    let path = try_state_path();
    if !path.exists() {
        return Err("No active try session. Run 'try_apply' first.".to_string());
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

fn remove_try_state() {
    let _ = std::fs::remove_file(try_state_path());
}

/// Parse a CLI string into a `NixValue` using the same heuristics as the
/// nixman CLI.
fn parse_nix_value(s: &str) -> NixValue {
    match s {
        "true" => NixValue::Bool(true),
        "false" => NixValue::Bool(false),
        "null" => NixValue::Null,
        _ => {
            if let Ok(n) = s.parse::<i64>() {
                return NixValue::Int(n);
            }

            if s.starts_with('[') || s.starts_with('{') {
                if let Some(value) = try_parse_nix_expr(s) {
                    return value;
                }
            }

            if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
                let inner = &s[1..s.len() - 1];
                let unescaped = inner.replace("\\\"", "\"").replace("\\\\", "\\");
                return NixValue::String(unescaped);
            }

            if s.contains('.') || s.contains(' ') || s.contains('(') || s.contains('$') {
                NixValue::Expression(s.to_string())
            } else {
                NixValue::String(s.to_string())
            }
        }
    }
}

fn try_parse_nix_expr(s: &str) -> Option<NixValue> {
    let nix_file = nixman_core::nix_parser::reader::parse_string(s).ok()?;
    let expr = nix_file.root.expr()?;
    Some(nixman_core::nix_parser::traversal::expr_to_value(&expr))
}

/// Shared state for the MCP server.
#[derive(Clone)]
#[allow(dead_code)]
pub struct NixmanMcpServer {
    /// Path to the NixOS workspace (e.g. `/etc/nixos`).
    workspace: Arc<Mutex<String>>,
    /// Tool router generated by the `#[tool_router]` macro.
    tool_router: ToolRouter<NixmanMcpServer>,
}

impl NixmanMcpServer {
    /// Create a new server pointing at the given workspace path.
    pub fn new(workspace: String) -> Self {
        Self {
            workspace: Arc::new(Mutex::new(workspace)),
            tool_router: Self::tool_router(),
        }
    }

    /// Return the current workspace path.
    async fn workspace(&self) -> String {
        self.workspace.lock().await.clone()
    }
}

#[tool_router]
impl NixmanMcpServer {

    /// Get the current value of a NixOS configuration option.
    ///
    /// Returns the option value as JSON, or a message indicating the option
    /// is not set, or an error if the option cannot be read.
    #[tool(description = "Get the value of a NixOS configuration option")]
    async fn option_get(
        &self,
        Parameters(OptionGetParams { path }): Parameters<OptionGetParams>,
    ) -> Result<CallToolResult, McpError> {
        let ws = self.workspace().await;
        let ws_path = Path::new(&ws);

        match editor::get_value(ws_path, &path) {
            Ok(Some(value)) => {
                let json = serde_json::to_string_pretty(&value)
                    .unwrap_or_else(|_| format!("{value:?}"));
                Ok(CallToolResult::success(vec![Content::text(json)]))
            }
            Ok(None) => Ok(CallToolResult::success(vec![Content::text(
                format!("Option `{path}` is not set in the configuration."),
            )])),
            Err(e) => Err(McpError::internal_error(
                format!("Failed to read option `{path}`: {e}"),
                None,
            )),
        }
    }


    /// Search nixpkgs for packages matching a query string.
    ///
    /// Runs `nix search <flake>#nixpkgs <query> --json` and returns the
    /// matching packages with name, version, and description.
    #[tool(description = "Search nixpkgs for packages matching a query")]
    async fn packages_search(
        &self,
        Parameters(PackagesSearchParams { query }): Parameters<PackagesSearchParams>,
    ) -> Result<CallToolResult, McpError> {
        let ws = self.workspace().await;
        let ws_path = Path::new(&ws);

        match nixman_core::packages::search::query(ws_path, &query).await {
            Ok(result) => {
                let json = serde_json::to_string_pretty(&result)
                    .unwrap_or_else(|_| format!("{result:?}"));
                Ok(CallToolResult::success(vec![Content::text(json)]))
            }
            Err(e) => Err(McpError::internal_error(
                format!("Package search failed: {e}"),
                None,
            )),
        }
    }


    /// Run pre-flight checks on the NixOS configuration.
    ///
    /// Verifies flakes are enabled, the configuration syntax is valid, and
    /// the workspace directory exists.
    #[tool(description = "Run pre-flight checks on the NixOS configuration")]
    async fn check(
        &self,
        _params: Parameters<CheckParams>,
    ) -> Result<CallToolResult, McpError> {
        let ws = self.workspace().await;
        let ws_path = Path::new(&ws);
        let mut checks: Vec<serde_json::Value> = Vec::new();

        // 1. Flakes availability
        match nixman_core::preflight::check_flakes_enabled() {
            Ok(()) => checks.push(serde_json::json!({
                "name": "flakes",
                "passed": true,
                "message": "Flakes are enabled"
            })),
            Err(e) => checks.push(serde_json::json!({
                "name": "flakes",
                "passed": false,
                "message": e
            })),
        }

        // 2. Config syntax validation
        let is_flake = ws_path.join("flake.nix").exists();
        if is_flake {
            let flake_ref = ws.to_string();
            let attr = format!(
                "{}#nixosConfigurations.{}.config.system.build.toplevel",
                flake_ref,
                nixman_core::workspace::detect::get_hostname()
            );
            let output = std::process::Command::new("nix")
                .args(["eval", &attr, "--json"])
                .stderr(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .output();

            match output {
                Ok(o) if o.status.success() => checks.push(serde_json::json!({
                    "name": "config_validity",
                    "passed": true,
                    "message": "Configuration syntax is valid"
                })),
                Ok(o) => checks.push(serde_json::json!({
                    "name": "config_validity",
                    "passed": false,
                    "message": String::from_utf8_lossy(&o.stderr).trim().to_string()
                })),
                Err(e) => checks.push(serde_json::json!({
                    "name": "config_validity",
                    "passed": false,
                    "message": format!("Failed to run nix eval: {e}")
                })),
            }
        } else {
            let config_path = ws_path.join("configuration.nix");
            let output = std::process::Command::new("nix-instantiate")
                .args(["--parse", &config_path.to_string_lossy()])
                .stderr(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .output();

            match output {
                Ok(o) if o.status.success() => checks.push(serde_json::json!({
                    "name": "config_validity",
                    "passed": true,
                    "message": "Configuration syntax is valid"
                })),
                Ok(o) => checks.push(serde_json::json!({
                    "name": "config_validity",
                    "passed": false,
                    "message": String::from_utf8_lossy(&o.stderr).trim().to_string()
                })),
                Err(e) => checks.push(serde_json::json!({
                    "name": "config_validity",
                    "passed": false,
                    "message": format!("Failed to run nix-instantiate: {e}")
                })),
            }
        }

        // 3. Workspace existence
        if ws_path.exists() {
            checks.push(serde_json::json!({
                "name": "workspace",
                "passed": true,
                "message": format!("Workspace found at {}", ws_path.display())
            }));
        } else {
            checks.push(serde_json::json!({
                "name": "workspace",
                "passed": false,
                "message": format!("Workspace not found at {}", ws_path.display())
            }));
        }

        let all_passed = checks.iter().all(|c| c["passed"].as_bool().unwrap_or(false));
        let output = serde_json::json!({
            "valid": all_passed,
            "checks": checks,
        });
        let json = serde_json::to_string_pretty(&output).unwrap_or_default();
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }


    /// Run system diagnostics: network, DNS, services, filesystems.
    ///
    /// Checks connectivity to the default gateway, DNS resolution via
    /// `getent hosts nixos.org`, failed systemd services, and disk usage.
    #[tool(description = "Run system diagnostics (network, DNS, services, filesystems)")]
    async fn doctor(
        &self,
        _params: Parameters<DoctorParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut checks: Vec<serde_json::Value> = Vec::new();

        let gateway = tokio::process::Command::new("ip")
            .args(["route", "show", "default"])
            .output()
            .await
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    String::from_utf8_lossy(&o.stdout)
                        .split_whitespace()
                        .nth(2)
                        .map(|s| s.to_string())
                } else {
                    None
                }
            });

        if let Some(ref gw) = gateway {
            let ping = tokio::process::Command::new("ping")
                .args(["-c", "1", "-W", "3", gw])
                .output()
                .await;
            match ping {
                Ok(o) if o.status.success() => checks.push(serde_json::json!({
                    "name": "network",
                    "passed": true,
                    "message": format!("Gateway {gw} reachable")
                })),
                _ => checks.push(serde_json::json!({
                    "name": "network",
                    "passed": false,
                    "message": format!("Cannot reach gateway {gw}")
                })),
            }
        } else {
            checks.push(serde_json::json!({
                "name": "network",
                "passed": false,
                "message": "No default route found"
            }));
        }

        let dns = tokio::process::Command::new("getent")
            .args(["hosts", "nixos.org"])
            .output()
            .await;
        match dns {
            Ok(o) if o.status.success() => checks.push(serde_json::json!({
                "name": "dns",
                "passed": true,
                "message": "DNS resolution working"
            })),
            _ => checks.push(serde_json::json!({
                "name": "dns",
                "passed": false,
                "message": "DNS resolution failed (getent hosts nixos.org)"
            })),
        }

        let failed_svc = tokio::process::Command::new("systemctl")
            .args(["--failed", "--no-legend", "--plain"])
            .output()
            .await;
        match failed_svc {
            Ok(o) if o.status.success() => {
                let text = String::from_utf8_lossy(&o.stdout).to_string();
                let failed: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
                if failed.is_empty() {
                    checks.push(serde_json::json!({
                        "name": "services",
                        "passed": true,
                        "message": "No failed services"
                    }));
                } else {
                    let names: Vec<&str> = failed
                        .iter()
                        .take(5)
                        .filter_map(|l| l.split_whitespace().next())
                        .collect();
                    checks.push(serde_json::json!({
                        "name": "services",
                        "passed": false,
                        "message": format!("{} failed service(s): {}", failed.len(), names.join(", "))
                    }));
                }
            }
            _ => checks.push(serde_json::json!({
                "name": "services",
                "passed": true,
                "message": "Could not check service status"
            })),
        }

        let df = tokio::process::Command::new("df")
            .args(["-h", "/", "/nix"])
            .output()
            .await;
        match df {
            Ok(o) if o.status.success() => {
                let text = String::from_utf8_lossy(&o.stdout).to_string();
                let mut warnings: Vec<String> = Vec::new();
                for line in text.lines().skip(1) {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 6 {
                        if let Some(pct) = parts[4].strip_suffix('%') {
                            if let Ok(pct) = pct.parse::<u32>() {
                                if pct > 90 {
                                    warnings.push(format!("{} at {}%", parts[5], pct));
                                }
                            }
                        }
                    }
                }
                if warnings.is_empty() {
                    checks.push(serde_json::json!({
                        "name": "filesystems",
                        "passed": true,
                        "message": "Disk usage normal"
                    }));
                } else {
                    checks.push(serde_json::json!({
                        "name": "filesystems",
                        "passed": false,
                        "message": format!("High disk usage: {}", warnings.join(", "))
                    }));
                }
            }
            _ => checks.push(serde_json::json!({
                "name": "filesystems",
                "passed": true,
                "message": "Could not check disk usage"
            })),
        }

        let all_passed = checks.iter().all(|c| c["passed"].as_bool().unwrap_or(false));
        let output = serde_json::json!({ "healthy": all_passed, "checks": checks });
        let json = serde_json::to_string_pretty(&output).unwrap_or_default();
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }


    /// Return workspace information: path, kind (flake/legacy), hostname,
    /// and ownership.
    #[tool(description = "Return workspace information")]
    async fn status(
        &self,
        _params: Parameters<StatusParams>,
    ) -> Result<CallToolResult, McpError> {
        match nixman_core::workspace::detect() {
            Ok(ws) => {
                let info = serde_json::json!({
                    "path": ws.path,
                    "kind": format!("{:?}", ws.kind),
                    "hostname": ws.hostname,
                    "owner": {
                        "uid": ws.owner.uid,
                        "is_user_owned": ws.owner.is_user_owned,
                    }
                });
                let json = serde_json::to_string_pretty(&info).unwrap_or_default();
                Ok(CallToolResult::success(vec![Content::text(json)]))
            }
            Err(e) => {
                let info = serde_json::json!({
                    "error": format!("{e}"),
                    "configured_path": self.workspace().await,
                });
                let json = serde_json::to_string_pretty(&info).unwrap_or_default();
                Ok(CallToolResult::success(vec![Content::text(json)]))
            }
        }
    }


    /// List all staged (pending) changes that have been queued but not yet
    /// applied to disk.
    #[tool(description = "List all staged (pending) changes")]
    async fn pending_list(
        &self,
        _params: Parameters<PendingListParams>,
    ) -> Result<CallToolResult, McpError> {
        let ws = self.workspace().await;
        let ws_path = Path::new(&ws);
        let staged = load_staged(ws_path);

        if staged.changes.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No pending changes.",
            )]));
        }

        let json =
            serde_json::to_string_pretty(&staged.changes).unwrap_or_else(|_| format!("{:?}", staged.changes));
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }


    /// Set a NixOS configuration option to a new value.
    ///
    /// When `stage` is true, the change is queued in the pending file without
    /// writing to disk. When false, the change is written immediately and
    /// validated before writing.
    #[tool(description = "Set a NixOS configuration option")]
    async fn option_set(
        &self,
        Parameters(OptionSetParams {
            path,
            value,
            stage,
        }): Parameters<OptionSetParams>,
    ) -> Result<CallToolResult, McpError> {
        let ws = self.workspace().await;
        let ws_path = Path::new(&ws);

        if stage {
            let mut staged = load_staged(ws_path);
            staged.add(
                path.clone(),
                value.clone(),
                "option_set".into(),
                None,
            );
            save_staged(&staged, ws_path).map_err(|e| {
                McpError::internal_error(format!("Failed to stage option `{path}`: {e}"), None)
            })?;
            Ok(CallToolResult::success(vec![Content::text(
                format!("Staged: option_set {} = {}. Run pending_apply to commit, or pending_discard to cancel.", path, value),
            )]))
        } else {
            let nix_value = parse_nix_value(&value);
            let mut pending = nixman_core::config::PendingChanges::new();
            editor::set_value(&mut pending, ws_path, &path, nix_value).map_err(|e| {
                McpError::internal_error(format!("Failed to set option `{path}`: {e}"), None)
            })?;
            editor::apply_pending(&mut pending, ws_path).map_err(|e| {
                McpError::internal_error(format!("Failed to apply option `{path}`: {e}"), None)
            })?;
            Ok(CallToolResult::success(vec![Content::text(
                format!("Option `{path}` set successfully"),
            )]))
        }
    }


    /// Add a package to `environment.systemPackages`.
    ///
    /// Optionally verifies the package exists in nixpkgs first (unless
    /// `no_verify` is true).  When `stage` is true, the change is queued
    /// in the pending file instead of writing immediately.
    #[tool(description = "Add a package to environment.systemPackages")]
    async fn packages_add(
        &self,
        Parameters(PackagesAddParams {
            package,
            stage,
            no_verify,
        }): Parameters<PackagesAddParams>,
    ) -> Result<CallToolResult, McpError> {
        let ws = self.workspace().await;
        let ws_path = Path::new(&ws);

        if stage {
            let mut staged = load_staged(ws_path);
            staged.add(
                package.clone(),
                String::new(),
                "package_add".into(),
                None,
            );
            save_staged(&staged, ws_path).map_err(|e| {
                McpError::internal_error(
                    format!("Failed to stage package `{package}`: {e}"),
                    None,
                )
            })?;
            return Ok(CallToolResult::success(vec![Content::text(
                format!("Staged: packages_add {}. Run pending_apply to commit, or pending_discard to cancel.", package),
            )]));
        }

        if !no_verify {
            nixman_core::packages::manage::verify_package(&package).map_err(|e| {
                McpError::internal_error(
                    format!("Package verification failed for `{package}`: {e}"),
                    None,
                )
            })?;
        }

        match nixman_core::packages::manage::add(ws_path, &package, None) {
            Ok(true) => Ok(CallToolResult::success(vec![Content::text(
                format!("Package `{package}` added to environment.systemPackages"),
            )])),
            Ok(false) => Ok(CallToolResult::success(vec![Content::text(
                format!("Package `{package}` is already in environment.systemPackages"),
            )])),
            Err(e) => Err(McpError::internal_error(
                format!("Failed to add package `{package}`: {e}"),
                None,
            )),
        }
    }


    /// Remove a package from `environment.systemPackages`.
    ///
    /// When `stage` is true, the change is queued in the pending file instead
    /// of writing immediately.
    #[tool(description = "Remove a package from environment.systemPackages")]
    async fn packages_remove(
        &self,
        Parameters(PackagesRemoveParams { package, stage }): Parameters<PackagesRemoveParams>,
    ) -> Result<CallToolResult, McpError> {
        let ws = self.workspace().await;
        let ws_path = Path::new(&ws);

        if stage {
            let mut staged = load_staged(ws_path);
            staged.add(
                package.clone(),
                String::new(),
                "package_remove".into(),
                None,
            );
            save_staged(&staged, ws_path).map_err(|e| {
                McpError::internal_error(
                    format!("Failed to stage removal of `{package}`: {e}"),
                    None,
                )
            })?;
            return Ok(CallToolResult::success(vec![Content::text(
                format!("Staged: packages_remove {}. Run pending_apply to commit, or pending_discard to cancel.", package),
            )]));
        }

        match nixman_core::packages::manage::remove(ws_path, &package, None) {
            Ok(true) => Ok(CallToolResult::success(vec![Content::text(
                format!("Package `{package}` removed from environment.systemPackages"),
            )])),
            Ok(false) => Ok(CallToolResult::success(vec![Content::text(
                format!("Package `{package}` was not in environment.systemPackages"),
            )])),
            Err(e) => Err(McpError::internal_error(
                format!("Failed to remove package `{package}`: {e}"),
                None,
            )),
        }
    }


    /// Run `nixos-rebuild` to apply configuration changes.
    ///
    /// Supports switch, boot, test, and build modes.  Optionally pipes errors
    /// through nixman's explainer and/or auto-rolls back on failure.
    #[tool(description = "Run nixos-rebuild to apply configuration changes")]
    async fn rebuild(
        &self,
        Parameters(RebuildParams {
            mode,
            explain,
            rollback_on_fail,
        }): Parameters<RebuildParams>,
    ) -> Result<CallToolResult, McpError> {
        let build_mode = match mode.as_str() {
            "switch" => nixman_core::builder::BuildMode::Switch,
            "boot" => nixman_core::builder::BuildMode::Boot,
            "test" => nixman_core::builder::BuildMode::Test,
            "build" => nixman_core::builder::BuildMode::Build,
            other => {
                return Err(McpError::internal_error(
                    format!("Unknown build mode '{other}'. Use switch, boot, test, or build."),
                    None,
                ))
            }
        };

        let ws = self.workspace().await;
        let ws_path = Path::new(&ws).to_path_buf();

        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let handle =
            tokio::spawn(async move { nixman_core::builder::rebuild::run(build_mode, &ws_path, tx).await });

        let mut output_lines: Vec<String> = Vec::new();
        while let Some(event) = rx.recv().await {
            match event {
                nixman_core::builder::BuildEvent::Output(line) => {
                    output_lines.push(line);
                }
                nixman_core::builder::BuildEvent::PhaseChanged(phase) => {
                    output_lines.push(format!("[phase: {:?}]", phase));
                }
                nixman_core::builder::BuildEvent::Complete(_) => break,
            }
        }

        let result = handle.await.map_err(|e| {
            McpError::internal_error(format!("Build task panicked: {e}"), None)
        })?;

        match result {
            Ok(build_result) => {
                if build_result.success {
                    let json = serde_json::json!({
                        "success": true,
                        "duration_secs": build_result.duration_secs,
                        "output": build_result.output,
                    });
                    Ok(CallToolResult::success(vec![Content::text(
                        serde_json::to_string_pretty(&json).unwrap_or_default(),
                    )]))
                } else {
                    if rollback_on_fail {
                        let _ = tokio::process::Command::new("pkexec")
                            .args(["nixos-rebuild", "switch", "--rollback"])
                            .output()
                            .await;
                    }

                    let error_msg = build_result
                        .error
                        .unwrap_or_else(|| "nixos-rebuild failed".to_string());

                    if explain {
                        // Build a simple explanation inline
                        let explanation = serde_json::json!({
                            "error": error_msg,
                            "suggestion": "Check nixos-rebuild output above for details"
                        });
                        let json = serde_json::json!({
                            "success": false,
                            "duration_secs": build_result.duration_secs,
                            "error": error_msg,
                            "explanation": explanation,
                            "output": build_result.output,
                        });
                        Ok(CallToolResult::success(vec![Content::text(
                            serde_json::to_string_pretty(&json).unwrap_or_default(),
                        )]))
                    } else {
                        let json = serde_json::json!({
                            "success": false,
                            "duration_secs": build_result.duration_secs,
                            "error": error_msg,
                            "output": build_result.output,
                        });
                        Ok(CallToolResult::success(vec![Content::text(
                            serde_json::to_string_pretty(&json).unwrap_or_default(),
                        )]))
                    }
                }
            }
            Err(build_error) => Err(McpError::internal_error(
                format!("Build failed: {build_error}"),
                None,
            )),
        }
    }


    /// Start a try session: apply staged changes in test mode with an
    /// auto-revert timeout.  If the build succeeds the changes are active
    /// until the timeout expires or `try_confirm` is called.
    #[tool(description = "Apply staged changes in test mode with auto-revert timeout")]
    async fn try_apply(
        &self,
        Parameters(TryApplyParams { timeout }): Parameters<TryApplyParams>,
    ) -> Result<CallToolResult, McpError> {
        let ws = self.workspace().await;
        let ws_path = Path::new(&ws).to_path_buf();
        let ws_clone = ws.clone();

        let staged = load_staged(&ws_path);
        if staged.changes.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No staged changes to apply. Use option_set, packages_add, or packages_remove with stage=true first.",
            )]));
        }

        let mut applied_paths: Vec<String> = Vec::new();

        for change in &staged.changes {
            match change.kind.as_str() {
                "package_add" => {
                    let file = change.file.as_ref().map(|f| Path::new(f.as_str()));
                    nixman_core::packages::manage::add(&ws_path, &change.option_path, file)
                        .map_err(|e| {
                            McpError::internal_error(
                                format!("Failed to add package '{}': {e}", change.option_path),
                                None,
                            )
                        })?;
                    applied_paths.push(format!("package_add:{}", change.option_path));
                }
                "package_remove" => {
                    let file = change.file.as_ref().map(|f| Path::new(f.as_str()));
                    nixman_core::packages::manage::remove(&ws_path, &change.option_path, file)
                        .map_err(|e| {
                            McpError::internal_error(
                                format!("Failed to remove package '{}': {e}", change.option_path),
                                None,
                            )
                        })?;
                    applied_paths.push(format!("package_remove:{}", change.option_path));
                }
                _ => {
                    let nix_value = parse_nix_value(&change.value);
                    let mut pending = nixman_core::config::PendingChanges::new();
                    editor::set_value(&mut pending, &ws_path, &change.option_path, nix_value)
                        .map_err(|e| {
                            McpError::internal_error(
                                format!("Failed to set option '{}': {e}", change.option_path),
                                None,
                            )
                        })?;
                    editor::apply_pending(&mut pending, &ws_path).map_err(|e| {
                        McpError::internal_error(
                            format!("Failed to apply option '{}': {e}", change.option_path),
                            None,
                        )
                    })?;
                    applied_paths.push(format!("option_set:{}", change.option_path));
                }
            }
        }

        let build_output = tokio::process::Command::new("sudo")
            .args(["nixos-rebuild", "test"])
            .current_dir(&ws_path)
            .output()
            .await
            .map_err(|e| {
                McpError::internal_error(format!("Failed to run nixos-rebuild: {e}"), None)
            })?;

        if !build_output.status.success() {
            let stderr = String::from_utf8_lossy(&build_output.stderr);
            // Revert via git checkout
            let _ = tokio::process::Command::new("git")
                .args(["checkout", "."])
                .current_dir(&ws_path)
                .output()
                .await;
            return Err(McpError::internal_error(
                format!("Build failed. Changes reverted.\n{stderr}"),
                None,
            ));
        }

        let state = TryState {
            workspace: ws_clone,
            timeout,
            started_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        save_try_state(&state).map_err(|e| {
            McpError::internal_error(format!("Failed to save try state: {e}"), None)
        })?;

        let _ = tokio::process::Command::new("systemd-run")
            .args([
                "--user",
                "--on-active",
                &format!("{timeout}s"),
                "--unit",
                "nixman-try-revert",
                "--description",
                "nixman try auto-revert",
                "bash",
                "-c",
                &format!(
                    "cd {} && git checkout . && sudo nixos-rebuild switch",
                    ws_path.display()
                ),
            ])
            .output()
            .await;

        let _changes_json = serde_json::to_string_pretty(&applied_paths).unwrap_or_default();
        let result = serde_json::json!({
            "success": true,
            "changes": applied_paths,
            "timeout": timeout,
            "message": format!(
                "Changes applied in test mode. Auto-revert in {} seconds.\n\
                 Run 'try_confirm' to make permanent, or wait for timeout.",
                timeout
            )
        });

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        )]))
    }


    /// Confirm a try session: cancel the auto-revert timer and run
    /// `nixos-rebuild switch` to make changes permanent.
    #[tool(description = "Confirm a try session and make changes permanent")]
    async fn try_confirm(
        &self,
        _params: Parameters<TryConfirmParams>,
    ) -> Result<CallToolResult, McpError> {
        let state = load_try_state().map_err(|e| {
            McpError::internal_error(format!("{e}"), None)
        })?;

        let ws_path = Path::new(&state.workspace).to_path_buf();

        let _ = tokio::process::Command::new("systemctl")
            .args(["--user", "stop", "nixman-try-revert.timer"])
            .output()
            .await;
        let _ = tokio::process::Command::new("systemctl")
            .args(["--user", "stop", "nixman-try-revert.service"])
            .output()
            .await;

        let output = tokio::process::Command::new("sudo")
            .args(["nixos-rebuild", "switch"])
            .current_dir(&ws_path)
            .output()
            .await
            .map_err(|e| {
                McpError::internal_error(format!("Failed to run nixos-rebuild: {e}"), None)
            })?;

        remove_try_state();

        if output.status.success() {
            Ok(CallToolResult::success(vec![Content::text(
                "Changes confirmed and made permanent.",
            )]))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(McpError::internal_error(
                format!("Failed to confirm: {stderr}"),
                None,
            ))
        }
    }


    /// Apply all staged (pending) changes immediately.
    ///
    /// Reads the pending changes file, applies each change using the
    /// appropriate core function, and clears the pending file on success.
    #[tool(description = "Apply all pending staged changes immediately")]
    async fn pending_apply(
        &self,
        _params: Parameters<PendingApplyParams>,
    ) -> Result<CallToolResult, McpError> {
        let ws = self.workspace().await;
        let ws_path = Path::new(&ws);
        let staged = load_staged(ws_path);

        if staged.changes.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No pending changes to apply.",
            )]));
        }

        let mut applied: Vec<String> = Vec::new();

        for change in &staged.changes {
            match change.kind.as_str() {
                "package_add" => {
                    let file = change.file.as_ref().map(|f| Path::new(f.as_str()));
                    nixman_core::packages::manage::add(&ws_path, &change.option_path, file)
                        .map_err(|e| {
                            McpError::internal_error(
                                format!("Failed to add package '{}': {e}", change.option_path),
                                None,
                            )
                        })?;
                    applied.push(format!("package_add:{}", change.option_path));
                }
                "package_remove" => {
                    let file = change.file.as_ref().map(|f| Path::new(f.as_str()));
                    nixman_core::packages::manage::remove(&ws_path, &change.option_path, file)
                        .map_err(|e| {
                            McpError::internal_error(
                                format!("Failed to remove package '{}': {e}", change.option_path),
                                None,
                            )
                        })?;
                    applied.push(format!("package_remove:{}", change.option_path));
                }
                _ => {
                    let nix_value = parse_nix_value(&change.value);
                    let mut pending = nixman_core::config::PendingChanges::new();
                    editor::set_value(&mut pending, &ws_path, &change.option_path, nix_value)
                        .map_err(|e| {
                            McpError::internal_error(
                                format!("Failed to set option '{}': {e}", change.option_path),
                                None,
                            )
                        })?;
                    editor::apply_pending(&mut pending, &ws_path).map_err(|e| {
                        McpError::internal_error(
                            format!("Failed to apply option '{}': {e}", change.option_path),
                            None,
                        )
                    })?;
                    applied.push(format!("option_set:{}", change.option_path));
                }
            }
        }

        save_staged(&StagedChanges::default(), ws_path).map_err(|e| {
            McpError::internal_error(format!("Failed to clear pending file: {e}"), None)
        })?;

        let result = serde_json::json!({
            "success": true,
            "applied": applied,
            "count": applied.len(),
            "message": format!(
                "Applied {} pending change(s). Run 'rebuild' or 'nixos-rebuild switch' to activate.",
                applied.len()
            )
        });

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        )]))
    }


    /// Discard all staged (pending) changes without applying them.
    #[tool(description = "Discard all pending staged changes without applying them")]
    async fn pending_discard(
        &self,
        _params: Parameters<PendingDiscardParams>,
    ) -> Result<CallToolResult, McpError> {
        let ws = self.workspace().await;
        let ws_path = Path::new(&ws);
        let staged = load_staged(ws_path);
        let count = staged.changes.len();

        save_staged(&StagedChanges::default(), ws_path).map_err(|e| {
            McpError::internal_error(format!("Failed to clear pending file: {e}"), None)
        })?;

        if count == 0 {
            Ok(CallToolResult::success(vec![Content::text(
                "No pending changes to discard.",
            )]))
        } else {
            Ok(CallToolResult::success(vec![Content::text(
                format!("Discarded {} pending change(s).", count),
            )]))
        }
    }
}

// Helper: add a staged change to the staged list (for option_set, packages_add, packages_remove).
impl StagedChanges {
    fn add(
        &mut self,
        option_path: String,
        value: String,
        kind: String,
        file: Option<String>,
    ) {
        if let Some(existing) = self
            .changes
            .iter_mut()
            .find(|c| c.kind == kind && c.option_path == option_path)
        {
            existing.value = value;
            existing.file = file;
            existing.timestamp = now_timestamp();
        } else {
            self.changes.push(StagedChange {
                kind,
                option_path,
                value,
                file,
                timestamp: now_timestamp(),
            });
        }
    }
}

#[tool_handler]
impl ServerHandler for NixmanMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .build(),
        )
        .with_server_info(Implementation::new(
            "nixman-mcp",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_protocol_version(ProtocolVersion::V_2024_11_05)
        .with_instructions(
            "NixOS configuration management tools.\n\n\
             Destructive tools (option_set, packages_add, packages_remove, \
             rebuild, try_apply) are staged by default. Use pending_apply to \
             commit or pending_discard to cancel.\n\n\
             Tools:\n\
             - option_get: Get the value of a NixOS configuration option\n\
             - packages_search: Search nixpkgs for packages\n\
             - check: Run pre-flight checks on the configuration\n\
             - doctor: Run system diagnostics\n\
             - status: Return workspace information\n\
             - pending_list: List all staged (pending) changes\n\
             - pending_apply: Apply all pending staged changes immediately\n\
             - pending_discard: Discard all pending staged changes\n\
             - option_set: Set a NixOS configuration option\n\
             - packages_add: Add a package to environment.systemPackages\n\
             - packages_remove: Remove a package from environment.systemPackages\n\
             - rebuild: Run nixos-rebuild to apply changes\n\
             - try_apply: Apply staged changes in test mode with auto-revert\n\
             - try_confirm: Confirm a try session and make changes permanent",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::sync::Mutex;

    static XDG_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn parse_bool() {
        assert_eq!(parse_nix_value("true"), NixValue::Bool(true));
        assert_eq!(parse_nix_value("false"), NixValue::Bool(false));
    }

    #[test]
    fn parse_null() {
        assert_eq!(parse_nix_value("null"), NixValue::Null);
    }

    #[test]
    fn parse_int() {
        assert_eq!(parse_nix_value("42"), NixValue::Int(42));
        assert_eq!(parse_nix_value("-7"), NixValue::Int(-7));
    }

    #[test]
    fn parse_list_of_strings() {
        let v = parse_nix_value(r#"[ "nfs" ]"#);
        match v {
            NixValue::List(items) => {
                assert_eq!(items.len(), 1);
                assert!(matches!(&items[0], NixValue::String(s) if s == "nfs"));
            }
            _ => panic!("expected list, got {:?}", v),
        }
    }

    #[test]
    fn parse_list_multiple() {
        let v = parse_nix_value(r#"[ "wheel" "docker" ]"#);
        match v {
            NixValue::List(items) => assert_eq!(items.len(), 2),
            _ => panic!("expected list"),
        }
    }

    #[test]
    fn parse_expression() {
        let v = parse_nix_value("pkgs.linuxPackages_latest");
        assert!(matches!(v, NixValue::Expression(_)));
    }

    #[test]
    fn parse_simple_string() {
        assert_eq!(parse_nix_value("hello"), NixValue::String("hello".into()));
    }

    #[test]
    fn parse_quoted_string() {
        assert_eq!(
            parse_nix_value(r#""hello world""#),
            NixValue::String("hello world".into())
        );
    }

    #[test]
    fn staged_add_new() {
        let mut c = StagedChanges::default();
        c.add("opt".into(), "true".into(), "option_set".into(), None);
        assert_eq!(c.changes.len(), 1);
        assert_eq!(c.changes[0].kind, "option_set");
        assert_eq!(c.changes[0].option_path, "opt");
        assert_eq!(c.changes[0].value, "true");
        assert!(c.changes[0].file.is_none());
        c.changes[0].timestamp.parse::<u64>().expect("timestamp must be u64");
    }

    #[test]
    fn staged_update_existing() {
        let mut c = StagedChanges::default();
        c.add("opt".into(), "v1".into(), "option_set".into(), None);
        c.add("opt".into(), "v2".into(), "option_set".into(), None);
        assert_eq!(c.changes.len(), 1);
        assert_eq!(c.changes[0].value, "v2");
    }

    #[test]
    fn staged_separate_entries() {
        let mut c = StagedChanges::default();
        c.add("opt".into(), "true".into(), "option_set".into(), None);
        c.add("pkg".into(), String::new(), "package_add".into(), None);
        assert_eq!(c.changes.len(), 2);
    }

    #[test]
    fn staged_with_file_override() {
        let mut c = StagedChanges::default();
        c.add("opt".into(), "true".into(), "option_set".into(), Some("override.nix".into()));
        assert_eq!(c.changes[0].file.as_deref(), Some("override.nix"));
    }

    #[test]
    fn load_save_roundtrip() {
        let _guard = XDG_MUTEX.lock().unwrap();
        let state = TempDir::new().unwrap();
        let ws = TempDir::new().unwrap();
        std::env::set_var("XDG_STATE_HOME", state.path());

        let mut staged = StagedChanges::default();
        staged.add("test.opt".into(), "42".into(), "option_set".into(), None);
        save_staged(&staged, ws.path()).unwrap();

        let loaded = load_staged(ws.path());
        assert_eq!(loaded.changes.len(), 1);
        assert_eq!(loaded.changes[0].option_path, "test.opt");
        assert_eq!(loaded.changes[0].value, "42");

    }

    #[test]
    fn load_nonexistent_returns_empty() {
        let ws = TempDir::new().unwrap();
        let loaded = load_staged(ws.path());
        assert!(loaded.changes.is_empty());
    }

    #[test]
    fn now_timestamp_is_valid() {
        let ts = now_timestamp();
        let secs: u64 = ts.parse().expect("timestamp must be a valid u64");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(now >= secs, "timestamp must not be in the future");
        assert!(now - secs < 10, "timestamp must be recent (within 10s)");
    }

    /// Extract concatenated text from a CallToolResult.
    fn extract_text(result: &CallToolResult) -> String {
        result
            .content
            .iter()
            .filter_map(|c| c.as_text().map(|t| t.text.as_str()))
            .collect::<Vec<_>>()
            .join("")
    }

    /// Set up a test environment with isolated state and workspace dirs.
    /// Returns (state_dir, workspace_dir, server) — order matters for drop.
    async fn staging_env() -> (TempDir, TempDir, NixmanMcpServer, std::sync::MutexGuard<'static, ()>) {
        let guard = XDG_MUTEX.lock().unwrap();
         let state = TempDir::new().unwrap();
         let ws = TempDir::new().unwrap();
         std::env::set_var("XDG_STATE_HOME", state.path());
         let server = NixmanMcpServer::new(ws.path().to_string_lossy().to_string());
         (state, ws, server, guard)
    }

    #[tokio::test]
    async fn option_set_default_stages() {
        let (_state, ws, server, _guard) = staging_env().await;
        let ws_path = ws.path().to_path_buf();

        let result = server
            .option_set(Parameters(OptionSetParams {
                path: "services.nginx.enable".into(),
                value: "true".into(),
                stage: true,
            }))
            .await;
        assert!(result.is_ok(), "option_set with stage=true should succeed");
        assert!(
            extract_text(&result.unwrap()).contains("Staged:"),
            "response should indicate staged"
        );

        let staged = load_staged(&ws_path);
        assert_eq!(staged.changes.len(), 1, "one change should be staged");
        assert_eq!(staged.changes[0].option_path, "services.nginx.enable");
        assert_eq!(staged.changes[0].value, "true");
        assert_eq!(staged.changes[0].kind, "option_set");
    }

    #[tokio::test]
    async fn option_set_stage_false_errors_without_config() {
        let (_state, ws, server, _guard) = staging_env().await;
        let ws_path = ws.path().to_path_buf();

        let result = server
            .option_set(Parameters(OptionSetParams {
                path: "test.option".into(),
                value: "true".into(),
                stage: false,
            }))
            .await;
        // Without a real NixOS config, this will fail — that's expected.
        assert!(result.is_err(), "option_set with stage=false should fail without real config");

        // Confirm no staging file was created as a side effect.
        let staged = load_staged(&ws_path);
        assert!(
            staged.changes.is_empty(),
            "option_set with stage=false should not create a staging file"
        );
    }

    #[tokio::test]
    async fn packages_add_default_stages() {
        let (_state, ws, server, _guard) = staging_env().await;
        let ws_path = ws.path().to_path_buf();

        let result = server
            .packages_add(Parameters(PackagesAddParams {
                package: "btop".into(),
                stage: true,
                no_verify: true,
            }))
            .await;
        assert!(result.is_ok(), "packages_add with stage=true should succeed");
        assert!(
            extract_text(&result.unwrap()).contains("Staged:"),
            "response should indicate staged"
        );

        let staged = load_staged(&ws_path);
        assert_eq!(staged.changes.len(), 1);
        assert_eq!(staged.changes[0].kind, "package_add");
        assert_eq!(staged.changes[0].option_path, "btop");
    }

    #[tokio::test]
    async fn packages_add_stage_false_errors_without_config() {
        let (_state, ws, server, _guard) = staging_env().await;
        let ws_path = ws.path().to_path_buf();

        let result = server
            .packages_add(Parameters(PackagesAddParams {
                package: "btop".into(),
                stage: false,
                no_verify: true,
            }))
            .await;
        // Without a real NixOS config, this will fail — that's expected.
        assert!(result.is_err(), "packages_add with stage=false should fail without real config");

        let staged = load_staged(&ws_path);
        assert!(
            staged.changes.is_empty(),
            "packages_add with stage=false should not create a staging file"
        );
    }

    #[tokio::test]
    async fn packages_remove_default_stages() {
        let (_state, ws, server, _guard) = staging_env().await;
        let ws_path = ws.path().to_path_buf();

        let result = server
            .packages_remove(Parameters(PackagesRemoveParams {
                package: "htop".into(),
                stage: true,
            }))
            .await;
        assert!(result.is_ok(), "packages_remove with stage=true should succeed");
        assert!(
            extract_text(&result.unwrap()).contains("Staged:"),
            "response should indicate staged"
        );

        let staged = load_staged(&ws_path);
        assert_eq!(staged.changes.len(), 1);
        assert_eq!(staged.changes[0].kind, "package_remove");
        assert_eq!(staged.changes[0].option_path, "htop");
    }

    #[tokio::test]
    async fn read_only_tools_dont_create_staging() {
        let (_state, ws, server, _guard) = staging_env().await;
        let ws_path = ws.path().to_path_buf();

        // option_get
        let _ = server
            .option_get(Parameters(OptionGetParams {
                path: "services.nginx.enable".into(),
            }))
            .await;
        assert!(
            load_staged(&ws_path).changes.is_empty(),
            "option_get should not create staging file"
        );

        // check
        let _ = server.check(Parameters(CheckParams)).await;
        assert!(
            load_staged(&ws_path).changes.is_empty(),
            "check should not create staging file"
        );

        // status
        let _ = server.status(Parameters(StatusParams)).await;
        assert!(
            load_staged(&ws_path).changes.is_empty(),
            "status should not create staging file"
        );

        // pending_list
        let _ = server.pending_list(Parameters(PendingListParams)).await;
        assert!(
            load_staged(&ws_path).changes.is_empty(),
            "pending_list should not create staging file"
        );

        // doctor
        let _ = server.doctor(Parameters(DoctorParams)).await;
        assert!(
            load_staged(&ws_path).changes.is_empty(),
            "doctor should not create staging file"
        );
    }

    #[tokio::test]
    async fn pending_lifecycle_stage_list_discard() {
        let (_state, ws, server, _guard) = staging_env().await;
        let ws_path = ws.path().to_path_buf();

        // Stage an option change
        server
            .option_set(Parameters(OptionSetParams {
                path: "test.option".into(),
                value: "true".into(),
                stage: true,
            }))
            .await
            .unwrap();
        assert_eq!(load_staged(&ws_path).changes.len(), 1);

        // pending_list shows it
        let list_resp = server.pending_list(Parameters(PendingListParams)).await.unwrap();
        let list_text = extract_text(&list_resp);
        assert!(list_text.contains("test.option"), "pending_list should show staged change");

        // pending_discard clears it
        let discard = server.pending_discard(Parameters(PendingDiscardParams)).await.unwrap();
        assert!(
            extract_text(&discard).contains("Discarded 1"),
            "should report 1 discarded change"
        );

        // Verify staging file is empty
        assert!(
            load_staged(&ws_path).changes.is_empty(),
            "staging file should be empty after discard"
        );
    }

    #[tokio::test]
    async fn pending_apply_fails_and_keeps_staged() {
        let (_state, ws, server, _guard) = staging_env().await;
        let ws_path = ws.path().to_path_buf();

        // Stage a change
        server
            .option_set(Parameters(OptionSetParams {
                path: "test.option".into(),
                value: "true".into(),
                stage: true,
            }))
            .await
            .unwrap();
        assert_eq!(load_staged(&ws_path).changes.len(), 1);

        // Apply — will fail because there's no real NixOS configuration
        let result = server.pending_apply(Parameters(PendingApplyParams)).await;
        assert!(
            result.is_err(),
            "pending_apply should fail without real NixOS configuration"
        );

        // Staging file must persist on failure (not cleared)
        assert_eq!(
            load_staged(&ws_path).changes.len(),
            1,
            "staging file should persist when pending_apply fails"
        );
    }

    #[tokio::test]
    async fn pending_apply_empty_returns_message() {
        let (_state, _ws, server, _guard) = staging_env().await;

        let result = server.pending_apply(Parameters(PendingApplyParams)).await;
        assert!(result.is_ok());
        assert!(
            extract_text(&result.unwrap()).contains("No pending changes"),
            "should report no pending changes"
        );
    }

    #[tokio::test]
    async fn pending_discard_empty_returns_message() {
        let (_state, _ws, server, _guard) = staging_env().await;

        let result = server.pending_discard(Parameters(PendingDiscardParams)).await;
        assert!(result.is_ok());
        assert!(
            extract_text(&result.unwrap()).contains("No pending changes"),
            "should report no pending changes"
        );
    }

    #[tokio::test]
    async fn packages_search_does_not_stage() {
        let (_state, ws, server, _guard) = staging_env().await;
        let ws_path = ws.path().to_path_buf();

        let _ = server
            .packages_search(Parameters(PackagesSearchParams {
                query: "btop".into(),
            }))
            .await;

        let staged = load_staged(&ws_path);
        assert!(
            staged.changes.is_empty(),
            "packages_search should not create staging file"
        );
    }
}
