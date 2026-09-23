use oag_storage::repo::events as events_repo;

use crate::error::GraphError;
use crate::service::GraphService;

#[derive(Debug, serde::Serialize)]
pub struct HistoryEntry {
    pub event_id: oag_core::EventId,
    pub event_type: String,
    pub created_at: i64,
    pub payload: serde_json::Value,
}

const VALID_OBJECT_TYPES: &[&str] = &["node", "edge", "assertion"];

impl GraphService {
    /// Every event that has touched `object_type`/`id`, oldest first (spec
    /// section 81 — explainability: edge -> assertion -> actor -> evidence
    /// -> observation -> event -> origin peer).
    pub async fn get_history(
        &self,
        object_type: &str,
        id: &str,
    ) -> Result<Vec<HistoryEntry>, GraphError> {
        if !VALID_OBJECT_TYPES.contains(&object_type) {
            return Err(GraphError::InvalidInput(format!(
                "unknown object_type '{object_type}', expected one of {VALID_OBJECT_TYPES:?}"
            )));
        }
        let hash: oag_core::Hash32 = id
            .parse()
            .map_err(|_| GraphError::InvalidInput(format!("invalid id '{id}'")))?;

        let mut conn = self
            .pool()
            .acquire()
            .await
            .map_err(oag_storage::StorageError::from)?;
        let rows = events_repo::history_for(&mut conn, object_type, hash.as_bytes()).await?;

        rows.into_iter()
            .map(|row| {
                let payload: serde_json::Value = serde_json::from_slice(&row.canonical_payload)
                    .map_err(oag_events::EventsError::from)?;
                Ok(HistoryEntry {
                    event_id: events_repo::event_row_to_id(&row)?,
                    event_type: row.event_type.clone(),
                    created_at: row.created_at,
                    payload,
                })
            })
            .collect()
    }
}
