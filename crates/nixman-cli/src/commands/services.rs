use clap::Subcommand;


#[derive(Subcommand)]
pub enum ServicesCmd {
    /// List all systemd service units
    List,
    /// Show status of a specific service
    Get { unit: String },
    /// Start a service
    Start { unit: String },
    /// Stop a service
    Stop { unit: String },
    /// Restart a service
    Restart { unit: String },
    /// Show journal logs for a service
    Logs {
        unit: String,
        /// Number of log lines to show
        #[arg(short = 'n', long, default_value = "50")]
        lines: u32,
    },
}

pub async fn run(
    cmd: ServicesCmd,
) -> Result<String, Box<dyn std::error::Error>> {
    match cmd {
        ServicesCmd::List => {
            let services = nixman_core::services::status::list_all().await?;
            Ok(serde_json::to_string_pretty(&services)?)
        }
        ServicesCmd::Get { unit } => {
            let resolved = nixman_core::services::config_map::resolve(&unit);
            let info = nixman_core::services::status::get(&resolved).await?;
            Ok(serde_json::to_string_pretty(&info)?)
        }
        ServicesCmd::Start { unit } => {
            let resolved = nixman_core::services::config_map::resolve(&unit);
            nixman_core::services::actions::start(&resolved).await?;
            Ok(format!("Started {}", resolved))
        }
        ServicesCmd::Stop { unit } => {
            let resolved = nixman_core::services::config_map::resolve(&unit);
            nixman_core::services::actions::stop(&resolved).await?;
            Ok(format!("Stopped {}", resolved))
        }
        ServicesCmd::Restart { unit } => {
            let resolved = nixman_core::services::config_map::resolve(&unit);
            nixman_core::services::actions::restart(&resolved).await?;
            Ok(format!("Restarted {}", resolved))
        }
        ServicesCmd::Logs { unit, lines } => {
            let resolved = nixman_core::services::config_map::resolve(&unit);
            let entries = nixman_core::services::logs::get(&resolved, lines).await?;
            Ok(serde_json::to_string_pretty(&entries)?)
        }
    }
}
