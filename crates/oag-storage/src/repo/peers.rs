use sqlx::SqliteConnection;

use crate::error::{bytes_to_array, StorageError};
use crate::models::{PeerAddressRow, PeerForkRow, PeerRow};

pub struct PeerInfo {
    pub peer_id: [u8; 32],
    pub public_key: [u8; 32],
    pub name: Option<String>,
    pub first_seen: i64,
    pub last_seen: Option<i64>,
    pub forked: bool,
}

fn row_to_info(row: PeerRow) -> Result<PeerInfo, StorageError> {
    Ok(PeerInfo {
        peer_id: bytes_to_array(&row.peer_id)?,
        public_key: bytes_to_array(&row.public_key)?,
        name: row.name,
        first_seen: row.first_seen,
        last_seen: row.last_seen,
        forked: row.forked,
    })
}

/// Learn about (or refresh) a peer. `public_key` MUST be the key this peer
/// itself presented (e.g. via `hello`) — it is never overwritten by a
/// differing key from an untrusted source (see [`upsert_peer`] semantics: an
/// existing row's `public_key` is preserved on conflict, since a changing
/// public key for the same peer id would itself be a red flag, not a normal
/// update).
pub async fn upsert_peer(
    conn: &mut SqliteConnection,
    peer_id: &[u8; 32],
    public_key: &[u8; 32],
    name: Option<&str>,
    seen_at: i64,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO peers (peer_id, public_key, name, first_seen, last_seen, forked) \
         VALUES (?, ?, ?, ?, ?, 0) \
         ON CONFLICT(peer_id) DO UPDATE SET last_seen = excluded.last_seen, \
            name = COALESCE(excluded.name, peers.name)",
    )
    .bind(peer_id.to_vec())
    .bind(public_key.to_vec())
    .bind(name)
    .bind(seen_at)
    .bind(seen_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn get_peer(
    conn: &mut SqliteConnection,
    peer_id: &[u8; 32],
) -> Result<Option<PeerInfo>, StorageError> {
    let row: Option<PeerRow> = sqlx::query_as("SELECT * FROM peers WHERE peer_id = ?")
        .bind(peer_id.to_vec())
        .fetch_optional(&mut *conn)
        .await?;
    row.map(row_to_info).transpose()
}

pub async fn list_peers(conn: &mut SqliteConnection) -> Result<Vec<PeerInfo>, StorageError> {
    let rows: Vec<PeerRow> = sqlx::query_as("SELECT * FROM peers ORDER BY first_seen")
        .fetch_all(&mut *conn)
        .await?;
    rows.into_iter().map(row_to_info).collect()
}

pub async fn add_address(
    conn: &mut SqliteConnection,
    peer_id: &[u8; 32],
    address: &str,
) -> Result<(), StorageError> {
    sqlx::query("INSERT OR IGNORE INTO peer_addresses (peer_id, address) VALUES (?, ?)")
        .bind(peer_id.to_vec())
        .bind(address)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

pub async fn remove_peer(
    conn: &mut SqliteConnection,
    peer_id: &[u8; 32],
) -> Result<(), StorageError> {
    sqlx::query("DELETE FROM peer_addresses WHERE peer_id = ?")
        .bind(peer_id.to_vec())
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM peers WHERE peer_id = ?")
        .bind(peer_id.to_vec())
        .execute(&mut *conn)
        .await?;
    Ok(())
}

pub async fn list_addresses(
    conn: &mut SqliteConnection,
    peer_id: &[u8; 32],
) -> Result<Vec<String>, StorageError> {
    let rows: Vec<PeerAddressRow> = sqlx::query_as("SELECT * FROM peer_addresses WHERE peer_id = ?")
        .bind(peer_id.to_vec())
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows.into_iter().map(|r| r.address).collect())
}

pub async fn list_all_addresses(
    conn: &mut SqliteConnection,
) -> Result<Vec<PeerAddressRow>, StorageError> {
    let rows: Vec<PeerAddressRow> = sqlx::query_as("SELECT * FROM peer_addresses").fetch_all(&mut *conn).await?;
    Ok(rows)
}

pub async fn mark_forked(
    conn: &mut SqliteConnection,
    peer_id: &[u8; 32],
) -> Result<(), StorageError> {
    sqlx::query("UPDATE peers SET forked = 1 WHERE peer_id = ?")
        .bind(peer_id.to_vec())
        .execute(&mut *conn)
        .await?;
    Ok(())
}

pub async fn record_fork(
    conn: &mut SqliteConnection,
    peer_id: &[u8; 32],
    sequence: i64,
    event_id_a: &[u8; 32],
    event_id_b: &[u8; 32],
    detected_at: i64,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT OR IGNORE INTO peer_forks (peer_id, sequence, event_id_a, event_id_b, detected_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(peer_id.to_vec())
    .bind(sequence)
    .bind(event_id_a.to_vec())
    .bind(event_id_b.to_vec())
    .bind(detected_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn list_forks(
    conn: &mut SqliteConnection,
    peer_id: &[u8; 32],
) -> Result<Vec<PeerForkRow>, StorageError> {
    let rows: Vec<PeerForkRow> = sqlx::query_as("SELECT * FROM peer_forks WHERE peer_id = ?")
        .bind(peer_id.to_vec())
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows)
}
