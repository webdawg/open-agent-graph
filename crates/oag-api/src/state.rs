use std::sync::Arc;

use oag_crawler::CrawlerService;
use oag_graph::GraphService;
use oag_sync::SyncService;

use crate::rate_limit::RateLimiter;

#[derive(Clone)]
pub struct AppState {
    pub graph: Arc<GraphService>,
    pub rate_limiter: Arc<RateLimiter>,
    pub embedding_provider: Arc<dyn oag_embeddings::EmbeddingProvider>,
    pub crawler: Arc<CrawlerService>,
    /// Backs `/metrics`'s replication-health gauges (spec section 86).
    /// Deliberately its own `SyncService` handle rather than threading the
    /// exact instance `oag serve` mounts its `/oag/sync/v1/*` router with:
    /// `SyncService` is a cheap `Clone` over the same shared pool, and
    /// `replication_status()` (a read-only query) is federation-policy-
    /// independent, so a second handle constructed straight from `graph`'s
    /// own pool/identity is exactly as correct and needs no wiring through
    /// `oag serve`'s startup sequence at all.
    pub sync: SyncService,
}

impl AppState {
    pub fn new(graph: Arc<GraphService>) -> Self {
        let crawler = Arc::new(CrawlerService::new(
            graph.clone(),
            oag_crawler::CrawlerConfig::default(),
            Arc::new(oag_crawler::DisabledExtractor),
        ));
        let sync = SyncService::new(
            graph.pool().clone(),
            graph.identity().peer_id(),
            graph.identity().verifying_key().to_bytes(),
            oag_sync::FederationPolicy::Open,
        );
        Self {
            graph,
            rate_limiter: Arc::new(RateLimiter::default()),
            embedding_provider: Arc::new(oag_embeddings::DisabledProvider),
            crawler,
            sync,
        }
    }

    /// Spec section 64: OAG must work with embeddings completely disabled,
    /// so `new` defaults to that -- this opts a real `oag serve` process
    /// into whatever provider `[search]` in config.toml resolves to.
    pub fn with_embedding_provider(mut self, provider: Arc<dyn oag_embeddings::EmbeddingProvider>) -> Self {
        self.embedding_provider = provider;
        self
    }

    /// `new`'s default `CrawlerService` uses `CrawlerConfig::default()`
    /// (SSRF defaults, no private networks) and `DisabledExtractor` -- this
    /// opts a real `oag serve` process into whatever `[crawler]` in
    /// config.toml actually resolves to.
    pub fn with_crawler(mut self, crawler: Arc<CrawlerService>) -> Self {
        self.crawler = crawler;
        self
    }
}
