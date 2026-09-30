use oag_core::{Actor, ActorId, ActorType, Permission};
use sqlx::SqliteConnection;

use crate::error::{bytes_to_array, StorageError};
use crate::models::{ActorKeyRow, ActorRow};

fn row_to_actor(row: ActorRow) -> Result<Actor, StorageError> {
    let actor_type = ActorType::parse(&row.actor_type)
        .ok_or_else(|| StorageError::UnknownEnumValue("actor_type", row.actor_type.clone()))?;
    let public_key = row
        .public_key
        .map(|bytes| bytes_to_array(&bytes))
        .transpose()?;
    Ok(Actor {
        id: ActorId::from_hash(oag_core::Hash32::from_bytes(bytes_to_array(&row.actor_id)?)),
        actor_type,
        name: row.name,
        public_key,
        identity_uri: row.identity_uri,
        metadata: serde_json::from_str(&row.metadata)?,
        created_at: row.created_at,
    })
}

pub async fn insert(conn: &mut SqliteConnection, actor: &Actor) -> Result<(), StorageError> {
    let metadata = serde_json::to_string(&actor.metadata)?;
    sqlx::query(
        "INSERT OR IGNORE INTO actors \
         (actor_id, actor_type, name, public_key, identity_uri, metadata, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(actor.id.as_hash().as_bytes().to_vec())
    .bind(actor.actor_type.as_str())
    .bind(&actor.name)
    .bind(actor.public_key.map(|k| k.to_vec()))
    .bind(&actor.identity_uri)
    .bind(metadata)
    .bind(actor.created_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn get_by_id(
    conn: &mut SqliteConnection,
    id: ActorId,
) -> Result<Option<Actor>, StorageError> {
    let row: Option<ActorRow> = sqlx::query_as("SELECT * FROM actors WHERE actor_id = ?")
        .bind(id.as_hash().as_bytes().to_vec())
        .fetch_optional(&mut *conn)
        .await?;
    row.map(row_to_actor).transpose()
}

/// Find an actor by its exact `name` and `actor_type`. Used to reuse a
/// stable local actor (e.g. "the crawler actor") across separate process
/// invocations, where `declare_actor`'s normal id-from-event-id derivation
/// would otherwise mint a fresh, unrelated identity every single time (spec
/// section 26 — one peer, one crawler actor, not one per crawl).
pub async fn find_by_name_and_type(
    conn: &mut SqliteConnection,
    name: &str,
    actor_type: ActorType,
) -> Result<Option<Actor>, StorageError> {
    let row: Option<ActorRow> =
        sqlx::query_as("SELECT * FROM actors WHERE name = ? AND actor_type = ?")
            .bind(name)
            .bind(actor_type.as_str())
            .fetch_optional(&mut *conn)
            .await?;
    row.map(row_to_actor).transpose()
}

pub async fn any_actor_exists(conn: &mut SqliteConnection) -> Result<bool, StorageError> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM actors")
        .fetch_one(&mut *conn)
        .await?;
    Ok(count.0 > 0)
}

/// Create an API key for `actor`, storing only `key_hash` (BLAKE3 of the raw
/// key) — the raw key is never persisted (spec section 57).
pub async fn create_key(
    conn: &mut SqliteConnection,
    key_hash: &[u8; 32],
    actor_id: ActorId,
    permissions: &[Permission],
    created_at: i64,
) -> Result<(), StorageError> {
    let permissions_json = serde_json::to_string(
        &permissions.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
    )?;
    sqlx::query(
        "INSERT INTO actor_keys (key_hash, actor_id, permissions, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(key_hash.to_vec())
    .bind(actor_id.as_hash().as_bytes().to_vec())
    .bind(permissions_json)
    .bind(created_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub struct AuthenticatedKey {
    pub actor_id: ActorId,
    pub permissions: Vec<Permission>,
}

pub async fn find_active_key(
    conn: &mut SqliteConnection,
    key_hash: &[u8; 32],
) -> Result<Option<AuthenticatedKey>, StorageError> {
    let row: Option<ActorKeyRow> = sqlx::query_as(
        "SELECT * FROM actor_keys WHERE key_hash = ? AND revoked_at IS NULL",
    )
    .bind(key_hash.to_vec())
    .fetch_optional(&mut *conn)
    .await?;
    let Some(row) = row else { return Ok(None) };
    let permission_strs: Vec<String> = serde_json::from_str(&row.permissions)?;
    let permissions = permission_strs
        .iter()
        .filter_map(|s| Permission::parse(s))
        .collect();
    Ok(Some(AuthenticatedKey {
        actor_id: ActorId::from_hash(oag_core::Hash32::from_bytes(bytes_to_array(
            &row.actor_id,
        )?)),
        permissions,
    }))
}

/// Auditable view of an issued key -- never the raw key itself (that's
/// shown once at creation and never stored, only its hash). `key_hash` is
/// what `oag key revoke` takes to identify which key to revoke.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ApiKeyInfo {
    pub key_hash: String,
    pub actor_id: ActorId,
    /// `Permission::as_str()` form (`"graph:read"`), matching exactly what
    /// `oag key create --permission` accepts and `Permission::parse` expects
    /// -- deliberately not `Vec<Permission>`, whose own `#[derive(Serialize)]`
    /// uses a *different*, kebab-case representation (`"graph-read"`) that a
    /// caller couldn't paste back into `--permission` without it being
    /// silently rejected as unknown.
    pub permissions: Vec<String>,
    pub created_at: i64,
    pub revoked_at: Option<i64>,
}

fn row_to_key_info(row: ActorKeyRow) -> Result<ApiKeyInfo, StorageError> {
    let permissions: Vec<String> = serde_json::from_str(&row.permissions)?;
    Ok(ApiKeyInfo {
        key_hash: hex::encode(&row.key_hash),
        actor_id: ActorId::from_hash(oag_core::Hash32::from_bytes(bytes_to_array(&row.actor_id)?)),
        permissions,
        created_at: row.created_at,
        revoked_at: row.revoked_at,
    })
}

/// Every key ever issued on this peer (active and revoked alike -- revoked
/// ones stay listed for audit purposes, just flagged via `revoked_at`).
pub async fn list_keys(conn: &mut SqliteConnection) -> Result<Vec<ApiKeyInfo>, StorageError> {
    let rows: Vec<ActorKeyRow> =
        sqlx::query_as("SELECT * FROM actor_keys ORDER BY created_at").fetch_all(&mut *conn).await?;
    rows.into_iter().map(row_to_key_info).collect()
}

/// Sets `revoked_at` on a currently-active key. Returns `false` (not an
/// error) if `key_hash` doesn't match any active key -- already-revoked or
/// unknown are both just "nothing to do" from this function's point of view;
/// the caller (`GraphService::revoke_key`) decides whether that's worth
/// surfacing as an error.
pub async fn revoke_key(
    conn: &mut SqliteConnection,
    key_hash: &[u8; 32],
    revoked_at: i64,
) -> Result<bool, StorageError> {
    let result = sqlx::query("UPDATE actor_keys SET revoked_at = ? WHERE key_hash = ? AND revoked_at IS NULL")
        .bind(revoked_at)
        .bind(key_hash.to_vec())
        .execute(&mut *conn)
        .await?;
    Ok(result.rows_affected() > 0)
}
