//! Storage for the "ant memory node" tensor pad (one small, persistent
//! vector per peer, keyed by that peer's own `peer_id`). The only writer in
//! v1 is `oag_sync`, nudging its own pad after each sync round; `oag-brain`
//! (outside this workspace) is the only other reader/writer, exchanging and
//! reprogramming pads via real attention.

use sqlx::SqliteConnection;

use crate::error::StorageError;
use crate::models::TensorPadRow;

fn encode_values(values: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn decode_values(bytes: &[u8]) -> Vec<f32> {
    let (chunks, _remainder) = bytes.as_chunks::<4>();
    chunks.iter().map(|&chunk| f32::from_le_bytes(chunk)).collect()
}

pub async fn get(
    conn: &mut SqliteConnection,
    peer_id: &[u8; 32],
) -> Result<Option<Vec<f32>>, StorageError> {
    let row: Option<TensorPadRow> = sqlx::query_as("SELECT * FROM tensor_pads WHERE peer_id = ?")
        .bind(peer_id.to_vec())
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.map(|r| decode_values(&r.pad_values)))
}

pub async fn upsert(
    conn: &mut SqliteConnection,
    peer_id: &[u8; 32],
    values: &[f32],
    updated_at: i64,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO tensor_pads (peer_id, pad_values, updated_at) VALUES (?, ?, ?) \
         ON CONFLICT(peer_id) DO UPDATE SET pad_values = excluded.pad_values, \
         updated_at = excluded.updated_at",
    )
    .bind(peer_id.to_vec())
    .bind(encode_values(values))
    .bind(updated_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}
