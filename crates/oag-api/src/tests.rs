use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use oag_core::{ActorType, Permission};
use oag_crypto::PeerIdentity;
use oag_graph::GraphService;
use oag_storage::pool::open_pool;
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::state::AppState;
use crate::build_router;

fn temp_db_path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "oag-api-test-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("oag.sqlite")
}

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn test_app(name: &str) -> (axum::Router, String) {
    let pool = open_pool(&temp_db_path(name)).await.unwrap();
    let identity = PeerIdentity::generate();
    let graph = Arc::new(GraphService::new(pool, identity));

    let actor_id = graph
        .declare_actor(ActorType::Agent, Some("admin".into()), None, None)
        .await
        .unwrap();
    let raw_key = graph
        .create_key(
            actor_id,
            vec![
                Permission::GraphRead,
                Permission::GraphAssert,
                Permission::GraphVerify,
                Permission::GraphRetractOwn,
            ],
        )
        .await
        .unwrap();

    let state = AppState::new(graph);
    (build_router(state), raw_key)
}

/// Like `test_app`, but with `graph:crawl` granted and a crawler config that
/// allows private networks -- needed to let the test hit its own loopback
/// fixture server, exactly like `oag-crawler`'s own tests do.
async fn test_app_with_crawler(name: &str) -> (axum::Router, String) {
    let pool = open_pool(&temp_db_path(name)).await.unwrap();
    let identity = PeerIdentity::generate();
    let graph = Arc::new(GraphService::new(pool, identity));

    let actor_id = graph
        .declare_actor(ActorType::Agent, Some("admin".into()), None, None)
        .await
        .unwrap();
    let raw_key = graph
        .create_key(
            actor_id,
            vec![Permission::GraphRead, Permission::GraphAssert, Permission::GraphCrawl],
        )
        .await
        .unwrap();

    let crawler_config = oag_crawler::CrawlerConfig { allow_private_networks: true, ..Default::default() };
    let crawler = Arc::new(oag_crawler::CrawlerService::new(
        graph.clone(),
        crawler_config,
        Arc::new(oag_crawler::DisabledExtractor),
        Arc::new(oag_ipfs::DisabledProvider),
    ));
    let state = AppState::new(graph).with_crawler(crawler);
    (build_router(state), raw_key)
}

