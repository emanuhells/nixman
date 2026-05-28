use std::path::Path;

pub async fn run(
    workspace: &Path,
    staged_only: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    if staged_only {
        let staged = crate::pending_store::StagedChanges::load(workspace);
        if staged.is_empty() {
            return Ok("No staged changes.".to_string());
        }
        return Ok(serde_json::to_string_pretty(&staged.changes)?);
    }

    if !nixman_core::git::is_git_repo(workspace) {
        return Err("Workspace is not a git repository. Cannot show diff.".into());
    }

    let output = tokio::process::Command::new("git")
        .args(["diff", "--no-color"])
        .current_dir(workspace)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git diff failed: {}", stderr).into());
    }

    let diff_text = String::from_utf8_lossy(&output.stdout).to_string();
    if diff_text.trim().is_empty() {
        return Ok("No uncommitted changes.".to_string());
    }

    Ok(diff_text)
}
