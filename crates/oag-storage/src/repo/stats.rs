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
/// Events whose `previous_event_id` is set but whose predecessor is not present locally.
///
/// A non-zero value means this peer holds a fragment of some peer's chain without the event that
/// precedes it -- the normal state while replication is still catching up, and a *persistent*
/// non-zero value means a predecessor is missing for good (the origin never sent it, or it was
/// pruned somewhere upstream). That distinction is not visible in `events_total`, which counts
/// rows and cannot tell a complete chain from a pile of fragments.
///
/// The self-join is on the primary key, so it is `O(n log n)` rather than a scan-compare-scan.
pub async fn events_pending_chain(conn: &mut SqliteConnection) -> Result<i64, StorageError> {
    count(
        conn,
        "SELECT COUNT(*) FROM events e \
         LEFT JOIN events p ON p.event_id = e.previous_event_id \
         WHERE e.previous_event_id IS NOT NULL AND p.event_id IS NULL",
    )
    .await
}

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

    /// The metric's whole purpose is the difference between "a complete chain" and "a pile of
    /// fragments", which `events_total` cannot see. Inserted directly rather than through the event
    /// log, because a fragment is exactly the state the normal ingest path never produces.
    #[tokio::test]
    async fn pending_chain_counts_events_whose_predecessor_is_missing() {
        let pool = open_pool(&temp_db_path("pending-chain")).await.unwrap();
        let mut conn = pool.acquire().await.unwrap();

        // `events` has a UNIQUE(origin_peer_id, sequence), so each row needs its own origin to
        // stand in for a different peer's chain.
        let insert = |event_tag: &[u8], origin: u8, prev: Option<Vec<u8>>| {
            sqlx::query(
                "INSERT INTO events (event_id, origin_peer_id, sequence, previous_event_id, event_type, canonical_payload, created_at, signature, received_at) \
                 VALUES (?, ?, 1, ?, 'ADD_EVIDENCE', X'00', 1, X'00', 1)",
            )
            .bind(oag_core::EventId::derive(event_tag).as_hash().as_bytes().to_vec())
            .bind(vec![origin; 32])
            .bind(prev)
        };

        // A genesis event has no predecessor by definition, so it is never pending.
        insert(b"stats-pending-genesis", 1, None).execute(&mut *conn).await.unwrap();
        assert_eq!(events_pending_chain(&mut conn).await.unwrap(), 0);

        // A child whose predecessor IS present is a complete chain, not a fragment.
        insert(
            b"stats-pending-child",
            2,
            Some(oag_core::EventId::derive(b"stats-pending-genesis").as_hash().as_bytes().to_vec()),
        )
            .execute(&mut *conn)
            .await
            .unwrap();
        assert_eq!(events_pending_chain(&mut conn).await.unwrap(), 0);

        // A child whose predecessor will never arrive is a dangling fragment.
        insert(
            b"stats-pending-dangling",
            3,
            Some(oag_core::EventId::derive(b"never-arrives").as_hash().as_bytes().to_vec()),
        )
            .execute(&mut *conn)
            .await
            .unwrap();
        assert_eq!(events_pending_chain(&mut conn).await.unwrap(), 1);
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
