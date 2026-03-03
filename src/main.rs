mod cli;
mod engine;
mod lang;
mod mcp;

use clap::Parser;

/// Top-level CLI for yoyo.
#[derive(Parser, Debug)]
#[command(name = "yoyo", version, about = "yoyo – Rust code intelligence engine and MCP server")]
struct Cli {
    /// Run as MCP server instead of human CLI.
    #[arg(long)]
    mcp_server: bool,

    /// Optional subcommand for human-facing CLI.
    #[command(subcommand)]
    command: Option<cli::Command>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.mcp_server {
        mcp::run_stdio_server().await?;
    } else {
        cli::run(cli.command).await?;
    }

    Ok(())
}

