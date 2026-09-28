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
}
