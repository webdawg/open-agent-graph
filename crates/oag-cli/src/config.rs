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
#[derive(Debug, Default, Deserialize)]
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
    #[serde(default)]
    pub crawler: CrawlerSection,
    #[serde(default)]
    pub reticulum: ReticulumSection,
    #[serde(default)]
    pub identity: IdentitySection,
    #[serde(default)]
    pub cluster: ClusterSection,
    #[serde(default)]
    pub ipfs: IpfsSection,
    #[serde(default)]
    pub addressing: AddressingSection,
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

/// `[search]` (spec section 64/94): semantic search is opt-in and OAG must
/// work with it entirely disabled by default. `embedding_provider = "disabled"`
/// (the default) makes that the literal config default too, not just a
/// runtime fallback. `semantic_enabled` controls whether `oag search` ranks
/// semantically *by default* without needing `--semantic`; the flag can
/// still force it on for a single invocation.
#[derive(Debug, Deserialize)]
pub struct SearchSection {
    #[serde(default)]
    pub semantic_enabled: bool,
    #[serde(default = "default_embedding_provider")]
    pub embedding_provider: String,
    pub embedding_base_url: Option<String>,
    pub embedding_api_key: Option<String>,
    #[serde(default = "default_embedding_model")]
    pub embedding_model: String,
}

fn default_embedding_provider() -> String {
    "disabled".to_string()
}

fn default_embedding_model() -> String {
    "text-embedding-3-small".to_string()
}

impl Default for SearchSection {
    fn default() -> Self {
        Self {
            semantic_enabled: false,
            embedding_provider: default_embedding_provider(),
            embedding_base_url: None,
            embedding_api_key: None,
            embedding_model: default_embedding_model(),
        }
    }
}

impl SearchSection {
    /// `"disabled"` (the default) or a missing `embedding_base_url` both
    /// yield `DisabledProvider` — spec section 64: OAG must work with
    /// embeddings completely disabled, so an incomplete config degrades to
    /// that safe default rather than erroring at startup.
    pub fn to_embedding_provider(&self) -> Box<dyn oag_embeddings::EmbeddingProvider> {
        match self.embedding_provider.as_str() {
            "openai_compatible" => match &self.embedding_base_url {
                Some(base_url) => Box::new(oag_embeddings::OpenAiCompatibleProvider::new(
                    base_url.clone(),
                    self.embedding_api_key.clone(),
                    self.embedding_model.clone(),
                )),
                None => Box::new(oag_embeddings::DisabledProvider),
            },
            _ => Box::new(oag_embeddings::DisabledProvider),
        }
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

/// `[crawler]` (spec section 94, section 72's SSRF defaults). `enabled` is
/// parsed for forward-compatibility with a future background-crawl-job
/// milestone; `oag crawl` (a synchronous one-shot command, v1's scope) runs
/// regardless of it — this only gates whatever automatic crawling a later
/// milestone adds to `oag serve` itself.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct CrawlerSection {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub allow_private_networks: bool,
    #[serde(default = "default_max_response_bytes")]
    pub max_response_bytes: usize,
    #[serde(default = "default_crawler_timeout_seconds")]
    pub request_timeout_seconds: u64,
    /// spec section 74: LLM extraction is opt-in and OAG must work with it
    /// entirely disabled -- same shape and default as `[search]`'s
    /// `embedding_provider`.
    #[serde(default = "default_llm_provider")]
    pub llm_provider: String,
    pub llm_base_url: Option<String>,
    pub llm_api_key: Option<String>,
    #[serde(default = "default_llm_model")]
    pub llm_model: String,
}

fn default_llm_provider() -> String {
    "disabled".to_string()
}

fn default_llm_model() -> String {
    "gpt-4o-mini".to_string()
}

fn default_max_response_bytes() -> usize {
    10 * 1024 * 1024
}

fn default_crawler_timeout_seconds() -> u64 {
    30
}

impl Default for CrawlerSection {
    fn default() -> Self {
        Self {
            enabled: true,
            allow_private_networks: false,
            max_response_bytes: default_max_response_bytes(),
            request_timeout_seconds: default_crawler_timeout_seconds(),
            llm_provider: default_llm_provider(),
            llm_base_url: None,
            llm_api_key: None,
            llm_model: default_llm_model(),
        }
    }
}

impl CrawlerSection {
    pub fn to_crawler_config(&self, allow_private_networks_override: bool) -> oag_crawler::CrawlerConfig {
        oag_crawler::CrawlerConfig {
            allow_private_networks: self.allow_private_networks || allow_private_networks_override,
            max_response_bytes: self.max_response_bytes,
            request_timeout: std::time::Duration::from_secs(self.request_timeout_seconds),
            ..oag_crawler::CrawlerConfig::default()
        }
    }

