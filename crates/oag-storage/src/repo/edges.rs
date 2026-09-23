use oag_core::{EdgeId, NodeId};
use sqlx::SqliteConnection;

use crate::error::{bytes_to_array, StorageError};
use crate::models::EdgeRow;

fn row_to_edge(row: EdgeRow) -> Result<oag_core::Edge, StorageError> {
    Ok(oag_core::Edge {
        id: EdgeId::from_hash(oag_core::Hash32::from_bytes(bytes_to_array(&row.edge_id)?)),
        subject: NodeId::from_hash(oag_core::Hash32::from_bytes(bytes_to_array(
            &row.subject_node_id,
        )?)),
        predicate: oag_core::Predicate::new(row.predicate),
        object: NodeId::from_hash(oag_core::Hash32::from_bytes(bytes_to_array(
            &row.object_node_id,
        )?)),
        created_at: row.created_at,
    })
}

pub async fn insert_if_missing(
    conn: &mut SqliteConnection,
    edge: &oag_core::Edge,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT OR IGNORE INTO edges (edge_id, subject_node_id, predicate, object_node_id, created_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(edge.id.as_hash().as_bytes().to_vec())
    .bind(edge.subject.as_hash().as_bytes().to_vec())
    .bind(edge.predicate.as_str())
    .bind(edge.object.as_hash().as_bytes().to_vec())
    .bind(edge.created_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn get_by_id(
    conn: &mut SqliteConnection,
    id: EdgeId,
) -> Result<Option<oag_core::Edge>, StorageError> {
    let row: Option<EdgeRow> = sqlx::query_as("SELECT * FROM edges WHERE edge_id = ?")
        .bind(id.as_hash().as_bytes().to_vec())
        .fetch_optional(&mut *conn)
        .await?;
    row.map(row_to_edge).transpose()
}

pub async fn list_by_subject(
    conn: &mut SqliteConnection,
    subject: NodeId,
) -> Result<Vec<oag_core::Edge>, StorageError> {
    let rows: Vec<EdgeRow> = sqlx::query_as("SELECT * FROM edges WHERE subject_node_id = ?")
        .bind(subject.as_hash().as_bytes().to_vec())
        .fetch_all(&mut *conn)
        .await?;
    rows.into_iter().map(row_to_edge).collect()
}

pub async fn list_by_object(
    conn: &mut SqliteConnection,
    object: NodeId,
) -> Result<Vec<oag_core::Edge>, StorageError> {
    let rows: Vec<EdgeRow> = sqlx::query_as("SELECT * FROM edges WHERE object_node_id = ?")
        .bind(object.as_hash().as_bytes().to_vec())
        .fetch_all(&mut *conn)
        .await?;
    rows.into_iter().map(row_to_edge).collect()
}

/// All edges touching `node` in either direction (used by the subgraph API).
pub async fn list_touching(
    conn: &mut SqliteConnection,
    node: NodeId,
) -> Result<Vec<oag_core::Edge>, StorageError> {
    let rows: Vec<EdgeRow> = sqlx::query_as(
        "SELECT * FROM edges WHERE subject_node_id = ? OR object_node_id = ?",
    )
    .bind(node.as_hash().as_bytes().to_vec())
    .bind(node.as_hash().as_bytes().to_vec())
    .fetch_all(&mut *conn)
    .await?;
    rows.into_iter().map(row_to_edge).collect()
}
