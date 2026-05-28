//! nixman-mcp — MCP server for nixman
//!
//! Exposes nixman-core functions as MCP tools over stdio or HTTP transport.

use clap::Parser;

mod server;

#[cfg(feature = "http")]
mod auth;

#[derive(Parser)]
#[command(
    name = "nixman-mcp",
    about = "MCP server for NixOS configuration management"
)]
struct Args {
    /// Transport: stdio or http
    #[arg(long, default_value = "stdio")]
    transport: String,

    /// HTTP listen port (used with --transport http)
    #[arg(long, default_value_t = 9876)]
    port: u16,

    /// Path to the NixOS workspace (auto-detects if omitted)
    #[arg(long)]
    workspace: Option<String>,

    /// API key for HTTP auth (optional)
    #[arg(long, env = "NIXMAN_MCP_TOKEN")]
    token: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialise tracing (stderr so stdio transport is uncontaminated).
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();

    let workspace = match args.workspace {
        Some(p) => p,
        None => {
            nixman_core::workspace::detect()
                .map(|w| w.path.display().to_string())
                .unwrap_or_else(|_| "/etc/nixos".to_string())
        }
    };
    match args.transport.as_str() {
        "stdio" => run_stdio(workspace).await,
        "http" => run_http(workspace, args.port, args.token).await,
        other => {
            anyhow::bail!("Unknown transport: {other}. Use --transport stdio or --transport http")
        }
    }
}

async fn run_stdio(workspace: String) -> anyhow::Result<()> {
    tracing::info!("Starting nixman-mcp over stdio (workspace: {workspace})");

    use rmcp::ServiceExt;

    let service = server::NixmanMcpServer::new(workspace);
    let peer = service
        .serve(rmcp::transport::stdio())
        .await
        .inspect_err(|e| tracing::error!("Failed to serve stdio transport: {e:?}"))?;

    peer.waiting().await?;
    Ok(())
}

#[cfg(feature = "http")]
async fn run_http(workspace: String, port: u16, token: Option<String>) -> anyhow::Result<()> {
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager,
        StreamableHttpServerConfig,
        StreamableHttpService,
    };
    use tokio_util::sync::CancellationToken;

    let bind = format!("0.0.0.0:{port}");
    tracing::info!("Starting nixman-mcp over HTTP on {bind} (workspace: {workspace})");

    let ct = CancellationToken::new();

    let ws_clone = workspace.clone();
    let service = StreamableHttpService::new(
        move || Ok(server::NixmanMcpServer::new(ws_clone.clone())),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default().with_cancellation_token(ct.child_token()),
    );

    let router = axum::Router::new()
        .nest_service("/mcp", service)
        .layer(axum::middleware::from_fn(auth::require_auth(token)));

    let tcp_listener = tokio::net::TcpListener::bind(&bind).await?;
    axum::serve(tcp_listener, router)
        .with_graceful_shutdown(async move {
            tokio::signal::ctrl_c().await.unwrap();
            ct.cancel();
        })
        .await?;

    Ok(())
}

#[cfg(not(feature = "http"))]
async fn run_http(_workspace: String, _port: u16, _token: Option<String>) -> anyhow::Result<()> {
    anyhow::bail!("HTTP transport not available — compile with the 'http' feature")
}
