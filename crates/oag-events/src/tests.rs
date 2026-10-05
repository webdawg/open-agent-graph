use oag_core::AssertionStatus;
use oag_crypto::PeerIdentity;
use oag_storage::pool::open_pool;

use crate::commit::commit_local_event;
use crate::payload::{
    ActorDeclarePayload, ActorKeyAddPayload, ActorKeyRevokePayload, AddEvidencePayload,
    AssertRelationPayload, DisputeAssertionPayload, EventPayload, NodeAliasPayload,
    RetractAssertionPayload, SupersedeAssertionPayload, VerifyAssertionPayload,
};
use crate::projector::ProjectionOutcome;

fn temp_db_path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "oag-events-test-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("oag.sqlite")
}

#[tokio::test]
async fn full_vertical_slice_through_commit_local_event() {
    let pool = open_pool(&temp_db_path("vertical-slice")).await.unwrap();
    let identity = PeerIdentity::generate();
    let now = 1_700_000_000_i64;

    // Register an actor and give it a key (mirrors the bootstrap-admin flow).
    let (_, outcome) = commit_local_event(
        &pool,
        &identity,
        EventPayload::ActorDeclare(ActorDeclarePayload {
            actor_type: "agent".into(),
            name: Some("test agent".into()),
            public_key: None,
            identity_uri: None,
        }),
        now,
    )
    .await
    .unwrap();
    let ProjectionOutcome::ActorDeclared { actor_id } = outcome else {
        panic!("expected ActorDeclared outcome");
    };

    commit_local_event(
        &pool,
        &identity,
        EventPayload::ActorKeyAdd(ActorKeyAddPayload {
            actor_id: actor_id.to_hex(),
            key_hash: blake3::hash(b"fake-raw-key").to_hex().to_string(),
            permissions: vec!["graph:assert".into()],
        }),
        now,
    )
    .await
    .unwrap();

    // Assert a relation.
    let (assertion_event_id, _) = commit_local_event(
        &pool,
        &identity,
        EventPayload::AssertRelation(AssertRelationPayload {
            subject_identifier: "url:https://github.com/example/foo".into(),
            subject_type: "repository".into(),
            predicate: "implements".into(),
            object_identifier: "concept:model-context-protocol".into(),
            object_type: "concept".into(),
            actor_id: actor_id.to_hex(),
            actor_confidence: Some(0.95),
            observed_at: Some(now),
            extraction_method: "direct".into(),
        }),
        now,
    )
    .await
    .unwrap();
    let assertion_id = assertion_event_id;

    // Attach evidence.
    commit_local_event(
        &pool,
        &identity,
        EventPayload::AddEvidence(AddEvidencePayload {
            assertion_id: assertion_id.to_hex(),
            evidence_type: "documentation".into(),
            uri: Some("https://github.com/example/foo/blob/main/README.md".into()),
            title: Some("README".into()),
            excerpt: None,
            content_hash: None,
            observed_at: Some(now),
            retrieved_at: Some(now),
        }),
        now,
    )
    .await
    .unwrap();

    {
        let mut conn = pool.acquire().await.unwrap();
        let fetched = oag_storage::repo::assertions::get_by_id(&mut conn, assertion_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, AssertionStatus::Active);
        let evidence = oag_storage::repo::assertions::list_evidence(&mut conn, assertion_id)
            .await
            .unwrap();
        assert_eq!(evidence.len(), 1);
    }

    // Dispute it.
    commit_local_event(
        &pool,
        &identity,
        EventPayload::DisputeAssertion(DisputeAssertionPayload {
            disputed_assertion_id: assertion_id.to_hex(),
            disputing_actor_id: actor_id.to_hex(),
            reason: Some("outdated".into()),
        }),
        now + 1,
    )
    .await
    .unwrap();

    {
        let mut conn = pool.acquire().await.unwrap();
        let fetched = oag_storage::repo::assertions::get_by_id(&mut conn, assertion_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, AssertionStatus::Disputed);
    }

    // Retract it — original assertion must remain resolvable with full history.
    commit_local_event(
        &pool,
        &identity,
        EventPayload::RetractAssertion(RetractAssertionPayload {
            retracted_assertion_id: assertion_id.to_hex(),
            actor_id: actor_id.to_hex(),
            reason: None,
        }),
        now + 2,
    )
    .await
    .unwrap();

    let mut conn = pool.acquire().await.unwrap();
    let fetched = oag_storage::repo::assertions::get_by_id(&mut conn, assertion_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, AssertionStatus::Retracted);

    let history =
        oag_storage::repo::events::history_for(&mut conn, "assertion", assertion_id.as_hash().as_bytes())
            .await
            .unwrap();
    // assert, evidence, dispute, retract all reference this assertion id.
    assert_eq!(history.len(), 4);
}

