//! Build-phase detection from `nixos-rebuild` output lines.
//!
//! Each line of output is matched against a set of known patterns.  The
//! patterns are tested case-insensitively so that minor formatting changes in
//! Nix or NixOS do not silently break detection.

use crate::builder::types::BuildPhase;

/// Inspect a single output line and return the [`BuildPhase`] it signals, or
/// `None` if the line does not match any known phase marker.
///
/// # Phase markers
///
/// | Phase        | Patterns (case-insensitive)                          |
/// |--------------|------------------------------------------------------|
/// | Evaluating   | `"evaluating"`, `"these derivations will be built"`  |
/// | Fetching     | `"copying"`, `"fetching"`                            |
/// | Building     | `"building '"`                                       |
/// | Activating   | `"activating the configuration"`, `"setting up"`     |
pub fn detect(line: &str) -> Option<BuildPhase> {
    let lower = line.to_lowercase();

    // Evaluating phase — Nix is parsing / evaluating the flake.
    if lower.contains("evaluating") || lower.contains("these derivations will be built") {
        return Some(BuildPhase::Evaluating);
    }

    // Fetching phase — Nix is downloading pre-built store paths.
    if lower.contains("copying") || lower.contains("fetching") {
        return Some(BuildPhase::Fetching);
    }

    // Building phase — Nix is compiling a derivation locally.
    // The single-quote is part of the nix output: `building '/nix/store/...'`.
    if lower.contains("building '") {
        return Some(BuildPhase::Building);
    }

    // Activating phase — NixOS is switching to the new configuration.
    if lower.contains("activating the configuration") || lower.contains("setting up") {
        return Some(BuildPhase::Activating);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_evaluating() {
        assert!(matches!(detect("evaluating derivation"), Some(BuildPhase::Evaluating)));
        assert!(matches!(
            detect("these derivations will be built:"),
            Some(BuildPhase::Evaluating)
        ));
    }

    #[test]
    fn detects_fetching() {
        assert!(matches!(detect("copying path '/nix/store/abc'"), Some(BuildPhase::Fetching)));
        assert!(matches!(detect("fetching /nix/store/xyz"), Some(BuildPhase::Fetching)));
    }

    #[test]
    fn detects_building() {
        assert!(matches!(
            detect("building '/nix/store/abc-foo-1.0.drv'"),
            Some(BuildPhase::Building)
        ));
    }

    #[test]
    fn detects_activating() {
        assert!(matches!(
            detect("activating the configuration..."),
            Some(BuildPhase::Activating)
        ));
        assert!(matches!(detect("setting up /etc..."), Some(BuildPhase::Activating)));
    }

    #[test]
    fn returns_none_for_unrecognised_lines() {
        assert!(detect("this is just some log output").is_none());
        assert!(detect("").is_none());
    }
}
