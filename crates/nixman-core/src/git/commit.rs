use std::path::Path;

use crate::git::types::{CommitRequest, GitError};

// ── Public API ────────────────────────────────────────────────────────────────

/// Stage the files listed in `request` and create a new commit in the
/// repository at (or above) `repo_path`.
///
/// Returns the full 40-character hex SHA-1 of the new commit on success.
///
/// # Errors
///
/// * [`GitError::NotARepo`] — `repo_path` is not inside a git repository.
/// * [`GitError::StagingFailed`] — a file could not be added to the index.
/// * [`GitError::CommitFailed`] — the commit object could not be written
///   (e.g. no git identity configured).
pub fn create(repo_path: &Path, request: CommitRequest) -> Result<String, GitError> {
    let repo = git2::Repository::discover(repo_path).map_err(|_| GitError::NotARepo)?;

    // Bare repos have no working tree; we cannot stage files there.
    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::CommitFailed("bare repositories are not supported".into()))?
        .to_path_buf();

    // ── Stage requested files ─────────────────────────────────────────────────
    let mut index = repo
        .index()
        .map_err(|e| GitError::StagingFailed(e.message().to_string()))?;

    for file in &request.files {
        // Convert absolute paths to repo-root-relative paths so git2 can
        // find them in the index.
        let rel = if file.is_absolute() {
            file.strip_prefix(&workdir)
                .map_err(|_| {
                    GitError::StagingFailed(format!(
                        "{} is outside the repository working tree",
                        file.display()
                    ))
                })?
                .to_path_buf()
        } else {
            file.clone()
        };

        index.add_path(&rel).map_err(|e| {
            GitError::StagingFailed(format!("{}: {}", rel.display(), e.message()))
        })?;
    }

    index
        .write()
        .map_err(|e| GitError::StagingFailed(e.message().to_string()))?;

    // ── Build the tree from the updated index ─────────────────────────────────
    let tree_id = index
        .write_tree()
        .map_err(|e| GitError::CommitFailed(e.message().to_string()))?;

    let tree = repo
        .find_tree(tree_id)
        .map_err(|e| GitError::CommitFailed(e.message().to_string()))?;

    // ── Resolve author / committer from git config ────────────────────────────
    let sig = repo.signature().map_err(|e| {
        GitError::CommitFailed(format!("no git identity configured: {}", e.message()))
    })?;

    // ── Collect parent commit (empty slice for the very first commit) ─────────
    let parent_commit: Option<git2::Commit<'_>> = match repo.head() {
        Ok(head) => {
            let oid = head
                .target()
                .ok_or_else(|| GitError::CommitFailed("HEAD has no target OID".into()))?;
            Some(
                repo.find_commit(oid)
                    .map_err(|e| GitError::CommitFailed(e.message().to_string()))?,
            )
        }
        Err(e) if e.code() == git2::ErrorCode::UnbornBranch => None,
        Err(e) => return Err(GitError::CommitFailed(e.message().to_string())),
    };

    let parents: Vec<&git2::Commit<'_>> = parent_commit.iter().collect();

    let commit_id = repo
        .commit(
            Some("HEAD"),
            &sig,
            &sig,
            &request.message,
            &tree,
            &parents,
        )
        .map_err(|e| GitError::CommitFailed(e.message().to_string()))?;

    Ok(commit_id.to_string())
}

