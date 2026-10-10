//! Storage for ephemeral peer trust (Phase 1): per-identity-lifetime
//! presence continuity (`peer_presence`) and the address-correlated
//! "announced restart" grace window (`address_going_offline`). See
//! `spec/24-ephemeral-peer-trust.md` for why these are two separate tables
//! rather than one.

use sqlx::SqliteConnection;

use crate::error::StorageError;
use crate::models::{AddressGoingOfflineRow, PeerPresenceRow};

pub async fn get(conn: &mut SqliteConnection, peer_id: &[u8; 32]) -> Result<Option<PeerPresenceRow>, StorageError> {
    let row = sqlx::query_as("SELECT * FROM peer_presence WHERE peer_id = ?")
        .bind(peer_id.to_vec())
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row)
}

/// Records a received heartbeat. `session_started_at` is only set on the
/// *first* heartbeat seen for a given `peer_id` (an ephemeral identity's
/// lifetime begins once) — later calls update `last_heartbeat_at`/`status`
/// without resetting it, which is exactly what lets a receiver compute
/// continuous-presence duration from this row alone.
pub async fn record_heartbeat(
    conn: &mut SqliteConnection,
    peer_id: &[u8; 32],
    session_started_at: i64,
    heartbeat_at: i64,
    status: &str,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO peer_presence (peer_id, session_started_at, last_heartbeat_at, status) \
         VALUES (?, ?, ?, ?) \
         ON CONFLICT(peer_id) DO UPDATE SET last_heartbeat_at = excluded.last_heartbeat_at, \
         status = excluded.status",
    )
    .bind(peer_id.to_vec())
    .bind(session_started_at)
    .bind(heartbeat_at)
    .bind(status)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn get_address_going_offline(
    conn: &mut SqliteConnection,
    address: &str,
) -> Result<Option<AddressGoingOfflineRow>, StorageError> {
    let row = sqlx::query_as("SELECT * FROM address_going_offline WHERE address = ?")
        .bind(address)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row)
}

pub async fn record_going_offline(
    conn: &mut SqliteConnection,
    address: &str,
    from_peer_id: &[u8; 32],
    announced_at: i64,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO address_going_offline (address, announced_at, from_peer_id) VALUES (?, ?, ?) \
         ON CONFLICT(address) DO UPDATE SET announced_at = excluded.announced_at, \
         from_peer_id = excluded.from_peer_id",
    )
    .bind(address)
    .bind(announced_at)
    .bind(from_peer_id.to_vec())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::open_pool;

    fn temp_db_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oag-storage-peer-presence-test-{name}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("oag.sqlite")
    }

    #[tokio::test]
    async fn first_heartbeat_sets_session_started_at_and_later_ones_dont_reset_it() {
        let pool = open_pool(&temp_db_path("heartbeat")).await.unwrap();
        let mut conn = pool.acquire().await.unwrap();
        let peer_id = [3u8; 32];

        record_heartbeat(&mut conn, &peer_id, 1_000, 1_000, "online").await.unwrap();
        record_heartbeat(&mut conn, &peer_id, 1_030, 1_030, "online").await.unwrap();

        let row = get(&mut conn, &peer_id).await.unwrap().unwrap();
        assert_eq!(row.session_started_at, 1_000, "session start must not reset on later heartbeats");
        assert_eq!(row.last_heartbeat_at, 1_030);
        assert_eq!(row.status, "online");
    }

    #[tokio::test]
    async fn going_offline_announcement_is_keyed_by_address_not_peer_id() {
        let pool = open_pool(&temp_db_path("going-offline")).await.unwrap();
        let mut conn = pool.acquire().await.unwrap();
        let old_peer_id = [5u8; 32];

        record_going_offline(&mut conn, "127.0.0.1:9001", &old_peer_id, 2_000).await.unwrap();
        let row = get_address_going_offline(&mut conn, "127.0.0.1:9001").await.unwrap().unwrap();
        assert_eq!(row.announced_at, 2_000);
        assert_eq!(row.from_peer_id, old_peer_id.to_vec());

        assert!(get_address_going_offline(&mut conn, "127.0.0.1:9002").await.unwrap().is_none());
    }
}
