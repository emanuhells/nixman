use clap::Subcommand;


#[derive(Subcommand)]
pub enum GenerationsCmd {
    /// List all system generations
    List,
    /// Show package diff between two generations
    Diff {
        /// Older generation number
        from: u32,
        /// Newer generation number
        to: u32,
    },
    /// Roll back to a previous generation
    Rollback {
        /// Generation number to activate
        number: u32,
    },
    /// Delete old generations and run garbage collection
    Gc {
        /// Number of most-recent generations to keep
        #[arg(long)]
        keep: Option<u32>,
    },
}

pub async fn run(
    cmd: GenerationsCmd,
) -> Result<String, Box<dyn std::error::Error>> {
    match cmd {
        GenerationsCmd::List => {
            let gens = nixman_core::generations::list::all().await?;
            Ok(serde_json::to_string_pretty(&gens)?)
        }
        GenerationsCmd::Diff { from, to } => {
            let diff = nixman_core::generations::diff::compare(from, to).await?;
            Ok(serde_json::to_string_pretty(&diff)?)
        }
        GenerationsCmd::Rollback { number } => {
            nixman_core::generations::rollback::to(number).await?;
            Ok(format!("Rolled back to generation {}", number))
        }
        GenerationsCmd::Gc { keep } => {
            let result = nixman_core::generations::gc::collect(keep).await?;
            Ok(serde_json::to_string_pretty(&result)?)
        }
    }
}