#[tokio::test]
async fn duplicate_event_ids_are_impossible_for_distinct_local_events() {
    // Sanity check that sequence numbers actually advance across calls
    // against the same pool (guards against the "already existed
    // unexpectedly" branch in commit_local_event firing spuriously).
    let pool = open_pool(&temp_db_path("sequence-advance")).await.unwrap();
    let identity = PeerIdentity::generate();

    let (id_a, _) = commit_local_event(
        &pool,
        &identity,
        EventPayload::ActorDeclare(ActorDeclarePayload {
            actor_type: "agent".into(),
            name: Some("a".into()),
            public_key: None,
            identity_uri: None,
        }),
        1,
    )
    .await
    .unwrap();

    let (id_b, _) = commit_local_event(
        &pool,
        &identity,
        EventPayload::ActorDeclare(ActorDeclarePayload {
            actor_type: "agent".into(),
            name: Some("b".into()),
            public_key: None,
            identity_uri: None,
        }),
        2,
    )
    .await
    .unwrap();

    assert_ne!(id_a, id_b);
}

#[tokio::test]
async fn node_alias_attaches_to_an_existing_node() {
    let pool = open_pool(&temp_db_path("node-alias")).await.unwrap();
    let identity = PeerIdentity::generate();
    let now = 1_700_000_000_i64;

    let (_, outcome) = commit_local_event(
        &pool,
        &identity,
        EventPayload::ActorDeclare(ActorDeclarePayload {
            actor_type: "crawler".into(),
            name: Some("test crawler".into()),
            public_key: None,
            identity_uri: None,
        }),
        now,
    )
    .await
    .unwrap();
    let ProjectionOutcome::ActorDeclared { actor_id } = outcome else {
        panic!("expected ActorDeclared outcome");
    };

    // The baseline assertion pattern the crawler uses: assert something
    // about the page first, which creates its node as a side effect.
    commit_local_event(
        &pool,
        &identity,
        EventPayload::AssertRelation(AssertRelationPayload {
            subject_identifier: "url:https://example.com/page".into(),
            subject_type: "document".into(),
            predicate: "instance_of".into(),
            object_identifier: "concept:document".into(),
            object_type: "concept".into(),
            actor_id: actor_id.to_hex(),
            actor_confidence: Some(1.0),
            observed_at: Some(now),
            extraction_method: "structured_extraction".into(),
        }),
        now,
    )
    .await
    .unwrap();

    let subject_node_id = oag_core::NodeId::from_canonical_identifier("url:https://example.com/page");

    commit_local_event(
        &pool,
        &identity,
        EventPayload::NodeAlias(NodeAliasPayload {
            node_id: subject_node_id.to_hex(),
            alias: "Example Page Title".into(),
            alias_type: "name".into(),
        }),
        now,
    )
    .await
    .unwrap();

    let mut conn = pool.acquire().await.unwrap();
    let aliases = oag_storage::repo::nodes::list_aliases(&mut conn, subject_node_id)
        .await
        .unwrap();
    assert_eq!(aliases.len(), 1);
    assert_eq!(aliases[0].alias, "Example Page Title");
    assert_eq!(aliases[0].alias_type, oag_core::AliasType::Name);
}

