//! Storage for the gravity layer (spec/25): one persisted, truly-random
//! `gravity_level` per peer. This module only stores and retrieves --
//! rolling a fresh value with a strong RNG when none exists yet is
//! `oag_sync::gravity`'s job, the same split as `tensor_pads.rs` (storage)
//! vs. `oag_sync::service::observe_sync_outcome` (the logic that decides
//! what to store).

use sqlx::SqliteConnection;

use crate::error::StorageError;

pub async fn get(conn: &mut SqliteConnection, peer_id: &[u8; 32]) -> Result<Option<f32>, StorageError> {
    let row: Option<(f64,)> = sqlx::query_as("SELECT gravity_level FROM node_gravity WHERE peer_id = ?")
        .bind(peer_id.to_vec())
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.map(|(level,)| level as f32))
}

/// `INSERT OR IGNORE`, not an upsert: once rolled, a node's gravity level
/// never changes underneath it -- a second call (e.g. a concurrent first
/// use racing with itself) must not silently reroll an already-persisted
/// value.
pub async fn insert_if_missing(
    conn: &mut SqliteConnection,
    peer_id: &[u8; 32],
    gravity_level: f32,
    generated_at: i64,
) -> Result<(), StorageError> {
    sqlx::query("INSERT OR IGNORE INTO node_gravity (peer_id, gravity_level, generated_at) VALUES (?, ?, ?)")
        .bind(peer_id.to_vec())
        .bind(gravity_level as f64)
        .bind(generated_at)
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
            "oag-storage-node-gravity-test-{name}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("oag.sqlite")
    }

    #[tokio::test]
    async fn get_is_none_until_a_value_is_inserted() {
        let pool = open_pool(&temp_db_path("missing")).await.unwrap();
        let mut conn = pool.acquire().await.unwrap();
        let peer_id = [1u8; 32];

        assert_eq!(get(&mut conn, &peer_id).await.unwrap(), None);

        insert_if_missing(&mut conn, &peer_id, 0.42, 1_000).await.unwrap();
        let level = get(&mut conn, &peer_id).await.unwrap().unwrap();
        assert!((level - 0.42).abs() < 1e-6);
    }

    #[tokio::test]
    async fn a_second_insert_never_overwrites_the_first() {
        let pool = open_pool(&temp_db_path("stable")).await.unwrap();
        let mut conn = pool.acquire().await.unwrap();
        let peer_id = [2u8; 32];

        insert_if_missing(&mut conn, &peer_id, 0.1, 1_000).await.unwrap();
        insert_if_missing(&mut conn, &peer_id, 0.9, 2_000).await.unwrap();

        let level = get(&mut conn, &peer_id).await.unwrap().unwrap();
        assert!((level - 0.1).abs() < 1e-6, "a node's gravity level must never change once rolled, got {level}");
    }
}
