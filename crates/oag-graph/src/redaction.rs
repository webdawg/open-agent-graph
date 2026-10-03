//! Spec section 85 (Deletion and Redaction). Both capabilities here are
//! local-only: they can never reach data already replicated to another
//! peer (no per-event "stop relaying" mechanism exists anywhere in
//! `oag-sync`, by design -- an immutable distributed log can't promise
//! that). `Permission::Admin` gates both, since these are rare,
//! operator-triggered actions, not something a normal actor calls.
//!
//! Evidence redaction and node search suppression are deliberately
//! different strengths of intervention:
//! - Redaction *destroys* the free-text `title`/`excerpt` content on the
//!   local `evidence` row -- irreversible at the tooling level (no
//!   "unredact" command exists; the original bytes still live forever in
//!   `events.canonical_payload`, but nothing ever reads them back).
//! - Search suppression only *hides* a node from search results -- the
//!   node, its edges, and its assertions are completely untouched, and
//!   `unsuppress_node_from_search` fully reverses it.

use oag_core::{EventId, NodeId, Permission};
use oag_storage::repo::{redactions as redactions_repo, search_suppressions as search_suppressions_repo};

use crate::assert::check_opt_len;
use crate::error::GraphError;
use crate::service::{AuthContext, GraphService};

pub use oag_storage::repo::redactions::Redaction;

// Same spec section 61 rationale as assert.rs's MAX_REASON_LEN (which this
// matches) -- an unbounded `reason` here isn't a signed event, so it can't
// bloat the replicated log, but it's still a permanent, unbounded write to
// this peer's own database with no guard at all.
const MAX_REASON_LEN: usize = 2048;

impl GraphService {
    /// Blanks `title`/`excerpt` on the evidence identified by `evidence_id`
    /// (an `ADD_EVIDENCE` event's id) and records a tombstone in
    /// `redactions` so the redaction survives every future `oag rebuild`
    /// (see `Self::rebuild_projection`). `uri`/`content_hash`/`evidence_type`
    /// and both timestamps are left intact.
    ///
    /// Never re-runs silently on an already-redacted id -- returns
    /// `GraphError::AlreadyRedacted` instead, so a caller can't mistake a
    /// repeat call for a fresh one.
    pub async fn redact_evidence(
        &self,
        auth: &AuthContext,
        evidence_id: EventId,
        reason: Option<String>,
    ) -> Result<(), GraphError> {
        auth.require(Permission::Admin)?;
        check_opt_len("reason", &reason, MAX_REASON_LEN)?;

        let mut tx = self.pool().begin().await.map_err(oag_storage::StorageError::from)?;

        if redactions_repo::exists(&mut tx, evidence_id).await? {
            return Err(GraphError::AlreadyRedacted(evidence_id));
        }

        let updated = oag_storage::repo::assertions::redact_evidence(&mut tx, evidence_id).await?;
        if !updated {
            return Err(GraphError::NotFound(format!("evidence {evidence_id}")));
        }

        redactions_repo::insert(&mut tx, evidence_id, self.now(), reason.as_deref()).await?;

        tx.commit().await.map_err(oag_storage::StorageError::from)?;
        Ok(())
    }

    /// Every redaction tombstone (spec section 85), for `oag redact list`'s
    /// audit view.
    pub async fn list_redactions(&self, auth: &AuthContext) -> Result<Vec<Redaction>, GraphError> {
        auth.require(Permission::Admin)?;
        let mut conn = self.pool().acquire().await.map_err(oag_storage::StorageError::from)?;
        Ok(redactions_repo::list_all(&mut conn).await?)
    }

    /// Hides `node_id` from `Self::search` results without touching the
    /// node, its edges, or its assertions. Idempotent -- suppressing an
    /// already-suppressed node is a no-op, not an error, since nothing
    /// destructive happens either way.
    pub async fn suppress_node_from_search(&self, auth: &AuthContext, node_id: NodeId) -> Result<(), GraphError> {
        auth.require(Permission::Admin)?;
        let mut conn = self.pool().acquire().await.map_err(oag_storage::StorageError::from)?;
        search_suppressions_repo::suppress(&mut conn, node_id, self.now()).await?;
        Ok(())
    }

    /// Reverses `Self::suppress_node_from_search`. A no-op (not an error) if
    /// `node_id` wasn't suppressed.
    pub async fn unsuppress_node_from_search(&self, auth: &AuthContext, node_id: NodeId) -> Result<(), GraphError> {
        auth.require(Permission::Admin)?;
        let mut conn = self.pool().acquire().await.map_err(oag_storage::StorageError::from)?;
        search_suppressions_repo::unsuppress(&mut conn, node_id).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use oag_core::ActorType;
    use oag_crypto::PeerIdentity;
    use oag_storage::pool::open_pool;

    use crate::assert::{AssertInput, EvidenceInput};
    use crate::resolve::ResolveOutcome;

    use super::*;

    fn temp_db_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oag-graph-redaction-test-{name}-{}",
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
        (service, AuthContext { actor_id, permissions: vec![Permission::GraphAssert, Permission::Admin] })
    }

