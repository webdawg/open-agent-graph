use std::collections::HashMap;

use oag_core::{Assertion, Edge, EdgeId, Node, NodeId};
use oag_storage::repo::{assertions as assertions_repo, edges, nodes};

use crate::error::GraphError;
use crate::service::GraphService;

#[derive(Debug, Clone, serde::Serialize)]
pub struct SubgraphNode {
    pub node: Node,
    /// Hops from the traversal's root (0 for the root itself). Spec section
    /// 65's `graph_distance` signal — meaningful only relative to a
    /// starting point, which a subgraph traversal has and an isolated edge
    /// lookup (`EdgeCorroboration`) does not.
    pub distance: u32,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct Subgraph {
    pub nodes: Vec<SubgraphNode>,
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

        let mut visited_nodes: HashMap<NodeId, (Node, u32)> = HashMap::new();
        let mut visited_edges: HashMap<EdgeId, Edge> = HashMap::new();
        visited_nodes.insert(root, (root_node, 0));
        let mut frontier = vec![root];

        for hop in 1..=depth {
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
                            // First time a BFS round reaches `candidate` is
                            // necessarily via a shortest path — later rounds
                            // never revisit it (the `contains_key` check
                            // above), so `hop` here is the true distance.
                            visited_nodes.insert(candidate, (node, hop));
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
            nodes: visited_nodes.into_values().map(|(node, distance)| SubgraphNode { node, distance }).collect(),
            edges: visited_edges.into_values().collect(),
            assertions,
        })
    }
}
