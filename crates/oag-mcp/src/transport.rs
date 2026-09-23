use std::sync::Arc;

use oag_graph::GraphService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};

use crate::server::OagMcpServer;

/// A `tower::Service` implementing MCP over Streamable HTTP (spec section
/// 68), ready to mount on the same Axum app as REST via
/// `router.route_service("/mcp", oag_mcp::streamable_http_service(graph))`.
/// `service_factory` clones the shared `Arc<GraphService>` per session, so
/// REST and MCP always read/write through the exact same service layer.
pub fn streamable_http_service(
    graph: Arc<GraphService>,
) -> StreamableHttpService<OagMcpServer, LocalSessionManager> {
    StreamableHttpService::new(
        move || Ok(OagMcpServer::new(graph.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    )
}
