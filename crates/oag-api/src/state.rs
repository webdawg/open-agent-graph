use std::sync::atomic::AtomicU64;
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
    /// Spec section 86's `peer_sync_errors` counter. Deliberately *not*
    /// read from `Self::sync` above -- that handle is its own independent
    /// `SyncService` instance (see its doc comment), so its own internal
    /// counter would only ever read zero. `oag serve` instead hands this
    /// the exact `Arc` the real gossip loop increments
    /// (`SyncService::sync_errors_counter`), via `Self::with_sync_errors`.
    /// Defaults to a fresh, always-zero counter so every other construction
    /// site (including every test) keeps working unchanged.
    pub sync_errors: Arc<AtomicU64>,
    /// The operator's own Kubo node's dialable libp2p WebSocket multiaddr
    /// (`[ipfs].libp2p_ws_multiaddr`), if configured -- handed to a browser
    /// loading `/ipfs-site/{cid}` as a direct dial target. `oag serve`
    /// itself never dials it; this is purely informational for the browser
    /// bundle (see `crates/oag-api/src/ipfs_site.rs`).
    pub ipfs_libp2p_ws_multiaddr: Option<Arc<str>>,
    /// Backs `POST /oag/transfer/v1/fetch` (`spec/31-service-receipts.md`) --
    /// `None` (the default) makes that endpoint answer every request with
    /// `ProviderDisabled`, same opt-in posture as every other `[ipfs]`
    /// setting. Distinct from `crawler`'s own internal provider handle
    /// (`CrawlerService` holds one but doesn't expose it) -- this is the one
    /// peer-to-peer serving needs to call directly.
    pub pinning_provider: Option<Arc<dyn oag_ipfs::PinningProvider>>,
    /// Per-caller (keyed by the signed request's `client_peer_id`, not the
    /// bearer-token-or-"anonymous" keying `rate_limit_middleware` uses)
    /// budget for `POST /oag/transfer/v1/fetch` -- a real IPFS fetch plus a
    /// graph write per request is meaningfully more expensive than most
    /// other routes, so it gets its own counter rather than sharing the
    /// single global "anonymous" bucket every other unauthenticated route
    /// (`/ipfs-site/*`) falls into.
    pub transfer_rate_limiter: Arc<RateLimiter>,
    /// Multi-hop relay (`spec/31-service-receipts.md` Phase 2,
    /// `[transfer].relay_peers` in `config.toml`) -- statically
    /// operator-configured upstream peers this peer may forward a transfer
    /// cache-miss to. Empty by default, same opt-in posture as every other
    /// `[ipfs]`/`[transfer]` setting; a server never relays unless an
    /// operator explicitly lists at least one upstream peer.
    pub relay_peers: Arc<Vec<crate::transfer::RelayPeer>>,
    /// A plain `reqwest::Client` this peer uses to act as the *client* of
    /// another peer when relaying -- distinct from any client the CLI uses,
    /// since relaying happens from inside a running `oag serve` process.
    pub http_client: Arc<reqwest::Client>,
}

impl AppState {
    pub fn new(graph: Arc<GraphService>) -> Self {
        let crawler = Arc::new(CrawlerService::new(
            graph.clone(),
            oag_crawler::CrawlerConfig::default(),
            Arc::new(oag_crawler::DisabledExtractor),
            Arc::new(oag_ipfs::DisabledProvider),
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
            sync_errors: Arc::new(AtomicU64::new(0)),
            ipfs_libp2p_ws_multiaddr: None,
            pinning_provider: None,
            transfer_rate_limiter: Arc::new(RateLimiter::default()),
            relay_peers: Arc::new(Vec::new()),
            http_client: Arc::new(reqwest::Client::new()),
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

    /// `new`'s default has no configured multiaddr -- this opts a real `oag
    /// serve` process into whatever `[ipfs].libp2p_ws_multiaddr` resolves
    /// to (unset by default, same opt-in posture as every other `[ipfs]`
    /// setting).
    pub fn with_ipfs_libp2p_ws_multiaddr(mut self, multiaddr: Option<Arc<str>>) -> Self {
        self.ipfs_libp2p_ws_multiaddr = multiaddr;
        self
    }

    /// `new`'s default has no provider -- this opts a real `oag serve`
    /// process into whatever `[ipfs].pinning_provider` resolves to, the same
    /// `Arc<dyn PinningProvider>` the crawler and `oag ipfs`/`oag backup
    /// --pin` already share.
    pub fn with_pinning_provider(mut self, provider: Option<Arc<dyn oag_ipfs::PinningProvider>>) -> Self {
        self.pinning_provider = provider;
        self
    }

    /// `new`'s default has no relay peers -- this opts a running `oag serve`
    /// process into whatever `[transfer].relay_peers` resolves to (empty by
    /// default, same opt-in posture as every other `[ipfs]`/`[transfer]`
    /// setting).
    pub fn with_relay_peers(mut self, relay_peers: Vec<crate::transfer::RelayPeer>) -> Self {
        self.relay_peers = Arc::new(relay_peers);
        self
    }

    /// `new`'s default is a fresh, always-zero counter -- this shares the
    /// real gossip loop's own `Arc<AtomicU64>` (`SyncService::
    /// sync_errors_counter`) so `/metrics`'s `peer_sync_errors` reflects
    /// live sync activity instead of an unrelated handle's zero.
    pub fn with_sync_errors(mut self, sync_errors: Arc<AtomicU64>) -> Self {
        self.sync_errors = sync_errors;
        self
    }
}