    /// `"disabled"` (the default) or a missing `llm_base_url` both yield
    /// `DisabledExtractor` -- spec section 74/11: OAG must work with LLM
    /// extraction completely disabled, so an incomplete config degrades to
    /// that safe default rather than erroring at startup.
    pub fn to_llm_extractor(&self) -> std::sync::Arc<dyn oag_crawler::LlmExtractor> {
        match self.llm_provider.as_str() {
            "openai_compatible" => match &self.llm_base_url {
                Some(base_url) => std::sync::Arc::new(oag_crawler::OpenAiCompatibleExtractor::new(
                    base_url.clone(),
                    self.llm_api_key.clone(),
                    self.llm_model.clone(),
                )),
                None => std::sync::Arc::new(oag_crawler::DisabledExtractor),
            },
            _ => std::sync::Arc::new(oag_crawler::DisabledExtractor),
        }
    }

}

/// `[ipfs]` (spec/29-ipfs-pinning.md): native IPFS pinning is opt-in and OAG
/// must work with it entirely disabled -- same shape and default as
/// `[search]`'s `embedding_provider`. Its own top-level section, not nested
/// under `[crawler]`: pinning now has two independent callers (`oag crawl`'s
/// automatic page-pinning and `oag backup --pin`), and backup has nothing to
/// do with crawling.
#[derive(Debug, Deserialize)]
pub struct IpfsSection {
    #[serde(default = "default_ipfs_pinning_provider")]
    pub pinning_provider: String,
    pub api_base_url: Option<String>,
    pub api_key: Option<String>,
    /// The operator's own Kubo node's dialable libp2p WebSocket multiaddr
    /// (e.g. `/ip4/1.2.3.4/tcp/4002/ws/p2p/<peer-id>`), unset by default.
    /// Purely informational: handed to a browser loading `oag ipfs
    /// host-page`-served content as a direct dial target, so it can reach
    /// this specific Kubo node without waiting on public DHT discovery.
    /// Never used for anything server-side -- `oag serve` itself never
    /// dials it.
    pub libp2p_ws_multiaddr: Option<String>,
    /// Only used by `pinning_provider = "embedded"`: multiaddrs for the
    /// embedded node to listen on. Default (when unset): one ephemeral-port
    /// `/ip4/0.0.0.0/tcp/0/ws` entry -- the only transport a browser can
    /// dial directly. Set this for a fixed, firewall-forwardable port.
    pub embedded_listen_addrs: Option<Vec<String>>,
}

fn default_ipfs_pinning_provider() -> String {
    "disabled".to_string()
}

impl Default for IpfsSection {
    fn default() -> Self {
        Self {
            pinning_provider: default_ipfs_pinning_provider(),
            api_base_url: None,
            api_key: None,
            libp2p_ws_multiaddr: None,
            embedded_listen_addrs: None,
        }
    }
}

fn default_embedded_listen_addrs() -> Vec<String> {
    vec!["/ip4/0.0.0.0/tcp/0/ws".to_string()]
}

impl IpfsSection {
    /// `"disabled"` (the default) or a missing `api_base_url` (for `"kubo"`)
    /// both yield `DisabledProvider` -- same "incomplete config degrades to
    /// the safe default rather than erroring at startup" posture as
    /// `to_llm_extractor`/`to_embedding_provider`. `"embedded"` is the only
    /// branch that's genuinely async/fallible (it starts a real libp2p
    /// node), which is why this whole function is -- a real signature
    /// change from the single-file-pinning milestone that first wrote this,
    /// when every branch was a cheap, synchronous constructor. `data_dir`
    /// is only used by `"embedded"`, to place its on-disk blockstore at
    /// `<data_dir>/ipfs/`, separate from OAG's own SQLite database.
    pub async fn to_pinning_provider(
        &self,
        data_dir: &std::path::Path,
    ) -> anyhow::Result<std::sync::Arc<dyn oag_ipfs::PinningProvider>> {
        match self.pinning_provider.as_str() {
            "kubo" => Ok(match &self.api_base_url {
                Some(base_url) => std::sync::Arc::new(oag_ipfs::KuboHttpProvider::new(
                    base_url.clone(),
                    self.api_key.clone(),
                )),
                None => std::sync::Arc::new(oag_ipfs::DisabledProvider),
            }),
            "embedded" => {
                let listen_addrs = self
                    .embedded_listen_addrs
                    .clone()
                    .unwrap_or_else(default_embedded_listen_addrs)
                    .iter()
                    .map(|addr| addr.parse())
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| anyhow::anyhow!("invalid [ipfs].embedded_listen_addrs entry: {e}"))?;
                let provider = oag_ipfs::EmbeddedIpfsProvider::new(data_dir.join("ipfs"), listen_addrs)
                    .await
                    .map_err(|e| anyhow::anyhow!("failed to start embedded IPFS node: {e}"))?;
                Ok(std::sync::Arc::new(provider))
            }
            _ => Ok(std::sync::Arc::new(oag_ipfs::DisabledProvider)),
        }
    }
}

