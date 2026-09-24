use oag_core::Node;
use oag_storage::repo::nodes;

use crate::error::GraphError;
use crate::identifier::canonicalize_value;
use crate::service::GraphService;

#[derive(Debug)]
pub enum ResolveOutcome {
    /// Exact match on canonical identifier (spec section 34 — confidence
    /// 1.0 since it's a direct lookup, not a search heuristic).
    Found { node: Node, confidence: f32 },
    /// No exact identifier match; these are full-text search candidates.
    Candidates(Vec<Node>),
    NotFound,
}

impl GraphService {
    pub async fn resolve(&self, value: &str) -> Result<ResolveOutcome, GraphError> {
        let mut conn = self
            .pool()
            .acquire()
            .await
            .map_err(oag_storage::StorageError::from)?;

        let (identifier, _kind) = canonicalize_value(value);
        if let Some(node) = nodes::get_by_canonical_identifier(&mut conn, &identifier).await? {
            return Ok(ResolveOutcome::Found { node, confidence: 1.0 });
        }

        let sanitized = crate::search::sanitize_fts_query(value);
        if sanitized.is_empty() {
            return Ok(ResolveOutcome::NotFound);
        }
        let candidates = nodes::search(&mut conn, &sanitized, 10).await?;
        if candidates.is_empty() {
            Ok(ResolveOutcome::NotFound)
        } else {
            Ok(ResolveOutcome::Candidates(candidates))
        }
    }

    pub async fn get_node(&self, id: oag_core::NodeId) -> Result<Option<Node>, GraphError> {
        let mut conn = self
            .pool()
            .acquire()
            .await
            .map_err(oag_storage::StorageError::from)?;
        Ok(nodes::get_by_id(&mut conn, id).await?)
    }

    pub async fn get_edges(&self, node: oag_core::NodeId) -> Result<Vec<oag_core::Edge>, GraphError> {
        let mut conn = self
            .pool()
            .acquire()
            .await
            .map_err(oag_storage::StorageError::from)?;
        Ok(oag_storage::repo::edges::list_touching(&mut conn, node).await?)
    }

    pub async fn get_edge(&self, id: oag_core::EdgeId) -> Result<Option<oag_core::Edge>, GraphError> {
        let mut conn = self
            .pool()
            .acquire()
            .await
            .map_err(oag_storage::StorageError::from)?;
        Ok(oag_storage::repo::edges::get_by_id(&mut conn, id).await?)
    }

    pub async fn list_aliases(
        &self,
        node_id: oag_core::NodeId,
    ) -> Result<Vec<oag_core::NodeAlias>, GraphError> {
        let mut conn = self
            .pool()
            .acquire()
            .await
            .map_err(oag_storage::StorageError::from)?;
        Ok(nodes::list_aliases(&mut conn, node_id).await?)
    }

    /// All assertions made on any edge touching `node` (backs
    /// `GET /nodes/{id}/assertions`).
    pub async fn list_assertions_for_node(
        &self,
        node: oag_core::NodeId,
    ) -> Result<Vec<oag_core::Assertion>, GraphError> {
        let mut conn = self
            .pool()
            .acquire()
            .await
            .map_err(oag_storage::StorageError::from)?;
        let touching = oag_storage::repo::edges::list_touching(&mut conn, node).await?;
        let mut assertions = Vec::new();
        for edge in touching {
            assertions.extend(oag_storage::repo::assertions::list_by_edge(&mut conn, edge.id).await?);
        }
        Ok(assertions)
    }
}
