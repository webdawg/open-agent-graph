use oag_core::AssertionStatus;
use oag_crypto::PeerIdentity;
use oag_storage::pool::open_pool;

use crate::commit::commit_local_event;
use crate::payload::{
    ActorDeclarePayload, ActorKeyAddPayload, AddEvidencePayload, AssertRelationPayload,
    DisputeAssertionPayload, EventPayload, RetractAssertionPayload,
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
