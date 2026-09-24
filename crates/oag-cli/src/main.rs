mod commands;
mod config;
mod serve;

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Open Agent Graph — a single, self-contained peer.
#[derive(Parser)]
#[command(name = "oag", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start this peer's REST + MCP server.
    Serve {
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long)]
        listen: Option<SocketAddr>,
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Show this peer's identity and basic status.
    Status {
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
    },
    /// Search the local graph.
    Search {
        query: String,
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
        #[arg(long, default_value_t = 20)]
        limit: i64,
    },
    /// Node inspection commands.
    Node {
        #[command(subcommand)]
        command: NodeCommand,
    },
    /// Assertion inspection commands.
    Assertion {
        #[command(subcommand)]
        command: AssertionCommand,
    },
    /// Peer identity commands.
    Identity {
        #[command(subcommand)]
        command: IdentityCommand,
    },
    /// API key management.
    Key {
        #[command(subcommand)]
        command: KeyCommand,
    },
    /// Run local health checks.
    Doctor {
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
    },
}

#[derive(Subcommand)]
enum NodeCommand {
    Get {
        id: String,
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
    },
}

#[derive(Subcommand)]
enum AssertionCommand {
    Get {
        id: String,
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
    },
}

#[derive(Subcommand)]
enum IdentityCommand {
    Show {
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
    },
    Backup {
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Subcommand)]
enum KeyCommand {
    Create {
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
        #[arg(long, default_value = "agent")]
        actor_type: String,
        #[arg(long)]
        name: Option<String>,
        /// Repeatable, e.g. --permission graph:read --permission graph:assert
        #[arg(long = "permission")]
        permissions: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Serve { data_dir, listen, config } => {
            let resolved = config::resolve(config, data_dir, listen)?;
            serve::run(resolved).await
        }
        Command::Status { data_dir } => commands::status(&data_dir).await,
        Command::Search { query, data_dir, limit } => {
            commands::search(&data_dir, &query, limit).await
        }
        Command::Node { command: NodeCommand::Get { id, data_dir } } => {
            commands::node_get(&data_dir, &id).await
        }
        Command::Assertion { command: AssertionCommand::Get { id, data_dir } } => {
            commands::assertion_get(&data_dir, &id).await
        }
        Command::Identity { command: IdentityCommand::Show { data_dir } } => {
            commands::identity_show(&data_dir).await
        }
        Command::Identity { command: IdentityCommand::Backup { data_dir, out } } => {
            commands::identity_backup(&data_dir, &out).await
        }
        Command::Key { command: KeyCommand::Create { data_dir, actor_type, name, permissions } } => {
            commands::key_create(&data_dir, &actor_type, name, permissions).await
        }
        Command::Doctor { data_dir } => commands::doctor(&data_dir).await,
    }
}
