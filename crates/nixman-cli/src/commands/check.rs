use std::path::Path;

pub async fn run(
    workspace: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    let is_flake = workspace.join("flake.nix").exists();

    let output = if is_flake {
        let hostname = nixman_core::workspace::detect::get_hostname();
        let flake_ref = workspace.to_string_lossy();
        let attr = format!("{}#nixosConfigurations.{}.config.system.build.toplevel", flake_ref, hostname);

        tokio::process::Command::new("nix")
            .args(["eval", &attr, "--json"])
            .stderr(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .output()
            .await?
    } else {
        let config_path = workspace.join("configuration.nix");
        tokio::process::Command::new("nix-instantiate")
            .args(["--parse", &config_path.to_string_lossy()])
            .stderr(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .output()
            .await?
    };

    if output.status.success() {
        Ok(serde_json::to_string_pretty(&serde_json::json!({
            "valid": true,
            "message": "Config is clean. Safe to rebuild.",
            "errors": [],
            "warnings": []
        }))?)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let errors = parse_nix_errors(&stderr);
        Ok(serde_json::to_string_pretty(&serde_json::json!({
            "valid": false,
            "message": "Configuration has errors.",
            "errors": errors,
            "warnings": []
        }))?)
    }
}

/// Parse nix eval/instantiate stderr into structured error objects.
fn parse_nix_errors(stderr: &str) -> Vec<serde_json::Value> {
    let mut errors = Vec::new();
    for line in stderr.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        if line.contains("error:") {
            errors.push(serde_json::json!({
                "message": line,
                "type": "eval_error"
            }));
        } else if line.contains("Failed assertion") || line.contains("assertion") {
            errors.push(serde_json::json!({
                "message": line,
                "type": "assertion_failure"
            }));
        }
    }
    if errors.is_empty() && !stderr.trim().is_empty() {
        errors.push(serde_json::json!({
            "message": stderr.trim(),
            "type": "unknown"
        }));
    }
    errors
}
