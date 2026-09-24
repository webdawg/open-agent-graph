use std::sync::Arc;

use oag_core::{ActorType, AssertionStatus, Permission};
use oag_crypto::PeerIdentity;
use oag_graph::{AssertInput, AuthContext, GraphService};
use oag_storage::pool::open_pool;

use crate::federation::FederationPolicy;
use crate::server::router;
use crate::service::SyncService;

struct TestPeer {
    graph: Arc<GraphService>,
    sync: SyncService,
    auth: AuthContext,
    addr: String,
}

async fn spawn_peer(name: &str) -> TestPeer {
    let dir = std::env::temp_dir().join(format!(
        "oag-sync-test-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let pool = open_pool(&dir.join("oag.sqlite")).await.unwrap();

    let identity = PeerIdentity::generate();
    let peer_id = identity.peer_id();
    let public_key = identity.verifying_key().to_bytes();

    let graph = Arc::new(GraphService::new(pool.clone(), identity));
    let actor_id = graph
        .declare_actor(ActorType::Service, Some(format!("{name}-actor")), None)
        .await
        .unwrap();
    let auth = AuthContext {
        actor_id,
        permissions: vec![Permission::GraphAssert],
    };

    let sync = SyncService::new(pool, peer_id, public_key, FederationPolicy::Open);

    let app = router(sync.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    TestPeer {
        graph,
        sync,
        auth,
        addr: format!("http://{addr}"),
    }
}

fn sample_assertion(subject: &str, object: &str) -> AssertInput {
    AssertInput {
        subject: subject.to_string(),
        subject_type: Some("repository".into()),
        predicate: "implements".into(),
        object: object.to_string(),
        object_type: None,
        evidence: vec![],
        actor_confidence: Some(0.9),
        observed_at: None,
        extraction_method: None,
    }
}

/// Spec section 97/109 — an assertion created on A must appear on B after B
/// syncs with A, without ever touching B directly.
#[tokio::test]
async fn two_peer_replication() {
    let a = spawn_peer("two-peer-a").await;
    let b = spawn_peer("two-peer-b").await;

    let assertion_id = a
        .graph
        .assert(&a.auth, sample_assertion("https://github.com/example/foo", "Model Context Protocol"))
        .await
        .unwrap();

    let summary = b.sync.sync_with_peer(&a.addr).await.unwrap();
    assert!(summary.applied >= 1, "expected at least one applied event, got {summary:?}");
    assert!(summary.errors.is_empty(), "unexpected errors: {:?}", summary.errors);

    let fetched = b.graph.get_assertion(assertion_id).await.unwrap();
    assert!(fetched.is_some(), "B should have A's assertion after sync");
    assert_eq!(fetched.unwrap().status, AssertionStatus::Active);

    // Idempotent: syncing again should not re-apply or error. Once caught
    // up, the head comparison skips re-fetching the range entirely (spec
    // section 51's anti-entropy is head-driven, not a full resend).
    let second = b.sync.sync_with_peer(&a.addr).await.unwrap();
    assert_eq!(second.applied, 0);
    assert!(second.errors.is_empty());
}

/// Spec section 98 — A and B each write independently with no sync between
/// (simulated partition), then reconnect. Both must converge to the same
/// graph with no manual conflict repair.
#[tokio::test]
async fn partition_reconnect_converges() {
    let a = spawn_peer("partition-a").await;
    let b = spawn_peer("partition-b").await;

    let assertion_a = a
        .graph
        .assert(&a.auth, sample_assertion("https://example.com/a-repo", "concept-a"))
        .await
        .unwrap();
    let assertion_b = b
        .graph
        .assert(&b.auth, sample_assertion("https://example.com/b-repo", "concept-b"))
        .await
        .unwrap();

    // Reconnect: sync both directions.
    a.sync.sync_with_peer(&b.addr).await.unwrap();
    b.sync.sync_with_peer(&a.addr).await.unwrap();

    assert!(a.graph.get_assertion(assertion_b).await.unwrap().is_some(), "A should have B's assertion");
    assert!(b.graph.get_assertion(assertion_a).await.unwrap().is_some(), "B should have A's assertion");

    // Both still have their own.
    assert!(a.graph.get_assertion(assertion_a).await.unwrap().is_some());
    assert!(b.graph.get_assertion(assertion_b).await.unwrap().is_some());
}

/// Spec section 99 — a peer that only ever talks to a relay (B) can still
/// obtain a third peer's (A's) full signed history, proving there is no
/// origin-server dependency once at least one live peer holds the data.
#[tokio::test]
async fn relay_without_origin_dependency() {
    let a = spawn_peer("relay-a").await;
    let b = spawn_peer("relay-b").await;

    let assertion_id = a
        .graph
        .assert(&a.auth, sample_assertion("https://github.com/example/relayed", "concept-relay"))
        .await
        .unwrap();

    // B learns about A and pulls A's history.
    let summary = b.sync.sync_with_peer(&a.addr).await.unwrap();
    assert!(summary.applied >= 1);
    assert!(b.graph.get_assertion(assertion_id).await.unwrap().is_some());

    // A is never contacted again from this point on — C only ever talks to B.
    let c = spawn_peer("relay-c").await;
    let summary = c.sync.sync_with_peer(&b.addr).await.unwrap();
    assert!(summary.applied >= 1, "C should pull A's relayed history through B, got {summary:?}");
    assert!(summary.errors.is_empty(), "unexpected errors: {:?}", summary.errors);

    let fetched = c.graph.get_assertion(assertion_id).await.unwrap();
    assert!(fetched.is_some(), "C should have A's original assertion via relay through B");
    assert_eq!(fetched.unwrap().status, AssertionStatus::Active);
}

fn one_signed_event_json() -> serde_json::Value {
    let identity = PeerIdentity::generate();
    let (signed, _id) = oag_events::build_and_sign(
        &identity,
        1,
        None,
        1_700_000_000,
        oag_events::EventPayload::ActorDeclare(oag_events::ActorDeclarePayload {
            actor_type: "agent".into(),
            name: None,
            public_key: None,
            identity_uri: None,
        }),
    )
    .unwrap();
    serde_json::to_value(&signed).unwrap()
}

/// Spec section 61 ("event flooding"): the global sync rate limiter must
/// eventually reject a caller that keeps hammering the same endpoint.
#[tokio::test]
async fn rate_limiter_rejects_after_threshold() {
    let peer = spawn_peer("rate-limit").await;
    let client = reqwest::Client::new();
    let url = format!("{}/oag/sync/v1/hello", peer.addr);

    let mut saw_429 = false;
    for _ in 0..650 {
        let resp = client.get(&url).send().await.unwrap();
        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            saw_429 = true;
            break;
        }
    }
    assert!(saw_429, "expected a 429 within 650 requests against a 600/window limit");
}

/// Spec section 61 ("event flooding" / "signature spam"): a push batch over
/// the per-request event-count cap is rejected before any per-event work.
#[tokio::test]
async fn push_event_count_over_limit_is_rejected() {
    let peer = spawn_peer("push-count-limit").await;
    let event = one_signed_event_json();
    let events: Vec<serde_json::Value> = (0..1001).map(|_| event.clone()).collect();

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/oag/sync/v1/events", peer.addr))
        .json(&serde_json::json!({ "events": events }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
}
