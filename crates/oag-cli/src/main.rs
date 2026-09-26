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
    /// Edge inspection commands.
    Edge {
        #[command(subcommand)]
        command: EdgeCommand,
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
    /// Peer replication commands.
    Peer {
        #[command(subcommand)]
        command: PeerCommand,
    },
    /// Durability/replication-factor visibility commands.
    Replication {
        #[command(subcommand)]
        command: ReplicationCommand,
    },
    /// Crawl one URL: fetch it safely, extract structured facts (JSON-LD,
    /// llms.txt, ARD, A2A), and assert them as evidence-backed claims.
    Crawl {
        url: String,
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
        #[arg(long)]
        config: Option<PathBuf>,
        /// Overrides config.toml's [crawler].allow_private_networks — off
        /// by default (spec section 72); only enable this for a trusted,
        /// deliberately-internal target.
        #[arg(long)]
        allow_private_networks: bool,
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
enum ReplicationCommand {
    /// Full durability status: target replication factor, how many known
    /// peers are confirmed caught up with this peer's own data, and which
    /// ones are lagging (or have never reported anything at all).
    Status {
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
    },
}

#[derive(Subcommand)]
enum EdgeCommand {
    /// Corroboration signals for one edge: source independence, evidence
    /// strength, and agreement — never collapsed into a single score.
    Corroboration {
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
        /// Hex-encoded Ed25519 public key this actor controls. Requires
        /// --key-proof. The actor's identity_assurance ranking signal (spec
        /// section 65) is only raised for actors registered with a verified
        /// key, not a bare claim.
        #[arg(long, requires = "key_proof")]
        public_key: Option<String>,
        /// Hex-encoded 64-byte signature proving possession of --public-key:
        /// sign_with_domain(your_signing_key, "OAG:ACTOR_KEY_PROOF:v1:",
        /// canonical_json_bytes({"actor_type": ..., "name": ..., "identity_uri": ...}))
        /// using the exact --actor-type/--name/--identity-uri given here.
        #[arg(long, requires = "public_key")]
        key_proof: Option<String>,
        #[arg(long)]
        identity_uri: Option<String>,
    },
}

#[derive(Subcommand)]
enum PeerCommand {
    /// Sync with a peer at URL immediately and remember it for `peer sync`.
    Add {
        url: String,
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
    },
    /// List known peers, their addresses, and fork status.
    List {
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
    },
    /// Forget a peer (does not affect already-replicated events).
    Remove {
        peer_id: String,
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
    },
    /// Sync with a previously-known peer (by id) or a fresh URL.
    Sync {
        peer_id_or_url: String,
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
    },
    /// Print this peer's Reticulum destination address, for sharing
    /// out-of-band with a peer that wants to sync over Reticulum.
    ReticulumAddress {
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
    },
    /// One-shot sync with a peer reachable over Reticulum, given its
    /// destination address and a TCP interface to reach the network
    /// through (either that peer's own `listen_tcp`, or a shared transport
    /// node both peers can reach).
    AddReticulum {
        address_hash: String,
        via_tcp: String,
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
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
        Command::Edge { command: EdgeCommand::Corroboration { id, data_dir } } => {
            commands::edge_corroboration(&data_dir, &id).await
        }
        Command::Replication { command: ReplicationCommand::Status { data_dir } } => {
            commands::replication_status(&data_dir).await
        }
        Command::Identity { command: IdentityCommand::Show { data_dir } } => {
            commands::identity_show(&data_dir).await
        }
        Command::Identity { command: IdentityCommand::Backup { data_dir, out } } => {
            commands::identity_backup(&data_dir, &out).await
        }
        Command::Key {
            command:
                KeyCommand::Create { data_dir, actor_type, name, permissions, public_key, key_proof, identity_uri },
        } => {
            commands::key_create(&data_dir, &actor_type, name, permissions, identity_uri, public_key, key_proof).await
        }
        Command::Peer { command: PeerCommand::Add { url, data_dir } } => {
            commands::peer_add(&data_dir, &url).await
        }
        Command::Peer { command: PeerCommand::List { data_dir } } => commands::peer_list(&data_dir).await,
        Command::Peer { command: PeerCommand::Remove { peer_id, data_dir } } => {
            commands::peer_remove(&data_dir, &peer_id).await
        }
        Command::Peer { command: PeerCommand::Sync { peer_id_or_url, data_dir } } => {
            commands::peer_sync(&data_dir, &peer_id_or_url).await
        }
        Command::Peer { command: PeerCommand::ReticulumAddress { data_dir } } => {
            commands::peer_reticulum_address(&data_dir).await
        }
        Command::Peer { command: PeerCommand::AddReticulum { address_hash, via_tcp, data_dir } } => {
            commands::peer_add_reticulum(&data_dir, &address_hash, &via_tcp).await
        }
        Command::Crawl { url, data_dir, config, allow_private_networks } => {
            commands::crawl(&data_dir, &url, config, allow_private_networks).await
        }
        Command::Doctor { data_dir } => commands::doctor(&data_dir).await,
    }
}
