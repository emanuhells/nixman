use std::path::Path;

use git2::ErrorCode;

use crate::git::types::{GitError, GitStatus};

// ── Public API ────────────────────────────────────────────────────────────────

/// Return `true` if `path` (or any ancestor directory) is inside a git
/// repository, `false` otherwise.
///
/// Uses `git2::Repository::discover` which walks up the directory tree looking
/// for a `.git` entry, matching the same search strategy as `git` itself.
pub fn is_git_repo(path: &Path) -> bool {
    git2::Repository::discover(path).is_ok()
}

/// Open the git repository at (or above) `path` and return a snapshot of its
/// current state.
///
/// Returns `Ok(None)` when `path` is not inside a git repository so callers
/// can distinguish "no repo" from an actual operational error.
///
/// # Errors
///
/// * [`GitError::NoCommits`] — the repo was initialised but has no commits.
/// * [`GitError::HeadDetached`] — HEAD is not pointing to a named branch.
pub fn status(path: &Path) -> Result<Option<GitStatus>, GitError> {
    // Discover the repo; translate "not found" into the None sentinel.
    let repo = match git2::Repository::discover(path) {
        Ok(r) => r,
        Err(e) if e.code() == ErrorCode::NotFound => return Ok(None),
        // Any other discovery error (e.g. I/O) is also treated as "no repo"
        // so non-git workspaces are always handled gracefully.
        Err(_) => return Ok(None),
    };

    // ── Resolve HEAD to a branch name ────────────────────────────────────────
    let head = match repo.head() {
        Err(e) if e.code() == ErrorCode::UnbornBranch => return Err(GitError::NoCommits),
        Err(_) => return Err(GitError::HeadDetached),
        Ok(h) => h,
    };

    if !head.is_branch() {
        return Err(GitError::HeadDetached);
    }

    let branch = head.shorthand().unwrap_or("HEAD").to_string();

    // ── Count changed files ───────────────────────────────────────────────────
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(false)
        .include_ignored(false)
        .include_unmodified(false);

    let statuses = repo
        .statuses(Some(&mut opts))
        .map_err(|e| GitError::CommitFailed(e.message().to_string()))?;

    let changed_files = statuses.len() as u32;
    let is_dirty = changed_files > 0;

    // ── Ahead / behind upstream ───────────────────────────────────────────────
    let (ahead, behind) = ahead_behind(&repo, &branch);

    Ok(Some(GitStatus {
        branch,
        is_dirty,
        changed_files,
        ahead,
        behind,
    }))
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Return `(ahead, behind)` commit counts for `branch_name` relative to its
/// configured upstream tracking branch.
///
/// Returns `(0, 0)` when no upstream is configured or any lookup fails — the
/// caller treats this as "no tracking information available".
fn ahead_behind(repo: &git2::Repository, branch_name: &str) -> (u32, u32) {
    let branch = match repo.find_branch(branch_name, git2::BranchType::Local) {
        Ok(b) => b,
        Err(_) => return (0, 0),
    };

    let upstream = match branch.upstream() {
        Ok(u) => u,
        Err(_) => return (0, 0), // No upstream configured — not an error.
    };

    let local_oid = match branch.get().target() {
        Some(oid) => oid,
        None => return (0, 0),
    };

    let upstream_oid = match upstream.get().target() {
        Some(oid) => oid,
        None => return (0, 0),
    };

    match repo.graph_ahead_behind(local_oid, upstream_oid) {
        Ok((a, b)) => (a as u32, b as u32),
        Err(_) => (0, 0),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn init_repo() -> TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        git2::Repository::init(dir.path()).expect("git init");
        dir
    }

    // ── is_git_repo ───────────────────────────────────────────────────────────

    #[test]
    fn is_git_repo_true_for_initialised_repo() {
        let dir = init_repo();
        assert!(is_git_repo(dir.path()));
    }

    #[test]
    fn is_git_repo_false_for_plain_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(!is_git_repo(dir.path()));
    }

    #[test]
    fn is_git_repo_true_for_subdirectory_of_repo() {
        let dir = init_repo();
        let sub = dir.path().join("nested/deep");
        std::fs::create_dir_all(&sub).expect("create dirs");
        assert!(is_git_repo(&sub));
    }

    // ── status ────────────────────────────────────────────────────────────────

    #[test]
    fn status_returns_none_for_non_repo() {
        let dir = tempfile::tempdir().expect("tempdir");
        let result = status(dir.path()).expect("status should not Err for non-repo");
        assert!(result.is_none());
    }

    #[test]
    fn status_returns_no_commits_error_for_empty_repo() {
        let dir = init_repo();
        let result = status(dir.path());
        assert!(
            matches!(result, Err(GitError::NoCommits)),
            "expected NoCommits, got {:?}",
            result
        );
    }

    #[test]
    fn status_returns_branch_and_clean_state_after_initial_commit() {
        let dir = init_repo();
        let repo = git2::Repository::open(dir.path()).expect("open");

        // Configure identity so git2 can build a signature.
        let mut cfg = repo.config().expect("config");
        cfg.set_str("user.name", "Test").expect("set name");
        cfg.set_str("user.email", "t@t.com").expect("set email");
        drop(cfg);

        // Create an empty tree and commit it.
        let tree_id = {
            let mut idx = repo.index().expect("index");
            idx.write_tree().expect("write_tree")
        };
        let tree = repo.find_tree(tree_id).expect("find_tree");
        let sig = repo.signature().expect("sig");
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .expect("commit");

        let s = status(dir.path())
            .expect("no error")
            .expect("Some status");

        assert!(!s.branch.is_empty());
        assert!(!s.is_dirty);
        assert_eq!(s.changed_files, 0);
    }
}
