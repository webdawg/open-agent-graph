//! Storage for peer clustering, Phase 1 (spec/26): a peer's latest
//! self-declared, signed cluster claim. Storage only -- building and
//! verifying the signed declaration itself is `oag_sync::cluster`'s job,
//! the same split as every other repo module in this project.

use sqlx::SqliteConnection;

use crate::error::StorageError;
use crate::models::PeerClusterClaimRow;

pub async fn get(conn: &mut SqliteConnection, peer_id: &[u8; 32]) -> Result<Option<PeerClusterClaimRow>, StorageError> {
    let row = sqlx::query_as("SELECT * FROM peer_cluster_claims WHERE peer_id = ?")
        .bind(peer_id.to_vec())
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row)
}

/// A plain upsert, unlike `node_gravity`'s `INSERT OR IGNORE` -- a claimed
/// cluster is current status, so a later declaration legitimately
/// overwrites an earlier one (see this migration's own doc comment).
pub async fn upsert(
    conn: &mut SqliteConnection,
    peer_id: &[u8; 32],
    cluster: &str,
    declared_at: i64,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO peer_cluster_claims (peer_id, cluster, declared_at) VALUES (?, ?, ?) \
         ON CONFLICT(peer_id) DO UPDATE SET cluster = excluded.cluster, declared_at = excluded.declared_at",
    )
    .bind(peer_id.to_vec())
    .bind(cluster)
    .bind(declared_at)
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
            "oag-storage-peer-cluster-test-{name}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("oag.sqlite")
    }

    #[tokio::test]
    async fn get_is_none_until_a_claim_is_declared() {
        let pool = open_pool(&temp_db_path("missing")).await.unwrap();
        let mut conn = pool.acquire().await.unwrap();
        let peer_id = [7u8; 32];

        assert!(get(&mut conn, &peer_id).await.unwrap().is_none());

        upsert(&mut conn, &peer_id, "home-lab", 1_000).await.unwrap();
        let row = get(&mut conn, &peer_id).await.unwrap().unwrap();
        assert_eq!(row.cluster, "home-lab");
        assert_eq!(row.declared_at, 1_000);
    }

    /// The opposite of `node_gravity`'s stability guarantee: a claimed
    /// cluster is current status, so a later declaration must win.
    #[tokio::test]
    async fn a_later_declaration_overwrites_an_earlier_one() {
        let pool = open_pool(&temp_db_path("overwrite")).await.unwrap();
        let mut conn = pool.acquire().await.unwrap();
        let peer_id = [8u8; 32];

        upsert(&mut conn, &peer_id, "home-lab", 1_000).await.unwrap();
        upsert(&mut conn, &peer_id, "other-site", 2_000).await.unwrap();

        let row = get(&mut conn, &peer_id).await.unwrap().unwrap();
        assert_eq!(row.cluster, "other-site", "a later declaration must overwrite the earlier claim");
        assert_eq!(row.declared_at, 2_000);
    }
}
