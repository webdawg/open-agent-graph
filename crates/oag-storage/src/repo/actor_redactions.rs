//! Storage for spec section 85's actor `identity_uri` redaction tombstones
//! -- the same durable-decision pattern as `crate::repo::redactions`
//! (evidence) and `crate::repo::search_suppressions` (nodes), just for one
//! more field. See `crate::repo::rebuild`'s doc comment for why this table
//! is excluded from `clear_projection_tables`'s wipe list, and
//! `oag_graph::redaction` for the actual scrubbing logic this backs.

use oag_core::ActorId;
use sqlx::SqliteConnection;

use crate::error::{bytes_to_array, StorageError};

#[derive(Debug, Clone, serde::Serialize)]
pub struct ActorRedaction {
    pub actor_id: ActorId,
    pub redacted_at: i64,
    pub reason: Option<String>,
}

pub async fn insert(
    conn: &mut SqliteConnection,
    actor_id: ActorId,
    redacted_at: i64,
    reason: Option<&str>,
) -> Result<(), StorageError> {
    sqlx::query("INSERT INTO actor_identity_uri_redactions (actor_id, redacted_at, reason) VALUES (?, ?, ?)")
        .bind(actor_id.as_hash().as_bytes().to_vec())
        .bind(redacted_at)
        .bind(reason)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

pub async fn exists(conn: &mut SqliteConnection, actor_id: ActorId) -> Result<bool, StorageError> {
    let row: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM actor_identity_uri_redactions WHERE actor_id = ?")
        .bind(actor_id.as_hash().as_bytes().to_vec())
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.is_some())
}

/// Every tombstone -- both for `oag redact list` (auditability) and for
/// `oag_graph::rebuild::rebuild_projection` to reapply after every replay.
pub async fn list_all(conn: &mut SqliteConnection) -> Result<Vec<ActorRedaction>, StorageError> {
    let rows: Vec<(Vec<u8>, i64, Option<String>)> =
        sqlx::query_as("SELECT actor_id, redacted_at, reason FROM actor_identity_uri_redactions")
            .fetch_all(&mut *conn)
            .await?;
    rows.into_iter()
        .map(|(actor_id, redacted_at, reason)| {
            Ok(ActorRedaction {
                actor_id: ActorId::from_hash(oag_core::Hash32::from_bytes(bytes_to_array(&actor_id)?)),
                redacted_at,
                reason,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::open_pool;
    use oag_core::{Actor, ActorType};

    fn temp_db_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oag-storage-actor-redactions-test-{name}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("oag.sqlite")
    }

    #[tokio::test]
    async fn insert_exists_and_list_round_trip() {
        let pool = open_pool(&temp_db_path("basic")).await.unwrap();
        let mut conn = pool.acquire().await.unwrap();

        let actor = Actor {
            id: ActorId::derive(b"actor-redaction-test"),
            actor_type: ActorType::Agent,
            name: Some("test".into()),
            public_key: None,
            identity_uri: Some("https://example.com/me".into()),
            metadata: serde_json::json!({}),
            created_at: 1,
        };
        crate::repo::actors::insert(&mut conn, &actor).await.unwrap();

        assert!(!exists(&mut conn, actor.id).await.unwrap());
        insert(&mut conn, actor.id, 42, Some("gdpr request")).await.unwrap();
        assert!(exists(&mut conn, actor.id).await.unwrap());

        let all = list_all(&mut conn).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].actor_id, actor.id);
        assert_eq!(all[0].redacted_at, 42);
        assert_eq!(all[0].reason.as_deref(), Some("gdpr request"));
    }
}