#[tokio::test]
async fn node_alias_for_nonexistent_node_is_not_found() {
    let pool = open_pool(&temp_db_path("node-alias-missing")).await.unwrap();
    let identity = PeerIdentity::generate();
    let bogus_node_id = oag_core::NodeId::from_canonical_identifier("url:https://never-asserted.example.com");

    let result = commit_local_event(
        &pool,
        &identity,
        EventPayload::NodeAlias(NodeAliasPayload {
            node_id: bogus_node_id.to_hex(),
            alias: "whatever".into(),
            alias_type: "name".into(),
        }),
        1_700_000_000,
    )
    .await;
    assert!(matches!(result, Err(crate::error::EventsError::NotFound(_))), "got {result:?}");
}

/// A SUPERSEDE_ASSERTION naming a nonexistent id on either side must be
/// rejected atomically -- same one-transaction guarantee as every other
/// event type (architecture.md: "projection failure rolls back the event
/// insert too"), confirmed here rather than just assumed: the real
/// assertion's status must come back untouched, not partially superseded.
#[tokio::test]
async fn supersede_assertion_with_an_unknown_id_on_either_side_is_rejected_atomically() {
    let pool = open_pool(&temp_db_path("supersede-unknown-id")).await.unwrap();
    let identity = PeerIdentity::generate();
    let now = 1_700_000_000;

    let (_, outcome) = commit_local_event(
        &pool,
        &identity,
        EventPayload::ActorDeclare(ActorDeclarePayload {
            actor_type: "agent".into(),
            name: Some("supersede-unknown-id-test".into()),
            public_key: None,
            identity_uri: None,
        }),
        now,
    )
    .await
    .unwrap();
    let ProjectionOutcome::ActorDeclared { actor_id } = outcome else {
        panic!("expected ActorDeclared outcome");
    };

    let (real_assertion_id, _) = commit_local_event(
        &pool,
        &identity,
        EventPayload::AssertRelation(AssertRelationPayload {
            subject_identifier: "url:https://example.com/supersede-unknown-id".into(),
            subject_type: "document".into(),
            predicate: "instance_of".into(),
            object_identifier: "concept:supersede-unknown-id-test".into(),
            object_type: "concept".into(),
            actor_id: actor_id.to_hex(),
            actor_confidence: Some(0.9),
            observed_at: Some(now),
            extraction_method: "direct".into(),
        }),
        now,
    )
    .await
    .unwrap();

    let fake_id = oag_core::AssertionId::derive(b"never-actually-asserted");

    let old_id_fake = commit_local_event(
        &pool,
        &identity,
        EventPayload::SupersedeAssertion(SupersedeAssertionPayload {
            old_assertion_id: fake_id.to_hex(),
            new_assertion_id: real_assertion_id.to_hex(),
            actor_id: actor_id.to_hex(),
        }),
        now,
    )
    .await;
    assert!(matches!(old_id_fake, Err(crate::error::EventsError::NotFound(_))), "got {old_id_fake:?}");

    let new_id_fake = commit_local_event(
        &pool,
        &identity,
        EventPayload::SupersedeAssertion(SupersedeAssertionPayload {
            old_assertion_id: real_assertion_id.to_hex(),
            new_assertion_id: fake_id.to_hex(),
            actor_id: actor_id.to_hex(),
        }),
        now,
    )
    .await;
    assert!(matches!(new_id_fake, Err(crate::error::EventsError::NotFound(_))), "got {new_id_fake:?}");

    let mut conn = pool.acquire().await.unwrap();
    let fetched = oag_storage::repo::assertions::get_by_id(&mut conn, real_assertion_id).await.unwrap().unwrap();
    assert_eq!(
        fetched.status,
        AssertionStatus::Active,
        "the real assertion must come back untouched -- neither failed attempt may have partially applied"
    );
}

