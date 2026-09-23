use oag_core::EventId;
use oag_crypto::PeerIdentity;
use oag_storage::repo::events as events_repo;
use oag_storage::SqlitePool;

use crate::builder::build_and_sign;
use crate::error::EventsError;
use crate::payload::EventPayload;
use crate::projector::{project, ProjectionOutcome};

/// Build, sign, durably store, and project a new event originating from
/// *this* peer, all in one SQLite transaction (spec section 38: the event
/// must be committed atomically with its projection — a crash must never
/// leave a projected event that doesn't exist in the local event log).
///
/// Sequence/`previous_event` are read from `event_origins` for our own
/// `peer_id` inside the same transaction, so concurrent calls serialize
/// correctly against SQLite's writer lock.
pub async fn commit_local_event(
    pool: &SqlitePool,
    identity: &PeerIdentity,
    payload: EventPayload,
    created_at: i64,
) -> Result<(EventId, ProjectionOutcome), EventsError> {
    let mut tx = pool.begin().await.map_err(oag_storage::StorageError::from)?;

    let peer_id_bytes = *identity.peer_id().as_bytes();
    let origin = events_repo::get_origin(&mut tx, &peer_id_bytes).await?;
    let (next_sequence, previous_event) = match origin {
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

    let (signed, event_id) =
        build_and_sign(identity, next_sequence, previous_event, created_at, payload)?;

    let canonical_payload = oag_core::canonical_json_bytes(&signed)?;
    let signature_bytes = hex::decode(&signed.signature)?;

    let inserted = events_repo::insert_event(
        &mut tx,
        &events_repo::StoredEvent {
            event_id,
            origin_peer_id: peer_id_bytes,
            sequence: next_sequence as i64,
            previous_event_id: previous_event,
            event_type: signed.unsigned.payload.type_str().to_string(),
            canonical_payload,
            created_at,
            signature: signature_bytes,
        },
        created_at,
    )
    .await?;

    if !inserted {
        // We derive sequence numbers from our own committed state, so this
        // should be unreachable for self-generated events — but projecting
        // twice would violate invariant 8, so refuse rather than guess.
        return Err(EventsError::NotFound(format!(
            "event {event_id} already existed unexpectedly"
        )));
    }

    let outcome = project(&mut tx, event_id, created_at, &signed.unsigned.payload).await?;

    events_repo::advance_origin(&mut tx, &peer_id_bytes, next_sequence as i64, event_id).await?;

    tx.commit().await.map_err(oag_storage::StorageError::from)?;

    Ok((event_id, outcome))
}
