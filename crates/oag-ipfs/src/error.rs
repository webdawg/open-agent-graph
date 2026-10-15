#[derive(Debug, thiserror::Error)]
pub enum IpfsError {
    /// The configured provider is `DisabledProvider` — IPFS pinning must work
    /// completely disabled (the default), so this is an expected, typed
    /// outcome rather than a bug, matching `EmbeddingError::Disabled`.
    #[error("IPFS pinning is disabled")]
    Disabled,
    #[error("IPFS request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("IPFS provider returned no CID")]
    EmptyResponse,
    #[error("embedded IPFS node error: {0}")]
    Embedded(String),
}
