use std::sync::Arc;

use oag_core::{ActorType, Permission};
use oag_crypto::PeerIdentity;
use oag_graph::GraphService;

use crate::config::ResolvedConfig;

/// `oag serve` (spec section 91's startup sequence): create/load identity ->
/// open/migrate SQLite -> bootstrap admin key if this is a fresh data dir ->
/// start REST -> start MCP -> start the sync HTTP endpoints -> start the
/// gossip loop against any configured bootstrap peers.
pub async fn run(config: ResolvedConfig) -> anyhow::Result<()> {
    std::fs::create_dir_all(&config.data_dir)?;

    // Ephemeral peer trust, Phase 1 (spec/24): a clean fork at startup, not
    // a new mode bolted onto the persistent path -- `identity.key` is never
    // read or written at all when ephemeral mode is on.
    let identity = if config.identity_ephemeral {
        PeerIdentity::generate()
    } else {
        let identity_path = config.data_dir.join("identity.key");
        PeerIdentity::load_or_generate(&identity_path)?
    };
    let peer_id = identity.peer_id();

    let db_path = config.data_dir.join("oag.sqlite");
    let pool = oag_storage::open_pool(&db_path).await?;

    let self_public_key = identity.verifying_key().to_bytes();
    let pool_for_sync = pool.clone();
    let pool_for_reticulum = pool.clone();
    let identity_for_reticulum = identity.clone();
    let identity_for_heartbeats = identity.clone();
    let identity_for_cluster = identity.clone();

    let graph = Arc::new(GraphService::new(pool, identity));

    if !graph.has_any_actor().await? {
        bootstrap_admin(&graph).await?;
    }

    let mut sync_service = oag_sync::SyncService::new(pool_for_sync, peer_id, self_public_key, config.federation);
    if config.identity_ephemeral {
        sync_service = sync_service
            .with_minimum_trust(oag_sync::EPHEMERAL_MINIMUM_TRUST, config.trust_config)
            .with_heartbeat_identity(identity_for_heartbeats);
        oag_sync::spawn_heartbeat_loop(sync_service.clone(), config.heartbeat_interval);
        // Ephemeral peer trust, Phase 2 (spec/24): live config reload for
        // the trust-check windows. Only meaningful in ephemeral mode (the
        // only mode that ever sets a nonzero minimum_trust), and only when
        // there's an actual file to re-read.
        if let Some(config_path) = config.config_path.clone() {
            tokio::spawn(reload_trust_config_periodically(
                sync_service.clone(),
                config_path,
                TRUST_CONFIG_RELOAD_INTERVAL,
            ));
        }
    }
    // Peer clustering, Phase 1 (spec/26): independent of ephemeral mode --
    // works the same for a permanent identity.
    if let Some(cluster_name) = config.cluster_name.clone() {
        sync_service = sync_service.with_cluster(identity_for_cluster, cluster_name);
        oag_sync::spawn_cluster_announce_loop(sync_service.clone(), config.cluster_announce_interval);
        oag_sync::spawn_cluster_gossip_loop(sync_service.clone(), config.cluster_gossip_interval);
    }
    let sync_router = oag_sync::router(sync_service.clone());
    oag_sync::spawn_gossip_loop(sync_service.clone(), config.bootstrap_peers.clone(), config.sync_interval);

    if let Some(reticulum_config) = config.reticulum.clone() {
        let address_hash = oag_reticulum::local_address_hash(&identity_for_reticulum);
        println!("oag: reticulum enabled, destination address = {}", address_hash.to_hex_string());
        tokio::spawn(oag_reticulum::run_listener(pool_for_reticulum, identity_for_reticulum, reticulum_config));
    }

    let embedding_provider: Arc<dyn oag_embeddings::EmbeddingProvider> = Arc::from(config.embedding_provider);
    // `[ipfs].libp2p_ws_multiaddr` (an operator override, e.g. for NAT/a
    // different public hostname) wins if set; otherwise, when
    // `pinning_provider = "embedded"`, ask the provider itself for its real,
    // just-bound dial address -- no separate daemon or manual config needed
    // for the common case. Every other provider (`disabled`/`kubo`) answers
    // this with an empty list, same as always.
    let ipfs_dial_multiaddr = match &config.ipfs_libp2p_ws_multiaddr {
        Some(configured) => Some(configured.clone()),
        None => config.pinning_provider.dialable_multiaddrs().await.into_iter().next(),
    };
    let pinning_provider = config.pinning_provider.clone();
    let crawler = Arc::new(oag_crawler::CrawlerService::new(
        graph.clone(),
        config.crawler_config,
        config.llm_extractor,
        config.pinning_provider,
    ));

    let state = oag_api::AppState::new(graph.clone())
        .with_embedding_provider(embedding_provider.clone())
        .with_crawler(crawler.clone())
        .with_ipfs_libp2p_ws_multiaddr(ipfs_dial_multiaddr.clone().map(Into::into))
        .with_pinning_provider(Some(pinning_provider));
    let rate_limit_state = state.clone();
    let rest_router = oag_api::build_router(state).merge(sync_router);
    let mcp_service =
        config.mcp_enabled.then(|| oag_mcp::streamable_http_service(graph.clone(), embedding_provider, crawler));
    let app = build_app(rest_router, mcp_service, rate_limit_state);

    println!("oag: peer_id = {peer_id}");
    if config.identity_ephemeral {
        println!("oag: identity: ephemeral (regenerates every restart)");
    } else {
        println!("oag: identity: permanent");
    }
    if let Some(cluster_name) = &config.cluster_name {
        println!("oag: cluster: {cluster_name}");
    }
    println!("oag: data dir = {}", config.data_dir.display());
    println!("oag: REST listening on http://{}/api/v1", config.listen);
    if config.mcp_enabled {
        println!("oag: MCP listening on http://{}/mcp", config.listen);
    }
    println!("oag: sync listening on http://{}/oag/sync/v1", config.listen);
    if let Some(multiaddr) = &ipfs_dial_multiaddr {
        println!("oag: ipfs listening on {multiaddr}");
    }
    if config.bootstrap_peers.is_empty() {
        println!("oag: no bootstrap_peers configured — replication is passive until a peer syncs with us, or `oag peer add` is used");
    } else {
        println!(
            "oag: gossiping with {} bootstrap peer(s) every {}s",
            config.bootstrap_peers.len(),
            config.sync_interval.as_secs()
        );
    }

    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(sync_service, config.identity_ephemeral))
        .await?;
    Ok(())
}