/// `[reticulum]` — an additional, optional peer transport (this milestone's
/// plan): OAG peers can still only replicate over HTTP by default.
/// `enabled = false` by default given the dependency's pre-1.0 maturity —
/// this is opt-in, not a replacement for `oag-sync`'s HTTP path.
#[derive(Debug, Deserialize)]
pub struct ReticulumSection {
    #[serde(default)]
    pub enabled: bool,
    pub listen_tcp: Option<SocketAddr>,
    pub uplink_tcp: Option<String>,
    #[serde(default = "default_announce_interval_seconds")]
    pub announce_interval_seconds: u64,
}

fn default_announce_interval_seconds() -> u64 {
    300
}

impl Default for ReticulumSection {
    fn default() -> Self {
        Self {
            enabled: false,
            listen_tcp: None,
            uplink_tcp: None,
            announce_interval_seconds: default_announce_interval_seconds(),
        }
    }
}

impl ReticulumSection {
    pub fn to_reticulum_config(&self) -> Option<oag_reticulum::ReticulumConfig> {
        if !self.enabled {
            return None;
        }
        Some(oag_reticulum::ReticulumConfig {
            listen_tcp: self.listen_tcp,
            uplink_tcp: self.uplink_tcp.clone(),
            announce_interval: std::time::Duration::from_secs(self.announce_interval_seconds),
        })
    }
}


