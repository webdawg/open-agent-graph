//! Spec section 86 (Metrics): cheap, correct-today `COUNT(*)`/`PRAGMA`
//! queries backing `GraphService::metrics_snapshot`. Deliberately just the
//! subset of section 86's suggested measurements that are simple current-state
//! reads over tables that already exist — see the repo root's
//! `OPEN_QUESTIONS.md` "Metrics" section for what's deferred and why (rate/
//! labeled/histogram metrics need real instrumentation, not a snapshot query).
use sqlx::SqliteConnection;

use crate::error::StorageError;

async fn count(conn: &mut SqliteConnection, sql: &'static str) -> Result<i64, StorageError> {
    let (count,): (i64,) = sqlx::query_as(sql).fetch_one(&mut *conn).await?;
    Ok(count)
}

pub async fn events_total(conn: &mut SqliteConnection) -> Result<i64, StorageError> {
    count(conn, "SELECT COUNT(*) FROM events").await
}

pub async fn nodes_total(conn: &mut SqliteConnection) -> Result<i64, StorageError> {
    count(conn, "SELECT COUNT(*) FROM nodes").await
}

pub async fn edges_total(conn: &mut SqliteConnection) -> Result<i64, StorageError> {
    count(conn, "SELECT COUNT(*) FROM edges").await
}

pub async fn assertions_total(conn: &mut SqliteConnection) -> Result<i64, StorageError> {
    count(conn, "SELECT COUNT(*) FROM assertions").await
}

pub async fn evidence_total(conn: &mut SqliteConnection) -> Result<i64, StorageError> {
    count(conn, "SELECT COUNT(*) FROM evidence").await
}

pub async fn peer_count(conn: &mut SqliteConnection) -> Result<i64, StorageError> {
    count(conn, "SELECT COUNT(*) FROM peers").await
}

/// Size in bytes of the primary SQLite database file, computed as
/// `page_count * page_size` rather than `stat()`-ing the file path directly —
/// correct in WAL mode (where committed-but-not-checkpointed data can live in
/// the separate `-wal` file, and a raw file size would undercount it) and
/// needs no db file path threaded through `GraphService`/`AppState`, since
/// both PRAGMAs are ordinary queries against the existing pool.
pub async fn sqlite_size_bytes(conn: &mut SqliteConnection) -> Result<i64, StorageError> {
    let page_count = count(conn, "PRAGMA page_count").await?;
    let page_size = count(conn, "PRAGMA page_size").await?;
    Ok(page_count * page_size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::open_pool;
    use oag_core::{Node, NodeType};

    fn temp_db_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oag-storage-stats-test-{name}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("oag.sqlite")
    }

    #[tokio::test]
    async fn counts_reflect_inserted_rows() {
        let pool = open_pool(&temp_db_path("counts")).await.unwrap();
        let mut conn = pool.acquire().await.unwrap();

        assert_eq!(nodes_total(&mut conn).await.unwrap(), 0);
        assert_eq!(edges_total(&mut conn).await.unwrap(), 0);
        assert_eq!(peer_count(&mut conn).await.unwrap(), 0);

        let node = Node::new(NodeType::concept(), "concept:stats-test", 1);
        crate::repo::nodes::insert_if_missing(&mut conn, &node).await.unwrap();
        assert_eq!(nodes_total(&mut conn).await.unwrap(), 1);

        crate::repo::peers::upsert_peer(&mut conn, &[7u8; 32], &[9u8; 32], Some("peer-a"), 1)
            .await
            .unwrap();
        assert_eq!(peer_count(&mut conn).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn sqlite_size_is_positive_after_migrations() {
        // Even an empty (freshly migrated) database has a nonzero page count
        // -- the schema itself occupies pages.
        let pool = open_pool(&temp_db_path("size")).await.unwrap();
        let mut conn = pool.acquire().await.unwrap();
        assert!(sqlite_size_bytes(&mut conn).await.unwrap() > 0);
    }
}