/// Fixed, deliberately *not* itself a config value (spec/24 Phase 2) -- a
/// hot-reloadable "how often do we check for changes to the check-interval"
/// would be a pointless regress. 30s matches `default_heartbeat_interval_
/// seconds`'s own default and is far more responsive than this setting's
/// own effect (trust decay/rebuild curves, which operate on minutes) needs.
const TRUST_CONFIG_RELOAD_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

/// Ephemeral peer trust, Phase 2 (spec/24): periodically re-reads
/// `[identity]` from `config_path` and pushes any change into
/// `sync_service` via `update_trust_config`. Unconditional overwrite every
/// tick -- the simplest correct behavior, since re-applying an unchanged
/// value is a no-op; a parse failure is swallowed (the config file being
/// transiently invalid mid-edit shouldn't crash a running peer) and simply
/// retried next tick.
async fn reload_trust_config_periodically(
    sync_service: oag_sync::SyncService,
    config_path: std::path::PathBuf,
    interval: std::time::Duration,
) {
    loop {
        tokio::time::sleep(interval).await;
        if let Ok(section) = crate::config::resolve_identity_section(Some(config_path.clone())) {
            sync_service.update_trust_config(section.to_trust_config());
        }
    }
}

/// Waits for Ctrl-C or SIGTERM, then — in ephemeral mode only — best-effort
/// broadcasts a signed `GoingOffline` heartbeat to every known peer before
/// `axum::serve` actually stops (spec/24: "an announced restart should be
/// treated more gently than an unannounced disappearance,"
/// `USER_INPUT_RECORD.md` Entry 4). Never blocks shutdown waiting on a dead
/// peer -- `broadcast_presence` already treats each address independently.
async fn shutdown_signal(sync_service: oag_sync::SyncService, ephemeral: bool) {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        let Ok(mut signal) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) else {
            return;
        };
        signal.recv().await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    if ephemeral {
        println!("oag: shutting down -- announcing planned downtime to known peers");
        let sent = sync_service.broadcast_presence(oag_sync::PresenceStatus::GoingOffline).await;
        println!("oag: announced to {sent} known peer address(es)");
    }
}