/// `[identity]` — ephemeral peer trust, Phase 1 (spec/24). `ephemeral =
/// false` by default: permanent, `identity.key`-backed identity stays the
/// default behavior for every existing deployment, same opt-in shape as
/// `[reticulum]`. When `ephemeral = true`, `oag serve` generates a fresh
/// identity every start and never reads or writes `identity.key` at all —
/// a clean fork at startup, not a new mode bolted onto the persistent path.
#[derive(Debug, Deserialize)]
pub struct IdentitySection {
    #[serde(default)]
    pub ephemeral: bool,
    #[serde(default = "default_heartbeat_interval_seconds")]
    pub heartbeat_interval_seconds: u64,
    #[serde(default = "default_trust_rebuild_seconds")]
    pub trust_rebuild_seconds: i64,
    #[serde(default = "default_announced_restart_grace_seconds")]
    pub announced_restart_grace_seconds: i64,
}

fn default_heartbeat_interval_seconds() -> u64 {
    30
}

fn default_trust_rebuild_seconds() -> i64 {
    3600
}

fn default_announced_restart_grace_seconds() -> i64 {
    300
}

impl Default for IdentitySection {
    fn default() -> Self {
        Self {
            ephemeral: false,
            heartbeat_interval_seconds: default_heartbeat_interval_seconds(),
            trust_rebuild_seconds: default_trust_rebuild_seconds(),
            announced_restart_grace_seconds: default_announced_restart_grace_seconds(),
        }
    }
}

impl IdentitySection {
    pub fn to_trust_config(&self) -> oag_sync::TrustConfig {
        oag_sync::TrustConfig {
            heartbeat_interval_seconds: self.heartbeat_interval_seconds as i64,
            trust_rebuild_seconds: self.trust_rebuild_seconds,
            announced_restart_grace_seconds: self.announced_restart_grace_seconds,
        }
    }
}

/// `[cluster]` — peer clustering, Phase 1 (spec/26). `name` unset (the
/// default): this peer isn't in any cluster, zero behavior change, same
/// opt-in-by-absence shape as `[identity]`/`[reticulum]`. When set, this
/// peer broadcasts a signed claim of membership and grants any other peer
/// claiming the *same* name a full federation override (see
/// `SyncService::federation_allows`'s security caveat).
#[derive(Debug, Deserialize)]
pub struct ClusterSection {
    pub name: Option<String>,
    #[serde(default = "default_cluster_announce_interval_seconds")]
    pub announce_interval_seconds: u64,
    /// Peer clustering, Phase 2 (spec/26): how often this peer syncs with
    /// its *own* same-cluster peers specifically, independent of
    /// `[network].sync_interval_seconds`'s standard, graph-wide cadence.
    /// Defaults meaningfully faster (5s vs. that section's own 30s
    /// default) -- a cluster is meant to be a tightly-cooperating group,
    /// not just a federation-override label.
    #[serde(default = "default_cluster_gossip_interval_seconds")]
    pub gossip_interval_seconds: u64,
}

fn default_cluster_announce_interval_seconds() -> u64 {
    60
}

fn default_cluster_gossip_interval_seconds() -> u64 {
    5
}

impl Default for ClusterSection {
    fn default() -> Self {
        Self {
            name: None,
            announce_interval_seconds: default_cluster_announce_interval_seconds(),
            gossip_interval_seconds: default_cluster_gossip_interval_seconds(),
        }
    }
}

/// `[addressing]` — long-word subdomain addressing, Phase 1
/// (spec/30-long-word-addressing.md). `domain` unset (the default): this
/// feature is entirely absent, same opt-in-by-absence shape as
/// `[cluster]`/`[reticulum]`. Domain ownership/cost is deliberately left
/// entirely to the operator -- this is nothing but a config value with no
/// real default, not wired into `oag serve`'s startup sequence at all (see
/// spec/30's "deliberately out of scope").
#[derive(Debug, Deserialize, Default)]
pub struct AddressingSection {
    pub domain: Option<String>,
}

