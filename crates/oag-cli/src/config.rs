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

/// `config.toml` per spec section 94. `[network]`/`[federation]` are parsed
/// but unused this milestone (no replication yet) — kept here only so the
/// file format doesn't need to change shape when that milestone lands.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // network/federation/search are parsed for forward-compat, not used yet
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
    pub network: serde_json::Value,
    #[serde(default)]
    pub federation: serde_json::Value,
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

impl Default for FileConfig {
    fn default() -> Self {
        Self {
            data: DataSection::default(),
            server: ServerSection::default(),
            search: SearchSection::default(),
            mcp: McpSection::default(),
            network: serde_json::json!({}),
            federation: serde_json::json!({}),
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
}

pub fn resolve(
    config_path: Option<PathBuf>,
    data_dir_override: Option<PathBuf>,
    listen_override: Option<SocketAddr>,
) -> anyhow::Result<ResolvedConfig> {
    let mut figment = Figment::from(figment::providers::Serialized::defaults(()));
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

    Ok(ResolvedConfig {
        data_dir,
        listen,
        mcp_enabled: file.mcp.enabled,
    })
}
