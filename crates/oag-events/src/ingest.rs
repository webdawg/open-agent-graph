use oag_core::EventId;
use oag_crypto::{PeerId, VerifyingKey};
use oag_storage::repo::events as events_repo;
use oag_storage::SqlitePool;

use crate::envelope::SignedEvent;
use crate::error::EventsError;
use crate::projector::{project, ProjectionOutcome};
use crate::validate::{validate_chain, verify_and_derive_id};

/// What happened when a remote event was offered to this peer.
#[derive(Debug)]
pub enum IngestOutcome {
    /// New event, verified, chain-valid, projected.
    Applied(EventId, ProjectionOutcome),
    /// Already had this exact event at this (origin, sequence) — a no-op
    /// (spec section 53: duplicate delivery must be idempotent).
    AlreadyKnown(EventId),
    /// A *different* event already exists at this origin's sequence number
    /// (spec section 55: origin fork). Recorded as evidence; neither event
    /// is projected or allowed to advance the origin's head.
    Forked { existing: EventId, incoming: EventId },
}

/// Ingest one event from a remote origin, inside a single transaction
/// mirroring [`crate::commit::commit_local_event`]'s atomicity (spec section
/// 38 applies equally to replicated events, not just self-generated ones).
///
/// `verifying_key` MUST be the caller's already-trusted public key for the
/// origin peer this event claims to be from (e.g. from a prior `hello` or
/// the local `peers` table) — this function checks that key actually
/// derives to the claimed `origin_peer` id, but it does not itself decide
/// *which* key to trust for a given peer id; that's the caller's job
/// (typically backed by [`oag_storage::repo::peers`]).
///
/// Note: out-of-order delivery (a gap before this sequence) is rejected
/// outright rather than queued as "pending" (spec section 54 allows either;
/// this milestone's sync client always fetches contiguous ranges in
/// ascending order, so a gap here indicates a genuine problem upstream, not
/// normal reordering).
pub async fn ingest_remote_event(
    pool: &SqlitePool,
    verifying_key: VerifyingKey,
    signed: SignedEvent,
    received_at: i64,
) -> Result<IngestOutcome, EventsError> {
    let event_id = verify_and_derive_id(&signed, &verifying_key)?;

    let claimed_peer_id: PeerId = signed.unsigned.origin_peer.parse()?;
    let derived_peer_id = PeerId::from_public_key(&verifying_key);
    if claimed_peer_id != derived_peer_id {
        return Err(EventsError::OriginKeyMismatch {
            claimed: claimed_peer_id.to_string(),
            derived: derived_peer_id.to_string(),
        });
    }
    let origin_peer_id = *claimed_peer_id.as_bytes();
    let sequence = signed.unsigned.sequence;
    let previous_event: Option<EventId> = signed
        .unsigned
        .previous_event
        .as_deref()
        .map(str::parse)
        .transpose()?;

    let mut tx = pool.begin().await.map_err(oag_storage::StorageError::from)?;

    if let Some(existing) = events_repo::find_event_id_at(&mut tx, &origin_peer_id, sequence as i64).await? {
        if existing == event_id {
            tx.commit().await.map_err(oag_storage::StorageError::from)?;
            return Ok(IngestOutcome::AlreadyKnown(event_id));
        }
        // A fork must be recorded even if this is the first time we've heard
        // of the origin at all (e.g. we only have its early history via a
        // relay) — ensure a peers row exists before flagging it.
        oag_storage::repo::peers::upsert_peer(
            &mut tx,
            &origin_peer_id,
            &verifying_key.to_bytes(),
            None,
            received_at,
        )
        .await?;
        oag_storage::repo::peers::record_fork(
            &mut tx,
            &origin_peer_id,
            sequence as i64,
            existing.as_hash().as_bytes(),
            event_id.as_hash().as_bytes(),
            received_at,
        )
        .await?;
        oag_storage::repo::peers::mark_forked(&mut tx, &origin_peer_id).await?;
        tx.commit().await.map_err(oag_storage::StorageError::from)?;
        return Ok(IngestOutcome::Forked { existing, incoming: event_id });
    }

    let origin = events_repo::get_origin(&mut tx, &origin_peer_id).await?;
    let (expected_next_sequence, expected_head) = match origin {
        Some(row) => {
            let head = row
                .head_event_id
                .map(|bytes| {
                    oag_storage::error::bytes_to_array(&bytes)
                        .map(|arr| EventId::from_hash(oag_core::Hash32::from_bytes(arr)))
                })
                .transpose()?;
            (row.highest_contiguous_sequence as u64 + 1, head)
        }
        None => (1u64, None),
    };
    validate_chain(expected_next_sequence, expected_head, sequence, previous_event)?;

    let canonical_payload = oag_core::canonical_json_bytes(&signed)?;
    let signature_bytes = hex::decode(&signed.signature)?;

    events_repo::insert_event(
        &mut tx,
        &events_repo::StoredEvent {
            event_id,
            origin_peer_id,
            sequence: sequence as i64,
            previous_event_id: previous_event,
            event_type: signed.unsigned.payload.type_str().to_string(),
            canonical_payload,
            created_at: signed.unsigned.created_at,
            signature: signature_bytes,
        },
        received_at,
    )
    .await?;

    let outcome = project(&mut tx, event_id, signed.unsigned.created_at, &signed.unsigned.payload).await?;

    events_repo::advance_origin(&mut tx, &origin_peer_id, sequence as i64, event_id).await?;

    tx.commit().await.map_err(oag_storage::StorageError::from)?;

    Ok(IngestOutcome::Applied(event_id, outcome))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::build_and_sign;
    use crate::commit::commit_local_event;
    use crate::payload::{ActorDeclarePayload, AssertRelationPayload};

    fn temp_pool_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oag-events-ingest-test-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("oag.sqlite")
    }

    fn assert_payload(actor_id_hex: &str) -> crate::payload::EventPayload {
        crate::payload::EventPayload::AssertRelation(AssertRelationPayload {
            subject_identifier: "url:https://github.com/example/foo".into(),
            subject_type: "repository".into(),
            predicate: "implements".into(),
            object_identifier: "concept:model-context-protocol".into(),
            object_type: "concept".into(),
            actor_id: actor_id_hex.to_string(),
            actor_confidence: Some(0.9),
            observed_at: None,
            extraction_method: "direct".into(),
        })
    }

    /// Sets up a pool with one "already synced" event (an ActorDeclare) from
    /// a remote identity, so the next event under test can continue a real
    /// chain. Returns (pool, remote_identity, actor_event_id, actor_id_hex).
    async fn seeded_pool(
        name: &str,
    ) -> (oag_storage::SqlitePool, oag_crypto::PeerIdentity, EventId, String) {
        let pool = oag_storage::open_pool(&temp_pool_path(name)).await.unwrap();
        let remote_identity = oag_crypto::PeerIdentity::generate();
        let (actor_event_id, outcome) = commit_local_event(
            &pool,
            &remote_identity,
            crate::payload::EventPayload::ActorDeclare(ActorDeclarePayload {
                actor_type: "agent".into(),
                name: Some("remote agent".into()),
                public_key: None,
                identity_uri: None,
            }),
            1_700_000_000,
        )
        .await
        .unwrap();
        let ProjectionOutcome::ActorDeclared { actor_id } = outcome else {
            panic!("expected ActorDeclared outcome");
        };
        (pool, remote_identity, actor_event_id, actor_id.to_hex())
    }

    #[tokio::test]
    async fn applies_new_event_and_is_idempotent_on_replay() {
        let (pool, remote_identity, actor_event_id, actor_id_hex) =
            seeded_pool("apply-idempotent").await;

        let (signed, expected_id) = build_and_sign(
            &remote_identity,
            2,
            Some(actor_event_id),
            1_700_000_001,
            assert_payload(&actor_id_hex),
        )
        .unwrap();

        let outcome = ingest_remote_event(&pool, remote_identity.verifying_key(), signed.clone(), 1_700_000_002)
            .await
            .unwrap();
        assert!(matches!(outcome, IngestOutcome::Applied(id, _) if id == expected_id));

        let replay = ingest_remote_event(&pool, remote_identity.verifying_key(), signed, 1_700_000_003)
            .await
            .unwrap();
        assert!(matches!(replay, IngestOutcome::AlreadyKnown(id) if id == expected_id));
    }

    #[tokio::test]
    async fn rejects_sequence_gap() {
        let (pool, remote_identity, actor_event_id, actor_id_hex) = seeded_pool("sequence-gap").await;

        // Sequence 3 without 2 ever having arrived.
        let (signed, _) = build_and_sign(
            &remote_identity,
            3,
            Some(actor_event_id),
            1_700_000_001,
            assert_payload(&actor_id_hex),
        )
        .unwrap();

        let result = ingest_remote_event(&pool, remote_identity.verifying_key(), signed, 1_700_000_002).await;
        assert!(matches!(result, Err(EventsError::SequenceGap { .. })));
    }

    #[tokio::test]
    async fn rejects_wrong_previous_event() {
        let (pool, remote_identity, _actor_event_id, actor_id_hex) =
            seeded_pool("wrong-previous").await;

        // Correct sequence (2), but previous_event doesn't match the real head.
        let bogus_previous = EventId::derive(b"not-the-real-head");
        let (signed, _) = build_and_sign(
            &remote_identity,
            2,
            Some(bogus_previous),
            1_700_000_001,
            assert_payload(&actor_id_hex),
        )
        .unwrap();

        let result = ingest_remote_event(&pool, remote_identity.verifying_key(), signed, 1_700_000_002).await;
        assert!(matches!(result, Err(EventsError::PreviousEventMismatch { .. })));
    }

    #[tokio::test]
    async fn detects_and_records_fork() {
        let (pool, remote_identity, actor_event_id, actor_id_hex) = seeded_pool("fork-detect").await;

        let (first, first_id) = build_and_sign(
            &remote_identity,
            2,
            Some(actor_event_id),
            1_700_000_001,
            assert_payload(&actor_id_hex),
        )
        .unwrap();
        ingest_remote_event(&pool, remote_identity.verifying_key(), first, 1_700_000_002)
            .await
            .unwrap();

        // A second, different event claiming the SAME (origin, sequence).
        let mut conflicting_payload = assert_payload(&actor_id_hex);
        if let crate::payload::EventPayload::AssertRelation(ref mut p) = conflicting_payload {
            p.predicate = "contradicts".into();
        }
        let (second, second_id) = build_and_sign(
            &remote_identity,
            2,
            Some(actor_event_id),
            1_700_000_001,
            conflicting_payload,
        )
        .unwrap();
        assert_ne!(first_id, second_id);

        let outcome = ingest_remote_event(&pool, remote_identity.verifying_key(), second, 1_700_000_003)
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            IngestOutcome::Forked { existing, incoming } if existing == first_id && incoming == second_id
        ));

        let mut conn = pool.acquire().await.unwrap();
        let peer_id_bytes = *remote_identity.peer_id().as_bytes();
        let info = oag_storage::repo::peers::get_peer(&mut conn, &peer_id_bytes)
            .await
            .unwrap()
            .unwrap();
        assert!(info.forked);
    }
}
