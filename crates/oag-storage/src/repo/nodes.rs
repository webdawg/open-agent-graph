use oag_core::{NodeAlias, NodeId};
use serde_json::Value as JsonValue;
use sqlx::{FromRow, SqliteConnection};
use std::str::FromStr;

use crate::error::{bytes_to_array, StorageError};
use crate::models::{NodeAliasRow, NodeRow};
use crate::versioning::{HasSchemaVersion, MigrationOutcome, Versioned};

impl HasSchemaVersion for NodeRow {
    fn schema_version(&self) -> i64 {
        self.schema_version
    }
}

impl Versioned for NodeRow {
    const CURRENT_VERSION: i64 = 1;
    const MIGRATIONS: &'static [crate::versioning::MigrationStep<NodeRow>] = &[];
}

impl HasSchemaVersion for NodeAliasRow {
    fn schema_version(&self) -> i64 {
        self.schema_version
    }
}

impl Versioned for NodeAliasRow {
    const CURRENT_VERSION: i64 = 1;
    const MIGRATIONS: &'static [crate::versioning::MigrationStep<NodeAliasRow>] = &[];
}

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
    let row = row.migrate_to_current();
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
    let row = row.migrate_to_current();
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
/// doesn't produce). Excludes nodes in `search_suppressions` (spec section
/// 85) -- a live filter rather than a cached flag on `nodes`, so it stays
/// correct across `oag rebuild` without any special-casing there.
pub async fn search(
    conn: &mut SqliteConnection,
    query: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<oag_core::Node>, StorageError> {
    // `OFFSET` without `LIMIT` is a syntax error in SQLite, so callers must
    // always pair them -- the service layer clamps both, which guarantees
    // that. A negative offset is clamped to 0 for the same reason `limit` is
    // clamped: SQLite's negative-LIMIT quirk does not apply to OFFSET, but
    // accepting one would silently mean "from the start" instead of the
    // error the caller probably expected.
    let rows: Vec<NodeRow> = sqlx::query_as(
        "SELECT nodes.* FROM nodes_fts \
         JOIN nodes ON nodes.rowid = nodes_fts.rowid \
         WHERE nodes_fts MATCH ? \
         AND nodes.node_id NOT IN (SELECT node_id FROM search_suppressions) \
         ORDER BY rank, nodes.node_id LIMIT ? OFFSET ?",
    )
    .bind(query)
    .bind(limit)
    .bind(offset)
    .fetch_all(&mut *conn)
    .await?;
    rows.into_iter().map(row_to_node).collect()
}

/// Where a keyset-paginated search resumes: the node the caller last saw.
///
/// Deliberately *not* the last row's rank. FTS5's bm25 rank is a function of the corpus --
/// its idf and length-normalisation terms read the document count and average length -- so
/// the same row scores differently before and after an insertion. A cursor that stored the
/// rank would compare a score computed under the old corpus against scores computed under
/// the new one, and would repeat the boundary row. Naming the row instead lets the query
/// resolve that row's *current* rank, which makes the boundary exact under any absolute
/// shift. Measured: inserting a document preserved the relative order of the existing
/// matches but moved every one of their ranks.
///
/// What this still cannot survive is a *reordering* -- if the corpus change moves a row
/// across the boundary relative to the rows around it, that row repeats or skips. That is
/// not fixable by any pagination scheme, because the ordering itself changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchCursor {
    pub node_id: NodeId,
}

impl SearchCursor {
    /// Opaque wire form: the node id's 64 hex digits. Callers must treat it as an opaque
    /// token; it is a row identity, not a position, and a cursor from another query names
    /// a row that query may not even match.
    pub fn encode(&self) -> String {
        self.node_id.to_hex()
    }

    pub fn decode(encoded: &str) -> Option<Self> {
        Some(Self { node_id: NodeId::from_str(encoded).ok()? })
    }
}

/// The rank FTS5 assigns `node_id` for `query` right now, or `None` if that node no longer
/// matches -- it was removed, or its text changed so the query no longer hits it.
async fn current_rank(
    conn: &mut SqliteConnection,
    query: &str,
    node_id: NodeId,
) -> Result<Option<f64>, StorageError> {
    sqlx::query_scalar(
        "SELECT nodes_fts.rank FROM nodes_fts \
         JOIN nodes ON nodes.rowid = nodes_fts.rowid \
         WHERE nodes_fts MATCH ? AND nodes.node_id = ?",
    )
    .bind(query)
    .bind(node_id.as_hash().as_bytes().as_slice())
    .fetch_optional(&mut *conn)
    .await
    .map_err(StorageError::from)
}

