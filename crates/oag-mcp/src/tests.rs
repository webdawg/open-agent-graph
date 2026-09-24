use std::sync::Arc;

use oag_core::{ActorType, Permission};
use oag_crypto::PeerIdentity;
use oag_graph::GraphService;
use oag_storage::pool::open_pool;
use rmcp::model::{CallToolRequestParams, ClientConfig};
use rmcp::transport::streamable_http_client::{
    StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
};
use rmcp::{ClientHandler, ServiceExt};
use serde_json::json;

#[derive(Debug, Clone, Default)]
struct DummyClientHandler;

impl ClientHandler for DummyClientHandler {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::default()
    }
}

fn temp_db_path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "oag-mcp-test-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("oag.sqlite")
}

/// Spins up the real Streamable HTTP MCP service on a real loopback TCP
/// socket and returns its base URL plus a bootstrap API key — this exercises
/// the exact code path a production `oag serve` process runs, including the
/// `Extension<http::request::Parts>` auth extraction that only works over a
/// real HTTP transport (not the in-memory duplex transport rmcp's own unit
/// tests use).
async fn spawn_mcp_server(name: &str) -> (String, String) {
    let pool = open_pool(&temp_db_path(name)).await.unwrap();
    let identity = PeerIdentity::generate();
    let graph = Arc::new(GraphService::new(pool, identity));

    let actor_id = graph
        .declare_actor(ActorType::Agent, Some("mcp-test".into()), None)
        .await
        .unwrap();
    let raw_key = graph
        .create_key(
            actor_id,
            vec![Permission::GraphRead, Permission::GraphAssert],
        )
        .await
        .unwrap();

    let app = axum::Router::new().route_service("/mcp", crate::streamable_http_service(graph));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (format!("http://{addr}/mcp"), raw_key)
}

#[tokio::test]
async fn graph_assert_and_get_subgraph_over_real_http() {
    let (url, raw_key) = spawn_mcp_server("assert-subgraph").await;

    let transport = StreamableHttpClientTransport::with_client(
        reqwest::Client::default(),
        StreamableHttpClientTransportConfig::with_uri(url).auth_header(raw_key),
    );
    let client = DummyClientHandler.serve(transport).await.unwrap();

    let assert_result = client
        .call_tool(CallToolRequestParams::new("graph_assert").with_arguments(
            json!({
                "subject": "https://github.com/example/foo",
                "subject_type": "repository",
                "predicate": "implements",
                "object": "Model Context Protocol",
                "evidence": [{
                    "type": "documentation",
                    "uri": "https://github.com/example/foo/blob/main/README.md"
                }],
                "confidence": 0.9
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();

    let structured = assert_result
        .structured_content
        .expect("graph_assert returns structured content");
    let assertion_id = structured["assertion_id"].as_str().unwrap().to_string();
    assert_eq!(structured["status"], "accepted_pending_verification");

    let resolve_result = client
        .call_tool(
            CallToolRequestParams::new("graph_resolve").with_arguments(
                json!({ "value": "https://github.com/example/foo" })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    let resolved = resolve_result.structured_content.unwrap();
    let node_id = resolved["node_id"].as_str().unwrap().to_string();
    assert_eq!(resolved["confidence"], 1.0);

    let subgraph_result = client
        .call_tool(
            CallToolRequestParams::new("graph_get_subgraph").with_arguments(
                json!({ "node_id": node_id, "depth": 2 })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    let subgraph = subgraph_result.structured_content.unwrap();
    assert_eq!(subgraph["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(subgraph["edges"].as_array().unwrap().len(), 1);
    assert_eq!(subgraph["assertions"].as_array().unwrap().len(), 1);

    let history_result = client
        .call_tool(
            CallToolRequestParams::new("graph_get_history").with_arguments(
                json!({ "object_type": "assertion", "id": assertion_id })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    let history = history_result.structured_content.unwrap();
    // ASSERT_RELATION + ADD_EVIDENCE
    assert_eq!(history["history"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn missing_auth_header_is_rejected_over_real_http() {
    let (url, _raw_key) = spawn_mcp_server("no-auth").await;

    let transport = StreamableHttpClientTransport::with_client(
        reqwest::Client::default(),
        StreamableHttpClientTransportConfig::with_uri(url),
    );
    let client = DummyClientHandler.serve(transport).await.unwrap();

    let result = client
        .call_tool(
            CallToolRequestParams::new("graph_search")
                .with_arguments(json!({ "query": "foo" }).as_object().unwrap().clone()),
        )
        .await;

    assert!(result.is_err(), "expected tool call without auth to fail");
}