/// Same missing-test-coverage gap as the supersede case above, for the two
/// other event types that reference an assertion by id without the
/// `GraphService` layer checking existence itself first (`verify_assertion`/
/// `dispute_assertion` both rely on the projector, same as
/// `supersede_assertion`) -- confirms the projector actually rejects a
/// fake assertion id for these two as well, not just assumed by analogy.
#[tokio::test]
async fn verify_and_dispute_assertion_for_a_nonexistent_assertion_are_both_rejected() {
    let pool = open_pool(&temp_db_path("verify-dispute-unknown-id")).await.unwrap();
    let identity = PeerIdentity::generate();
    let now = 1_700_000_000;

    let (_, outcome) = commit_local_event(
        &pool,
        &identity,
        EventPayload::ActorDeclare(ActorDeclarePayload {
            actor_type: "agent".into(),
            name: Some("verify-dispute-unknown-id-test".into()),
            public_key: None,
            identity_uri: None,
        }),
        now,
    )
    .await
    .unwrap();
    let ProjectionOutcome::ActorDeclared { actor_id } = outcome else {
        panic!("expected ActorDeclared outcome");
    };

    let fake_id = oag_core::AssertionId::derive(b"never-actually-asserted-verify-dispute");

    let verify_result = commit_local_event(
        &pool,
        &identity,
        EventPayload::VerifyAssertion(VerifyAssertionPayload {
            assertion_id: fake_id.to_hex(),
            observer_actor_id: actor_id.to_hex(),
            result: "confirmed".into(),
            observed_at: now,
        }),
        now,
    )
    .await;
    assert!(matches!(verify_result, Err(crate::error::EventsError::NotFound(_))), "got {verify_result:?}");

    let dispute_result = commit_local_event(
        &pool,
        &identity,
        EventPayload::DisputeAssertion(DisputeAssertionPayload {
            disputed_assertion_id: fake_id.to_hex(),
            disputing_actor_id: actor_id.to_hex(),
            reason: None,
        }),
        now,
    )
    .await;
    assert!(matches!(dispute_result, Err(crate::error::EventsError::NotFound(_))), "got {dispute_result:?}");
}

/// Same gap as the two tests above, for the fifth and last
/// `EventsError::NotFound`-returning projector case: `project_add_evidence`.
#[tokio::test]
async fn add_evidence_for_a_nonexistent_assertion_is_not_found() {
    let pool = open_pool(&temp_db_path("add-evidence-unknown-id")).await.unwrap();
    let identity = PeerIdentity::generate();
    let now = 1_700_000_000;

    let fake_id = oag_core::AssertionId::derive(b"never-actually-asserted-add-evidence");

    let result = commit_local_event(
        &pool,
        &identity,
        EventPayload::AddEvidence(AddEvidencePayload {
            assertion_id: fake_id.to_hex(),
            evidence_type: "documentation".into(),
            uri: Some("https://example.com/docs".into()),
            title: None,
            excerpt: None,
            content_hash: None,
            observed_at: Some(now),
            retrieved_at: Some(now),
        }),
        now,
    )
    .await;
    assert!(matches!(result, Err(crate::error::EventsError::NotFound(_))), "got {result:?}");
}

