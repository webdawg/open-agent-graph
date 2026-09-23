use oag_core::EventId;
use sqlx::SqliteConnection;

use crate::error::{bytes_to_array, StorageError};
use crate::models::{EventOriginRow, EventRow};

pub struct StoredEvent {
    pub event_id: EventId,
    pub origin_peer_id: [u8; 32],
    pub sequence: i64,
    pub previous_event_id: Option<EventId>,
    pub event_type: String,
    pub canonical_payload: Vec<u8>,
    pub created_at: i64,
    pub signature: Vec<u8>,
}

/// Insert the raw event row. Returns `false` if the event already existed
/// (duplicate delivery is harmless and MUST be idempotent — spec section 53).
pub async fn insert_event(
    conn: &mut SqliteConnection,
    event: &StoredEvent,
    received_at: i64,
) -> Result<bool, StorageError> {
    let result = sqlx::query(
        "INSERT OR IGNORE INTO events \
         (event_id, origin_peer_id, sequence, previous_event_id, event_type, canonical_payload, created_at, signature, received_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(event.event_id.as_hash().as_bytes().to_vec())
    .bind(event.origin_peer_id.to_vec())
    .bind(event.sequence)
    .bind(event.previous_event_id.map(|id| id.as_hash().as_bytes().to_vec()))
    .bind(&event.event_type)
    .bind(&event.canonical_payload)
    .bind(event.created_at)
    .bind(&event.signature)
    .bind(received_at)
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn event_exists(
    conn: &mut SqliteConnection,
    event_id: EventId,
) -> Result<bool, StorageError> {
    let row: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM events WHERE event_id = ?")
        .bind(event_id.as_hash().as_bytes().to_vec())
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.is_some())
}

pub async fn get_origin(
    conn: &mut SqliteConnection,
    origin_peer_id: &[u8; 32],
) -> Result<Option<EventOriginRow>, StorageError> {
    let row: Option<EventOriginRow> =
        sqlx::query_as("SELECT * FROM event_origins WHERE origin_peer_id = ?")
            .bind(origin_peer_id.to_vec())
            .fetch_optional(&mut *conn)
            .await?;
    Ok(row)
}

/// Advance this origin's head after a new event is durably stored in the
/// same transaction (spec section 41).
pub async fn advance_origin(
    conn: &mut SqliteConnection,
    origin_peer_id: &[u8; 32],
    new_sequence: i64,
    new_head: EventId,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO event_origins (origin_peer_id, highest_contiguous_sequence, highest_seen_sequence, head_event_id) \
         VALUES (?, ?, ?, ?) \
         ON CONFLICT(origin_peer_id) DO UPDATE SET \
            highest_contiguous_sequence = excluded.highest_contiguous_sequence, \
            highest_seen_sequence = excluded.highest_seen_sequence, \
            head_event_id = excluded.head_event_id",
    )
    .bind(origin_peer_id.to_vec())
    .bind(new_sequence)
    .bind(new_sequence)
    .bind(new_head.as_hash().as_bytes().to_vec())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Record that `event_id` touches `ref_type`/`ref_id` (e.g. `"assertion"`,
/// the assertion's id), so `/history/{object_type}/{id}` can find it without
/// scanning payload blobs.
pub async fn add_ref(
    conn: &mut SqliteConnection,
    event_id: EventId,
    ref_type: &str,
    ref_id: &[u8; 32],
) -> Result<(), StorageError> {
    sqlx::query("INSERT OR IGNORE INTO event_refs (event_id, ref_type, ref_id) VALUES (?, ?, ?)")
        .bind(event_id.as_hash().as_bytes().to_vec())
        .bind(ref_type)
        .bind(ref_id.to_vec())
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// All events referencing `ref_id` (of type `ref_type`), oldest first —
/// backs the history API.
pub async fn history_for(
    conn: &mut SqliteConnection,
    ref_type: &str,
    ref_id: &[u8; 32],
) -> Result<Vec<EventRow>, StorageError> {
    let rows: Vec<EventRow> = sqlx::query_as(
        "SELECT events.* FROM events \
         JOIN event_refs ON event_refs.event_id = events.event_id \
         WHERE event_refs.ref_type = ? AND event_refs.ref_id = ? \
         ORDER BY events.origin_peer_id, events.sequence",
    )
    .bind(ref_type)
    .bind(ref_id.to_vec())
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows)
}

pub fn event_row_to_id(row: &EventRow) -> Result<EventId, StorageError> {
    Ok(EventId::from_hash(oag_core::Hash32::from_bytes(bytes_to_array(
        &row.event_id,
    )?)))
}
