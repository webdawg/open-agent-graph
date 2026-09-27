//! Spec section 65's `authority` ranking signal: a node-level, query-
//! independent PageRank-style score over the local edge graph — "how many
//! things point to me, weighted by how important those things are."
//! Deliberately batch/on-demand rather than continuously maintained (see
//! `recompute_authority`'s doc comment), same shape as the spec's own
//! operator-triggered `oag rebuild` (section 83).
use std::collections::{BTreeSet, HashMap};

use oag_core::NodeId;
use oag_storage::repo::{edges as edges_repo, node_authority as node_authority_repo};

use crate::error::GraphError;
use crate::service::GraphService;

/// The standard PageRank default: the probability a "random surfer" follows
/// an outgoing edge rather than jumping to a uniformly random node.
const DAMPING: f32 = 0.85;
/// Power iteration converges quickly for graphs this project's scale is
/// meant for; this is a safety cap, not the expected iteration count.
const MAX_ITERATIONS: usize = 50;
/// Stop early once the total per-iteration score movement drops below this.
const CONVERGENCE_EPSILON: f32 = 1e-6;

#[derive(Debug, Clone, serde::Serialize)]
pub struct AuthoritySummary {
    pub nodes_scored: usize,
    pub edges_considered: usize,
    pub iterations: usize,
}

/// Pure power-iteration PageRank over a plain edge list — no I/O, so it's
/// directly unit-testable. Only nodes that appear in at least one edge get
/// a score; a node with zero edges has no structural position in the graph
/// to have authority over (consistent with real PageRank, where an isolated
/// node's rank is a fixed, uninformative constant anyway).
///
/// Dangling nodes (out-degree zero) redistribute their score uniformly
/// across every node each iteration — the standard PageRank fix for rank
/// otherwise leaking out of the system with nowhere to go.
fn pagerank(edges: &[(NodeId, NodeId)]) -> (HashMap<NodeId, f32>, usize) {
    let mut nodes: BTreeSet<NodeId> = BTreeSet::new();
    for &(s, o) in edges {
        nodes.insert(s);
        nodes.insert(o);
    }
    let n = nodes.len();
    if n == 0 {
        return (HashMap::new(), 0);
    }

    let mut out_edges: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    for &(s, o) in edges {
        out_edges.entry(s).or_default().push(o);
    }

    let mut scores: HashMap<NodeId, f32> = nodes.iter().map(|&id| (id, 1.0 / n as f32)).collect();
    let mut iterations_run = 0;

    for iteration in 0..MAX_ITERATIONS {
        iterations_run = iteration + 1;

        let dangling_mass: f32 = nodes
            .iter()
            .filter(|id| out_edges.get(id).map(Vec::is_empty).unwrap_or(true))
            .map(|id| scores[id])
            .sum();

        let base = (1.0 - DAMPING) / n as f32 + DAMPING * dangling_mass / n as f32;
        let mut new_scores: HashMap<NodeId, f32> = nodes.iter().map(|&id| (id, base)).collect();

        for (source, targets) in &out_edges {
            if targets.is_empty() {
                continue;
            }
            let share = DAMPING * scores[source] / targets.len() as f32;
            for target in targets {
                *new_scores.get_mut(target).unwrap() += share;
            }
        }

        let diff: f32 = nodes.iter().map(|id| (new_scores[id] - scores[id]).abs()).sum();
        scores = new_scores;
        if diff < CONVERGENCE_EPSILON {
            break;
        }
    }

    (scores, iterations_run)
}

impl GraphService {
    /// Recompute every known node's authority score from scratch. Not
    /// triggered automatically on writes or at `oag serve` startup — spec
    /// section 65 doesn't require real-time freshness for this signal, and
    /// recomputing after every single assertion would be wasteful for a
    /// score that only meaningfully shifts as the graph's overall shape
    /// changes. An operator re-runs this periodically (or after a large
    /// import), same as `oag rebuild`.
    pub async fn recompute_authority(&self) -> Result<AuthoritySummary, GraphError> {
        let mut conn = self.pool().acquire().await.map_err(oag_storage::StorageError::from)?;

        let all_edges = edges_repo::list_all(&mut conn).await?;
        let edge_pairs: Vec<(NodeId, NodeId)> = all_edges.iter().map(|e| (e.subject, e.object)).collect();
        let (scores, iterations) = pagerank(&edge_pairs);

        node_authority_repo::clear_all(&mut conn).await?;
        let now = self.now();
        for (&node_id, &score) in &scores {
            node_authority_repo::upsert(&mut conn, node_id, score, now).await?;
        }

        Ok(AuthoritySummary {
            nodes_scored: scores.len(),
            edges_considered: all_edges.len(),
            iterations,
        })
    }

    /// `None` if authority has never been computed at all yet, or this node
    /// has never appeared in any edge.
    pub async fn get_node_authority(&self, node_id: NodeId) -> Result<Option<f32>, GraphError> {
        let mut conn = self.pool().acquire().await.map_err(oag_storage::StorageError::from)?;
        Ok(node_authority_repo::get(&mut conn, node_id).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(tag: u8) -> NodeId {
        NodeId::derive(&[tag])
    }

    #[test]
    fn empty_graph_yields_no_scores() {
        let (scores, iterations) = pagerank(&[]);
        assert!(scores.is_empty());
        assert_eq!(iterations, 0);
    }

    #[test]
    fn hub_outranks_a_leaf() {
        // a, b, c, d all point to "hub" -- hub should end up with by far the
        // highest score, and none of the leaves' scores blow up or go
        // negative.
        let hub = node(0);
        let edges = [(node(1), hub), (node(2), hub), (node(3), hub), (node(4), hub)];
        let (scores, _) = pagerank(&edges);

        let hub_score = scores[&hub];
        for tag in 1..=4u8 {
            let leaf_score = scores[&node(tag)];
            assert!(hub_score > leaf_score, "hub ({hub_score}) should outrank leaf {tag} ({leaf_score})");
        }
    }

    #[test]
    fn dangling_node_does_not_leak_rank_out_of_the_system() {
        // b has no outgoing edges at all (dangling). Total score mass
        // across all participating nodes should still sum to ~1.0 (modulo
        // floating point) rather than draining away each iteration.
        let a = node(1);
        let b = node(2);
        let edges = [(a, b)];
        let (scores, _) = pagerank(&edges);
        let total: f32 = scores.values().sum();
        assert!((total - 1.0).abs() < 1e-3, "total score mass drifted to {total}");
    }

    #[test]
    fn cyclic_graph_converges_within_the_iteration_cap() {
        let a = node(1);
        let b = node(2);
        let c = node(3);
        let edges = [(a, b), (b, c), (c, a)];
        let (scores, iterations) = pagerank(&edges);
        assert!(iterations < MAX_ITERATIONS, "expected early convergence, ran all {iterations} iterations");
        // A symmetric 3-cycle should score all three nodes equally.
        let a_score = scores[&a];
        for &n in &[b, c] {
            assert!((scores[&n] - a_score).abs() < 1e-4, "expected a symmetric cycle to score evenly");
        }
    }
}