#[tokio::test]
async fn actor_key_revoke_deactivates_the_key() {
    let pool = open_pool(&temp_db_path("key-revoke")).await.unwrap();
    let identity = PeerIdentity::generate();
    let now = 1_700_000_000_i64;

    let (_, outcome) = commit_local_event(
        &pool,
        &identity,
        EventPayload::ActorDeclare(ActorDeclarePayload {
            actor_type: "agent".into(),
            name: Some("revoke-test".into()),
            public_key: None,
            identity_uri: None,
        }),
        now,
    )
    .await
    .unwrap();
    let ProjectionOutcome::ActorDeclared { actor_id } = outcome else {
        panic!("expected ActorDeclared outcome");
    };

    let key_hash = blake3::hash(b"revoke-me").to_hex().to_string();
    commit_local_event(
        &pool,
        &identity,
        EventPayload::ActorKeyAdd(ActorKeyAddPayload {
            actor_id: actor_id.to_hex(),
            key_hash: key_hash.clone(),
            permissions: vec!["graph:assert".into()],
        }),
        now,
    )
    .await
    .unwrap();

    let key_hash_bytes: [u8; 32] = hex::decode(&key_hash).unwrap().try_into().unwrap();
    let mut conn = pool.acquire().await.unwrap();
    assert!(
        oag_storage::repo::actors::find_active_key(&mut conn, &key_hash_bytes).await.unwrap().is_some(),
        "key must be active before revocation"
    );
    drop(conn);

    commit_local_event(
        &pool,
        &identity,
        EventPayload::ActorKeyRevoke(ActorKeyRevokePayload { actor_id: actor_id.to_hex(), key_hash: key_hash.clone() }),
        now + 1,
    )
    .await
    .unwrap();

    let mut conn = pool.acquire().await.unwrap();
    assert!(
        oag_storage::repo::actors::find_active_key(&mut conn, &key_hash_bytes).await.unwrap().is_none(),
        "key must no longer authenticate after revocation"
    );

    let keys = oag_storage::repo::actors::list_keys(&mut conn).await.unwrap();
    assert_eq!(keys.len(), 1);
    assert!(keys[0].revoked_at.is_some(), "list_keys must still show the key, flagged as revoked");
}

/// `commit_local_event` itself is **not** safe to call concurrently for the
/// same peer -- found live, not assumed: this function's own doc comment
/// claimed "concurrent calls serialize correctly against SQLite's writer
/// lock," which doesn't hold in WAL mode. Each call reads the current head
/// under its own snapshot, then writes based on it; when two calls
/// interleave, the second to attempt its write hits `SQLITE_BUSY_SNAPSHOT`
/// (surfaced here as a generic "database is locked" error) rather than
/// waiting (a `busy_timeout` can't fix a stale read snapshot by waiting --
/// only restarting the whole read-then-write can). This test exists to
/// document that fact and guard against someone "fixing" it by deleting
/// this test rather than understanding why it fails: the real fix is a
/// serialization lock one layer up, in `oag_graph::GraphService::
/// commit_event` (every `GraphService` method goes through it, there is
/// no direct caller of this function outside that one wrapper), verified
/// by the analogous test in `oag-graph`.
#[tokio::test]
async fn commit_local_event_itself_is_not_safe_for_concurrent_same_peer_calls() {
    let pool = open_pool(&temp_db_path("concurrent-commits-raw")).await.unwrap();
    let identity = PeerIdentity::generate();

    let mut tasks = Vec::new();
    for i in 0..20 {
        let pool = pool.clone();
        let identity = identity.clone();
        tasks.push(tokio::spawn(async move {
            commit_local_event(
                &pool,
                &identity,
                EventPayload::ActorDeclare(ActorDeclarePayload {
                    actor_type: "agent".into(),
                    name: Some(format!("concurrent-actor-{i}")),
                    public_key: None,
                    identity_uri: None,
                }),
                1_700_000_000,
            )
            .await
        }));
    }

    let mut errors = 0;
    for task in tasks {
        if task.await.unwrap().is_err() {
            errors += 1;
        }
    }
    assert!(
        errors > 0,
        "expected at least some of these 20 unserialized concurrent calls to race and fail -- if this \
         now passes, something about SQLite's own locking behavior changed, and GraphService::commit_event's \
         serialization lock may no longer be load-bearing (or this test got lucky; rerun before concluding that)"
    );
}
