//! Parse assertion failure messages from `nix eval` stderr output.

use crate::intent::types::Conflict;

/// Parse stderr output from `nix eval` to extract assertion failures.
/// Returns an empty vec if no assertion failures are found (e.g., if the error
/// is a type error or syntax error rather than an assertion failure).
pub fn parse_assertions(stderr: &str) -> Vec<Conflict> {
    // Look for "Failed assertions:" in the output
    // Then collect all lines starting with "- " until a blank line or end
    // Each "- " line is one assertion failure message

    let mut conflicts = Vec::new();
    let mut in_assertions = false;

    for line in stderr.lines() {
        let trimmed = line.trim();

        if trimmed.contains("Failed assertions:") {
            in_assertions = true;
            continue;
        }

        if in_assertions {
            if let Some(msg) = trimmed.strip_prefix("- ") {
                conflicts.push(Conflict {
                    message: msg.to_string(),
                    related_options: extract_option_paths(msg),
                    resolved: false,
                    resolution: None,
                });
            } else if trimmed.is_empty() || (!trimmed.starts_with('-') && !trimmed.is_empty()) {
                // End of assertions block (unless it's a continuation line)
                // Actually, some assertions span multiple lines. Let's be lenient:
                // only stop at completely empty lines or lines that look like new error sections
                if trimmed.is_empty() || trimmed.starts_with("error:") {
                    in_assertions = false;
                }
            }
        }
    }

    conflicts
}

/// Try to extract NixOS option paths from an assertion message.
/// Uses heuristics: looks for dot-separated words that look like option paths
/// (e.g., "services.xserver.displayManager.gdm.enable").
fn extract_option_paths(message: &str) -> Vec<String> {
    let mut paths = Vec::new();

    // Regex-free heuristic: split on whitespace and non-alnum/dot chars,
    // find tokens that contain at least 2 dots and look like option paths
    for word in message.split(|c: char| c.is_whitespace() || c == '`' || c == '\'' || c == '"') {
        let word = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '.' && c != '-' && c != '_');
        if word.contains('.')
            && word.matches('.').count() >= 2
            && word.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '-' || c == '_')
            && !word.starts_with('.')
            && !word.ends_with('.')
        {
            paths.push(word.to_string());
        }
    }

    paths
}

/// Check if the stderr indicates an assertion failure (vs. other eval errors).
pub fn is_assertion_failure(stderr: &str) -> bool {
    stderr.contains("Failed assertions:")
}

/// Check if the stderr indicates a general eval error (type error, missing attr, etc.)
pub fn is_eval_error(stderr: &str) -> bool {
    stderr.contains("error:") && !is_assertion_failure(stderr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_assertions() {
        let stderr = r#"error: Failed assertions:
- GDM and SDDM cannot both be enabled.
- The firewall module requires networking to be enabled."#;

        let conflicts = parse_assertions(stderr);
        assert_eq!(conflicts.len(), 2);
        assert_eq!(conflicts[0].message, "GDM and SDDM cannot both be enabled.");
        assert_eq!(conflicts[1].message, "The firewall module requires networking to be enabled.");
    }

    #[test]
    fn test_parse_indented_assertions() {
        let stderr = r#"error:
       Failed assertions:
       - services.xserver.displayManager.gdm.enable and services.xserver.displayManager.sddm.enable are mutually exclusive."#;

        let conflicts = parse_assertions(stderr);
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts[0].message.contains("mutually exclusive"));
    }

    #[test]
    fn test_extract_option_paths() {
        let msg = "services.xserver.displayManager.gdm.enable and services.xserver.displayManager.sddm.enable are mutually exclusive";
        let paths = extract_option_paths(msg);
        assert!(paths.contains(&"services.xserver.displayManager.gdm.enable".to_string()));
        assert!(paths.contains(&"services.xserver.displayManager.sddm.enable".to_string()));
    }

    #[test]
    fn test_no_assertions_in_type_error() {
        let stderr = "error: attribute 'foo' missing at /nix/store/...";
        let conflicts = parse_assertions(stderr);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_is_assertion_failure() {
        assert!(is_assertion_failure("error: Failed assertions:\n- foo"));
        assert!(!is_assertion_failure("error: attribute missing"));
    }

    #[test]
    fn test_empty_stderr() {
        let conflicts = parse_assertions("");
        assert!(conflicts.is_empty());
    }
}
