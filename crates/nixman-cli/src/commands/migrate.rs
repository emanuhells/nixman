use std::path::Path;

#[derive(serde::Serialize)]
struct MigrationIssue {
    old_path: String,
    new_path: Option<String>,
    version_removed: String,
    action: String,  // "rename", "remove", "replace"
    file: String,
    line: usize,
}

pub async fn run(
    workspace: &Path,
    auto_fix: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    let migrations = known_migrations();
    let mut issues: Vec<MigrationIssue> = Vec::new();

    let nix_files = find_nix_files(workspace)?;
    
    for file in &nix_files {
        let content = std::fs::read_to_string(file)?;
        for (line_num, line) in content.lines().enumerate() {
            for migration in &migrations {
                if line.contains(&migration.old_path) {
                    issues.push(MigrationIssue {
                        old_path: migration.old_path.clone(),
                        new_path: migration.new_path.clone(),
                        version_removed: migration.version.clone(),
                        action: migration.action.clone(),
                        file: file.to_string_lossy().to_string(),
                        line: line_num + 1,
                    });
                }
            }
        }
    }

    if issues.is_empty() {
        return Ok(serde_json::to_string_pretty(&serde_json::json!({
            "status": "up_to_date",
            "message": "Configuration is up to date. No deprecated options found.",
            "issues": []
        }))?);
    }

    if auto_fix {
        let mut fixed = 0;
        for issue in &issues {
            if issue.action == "rename" {
                if let Some(ref new_path) = issue.new_path {
                    let file_path = Path::new(&issue.file);
                    let content = std::fs::read_to_string(file_path)?;
                    let updated = content.replace(&issue.old_path, new_path);
                    if updated != content {
                        std::fs::write(file_path, updated)?;
                        fixed += 1;
                    }
                }
            }
        }
        return Ok(serde_json::to_string_pretty(&serde_json::json!({
            "status": "fixed",
            "message": format!("Fixed {} of {} issues.", fixed, issues.len()),
            "issues": issues,
        }))?);
    }

    Ok(serde_json::to_string_pretty(&serde_json::json!({
        "status": "issues_found",
        "message": format!("Found {} deprecated option(s).", issues.len()),
        "issues": issues,
    }))?)
}

struct Migration {
    old_path: String,
    new_path: Option<String>,
    version: String,
    action: String,
}

/// Known option migrations across NixOS versions.
fn known_migrations() -> Vec<Migration> {
    vec![
        // NixOS 24.05 → 24.11
        Migration {
            old_path: "services.xserver.displayManager.sddm".into(),
            new_path: Some("services.displayManager.sddm".into()),
            version: "24.11".into(),
            action: "rename".into(),
        },
        Migration {
            old_path: "services.xserver.displayManager.gdm".into(),
            new_path: Some("services.displayManager.gdm".into()),
            version: "24.11".into(),
            action: "rename".into(),
        },
        Migration {
            old_path: "services.xserver.displayManager.lightdm".into(),
            new_path: Some("services.displayManager.lightdm".into()),
            version: "24.11".into(),
            action: "rename".into(),
        },
        Migration {
            old_path: "services.xserver.displayManager.autoLogin".into(),
            new_path: Some("services.displayManager.autoLogin".into()),
            version: "24.11".into(),
            action: "rename".into(),
        },
        Migration {
            old_path: "services.xserver.displayManager.defaultSession".into(),
            new_path: Some("services.displayManager.defaultSession".into()),
            version: "24.11".into(),
            action: "rename".into(),
        },
        // NixOS 25.05
        Migration {
            old_path: "services.xserver.libinput.enable".into(),
            new_path: None,
            version: "25.05".into(),
            action: "remove".into(),
        },
        Migration {
            old_path: "services.xserver.libinput.touchpad".into(),
            new_path: None,
            version: "25.05".into(),
            action: "remove".into(),
        },
        // Networking
        Migration {
            old_path: "networking.useDHCP".into(),
            new_path: Some("networking.interfaces.<name>.useDHCP".into()),
            version: "24.05".into(),
            action: "rename".into(),
        },
        // Nvidia
        Migration {
            old_path: "hardware.nvidia.modesetting.enable".into(),
            new_path: Some("hardware.nvidia.open".into()),
            version: "25.05".into(),
            action: "rename".into(),
        },
        // Sound
        Migration {
            old_path: "sound.enable".into(),
            new_path: None,
            version: "25.05".into(),
            action: "remove".into(),
        },
    ]
}

/// Find all .nix files in workspace (recursive).
fn find_nix_files(workspace: &Path) -> Result<Vec<std::path::PathBuf>, Box<dyn std::error::Error>> {
    let mut files = Vec::new();
    find_nix_recursive(workspace, &mut files)?;
    Ok(files)
}

fn find_nix_recursive(dir: &Path, files: &mut Vec<std::path::PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    if !dir.is_dir() { return Ok(()); }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if !name.starts_with('.') && name != "result" {
                find_nix_recursive(&path, files)?;
            }
        } else if path.extension().map(|e| e == "nix").unwrap_or(false) {
            files.push(path);
        }
    }
    Ok(())
}