/// Generate a commit message that summarises which NixOS options changed.
///
/// The result always starts with `"nixman: "` followed by a short
/// human-readable description of the changes.  When more than three options
/// changed, only the first three are listed and a count suffix is appended.
///
/// # Examples
///
/// ```
/// # use nixman_core::git::commit::suggest_message;
/// let msg = suggest_message(&["services.nginx.enable".to_string()]);
/// assert!(msg.starts_with("nixman:"));
/// assert!(msg.contains("nginx"));
/// ```
pub fn suggest_message(changed_options: &[String]) -> String {
    if changed_options.is_empty() {
        return "nixman: update configuration".to_string();
    }

    let descriptions: Vec<String> = changed_options
        .iter()
        .map(|opt| describe_option(opt))
        .collect();

    let summary = if descriptions.len() <= 3 {
        descriptions.join(", ")
    } else {
        format!(
            "{}, and {} more",
            descriptions[..3].join(", "),
            descriptions.len() - 3
        )
    };

    format!("nixman: {}", summary)
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Convert a dotted NixOS option path into a short human-readable phrase.
///
/// Common patterns are matched explicitly; everything else falls back to a
/// generic two-component description derived from the last two path segments.
///
/// | Option path                           | Result                          |
/// |---------------------------------------|---------------------------------|
/// | `services.nginx.enable`               | `enable nginx`                  |
/// | `networking.firewall.allowedTCPPorts` | `update firewall allowed ports` |
/// | `boot.loader.systemd-boot.enable`     | `configure systemd-boot bootloader` |
/// | `environment.systemPackages`          | `update system packages`        |
fn describe_option(opt: &str) -> String {
    let parts: Vec<&str> = opt.split('.').collect();

    match parts.as_slice() {
        // ── services ──────────────────────────────────────────────────────────
        ["services", svc, "enable"] => format!("enable {svc}"),
        ["services", svc, ..] => format!("update {svc} service"),

        // ── networking ───────────────────────────────────────────────────────
        ["networking", "firewall", field, ..] => {
            format!("update firewall {}", humanise(field))
        }
        ["networking", field, ..] => format!("update network {}", humanise(field)),

        // ── boot ──────────────────────────────────────────────────────────────
        ["boot", "loader", loader, ..] => format!("configure {loader} bootloader"),
        ["boot", ..] => "update boot configuration".to_string(),

        // ── environment ───────────────────────────────────────────────────────
        ["environment", "systemPackages"] => "update system packages".to_string(),
        ["environment", field, ..] => format!("update environment {}", humanise(field)),

        // ── users ─────────────────────────────────────────────────────────────
        ["users", "users", user, ..] => format!("update user {user}"),
        ["users", ..] => "update users".to_string(),

        // ── hardware ─────────────────────────────────────────────────────────
        ["hardware", field, ..] => format!("configure {} hardware", humanise(field)),

        // ── programs ─────────────────────────────────────────────────────────
        ["programs", prog, "enable"] => format!("enable {prog} program"),
        ["programs", prog, ..] => format!("update {prog} program"),

        // ── generic fallback: last two path segments ──────────────────────────
        [.., parent, leaf] => format!("{} {}", humanise(leaf), humanise(parent)),
        [single] => humanise(single),
        [] => "update configuration".to_string(),
    }
}

/// Convert a camelCase or hyphen-separated identifier to lowercase spaced words.
///
/// | Input              | Output              |
/// |--------------------|---------------------|
/// | `allowedTCPPorts`  | `allowed tcp ports` |
/// | `systemd-boot`     | `systemd boot`      |
/// | `firewall`         | `firewall`          |
fn humanise(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    let mut prev_was_upper = false;

    for (i, c) in s.chars().enumerate() {
        if c == '-' || c == '_' {
            if !result.ends_with(' ') {
                result.push(' ');
            }
            prev_was_upper = false;
        } else if c.is_uppercase() {
            // Insert a space before a run of uppercase letters begins,
            // but not between consecutive uppercase letters (e.g. "TCP").
            if i > 0 && !prev_was_upper && !result.ends_with(' ') {
                result.push(' ');
            }
            result.push(c.to_ascii_lowercase());
            prev_was_upper = true;
        } else {
            // Lowercase letter after an uppercase run: insert a space before
            // the *last* uppercase letter of the run (e.g. "TCPorts" → "tc ports").
            // We detect this by checking if the previous character was uppercase
            // and the result has more than one char.
            if prev_was_upper && result.len() > 1 {
                let last = result.pop().unwrap(); // last uppercase letter
                if !result.ends_with(' ') {
                    result.push(' ');
                }
                result.push(last);
            }
            result.push(c);
            prev_was_upper = false;
        }
    }

    result
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ── suggest_message ───────────────────────────────────────────────────────

    #[test]
    fn suggest_message_empty_slice() {
        assert_eq!(suggest_message(&[]), "nixman: update configuration");
    }

    #[test]
    fn suggest_message_nginx_enable() {
        let opts = vec!["services.nginx.enable".to_string()];
        let msg = suggest_message(&opts);
        assert!(msg.starts_with("nixman:"), "prefix missing: {msg}");
        assert!(msg.contains("nginx"), "nginx not in: {msg}");
    }

    #[test]
    fn suggest_message_three_options() {
        let opts = vec![
            "services.nginx.enable".to_string(),
            "networking.firewall.allowedTCPPorts".to_string(),
            "environment.systemPackages".to_string(),
        ];
        let msg = suggest_message(&opts);
        assert!(msg.starts_with("nixman:"), "{msg}");
        // All three descriptions should appear (no "more" suffix).
        assert!(!msg.contains("more"), "unexpected truncation: {msg}");
    }

    #[test]
    fn suggest_message_truncates_beyond_three() {
        let opts: Vec<String> = (0..5)
            .map(|i| format!("services.svc{i}.enable"))
            .collect();
        let msg = suggest_message(&opts);
        assert!(msg.contains("and 2 more"), "expected truncation suffix: {msg}");
    }

    #[test]
    fn suggest_message_meaningful_for_firewall() {
        let opts = vec!["networking.firewall.allowedTCPPorts".to_string()];
        let msg = suggest_message(&opts);
        assert!(msg.contains("firewall"), "{msg}");
    }

    // ── humanise ─────────────────────────────────────────────────────────────

    #[test]
    fn humanise_hyphen_separated() {
        assert_eq!(humanise("systemd-boot"), "systemd boot");
    }

    #[test]
    fn humanise_plain_lowercase() {
        assert_eq!(humanise("firewall"), "firewall");
    }

    // ── create ────────────────────────────────────────────────────────────────

    fn setup_repo_with_identity() -> TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = git2::Repository::init(dir.path()).expect("git init");
        let mut cfg = repo.config().expect("config");
        cfg.set_str("user.name", "Nixman Test").expect("name");
        cfg.set_str("user.email", "test@nixman").expect("email");
        dir
    }

    #[test]
    fn create_initial_commit_returns_40_char_sha() {
        let dir = setup_repo_with_identity();
        let file = dir.path().join("configuration.nix");
        fs::write(&file, "{ }").expect("write");

        let req = CommitRequest {
            message: "initial commit".to_string(),
            files: vec![file],
        };
        let hash = create(dir.path(), req).expect("commit");
        assert_eq!(hash.len(), 40, "expected full SHA1 hex, got: {hash}");
    }

    #[test]
    fn create_second_commit_with_relative_path() {
        let dir = setup_repo_with_identity();
        let repo = git2::Repository::open(dir.path()).expect("open");

        // First commit: empty tree.
        let sig = repo.signature().expect("sig");
        let tree_id = repo.index().expect("idx").write_tree().expect("tree");
        let tree = repo.find_tree(tree_id).expect("find");
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .expect("first commit");

        // Second commit via our helper using a relative path.
        let file = dir.path().join("flake.nix");
        fs::write(&file, "{ outputs = _: {}; }").expect("write");

        let req = CommitRequest {
            message: "add flake.nix".to_string(),
            files: vec![std::path::PathBuf::from("flake.nix")],
        };
        let hash = create(dir.path(), req).expect("second commit");
        assert_eq!(hash.len(), 40, "expected SHA1 hex, got: {hash}");
    }

    #[test]
    fn create_errors_on_non_repo() {
        let dir = tempfile::tempdir().expect("tempdir");
        let req = CommitRequest {
            message: "test".to_string(),
            files: vec![],
        };
        assert!(
            matches!(create(dir.path(), req), Err(GitError::NotARepo)),
            "expected NotARepo"
        );
    }
}
