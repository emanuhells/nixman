use std::path::Path;

pub async fn run(
    workspace: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut info = serde_json::Map::new();

    info.insert("workspace".into(), serde_json::json!(workspace.display().to_string()));

    let kind = if workspace.join("flake.nix").exists() { "flake" } else { "legacy" };
    info.insert("kind".into(), serde_json::json!(kind));

    let hostname = std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".into());
    info.insert("hostname".into(), serde_json::json!(hostname));

    if let Ok(output) = tokio::process::Command::new("nixos-rebuild")
        .args(["list-generations", "--json"])
        .output().await
    {
        if output.status.success() {
            if let Ok(gens) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                if let Some(arr) = gens.as_array() {
                    if let Some(last) = arr.last() {
                        info.insert("current_generation".into(), last.clone());
                    }
                }
            }
        }
    }

    match nixman_core::packages::installed::list(workspace).await {
        Ok(pkgs) => { info.insert("package_count".into(), serde_json::json!(pkgs.len())); }
        Err(_) => { info.insert("package_count".into(), serde_json::json!(null)); }
    }

    let staged = crate::pending_store::StagedChanges::load(workspace);
    info.insert("pending_count".into(), serde_json::json!(staged.count()));

    if nixman_core::git::is_git_repo(workspace) {
        if let Ok(Some(git_status)) = nixman_core::git::status(workspace) {
            info.insert("git".into(), serde_json::json!(git_status));
        }
    }

    Ok(serde_json::to_string_pretty(&info)?)
}
