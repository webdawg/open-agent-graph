//! End-to-end: a real event asserted on peer A reaches peer B purely over a
//! Reticulum TCP interface + Link, proving the transport actually works
//! without needing a public Reticulum network (spec sections 46, 50-52,
//! carried over the new transport from this plan's Part 2).
use std::time::Duration;

use oag_core::{ActorType, Permission};
use oag_crypto::PeerIdentity;
use oag_graph::{AssertInput, AuthContext, GraphService, ResolveOutcome};
use oag_storage::pool::open_pool;

fn temp_db_path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "oag-reticulum-test-{name}-{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("oag.sqlite")
}

fn free_local_port() -> u16 {
    // Grab an OS-assigned free port, then release it immediately -- the
    // Reticulum `TcpServer` needs a bindable address string of its own
    // rather than an already-open listener, so we can't hand this listener
    // to it directly (same tradeoff every "give me a free port" test helper
    // makes: a vanishingly small race window between release and rebind).
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

#[tokio::test]
async fn event_created_on_peer_a_syncs_to_peer_b_over_reticulum() {
    let identity_a = PeerIdentity::generate();
    let identity_b = PeerIdentity::generate();

    let pool_a = open_pool(&temp_db_path("a")).await.unwrap();
    let pool_b = open_pool(&temp_db_path("b")).await.unwrap();

    // Seed peer A with one real assertion via the normal GraphService path.
    let graph_a = GraphService::new(pool_a.clone(), identity_a.clone());
    let actor_id = graph_a.declare_actor(ActorType::Agent, Some("tester".into()), None).await.unwrap();
    let auth = AuthContext { actor_id, permissions: vec![Permission::GraphAssert] };
    graph_a
        .assert(
            &auth,
            AssertInput {
                subject: "https://example.com/reticulum-test".into(),
                subject_type: None,
                predicate: "related_to".into(),
                object: "https://example.org".into(),
                object_type: None,
                evidence: vec![],
                actor_confidence: None,
                observed_at: None,
                extraction_method: None,
            },
        )
        .await
        .unwrap();

    let listen_addr: std::net::SocketAddr = format!("127.0.0.1:{}", free_local_port()).parse().unwrap();

    let config = oag_reticulum::ReticulumConfig {
        listen_tcp: Some(listen_addr),
        uplink_tcp: None,
        announce_interval: Duration::from_millis(300),
    };
    tokio::spawn(oag_reticulum::run_listener(pool_a, identity_a.clone(), config));

    // Give the listener a moment to bind before peer B tries to connect.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let target = oag_reticulum::local_address_hash(&identity_a);
    let summary = tokio::time::timeout(
        Duration::from_secs(20),
        oag_reticulum::sync_with_peer(&pool_b, &identity_b, &listen_addr.to_string(), target),
    )
    .await
    .expect("sync_with_peer should not hang")
    .expect("reticulum sync should succeed");

    // ACTOR_DECLARE (from declare_actor) + ASSERT_RELATION (from assert) --
    // both are peer A's own events and both must cross before B knows the
    // asserting actor at all.
    assert_eq!(summary.applied, 2, "expected the actor-declare + assert-relation events to apply, got {summary:?}");
    assert_eq!(summary.forks, 0);
    assert!(summary.errors.is_empty(), "unexpected errors: {:?}", summary.errors);

    let graph_b = GraphService::new(pool_b, identity_b);
    let resolved = graph_b.resolve("https://example.com/reticulum-test").await.unwrap();
    assert!(matches!(resolved, ResolveOutcome::Found { .. }), "got {resolved:?}");
}
