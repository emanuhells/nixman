use clap::{Parser, Subcommand};
use clap_complete::Shell;

mod commands;
mod diff_util;
mod value_parser;
mod error;
mod verbosity;
mod confirm;

pub use error::CliError;
mod pending_store;

fn build_version() -> &'static str {
    concat!(
        env!("CARGO_PKG_VERSION"),
        " (",
        env!("NIXMAN_GIT_HASH"),
        " ",
        env!("NIXMAN_BUILD_DATE"),
        ", rustc ",
        env!("NIXMAN_RUSTC_VERSION"),
        ")"
    )
}

#[derive(Parser)]
#[command(name = "nixman")]
#[command(about = "Manage NixOS configuration from the command line")]
#[command(version = build_version())]
pub struct Cli {
    /// Path to NixOS config workspace (auto-detects if omitted)
    #[arg(long, global = true)]
    workspace: Option<String>,

    /// Suppress informational messages
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Increase verbosity (-v verbose, -vv trace)
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Skip confirmation prompts
    #[arg(short = 'y', long, global = true)]
    yes: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Workspace detection and setup
    Workspace {
        #[command(subcommand)]
        command: commands::workspace::WorkspaceCmd,
    },
    /// Get, set, search NixOS options
    Option {
        #[command(subcommand)]
        command: commands::option::OptionCmd,
    },
    /// Manage pending changes
    Pending {
        #[command(subcommand)]
        command: commands::pending::PendingCmd,
    },
    /// Search and manage packages
    Packages {
        #[command(subcommand)]
        command: commands::packages::PackagesCmd,
    },
    /// List and control systemd services
    Services {
        #[command(subcommand)]
        command: commands::services::ServicesCmd,
    },
    /// List, diff, rollback generations
    Generations {
        #[command(subcommand)]
        command: commands::generations::GenerationsCmd,
    },
    /// Enhanced generation history with change context
    History {
        /// Show package changes between generations
        #[arg(long)]
        diff: bool,
    },
    /// Show uncommitted config changes
    Diff {
        /// Show only file-backed staged changes
        #[arg(long)]
        staged: bool,
    },
    /// Run system health checks
    Doctor,
    /// Manage flake inputs
    Flake {
        #[command(subcommand)]
        command: commands::flake::FlakeCmd,
    },
    /// Run nixos-rebuild
    Rebuild {
        /// Build mode: switch, boot, test, or build
        #[arg(default_value = "switch")]
        mode: String,
        /// Show what will change before building
        #[arg(long)]
        explain: bool,
        /// Auto-rollback if health checks fail after switch
        #[arg(long)]
        rollback_on_fail: bool,
    },
    /// Quick workspace and system overview
    Status,
    /// Validate configuration without building
    Check,
    /// Propose, review, apply, or discard configuration change plans
    Intent {
        #[command(subcommand)]
        command: commands::intent::IntentCmd,
    },
    /// Temporary config changes with auto-revert
    Try {
        #[command(subcommand)]
        command: commands::try_cmd::TryCmd,
    },
    /// Output command schema for AI agents
    Schema,
    /// Detect and fix deprecated options
    Migrate {
        /// Auto-fix renameable options
        #[arg(long)]
        fix: bool,
    },

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Complete option paths (hidden, used by shell completions)
    #[command(hide = true)]
    CompleteOption {
        /// Prefix to complete
        prefix: String,
    },
    /// Manage Home Manager user configuration
    Hm {
        #[command(subcommand)]
        command: commands::hm::HmCmd,
    },
    /// Explain a Nix error in plain English
    Explain {
        /// Error text to explain
        error: Option<String>,
        /// Read error from stdin
        #[arg(long)]
        stdin: bool,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let result = run(cli).await;
    match result {
        Ok(output) => {
            if !output.is_empty() {
                println!("{}", output);
            }
            std::process::exit(0);
        }
        Err(e) => {
            let cli_err = e.downcast_ref::<CliError>();
            if let Some(cli_err) = cli_err {
                if cli_err.is_noop() {
                    eprintln!("{}", cli_err.exit_message());
                    std::process::exit(3);
                }
                eprintln!("Error: {}", cli_err);
                std::process::exit(cli_err.exit_code());
            }
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

async fn run(cli: Cli) -> Result<String, Box<dyn std::error::Error>> {
    // This lets us skip the flakes gate for Legacy workspaces.
    let (workspace_path, workspace_kind) = match cli.workspace {
        Some(p) => {
            let path = std::path::PathBuf::from(p);
            let kind = if path.join("flake.nix").exists() {
                nixman_core::workspace::WorkspaceKind::Flake
            } else {
                // Without flake.nix treat as Legacy so preflight never blocks.
                nixman_core::workspace::WorkspaceKind::Legacy
            };
            (path, kind)
        }
        None => {
            match nixman_core::workspace::detect() {
                Ok(ws) => (ws.path, ws.kind),
                Err(_) => {
                    // Best-effort: fall back to cwd. Assume Legacy so
                    // preflight never blocks a workspace-less command.
                    (std::env::current_dir()?, nixman_core::workspace::WorkspaceKind::Legacy)
                }
            }
        }
    };

    // Pre-flight: ensure flakes are available when the workspace needs them.
    // Legacy workspaces and commands that don't touch flake config skip this.
    let needs_flakes = matches!(cli.command,
        Commands::Option { .. }
        | Commands::Packages { .. }
        | Commands::Flake { .. }
        | Commands::Rebuild { .. }
        | Commands::Intent { .. }
        | Commands::Pending { .. }
    );
    if needs_flakes
        && workspace_kind == nixman_core::workspace::WorkspaceKind::Flake
        && std::env::var("NIXMAN_SKIP_PREFLIGHT").is_err()
    {
        nixman_core::preflight::check_flakes_enabled()?;
    }

    match cli.command {
        Commands::Workspace { command } => commands::workspace::run(command).await,
        Commands::Option { command } => {
            commands::option::run(command, &workspace_path).await
        }
        Commands::Pending { command } => {
            commands::pending::run(command, &workspace_path).await
        }
        Commands::Packages { command } => {
            commands::packages::run(command, &workspace_path).await
        }
        Commands::Services { command } => commands::services::run(command).await,
        Commands::Generations { command } => commands::generations::run(command).await,
        Commands::History { diff } => commands::history::run(&workspace_path, diff).await,
        Commands::Diff { staged } => commands::diff::run(&workspace_path, staged).await,
        Commands::Doctor => commands::doctor::run(&workspace_path).await,
        Commands::Flake { command } => {
            commands::flake::run(command, &workspace_path).await
        }
        Commands::Rebuild { mode, explain, rollback_on_fail } => {
            commands::rebuild::run(&mode, &workspace_path, explain, rollback_on_fail).await
        }
        Commands::Status => commands::status::run(&workspace_path).await,
        Commands::Check => commands::check::run(&workspace_path).await,
        Commands::Intent { command } => commands::intent::run(command, &workspace_path).await,
        Commands::Try { command } => commands::try_cmd::run(command, &workspace_path).await,
        Commands::Schema => commands::schema::run().await,
        Commands::Migrate { fix } => commands::migrate::run(&workspace_path, fix).await,
        Commands::Completions { shell } => commands::completions::run(shell).await,
        Commands::CompleteOption { prefix } => {
            commands::completions::complete_option(&prefix, &workspace_path).await
        }
        Commands::Hm { command } => commands::hm::run(command).await,
        Commands::Explain { error, stdin } => commands::explain::run(error, stdin).await,
    }
}
