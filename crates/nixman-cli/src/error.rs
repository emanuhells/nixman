use std::fmt;

/// CLI-layer error type with actionable messages.
#[derive(Debug)]
pub enum CliError {
    /// A core library error.
    Core(String),
    /// I/O error.
    Io(std::io::Error),
    /// Bad arguments or missing values.
    Usage(String),
    /// Option or package not found (includes suggestions).
    NotFound { message: String, suggestions: Vec<String> },
    /// Permission denied (needs root/polkit).
    PermissionDenied(String),
    /// A Nix command failed.
    NixError(String),
    /// Operation completed successfully but no change was needed.
    Noop(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Core(msg) => write!(f, "{}", msg),
            Self::Io(e) => write!(f, "I/O error: {}", e),
            Self::Usage(msg) => write!(f, "{}", msg),
            Self::NotFound { message, suggestions } => {
                write!(f, "{}", message)?;
                if !suggestions.is_empty() {
                    write!(f, "\n\nDid you mean:")?;
                    for s in suggestions {
                        write!(f, "\n  {}", s)?;
                    }
                }
                Ok(())
            }
            Self::PermissionDenied(msg) => write!(f, "Permission denied: {}\n\nTry running with sudo or configure polkit.", msg),
            Self::NixError(msg) => write!(f, "Nix error: {}", msg),
            Self::Noop(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for CliError {}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self { Self::Io(e) }
}

impl From<String> for CliError {
    fn from(s: String) -> Self { Self::Core(s) }
}

impl From<Box<dyn std::error::Error>> for CliError {
    fn from(e: Box<dyn std::error::Error>) -> Self { Self::Core(e.to_string()) }
}

impl CliError {
    /// Exit code for this error type.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) => 2,
            Self::Noop(_) => 3,
            _ => 1,
        }
    }

    /// JSON representation for machine-readable error output.
    pub fn to_json(&self) -> String {
        let kind = match self {
            Self::Core(_) => "core",
            Self::Io(_) => "io",
            Self::Usage(_) => "usage",
            Self::NotFound { .. } => "not_found",
            Self::PermissionDenied(_) => "permission_denied",
            Self::NixError(_) => "nix_error",
            Self::Noop(_) => "noop",
        };
        let suggestion = match self {
            Self::NotFound { suggestions, .. } => suggestions.first().cloned(),
            Self::PermissionDenied(_) => Some("Try running with sudo".to_string()),
            _ => None,
        };
        serde_json::json!({
            "error": {
                "kind": kind,
                "message": self.to_string(),
                "suggestion": suggestion,
            }
        }).to_string()
    }

    /// Whether this is a "no change needed" result.
    pub fn is_noop(&self) -> bool {
        matches!(self, Self::Noop(_))
    }

    /// Human-readable message for exit, without error prefix.
    pub fn exit_message(&self) -> String {
        match self {
            Self::Noop(msg) => msg.clone(),
            other => other.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_variants() -> Vec<(&'static str, CliError, i32)> {
        vec![
            ("core", CliError::Core("core error".into()), 1),
            (
                "io",
                CliError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "file not found")),
                1,
            ),
            ("usage", CliError::Usage("bad argument".into()), 2),
            (
                "not_found",
                CliError::NotFound {
                    message: "option not found".into(),
                    suggestions: vec!["services.nginx.enable".into()],
                },
                1,
            ),
            (
                "not_found_empty_suggestions",
                CliError::NotFound {
                    message: "not found".into(),
                    suggestions: vec![],
                },
                1,
            ),
            (
                "permission_denied",
                CliError::PermissionDenied("access denied".into()),
                1,
            ),
            ("nix_error", CliError::NixError("build failed".into()), 1),
            ("noop", CliError::Noop("nothing to change".into()), 3),
        ]
    }

    #[test]
    fn exit_code_values() {
        for (_name, err, expected) in make_variants() {
            assert_eq!(
                err.exit_code(),
                expected,
                "exit_code() for {:?} should be {}",
                _name,
                expected
            );
        }
    }

    #[test]
    fn is_noop_only_for_noop() {
        for (name, err, _code) in make_variants() {
            let expected = name == "noop";
            assert_eq!(
                err.is_noop(),
                expected,
                "is_noop() for {:?} should be {}",
                name,
                expected
            );
        }
    }

    #[test]
    fn exit_message_returns_something() {
        for (_name, err, _code) in make_variants() {
            let msg = err.exit_message();
            assert!(!msg.is_empty(), "exit_message() for {:?} should not be empty", _name);
        }
    }

