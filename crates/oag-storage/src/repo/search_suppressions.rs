//! Storage for spec section 85's search suppression -- a lesser, fully
//! reversible intervention than evidence redaction (`crate::repo::
//! redactions`): the node itself, its edges, and its assertions are
//! untouched, it's just excluded from `nodes::search`'s results. See
//! `crate::repo::rebuild`'s doc comment for why this table is excluded from
//! `clear_projection_tables`'s wipe list.

use oag_core::NodeId;
use sqlx::SqliteConnection;

use crate::error::StorageError;

pub async fn suppress(
    conn: &mut SqliteConnection,
    node_id: NodeId,
    suppressed_at: i64,
) -> Result<(), StorageError> {
    sqlx::query("INSERT OR IGNORE INTO search_suppressions (node_id, suppressed_at) VALUES (?, ?)")
        .bind(node_id.as_hash().as_bytes().to_vec())
        .bind(suppressed_at)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

pub async fn unsuppress(conn: &mut SqliteConnection, node_id: NodeId) -> Result<(), StorageError> {
    sqlx::query("DELETE FROM search_suppressions WHERE node_id = ?")
        .bind(node_id.as_hash().as_bytes().to_vec())
        .execute(&mut *conn)
        .await?;
    Ok(())
}

pub async fn is_suppressed(conn: &mut SqliteConnection, node_id: NodeId) -> Result<bool, StorageError> {
    let row: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM search_suppressions WHERE node_id = ?")
        .bind(node_id.as_hash().as_bytes().to_vec())
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::open_pool;
    use oag_core::{Node, NodeType};

    fn temp_db_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oag-storage-search-suppressions-test-{name}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("oag.sqlite")
    }

    #[tokio::test]
    async fn suppress_unsuppress_round_trip() {
        let pool = open_pool(&temp_db_path("basic")).await.unwrap();
        let mut conn = pool.acquire().await.unwrap();

        let node = Node::new(NodeType::concept(), "concept:suppress-test", 1);
        crate::repo::nodes::insert_if_missing(&mut conn, &node).await.unwrap();

        assert!(!is_suppressed(&mut conn, node.id).await.unwrap());
        suppress(&mut conn, node.id, 42).await.unwrap();
        assert!(is_suppressed(&mut conn, node.id).await.unwrap());

        // Suppressing twice is a no-op, not an error (INSERT OR IGNORE).
        suppress(&mut conn, node.id, 99).await.unwrap();

        unsuppress(&mut conn, node.id).await.unwrap();
        assert!(!is_suppressed(&mut conn, node.id).await.unwrap());
    }
}
