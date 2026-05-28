//! Trial evaluation of proposed NixOS configuration changes.
//!
//! Strategy:
//! 1. Copy the user's workspace to a temp directory (only .nix files and flake.lock)
//! 2. Apply proposed changes to the copy using the AST writer
//! 3. Run `nix eval .#nixosConfigurations.<host>.config.system.build.toplevel`
//! 4. Capture stdout/stderr and exit code
//! 5. Return structured TrialResult

use std::path::Path;
use std::time::Instant;

use crate::intent::types::{IntentError, ProposedChange, TrialResult};
use crate::nix_parser::types::NixValue;

// Public API

/// Run a trial eval with proposed changes applied to a temporary config copy.
pub async fn run_trial(
    workspace: &Path,
    hostname: &str,
    changes: &[ProposedChange],
) -> Result<TrialResult, IntentError> {
    let start = Instant::now();

    // 1. Create temp copy of workspace
    let temp_dir = create_temp_copy(workspace)?;
    let temp_path = temp_dir.path().to_path_buf();

    // 2. Apply changes to the temp copy
    apply_changes_to_copy(&temp_path, changes)?;

    // 3. Run nix eval on the temp copy
    let result = run_nix_eval(&temp_path, hostname).await?;

    let elapsed = start.elapsed().as_millis() as u64;

    // temp_dir is dropped here, cleaning up

    Ok(TrialResult {
        success: result.0 == 0,
        stderr: result.2,
        stdout: result.1,
        eval_time_ms: elapsed,
        exit_code: result.0,
    })
}

// Workspace copy

/// Copy the workspace to a temporary directory.
/// Only copies .nix files and flake.lock to keep it fast.
pub(crate) fn create_temp_copy(workspace: &Path) -> Result<tempfile::TempDir, IntentError> {
    let temp = tempfile::tempdir()
        .map_err(|e| IntentError::TempCopyFailed(e.to_string()))?;

    copy_nix_files(workspace, temp.path())?;

    Ok(temp)
}

/// Recursively copy .nix files and flake.lock from src to dst.
fn copy_nix_files(src: &Path, dst: &Path) -> Result<(), IntentError> {
    use std::fs;

    for entry in fs::read_dir(src).map_err(IntentError::IoError)? {
        let entry = entry.map_err(IntentError::IoError)?;
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name_str = file_name.to_string_lossy();

        // Skip hidden files/dirs (.git, .gitignore, etc.)
        if file_name_str.starts_with('.') {
            continue;
        }
        // Skip common non-nix build artifacts
        if matches!(file_name_str.as_ref(), "node_modules" | "target" | "result") {
            continue;
        }

        let dst_path = dst.join(&file_name);

        if path.is_dir() {
            fs::create_dir_all(&dst_path).map_err(IntentError::IoError)?;
            copy_nix_files(&path, &dst_path)?;
        } else if file_name_str.ends_with(".nix") || file_name_str == "flake.lock" {
            fs::copy(&path, &dst_path).map_err(IntentError::IoError)?;
        }
    }

    Ok(())
}

// Change injection

/// Parse a Nix value string (e.g. "true", "42", `"\"hello\""`) into a NixValue.
///
/// Wraps the string in a dummy attribute set and extracts the value via the
/// AST parser — this reuses existing parser infrastructure and handles the
/// full range of Nix value syntax.
fn parse_nix_value_str(value_str: &str) -> Result<NixValue, IntentError> {
    use crate::nix_parser::{find_option, reader::parse_string};

    let src = format!("{{ __nixman_v = {}; }}", value_str);
    let nix_file = parse_string(&src).map_err(|e| {
        IntentError::AstError(format!("invalid value '{}': {}", value_str, e))
    })?;
    let node = find_option(&nix_file, "__nixman_v").ok_or_else(|| {
        IntentError::AstError(format!("could not extract value '{}'", value_str))
    })?;

    Ok(node.to_nix_value())
}

/// Apply proposed changes to the config files in the temp copy.
///
/// Builds the module graph rooted at `workspace/configuration.nix`, resolves
/// each option to its file, and uses the AST writer to inject or overwrite
/// the value in-place.  Changes to options that don't yet exist in any file
/// are inserted via [`crate::nix_parser::insert::add_option`].
pub(crate) fn apply_changes_to_copy(workspace: &Path, changes: &[ProposedChange]) -> Result<(), IntentError> {
    use crate::nix_parser::{
        find_option,
        insert::add_option,
        modules::build_graph,
        reader::parse_file,
        resolver::locate,
        writer::set_value,
    };

    if changes.is_empty() {
        return Ok(());
    }

    let entry = workspace.join("configuration.nix");
    let graph = build_graph(&entry).map_err(|e| {
        IntentError::InjectionFailed(format!("failed to build module graph: {}", e))
    })?;

    for change in changes {
        let new_value = parse_nix_value_str(&change.value)?;

        let resolved = locate(&graph, workspace, &change.path).map_err(|e| {
            IntentError::InjectionFailed(format!("failed to locate '{}': {}", change.path, e))
        })?;

        let file_path = &resolved.file;
        let source = std::fs::read_to_string(file_path)?;

        let new_source = if resolved.exists {
            // Option already set — find its value range and overwrite in-place.
            let nix_file = parse_file(file_path).map_err(|e| {
                IntentError::AstError(format!(
                    "failed to parse '{}': {}",
                    file_path.display(),
                    e
                ))
            })?;
            let node = find_option(&nix_file, &change.path).ok_or_else(|| {
                IntentError::AstError(format!(
                    "option '{}' unexpectedly missing after locate",
                    change.path
                ))
            })?;
            let range = node.text_range();
            set_value(&source, range, &new_value).map_err(|e| {
                IntentError::InjectionFailed(format!(
                    "failed to set '{}' = '{}': {}",
                    change.path, change.value, e
                ))
            })?
        } else {
            // Option not yet set — insert it.
            add_option(&source, &change.path, &new_value, "  ").map_err(|e| {
                IntentError::InjectionFailed(format!(
                    "failed to add '{}' = '{}': {}",
                    change.path, change.value, e
                ))
            })?
        };

        std::fs::write(file_path, new_source)?;
    }

    Ok(())
}

// nix eval runner

/// Run `nix eval` on the config and return `(exit_code, stdout, stderr)`.
///
/// Evaluates `system.build.toplevel` which forces full NixOS module evaluation
/// including all assertion checks — assertion failures surface as non-zero exit
/// codes with human-readable messages in stderr.
async fn run_nix_eval(
    workspace: &Path,
    hostname: &str,
) -> Result<(i32, String, String), IntentError> {
    use tokio::process::Command;

    let eval_attr = format!(
        ".#nixosConfigurations.{}.config.system.build.toplevel",
        hostname
    );

    let output = Command::new("nix")
        .args(["eval", &eval_attr, "--no-write-lock-file"])
        .current_dir(workspace)
        .output()
        .await
        .map_err(|e| IntentError::EvalCommandFailed(format!("failed to spawn nix: {}", e)))?;

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    Ok((exit_code, stdout, stderr))
}
