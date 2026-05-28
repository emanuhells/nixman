use std::io::Read;

pub async fn run(
    error_text: Option<String>,
    use_stdin: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    let text = if use_stdin {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else if let Some(t) = error_text {
        t
    } else {
        return Err("Provide error text as argument or use --stdin".into());
    };

    let explanation = explain_error(&text);
    Ok(serde_json::to_string_pretty(&explanation)?)
}

#[derive(Debug, serde::Serialize)]
pub struct Explanation {
    pub understood: bool,
    pub error_type: String,
    pub summary: String,
    pub fix: Option<String>,
    pub details: Option<String>,
}

pub fn explain_error(text: &str) -> Explanation {
    let text_lower = text.to_lowercase();

    if text_lower.contains("does not exist") || (text_lower.contains("the option") && text_lower.contains("not exist")) {
        let option_name = extract_quoted(text).unwrap_or_default();
        return Explanation {
            understood: true,
            error_type: "option_removed_or_renamed".into(),
            summary: format!("The option '{}' does not exist in this NixOS version.", option_name),
            fix: Some(format!("Run: nixman option search {}", option_name.split('.').last().unwrap_or(&option_name))),
            details: Some("This option may have been renamed or removed in a NixOS update. Use 'nixman option search' or 'nixman migrate' to find the replacement.".into()),
        };
    }

    if text_lower.contains("failed assertion") {
        return Explanation {
            understood: true,
            error_type: "assertion_failure".into(),
            summary: "A NixOS module assertion failed. Two conflicting options are likely enabled.".into(),
            fix: Some("Run: nixman check (to see all assertion failures with context)".into()),
            details: Some(extract_assertion_detail(text)),
        };
    }

    if text_lower.contains("undefined variable") {
        let var_name = extract_quoted(text).unwrap_or_default();
        return Explanation {
            understood: true,
            error_type: "undefined_variable".into(),
            summary: format!("Variable '{}' is not defined.", var_name),
            fix: Some(format!("Check spelling. For packages, try: nixman packages search {}", var_name)),
            details: Some("In Nix, hyphens are valid in identifiers. 'undefined variable' means the attribute doesn't exist in the current scope, not a parse error.".into()),
        };
    }

    if text_lower.contains("infinite recursion") {
        return Explanation {
            understood: true,
            error_type: "infinite_recursion".into(),
            summary: "Configuration has a circular dependency.".into(),
            fix: Some("Check for options that reference each other. Common cause: an overlay that depends on the package it's overlaying.".into()),
            details: None,
        };
    }

    if text_lower.contains("collision between") {
        return Explanation {
            understood: true,
            error_type: "package_collision".into(),
            summary: "Two packages provide the same file.".into(),
            fix: Some("Remove one of the conflicting packages, or use `lib.hiPrio` to set priority.".into()),
            details: Some(text.to_string()),
        };
    }

    if text_lower.contains("syntax error") || text_lower.contains("unexpected") {
        return Explanation {
            understood: true,
            error_type: "syntax_error".into(),
            summary: "Nix syntax error in configuration.".into(),
            fix: Some("Check for: missing semicolons, unbalanced braces, or unclosed strings at the location shown.".into()),
            details: Some(text.to_string()),
        };
    }

    if text_lower.contains("hash mismatch") {
        return Explanation {
            understood: true,
            error_type: "hash_mismatch".into(),
            summary: "A fixed-output derivation's hash doesn't match. Usually means a dependency was updated.".into(),
            fix: Some("Update the hash in your derivation, or run 'nix flake update' if it's an input.".into()),
            details: None,
        };
    }

    if (text_lower.contains("attribute") && text_lower.contains("missing")) || text_lower.contains("has no attribute") {
        let attr = extract_quoted(text).unwrap_or_default();
        return Explanation {
            understood: true,
            error_type: "missing_attribute".into(),
            summary: format!("Attribute '{}' not found.", attr),
            fix: Some(format!("The package or option may not exist. Try: nixman packages search {}", attr)),
            details: None,
        };
    }

    if text_lower.contains("permission denied") {
        return Explanation {
            understood: true,
            error_type: "permission_denied".into(),
            summary: "Operation requires elevated privileges.".into(),
            fix: Some("Run with sudo, or configure polkit rules for your user.".into()),
            details: None,
        };
    }

    if text_lower.contains("no space left") || text_lower.contains("disk full") {
        return Explanation {
            understood: true,
            error_type: "disk_full".into(),
            summary: "Not enough disk space to complete the operation.".into(),
            fix: Some("Run: nixman generations gc --keep 3 (to remove old generations and free space)".into()),
            details: None,
        };
    }

    Explanation {
        understood: false,
        error_type: "unknown".into(),
        summary: "Could not explain this error automatically.".into(),
        fix: None,
        details: Some(text.to_string()),
    }
}

/// Extract first quoted string from error text.
fn extract_quoted(text: &str) -> Option<String> {
    if let Some(start) = text.find('\'') {
        if let Some(end) = text[start+1..].find('\'') {
            return Some(text[start+1..start+1+end].to_string());
        }
    }
    if let Some(start) = text.find('`') {
        if let Some(end) = text[start+1..].find('`') {
            return Some(text[start+1..start+1+end].to_string());
        }
    }
    None
}

/// Extract assertion detail from error message.
fn extract_assertion_detail(text: &str) -> String {
    text.lines()
        .filter(|l| {
            let lower = l.to_lowercase();
            lower.contains("assert") || lower.contains("conflict") ||
            lower.contains("enable") || lower.contains("cannot")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
