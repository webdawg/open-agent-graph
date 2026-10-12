//! spec/28-roadmap-governance.md: "what should this project build next" is
//! deliberately not a new consensus mechanism -- a roadmap proposal is just
//! an ordinary assertion, `(proposal, instance_of, concept:oag-roadmap-
//! proposal)`, backed by ordinary evidence, ranked by the same corroboration
//! signals (`Self::get_edge_corroboration`) every other claim already gets.
//! The hub node needs no bootstrap: like `concept:software-architecture-
//! reference` before it, it springs into existence the first time anything
//! is asserted against it, via the completely ordinary `resolve_or_create_node`
//! path every assertion already goes through.
//!
//! This module adds exactly the one thing that didn't already exist: a way
//! to *list* every proposal with its corroboration, rather than having to
//! already know an edge id to look one up. No new predicate, no new schema,
//! no new write path -- submitting a proposal is just the existing
//! `GraphService::assert`/`POST /api/v1/assertions`.

use oag_storage::repo::{edges, nodes};

use crate::corroboration::EdgeCorroboration;
use crate::error::GraphError;
use crate::service::GraphService;

/// The hub node every roadmap proposal is asserted against. Not pre-seeded
/// anywhere -- the first proposal ever asserted is what creates this node,
/// exactly like `concept:software-architecture-reference`.
pub const ROADMAP_PROPOSAL_HUB: &str = "concept:oag-roadmap-proposal";

/// `instance_of` rather than a new predicate -- already documented
/// (`docs/predicates.md`) as the baseline-typing relationship, and reusing
/// it keeps this feature at zero new vocabulary.
const ROADMAP_PROPOSAL_PREDICATE: &str = "instance_of";

/// One proposal, with the same per-signal corroboration breakdown every
/// other edge gets -- deliberately not collapsed into a single rank number,
/// matching `EdgeCorroboration`'s own existing design philosophy.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RoadmapProposal {
    pub node: oag_core::Node,
    pub corroboration: EdgeCorroboration,
}

