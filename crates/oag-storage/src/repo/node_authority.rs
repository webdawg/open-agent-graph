//! Thin, fully-rebuildable cache for the `authority` ranking signal (spec
//! section 65) — `oag_graph::authority::recompute_authority` is the only
//! writer, replacing the whole table each time it runs.
use oag_core::NodeId;
use sqlx::SqliteConnection;

use crate::error::StorageError;
use crate::models::NodeAuthorityRow;

/// Replace this node's stored score (insert or overwrite) — callers doing a
/// full recompute call this once per node in the current graph.
pub async fn upsert(
    conn: &mut SqliteConnection,
    node_id: NodeId,
    score: f32,
    computed_at: i64,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO node_authority (node_id, score, computed_at) VALUES (?, ?, ?) \
         ON CONFLICT(node_id) DO UPDATE SET score = excluded.score, computed_at = excluded.computed_at",
    )
    .bind(node_id.as_hash().as_bytes().to_vec())
    .bind(score as f64)
    .bind(computed_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Drop every stored score — called at the start of a full recompute so a
/// node that dropped out of the graph (all its edges gone) doesn't keep a
/// stale score forever.
pub async fn clear_all(conn: &mut SqliteConnection) -> Result<(), StorageError> {
    sqlx::query("DELETE FROM node_authority").execute(&mut *conn).await?;
    Ok(())
}

pub async fn get(conn: &mut SqliteConnection, node_id: NodeId) -> Result<Option<f32>, StorageError> {
    let row: Option<NodeAuthorityRow> = sqlx::query_as("SELECT * FROM node_authority WHERE node_id = ?")
        .bind(node_id.as_hash().as_bytes().to_vec())
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.map(|r| r.score as f32))
}

