use std::sync::Arc;

use oag_core::{ActorType, AssertionStatus, Permission};
use oag_crypto::PeerIdentity;
use oag_graph::{AssertInput, AuthContext, GraphService};
use oag_storage::pool::open_pool;

use crate::federation::FederationPolicy;
use crate::server::router;
use crate::service::SyncService;

struct TestPeer {
    dir: std::path::PathBuf,
    graph: Arc<GraphService>,
    sync: SyncService,
    auth: AuthContext,
    addr: String,
}

fn fresh_temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "oag-sync-test-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn spawn_peer(name: &str) -> TestPeer {
    spawn_peer_at(fresh_temp_dir(name), name).await
}

/// `PeerIdentity::load_or_generate` and a find-or-declare actor lookup (same
/// pattern as `oag-crawler`'s `crawler_auth`) rather than always generating
/// fresh -- so `restart_peer` below can reopen an *existing* data directory
/// and get back the exact same `peer_id`/`actor_id`, simulating a process
/// restart (spec section 104's "peer restart" replication test) rather than
/// a brand-new peer.
async fn spawn_peer_at(dir: std::path::PathBuf, name: &str) -> TestPeer {
    let pool = open_pool(&dir.join("oag.sqlite")).await.unwrap();

    let identity = PeerIdentity::load_or_generate(&dir.join("identity.key")).unwrap();
    let peer_id = identity.peer_id();
    let public_key = identity.verifying_key().to_bytes();

    let graph = Arc::new(GraphService::new(pool.clone(), identity));
    let actor_name = format!("{name}-actor");
    let actor_id = match graph.find_actor_by_name(&actor_name, ActorType::Service).await.unwrap() {
        Some(actor) => actor.id,
        None => graph.declare_actor(ActorType::Service, Some(actor_name), None, None).await.unwrap(),
    };
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
        dir,
        graph,
        sync,
        auth,
        addr: format!("http://{addr}"),
    }
}

