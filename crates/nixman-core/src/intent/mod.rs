//! Intent engine — trial evaluation for safe multi-option NixOS changes.
//!
//! The intent engine takes a set of proposed configuration changes, creates a
//! temporary copy of the user's config, applies the changes, and runs `nix eval`
//! to detect conflicts (assertion failures) and implications (mkIf propagation).
//!
//! # Usage
//!
//! ```ignore
//! use nixman_core::intent::{propose, types::ProposedChange};
//!
//! let changes = vec![
//!     ProposedChange {
//!         path: "programs.hyprland.enable".into(),
//!         value: "true".into(),
//!         reason: Some("User wants Hyprland".into()),
//!     },
//! ];
//! let plan = propose(workspace_path, "myhostname", changes).await?;
//! ```

pub mod assertions;
pub mod implications;
pub mod resolver;
pub mod trial;
pub mod types;

use std::path::Path;
use types::{ChangePlan, IntentError, ProposedChange};

/// Propose changes and run a trial eval to validate them.
///
/// Runs an initial trial, attempts to auto-resolve conflicts, then re-runs
/// if any resolutions were applied. Returns a `ChangePlan` with the final
/// set of changes, conflicts, and implications.
pub async fn propose(
    workspace: &Path,
    hostname: &str,
    changes: Vec<ProposedChange>,
) -> Result<ChangePlan, IntentError> {
    let trial_result = trial::run_trial(workspace, hostname, &changes).await?;

    let mut conflicts = if !trial_result.success {
        assertions::parse_assertions(&trial_result.stderr)
    } else {
        Vec::new()
    };

    let resolution_changes = resolver::resolve_conflicts(&mut conflicts, &changes);

    // If we resolved conflicts, re-run trial with additional changes
    let (final_valid, final_eval_time, final_conflicts) = if !resolution_changes.is_empty() {
        let mut all_changes = changes.clone();
        all_changes.extend(resolution_changes.clone());
        let retry = trial::run_trial(workspace, hostname, &all_changes).await?;
        let retry_conflicts = if !retry.success {
            assertions::parse_assertions(&retry.stderr)
        } else {
            Vec::new()
        };
        (retry.success, trial_result.eval_time_ms + retry.eval_time_ms, retry_conflicts)
    } else {
        (trial_result.success, trial_result.eval_time_ms, conflicts)
    };

    let mut all_changes = changes.clone();
    all_changes.extend(resolution_changes);

    let detected_implications = if final_valid {
        let related = implications::suggest_related_paths(&all_changes);
        implications::detect_implications(workspace, hostname, &all_changes, &related)
            .await
            .unwrap_or_default() // Don't fail the whole proposal if implication detection fails
    } else {
        Vec::new()
    };

    Ok(ChangePlan {
        changes: all_changes,
        conflicts: final_conflicts,
        implications: detected_implications,
        warnings: Vec::new(),
        valid: final_valid,
        eval_time_ms: final_eval_time,
    })
}
