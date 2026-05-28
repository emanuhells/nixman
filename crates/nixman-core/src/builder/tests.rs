//! Unit tests for build-phase detection.
//!
//! Tests that the `phases::detect` function correctly maps raw `nixos-rebuild`
//! output lines to `BuildPhase` variants (or `None` for unrecognised lines).

use crate::builder::{phases::detect, BuildPhase};

// Evaluating phase

/// The word "evaluating" anywhere in the line signals the Evaluating phase.
#[test]
fn test_phase_detection_evaluating() {
    assert!(matches!(
        detect("evaluating derivation"),
        Some(BuildPhase::Evaluating)
    ));
}

/// The "these derivations will be built" marker also signals Evaluating.
#[test]
fn test_phase_detection_evaluating_derivations_marker() {
    assert!(matches!(
        detect("these derivations will be built:"),
        Some(BuildPhase::Evaluating)
    ));
}

/// Detection is case-insensitive.
#[test]
fn test_phase_detection_evaluating_case_insensitive() {
    assert!(matches!(
        detect("Evaluating flake inputs..."),
        Some(BuildPhase::Evaluating)
    ));
}

// Fetching phase

/// Lines containing "copying" signal the Fetching phase.
#[test]
fn test_phase_detection_fetching_copying() {
    assert!(matches!(
        detect("copying path '/nix/store/abc123'"),
        Some(BuildPhase::Fetching)
    ));
}

/// Lines containing "fetching" also signal the Fetching phase.
#[test]
fn test_phase_detection_fetching_keyword() {
    assert!(matches!(
        detect("fetching /nix/store/xyz"),
        Some(BuildPhase::Fetching)
    ));
}

// Building phase

/// The "building '" pattern (with single-quote) signals the Building phase.
#[test]
fn test_phase_detection_building() {
    assert!(matches!(
        detect("building '/nix/store/abc-foo-1.0.drv'"),
        Some(BuildPhase::Building)
    ));
}

/// A realistic multi-component store path is detected correctly.
#[test]
fn test_phase_detection_building_long_path() {
    assert!(matches!(
        detect("building '/nix/store/zzzzzzzzzz-nixos-system-myhost-23.11.drv'"),
        Some(BuildPhase::Building)
    ));
}

// Activating phase

/// "activating the configuration" signals the Activating phase.
#[test]
fn test_phase_detection_activating() {
    assert!(matches!(
        detect("activating the configuration..."),
        Some(BuildPhase::Activating)
    ));
}

/// "setting up" (e.g. "setting up /etc...") also signals Activating.
#[test]
fn test_phase_detection_activating_setting_up() {
    assert!(matches!(
        detect("setting up /etc..."),
        Some(BuildPhase::Activating)
    ));
}

/// Case-insensitive match for activating.
#[test]
fn test_phase_detection_activating_case_insensitive() {
    assert!(matches!(
        detect("Activating The Configuration"),
        Some(BuildPhase::Activating)
    ));
}

// Non-phase lines → None

/// Ordinary log output that does not match any phase returns `None`.
#[test]
fn test_phase_detection_none() {
    assert!(detect("this is just some log output").is_none());
}

/// An empty string returns `None`.
#[test]
fn test_phase_detection_empty_line() {
    assert!(detect("").is_none());
}

/// A line with only whitespace returns `None`.
#[test]
fn test_phase_detection_whitespace_only() {
    assert!(detect("   ").is_none());
}

/// A line about a warning unrelated to any known phase returns `None`.
#[test]
fn test_phase_detection_warning_line() {
    assert!(detect("warning: Git tree '/etc/nixos' is dirty").is_none());
}
