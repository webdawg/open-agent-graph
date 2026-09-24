use std::net::SocketAddr;
use std::path::PathBuf;

use figment::providers::{Format, Toml};
use figment::Figment;
use serde::Deserialize;

fn default_listen() -> SocketAddr {
    "127.0.0.1:7443".parse().unwrap()
}

fn default_true() -> bool {
    true
}

/// `config.toml` per spec section 94.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // search.semantic_enabled is parsed for forward-compat, not used yet
pub struct FileConfig {
    #[serde(default)]
    pub data: DataSection,
    #[serde(default)]
    pub server: ServerSection,
    #[serde(default)]
    pub search: SearchSection,
    #[serde(default)]
    pub mcp: McpSection,
    #[serde(default)]
    pub network: NetworkSection,
    #[serde(default)]
    pub federation: FederationSection,
}

#[derive(Debug, Deserialize, Default)]
pub struct DataSection {
    pub directory: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub struct ServerSection {
    #[serde(default = "default_listen")]
    pub listen: SocketAddr,
}

impl Default for ServerSection {
    fn default() -> Self {
        Self {
            listen: default_listen(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SearchSection {
    #[serde(default)]
    pub semantic_enabled: bool,
}

impl Default for SearchSection {
    fn default() -> Self {
        Self { semantic_enabled: false }
    }
}

#[derive(Debug, Deserialize)]
pub struct McpSection {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for McpSection {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// `[network]` (spec section 94/45): who to contact on startup and how
/// often to re-sync with every known peer.
#[derive(Debug, Deserialize)]
pub struct NetworkSection {
    #[serde(default)]
    pub bootstrap_peers: Vec<String>,
    #[serde(default = "default_sync_interval")]
    pub sync_interval_seconds: u64,
}

fn default_sync_interval() -> u64 {
    30
}

impl Default for NetworkSection {
    fn default() -> Self {
        Self {
            bootstrap_peers: Vec::new(),
            sync_interval_seconds: default_sync_interval(),
        }
    }
}

/// `[federation]` (spec section 59): which origins' events this peer will
/// accept into storage. Enforced at the storage-acceptance layer
/// (`oag_sync::FederationPolicy`), not at the HTTP transport layer — see
/// `oag-sync`'s design notes.
#[derive(Debug, Deserialize)]
pub struct FederationSection {
    #[serde(default = "default_federation_mode")]
    pub mode: String,
    #[serde(default)]
    pub allowed_peers: Vec<String>,
}

fn default_federation_mode() -> String {
    "open".to_string()
}

impl Default for FederationSection {
    fn default() -> Self {
        Self {
            mode: default_federation_mode(),
            allowed_peers: Vec::new(),
        }
    }
}

impl FederationSection {
    pub fn to_policy(&self) -> anyhow::Result<oag_sync::FederationPolicy> {
        match self.mode.as_str() {
            "open" => Ok(oag_sync::FederationPolicy::Open),
            "allowlist" => {
                let peers = self
                    .allowed_peers
                    .iter()
                    .map(|s| s.parse::<oag_crypto::PeerId>())
                    .collect::<Result<_, _>>()
                    .map_err(|e| anyhow::anyhow!("invalid entry in federation.allowed_peers: {e}"))?;
                Ok(oag_sync::FederationPolicy::Allowlist(peers))
            }
            other => anyhow::bail!("unknown federation.mode '{other}', expected 'open' or 'allowlist'"),
        }
    }
}

impl Default for FileConfig {
    fn default() -> Self {
        Self {
            data: DataSection::default(),
            server: ServerSection::default(),
            search: SearchSection::default(),
            mcp: McpSection::default(),
            network: NetworkSection::default(),
            federation: FederationSection::default(),
        }
    }
}

/// The fully resolved settings a `serve` invocation runs with: CLI flags
/// take priority, falling back to `--config <file>` (file + `OAG_*` env
/// overrides via figment), falling back to hardcoded defaults.
pub struct ResolvedConfig {
    pub data_dir: PathBuf,
    pub listen: SocketAddr,
    pub mcp_enabled: bool,
    pub bootstrap_peers: Vec<String>,
    pub sync_interval: std::time::Duration,
    pub federation: oag_sync::FederationPolicy,
}

pub fn resolve(
    config_path: Option<PathBuf>,
    data_dir_override: Option<PathBuf>,
    listen_override: Option<SocketAddr>,
) -> anyhow::Result<ResolvedConfig> {
    let mut figment = Figment::new();
    if let Some(path) = &config_path {
        figment = figment.merge(Toml::file(path));
    }
    figment = figment.merge(figment::providers::Env::prefixed("OAG_").split("_"));

    // figment needs a concrete target; extract into FileConfig, tolerating a
    // completely absent/empty config source via Default.
    let file: FileConfig = if config_path.is_some() {
        figment.extract()?
    } else {
        FileConfig::default()
    };

    let data_dir = data_dir_override
        .or(file.data.directory)
        .unwrap_or_else(|| PathBuf::from("./data"));
    let listen = listen_override.unwrap_or(file.server.listen);
    let federation = file.federation.to_policy()?;

    Ok(ResolvedConfig {
        data_dir,
        listen,
        mcp_enabled: file.mcp.enabled,
        bootstrap_peers: file.network.bootstrap_peers,
        sync_interval: std::time::Duration::from_secs(file.network.sync_interval_seconds),
        federation,
    })
}
