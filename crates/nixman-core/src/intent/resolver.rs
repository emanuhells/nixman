//! Automatic conflict resolution for common NixOS assertion patterns.

use crate::intent::types::{Conflict, ProposedChange, Resolution};

/// Attempt to resolve conflicts automatically.
///
/// For each conflict, checks if it matches a known resolvable pattern.
/// Returns the conflicts with `resolved = true` and a `resolution` if successful.
pub fn resolve_conflicts(
    conflicts: &mut [Conflict],
    proposed_changes: &[ProposedChange],
) -> Vec<ProposedChange> {
    let mut additional_changes = Vec::new();
    let proposed_paths: Vec<&str> = proposed_changes.iter().map(|c| c.path.as_str()).collect();

    for conflict in conflicts.iter_mut() {
        if conflict.resolved {
            continue;
        }

        // Try resolution strategies in order
        if let Some(resolution) = try_mutual_exclusion_resolution(conflict, &proposed_paths) {
            additional_changes.push(resolution.change.clone());
            conflict.resolved = true;
            conflict.resolution = Some(resolution);
        }
    }

    additional_changes
}

/// Resolve "X and Y cannot both be enabled" patterns.
///
/// If the conflict mentions two `.enable` options, and only one was proposed
/// by the user, disable the other one.
fn try_mutual_exclusion_resolution(
    conflict: &Conflict,
    proposed_paths: &[&str],
) -> Option<Resolution> {
    // Find enable options in the conflict's related_options
    let enable_options: Vec<&String> = conflict.related_options.iter()
        .filter(|p| p.ends_with(".enable"))
        .collect();

    if enable_options.len() < 2 {
        return None;
    }

    // Find which ones are proposed by the user and which are not
    let user_proposed: Vec<&&String> = enable_options.iter()
        .filter(|p| proposed_paths.contains(&p.as_str()))
        .collect();
    let not_proposed: Vec<&&String> = enable_options.iter()
        .filter(|p| !proposed_paths.contains(&p.as_str()))
        .collect();

    // If exactly one is user-proposed and at least one is not, disable the non-proposed one(s)
    if !user_proposed.is_empty() && !not_proposed.is_empty() {
        // Disable the first non-proposed enable option
        let to_disable = not_proposed[0].to_string();
        let explanation = format!(
            "Disabled '{}' because it conflicts with your requested change. {}",
            to_disable, conflict.message
        );

        return Some(Resolution {
            change: ProposedChange {
                path: to_disable.clone(),
                value: "false".to_string(),
                reason: Some(format!("Auto-resolved conflict: {}", conflict.message)),
            },
            explanation,
        });
    }

    None
}

/// Check if a message indicates a mutual exclusion conflict.
/// Looks for keywords like "cannot both", "mutually exclusive", "only one", etc.
pub fn is_mutual_exclusion(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("cannot both")
        || lower.contains("mutually exclusive")
        || lower.contains("only one")
        || lower.contains("conflicts with")
        || lower.contains("incompatible")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_mutual_exclusion() {
        let mut conflicts = vec![Conflict {
            message: "services.xserver.displayManager.gdm.enable and services.xserver.displayManager.sddm.enable cannot both be enabled".to_string(),
            related_options: vec![
                "services.xserver.displayManager.gdm.enable".to_string(),
                "services.xserver.displayManager.sddm.enable".to_string(),
            ],
            resolved: false,
            resolution: None,
        }];

        let proposed = vec![ProposedChange {
            path: "services.xserver.displayManager.gdm.enable".to_string(),
            value: "true".to_string(),
            reason: None,
        }];

        let additional = resolve_conflicts(&mut conflicts, &proposed);

        assert_eq!(additional.len(), 1);
        assert_eq!(additional[0].path, "services.xserver.displayManager.sddm.enable");
        assert_eq!(additional[0].value, "false");
        assert!(conflicts[0].resolved);
    }

    #[test]
    fn test_no_resolution_when_both_proposed() {
        let mut conflicts = vec![Conflict {
            message: "X and Y cannot both be enabled".to_string(),
            related_options: vec![
                "services.x.enable".to_string(),
                "services.y.enable".to_string(),
            ],
            resolved: false,
            resolution: None,
        }];

        let proposed = vec![
            ProposedChange { path: "services.x.enable".to_string(), value: "true".to_string(), reason: None },
            ProposedChange { path: "services.y.enable".to_string(), value: "true".to_string(), reason: None },
        ];

        let additional = resolve_conflicts(&mut conflicts, &proposed);
        assert!(additional.is_empty());
        assert!(!conflicts[0].resolved);
    }

    #[test]
    fn test_no_resolution_without_enable_options() {
        let mut conflicts = vec![Conflict {
            message: "Something is wrong".to_string(),
            related_options: vec!["some.other.option".to_string()],
            resolved: false,
            resolution: None,
        }];

        let proposed = vec![];
        let additional = resolve_conflicts(&mut conflicts, &proposed);
        assert!(additional.is_empty());
    }

    #[test]
    fn test_is_mutual_exclusion() {
        assert!(is_mutual_exclusion("GDM and SDDM cannot both be enabled"));
        assert!(is_mutual_exclusion("These options are mutually exclusive"));
        assert!(is_mutual_exclusion("Only one display manager can be active"));
        assert!(!is_mutual_exclusion("The option requires networking"));
    }
}
