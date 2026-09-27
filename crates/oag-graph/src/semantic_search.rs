//! Spec section 64's semantic search: embeddings stored locally, ranked by
//! brute-force cosine similarity (see `oag_embeddings::similarity` — no
//! mandatory external vector database, no ANN index in v1). Recomputation is
//! a batch operation over the current node set, same operator-triggered
//! shape as `authority::recompute_authority` and `oag rebuild` (section 83),
//! rather than something maintained incrementally on every write.
//!
//! `EmbeddingProvider` is taken as a parameter on each call rather than
//! stored on `GraphService` itself, since it's cheap to construct from
//! config and this avoids threading a provider through every one of
//! `GraphService::new`'s many existing call sites for a feature that's off
//! by default (spec section 64: "OAG itself must work with embeddings
//! completely disabled").

use oag_core::Node;
use oag_embeddings::EmbeddingProvider;
use oag_storage::repo::{node_embeddings as node_embeddings_repo, nodes as nodes_repo};

use crate::error::GraphError;
use crate::service::GraphService;

#[derive(Debug, Clone, serde::Serialize)]
pub struct EmbeddingSummary {
    pub nodes_embedded: usize,
    pub nodes_skipped: usize,
}

/// `name` + `description` + `canonical_identifier`, whichever are present,
/// joined with spaces. `canonical_identifier` is a required field on every
/// node, so this is never empty in practice -- the empty check is defensive
/// rather than load-bearing.
fn embeddable_text(node: &Node) -> String {
    [Some(node.canonical_identifier.as_str()), node.name.as_deref(), node.description.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ")
}

impl GraphService {
    /// Recompute every node's embedding from scratch against `provider`.
    /// Wipes and regenerates the whole `node_embeddings` table (same
    /// rebuildable-projection shape as `recompute_authority`) so a provider
    /// or model change doesn't leave stale vectors from a different vector
    /// space mixed in with fresh ones.
    pub async fn recompute_embeddings(
        &self,
        provider: &dyn EmbeddingProvider,
    ) -> Result<EmbeddingSummary, GraphError> {
        let mut conn = self.pool().acquire().await.map_err(oag_storage::StorageError::from)?;

        let all_nodes = nodes_repo::list_all(&mut conn).await?;
        node_embeddings_repo::clear_all(&mut conn).await?;

        let now = self.now();
        let mut nodes_embedded = 0;
        let mut nodes_skipped = 0;
        for node in &all_nodes {
            let text = embeddable_text(node);
            if text.trim().is_empty() {
                nodes_skipped += 1;
                continue;
            }
            let embedding = provider.embed(&text).await?;
            node_embeddings_repo::upsert(
                &mut conn,
                node.id,
                provider.provider_name(),
                provider.model_name(),
                &embedding,
                now,
            )
            .await?;
            nodes_embedded += 1;
        }

        Ok(EmbeddingSummary { nodes_embedded, nodes_skipped })
    }

    /// Embed `query` against `provider` and rank stored node embeddings by
    /// cosine similarity, highest first. If `provider` is
    /// `oag_embeddings::DisabledProvider` (the default), this returns
    /// `GraphError::Embedding(EmbeddingError::Disabled)` rather than a panic
    /// or a silently empty result -- callers get a clear typed signal that
    /// semantic search isn't configured.
    pub async fn semantic_search(
        &self,
        provider: &dyn EmbeddingProvider,
        query: &str,
        limit: i64,
    ) -> Result<Vec<(Node, f32)>, GraphError> {
        let query_embedding = provider.embed(query).await?;

        let mut conn = self.pool().acquire().await.map_err(oag_storage::StorageError::from)?;
        let candidates = node_embeddings_repo::list_all(&mut conn).await?;
        let ranked = oag_embeddings::rank(&query_embedding, &candidates, limit.max(0) as usize);

        let mut results = Vec::with_capacity(ranked.len());
        for (node_id, score) in ranked {
            if let Some(node) = nodes_repo::get_by_id(&mut conn, node_id).await? {
                results.push((node, score));
            }
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(canonical_identifier: &str, name: Option<&str>, description: Option<&str>) -> Node {
        Node {
            id: oag_core::NodeId::derive(canonical_identifier.as_bytes()),
            node_type: oag_core::NodeType::new("test"),
            canonical_identifier: canonical_identifier.to_string(),
            canonical_uri: None,
            name: name.map(str::to_string),
            description: description.map(str::to_string),
            metadata: serde_json::json!({}),
            created_at: 0,
        }
    }

    #[test]
    fn embeddable_text_joins_present_fields() {
        let n = node("url:https://example.com", Some("Example"), Some("An example site"));
        assert_eq!(embeddable_text(&n), "url:https://example.com Example An example site");
    }

    #[test]
    fn embeddable_text_falls_back_to_canonical_identifier_alone() {
        let n = node("url:https://example.com", None, None);
        assert_eq!(embeddable_text(&n), "url:https://example.com");
    }
}
