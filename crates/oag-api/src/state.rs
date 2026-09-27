use std::sync::Arc;

use oag_graph::GraphService;

use crate::rate_limit::RateLimiter;

#[derive(Clone)]
pub struct AppState {
    pub graph: Arc<GraphService>,
    pub rate_limiter: Arc<RateLimiter>,
    pub embedding_provider: Arc<dyn oag_embeddings::EmbeddingProvider>,
}

impl AppState {
    pub fn new(graph: Arc<GraphService>) -> Self {
        Self {
            graph,
            rate_limiter: Arc::new(RateLimiter::default()),
            embedding_provider: Arc::new(oag_embeddings::DisabledProvider),
        }
    }

    /// Spec section 64: OAG must work with embeddings completely disabled,
    /// so `new` defaults to that -- this opts a real `oag serve` process
    /// into whatever provider `[search]` in config.toml resolves to.
    pub fn with_embedding_provider(mut self, provider: Arc<dyn oag_embeddings::EmbeddingProvider>) -> Self {
        self.embedding_provider = provider;
        self
    }
}
