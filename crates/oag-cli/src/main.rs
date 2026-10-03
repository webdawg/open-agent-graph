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
        /// Rank by embedding cosine similarity (spec section 64) instead of
        /// keyword FTS. Requires `[search]` in config.toml to have a real
        /// embedding provider configured -- errors clearly rather than
        /// silently falling back if embeddings aren't set up.
        #[arg(long)]
        semantic: bool,
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Node inspection commands.
    Node {
        #[command(subcommand)]
        command: NodeCommand,
    },
    /// Actor inspection commands.
    Actor {
        #[command(subcommand)]
        command: ActorCommand,
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
    /// Node authority (PageRank-style) commands.
    Authority {
        #[command(subcommand)]
        command: AuthorityCommand,
    },
    /// Semantic search embedding commands (spec section 64).
    Embeddings {
        #[command(subcommand)]
        command: EmbeddingsCommand,
    },
    /// Create a consistent snapshot of the whole local database (spec
    /// section 82) -- safe to run against a live `oag serve` (uses SQLite's
    /// `VACUUM INTO`, not a raw file copy, which could grab an inconsistent
    /// mid-write state on an active WAL-mode database).
    Backup {
        out: PathBuf,
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
    },
    /// Wipe every derived graph table and replay the full signed event log
    /// to regenerate them from scratch (spec section 83). The event log
    /// itself is never modified. Best run with `oag serve` stopped.
    Rebuild {
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
    },
    /// Deletion and redaction commands (spec section 85) -- local-only,
    /// gated by filesystem access to the data directory. These can never
    /// reach copies of the same data already replicated to other peers.
    Redact {
        #[command(subcommand)]
        command: RedactCommand,
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
        #[arg(long)]
        config: Option<PathBuf>,
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
enum ActorCommand {
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
enum AuthorityCommand {
    /// Recompute every node's authority score from scratch (spec section
    /// 65) -- an on-demand batch job, not triggered automatically. Re-run
    /// periodically or after a large import; `oag node get` prints the
    /// current score for any node.
    Recompute {
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
    },
}

#[derive(Subcommand)]
enum EmbeddingsCommand {
    /// Recompute every node's embedding from scratch against the configured
    /// provider (spec section 64) -- an on-demand batch job, like `oag
    /// authority recompute`. Requires `[search]` in config.toml to have a
    /// real embedding provider configured; with embeddings left disabled
    /// (the default) this errors clearly rather than silently no-op'ing.
    Recompute {
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum RedactCommand {
    /// Permanently blank an evidence record's title/excerpt in this peer's
    /// own local database. Cannot be undone by any command in this tool,
    /// and cannot reach copies of this event already replicated to other
    /// peers -- refuses without --force.
    Evidence {
        evidence_id: String,
        #[arg(long)]
        reason: Option<String>,
        #[arg(long)]
        force: bool,
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
    },
    /// List every evidence redaction recorded on this peer, for auditing.
    List {
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
    },
    /// Hide a node from search results without touching the node, its
    /// edges, or its assertions. Fully reversible via unsuppress-node.
    SuppressNode {
        node_id: String,
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
    },
    UnsuppressNode {
        node_id: String,
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
    Get {
        id: String,
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
    },
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
    /// Restore identity.key from a backup made with `oag identity backup`.
    /// Refuses to overwrite an existing identity.key unless --force is
    /// given: silently swapping a peer's identity out from under its
    /// existing database would desync its peer_id from every event it has
    /// already signed under the old key.
    Restore {
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
        backup_path: PathBuf,
        #[arg(long)]
        force: bool,
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
    /// List every key ever issued on this peer (active and revoked), by
    /// hash -- never the raw key itself, which is only ever shown once at
    /// creation.
    List {
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
    },
    /// Revoke a key by the hash `oag key list` shows for it.
    Revoke {
        key_hash: String,
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
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
    /// Show recorded fork evidence for a peer (spec section 55) -- both
    /// conflicting event ids at each forked sequence, plus the full signed
    /// JSON of the incoming event that triggered detection (the
    /// already-accepted side is retrievable the normal way, by its id).
    Forks {
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
        Command::Search { query, data_dir, limit, semantic, config } => {
            commands::search(&data_dir, &query, limit, semantic, config).await
        }
        Command::Node { command: NodeCommand::Get { id, data_dir } } => {
            commands::node_get(&data_dir, &id).await
        }
        Command::Actor { command: ActorCommand::Get { id, data_dir } } => {
            commands::actor_get(&data_dir, &id).await
        }
        Command::Assertion { command: AssertionCommand::Get { id, data_dir } } => {
            commands::assertion_get(&data_dir, &id).await
        }
        Command::Edge { command: EdgeCommand::Get { id, data_dir } } => commands::edge_get(&data_dir, &id).await,
        Command::Edge { command: EdgeCommand::Corroboration { id, data_dir } } => {
            commands::edge_corroboration(&data_dir, &id).await
        }
        Command::Replication { command: ReplicationCommand::Status { data_dir } } => {
            commands::replication_status(&data_dir).await
        }
        Command::Authority { command: AuthorityCommand::Recompute { data_dir } } => {
            commands::authority_recompute(&data_dir).await
        }
        Command::Embeddings { command: EmbeddingsCommand::Recompute { data_dir, config } } => {
            commands::embeddings_recompute(&data_dir, config).await
        }
        Command::Backup { out, data_dir } => commands::backup(&data_dir, &out).await,
        Command::Rebuild { data_dir } => commands::rebuild(&data_dir).await,
        Command::Redact { command: RedactCommand::Evidence { evidence_id, reason, force, data_dir } } => {
            commands::redact_evidence(&data_dir, &evidence_id, reason, force).await
        }
        Command::Redact { command: RedactCommand::List { data_dir } } => commands::redact_list(&data_dir).await,
        Command::Redact { command: RedactCommand::SuppressNode { node_id, data_dir } } => {
            commands::redact_suppress_node(&data_dir, &node_id).await
        }
        Command::Redact { command: RedactCommand::UnsuppressNode { node_id, data_dir } } => {
            commands::redact_unsuppress_node(&data_dir, &node_id).await
        }
        Command::Identity { command: IdentityCommand::Show { data_dir } } => {
            commands::identity_show(&data_dir).await
        }
        Command::Identity { command: IdentityCommand::Backup { data_dir, out } } => {
            commands::identity_backup(&data_dir, &out).await
        }
        Command::Identity { command: IdentityCommand::Restore { data_dir, backup_path, force } } => {
            commands::identity_restore(&data_dir, &backup_path, force).await
        }
        Command::Key {
            command:
                KeyCommand::Create { data_dir, actor_type, name, permissions, public_key, key_proof, identity_uri },
        } => {
            commands::key_create(&data_dir, &actor_type, name, permissions, identity_uri, public_key, key_proof).await
        }
        Command::Key { command: KeyCommand::List { data_dir } } => commands::key_list(&data_dir).await,
        Command::Key { command: KeyCommand::Revoke { key_hash, data_dir } } => {
            commands::key_revoke(&data_dir, &key_hash).await
        }
        Command::Peer { command: PeerCommand::Add { url, data_dir } } => {
            commands::peer_add(&data_dir, &url).await
        }
        Command::Peer { command: PeerCommand::List { data_dir } } => commands::peer_list(&data_dir).await,
        Command::Peer { command: PeerCommand::Remove { peer_id, data_dir } } => {
            commands::peer_remove(&data_dir, &peer_id).await
        }
        Command::Peer { command: PeerCommand::Forks { peer_id, data_dir } } => {
            commands::peer_forks(&data_dir, &peer_id).await
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
        Command::Doctor { data_dir, config } => commands::doctor(&data_dir, config).await,
    }
}