#[tokio::test]
async fn full_rest_vertical_slice() {
    let (app, key) = test_app("vertical-slice").await;
    let auth_header = format!("Bearer {key}");

    let create_body = json!({
        "subject": "https://github.com/example/foo",
        "subject_type": "repository",
        "predicate": "implements",
        "object": "Model Context Protocol",
        "evidence": [{
            "type": "documentation",
            "uri": "https://github.com/example/foo/blob/main/README.md"
        }],
        "actor_confidence": 0.95
    });

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/assertions")
                .header("authorization", &auth_header)
                .header("content-type", "application/json")
                .body(Body::from(create_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    let assertion_id = body["assertion_id"].as_str().unwrap().to_string();

    // GET the assertion back.
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/assertions/{assertion_id}"))
                .header("authorization", &auth_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["assertion"]["status"], "active");
    assert_eq!(body["evidence"].as_array().unwrap().len(), 1);

    // resolve() finds the subject node.
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/resolve")
                .header("authorization", &auth_header)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "value": "https://github.com/example/foo" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    let node_id = body["node_id"].as_str().unwrap().to_string();
    assert_eq!(body["confidence"], 1.0);

    // get_node includes an (empty, for now) aliases list alongside the node.
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/nodes/{node_id}"))
                .header("authorization", &auth_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["node"]["id"], node_id);
    assert_eq!(body["aliases"].as_array().unwrap().len(), 0);

    // subgraph around that node.
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/subgraph?node={node_id}&depth=2"))
                .header("authorization", &auth_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(body["edges"].as_array().unwrap().len(), 1);
    let edge_id = body["edges"][0]["id"].as_str().unwrap().to_string();

    // corroboration for that edge: one assertion, one evidence-backed source
    // group, no disputes/observations yet.
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/edges/{edge_id}/corroboration"))
                .header("authorization", &auth_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["active_assertions"], 1);
    assert_eq!(body["source_groups"].as_array().unwrap().len(), 1);
    assert_eq!(body["agreement"], 1.0);
    assert!(body["freshness"].as_f64().unwrap() > 0.999);

    // verify the assertion; observations should now surface on GET assertion.
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/assertions/{assertion_id}/verify"))
                .header("authorization", &auth_header)
                .header("content-type", "application/json")
                .body(Body::from(json!({ "result": "confirmed" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // dispute then retract.
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/assertions/{assertion_id}/dispute"))
                .header("authorization", &auth_header)
                .header("content-type", "application/json")
                .body(Body::from(json!({ "reason": "outdated" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/assertions/{assertion_id}/retract"))
                .header("authorization", &auth_header)
                .header("content-type", "application/json")
                .body(Body::from(json!({}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // history shows the full lifecycle; original assertion still resolvable.
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/history/assertion/{assertion_id}"))
                .header("authorization", &auth_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["history"].as_array().unwrap().len(), 5); // assert, evidence, verify, dispute, retract

    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/assertions/{assertion_id}"))
                .header("authorization", &auth_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = body_json(response).await;
    assert_eq!(body["assertion"]["status"], "retracted");
    assert_eq!(body["observations"].as_array().unwrap().len(), 1);
    assert_eq!(body["observations"][0]["result"], "confirmed");
    assert_eq!(body["disputes"].as_array().unwrap().len(), 1);
    assert_eq!(body["disputes"][0]["reason"], "outdated");
    assert_eq!(body["retractions"].as_array().unwrap().len(), 1);
    assert_eq!(body["retractions"][0]["reason"], serde_json::Value::Null, "retracted with no reason given");
}

#[tokio::test]
async fn missing_auth_header_is_rejected() {
    let (app, _key) = test_app("no-auth").await;
    let response = app
        .oneshot(
            Request::get("/api/v1/search?q=test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn oversized_request_body_is_rejected() {
    let (app, key) = test_app("oversized-body").await;
    let auth_header = format!("Bearer {key}");

    // A single evidence excerpt field well past both the per-field
    // (oag-graph) and whole-body (4 MiB, this layer) limits.
    let huge = "x".repeat(6 * 1024 * 1024);
    let body = json!({
        "subject": "https://example.com/oversized",
        "predicate": "implements",
        "object": "concept:oversized",
        "evidence": [{"excerpt": huge}]
    });

    let response = app
        .oneshot(
            Request::post("/api/v1/assertions")
                .header("authorization", &auth_header)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn status_endpoint_needs_no_auth() {
    let (app, _key) = test_app("status").await;
    let response = app
        .oneshot(Request::get("/api/v1/status").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert!(body["peer_id"].as_str().unwrap().starts_with("oagp_"));
}

#[tokio::test]
async fn metrics_endpoint_needs_no_auth_and_reflects_seeded_data() {
    let (app, key) = test_app("metrics").await;
    let auth_header = format!("Bearer {key}");

    // Seed one assertion (with one piece of evidence) so the counters are
    // exercising real rows, not just zeros.
    let create_body = json!({
        "subject": "https://example.com/metrics-target",
        "predicate": "implements",
        "object": "concept:metrics-test",
        "evidence": [{"type": "documentation", "uri": "https://example.com/docs"}]
    });
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/assertions")
                .header("authorization", &auth_header)
                .header("content-type", "application/json")
                .body(Body::from(create_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // No Authorization header at all -- /metrics must still work.
    let response = app
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/plain; version=0.0.4"
    );

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(bytes.to_vec()).unwrap();

    assert!(text.contains("# TYPE nodes_total counter"));
    assert!(text.contains("# TYPE replication_target gauge"), "expected live replication gauges:\n{text}");
    assert!(text.contains("# TYPE sqlite_size_bytes gauge"));

    let value_of = |metric: &str| -> i64 {
        text.lines()
            .find(|line| line.starts_with(&format!("{metric} ")))
            .unwrap_or_else(|| panic!("missing value line for {metric} in:\n{text}"))
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap()
    };

    // One assertion created two nodes (subject + object) and one edge.
    assert_eq!(value_of("nodes_total"), 2);
    assert_eq!(value_of("edges_total"), 1);
    assert_eq!(value_of("assertions_total"), 1);
    assert_eq!(value_of("evidence_total"), 1);
    assert!(value_of("events_total") > 0);
    assert!(value_of("sqlite_size_bytes") > 0);
    // No crawl was triggered in this test.
    assert_eq!(value_of("crawl_jobs"), 0);
    assert_eq!(value_of("crawl_failures"), 0);
}

async fn body_text(response: axum::response::Response) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// Spec section 80: the Human Interface reuses the same `graph:read` API
/// keys as REST, accepted via `?key=` (a plain link can't set a header) --
/// missing or wrong key must be rejected exactly like a REST call would be.
#[tokio::test]
async fn human_interface_pages_require_the_same_api_key_as_rest() {
    let (app, key) = test_app("human-auth").await;
    let auth_header = format!("Bearer {key}");

    let create_body = json!({
        "subject": "https://example.com/human-ui-test",
        "predicate": "instance_of",
        "object": "concept:human-ui-test",
    });
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/assertions")
                .header("authorization", &auth_header)
                .header("content-type", "application/json")
                .body(Body::from(create_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let assertion_id = body_json(response).await["assertion_id"].as_str().unwrap().to_string();

    let resolve_response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/resolve")
                .header("authorization", &auth_header)
                .header("content-type", "application/json")
                .body(Body::from(json!({ "value": "https://example.com/human-ui-test" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let node_id = body_json(resolve_response).await["node_id"].as_str().unwrap().to_string();

    // No key at all.
    let response = app
        .clone()
        .oneshot(Request::get(format!("/ui/nodes/{node_id}")).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Wrong key.
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/ui/nodes/{node_id}?key=oagk_wrong"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Correct key.
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/ui/nodes/{node_id}?key={key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .oneshot(
            Request::get(format!("/ui/assertions/{assertion_id}?key={key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

/// The core security property of the human interface: fields containing
/// attacker/crawler-controlled text (here, an evidence title and excerpt)
/// must render HTML-escaped, never as live markup -- proves askama's
/// default autoescaping is actually active on these templates, not just
/// assumed.
#[tokio::test]
async fn human_interface_escapes_hostile_content() {
    let (app, key) = test_app("human-xss").await;
    let auth_header = format!("Bearer {key}");

    let create_body = json!({
        "subject": "https://example.com/xss-test",
        "predicate": "instance_of",
        "object": "concept:xss-test",
        "evidence": [{
            "type": "documentation",
            "title": "<script>alert(1)</script>",
            "excerpt": "<img src=x onerror=alert(2)>"
        }]
    });
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/assertions")
                .header("authorization", &auth_header)
                .header("content-type", "application/json")
                .body(Body::from(create_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let assertion_id = body_json(response).await["assertion_id"].as_str().unwrap().to_string();

    let response = app
        .oneshot(
            Request::get(format!("/ui/assertions/{assertion_id}?key={key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_text(response).await;

    assert!(!html.contains("<script>"), "raw <script> tag leaked into rendered HTML:\n{html}");
    assert!(!html.contains("<img src=x"), "raw <img> tag leaked into rendered HTML:\n{html}");
    // askama's default HTML escaper uses numeric character references, not
    // named ones (e.g. `&#60;` not `&lt;`) -- confirmed against its own
    // documented example output.
    assert!(html.contains("&#60;script&#62;"), "expected the escaped script tag to be present:\n{html}");
    assert!(html.contains("&#60;img src=x"), "expected the escaped img tag to be present:\n{html}");
}

#[tokio::test]
async fn human_search_page_finds_nodes_and_links_to_them() {
    let (app, key) = test_app("human-search").await;
    let auth_header = format!("Bearer {key}");

    let create_body = json!({
        "subject": "https://example.com/searchable-thing",
        "predicate": "instance_of",
        "object": "concept:searchable-thing",
    });
    app.clone()
        .oneshot(
            Request::post("/api/v1/assertions")
                .header("authorization", &auth_header)
                .header("content-type", "application/json")
                .body(Body::from(create_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    // No query yet -- just the search form, no results section.
    let response = app
        .clone()
        .oneshot(Request::get(format!("/ui/search?key={key}")).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_text(response).await;
    assert!(!html.contains("No matches"), "empty query shouldn't run a search at all:\n{html}");

    // A real query finds the node and links to its /ui/nodes/{id} page.
    let response = app
        .oneshot(
            Request::get(format!("/ui/search?key={key}&q=searchable-thing"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_text(response).await;
    assert!(html.contains("concept:searchable-thing"), "expected the match to appear:\n{html}");
    assert!(html.contains("/ui/nodes/"), "expected a link into the node page:\n{html}");
}

/// Spec section 71 exposed over REST: a caller with graph:crawl can trigger
/// this peer's own crawler against a real fixture server, and the crawled
/// facts land in the graph exactly like a CLI `oag crawl` would produce.
#[tokio::test]
async fn crawl_endpoint_requires_graph_crawl_permission_and_populates_the_graph() {
    let fixture = axum::Router::new().route(
        "/",
        axum::routing::get(|| async {
            (
                [(axum::http::header::CONTENT_TYPE, "text/html")],
                "<html><head><title>REST Crawl Fixture</title></head><body>hi</body></html>",
            )
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fixture_addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, fixture).await.unwrap();
    });
    let fixture_url = format!("http://{fixture_addr}/");

    // A key with graph:assert but not graph:crawl is rejected.
    let (app_no_crawl, key_no_crawl) = test_app("crawl-no-permission").await;
    let response = app_no_crawl
        .oneshot(
            Request::post("/api/v1/crawl")
                .header("authorization", format!("Bearer {key_no_crawl}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({ "url": fixture_url }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // A key with graph:crawl succeeds and the crawled facts are queryable.
    let (app, key) = test_app_with_crawler("crawl-with-permission").await;
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/crawl")
                .header("authorization", format!("Bearer {key}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({ "url": fixture_url }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert!(body["facts_asserted"].as_u64().unwrap() > 0);

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/resolve")
                .header("authorization", format!("Bearer {key}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({ "value": fixture_url }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["confidence"], 1.0);

    // The one successful crawl above is reflected in /metrics.
    let response = app.oneshot(Request::get("/metrics").body(Body::empty()).unwrap()).await.unwrap();
    let text = body_text(response).await;
    assert!(text.contains("crawl_jobs 1"), "expected crawl_jobs 1 in:\n{text}");
    assert!(text.contains("crawl_failures 0"), "expected crawl_failures 0 in:\n{text}");
}

#[tokio::test]
async fn get_actor_endpoint_returns_the_asserting_actor() {
    let (app, key) = test_app("get-actor").await;
    let auth_header = format!("Bearer {key}");

    let create_body = json!({
        "subject": "https://example.com/actor-lookup-test",
        "predicate": "instance_of",
        "object": "concept:actor-lookup-test",
    });
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/assertions")
                .header("authorization", &auth_header)
                .header("content-type", "application/json")
                .body(Body::from(create_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let assertion_id = body_json(response).await["assertion_id"].as_str().unwrap().to_string();

    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/assertions/{assertion_id}"))
                .header("authorization", &auth_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let actor_id = body_json(response).await["assertion"]["actor_id"].as_str().unwrap().to_string();

    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/actors/{actor_id}"))
                .header("authorization", &auth_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["actor"]["name"], "admin");

    // Unknown actor id -> 404, not a panic.
    let response = app
        .oneshot(
            Request::get(format!("/api/v1/actors/{}", "0".repeat(64)))
                .header("authorization", &auth_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_edge_endpoint_returns_the_edge() {
    let (app, key) = test_app("get-edge").await;
    let auth_header = format!("Bearer {key}");

    let create_body = json!({
        "subject": "https://example.com/edge-lookup-test",
        "predicate": "implements",
        "object": "concept:edge-lookup-test",
    });
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/assertions")
                .header("authorization", &auth_header)
                .header("content-type", "application/json")
                .body(Body::from(create_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let assertion_id = body_json(response).await["assertion_id"].as_str().unwrap().to_string();

    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/assertions/{assertion_id}"))
                .header("authorization", &auth_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let edge_id = body_json(response).await["assertion"]["edge_id"].as_str().unwrap().to_string();

    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/edges/{edge_id}"))
                .header("authorization", &auth_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["edge"]["predicate"], "implements");

    let response = app
        .oneshot(
            Request::get(format!("/api/v1/edges/{}", "0".repeat(64)))
                .header("authorization", &auth_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// Spec section 36 (Supersession): GraphService::supersede_assertion
/// already existed but had no REST/MCP/CLI surface at all until now --
/// the old assertion must remain inspectable, just flagged, exactly like
/// dispute/retract never delete anything.
#[tokio::test]
async fn supersede_endpoint_flags_the_old_assertion_without_deleting_it() {
    let (app, key) = test_app("supersede").await;
    let auth_header = format!("Bearer {key}");

    async fn create_assertion(app: axum::Router, auth_header: &str, object: &str) -> String {
        let body = json!({
            "subject": "https://example.com/supersede-test",
            "predicate": "instance_of",
            "object": object,
        });
        let response = app
            .oneshot(
                Request::post("/api/v1/assertions")
                    .header("authorization", auth_header)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        body_json(response).await["assertion_id"].as_str().unwrap().to_string()
    }

    let old_id = create_assertion(app.clone(), &auth_header, "concept:supersede-test-v1").await;
    let new_id = create_assertion(app.clone(), &auth_header, "concept:supersede-test-v2").await;

    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/assertions/{old_id}/supersede"))
                .header("authorization", &auth_header)
                .header("content-type", "application/json")
                .body(Body::from(json!({ "new_assertion_id": new_id }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .oneshot(
            Request::get(format!("/api/v1/assertions/{old_id}"))
                .header("authorization", &auth_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = body_json(response).await;
    assert_eq!(body["assertion"]["status"], "superseded", "old assertion must be flagged, still fully present");
}

#[tokio::test]
async fn get_node_sources_returns_evidence_from_touching_edges() {
    let (app, key) = test_app("node-sources").await;
    let auth_header = format!("Bearer {key}");

    let create_body = json!({
        "subject": "https://example.com/sources-test",
        "predicate": "implements",
        "object": "concept:sources-test",
        "evidence": [{"type": "documentation", "uri": "https://example.com/sources-docs"}]
    });
    app.clone()
        .oneshot(
            Request::post("/api/v1/assertions")
                .header("authorization", &auth_header)
                .header("content-type", "application/json")
                .body(Body::from(create_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    let resolve_response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/resolve")
                .header("authorization", &auth_header)
                .header("content-type", "application/json")
                .body(Body::from(json!({ "value": "https://example.com/sources-test" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let node_id = body_json(resolve_response).await["node_id"].as_str().unwrap().to_string();

    let response = app
        .oneshot(
            Request::get(format!("/api/v1/nodes/{node_id}/sources"))
                .header("authorization", &auth_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    let sources = body["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0]["uri"], "https://example.com/sources-docs");
}

/// `spec/31-service-receipts.md`: a real two-peer exchange over
/// `POST /oag/transfer/v1/fetch` -- a real `EmbeddedIpfsProvider` backing
/// the "server" peer's content, a real signed request from an independent
/// "client" peer's own identity/database, and independent verification +
/// graph writes on both sides, modeled on
/// `crates/oag-reticulum/tests/two_peer_sync.rs`'s real-two-peer style.
mod transfer_tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use oag_core::{ActorType, ExtractionMethod};
    use oag_crypto::PeerIdentity;
    use oag_graph::GraphService;
    use oag_ipfs::PinningProvider;
    use tower::ServiceExt;

    use crate::state::AppState;
    use crate::transfer::{build_and_sign_request, now_unix, record_transfer, verify_receipt, TransferReceipt};

    use super::temp_db_path;

    #[tokio::test]
    async fn server_and_client_end_up_with_matching_corroborating_records() {
        // -- Server peer: a real embedded IPFS node with real pinned content.
        let server_pool = oag_storage::open_pool(&temp_db_path("transfer-server")).await.unwrap();
        let server_identity = PeerIdentity::generate();
        let server_graph = std::sync::Arc::new(GraphService::new(server_pool, server_identity.clone()));

        let ipfs_dir = std::env::temp_dir().join(format!(
            "oag-api-transfer-test-ipfs-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&ipfs_dir).unwrap();
        let provider = oag_ipfs::EmbeddedIpfsProvider::new(ipfs_dir, vec![]).await.unwrap();
        let content = b"hello from the server peer".to_vec();
        let cid = provider.pin(&content).await.unwrap();

        let state = AppState::new(server_graph.clone()).with_pinning_provider(Some(std::sync::Arc::new(provider)));
        let app = crate::build_router(state);

        // -- Client peer: a completely separate identity and database.
        let client_pool = oag_storage::open_pool(&temp_db_path("transfer-client")).await.unwrap();
        let client_identity = PeerIdentity::generate();
        let client_graph = GraphService::new(client_pool, client_identity.clone());

        let client_nonce = oag_crypto::random_bytes_32();
        let request = build_and_sign_request(&client_identity, &cid, &client_nonce, now_unix(), 0, &[]);

        let response = app
            .oneshot(
                Request::post("/oag/transfer/v1/fetch")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&request).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let receipt_header = response
            .headers()
            .get("x-oag-transfer-receipt")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let receipt: TransferReceipt = serde_json::from_str(&receipt_header).unwrap();
        let received_bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(received_bytes.as_ref(), content.as_slice());

        let content_hash = oag_crypto::blake3_content_hash(&received_bytes);
        assert_eq!(content_hash, receipt.unsigned.content_hash);

        let verified_server_peer_id = verify_receipt(&receipt, &cid, &client_nonce, now_unix()).unwrap();
        assert_eq!(verified_server_peer_id, server_identity.peer_id());

        // Client records its own corroborating assertion, independently.
        let client_assertion_id = record_transfer(
            &client_graph,
            &client_identity,
            &verified_server_peer_id,
            &client_identity.peer_id(),
            &cid,
            received_bytes.len(),
            &receipt.unsigned.client_nonce,
            &receipt.unsigned.server_nonce,
            &content_hash,
            &receipt.unsigned.receipt_hash,
            "client",
            receipt.unsigned.served_at,
            ExtractionMethod::Verification,
            None,
        )
        .await
        .unwrap();

        // Both sides' local databases now carry the same edge/receipt_hash,
        // from each one's own perspective -- the "mutual" property: a third
        // party seeing only one side's record couldn't have told whether it
        // was fabricated, but having both (as oag-sync gossip would later
        // deliver) shows they agree on a hash neither could compute alone.
        let client_assertion = client_graph.get_assertion(client_assertion_id).await.unwrap().unwrap();
        assert_eq!(client_assertion.extraction_method, ExtractionMethod::Verification);
        let client_evidence = client_graph.list_evidence(client_assertion_id).await.unwrap();
        assert_eq!(client_evidence.len(), 1);
        assert_eq!(
            client_evidence[0].metadata["receipt_hash"],
            receipt.unsigned.receipt_hash
        );
        assert_eq!(client_evidence[0].metadata["peer_role"], "client");

        // The server's own record, from its perspective, on the exact same
        // edge (same subject/predicate/object -> same deterministic EdgeId).
        let edge = server_graph.get_edge(client_assertion.edge_id).await.unwrap().unwrap();
        assert_eq!(edge.predicate.as_str(), "sent_to");
        let server_assertions = server_graph.list_assertions_for_node(edge.subject).await.unwrap();
        assert_eq!(server_assertions.len(), 1);
        let server_assertion_id = server_assertions[0].id;
        assert_eq!(server_assertions[0].extraction_method, ExtractionMethod::Direct);
        let server_evidence = server_graph.list_evidence(server_assertion_id).await.unwrap();
        assert_eq!(server_evidence.len(), 1);
        assert_eq!(
            server_evidence[0].metadata["receipt_hash"],
            receipt.unsigned.receipt_hash
        );
        assert_eq!(server_evidence[0].metadata["peer_role"], "server");
        assert_eq!(server_evidence[0].content_hash.as_deref(), Some(content_hash.as_str()));

        // Both are actors of the first real `ActorType::Peer` kind.
        let server_actor = server_graph.get_actor(server_assertions[0].actor_id).await.unwrap().unwrap();
        assert_eq!(server_actor.actor_type, ActorType::Peer);
    }

    #[tokio::test]
    async fn disabled_provider_answers_service_unavailable() {
        let pool = oag_storage::open_pool(&temp_db_path("transfer-disabled")).await.unwrap();
        let identity = PeerIdentity::generate();
        let graph = std::sync::Arc::new(GraphService::new(pool, identity));
        let state = AppState::new(graph); // default: no pinning_provider configured
        let app = crate::build_router(state);

        let client_identity = PeerIdentity::generate();
        let client_nonce = oag_crypto::random_bytes_32();
        let request = build_and_sign_request(&client_identity, "bafyirrelevantcid", &client_nonce, now_unix(), 0, &[]);

        let response = app
            .oneshot(
                Request::post("/oag/transfer/v1/fetch")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&request).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn tampered_request_signature_is_rejected_before_any_fetch_or_graph_write() {
        let pool = oag_storage::open_pool(&temp_db_path("transfer-tampered")).await.unwrap();
        let identity = PeerIdentity::generate();
        let graph = std::sync::Arc::new(GraphService::new(pool, identity));
        let state = AppState::new(graph);
        let app = crate::build_router(state);

        let client_identity = PeerIdentity::generate();
        let client_nonce = oag_crypto::random_bytes_32();
        let mut request = build_and_sign_request(&client_identity, "bafyirrelevantcid", &client_nonce, now_unix(), 0, &[]);
        request.unsigned.cid = "bafyswappedcid".to_string(); // tamper after signing

        let response = app
            .oneshot(
                Request::post("/oag/transfer/v1/fetch")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&request).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    /// Phase 2 (multi-hop relay, spec/31): a real three-party chain --
    /// origin peer C has the content pinned; relay peer B has nothing
    /// pinned locally but is configured with `relay_peers = [C]`; a client
    /// fetches from B (never having heard of C) and still gets the correct,
    /// verified content, because B relayed the cache miss to C on its
    /// behalf. Both B's and C's real axum servers are bound to real TCP
    /// listeners (relaying does a real `reqwest` round trip, not an
    /// in-process `oneshot`), matching this crate's live-verification norm.
    #[tokio::test]
    async fn relay_peer_forwards_a_cache_miss_to_its_own_configured_upstream() {
        // -- Origin peer C: has the content, a real embedded IPFS node.
        let origin_pool = oag_storage::open_pool(&temp_db_path("transfer-relay-origin")).await.unwrap();
        let origin_identity = PeerIdentity::generate();
        let origin_graph = std::sync::Arc::new(GraphService::new(origin_pool, origin_identity.clone()));

        let origin_ipfs_dir = std::env::temp_dir().join(format!(
            "oag-api-transfer-relay-origin-ipfs-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&origin_ipfs_dir).unwrap();
        let origin_provider = oag_ipfs::EmbeddedIpfsProvider::new(origin_ipfs_dir, vec![]).await.unwrap();
        let content = b"hello from the far side of a relay chain".to_vec();
        let cid = origin_provider.pin(&content).await.unwrap();

        let origin_state =
            AppState::new(origin_graph.clone()).with_pinning_provider(Some(std::sync::Arc::new(origin_provider)));
        let origin_app = crate::build_router(origin_state);
        let origin_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin_addr = origin_listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(origin_listener, origin_app).await.unwrap();
        });
        let origin_address = format!("http://{origin_addr}");

        // -- Relay peer B: a disabled... no, an *enabled but empty*
        // pinning provider (so its own local fetch genuinely misses),
        // configured to relay to the origin above.
        let relay_pool = oag_storage::open_pool(&temp_db_path("transfer-relay-middle")).await.unwrap();
        let relay_identity = PeerIdentity::generate();
        let relay_graph = std::sync::Arc::new(GraphService::new(relay_pool, relay_identity.clone()));

        let relay_ipfs_dir = std::env::temp_dir().join(format!(
            "oag-api-transfer-relay-middle-ipfs-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&relay_ipfs_dir).unwrap();
        // B's own node, empty -- it never pinned `cid` itself, so fetching
        // it locally genuinely misses, same as a real cold cache would.
        let relay_provider = oag_ipfs::EmbeddedIpfsProvider::new(relay_ipfs_dir, vec![]).await.unwrap();

        let relay_state = AppState::new(relay_graph.clone())
            .with_pinning_provider(Some(std::sync::Arc::new(relay_provider)))
            .with_relay_peers(vec![crate::transfer::RelayPeer {
                address: origin_address.clone(),
                peer_id: origin_identity.peer_id(),
            }]);
        let relay_app = crate::build_router(relay_state);
        let relay_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let relay_addr = relay_listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(relay_listener, relay_app).await.unwrap();
        });
        let relay_address = format!("http://{relay_addr}");

        // -- Client: has never heard of the origin peer, only the relay.
        let client_pool = oag_storage::open_pool(&temp_db_path("transfer-relay-client")).await.unwrap();
        let client_identity = PeerIdentity::generate();
        let client_graph = GraphService::new(client_pool, client_identity.clone());
        let http = reqwest::Client::new();

        let verified = crate::transfer::fetch_and_verify(
            &http,
            &client_graph,
            &client_identity,
            &relay_address,
            &relay_identity.peer_id(),
            &cid,
            4, // plenty of hops for a 1-hop relay
            &[],
        )
        .await
        .unwrap();

        assert_eq!(verified.content, content);
        assert_eq!(verified.server_peer_id, relay_identity.peer_id());

        // The relay's own `peer:<id>` node now touches two edges: it served
        // the client (Direct) AND it verified a fetch from the origin
        // (Verification) -- distinguish the two records by extraction
        // method, each hop here is an independent, ordinary pairwise event.
        let relay_node = match relay_graph.resolve(&format!("peer:{}", relay_identity.peer_id())).await.unwrap() {
            oag_graph::ResolveOutcome::Found { node, .. } => node,
            other => panic!("expected relay's own peer node to exist, got {other:?}"),
        };
        let relay_node_assertions = relay_graph.list_assertions_for_node(relay_node.id).await.unwrap();
        assert_eq!(relay_node_assertions.len(), 2);

        // The relay's server-side record carries `relayed_via` pointing at
        // the origin -- this is what a later audit pass would follow to
        // reconstruct the chain, without any new signed multi-party
        // structure (just an ordinary metadata field on an ordinary
        // evidence row).
        let relay_as_server = relay_node_assertions
            .iter()
            .find(|a| a.extraction_method == ExtractionMethod::Direct)
            .expect("relay should have a Direct (server-role) assertion toward the client");
        let relay_server_evidence = relay_graph.list_evidence(relay_as_server.id).await.unwrap();
        assert_eq!(relay_server_evidence.len(), 1);
        assert_eq!(
            relay_server_evidence[0].metadata["relayed_via"],
            origin_identity.peer_id().to_string()
        );
        assert_eq!(relay_server_evidence[0].metadata["peer_role"], "server");

        // The relay ALSO recorded its own `Verification` assertion as the
        // *client* of the origin -- it independently verified the origin's
        // receipt before using the content, exactly like any other client.
        let relay_as_client = relay_node_assertions
            .iter()
            .find(|a| a.extraction_method == ExtractionMethod::Verification)
            .expect("relay should have a Verification (client-role) assertion toward the origin");
        assert_ne!(relay_as_server.id, relay_as_client.id);

        // The origin's own record has the relay (not the original client)
        // as its counterparty -- it never heard of the client at all.
        let origin_node = match origin_graph.resolve(&format!("peer:{}", origin_identity.peer_id())).await.unwrap() {
            oag_graph::ResolveOutcome::Found { node, .. } => node,
            other => panic!("expected origin's own peer node to exist, got {other:?}"),
        };
        let origin_assertions = origin_graph.list_assertions_for_node(origin_node.id).await.unwrap();
        assert_eq!(origin_assertions.len(), 1);
        let origin_evidence = origin_graph.list_evidence(origin_assertions[0].id).await.unwrap();
        assert_eq!(
            origin_evidence[0].metadata["counterparty_peer_id"],
            relay_identity.peer_id().to_string()
        );
    }

    /// `relay_hops_remaining: 0` (the default for an old/plain client that
    /// predates Phase 2) must behave exactly like Phase 1 always did: a
    /// local cache miss is a 404, never silently relayed, even when the
    /// server itself has `relay_peers` configured and reachable.
    #[tokio::test]
    async fn zero_hops_remaining_never_relays_even_with_relay_peers_configured() {
        // No origin server is actually started in this test -- the relay's
        // configured upstream address is deliberately never dialed, which
        // is exactly the point: `relay_hops_remaining: 0` must make the
        // relay give up on its own local cache miss before ever attempting
        // to contact anyone, so a bogus/unreachable upstream must not
        // matter. A throwaway identity stands in for "some origin peer" --
        // only its PeerId is used, to pin a syntactically valid CID.
        let origin_identity = PeerIdentity::generate();
        let origin_ipfs_dir = std::env::temp_dir().join(format!(
            "oag-api-transfer-relay-zerohop-origin-ipfs-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&origin_ipfs_dir).unwrap();
        let origin_provider = oag_ipfs::EmbeddedIpfsProvider::new(origin_ipfs_dir, vec![]).await.unwrap();
        let cid = origin_provider.pin(b"irrelevant content").await.unwrap();

        let relay_pool = oag_storage::open_pool(&temp_db_path("transfer-relay-zerohop-middle")).await.unwrap();
        let relay_identity = PeerIdentity::generate();
        let relay_graph = std::sync::Arc::new(GraphService::new(relay_pool, relay_identity.clone()));
        let relay_ipfs_dir = std::env::temp_dir().join(format!(
            "oag-api-transfer-relay-zerohop-middle-ipfs-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&relay_ipfs_dir).unwrap();
        let relay_provider = oag_ipfs::EmbeddedIpfsProvider::new(relay_ipfs_dir, vec![]).await.unwrap();
        let relay_state = AppState::new(relay_graph.clone())
            .with_pinning_provider(Some(std::sync::Arc::new(relay_provider)))
            .with_relay_peers(vec![crate::transfer::RelayPeer {
                address: format!("http://{}", "127.0.0.1:1"), // never actually dialed
                peer_id: origin_identity.peer_id(),
            }]);
        let relay_app = crate::build_router(relay_state);

        let client_identity = PeerIdentity::generate();
        let client_nonce = oag_crypto::random_bytes_32();
        let request = build_and_sign_request(&client_identity, &cid, &client_nonce, now_unix(), 0, &[]);

        let response = relay_app
            .oneshot(
                Request::post("/oag/transfer/v1/fetch")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&request).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
