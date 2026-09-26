//! Durability visibility: what each known peer has told us, via its own
//! `hello` response, about how far it's gotten with every origin it knows
//! about. Self-reported and never independently verified — the same trust
//! level `/peers` discovery data already carries (see `oag-sync`'s
//! `discover_peers`) — this is a monitoring signal, not a security
//! boundary.
use sqlx::SqliteConnection;

use crate::error::{bytes_to_array, StorageError};
use crate::models::PeerKnownHeadRow;

pub struct KnownHead {
    pub peer_id: [u8; 32],
    pub sequence: i64,
    pub observed_at: i64,
}

fn row_to_known_head(row: PeerKnownHeadRow) -> Result<KnownHead, StorageError> {
    Ok(KnownHead {
        peer_id: bytes_to_array(&row.peer_id)?,
        sequence: row.sequence,
        observed_at: row.observed_at,
    })
}

/// Record (or update) what `peer_id` reported as its own head sequence for
/// `origin_peer_id`. A later, lower value simply overwrites the earlier one
/// — there's no attempt to detect or reject a peer reporting regression,
/// consistent with this being informational rather than security-critical.
pub async fn upsert_known_head(
    conn: &mut SqliteConnection,
    peer_id: &[u8; 32],
    origin_peer_id: &[u8; 32],
    sequence: i64,
    observed_at: i64,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO peer_known_heads (peer_id, origin_peer_id, sequence, observed_at) \
         VALUES (?, ?, ?, ?) \
         ON CONFLICT(peer_id, origin_peer_id) DO UPDATE SET \
            sequence = excluded.sequence, observed_at = excluded.observed_at",
    )
    .bind(peer_id.to_vec())
    .bind(origin_peer_id.to_vec())
    .bind(sequence)
    .bind(observed_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Every known peer's last-reported head for `origin_peer_id`, one row per
/// peer that has ever reported anything about it.
pub async fn list_known_heads_for_origin(
    conn: &mut SqliteConnection,
    origin_peer_id: &[u8; 32],
) -> Result<Vec<KnownHead>, StorageError> {
    let rows: Vec<PeerKnownHeadRow> =
        sqlx::query_as("SELECT * FROM peer_known_heads WHERE origin_peer_id = ?")
            .bind(origin_peer_id.to_vec())
            .fetch_all(&mut *conn)
            .await?;
    rows.into_iter().map(row_to_known_head).collect()
}
