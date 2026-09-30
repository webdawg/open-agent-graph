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
