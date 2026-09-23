use std::collections::HashMap;

use oag_core::{Assertion, Edge, EdgeId, Node, NodeId};
use oag_storage::repo::{assertions as assertions_repo, edges, nodes};

use crate::error::GraphError;
use crate::service::GraphService;

#[derive(Debug, Default, serde::Serialize)]
pub struct Subgraph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub assertions: Vec<Assertion>,
}

impl GraphService {
    /// A compact semantic neighborhood around `root` (spec section 35) —
    /// breadth-first out to `depth` hops, capped at `max_nodes`.
    pub async fn get_subgraph(
        &self,
        root: NodeId,
        depth: u32,
        max_nodes: usize,
    ) -> Result<Subgraph, GraphError> {
        let mut conn = self
            .pool()
            .acquire()
            .await
            .map_err(oag_storage::StorageError::from)?;

        let root_node = nodes::get_by_id(&mut conn, root)
            .await?
            .ok_or_else(|| GraphError::NotFound(format!("node {root}")))?;

        let mut visited_nodes: HashMap<NodeId, Node> = HashMap::new();
        let mut visited_edges: HashMap<EdgeId, Edge> = HashMap::new();
        visited_nodes.insert(root, root_node);
        let mut frontier = vec![root];

        for _ in 0..depth {
            if visited_nodes.len() >= max_nodes {
                break;
            }
            let mut next_frontier = Vec::new();
            for node_id in frontier {
                let touching = edges::list_touching(&mut conn, node_id).await?;
                for edge in touching {
                    if visited_nodes.len() >= max_nodes {
                        break;
                    }
                    visited_edges.insert(edge.id, edge.clone());
                    for candidate in [edge.subject, edge.object] {
                        if visited_nodes.contains_key(&candidate) {
                            continue;
                        }
                        if let Some(node) = nodes::get_by_id(&mut conn, candidate).await? {
                            visited_nodes.insert(candidate, node);
                            next_frontier.push(candidate);
                        }
                    }
                }
            }
            frontier = next_frontier;
        }

        let mut assertions = Vec::new();
        for edge_id in visited_edges.keys() {
            assertions.extend(assertions_repo::list_by_edge(&mut conn, *edge_id).await?);
        }

        Ok(Subgraph {
            nodes: visited_nodes.into_values().collect(),
            edges: visited_edges.into_values().collect(),
            assertions,
        })
    }
}
