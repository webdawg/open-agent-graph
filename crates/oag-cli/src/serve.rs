use std::sync::Arc;

use oag_core::{ActorType, Permission};
use oag_crypto::PeerIdentity;
use oag_graph::GraphService;

use crate::config::ResolvedConfig;

/// `oag serve` (spec section 91's startup sequence, minus replication):
/// create/load identity -> open/migrate SQLite -> bootstrap admin key if
/// this is a fresh data dir -> start REST -> start MCP.
pub async fn run(config: ResolvedConfig) -> anyhow::Result<()> {
    std::fs::create_dir_all(&config.data_dir)?;

    let identity_path = config.data_dir.join("identity.key");
    let identity = PeerIdentity::load_or_generate(&identity_path)?;
    let peer_id = identity.peer_id();

    let db_path = config.data_dir.join("oag.sqlite");
    let pool = oag_storage::open_pool(&db_path).await?;

    let graph = Arc::new(GraphService::new(pool, identity));

    if !graph.has_any_actor().await? {
        bootstrap_admin(&graph).await?;
    }

    let state = oag_api::AppState::new(graph.clone());
    let rest_router = oag_api::build_router(state);

    let app = if config.mcp_enabled {
        rest_router.route_service("/mcp", oag_mcp::streamable_http_service(graph.clone()))
    } else {
        rest_router
    };

    println!("oag: peer_id = {peer_id}");
    println!("oag: data dir = {}", config.data_dir.display());
    println!("oag: REST listening on http://{}/api/v1", config.listen);
    if config.mcp_enabled {
        println!("oag: MCP listening on http://{}/mcp", config.listen);
    }

    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn bootstrap_admin(graph: &GraphService) -> anyhow::Result<()> {
    let actor_id = graph
        .declare_actor(ActorType::Service, Some("admin".to_string()), None)
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