    #[test]
    fn display_does_not_panic() {
        for (_name, err, _code) in make_variants() {
            let display = format!("{}", err);
            assert!(!display.is_empty(), "Display for {:?} should not be empty", _name);
        }
    }

    #[test]
    fn to_json_contains_kind_field() {
        for (name, err, _code) in make_variants() {
            let json_str = err.to_json();
            let parsed: serde_json::Value = serde_json::from_str(&json_str)
                .unwrap_or_else(|e| panic!("to_json() for {:?} must be valid JSON: {}", name, e));
            let kind = parsed["error"]["kind"].as_str().unwrap_or_else(|| {
                panic!("to_json() for {:?} must have error.kind field", name)
            });
            assert!(!kind.is_empty(), "kind should not be empty for {:?}", name);
            assert!(parsed["error"]["message"].is_string(), "message should be a string for {:?}", name);
        }
    }

    #[test]
    fn to_json_not_found_has_suggestion() {
        let err = CliError::NotFound {
            message: "not found".into(),
            suggestions: vec!["try this".into()],
        };
        let parsed: serde_json::Value = serde_json::from_str(&err.to_json()).unwrap();
        assert_eq!(parsed["error"]["kind"], "not_found");
        assert_eq!(parsed["error"]["suggestion"], "try this");
    }

    #[test]
    fn to_json_permission_denied_has_suggestion() {
        let err = CliError::PermissionDenied("no".into());
        let parsed: serde_json::Value = serde_json::from_str(&err.to_json()).unwrap();
        assert_eq!(parsed["error"]["kind"], "permission_denied");
        assert_eq!(parsed["error"]["suggestion"], "Try running with sudo");
    }

    #[test]
    fn to_json_noop_no_suggestion() {
        let err = CliError::Noop("ok".into());
        let parsed: serde_json::Value = serde_json::from_str(&err.to_json()).unwrap();
        assert_eq!(parsed["error"]["kind"], "noop");
        assert!(parsed["error"]["suggestion"].is_null());
    }

    #[test]
    fn display_not_found_with_suggestions_includes_did_you_mean() {
        let err = CliError::NotFound {
            message: "not found".into(),
            suggestions: vec!["suggestion-a".into(), "suggestion-b".into()],
        };
        let display = err.to_string();
        assert!(display.contains("not found"), "should contain message");
        assert!(display.contains("Did you mean"), "should include Did you mean");
        assert!(display.contains("suggestion-a"), "should include first suggestion");
        assert!(display.contains("suggestion-b"), "should include second suggestion");
    }

    #[test]
    fn display_not_found_empty_suggestions_no_did_you_mean() {
        let err = CliError::NotFound {
            message: "just not found".into(),
            suggestions: vec![],
        };
        let display = err.to_string();
        assert!(display.contains("just not found"));
        assert!(!display.contains("Did you mean"), "no suggestions should not show Did you mean");
    }

    #[test]
    fn display_permission_denied_includes_tip() {
        let err = CliError::PermissionDenied("no".into());
        let display = err.to_string();
        assert!(display.contains("Permission denied"));
        assert!(display.contains("sudo"));
    }

    #[test]
    fn display_io_shows_error() {
        let err = CliError::Io(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"));
        let display = err.to_string();
        assert!(display.contains("I/O error"));
        assert!(display.contains("denied"));
    }

    #[test]
    fn display_nix_error_shows_message() {
        let err = CliError::NixError("derivation failed".into());
        let display = err.to_string();
        assert!(display.contains("Nix error"));
        assert!(display.contains("derivation failed"));
    }

    #[test]
    fn from_string_creates_core() {
        let err: CliError = String::from("something went wrong").into();
        assert!(matches!(err, CliError::Core(_)));
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn from_io_error_creates_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let err: CliError = io_err.into();
        assert!(matches!(err, CliError::Io(_)));
    }

    #[test]
    fn from_boxed_error_creates_core() {
        let boxed: Box<dyn std::error::Error> = "generic failure".into();
        let err: CliError = boxed.into();
        assert!(matches!(err, CliError::Core(_)));
    }

    #[test]
    fn debug_output_does_not_panic() {
        for (_name, err, _code) in make_variants() {
            let debug = format!("{:?}", err);
            assert!(!debug.is_empty(), "Debug for {:?} should not be empty", _name);
        }
    }

    #[test]
    fn error_trait_is_implemented() {
        fn assert_error<T: std::error::Error>() {}
        assert_error::<CliError>();
    }
}
