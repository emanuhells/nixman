//! Detect options auto-set by mkIf propagation.
//!
//! Strategy: eval specific option paths before and after applying changes,
//! then diff to find values that changed without being explicitly requested.

use std::collections::HashMap;
use std::path::Path;

use crate::intent::types::{Implication, IntentError, ProposedChange};

/// Detect implications by comparing option values before and after changes.
///
/// `related_paths` are additional option paths to check beyond the proposed changes.
/// These should be options likely affected by the proposed changes (e.g., if enabling
/// hyprland, check xwayland, polkit, wayland-related options).
pub async fn detect_implications(
    workspace: &Path,
    hostname: &str,
    proposed_changes: &[ProposedChange],
    related_paths: &[String],
) -> Result<Vec<Implication>, IntentError> {
    // Collect paths to check — related paths that aren't being explicitly set
    let explicit_paths: Vec<&str> = proposed_changes.iter().map(|c| c.path.as_str()).collect();
    let check_paths: Vec<&str> = related_paths
        .iter()
        .map(|s| s.as_str())
        .filter(|p| !explicit_paths.contains(p))
        .collect();

    if check_paths.is_empty() {
        return Ok(Vec::new());
    }

    // Get base values (current config without changes)
    let base_values = eval_option_values(workspace, hostname, &check_paths).await?;

    // Get trial values (config with changes applied in a temp copy)
    let trial_workspace =
        crate::intent::trial::create_temp_copy(workspace)?;
    crate::intent::trial::apply_changes_to_copy(trial_workspace.path(), proposed_changes)?;
    let trial_values =
        eval_option_values(trial_workspace.path(), hostname, &check_paths).await?;

    // Diff: any path whose value changed is an implication
    let mut implications = Vec::new();
    for path in &check_paths {
        let base_val = base_values.get(*path).map(|s| s.as_str()).unwrap_or("null");
        let trial_val = trial_values.get(*path).map(|s| s.as_str()).unwrap_or("null");

        if base_val != trial_val {
            implications.push(Implication {
                path: path.to_string(),
                value: trial_val.to_string(),
                reason: None, // Can't easily determine which module set it
            });
        }
    }

    Ok(implications)
}

/// Evaluate specific option paths and return their values as strings.
///
/// Tries a single batched nix expression first; falls back to per-path evals
/// if the batch fails (e.g., some options aren't JSON-serializable).
async fn eval_option_values(
    workspace: &Path,
    hostname: &str,
    paths: &[&str],
) -> Result<HashMap<String, String>, IntentError> {
    use tokio::process::Command;

    let mut results = HashMap::new();

    let nix_expr = build_batch_eval_expr(hostname, paths);

    let output = Command::new("nix")
        .args([
            "eval",
            "--json",
            "--impure",
            "--expr",
            &nix_expr,
            "--no-write-lock-file",
        ])
        .current_dir(workspace)
        .output()
        .await
        .map_err(|e| IntentError::EvalCommandFailed(format!("failed to spawn nix: {e}")))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Ok(map) = serde_json::from_str::<HashMap<String, serde_json::Value>>(&stdout) {
            for (key, val) in map {
                results.insert(key, val.to_string());
            }
        }
    } else {
        // Batch eval failed — try each path individually so partial results
        // are still returned (some options can't be serialized to JSON).
        for path in paths {
            let attr = format!(
                ".#nixosConfigurations.{}.config.{}",
                hostname, path
            );
            let out = Command::new("nix")
                .args(["eval", &attr, "--json", "--no-write-lock-file"])
                .current_dir(workspace)
                .output()
                .await
                .map_err(|e| IntentError::EvalCommandFailed(e.to_string()))?;

            if out.status.success() {
                let val = String::from_utf8_lossy(&out.stdout).trim().to_string();
                results.insert(path.to_string(), val);
            }
            // Skip paths that can't be evaluated (submodules, functions, etc.)
        }
    }

    Ok(results)
}