/// The fully resolved settings a `serve` invocation runs with: CLI flags
/// take priority, falling back to `--config <file>` (file + `OAG_*` env
/// overrides via figment), falling back to hardcoded defaults.
pub struct ResolvedConfig {
    /// The `--config <path>` this was resolved from, if any -- threaded
    /// through so `oag serve` can periodically re-read the same file for
    /// the trust-config reload loop (spec/24 Phase 2). `None` means there's
    /// nothing to reload from, same as every other field here defaulting
    /// when no config file was given.
    pub config_path: Option<PathBuf>,
    pub data_dir: PathBuf,
    pub listen: SocketAddr,
    pub mcp_enabled: bool,
    pub bootstrap_peers: Vec<String>,
    pub sync_interval: std::time::Duration,
    pub federation: oag_sync::FederationPolicy,
    pub reticulum: Option<oag_reticulum::ReticulumConfig>,
    pub identity_ephemeral: bool,
    pub heartbeat_interval: std::time::Duration,
    pub trust_config: oag_sync::TrustConfig,
    pub cluster_name: Option<String>,
    pub cluster_announce_interval: std::time::Duration,
    /// Peer clustering, Phase 2 (spec/26): `spawn_cluster_gossip_loop`'s
    /// own interval, independent of `sync_interval` above.
    pub cluster_gossip_interval: std::time::Duration,
    pub embedding_provider: Box<dyn oag_embeddings::EmbeddingProvider>,
    pub crawler_config: oag_crawler::CrawlerConfig,
    pub llm_extractor: std::sync::Arc<dyn oag_crawler::LlmExtractor>,
    pub pinning_provider: std::sync::Arc<dyn oag_ipfs::PinningProvider>,
    pub ipfs_libp2p_ws_multiaddr: Option<String>,
}

pub async fn resolve(
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
    let pinning_provider = file.ipfs.to_pinning_provider(&data_dir).await?;

    Ok(ResolvedConfig {
        config_path,
        data_dir,
        listen,
        mcp_enabled: file.mcp.enabled,
        bootstrap_peers: file.network.bootstrap_peers,
        sync_interval: std::time::Duration::from_secs(file.network.sync_interval_seconds),
        federation,
        reticulum: file.reticulum.to_reticulum_config(),
        identity_ephemeral: file.identity.ephemeral,
        heartbeat_interval: std::time::Duration::from_secs(file.identity.heartbeat_interval_seconds),
        trust_config: file.identity.to_trust_config(),
        cluster_name: file.cluster.name,
        cluster_announce_interval: std::time::Duration::from_secs(file.cluster.announce_interval_seconds),
        cluster_gossip_interval: std::time::Duration::from_secs(file.cluster.gossip_interval_seconds),
        embedding_provider: file.search.to_embedding_provider(),
        // `false` here, always -- unlike `oag crawl`'s one-shot
        // `--allow-private-networks` flag (an operator's own explicit,
        // local, per-invocation opt-in), a running `oag serve` has no
        // per-request override at all: only `[crawler].allow_private_networks`
        // in config.toml can ever enable it. A REST/MCP-triggered crawl
        // (spec section 71, exposed remotely) must never let a caller
        // widen this peer's own SSRF policy for their request.
        crawler_config: file.crawler.to_crawler_config(false),
        llm_extractor: file.crawler.to_llm_extractor(),
        pinning_provider,
        ipfs_libp2p_ws_multiaddr: file.ipfs.libp2p_ws_multiaddr.clone(),
    })
}

/// `oag crawl`/`oag ipfs cat`'s resolved `[crawler]`-section providers.
type CrawlerProviders = (
    oag_crawler::CrawlerConfig,
    std::sync::Arc<dyn oag_crawler::LlmExtractor>,
    std::sync::Arc<dyn oag_ipfs::PinningProvider>,
);

