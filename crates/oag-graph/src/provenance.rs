//! Spec sections 80-81: the Human Interface's Assertion page needs to show
//! an assertion's origin peer and signature status, and explainability's
//! chain ends at exactly `event -> origin peer`. Neither is carried by the
//! projected `Assertion` row itself -- only the *event* that created it
//! knows its origin, so this reaches back to the raw signed event
//! (`Assertion.id` *is* that event's id, spec section 21).

use oag_core::EventId;
use oag_crypto::PeerId;
use oag_events::envelope::SignedEvent;
use oag_events::validate::verify_and_derive_id;
use oag_storage::repo::{events as events_repo, peers as peers_repo};

use crate::error::GraphError;
use crate::service::GraphService;

#[derive(Debug, Clone, serde::Serialize)]
pub struct EventProvenance {
    pub event_id: EventId,
    pub origin_peer: String,
    pub event_type: String,
    pub created_at: i64,
    /// Re-verified against the origin's known public key, not just trusted
    /// from whatever this row already says -- `false` if we don't have that
    /// key on file at all (`origin_key_known: false`), never a false
    /// "valid".
    pub signature_valid: bool,
    pub origin_key_known: bool,
}

impl GraphService {
    /// `None` if this peer doesn't have `event_id` at all (shouldn't happen
    /// for an assertion's own creating event, since that's how the
    /// assertion got projected in the first place -- but never assumed).
    pub async fn get_event_provenance(&self, event_id: EventId) -> Result<Option<EventProvenance>, GraphError> {
        let mut conn = self.pool().acquire().await.map_err(oag_storage::StorageError::from)?;

        let Some(row) = events_repo::get_by_id(&mut conn, event_id).await? else {
            return Ok(None);
        };
        let signed: SignedEvent = serde_json::from_slice(&row.canonical_payload).map_err(oag_events::EventsError::from)?;
        let origin_peer: PeerId = signed.unsigned.origin_peer.parse().map_err(|_| {
            GraphError::InvalidInput(format!("stored event {event_id} has an unparseable origin_peer"))
        })?;

        let verifying_key = if origin_peer == self.identity().peer_id() {
            Some(self.identity().verifying_key())
        } else {
            peers_repo::get_peer(&mut conn, origin_peer.as_bytes())
                .await?
                .map(|info| oag_crypto::VerifyingKey::from_bytes(&info.public_key))
                .transpose()
                .map_err(|_| GraphError::InvalidInput(format!("stored public key for {origin_peer} is invalid")))?
        };

        let (signature_valid, origin_key_known) = match verifying_key {
            Some(key) => (verify_and_derive_id(&signed, &key).map(|derived| derived == event_id).unwrap_or(false), true),
            None => (false, false),
        };

        Ok(Some(EventProvenance {
            event_id,
            origin_peer: origin_peer.to_string(),
            event_type: row.event_type,
            created_at: row.created_at,
            signature_valid,
            origin_key_known,
        }))
    }
}

#[cfg(test)]
mod tests {
    use oag_core::{ActorType, Permission};
    use oag_crypto::PeerIdentity;
    use oag_storage::pool::open_pool;

    use crate::assert::AssertInput;
    use crate::service::AuthContext;

    use super::*;

    fn temp_db_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oag-graph-provenance-test-{name}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("oag.sqlite")
    }

    async fn service_with_admin(name: &str) -> (GraphService, AuthContext) {
        let pool = open_pool(&temp_db_path(name)).await.unwrap();
        let identity = PeerIdentity::generate();
        let service = GraphService::new(pool, identity);
        let actor_id =
            service.declare_actor(ActorType::Agent, Some("admin".into()), None, None).await.unwrap();
        (service, AuthContext { actor_id, permissions: vec![Permission::GraphAssert] })
    }

    #[tokio::test]
    async fn self_authored_event_has_a_valid_signature() {
        let (service, auth) = service_with_admin("self-authored").await;
        let assertion_id = service
            .assert(
                &auth,
                AssertInput {
                    subject: "https://example.com/provenance-test".into(),
                    subject_type: None,
                    predicate: "instance_of".into(),
                    object: "concept:provenance-test".into(),
                    object_type: None,
                    evidence: vec![],
                    actor_confidence: Some(0.9),
                    observed_at: None,
                    extraction_method: None,
                },
            )
            .await
            .unwrap();

        // Assertion.id is the ASSERT_RELATION event's own id.
        let event_id = EventId::from_hash(assertion_id.as_hash());
        let provenance = service.get_event_provenance(event_id).await.unwrap().unwrap();

        assert_eq!(provenance.origin_peer, service.identity().peer_id().to_string());
        assert_eq!(provenance.event_type, "ASSERT_RELATION");
        assert!(provenance.origin_key_known);
        assert!(provenance.signature_valid);
    }

    #[tokio::test]
    async fn unknown_event_id_is_none() {
        let (service, _auth) = service_with_admin("unknown-event").await;
        let fake_id = EventId::derive(b"does-not-exist");
        assert!(service.get_event_provenance(fake_id).await.unwrap().is_none());
    }
}
