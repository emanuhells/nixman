use clap::Subcommand;

#[derive(Subcommand)]
pub enum WorkspaceCmd {
    /// Auto-detect NixOS configuration location
    Detect,
    /// Run first-time setup wizard
    Wizard {
        /// Target directory for new workspace
        #[arg(long)]
        path: Option<String>,
    },
}

pub async fn run(cmd: WorkspaceCmd) -> Result<String, Box<dyn std::error::Error>> {
    match cmd {
        WorkspaceCmd::Detect => {
            let ws = nixman_core::workspace::detect()?;
            Ok(serde_json::to_string_pretty(&serde_json::json!({
                "path": ws.path.display().to_string(),
                "kind": format!("{:?}", ws.kind),
                "owner": {
                    "uid": ws.owner.uid,
                    "is_user_owned": ws.owner.is_user_owned,
                },
                "hostname": ws.hostname,
            }))?)
        }
        WorkspaceCmd::Wizard { path } => {
            let target = path
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("./nix-config"));
            nixman_core::workspace::wizard::create_flake_workspace(&target)?;
            Ok(format!("Workspace created at {}", target.display()))
        }
    }
}
