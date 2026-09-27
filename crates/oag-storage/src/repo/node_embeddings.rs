//! Storage for the `node_embeddings` cache table (spec section 64). Like
//! `node_authority`, this is a thin, fully-rebuildable projection: the only
//! writer is `oag_graph`'s embeddings recompute, which clears and
//! regenerates the whole table from current node content. There is no ANN
//! index here — `list_all` hands back every row so the caller can brute-force
//! cosine similarity in Rust (v1 scope, see OPEN_QUESTIONS.md).

use oag_core::NodeId;
use sqlx::SqliteConnection;

use crate::error::{bytes_to_array, StorageError};
use crate::models::NodeEmbeddingRow;

fn encode_embedding(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn decode_embedding(bytes: &[u8]) -> Vec<f32> {
    let (chunks, _remainder) = bytes.as_chunks::<4>();
    chunks.iter().map(|&chunk| f32::from_le_bytes(chunk)).collect()
}

pub async fn upsert(
    conn: &mut SqliteConnection,
    node_id: NodeId,
    provider: &str,
    model: &str,
    embedding: &[f32],
    computed_at: i64,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO node_embeddings (node_id, provider, model, dim, embedding, computed_at) \
         VALUES (?, ?, ?, ?, ?, ?) \
         ON CONFLICT(node_id) DO UPDATE SET provider = excluded.provider, model = excluded.model, \
         dim = excluded.dim, embedding = excluded.embedding, computed_at = excluded.computed_at",
    )
    .bind(node_id.as_hash().as_bytes().to_vec())
    .bind(provider)
    .bind(model)
    .bind(embedding.len() as i64)
    .bind(encode_embedding(embedding))
    .bind(computed_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn clear_all(conn: &mut SqliteConnection) -> Result<(), StorageError> {
    sqlx::query("DELETE FROM node_embeddings").execute(&mut *conn).await?;
    Ok(())
}

pub async fn get(
    conn: &mut SqliteConnection,
    node_id: NodeId,
) -> Result<Option<Vec<f32>>, StorageError> {
    let row: Option<NodeEmbeddingRow> = sqlx::query_as("SELECT * FROM node_embeddings WHERE node_id = ?")
        .bind(node_id.as_hash().as_bytes().to_vec())
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.map(|r| decode_embedding(&r.embedding)))
}

/// Every stored embedding — the brute-force cosine similarity scan in
/// `oag_graph::semantic_search` needs the whole set, not a lookup.
pub async fn list_all(
    conn: &mut SqliteConnection,
) -> Result<Vec<(NodeId, Vec<f32>)>, StorageError> {
    let rows: Vec<NodeEmbeddingRow> =
        sqlx::query_as("SELECT * FROM node_embeddings").fetch_all(&mut *conn).await?;
    rows.into_iter()
        .map(|row| {
            let node_id = NodeId::from_hash(oag_core::Hash32::from_bytes(bytes_to_array(&row.node_id)?));
            Ok((node_id, decode_embedding(&row.embedding)))
        })
        .collect()
}
