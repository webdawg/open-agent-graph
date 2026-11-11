//! CIDs this peer has pinned, from every path that records one.
//!
//! One row per CID, keyed by the CID itself, so the discovery surface (`GET /api/v1/pins`) is a
//! indexed lookup rather than a scan over `evidence.metadata`.
//!
//! `source` says which path recorded it -- `evidence` (the assert/evidence path, which is how the
//! crawler's page pin lands), `cli-pin`, `backup-pin`, or `host-page`. It is descriptive metadata
//! for an operator, never a permission or trust signal: a CID pinned by a script is not more
//! trustworthy than one pinned by a person.

use sqlx::SqliteConnection;

use crate::error::StorageError;

/// How many CIDs `list` returns when the caller does not say.
pub const DEFAULT_LIMIT: i64 = 100;

/// Upper bound on `list`, matching `GraphService::search`'s and `list_pins`'s cap.
pub const MAX_LIMIT: i64 = 1000;

/// Records that this peer pinned `cid`. Idempotent per CID: re-recording updates the timestamp and
/// keeps the first `source`, because "what did this peer hold and roughly when" should not change
/// just because it pinned the same bytes again.
pub async fn record(
    conn: &mut SqliteConnection,
    cid: &str,
    source: &str,
    pinned_at: i64,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO pinned_cids (cid, pinned_at, source) VALUES (?, ?, ?) \
         ON CONFLICT(cid) DO UPDATE SET pinned_at = excluded.pinned_at",
    )
    .bind(cid)
    .bind(pinned_at)
    .bind(source)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// CIDs this peer has pinned, most recent first, bounded by `limit`.
pub async fn list(conn: &mut SqliteConnection, limit: i64) -> Result<Vec<(String, String)>, StorageError> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT cid, source FROM pinned_cids ORDER BY pinned_at DESC, cid ASC LIMIT ?")
            .bind(limit)
            .fetch_all(&mut *conn)
            .await?;
    Ok(rows)
}

/// Whether this peer has pinned `cid`.
pub async fn holds(conn: &mut SqliteConnection, cid: &str) -> Result<bool, StorageError> {
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM pinned_cids WHERE cid = ? LIMIT 1")
            .bind(cid)
            .fetch_one(&mut *conn)
            .await?;
    Ok(count > 0)
}

/// How many distinct CIDs this peer has pinned.
pub async fn count(conn: &mut SqliteConnection) -> Result<i64, StorageError> {
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM pinned_cids")
        .fetch_one(&mut *conn)
        .await?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::open_pool;

    fn temp_db_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oag-storage-pinned-cids-test-{name}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("oag.sqlite")
    }

    #[tokio::test]
    async fn recording_is_idempotent_per_cid_and_lists_most_recent_first() {
        let pool = open_pool(&temp_db_path("basic")).await.unwrap();
        let mut conn = pool.acquire().await.unwrap();

        assert_eq!(count(&mut conn).await.unwrap(), 0);
        assert!(!holds(&mut conn, "QmAlpha").await.unwrap());

        record(&mut conn, "QmAlpha", "evidence", 100).await.unwrap();
        record(&mut conn, "QmBeta", "cli-pin", 200).await.unwrap();
        record(&mut conn, "QmAlpha", "cli-pin", 300).await.unwrap(); // re-pin the same CID

        assert_eq!(count(&mut conn).await.unwrap(), 2, "one row per CID");
        assert!(holds(&mut conn, "QmAlpha").await.unwrap());
        assert!(holds(&mut conn, "QmBeta").await.unwrap());
        assert!(!holds(&mut conn, "QmGamma").await.unwrap());

        let listed = list(&mut conn, 10).await.unwrap();
        assert_eq!(listed.len(), 2);
        // QmAlpha's timestamp was updated by the re-pin, so it is now the most recent.
        assert_eq!(listed[0].0, "QmAlpha");
        assert_eq!(listed[1].0, "QmBeta");
    }

    #[tokio::test]
    async fn the_limit_is_honoured() {
        let pool = open_pool(&temp_db_path("limit")).await.unwrap();
        let mut conn = pool.acquire().await.unwrap();
        for i in 0..5 {
            record(&mut conn, &format!("Qm{i}"), "cli-pin", i).await.unwrap();
        }
        assert_eq!(list(&mut conn, 2).await.unwrap().len(), 2);
    }

    /// The migration's backfill is the whole reason a long-running peer does not report an empty
    /// pin list after upgrading, so it is tested directly rather than assumed.
    #[tokio::test]
    async fn the_backfill_is_idempotent() {
        let pool = open_pool(&temp_db_path("backfill")).await.unwrap();
        let mut conn = pool.acquire().await.unwrap();

        // Running the backfill statement twice must not duplicate rows -- `OR IGNORE` plus the
        // primary key is what makes that true.
        for _ in 0..2 {
            sqlx::query(
                "INSERT OR IGNORE INTO pinned_cids (cid, pinned_at, source) \
                 SELECT json_extract(metadata, '$.ipfs_cid'), MAX(COALESCE(retrieved_at, 0)), 'evidence' \
                 FROM evidence WHERE json_extract(metadata, '$.ipfs_cid') IS NOT NULL \
                 GROUP BY json_extract(metadata, '$.ipfs_cid')",
            )
            .execute(&mut *conn)
            .await
            .unwrap();
        }
        assert_eq!(count(&mut conn).await.unwrap(), 0, "no evidence seeded, so nothing to backfill");
    }
}
