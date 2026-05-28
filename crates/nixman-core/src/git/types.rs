use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Current state of a git repository at a given path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitStatus {
    /// Short name of the currently checked-out branch (e.g. `"main"`).
    pub branch: String,
    /// `true` when there are uncommitted changes in the index or working tree.
    pub is_dirty: bool,
    /// Number of files with uncommitted changes (modified, added, deleted, etc.).
    pub changed_files: u32,
    /// Number of commits the local branch is ahead of its upstream.
    pub ahead: u32,
    /// Number of commits the local branch is behind its upstream.
    pub behind: u32,
}

/// A git branch together with its optional upstream tracking reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitBranch {
    /// Short branch name (e.g. `"main"`, `"feature/my-change"`).
    pub name: String,
    /// Full name of the configured upstream, if any (e.g. `"origin/main"`).
    pub upstream: Option<String>,
}

/// Request to stage a set of files and create a commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitRequest {
    /// Commit message.
    pub message: String,
    /// Paths of files to stage.  May be absolute (within the worktree) or
    /// relative to the repository root.
    pub files: Vec<PathBuf>,
}

/// Errors produced by git operations in this module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GitError {
    /// The path is not inside a git repository.
    NotARepo,
    /// HEAD is in detached state — not pointing to a named branch.
    HeadDetached,
    /// The repository has no commits yet (unborn branch).
    NoCommits,
    /// One or more files could not be staged.
    StagingFailed(String),
    /// The commit object could not be created.
    CommitFailed(String),
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotARepo => write!(f, "path is not inside a git repository"),
            Self::HeadDetached => write!(f, "HEAD is in detached state"),
            Self::NoCommits => write!(f, "repository has no commits yet"),
            Self::StagingFailed(msg) => write!(f, "staging failed: {msg}"),
            Self::CommitFailed(msg) => write!(f, "commit failed: {msg}"),
        }
    }
}

impl std::error::Error for GitError {}
