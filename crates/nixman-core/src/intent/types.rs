use serde::{Deserialize, Serialize};

/// A change proposed by the user or agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposedChange {
    /// NixOS option path (e.g., "programs.hyprland.enable")
    pub path: String,
    /// Value to set (serialized as string for now; "true", "false", "\"hello\"", etc.)
    pub value: String,
    /// Optional human-readable reason
    pub reason: Option<String>,
}

/// Result of a trial evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrialResult {
    /// Did the eval succeed (no assertion failures, no errors)?
    pub success: bool,
    /// Raw stderr output from nix eval (contains assertion messages on failure)
    pub stderr: String,
    /// Raw stdout output (if any)
    pub stdout: String,
    /// How long the eval took in milliseconds
    pub eval_time_ms: u64,
    /// Exit code of the nix process
    pub exit_code: i32,
}

/// A conflict detected from assertion failures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    /// The assertion message from NixOS
    pub message: String,
    /// Options that are likely involved (parsed from message if possible)
    pub related_options: Vec<String>,
    /// Was this conflict auto-resolved?
    pub resolved: bool,
    /// How it was resolved (if resolved)
    pub resolution: Option<Resolution>,
}

/// How a conflict was resolved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resolution {
    /// The change made to resolve the conflict
    pub change: ProposedChange,
    /// Explanation
    pub explanation: String,
}

/// An option that was auto-set by mkIf propagation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Implication {
    /// Option path that was auto-set
    pub path: String,
    /// Value it was set to
    pub value: String,
    /// Why (if detectable)
    pub reason: Option<String>,
}

/// The complete change plan returned by `propose()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangePlan {
    /// The final set of changes to apply (includes user's + conflict resolutions)
    pub changes: Vec<ProposedChange>,
    /// Conflicts detected during trial eval
    pub conflicts: Vec<Conflict>,
    /// Options that were auto-set by the module system
    pub implications: Vec<Implication>,
    /// Non-blocking warnings
    pub warnings: Vec<String>,
    /// Is the plan valid (eval passes after all resolutions)?
    pub valid: bool,
    /// Total eval time in milliseconds
    pub eval_time_ms: u64,
}

/// Errors from the intent engine.
#[derive(Debug)]
pub enum IntentError {
    /// Workspace not found or invalid
    WorkspaceError(String),
    /// Failed to create temporary config copy
    TempCopyFailed(String),
    /// Failed to inject changes into config
    InjectionFailed(String),
    /// nix eval command failed to execute (not assertion failure — actual crash)
    EvalCommandFailed(String),
    /// AST parsing/writing error
    AstError(String),
    /// IO error
    IoError(std::io::Error),
}

impl std::fmt::Display for IntentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IntentError::WorkspaceError(m) => write!(f, "workspace error: {m}"),
            IntentError::TempCopyFailed(m) => write!(f, "temp copy failed: {m}"),
            IntentError::InjectionFailed(m) => write!(f, "injection failed: {m}"),
            IntentError::EvalCommandFailed(m) => write!(f, "eval command failed: {m}"),
            IntentError::AstError(m) => write!(f, "AST error: {m}"),
            IntentError::IoError(e) => write!(f, "IO error: {e}"),
        }
    }
}

impl std::error::Error for IntentError {}

impl From<std::io::Error> for IntentError {
    fn from(e: std::io::Error) -> Self {
        IntentError::IoError(e)
    }
}
