//! Storage for spec section 85's redaction tombstones. This table is the
//! durable record of an operator's decision to scrub a piece of evidence's
//! free-text content locally -- see `crate::repo::rebuild`'s doc comment for
//! why it's deliberately excluded from `clear_projection_tables`'s wipe
//! list, and `oag_graph::redaction` for the actual scrubbing logic this
//! backs.

use oag_core::EventId;
use sqlx::SqliteConnection;

use crate::error::{bytes_to_array, StorageError};
use crate::models::RedactionRow;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Redaction {
    pub event_id: EventId,
    pub redacted_at: i64,
    pub reason: Option<String>,
}

fn row_to_redaction(row: RedactionRow) -> Result<Redaction, StorageError> {
    Ok(Redaction {
        event_id: EventId::from_hash(oag_core::Hash32::from_bytes(bytes_to_array(&row.event_id)?)),
        redacted_at: row.redacted_at,
        reason: row.reason,
    })
}

pub async fn insert(
    conn: &mut SqliteConnection,
    event_id: EventId,
    redacted_at: i64,
    reason: Option<&str>,
) -> Result<(), StorageError> {
    sqlx::query("INSERT INTO redactions (event_id, redacted_at, reason) VALUES (?, ?, ?)")
        .bind(event_id.as_hash().as_bytes().to_vec())
        .bind(redacted_at)
        .bind(reason)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

pub async fn exists(conn: &mut SqliteConnection, event_id: EventId) -> Result<bool, StorageError> {
    let row: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM redactions WHERE event_id = ?")
        .bind(event_id.as_hash().as_bytes().to_vec())
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.is_some())
}

/// Every tombstone -- both for `oag redact list` (auditability) and for
/// `oag_graph::rebuild::rebuild_projection` to reapply after every replay.
pub async fn list_all(conn: &mut SqliteConnection) -> Result<Vec<Redaction>, StorageError> {
    let rows: Vec<RedactionRow> = sqlx::query_as("SELECT * FROM redactions").fetch_all(&mut *conn).await?;
    rows.into_iter().map(row_to_redaction).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::open_pool;

    fn temp_db_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oag-storage-redactions-test-{name}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("oag.sqlite")
    }

    #[tokio::test]
    async fn insert_exists_and_list_round_trip() {
        let pool = open_pool(&temp_db_path("basic")).await.unwrap();
        let mut conn = pool.acquire().await.unwrap();

        // Insert a real event row first -- `event_id` has a FK to `events`.
        let event_id = EventId::derive(b"redaction-test-event");
        sqlx::query(
            "INSERT INTO events (event_id, origin_peer_id, sequence, previous_event_id, event_type, canonical_payload, created_at, signature, received_at) \
             VALUES (?, ?, 1, NULL, 'ADD_EVIDENCE', X'00', 1, X'00', 1)",
        )
        .bind(event_id.as_hash().as_bytes().to_vec())
        .bind(vec![1u8; 32])
        .execute(&mut *conn)
        .await
        .unwrap();

        assert!(!exists(&mut conn, event_id).await.unwrap());
        insert(&mut conn, event_id, 42, Some("gdpr request")).await.unwrap();
        assert!(exists(&mut conn, event_id).await.unwrap());

        let all = list_all(&mut conn).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].event_id, event_id);
        assert_eq!(all[0].redacted_at, 42);
        assert_eq!(all[0].reason.as_deref(), Some("gdpr request"));
    }
}