    async fn assert_with_evidence(service: &GraphService, auth: &AuthContext) -> EventId {
        let assertion_id = service
            .assert(
                auth,
                AssertInput {
                    subject: "https://example.com/redaction-target".into(),
                    subject_type: None,
                    predicate: "instance_of".into(),
                    object: "concept:redaction-test".into(),
                    object_type: None,
                    evidence: vec![],
                    actor_confidence: Some(0.9),
                    observed_at: None,
                    extraction_method: None,
                },
            )
            .await
            .unwrap();
        service
            .add_evidence(
                auth,
                assertion_id,
                EvidenceInput {
                    evidence_type: Some("documentation".into()),
                    uri: Some("https://example.com/docs".into()),
                    title: Some("A Very Personal Title".into()),
                    excerpt: Some("Some excerpt containing personal details".into()),
                    content_hash: Some("blake3:deadbeef".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        service.list_evidence(assertion_id).await.unwrap()[0].id
    }

    #[tokio::test]
    async fn redact_evidence_is_reflected_via_list_evidence() {
        let (service, auth) = service_with_admin("redact-list-evidence").await;
        let evidence_id = assert_with_evidence(&service, &auth).await;

        // Find the owning assertion the same way a real caller would: via
        // resolve + get_edges + list_assertions_for_node.
        let ResolveOutcome::Found { node, .. } =
            service.resolve("https://example.com/redaction-target").await.unwrap()
        else {
            panic!("expected the asserted subject to resolve");
        };
        let assertions = service.list_assertions_for_node(node.id).await.unwrap();
        let assertion_id = assertions[0].id;

        service.redact_evidence(&auth, evidence_id, Some("gdpr request".to_string())).await.unwrap();

        let evidence = service.list_evidence(assertion_id).await.unwrap();
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].title, None);
        assert_eq!(evidence[0].excerpt, None);
        assert_eq!(evidence[0].uri.as_deref(), Some("https://example.com/docs"));
        assert_eq!(evidence[0].content_hash.as_deref(), Some("blake3:deadbeef"));

        let redactions = service.list_redactions(&auth).await.unwrap();
        assert_eq!(redactions.len(), 1);
        assert_eq!(redactions[0].event_id, evidence_id);
        assert_eq!(redactions[0].reason.as_deref(), Some("gdpr request"));
    }

    #[tokio::test]
    async fn redacting_twice_is_a_typed_error_not_a_silent_noop() {
        let (service, auth) = service_with_admin("redact-twice").await;
        let evidence_id = assert_with_evidence(&service, &auth).await;

        service.redact_evidence(&auth, evidence_id, None).await.unwrap();
        let err = service.redact_evidence(&auth, evidence_id, None).await.unwrap_err();
        assert!(matches!(err, GraphError::AlreadyRedacted(id) if id == evidence_id));
    }

    #[tokio::test]
    async fn redacting_with_an_oversized_reason_is_rejected() {
        let (service, auth) = service_with_admin("redact-oversized-reason").await;
        let evidence_id = assert_with_evidence(&service, &auth).await;

        let huge_reason = "x".repeat(10_000);
        let err = service.redact_evidence(&auth, evidence_id, Some(huge_reason)).await.unwrap_err();
        assert!(matches!(err, GraphError::InvalidInput(_)), "got {err:?}");

        // Must be rejected before any write -- a second attempt with a
        // valid reason should still succeed, not hit AlreadyRedacted.
        service.redact_evidence(&auth, evidence_id, Some("valid reason".to_string())).await.unwrap();
    }

    #[tokio::test]
    async fn redacting_unknown_evidence_id_is_not_found() {
        let (service, auth) = service_with_admin("redact-unknown").await;
        let fake_id = EventId::derive(b"does-not-exist");
        let err = service.redact_evidence(&auth, fake_id, None).await.unwrap_err();
        assert!(matches!(err, GraphError::NotFound(_)));
    }

    #[tokio::test]
    async fn redaction_survives_rebuild() {
        let (service, auth) = service_with_admin("redact-rebuild").await;
        let evidence_id = assert_with_evidence(&service, &auth).await;
        service.redact_evidence(&auth, evidence_id, Some("survives rebuild".to_string())).await.unwrap();

        let summary = service.rebuild_projection().await.unwrap();
        assert_eq!(summary.redactions_reapplied, 1);

        let ResolveOutcome::Found { node, .. } =
            service.resolve("https://example.com/redaction-target").await.unwrap()
        else {
            panic!("expected the asserted subject to still resolve after rebuild");
        };
        let assertions = service.list_assertions_for_node(node.id).await.unwrap();
        let evidence = service.list_evidence(assertions[0].id).await.unwrap();
        assert_eq!(evidence[0].title, None, "redaction must survive rebuild, not be resurrected by replay");
        assert_eq!(evidence[0].excerpt, None);
    }

    #[tokio::test]
    async fn suppress_node_hides_it_from_search_but_not_from_direct_lookup() {
        let (service, auth) = service_with_admin("suppress-basic").await;
        assert_with_evidence(&service, &auth).await;

        let ResolveOutcome::Found { node, .. } =
            service.resolve("concept:redaction-test").await.unwrap()
        else {
            panic!("expected the asserted object to resolve");
        };

        assert!(!service.search("redaction-test", 10).await.unwrap().is_empty());

        service.suppress_node_from_search(&auth, node.id).await.unwrap();
        assert!(service.search("redaction-test", 10).await.unwrap().is_empty());
        // Direct lookup and edges are completely untouched.
        assert!(service.get_node(node.id).await.unwrap().is_some());

        service.unsuppress_node_from_search(&auth, node.id).await.unwrap();
        assert!(!service.search("redaction-test", 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn search_suppression_survives_rebuild() {
        let (service, auth) = service_with_admin("suppress-rebuild").await;
        assert_with_evidence(&service, &auth).await;
        let ResolveOutcome::Found { node, .. } =
            service.resolve("concept:redaction-test").await.unwrap()
        else {
            panic!("expected the asserted object to resolve");
        };

        service.suppress_node_from_search(&auth, node.id).await.unwrap();
        service.rebuild_projection().await.unwrap();

        assert!(service.search("redaction-test", 10).await.unwrap().is_empty());
    }
}