/// Lightweight resolver for `oag crawl`, which — like `oag peer *` — is a
/// one-shot command that doesn't need `serve`'s full `ResolvedConfig`
/// (data dir is passed separately, network/federation are irrelevant here).
/// Returns the LLM extractor (spec section 74) and pinning provider (spec/29)
/// alongside the fetch config since all three come from the same
/// `[crawler]` section.
pub async fn resolve_crawler_config(
    config_path: Option<PathBuf>,
    allow_private_networks_flag: bool,
    data_dir: &std::path::Path,
) -> anyhow::Result<CrawlerProviders> {
    let mut figment = Figment::new();
    if let Some(path) = &config_path {
        figment = figment.merge(Toml::file(path));
    }
    figment = figment.merge(figment::providers::Env::prefixed("OAG_").split("_"));

    let file: FileConfig = if config_path.is_some() {
        figment.extract()?
    } else {
        FileConfig::default()
    };

    let pinning_provider = file.ipfs.to_pinning_provider(data_dir).await?;
    Ok((
        file.crawler.to_crawler_config(allow_private_networks_flag),
        file.crawler.to_llm_extractor(),
        pinning_provider,
    ))
}

/// Lightweight resolver for `oag embeddings recompute` and `oag search
/// --semantic`, which — like `oag crawl` — are one-shot commands that don't
/// need `serve`'s full `ResolvedConfig`.
pub fn resolve_search_section(config_path: Option<PathBuf>) -> anyhow::Result<SearchSection> {
    let mut figment = Figment::new();
    if let Some(path) = &config_path {
        figment = figment.merge(Toml::file(path));
    }
    figment = figment.merge(figment::providers::Env::prefixed("OAG_").split("_"));

    let file: FileConfig = if config_path.is_some() {
        figment.extract()?
    } else {
        FileConfig::default()
    };

    Ok(file.search)
}

/// Lightweight resolver for `oag backup --pin`, which — unlike `oag crawl`
/// — has nothing to do with the `[crawler]` section at all; it only needs
/// `[ipfs]`'s pinning provider.
pub async fn resolve_pinning_provider(
    config_path: Option<PathBuf>,
    data_dir: &std::path::Path,
) -> anyhow::Result<std::sync::Arc<dyn oag_ipfs::PinningProvider>> {
    let mut figment = Figment::new();
    if let Some(path) = &config_path {
        figment = figment.merge(Toml::file(path));
    }
    figment = figment.merge(figment::providers::Env::prefixed("OAG_").split("_"));

    let file: FileConfig = if config_path.is_some() {
        figment.extract()?
    } else {
        FileConfig::default()
    };

    file.ipfs.to_pinning_provider(data_dir).await
}

/// Lightweight resolver for `oag serve`'s trust-config reload loop
/// (spec/24-ephemeral-peer-trust.md Phase 2) -- periodically re-reads just
/// `[identity]` from the same config file `oag serve` started from,
/// identical figment-loading shape to `resolve_search_section`/
/// `resolve_pinning_provider`.
pub fn resolve_identity_section(config_path: Option<PathBuf>) -> anyhow::Result<IdentitySection> {
    let mut figment = Figment::new();
    if let Some(path) = &config_path {
        figment = figment.merge(Toml::file(path));
    }
    figment = figment.merge(figment::providers::Env::prefixed("OAG_").split("_"));

    let file: FileConfig = if config_path.is_some() {
        figment.extract()?
    } else {
        FileConfig::default()
    };

    Ok(file.identity)
}

/// Lightweight resolver for `oag identity show`/`oag peer resolve`
/// (spec/30-long-word-addressing.md) -- neither needs `serve`'s full
/// `ResolvedConfig`, just `[addressing]`.
pub fn resolve_addressing_section(config_path: Option<PathBuf>) -> anyhow::Result<AddressingSection> {
    let mut figment = Figment::new();
    if let Some(path) = &config_path {
        figment = figment.merge(Toml::file(path));
    }
    figment = figment.merge(figment::providers::Env::prefixed("OAG_").split("_"));

    let file: FileConfig = if config_path.is_some() {
        figment.extract()?
    } else {
        FileConfig::default()
    };

    Ok(file.addressing)
}
