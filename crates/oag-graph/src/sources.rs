use oag_core::{Evidence, NodeId};
use oag_storage::repo::{assertions as assertions_repo, edges};

use crate::error::GraphError;
use crate::service::GraphService;

impl GraphService {
    /// All evidence backing any assertion whose edge touches `node` (spec's
    /// `graph_find_sources` MCP tool / explainability requirement, section
    /// 81) — "why is this relationship here" starts from a node's edges.
    pub async fn find_sources(&self, node: NodeId) -> Result<Vec<Evidence>, GraphError> {
        let mut conn = self
            .pool()
            .acquire()
            .await
            .map_err(oag_storage::StorageError::from)?;

        let touching = edges::list_touching(&mut conn, node).await?;
        let mut evidence = Vec::new();
        for edge in touching {
            let assertions = assertions_repo::list_by_edge(&mut conn, edge.id).await?;
            for assertion in assertions {
                evidence.extend(assertions_repo::list_evidence(&mut conn, assertion.id).await?);
            }
        }
        Ok(evidence)
    }
}