/// Simulates `old`'s process being killed and `oag serve` started again
/// against the same data directory: a fresh `SqlitePool`/`GraphService`/
/// `SyncService`/HTTP listener (on a new port -- restarting the exact same
/// port isn't the point here, only that identity and data survive), same
/// `peer_id` and `actor_id` as before. `old`'s own background HTTP server
/// task is simply abandoned (nobody talks to it again), same as a killed
/// process's listener socket would be.
async fn restart_peer(old: TestPeer, name: &str) -> TestPeer {
    let dir = old.dir.clone();
    drop(old);
    spawn_peer_at(dir, name).await
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

/// Spec section 12/40's whole identity model rests on `PeerId` being
/// *derived* from a public key, never independently chosen -- `discover_peers`
/// (the `/peers`-gossip path) previously stored whatever `(peer_id,
/// public_key)` pairing a relay offered with no check that the pairing was
/// even internally consistent, unlike `sync_with_peer`'s own `hello` path a
/// few lines above it in the same file, which already validated this. See
/// `forged_event_lying_about_its_origin_peer_is_rejected` and
/// `a_wrong_key_on_file_for_a_peer_blocks_their_real_events_rather_than_accepting_forgeries`
/// below for exactly what adopting a bad pairing would and wouldn't have
/// cost (denial-of-replication against the real peer, not impersonation --
/// `ingest_remote_event`'s own `OriginKeyMismatch` check is a second,
/// independent backstop against the forgery case regardless of this fix).
///
/// This test plays the role of a relay ("attacker") that has poisoned its
/// *own* `peers` table with exactly that kind of mismatched pairing (which
/// a real attacker could arrange identically -- nothing stops a peer from
/// inserting whatever it wants into its own database), then has a fresh
/// peer ("victim") sync with it and confirms the poisoned entry was never
/// adopted.
#[tokio::test]
async fn gossip_with_a_mismatched_peer_id_and_public_key_is_rejected() {
    let attacker = spawn_peer("gossip-poison-attacker").await;
    let victim = spawn_peer("gossip-poison-victim").await;

    let real_third_party_identity = PeerIdentity::generate();
    let claimed_victim_of_spoofing = real_third_party_identity.peer_id();
    // The attacker's own key -- deliberately paired with a peer_id it does
    // not derive from.
    let attackers_own_public_key = attacker.sync.self_public_key();

    {
        let mut conn = attacker.graph.pool().acquire().await.unwrap();
        oag_storage::repo::peers::upsert_peer(
            &mut conn,
            claimed_victim_of_spoofing.as_bytes(),
            &attackers_own_public_key,
            None,
            0,
        )
        .await
        .unwrap();
        oag_storage::repo::peers::add_address(&mut conn, claimed_victim_of_spoofing.as_bytes(), "http://127.0.0.1:1")
            .await
            .unwrap();
    }

    victim.sync.sync_with_peer(&attacker.addr).await.unwrap();

    let mut victim_conn = victim.graph.pool().acquire().await.unwrap();
    let stored =
        oag_storage::repo::peers::get_peer(&mut victim_conn, claimed_victim_of_spoofing.as_bytes()).await.unwrap();
    assert!(
        stored.is_none(),
        "victim must never adopt a peer_id/public_key pairing that doesn't derive correctly"
    );
}

/// Durability visibility: nothing assumes any one peer's storage is safe on
/// its own — `replication_status()` should show the real, monitored picture
/// of how many *other* peers are known to have caught up with this peer's
/// own data, before and after a round of mutual syncing.
#[tokio::test]
async fn replication_status_reflects_sync_state() {
    let a = spawn_peer("replication-a").await;
    let b = spawn_peer("replication-b").await;

    let before = a.sync.replication_status().await.unwrap();
    assert_eq!(before.known_peer_count, 0);
    assert_eq!(before.peers_fully_caught_up, 0);
    assert!(!before.meets_target);

    a.graph
        .assert(&a.auth, sample_assertion("https://example.com/replication-test", "concept-durability"))
        .await
        .unwrap();

    // B pulls A's events...
    b.sync.sync_with_peer(&a.addr).await.unwrap();
    // ...then A syncs *with* B, receiving B's hello -- which now reports B's
    // own knowledge of A's origin, exactly the head A needs to see itself
    // reflected back through another peer.
    a.sync.sync_with_peer(&b.addr).await.unwrap();

    let after = a.sync.replication_status().await.unwrap();
    assert_eq!(after.known_peer_count, 1);
    assert_eq!(after.peers_fully_caught_up, 1, "B should be recorded as fully caught up with A's origin");
    assert!(after.lagging_peers.is_empty());
    assert_eq!(after.target_replication_factor, 3);
    assert!(!after.meets_target, "one caught-up peer is still below the floor of 3");
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

/// Regression test for the exact bug found live: `discover_peers`'
/// per-response cap (`MAX_PEERS_PER_RESPONSE`) doesn't bound the *total*
/// peers table size on its own, since the gossip loop re-syncs with every
/// known address forever -- a single malicious relay feeding fresh,
/// cheaply-generated-but-cryptographically-valid fake identities every
/// round would otherwise grow `peers`/`peer_addresses` on disk without
/// bound. Seeds a victim already at the global cap, then confirms a
/// *real, legitimate* peer introduced via gossip is still correctly
/// rejected -- not because anything about the introduction is wrong, but
/// purely because the table is already full.
#[tokio::test]
async fn discover_peers_does_not_grow_the_table_past_the_global_cap() {
    let a = spawn_peer("cap-a").await;
    let b = spawn_peer("cap-b").await;
    let victim = spawn_peer("cap-victim").await;

    // B genuinely knows about A (direct hello-handshake contact).
    b.sync.sync_with_peer(&a.addr).await.unwrap();

    // Fill victim's own table up to the same cap used in service.rs.
    const MAX_TOTAL_KNOWN_PEERS: usize = 10_000;
    {
        let mut conn = victim.graph.pool().acquire().await.unwrap();
        for _ in 0..MAX_TOTAL_KNOWN_PEERS {
            let filler_identity = PeerIdentity::generate();
            oag_storage::repo::peers::upsert_peer(
                &mut conn,
                filler_identity.peer_id().as_bytes(),
                &filler_identity.verifying_key().to_bytes(),
                None,
                0,
            )
            .await
            .unwrap();
        }
    }

    // Victim syncs with B -- B's own /peers response will include A.
    victim.sync.sync_with_peer(&b.addr).await.unwrap();

    let mut conn = victim.graph.pool().acquire().await.unwrap();
    let learned_a =
        oag_storage::repo::peers::get_peer(&mut conn, a.sync.self_peer_id().as_bytes()).await.unwrap();
    assert!(
        learned_a.is_none(),
        "a real, legitimate peer introduced via gossip must still be rejected once the victim is already at the global cap"
    );
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

/// Spec section 104's "peer restart" replication test: a peer's process
/// dying and coming back up against the same data directory must not lose
/// data, desync its identity, or prevent it from continuing to converge
/// with the network -- distinct from `partition_reconnect_converges`, which
/// only ever simulates a network split, never an actual process restart
/// (same `GraphService`/`SyncService` instances throughout that test).
#[tokio::test]
async fn peer_restart_resumes_and_converges() {
    let a = spawn_peer("restart-a").await;
    let b = spawn_peer("restart-b").await;

    let before_restart = b
        .graph
        .assert(&b.auth, sample_assertion("https://example.com/before-restart", "concept-before"))
        .await
        .unwrap();
    a.sync.sync_with_peer(&b.addr).await.unwrap();
    assert!(a.graph.get_assertion(before_restart).await.unwrap().is_some());

    let original_peer_id = b.sync.self_peer_id();
    let b = restart_peer(b, "restart-b").await;
    assert_eq!(b.sync.self_peer_id(), original_peer_id, "restart must not desync the peer's identity");

    // Data written before the "crash" is still there after reopening the
    // same data directory fresh.
    assert!(
        b.graph.get_assertion(before_restart).await.unwrap().is_some(),
        "data committed before restart must survive it"
    );

    // The restarted peer can still author new events under its own chain...
    let after_restart = b
        .graph
        .assert(&b.auth, sample_assertion("https://example.com/after-restart", "concept-after"))
        .await
        .unwrap();
    // ...and the network still converges around it post-restart.
    a.sync.sync_with_peer(&b.addr).await.unwrap();
    assert!(a.graph.get_assertion(after_restart).await.unwrap().is_some(), "A should see B's post-restart event");
}

/// `ingest_remote_event`'s own `OriginKeyMismatch` check is a second,
/// independent backstop against exactly the attack the `discover_peers`
/// fix above closes one path to: even if a wrong `(peer_id, public_key)`
/// pairing ever *did* end up on file for some origin (via this bug, or any
/// other means), an event lying about its own `origin_peer` relative to
/// whatever key actually signed it is still rejected here, regardless of
/// what any local table says -- `derived_peer_id` is computed purely from
/// the verifying key's own bytes, never trusted from storage.
#[tokio::test]
async fn forged_event_lying_about_its_origin_peer_is_rejected() {
    let attacker_identity = PeerIdentity::generate();
    let victim_identity = PeerIdentity::generate();
    let victim_id = victim_identity.peer_id();

    // Manually build an envelope that lies: origin_peer claims to be the
    // victim, but it's signed with the attacker's own key (build_and_sign
    // can't produce this -- it always sets origin_peer to the signing
    // identity's own true id, so forging the claim requires going one
    // layer lower, the same way a real attacker would have to).
    let unsigned = oag_events::envelope::UnsignedEvent {
        version: 1,
        origin_peer: victim_id.to_string(),
        sequence: 1,
        previous_event: None,
        created_at: 1_700_000_000,
        payload: oag_events::payload::EventPayload::ActorDeclare(oag_events::payload::ActorDeclarePayload {
            actor_type: "agent".into(),
            name: Some("forged".into()),
            public_key: None,
            identity_uri: None,
        }),
    };
    let canonical_unsigned = oag_core::canonical_json_bytes(&unsigned).unwrap();
    let signature = oag_crypto::sign_with_domain(
        attacker_identity.signing_key(),
        oag_events::envelope::EVENT_ID_DOMAIN,
        &canonical_unsigned,
    );
    let signed = oag_events::envelope::SignedEvent {
        unsigned,
        signature: hex::encode(signature.to_bytes()),
    };

    let pool = open_pool(&fresh_temp_dir("forged-origin-peer").join("oag.sqlite")).await.unwrap();

    // Exactly what post_events does: verify using whatever key is on file
    // for the event's own claimed origin_peer -- using the attacker's real
    // key directly here, as if some wrong entry had handed it back for
    // victim_id, isolates the question to this function's own internal
    // check, independent of how the key was looked up.
    let result = oag_events::ingest_remote_event(&pool, attacker_identity.verifying_key(), signed, 1_700_000_001).await;
    assert!(
        matches!(result, Err(oag_events::EventsError::OriginKeyMismatch { .. })),
        "an event lying about its own origin_peer must be rejected regardless of which key verifies it, got {result:?}"
    );
}

/// What a wrong `(peer_id, public_key)` entry actually costs, precisely
/// characterized (worth keeping as the record of this, since it's easy to
/// overstate): it does **not** let an attacker's forged event be accepted
/// as the real peer's -- `OriginKeyMismatch` above blocks that regardless.
/// What it *does* do is block the real peer's own legitimate events from
/// ever being accepted, since they'd be verified against the wrong key and
/// fail signature verification -- a denial-of-replication effect against
/// whichever real peer's identity got poisoned, not an impersonation one.
#[tokio::test]
async fn a_wrong_key_on_file_for_a_peer_blocks_their_real_events_rather_than_accepting_forgeries() {
    let victim_identity = PeerIdentity::generate();
    let attacker_identity = PeerIdentity::generate();

    // The victim signs a perfectly real, legitimate event with their own key.
    let (signed, _) = oag_events::builder::build_and_sign(
        &victim_identity,
        1,
        None,
        1_700_000_000,
        oag_events::payload::EventPayload::ActorDeclare(oag_events::payload::ActorDeclarePayload {
            actor_type: "agent".into(),
            name: Some("real-victim-actor".into()),
            public_key: None,
            identity_uri: None,
        }),
    )
    .unwrap();

    let pool = open_pool(&fresh_temp_dir("wrong-key-blocks-real-events").join("oag.sqlite")).await.unwrap();

    // A peer with the wrong (attacker's) key on file for this origin can't
    // accept the victim's real event at all.
    let blocked =
        oag_events::ingest_remote_event(&pool, attacker_identity.verifying_key(), signed.clone(), 1_700_000_001).await;
    assert!(matches!(blocked, Err(oag_events::EventsError::InvalidSignature(_))), "got {blocked:?}");

    // The same event, verified against the victim's actual real key, must
    // still succeed -- confirming the above wasn't blocked for some other
    // reason.
    let applied = oag_events::ingest_remote_event(&pool, victim_identity.verifying_key(), signed, 1_700_000_002).await;
    assert!(matches!(applied, Ok(oag_events::IngestOutcome::Applied(_, _))), "got {applied:?}");
}
