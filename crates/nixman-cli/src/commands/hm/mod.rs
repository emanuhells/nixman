use clap::Subcommand;

mod option;
mod packages;
mod rebuild;
mod status;

#[derive(Subcommand)]
pub enum HmCmd {
    /// Show Home Manager workspace status
    Status,
    /// Get, set, search Home Manager options
    Option {
        #[command(subcommand)]
        command: option::HmOptionCmd,
    },
    /// Search and manage Home Manager packages
    Packages {
        #[command(subcommand)]
        command: packages::HmPackagesCmd,
    },
    /// Rebuild Home Manager configuration
    Rebuild(rebuild::HmRebuildArgs),
}

pub async fn run(cmd: HmCmd) -> Result<String, Box<dyn std::error::Error>> {
    match cmd {
        HmCmd::Status => status::run().await,
        HmCmd::Option { command } => option::run(command).await,
        HmCmd::Packages { command } => packages::run(command).await,
        HmCmd::Rebuild(args) => rebuild::run(args).await,
    }
}
