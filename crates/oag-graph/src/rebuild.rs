//! Spec section 83: the event log is immutable and never touched here —
//! every other graph table is a derived cache of it, rebuilt from scratch
//! by wiping and replaying. See `oag_storage::repo::rebuild`'s doc comment
//! for exactly which tables that means (and, just as importantly, which it
//! doesn't).
use oag_events::SignedEvent;
use oag_storage::repo::{assertions as assertions_repo, events as events_repo, rebuild as rebuild_repo, redactions as redactions_repo};

use crate::error::GraphError;
use crate::service::GraphService;

#[derive(Debug, Clone, serde::Serialize)]
pub struct RebuildSummary {
    pub events_replayed: usize,
    /// Spec section 85: outstanding redactions reapplied after replay --
    /// `redactions` itself is never wiped (see `oag_storage::repo::
    /// rebuild`'s doc comment), but replaying `ADD_EVIDENCE` events fresh
    /// would otherwise resurrect the original `title`/`excerpt` this many
    /// times over.
    pub redactions_reapplied: usize,
}

impl GraphService {
    /// Wipes every derived graph table and replays the full event log, in
    /// original insertion order, to regenerate them. The event log itself
    /// is never modified — this can only ever reproduce what a peer already
    /// had, never lose or gain events.
    ///
    /// Holds one large write transaction for the whole rebuild. Safe to run
    /// against a live `oag serve` (SQLite's busy_timeout will just serialize
    /// concurrent writers rather than corrupt anything), but a live server
    /// would see a long stall for the duration — best run with it stopped.
    pub async fn rebuild_projection(&self) -> Result<RebuildSummary, GraphError> {
        let mut tx = self.pool().begin().await.map_err(oag_storage::StorageError::from)?;

        rebuild_repo::clear_projection_tables(&mut tx).await?;

        let all_events = events_repo::list_all_in_insertion_order(&mut tx).await?;
        let mut replayed = 0;
        for row in &all_events {
            let signed: SignedEvent = serde_json::from_slice(&row.canonical_payload)
                .map_err(oag_events::EventsError::from)?;
            let event_id = events_repo::event_row_to_id(row)?;
            oag_events::projector::project(&mut tx, event_id, row.created_at, &signed.unsigned.payload).await?;
            replayed += 1;
        }

        let outstanding_redactions = redactions_repo::list_all(&mut tx).await?;
        for redaction in &outstanding_redactions {
            assertions_repo::redact_evidence(&mut tx, redaction.event_id).await?;
        }

        tx.commit().await.map_err(oag_storage::StorageError::from)?;

        Ok(RebuildSummary {
            events_replayed: replayed,
            redactions_reapplied: outstanding_redactions.len(),
        })
    }
}