/// Build a Nix expression that evaluates multiple option paths at once.
///
/// Returns an attrset where each key is the option path string and each value
/// is the evaluated option value (or null if evaluation fails).
fn build_batch_eval_expr(hostname: &str, paths: &[&str]) -> String {
    let attrs: Vec<String> = paths
        .iter()
        .map(|p| {
            format!(
                "    \"{}\" = let r = builtins.tryEval config.{}; in if r.success then r.value else null;",
                p, p
            )
        })
        .collect();

    format!(
        "let config = (builtins.getFlake (toString ./.)).nixosConfigurations.{}.config; in {{\n{}\n}}",
        hostname,
        attrs.join("\n")
    )
}

/// Given a set of proposed changes, suggest related option paths that might be
/// affected by mkIf propagation. Uses heuristics based on the option prefix.
pub fn suggest_related_paths(changes: &[ProposedChange]) -> Vec<String> {
    let mut related = Vec::new();

    for change in changes {
        let parts: Vec<&str> = change.path.split('.').collect();

        // Heuristic: check sibling options under the same parent prefix
        if parts.len() >= 2 {
            let prefix = parts[..parts.len() - 1].join(".");
            related.push(format!("{}.wayland", prefix));
            related.push(format!("{}.autoSuspend", prefix));
        }

        // Common implication patterns keyed on the option path
        match change.path.as_str() {
            p if p.contains("hyprland") => {
                related.extend([
                    "programs.xwayland.enable".to_string(),
                    "security.polkit.enable".to_string(),
                    "hardware.opengl.enable".to_string(),
                    "services.xserver.enable".to_string(),
                ]);
            }
            p if p.contains("displayManager.gdm") => {
                related.extend([
                    "services.xserver.displayManager.gdm.wayland".to_string(),
                    "services.xserver.enable".to_string(),
                ]);
            }
            p if p.contains("displayManager.sddm") => {
                related.extend(["services.xserver.enable".to_string()]);
            }
            p if p.contains("docker") => {
                related.extend([
                    "virtualisation.docker.enable".to_string(),
                    "virtualisation.oci-containers.backend".to_string(),
                ]);
            }
            _ => {}
        }
    }

    related.sort();
    related.dedup();
    related
}

// Tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_suggest_related_paths_hyprland() {
        let changes = vec![ProposedChange {
            path: "programs.hyprland.enable".to_string(),
            value: "true".to_string(),
            reason: None,
        }];
        let related = suggest_related_paths(&changes);
        assert!(related.contains(&"programs.xwayland.enable".to_string()));
        assert!(related.contains(&"security.polkit.enable".to_string()));
    }

    #[test]
    fn test_suggest_related_paths_gdm() {
        let changes = vec![ProposedChange {
            path: "services.xserver.displayManager.gdm.enable".to_string(),
            value: "true".to_string(),
            reason: None,
        }];
        let related = suggest_related_paths(&changes);
        assert!(related.contains(
            &"services.xserver.displayManager.gdm.wayland".to_string()
        ));
    }

    #[test]
    fn test_build_batch_eval_expr() {
        let expr = build_batch_eval_expr(
            "myhost",
            &["services.xserver.enable", "programs.hyprland.enable"],
        );
        assert!(expr.contains("myhost"));
        assert!(expr.contains("services.xserver.enable"));
        assert!(expr.contains("builtins.tryEval"));
    }

    #[test]
    fn test_suggest_related_paths_deduplicates() {
        // Two hyprland changes shouldn't produce duplicate related paths
        let changes = vec![
            ProposedChange {
                path: "programs.hyprland.enable".to_string(),
                value: "true".to_string(),
                reason: None,
            },
            ProposedChange {
                path: "programs.hyprland.xwayland.enable".to_string(),
                value: "true".to_string(),
                reason: None,
            },
        ];
        let related = suggest_related_paths(&changes);
        let polkit_count = related
            .iter()
            .filter(|p| p.as_str() == "security.polkit.enable")
            .count();
        assert_eq!(polkit_count, 1);
    }

    #[test]
    fn test_suggest_related_paths_excludes_explicit() {
        // suggest_related_paths doesn't filter explicit changes — that's done
        // in detect_implications. Verify the function itself just returns paths.
        let changes = vec![ProposedChange {
            path: "programs.hyprland.enable".to_string(),
            value: "true".to_string(),
            reason: None,
        }];
        let related = suggest_related_paths(&changes);
        assert!(!related.is_empty());
    }
}
