//! Spec section 82: a safe, consistent whole-database snapshot. Copying
//! `oag.sqlite`/`-wal`/`-shm` file-by-file while a server is running can
//! capture an inconsistent mid-write state — `VACUUM INTO` is SQLite's own
//! sanctioned way to produce one complete, compacted, consistent snapshot
//! file safely, even with other connections open against the live database
//! (unlike plain `VACUUM`, which needs exclusive access).
use std::path::Path;

use sqlx::SqlitePool;

use crate::error::StorageError;

pub async fn backup_to(pool: &SqlitePool, out_path: &Path) -> Result<(), StorageError> {
    let out_path = out_path.to_string_lossy().into_owned();
    sqlx::query("VACUUM INTO ?").bind(out_path).execute(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::open_pool;
    use crate::repo;
    use oag_core::{Node, NodeType};

    fn temp_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oag-storage-backup-test-{name}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn backup_produces_a_complete_openable_snapshot() {
        let source_dir = temp_path("source");
        let pool = open_pool(&source_dir.join("oag.sqlite")).await.unwrap();
        {
            let mut conn = pool.acquire().await.unwrap();
            let node = Node::new(NodeType::concept(), "concept:backuptest", 1);
            repo::nodes::insert_if_missing(&mut conn, &node).await.unwrap();
        }

        let backup_dir = temp_path("backup");
        let backup_path = backup_dir.join("snapshot.sqlite");
        backup_to(&pool, &backup_path).await.unwrap();

        assert!(backup_path.exists());

        // Open the snapshot as a completely independent database and
        // confirm it actually has the data, not just that a file exists.
        let restored_pool = open_pool(&backup_path).await.unwrap();
        let mut conn = restored_pool.acquire().await.unwrap();
        let found = repo::nodes::search(&mut conn, "backuptest", 10).await.unwrap();
        assert_eq!(found.len(), 1, "snapshot should contain the same data as the live database");
    }
}