impl GraphService {
    /// Every proposal currently asserted against the roadmap hub, each with
    /// its corroboration signals, sorted by `(active_assertions,
    /// source_independence, evidence_strength)` descending -- a
    /// deterministic multi-key sort, not a collapsed score. Returns an empty
    /// list, not an error, if the hub has never been asserted against (no
    /// proposal has ever been made).
    pub async fn list_roadmap_proposals(&self) -> Result<Vec<RoadmapProposal>, GraphError> {
        let mut conn = self.pool().acquire().await.map_err(oag_storage::StorageError::from)?;

        let hub = nodes::get_by_canonical_identifier(&mut conn, ROADMAP_PROPOSAL_HUB).await?;
        let Some(hub) = hub else {
            return Ok(Vec::new());
        };

        let proposal_edges =
            edges::list_by_predicate_and_object(&mut conn, ROADMAP_PROPOSAL_PREDICATE, hub.id).await?;

        let mut proposals = Vec::with_capacity(proposal_edges.len());
        for edge in proposal_edges {
            let Some(node) = nodes::get_by_id(&mut conn, edge.subject).await? else { continue };
            let corroboration = self.get_edge_corroboration(edge.id).await?;
            proposals.push(RoadmapProposal { node, corroboration });
        }

        proposals.sort_by(|a, b| {
            b.corroboration
                .active_assertions
                .cmp(&a.corroboration.active_assertions)
                .then(
                    b.corroboration
                        .source_independence
                        .partial_cmp(&a.corroboration.source_independence)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
                .then(
                    b.corroboration
                        .evidence_strength
                        .partial_cmp(&a.corroboration.evidence_strength)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
        });

        Ok(proposals)
    }
}

/// Only used by tests below to resolve the hub id after asserting against
/// it -- not part of the public read path, which never needs a bare
/// `NodeId` for the hub.
#[cfg(test)]
async fn hub_node_id(conn: &mut sqlx::SqliteConnection) -> Option<oag_core::NodeId> {
    nodes::get_by_canonical_identifier(conn, ROADMAP_PROPOSAL_HUB).await.ok()?.map(|n| n.id)
}

#[cfg(test)]
mod tests {
    use oag_core::ActorType;
    use oag_crypto::PeerIdentity;
    use oag_storage::pool::open_pool;

    use crate::assert::{AssertInput, EvidenceInput};
    use crate::service::AuthContext;

    use super::*;

    fn temp_db_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oag-graph-roadmap-test-{name}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("oag.sqlite")
    }

    async fn service_with_actor(name: &str, actor_name: &str) -> (GraphService, AuthContext) {
        let pool = open_pool(&temp_db_path(name)).await.unwrap();
        let identity = PeerIdentity::generate();
        let service = GraphService::new(pool, identity);
        let actor_id =
            service.declare_actor(ActorType::Agent, Some(actor_name.into()), None, None).await.unwrap();
        (service, AuthContext { actor_id, permissions: vec![oag_core::Permission::GraphAssert] })
    }

    async fn propose(service: &GraphService, auth: &AuthContext, subject: &str, evidence_uri: &str) {
        service
            .assert(
                auth,
                AssertInput {
                    subject: subject.into(),
                    subject_type: None,
                    predicate: "instance_of".into(),
                    object: ROADMAP_PROPOSAL_HUB.into(),
                    object_type: None,
                    evidence: vec![EvidenceInput {
                        evidence_type: Some("documentation".into()),
                        uri: Some(evidence_uri.into()),
                        ..Default::default()
                    }],
                    actor_confidence: None,
                    observed_at: None,
                    extraction_method: None,
                },
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn no_proposals_ever_asserted_returns_an_empty_list_not_an_error() {
        let (service, _auth) = service_with_actor("roadmap-empty", "nobody").await;
        let proposals = service.list_roadmap_proposals().await.unwrap();
        assert!(proposals.is_empty());
    }

    #[tokio::test]
    async fn proposals_from_different_actors_are_both_listed_and_ranked() {
        let (service, auth_a) = service_with_actor("roadmap-rank", "actor-a").await;
        let actor_b_id = service.declare_actor(ActorType::Agent, Some("actor-b".into()), None, None).await.unwrap();
        let auth_b = AuthContext { actor_id: actor_b_id, permissions: vec![oag_core::Permission::GraphAssert] };

        // proposal-a: only one actor, one source group.
        propose(&service, &auth_a, "concept:proposal-a", "https://example.com/a-rationale").await;

        // proposal-b: two independent actors from two independent domains --
        // should out-rank proposal-a on source_independence.
        propose(&service, &auth_a, "concept:proposal-b", "https://example.com/b-rationale").await;
        propose(&service, &auth_b, "concept:proposal-b", "https://other-domain.example/b-rationale-2").await;

        let proposals = service.list_roadmap_proposals().await.unwrap();
        assert_eq!(proposals.len(), 2);
        assert_eq!(proposals[0].node.canonical_identifier, "concept:proposal-b");
        assert_eq!(proposals[0].corroboration.active_assertions, 2);
        assert_eq!(proposals[0].corroboration.distinct_actors, 2);
        assert_eq!(proposals[1].node.canonical_identifier, "concept:proposal-a");
        assert_eq!(proposals[1].corroboration.active_assertions, 1);
    }

    #[tokio::test]
    async fn hub_node_exists_after_first_proposal_with_no_special_bootstrap() {
        let (service, auth) = service_with_actor("roadmap-hub-bootstrap", "actor").await;
        let mut conn = service.pool().acquire().await.unwrap();
        assert!(hub_node_id(&mut conn).await.is_none(), "hub must not exist before any proposal");
        drop(conn);

        propose(&service, &auth, "concept:first-ever-proposal", "https://example.com/rationale").await;

        let mut conn = service.pool().acquire().await.unwrap();
        assert!(hub_node_id(&mut conn).await.is_some(), "hub must exist after the first proposal, via the ordinary resolve_or_create_node path");
    }
}