/// Keyset-paginated search: the `limit` rows that follow `cursor` in `(rank, node_id)` order.
///
/// Unlike [`search`]'s `OFFSET`, the boundary is the last row the caller actually saw, so a
/// document arriving that outranks the current page does not shift it --
/// `search_cursor_walk_covers_every_match_once_across_an_insertion` asserts exactly that.
///
/// A cursor whose node no longer matches starts the walk over. That is a stale cursor, and
/// the alternative -- refusing it -- would 500 a caller that cannot recover mid-walk.
///
/// Returns `(node, rank)` pairs so the caller can build the next cursor without a second
/// query for the row it already has.
pub async fn search_after(
    conn: &mut SqliteConnection,
    query: &str,
    limit: i64,
    cursor: Option<SearchCursor>,
) -> Result<Vec<(oag_core::Node, f64)>, StorageError> {
    let boundary = match cursor {
        None => None,
        Some(cursor) => current_rank(conn, query, cursor.node_id).await?,
    };
    // Only consulted when `boundary` is non-NULL, so a stale cursor's id is irrelevant.
    let boundary_id: Option<Vec<u8>> =
        cursor.map(|c| c.node_id.as_hash().as_bytes().to_vec());
    // `rank` is FTS5's bm25 and is only computed for the query being run, which is why a
    // cursor is only meaningful against the same query. The subquery materialises it so the
    // outer filter can compare it -- FTS5 does not accept `rank` in a `WHERE` directly.
    //
    // A `NULL` boundary means "no cursor, or a stale one": every row sorts after it, which
    // is the same as starting over.
    let rows: Vec<RankedNodeRow> = sqlx::query_as(
        "SELECT node_id, node_type, canonical_identifier, canonical_uri, name, description, \
         metadata, created_at, schema_version, search_rank \
         FROM ( \
             SELECT nodes.*, nodes_fts.rank AS search_rank \
             FROM nodes_fts \
             JOIN nodes ON nodes.rowid = nodes_fts.rowid \
             WHERE nodes_fts MATCH ? \
             AND nodes.node_id NOT IN (SELECT node_id FROM search_suppressions) \
         ) \
         WHERE ? IS NULL OR search_rank > ? OR (search_rank = ? AND node_id > ?) \
         ORDER BY search_rank, node_id \
         LIMIT ?",
    )
    .bind(query)
    .bind(boundary)
    .bind(boundary)
    .bind(boundary)
    .bind(boundary_id)
    .bind(limit)
    .fetch_all(&mut *conn)
    .await?;

    rows.into_iter()
        .map(|row| {
            let rank = row.search_rank;
            let row = NodeRow {
                node_id: row.node_id,
                node_type: row.node_type,
                canonical_identifier: row.canonical_identifier,
                canonical_uri: row.canonical_uri,
                name: row.name,
                description: row.description,
                metadata: row.metadata,
                created_at: row.created_at,
                schema_version: row.schema_version,
            };
            row_to_node(row).map(|node| (node, rank))
        })
        .collect()
}

/// [`NodeRow`] plus the FTS5 `rank` the row was sorted by, so a page can be resumed from its
/// last row. Separate from [`NodeRow`] because sqlx maps structs by column name and would
/// otherwise have to treat `search_rank` as a node field.
#[derive(Debug, FromRow)]
struct RankedNodeRow {
    node_id: Vec<u8>,
    node_type: String,
    canonical_identifier: String,
    canonical_uri: Option<String>,
    name: Option<String>,
    description: Option<String>,
    metadata: String,
    created_at: i64,
    schema_version: i64,
    search_rank: f64,
}

/// Every node in the local graph — used by embeddings recomputation (spec
/// section 64), which needs the whole node set rather than a query-scoped
/// slice of it (same shape as `edges::list_all` for authority/PageRank).
pub async fn list_all(conn: &mut SqliteConnection) -> Result<Vec<oag_core::Node>, StorageError> {
    let rows: Vec<NodeRow> = sqlx::query_as("SELECT * FROM nodes").fetch_all(&mut *conn).await?;
    rows.into_iter().map(row_to_node).collect()
}

pub fn metadata_object() -> JsonValue {
    JsonValue::Object(Default::default())
}

/// The "convert perm" access mode (spec/27-artifact-versioning.md): rewrites
/// `node_id`'s row to its current shape in place so future reads skip the
/// migration chain `row_to_node` otherwise applies in memory on every read.
/// `NodeRow::MIGRATIONS` is empty today, so this always reports
/// `AlreadyCurrent` for any row that exists -- a true, honest no-op until a
/// real second node shape is ever introduced.
pub async fn migrate_to_current(
    conn: &mut SqliteConnection,
    id: NodeId,
) -> Result<MigrationOutcome, StorageError> {
    let row: Option<NodeRow> = sqlx::query_as("SELECT * FROM nodes WHERE node_id = ?")
        .bind(id.as_hash().as_bytes().to_vec())
        .fetch_optional(&mut *conn)
        .await?;
    let row = row.ok_or(StorageError::NotFound)?;
    let from = row.schema_version();
    if from >= NodeRow::CURRENT_VERSION {
        return Ok(MigrationOutcome::AlreadyCurrent { version: from });
    }
    let migrated = row.migrate_to_current();
    let to = migrated.schema_version();
    sqlx::query("UPDATE nodes SET schema_version = ? WHERE node_id = ?")
        .bind(to)
        .bind(id.as_hash().as_bytes().to_vec())
        .execute(&mut *conn)
        .await?;
    Ok(MigrationOutcome::Migrated { from, to })
}
