use oag_core::{NodeAlias, NodeId};
use serde_json::Value as JsonValue;
use sqlx::SqliteConnection;

use crate::error::{bytes_to_array, StorageError};
use crate::models::{NodeAliasRow, NodeRow};

fn parse_alias_type(s: &str) -> Result<oag_core::AliasType, StorageError> {
    Ok(match s {
        "name" => oag_core::AliasType::Name,
        "url" => oag_core::AliasType::Url,
        "urn" => oag_core::AliasType::Urn,
        "package" => oag_core::AliasType::Package,
        "external_id" => oag_core::AliasType::ExternalId,
        "acronym" => oag_core::AliasType::Acronym,
        other => return Err(StorageError::UnknownEnumValue("alias_type", other.to_string())),
    })
}

fn row_to_alias(row: NodeAliasRow) -> Result<NodeAlias, StorageError> {
    Ok(NodeAlias {
        node_id: NodeId::from_hash(oag_core::Hash32::from_bytes(bytes_to_array(&row.node_id)?)),
        alias: row.alias,
        alias_type: parse_alias_type(&row.alias_type)?,
        source_assertion: row
            .source_assertion
            .map(|bytes| {
                bytes_to_array(&bytes)
                    .map(|arr| oag_core::AssertionId::from_hash(oag_core::Hash32::from_bytes(arr)))
            })
            .transpose()?,
    })
}

fn row_to_node(row: NodeRow) -> Result<oag_core::Node, StorageError> {
    Ok(oag_core::Node {
        id: NodeId::from_hash(oag_core::Hash32::from_bytes(bytes_to_array(&row.node_id)?)),
        node_type: oag_core::NodeType::new(row.node_type),
        canonical_identifier: row.canonical_identifier,
        canonical_uri: row.canonical_uri,
        name: row.name,
        description: row.description,
        metadata: serde_json::from_str(&row.metadata)?,
        created_at: row.created_at,
    })
}

/// Insert a node if it doesn't already exist (idempotent by `NodeId`, which
/// is deterministic from `canonical_identifier` — spec invariant 6).
pub async fn insert_if_missing(
    conn: &mut SqliteConnection,
    node: &oag_core::Node,
) -> Result<(), StorageError> {
    let metadata = serde_json::to_string(&node.metadata)?;
    sqlx::query(
        "INSERT OR IGNORE INTO nodes \
         (node_id, node_type, canonical_identifier, canonical_uri, name, description, metadata, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(node.id.as_hash().as_bytes().to_vec())
    .bind(node.node_type.as_str())
    .bind(&node.canonical_identifier)
    .bind(&node.canonical_uri)
    .bind(&node.name)
    .bind(&node.description)
    .bind(metadata)
    .bind(node.created_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn get_by_id(
    conn: &mut SqliteConnection,
    id: NodeId,
) -> Result<Option<oag_core::Node>, StorageError> {
    let row: Option<NodeRow> = sqlx::query_as("SELECT * FROM nodes WHERE node_id = ?")
        .bind(id.as_hash().as_bytes().to_vec())
        .fetch_optional(&mut *conn)
        .await?;
    row.map(row_to_node).transpose()
}

pub async fn get_by_canonical_identifier(
    conn: &mut SqliteConnection,
    identifier: &str,
) -> Result<Option<oag_core::Node>, StorageError> {
    let row: Option<NodeRow> = sqlx::query_as("SELECT * FROM nodes WHERE canonical_identifier = ?")
        .bind(identifier)
        .fetch_optional(&mut *conn)
        .await?;
    row.map(row_to_node).transpose()
}

pub async fn insert_alias(
    conn: &mut SqliteConnection,
    alias: &NodeAlias,
) -> Result<(), StorageError> {
    let alias_type = match alias.alias_type {
        oag_core::AliasType::Name => "name",
        oag_core::AliasType::Url => "url",
        oag_core::AliasType::Urn => "urn",
        oag_core::AliasType::Package => "package",
        oag_core::AliasType::ExternalId => "external_id",
        oag_core::AliasType::Acronym => "acronym",
    };
    sqlx::query(
        "INSERT OR IGNORE INTO node_aliases (node_id, alias, alias_type, source_assertion) \
         VALUES (?, ?, ?, ?)",
    )
    .bind(alias.node_id.as_hash().as_bytes().to_vec())
    .bind(&alias.alias)
    .bind(alias_type)
    .bind(alias.source_assertion.map(|a| a.as_hash().as_bytes().to_vec()))
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn list_aliases(
    conn: &mut SqliteConnection,
    node_id: NodeId,
) -> Result<Vec<NodeAlias>, StorageError> {
    let rows: Vec<NodeAliasRow> = sqlx::query_as("SELECT * FROM node_aliases WHERE node_id = ?")
        .bind(node_id.as_hash().as_bytes().to_vec())
        .fetch_all(&mut *conn)
        .await?;
    rows.into_iter().map(row_to_alias).collect()
}

/// Simple relevance search over node name/description via FTS5 (spec
/// section 63/65 — the `relevance` signal only; authority/evidence_strength/
/// freshness/source_independence need corroboration data this milestone
/// doesn't produce).
pub async fn search(
    conn: &mut SqliteConnection,
    query: &str,
    limit: i64,
) -> Result<Vec<oag_core::Node>, StorageError> {
    let rows: Vec<NodeRow> = sqlx::query_as(
        "SELECT nodes.* FROM nodes_fts \
         JOIN nodes ON nodes.rowid = nodes_fts.rowid \
         WHERE nodes_fts MATCH ? \
         ORDER BY rank LIMIT ?",
    )
    .bind(query)
    .bind(limit)
    .fetch_all(&mut *conn)
    .await?;
    rows.into_iter().map(row_to_node).collect()
}

pub fn metadata_object() -> JsonValue {
    JsonValue::Object(Default::default())
}