/// Mounts `/mcp` (if `mcp_service` is `Some`) onto `rest_router`, with its
/// own `rate_limit_middleware` layer applied directly to a small router
/// containing only that one route.
///
/// This is its own function, separate from `run`, specifically so it's
/// unit-testable without a real server/TCP listener: a router built, then
/// `.route_service`'d onto *after* merging, would silently miss whatever
/// layers `rest_router` already carries from its own construction (its
/// `rate_limit_middleware`/body-limit layers only ever applied to the
/// routes that existed when they were added) -- confirmed live, a key that
/// gets 429'd on REST after 120 requests/60s previously sailed through
/// 130+ consecutive `/mcp` calls with no rate limiting at all. Giving
/// `/mcp` its own small router with its own layer, merged in afterward,
/// sidesteps that ordering question entirely rather than relying on it.
fn build_app<S>(rest_router: axum::Router, mcp_service: Option<S>, rate_limit_state: oag_api::AppState) -> axum::Router
where
    S: tower::Service<axum::extract::Request, Error = std::convert::Infallible> + Clone + Send + Sync + 'static,
    S::Response: axum::response::IntoResponse,
    S::Future: Send + 'static,
{
    match mcp_service {
        Some(mcp_service) => {
            let mcp_router = axum::Router::new().route_service("/mcp", mcp_service).layer(
                axum::middleware::from_fn_with_state(rate_limit_state, oag_api::rate_limit::rate_limit_middleware),
            );
            rest_router.merge(mcp_router)
        }
        None => rest_router,
    }
}

async fn bootstrap_admin(graph: &GraphService) -> anyhow::Result<()> {
    let actor_id = graph
        .declare_actor(ActorType::Service, Some("admin".to_string()), None, None)
        .await?;
    let raw_key = graph.create_key(actor_id, vec![Permission::Admin]).await?;

    println!();
    println!("=================================================================");
    println!(" First run: created bootstrap admin API key.");
    println!(" This is shown ONCE — only its hash is stored. Save it now:");
    println!();
    println!("   {raw_key}");
    println!();
    println!(" Use it as: Authorization: Bearer {raw_key}");
    println!("=================================================================");
    println!();
    Ok(())
}

#[cfg(test)]
mod tests {
    use oag_core::{ActorType, Permission};

    use super::*;

    async fn spawn_test_app(name: &str) -> (String, String) {
        let dir = std::env::temp_dir().join(format!(
            "oag-cli-serve-test-{name}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = oag_storage::open_pool(&dir.join("oag.sqlite")).await.unwrap();
        let identity = PeerIdentity::generate();
        let graph = Arc::new(GraphService::new(pool, identity));

        let actor_id =
            graph.declare_actor(ActorType::Agent, Some("serve-test".into()), None, None).await.unwrap();
        let raw_key = graph.create_key(actor_id, vec![Permission::GraphRead]).await.unwrap();

        let embedding_provider: Arc<dyn oag_embeddings::EmbeddingProvider> = Arc::new(oag_embeddings::DisabledProvider);
        let crawler = Arc::new(oag_crawler::CrawlerService::new(
            graph.clone(),
            oag_crawler::CrawlerConfig::default(),
            Arc::new(oag_crawler::DisabledExtractor),
            Arc::new(oag_ipfs::DisabledProvider),
        ));
        let state = oag_api::AppState::new(graph.clone());
        let rate_limit_state = state.clone();
        let rest_router = oag_api::build_router(state);
        let mcp_service = Some(oag_mcp::streamable_http_service(graph, embedding_provider, crawler));
        let app = build_app(rest_router, mcp_service, rate_limit_state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{addr}"), raw_key)
    }

    /// Regression test for the exact gap found and fixed live: `/mcp`,
    /// mounted via `route_service` after `rest_router` already had its own
    /// `rate_limit_middleware` layer applied, silently never got that
    /// protection -- a key that gets 429'd on REST after 120 requests/60s
    /// could otherwise hammer `/mcp` without limit. `build_app` fixes this
    /// by giving `/mcp` its own layer on its own small router; this proves
    /// that actually holds over real HTTP, not just REST's own endpoints.
    #[tokio::test]
    async fn mcp_endpoint_is_rate_limited_same_as_rest() {
        let (base, key) = spawn_test_app("mcp-rate-limit").await;
        let client = reqwest::Client::new();

        let mut saw_429 = false;
        for i in 0..130 {
            let response = client
                .post(format!("{base}/mcp"))
                .header("authorization", format!("Bearer {key}"))
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .body(format!(r#"{{"jsonrpc":"2.0","id":{i},"method":"tools/list"}}"#))
                .send()
                .await
                .unwrap();
            if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                saw_429 = true;
                break;
            }
        }
        assert!(saw_429, "expected /mcp to start returning 429 well within 130 requests, same as REST does");
    }
}
