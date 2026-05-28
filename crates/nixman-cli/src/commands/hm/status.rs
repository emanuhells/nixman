use nixman_core::workspace::hm::detect_hm;

pub async fn run() -> Result<String, Box<dyn std::error::Error>> {
    match detect_hm() {
        Ok(ws) => {
            let kind_str = match ws.kind {
                nixman_core::workspace::WorkspaceKind::Flake => "flake",
                nixman_core::workspace::WorkspaceKind::Legacy => "legacy",
            };
            let output = serde_json::to_string_pretty(&serde_json::json!({
                "path": ws.path.display().to_string(),
                "kind": kind_str,
                "username": ws.username,
            }))?;
            Ok(output)
        }
        Err(e) => Err(format!("Home Manager workspace not found: {}", e).into()),
    }
}
