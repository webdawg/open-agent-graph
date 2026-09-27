#[derive(Debug, thiserror::Error)]
pub enum EmbeddingError {
    /// The configured provider is `DisabledProvider` — spec section 64 requires
    /// OAG to work with embeddings completely disabled, so this is an expected,
    /// typed outcome rather than a bug.
    #[error("embeddings are disabled")]
    Disabled,
    #[error("embedding request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("embedding provider returned no data")]
    EmptyResponse,
}
